// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use hbb_common::config::Config;
use serde_derive::Deserialize;
use sha2::{Digest, Sha256};
#[cfg(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "android",
    target_os = "ios"
))]
use std::process::{Command, Stdio};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

const CACHE_TTL: Duration = Duration::from_secs(600);
/// Oltre `CACHE_TTL` la postura si usa ancora, rinnovandola in background, fino a
/// questa età: la connessione non aspetta la raccolta (fino a 3-4 secondi).
const STALE_TTL: Duration = Duration::from_secs(24 * 3600);
const COLLECT_TIMEOUT: Duration = Duration::from_secs(3);
const DISK_CACHE: &str = "posture.cache";

static CACHE: Mutex<Option<(Instant, HashMap<String, String>)>> = Mutex::new(None);
static COLLECTING: AtomicBool = AtomicBool::new(false);

struct CollectingGuard;

impl Drop for CollectingGuard {
    fn drop(&mut self) {
        COLLECTING.store(false, Ordering::Release);
    }
}

pub const OPT_REQUIRE_PROTECTED_DEVICE: &str = "clanto-require-protected-device";
pub const OPT_POSTURE_BYPASS: &str = "clanto-posture-bypass";
pub const POSTURE_API_URL: &str = "https://clanto.it/api/clantodesk-posture.php";
pub const MIN_POSTURE_VERSION: &str = "1.5.0";
pub const OUTDATED_LOGIN_ERROR_MARKER: &str = "\u{2063}\u{2064}\u{2063}";
pub const LOGIN_ERROR_OUTDATED: &str = "clanto_posture_outdated_error";
pub const LOGIN_ERROR_UNOFFICIAL: &str = "clanto_posture_unofficial_error";
pub const LOGIN_ERROR_UNREACHABLE: &str = "clanto_posture_unreachable_error";
pub const LOGIN_ERROR_POLICY: &str = "clanto_posture_policy_error";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationMode {
    None,
    Signature,
    Full,
}

pub fn verification_mode(
    require_protected_device: bool,
    require_controller_watermark: bool,
) -> VerificationMode {
    if require_protected_device {
        VerificationMode::Full
    } else if require_controller_watermark {
        VerificationMode::Signature
    } else {
        VerificationMode::None
    }
}

pub fn configured_verification_mode() -> VerificationMode {
    verification_mode(
        Config::get_option(OPT_REQUIRE_PROTECTED_DEVICE) == "Y",
        Config::get_option(crate::clanto::watermark::OPT_REQUIRE_CONTROLLER_WATERMARK) == "Y",
    )
}

const UNKNOWN: &str = "unknown";
pub const POSTURE_KEYS: &[&str] = &[
    "platform",
    "os_version",
    "os_build",
    "client_version",
    "av_enabled",
    "av_uptodate",
    "firewall_enabled",
    "disk_encrypted",
    "secure_boot",
    "screen_lock",
    "firmware_type",
    "tpm_version",
    "vbs_enabled",
    "last_update_days",
    "os_ubr",
    "os_patch_days",
    "auto_update_enabled",
    "pending_reboot",
    "arch",
    "os_edition",
    "av_products",
    "av_signature_days",
    "firewall_public",
    "uac_enabled",
    "sip_enabled",
    "gatekeeper_enabled",
    "user_is_admin",
    "rooted",
    "dev_options",
    "unknown_sources",
    "is_vm",
    "net_type",
    "wifi_security",
    "vpn_active",
    "proxy_set",
];

#[derive(Clone, Debug, Deserialize)]
pub struct FailedRule {
    pub id: String,
    pub message: String,
    pub bypassable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectionKind {
    Outdated,
    Unofficial,
    Unreachable,
    Policy,
}

impl RejectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Outdated => "outdated",
            Self::Unofficial => "unofficial",
            Self::Unreachable => "unreachable",
            Self::Policy => "policy",
        }
    }
}

pub enum VerificationFailure<'a> {
    MissingCredentials,
    Unreachable,
    Rules(&'a [FailedRule]),
}

pub struct VerificationRejection {
    pub kind: RejectionKind,
    pub technical_detail: String,
    pub policy_message: Option<String>,
    pub failed_rules: Vec<String>,
}

pub fn classify_rejection(version: &str, failure: VerificationFailure<'_>) -> RejectionKind {
    if hbb_common::get_version_number(version) < hbb_common::get_version_number(MIN_POSTURE_VERSION)
    {
        return RejectionKind::Outdated;
    }
    match failure {
        VerificationFailure::MissingCredentials => RejectionKind::Unofficial,
        VerificationFailure::Unreachable => RejectionKind::Unreachable,
        VerificationFailure::Rules(failed) => {
            if failed.iter().any(|rule| rule.id == "core:revoked") {
                RejectionKind::Outdated
            } else if failed.iter().any(|rule| rule.id == "core:signature") {
                RejectionKind::Unofficial
            } else {
                RejectionKind::Policy
            }
        }
    }
}

impl VerificationRejection {
    pub fn missing_credentials(version: &str) -> Self {
        Self {
            kind: classify_rejection(version, VerificationFailure::MissingCredentials),
            technical_detail: "posture or signature missing".to_owned(),
            policy_message: None,
            failed_rules: Vec::new(),
        }
    }

    pub fn unreachable(version: &str, technical_detail: String) -> Self {
        Self {
            kind: classify_rejection(version, VerificationFailure::Unreachable),
            technical_detail,
            policy_message: None,
            failed_rules: Vec::new(),
        }
    }

    pub fn rules(version: &str, failed: &[FailedRule], technical_reason: String) -> Self {
        let failed_rules = failed
            .iter()
            .map(|rule| rule.id.clone())
            .collect::<Vec<_>>();
        let policy_message = failed
            .iter()
            .map(|rule| rule.message.trim())
            .filter(|message| !message.is_empty() && !message.contains("core:"))
            .collect::<Vec<_>>()
            .join("; ");
        let technical_detail = if failed_rules.is_empty() {
            technical_reason
        } else {
            format!("{} [{}]", technical_reason, failed_rules.join(", "))
        };
        Self {
            kind: classify_rejection(version, VerificationFailure::Rules(failed)),
            technical_detail,
            policy_message: (!policy_message.is_empty()).then_some(policy_message),
            failed_rules,
        }
    }
}

fn cached_within(ttl: Duration) -> Option<HashMap<String, String>> {
    let mut cache = CACHE.lock().unwrap_or_else(|err| err.into_inner());
    if cache.is_none() {
        *cache = load_disk_cache();
    }
    cache
        .as_ref()
        .filter(|(at, _)| at.elapsed() < ttl)
        .map(|(_, posture)| posture.clone())
}

fn cached() -> Option<HashMap<String, String>> {
    cached_within(CACHE_TTL)
}

fn collect_and_store() -> HashMap<String, String> {
    let posture = gather();
    *CACHE.lock().unwrap_or_else(|err| err.into_inner()) = Some((Instant::now(), posture.clone()));
    store_disk_cache(&posture);
    posture
}

/// Messaggio autenticato della cache su disco: stessa chiave della firma di
/// login, così modificare il file non vale più che falsificare la postura.
fn disk_cache_mac(at: u64, posture: &HashMap<String, String>) -> Option<String> {
    let key = release_key()?;
    let message = format!(
        "clanto-posture-cache-v2
{}",
        canonical_message(posture, "", &at.to_string(), crate::VERSION)
    );
    Some(hex::encode(hmac_sha256(&key, message.as_bytes())))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Postura salvata da un'esecuzione precedente: valida se firmata, della stessa
/// versione del client e più recente di `STALE_TTL`.
fn load_disk_cache() -> Option<(Instant, HashMap<String, String>)> {
    let text = std::fs::read_to_string(Config::path(DISK_CACHE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let at = value.get("at")?.as_u64()?;
    let posture: HashMap<String, String> =
        serde_json::from_value(value.get("posture")?.clone()).ok()?;
    let mac = value.get("mac")?.as_str()?;
    if Some(mac) != disk_cache_mac(at, &posture).as_deref()
        || posture.get("client_version").map(String::as_str) != Some(crate::VERSION)
        || POSTURE_KEYS.iter().any(|key| !posture.contains_key(*key))
    {
        return None;
    }
    let age = Duration::from_secs(unix_now().checked_sub(at)?);
    if age >= STALE_TTL {
        return None;
    }
    Some((Instant::now().checked_sub(age)?, posture))
}

fn store_disk_cache(posture: &HashMap<String, String>) {
    let at = unix_now();
    let Some(mac) = disk_cache_mac(at, posture) else {
        return;
    };
    let value = serde_json::json!({ "at": at, "posture": posture, "mac": mac });
    if let Err(err) = std::fs::write(Config::path(DISK_CACHE), value.to_string()) {
        hbb_common::log::debug!("posture cache not saved: {err}");
    }
}

/// Avvia la raccolta in un thread dedicato se la cache manca o è scaduta.
pub fn prewarm() {
    if cached().is_some()
        || COLLECTING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }
    std::thread::spawn(|| {
        let _guard = CollectingGuard;
        collect_and_store();
    });
}

/// Riempie la cache fuori dall'executor; `collect()` poi non blocca.
pub async fn warm() {
    if let Err(err) = hbb_common::tokio::task::spawn_blocking(collect).await {
        hbb_common::log::warn!("posture collection failed: {err}");
    }
}

/// Bloccante: da chiamare fuori dall'executor (vedi `warm`).
/// Con una postura scaduta ma più recente di `STALE_TTL` non aspetta: la
/// restituisce e la rinnova in background.
pub fn collect() -> HashMap<String, String> {
    if let Some(posture) = cached() {
        return posture;
    }
    if let Some(posture) = cached_within(STALE_TTL) {
        prewarm();
        return posture;
    }
    // Se un prewarm è in corso lo si attende, per non lanciare due raccolte.
    let wait_until = Instant::now() + COLLECT_TIMEOUT + Duration::from_secs(1);
    while COLLECTING.load(Ordering::Acquire) && Instant::now() < wait_until {
        std::thread::sleep(Duration::from_millis(50));
        if let Some(posture) = cached() {
            return posture;
        }
    }
    collect_and_store()
}

fn gather() -> HashMap<String, String> {
    let deadline = Instant::now() + COLLECT_TIMEOUT;
    let mut posture = POSTURE_KEYS
        .iter()
        .map(|key| ((*key).to_owned(), UNKNOWN.to_owned()))
        .collect::<HashMap<_, _>>();
    posture.insert("platform".to_owned(), platform_name());
    posture.insert("client_version".to_owned(), crate::VERSION.to_owned());
    posture.insert("arch".to_owned(), std::env::consts::ARCH.to_owned());
    collect_platform(&mut posture, deadline);
    posture
}

pub fn decide(ok: bool, failed: &[FailedRule], bypass: &[String]) -> Result<Vec<String>, String> {
    if ok && failed.is_empty() {
        return Ok(Vec::new());
    }
    if !ok && !failed.is_empty() {
        let bypassed = failed.iter().all(|rule| {
            rule.bypassable
                && !rule.id.starts_with("core:")
                && bypass.iter().any(|id| id == &rule.id)
        });
        if bypassed {
            return Ok(failed.iter().map(|rule| rule.id.clone()).collect());
        }
    }
    let messages = failed
        .iter()
        .filter(|rule| {
            rule.id.starts_with("core:")
                || !rule.bypassable
                || !bypass.iter().any(|id| id == &rule.id)
        })
        .map(|rule| rule.message.as_str())
        .filter(|message| !message.is_empty())
        .collect::<Vec<_>>();
    Err(if messages.is_empty() {
        "requisiti di sicurezza non soddisfatti".to_owned()
    } else {
        messages.join("; ")
    })
}

pub fn sign(
    posture: &HashMap<String, String>,
    host_id: &str,
    challenge: &str,
    version: &str,
) -> Vec<u8> {
    let Some(key) = release_key() else {
        return Vec::new();
    };
    hmac_sha256(
        &key,
        canonical_message(posture, host_id, challenge, version).as_bytes(),
    )
    .to_vec()
}

fn canonical_message(
    posture: &HashMap<String, String>,
    host_id: &str,
    challenge: &str,
    version: &str,
) -> String {
    let mut entries = posture.iter().collect::<Vec<_>>();
    entries.sort_unstable_by(|(left, _), (right, _)| left.as_bytes().cmp(right.as_bytes()));
    let mut message = format!("clanto-posture-v1\n{host_id}\n{challenge}\n{version}\n");
    for (key, value) in entries {
        message.push_str(key);
        message.push('=');
        message.push_str(value);
        message.push('\n');
    }
    message
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0_u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        key_block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(key_block.map(|byte| byte ^ 0x36));
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(key_block.map(|byte| byte ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

#[cfg(not(test))]
fn release_key() -> Option<Vec<u8>> {
    let masked = hex::decode(option_env!("CLANTO_POSTURE_KEY_MASKED")?).ok()?;
    let mask = hex::decode(option_env!("CLANTO_POSTURE_KEY_MASK")?).ok()?;
    if masked.len() != 32 || mask.len() != 32 {
        return None;
    }
    Some(
        masked
            .into_iter()
            .zip(mask)
            .map(|(key, mask)| key ^ mask)
            .collect(),
    )
}

#[cfg(test)]
fn release_key() -> Option<Vec<u8>> {
    Some(Sha256::digest(b"clanto-posture-test").to_vec())
}

fn platform_name() -> String {
    #[cfg(target_os = "android")]
    {
        "Android".to_owned()
    }
    #[cfg(not(target_os = "android"))]
    {
        hbb_common::whoami::platform().to_string()
    }
}

/// Windows: API native (`libs/clanto-win`), niente PowerShell. In un thread a
/// parte per COM; le voci arrivano a passi, e alla scadenza si tiene quello
/// che è già arrivato.
#[cfg(target_os = "windows")]
fn collect_platform(posture: &mut HashMap<String, String>, deadline: Instant) {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("clanto-posture".to_owned())
        .spawn(move || {
            clanto_win::posture::collect_each(|pairs| {
                let _ = tx.send(pairs);
            });
        });
    if spawned.is_err() {
        return;
    }
    loop {
        let timeout = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(timeout) {
            Ok(pairs) => {
                let output = pairs
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                apply_lines(posture, &output);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                hbb_common::log::warn!("posture collection incomplete after {COLLECT_TIMEOUT:?}");
                return;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(target_os = "macos")]
fn collect_platform(posture: &mut HashMap<String, String>, deadline: Instant) {
    let script = r#"
emit() { [ -n "$2" ] && printf '%s=%s\n' "$1" "$2"; }
emit os_version "$(sw_vers -productVersion 2>/dev/null)"
emit os_build "$(sw_vers -buildVersion 2>/dev/null)"
emit os_edition "$(sw_vers -productName 2>/dev/null)"
flag() { case "$2" in *"$3"*) emit "$1" true;; *"$4"*) emit "$1" false;; esac; }
flag disk_encrypted "$(fdesetup status 2>/dev/null)" 'FileVault is On' 'FileVault is Off'
flag firewall_enabled "$(/usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate 2>/dev/null)" 'is enabled' 'is disabled'
flag sip_enabled "$(csrutil status 2>/dev/null)" 'status: enabled' 'status: disabled'
flag gatekeeper_enabled "$(spctl --status 2>/dev/null)" 'assessments enabled' 'assessments disabled'
sysctl -n kern.hv_vmm_present 2>/dev/null | grep -q '^1$' && emit is_vm true
[ "$(id -u)" = 0 ] && emit user_is_admin true || emit user_is_admin false
softwareupdate --schedule 2>/dev/null | grep -qi on && emit auto_update_enabled true
update_date="$(softwareupdate --history 2>/dev/null | awk 'NR > 2 {print $1; exit}')"
update_epoch="$(date -j -f '%Y-%m-%d' "$update_date" +%s 2>/dev/null)"
[ -n "$update_epoch" ] && emit last_update_days "$(( ($(date +%s) - update_epoch) / 86400 ))"
scutil --proxy 2>/dev/null | grep -q 'Enable : 1' && emit proxy_set true || emit proxy_set false
"#;
    if let Some(output) = command_output(deadline, "/bin/sh", &["-c", script]) {
        apply_lines(posture, &output);
    }
}

#[cfg(target_os = "linux")]
fn collect_platform(posture: &mut HashMap<String, String>, deadline: Instant) {
    if let Ok(os_release) = std::fs::read_to_string("/etc/os-release") {
        let values = os_release
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key, value.trim_matches('"')))
            .collect::<HashMap<_, _>>();
        if let (Some(id), Some(version)) = (values.get("ID"), values.get("VERSION_ID")) {
            posture.insert("os_version".to_owned(), format!("{id}{version}"));
        }
        if let Some(name) = values.get("PRETTY_NAME") {
            posture.insert("os_edition".to_owned(), (*name).to_owned());
        }
    }
    posture.insert(
        "firmware_type".to_owned(),
        if std::path::Path::new("/sys/firmware/efi").exists() {
            "uefi"
        } else {
            "bios"
        }
        .to_owned(),
    );
    if let Ok(entries) = std::fs::read_dir("/sys/class/tpm") {
        let tpm = entries.flatten().next();
        let version = tpm
            .and_then(|entry| std::fs::read_to_string(entry.path().join("device/description")).ok())
            .and_then(|description| {
                ["2.0", "1.2"]
                    .into_iter()
                    .find(|version| description.contains(version))
            })
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if std::path::Path::new("/sys/class/tpm/tpm0").exists() {
                    UNKNOWN.to_owned()
                } else {
                    "none".to_owned()
                }
            });
        posture.insert("tpm_version".to_owned(), version);
    }
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        if let Some(uid) = status.lines().find_map(|line| line.strip_prefix("Uid:\t")) {
            posture.insert(
                "user_is_admin".to_owned(),
                (uid.split_whitespace().next() == Some("0")).to_string(),
            );
        }
    }
    posture.insert(
        "proxy_set".to_owned(),
        std::env::vars()
            .any(|(key, value)| key.to_ascii_lowercase().ends_with("_proxy") && !value.is_empty())
            .to_string(),
    );
    if let Some(output) = command_output(
        deadline,
        "/bin/sh",
        &[
            "-c",
            r#"
if systemd-detect-virt -q 2>/dev/null; then echo is_vm=true; else echo is_vm=false; fi
if [ -d /sys/class/net ]; then
  for i in /sys/class/net/*; do [ "$(cat "$i/operstate" 2>/dev/null)" = up ] || continue; n=$(basename "$i"); case "$n" in wl*|wlan*) echo net_type=wifi; break;; en*|eth*) echo net_type=ethernet; break;; esac; done
fi
for i in /sys/class/net/*; do n=$(basename "$i"); case "$n" in tun*|tap*|wg*) [ "$(cat "$i/operstate" 2>/dev/null)" = up ] && echo vpn_active=true; break;; esac; done
for log in /var/log/apt/history.log /var/log/dnf.rpm.log; do if [ -f "$log" ]; then d=$(( ($(date +%s) - $(stat -c %Y "$log")) / 86400 )); [ "$d" -ge 0 ] && echo last_update_days=$d; break; fi; done
"#,
        ],
    ) {
        apply_lines(posture, &output);
    }
}

#[cfg(target_os = "android")]
fn collect_platform(posture: &mut HashMap<String, String>, deadline: Instant) {
    let script = r#"
emit() { [ -n "$2" ] && printf '%s=%s\n' "$1" "$2"; }
emit os_version "$(getprop ro.build.version.release)"
emit os_build "$(getprop ro.build.version.security_patch)"
emit os_edition "$(getprop ro.build.version.incremental)"
emit arch "$(getprop ro.product.cpu.abi)"
tags="$(getprop ro.build.tags)"; case "$tags" in *test-keys*) emit rooted true;; esac
for p in /system/xbin/su /system/bin/su /sbin/su; do [ -x "$p" ] && emit rooted true && break; done
[ "$(settings get global development_settings_enabled 2>/dev/null)" = 1 ] && emit dev_options true
[ "$(settings get secure install_non_market_apps 2>/dev/null)" = 1 ] && emit unknown_sources true
[ "$(getprop ro.kernel.qemu)" = 1 ] && emit is_vm true
"#;
    if let Some(output) = command_output(deadline, "sh", &["-c", script]) {
        apply_lines(posture, &output);
    }
}

#[cfg(target_os = "ios")]
fn collect_platform(posture: &mut HashMap<String, String>, deadline: Instant) {
    if let Some(value) = command_output(deadline, "sysctl", &["-n", "kern.osproductversion"])
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        posture.insert("os_version".to_owned(), value);
    }
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "android",
    target_os = "ios"
)))]
fn collect_platform(_posture: &mut HashMap<String, String>, _deadline: Instant) {}

#[cfg(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "android",
    target_os = "ios"
))]
fn command_output(deadline: Instant, command: &str, args: &[&str]) -> Option<String> {
    use std::{io::Read, sync::Arc};
    let mut command = Command::new(command);
    // Senza CREATE_NO_WINDOW ogni sonda apre per un istante una console PowerShell.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let reader = {
        let buffer = buffer.clone();
        std::thread::spawn(move || {
            let mut chunk = [0_u8; 4096];
            while let Ok(read) = stdout.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                buffer
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .extend_from_slice(&chunk[..read]);
            }
        })
    };
    let snapshot = || buffer.lock().unwrap_or_else(|err| err.into_inner()).clone();
    loop {
        // Il codice di uscita non conta: gli script finiscono spesso con un test
        // falso (`[ ... ] && emit ...`, es. ro.kernel.qemu su un telefono vero),
        // e scartare l'output lasciava tutta la postura a "unknown".
        if child.try_wait().ok()?.is_some() {
            let _ = reader.join();
            return String::from_utf8(snapshot()).ok();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            // Il lettore può restare bloccato se un nipote tiene la pipe: non lo si attende.
            return Some(complete_lines(&snapshot()));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Testo fino all'ultimo a capo: scarta la riga finale troncata dal timeout.
fn complete_lines(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn apply_lines(posture: &mut HashMap<String, String>, output: &str) {
    for line in output.lines() {
        if let Some((key, value)) = line.trim().split_once('=') {
            if POSTURE_KEYS.contains(&key)
                && !value.is_empty()
                && !value.contains('\n')
                && !value.contains('\r')
            {
                posture.insert(key.to_owned(), value.to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_lines, classify_rejection, collect, command_output, complete_lines, decide, release_key, sign,
        verification_mode, FailedRule, RejectionKind, VerificationFailure, VerificationMode,
        POSTURE_KEYS,
    };
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::collections::HashMap;

    /// Come lo script Android su un telefono vero: l'ultimo test e' falso e sh
    /// esce con 1, ma i valori gia' emessi devono arrivare alla postura.
    #[cfg(unix)]
    #[test]
    fn probe_output_survives_a_failing_last_command() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let output = command_output(
            deadline,
            "sh",
            &["-c", "printf 'os_build=2026-05-01\\n'; [ 0 = 1 ] && printf 'is_vm=true\\n'"],
        )
        .expect("output of a probe that exits with status 1");
        let mut posture = HashMap::from([("os_build".to_owned(), "unknown".to_owned())]);
        apply_lines(&mut posture, &output);
        assert_eq!(posture["os_build"], "2026-05-01");
    }

    #[test]
    fn sign_uses_bytewise_sorted_canonical_posture() {
        let posture = HashMap::from([
            ("zeta".to_owned(), "last".to_owned()),
            ("alpha".to_owned(), "first".to_owned()),
        ]);
        let expected_message =
            b"clanto-posture-v1\nhost-1\nchallenge-1\n1.5.0\nalpha=first\nzeta=last\n";
        let mut expected =
            Hmac::<Sha256>::new_from_slice(&release_key().unwrap_or_default()).unwrap();
        expected.update(expected_message);
        let expected = expected.finalize().into_bytes().to_vec();
        assert_eq!(sign(&posture, "host-1", "challenge-1", "1.5.0"), expected);
    }

    #[test]
    fn verification_mode_uses_full_before_signature() {
        assert_eq!(verification_mode(false, false), VerificationMode::None);
        assert_eq!(verification_mode(false, true), VerificationMode::Signature);
        assert_eq!(verification_mode(true, false), VerificationMode::Full);
        assert_eq!(verification_mode(true, true), VerificationMode::Full);
    }

    #[test]
    fn classifies_posture_rejections_by_version_and_failure() {
        let revoked = [failed("core:revoked", false)];
        let signature = [failed("core:signature", false)];
        let policy = [failed("antivirus", false)];

        assert_eq!(
            classify_rejection("1.4.9", VerificationFailure::MissingCredentials),
            RejectionKind::Outdated
        );
        assert_eq!(
            classify_rejection("1.10.0", VerificationFailure::MissingCredentials),
            RejectionKind::Unofficial
        );
        assert_eq!(
            classify_rejection("1.5.0", VerificationFailure::Rules(&revoked)),
            RejectionKind::Outdated
        );
        assert_eq!(
            classify_rejection("1.5.0", VerificationFailure::Rules(&signature)),
            RejectionKind::Unofficial
        );
        assert_eq!(
            classify_rejection("1.5.0", VerificationFailure::Unreachable),
            RejectionKind::Unreachable
        );
        assert_eq!(
            classify_rejection("1.5.0", VerificationFailure::Rules(&policy)),
            RejectionKind::Policy
        );
    }

    #[test]
    fn disk_cache_mac_depends_on_time_and_values() {
        let posture = HashMap::from([("os_version".to_owned(), "10".to_owned())]);
        let mac = super::disk_cache_mac(100, &posture).unwrap();
        assert_ne!(Some(mac.clone()), super::disk_cache_mac(101, &posture));
        let changed = HashMap::from([("os_version".to_owned(), "11".to_owned())]);
        assert_ne!(Some(mac), super::disk_cache_mac(100, &changed));
    }

    #[test]
    fn collect_includes_every_posture_key() {
        let posture = collect();
        assert!(POSTURE_KEYS.iter().all(|key| posture.contains_key(*key)));
    }

    #[test]
    fn partial_output_keeps_only_complete_lines() {
        let mut posture = HashMap::new();
        apply_lines(
            &mut posture,
            &complete_lines(b"os_version=10\r\nav_enabled=true\nav_uptodate=tr"),
        );
        assert_eq!(posture.get("os_version").map(String::as_str), Some("10"));
        assert_eq!(posture.get("av_enabled").map(String::as_str), Some("true"));
        assert!(!posture.contains_key("av_uptodate"));
    }

    fn failed(id: &str, bypassable: bool) -> FailedRule {
        FailedRule {
            id: id.to_owned(),
            message: format!("message for {id}"),
            bypassable,
        }
    }

    #[test]
    fn decide_rejects_core_rules_even_when_bypassed() {
        assert!(decide(
            false,
            &[failed("core:signature", true)],
            &["core:signature".to_owned()]
        )
        .is_err());
    }

    #[test]
    fn decide_rejects_failed_response_without_rules() {
        assert!(decide(false, &[], &[]).is_err());
    }

    #[test]
    fn decide_accepts_all_bypassable_failed_rules_in_bypass_list() {
        assert_eq!(
            decide(
                false,
                &[failed("av", true), failed("updates", true)],
                &["av".to_owned(), "updates".to_owned()]
            ),
            Ok(vec!["av".to_owned(), "updates".to_owned()])
        );
    }

    #[test]
    fn decide_rejects_missing_bypassable_rule() {
        assert!(decide(false, &[failed("av", true)], &[]).is_err());
    }
}
