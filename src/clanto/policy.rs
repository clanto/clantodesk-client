// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Criteri del tenant, firmati dall'api-server con la chiave del tenant.
//!
//! - Si accettano solo con firma valida, per questo dispositivo e con versione
//!   non inferiore a quella già applicata.
//! - Le `options` diventano impostazioni gestite (`OVERWRITE_SETTINGS`): solo
//!   chiavi che potrebbe impostare anche la GPO (`GPO_KEYS`), mai quelle di
//!   iscrizione.
//! - Sulla stessa chiave GPO e pannello si combinano: `enable-*` → vince `N`;
//!   `clanto-require-*`, filigrane e blocco impostazioni → vince `Y`; il resto
//!   → vince la GPO.
//! - Il criterio è salvato e riapplicato all'avvio, anche senza rete.
//! - La UI desktop è un altro processo: riceve i valori effettivi dal servizio
//!   via IPC (`sync_ui`).

use super::{enrollment, gpo::GPO_KEYS};
use hbb_common::{
    bail,
    config::{self, Config},
    log,
    sodiumoxide::crypto::sign,
    ResultType,
};
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

pub const IPC_EFFECTIVE: &str = "clanto-policy-effective";
const POLICY_FILE: &str = "clanto-policy.json";
const UI_SYNC_INTERVAL: Duration = Duration::from_secs(5);
const ENROLLMENT_KEYS: &[&str] = &[enrollment::OPT_ENROLL_TOKEN, enrollment::OPT_UNENROLL];
const REQUIRE_WINS: &[&str] = &[
    "clanto-host-watermark",
    super::settings_lock::OPT_LOCK_SETTINGS,
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignedPolicy {
    pub payload: String,
    pub signature: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PolicySettings {
    #[serde(default)]
    pub enable_account: bool,
    #[serde(default)]
    pub record_upload: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub tenant_id: String,
    pub device_id: String,
    pub version: u64,
    #[serde(default)]
    pub issued_at: i64,
    #[serde(default)]
    pub options: HashMap<String, String>,
    #[serde(default)]
    pub settings: PolicySettings,
}

#[derive(Default)]
struct Applied {
    /// Valori di `OVERWRITE_SETTINGS` (GPO) prima del primo criterio.
    baseline: Option<HashMap<String, String>>,
    /// Chiavi scritte dall'ultimo criterio, con il valore effettivo.
    effective: HashMap<String, String>,
    policy: Option<Policy>,
    last_ui_sync: Option<Instant>,
}

static APPLIED: OnceLock<Mutex<Applied>> = OnceLock::new();

fn applied() -> &'static Mutex<Applied> {
    APPLIED.get_or_init(Default::default)
}

pub fn policy_key_allowed(key: &str) -> bool {
    GPO_KEYS.contains(&key) && !ENROLLMENT_KEYS.contains(&key)
}

/// Valore effettivo quando GPO e criterio impostano la stessa chiave.
pub fn merge(key: &str, gpo: Option<&str>, policy: &str) -> String {
    let Some(gpo) = gpo else {
        return policy.to_owned();
    };
    if key.starts_with("enable-") {
        return if gpo == "N" || policy == "N" { "N" } else { gpo }.to_owned();
    }
    if key.starts_with("clanto-require-") || REQUIRE_WINS.contains(&key) {
        return if gpo == "Y" || policy == "Y" { "Y" } else { gpo }.to_owned();
    }
    gpo.to_owned()
}

/// Verifica firma, tenant e dispositivo.
pub fn verify(signed: &SignedPolicy, enrollment: &enrollment::Enrollment) -> ResultType<Policy> {
    let Some(key) = enrollment::decode_key(&enrollment.tenant_key) else {
        bail!("invalid tenant key");
    };
    let payload = crate::decode64(&signed.payload)?;
    let signature = crate::decode64(&signed.signature)?;
    let Ok(signature) = sign::Signature::from_bytes(&signature) else {
        bail!("invalid signature");
    };
    if !sign::verify_detached(&signature, &payload, &key) {
        bail!("policy signature mismatch");
    }
    let policy: Policy = serde_json::from_slice(&payload)?;
    if policy.device_id != enrollment.device_id || policy.tenant_id != enrollment.tenant_id {
        bail!("policy issued for another device");
    }
    Ok(policy)
}

/// Scrive nella mappa le chiavi effettive, ripristinando quelle tolte.
fn write_overwrite(
    overwrite: &mut HashMap<String, String>,
    baseline: &HashMap<String, String>,
    previous: &HashMap<String, String>,
    next: &HashMap<String, String>,
) {
    for key in previous.keys().filter(|k| !next.contains_key(*k)) {
        match baseline.get(key) {
            Some(v) => overwrite.insert(key.clone(), v.clone()),
            None => overwrite.remove(key),
        };
    }
    for (key, value) in next {
        overwrite.insert(key.clone(), value.clone());
    }
}

fn effective_options(policy: &Policy, baseline: &HashMap<String, String>) -> HashMap<String, String> {
    policy
        .options
        .iter()
        .filter(|(k, _)| policy_key_allowed(k))
        .map(|(k, v)| (k.clone(), merge(k, baseline.get(k).map(String::as_str), v)))
        .collect()
}

fn set_account_enabled(enabled: bool) {
    config::HARD_SETTINGS.write().unwrap().insert(
        "disable-account".to_owned(),
        if enabled { "N" } else { "Y" }.to_owned(),
    );
}

/// Applica una mappa di valori già effettivi (servizio o UI).
fn apply_effective(next: HashMap<String, String>, policy: Option<Policy>) {
    let Ok(mut state) = applied().lock() else {
        return;
    };
    let mut overwrite = config::OVERWRITE_SETTINGS.write().unwrap();
    let baseline = state
        .baseline
        .get_or_insert_with(|| {
            overwrite
                .iter()
                .filter(|(k, _)| policy_key_allowed(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .clone();
    write_overwrite(&mut overwrite, &baseline, &state.effective, &next);
    drop(overwrite);
    set_account_enabled(policy.as_ref().map_or(false, |p| p.settings.enable_account));
    state.effective = next;
    state.policy = policy;
}

fn apply(policy: Policy) {
    let baseline = applied()
        .lock()
        .ok()
        .and_then(|a| a.baseline.clone())
        .unwrap_or_else(|| {
            config::OVERWRITE_SETTINGS
                .read()
                .unwrap()
                .iter()
                .filter(|(k, _)| policy_key_allowed(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        });
    let next = effective_options(&policy, &baseline);
    log::info!("tenant policy v{} applied ({} options)", policy.version, next.len());
    apply_effective(next, Some(policy));
}

pub fn version() -> u64 {
    applied()
        .lock()
        .ok()
        .and_then(|a| a.policy.as_ref().map(|p| p.version))
        .unwrap_or(0)
}

pub fn settings() -> PolicySettings {
    applied()
        .lock()
        .ok()
        .and_then(|a| a.policy.as_ref().map(|p| p.settings.clone()))
        .unwrap_or_default()
}

/// Servizio: criterio ricevuto dal server (iscrizione o heartbeat).
pub fn receive(signed: &SignedPolicy) {
    let Some(enrollment) = enrollment::current() else {
        return;
    };
    let policy = match verify(signed, &enrollment) {
        Ok(policy) => policy,
        Err(err) => {
            log::error!("tenant policy rejected: {err}");
            return;
        }
    };
    if policy.version < version() {
        log::warn!("tenant policy v{} older than the applied one", policy.version);
        return;
    }
    match serde_json::to_vec(signed) {
        Ok(bytes) => {
            if let Err(err) = std::fs::write(Config::path(POLICY_FILE), bytes) {
                log::error!("cannot save tenant policy: {err}");
            }
        }
        Err(err) => log::error!("cannot encode tenant policy: {err}"),
    }
    apply(policy);
}

/// Servizio, all'avvio: riapplica l'ultimo criterio verificato.
pub fn apply_saved() {
    let Some(enrollment) = enrollment::current() else {
        return;
    };
    let Ok(bytes) = std::fs::read(Config::path(POLICY_FILE)) else {
        return;
    };
    let result = serde_json::from_slice::<SignedPolicy>(&bytes)
        .map_err(Into::into)
        .and_then(|signed| verify(&signed, &enrollment));
    match result {
        Ok(policy) => apply(policy),
        Err(err) => log::error!("saved tenant policy ignored: {err}"),
    }
}

/// Scollegamento: toglie i valori del criterio e ripristina la GPO.
pub fn clear() {
    let path = Config::path(POLICY_FILE);
    if path.exists() {
        if let Err(err) = std::fs::remove_file(&path) {
            log::error!("cannot remove tenant policy: {err}");
        }
    }
    apply_effective(HashMap::new(), None);
}

fn effective_json() -> String {
    let Ok(state) = applied().lock() else {
        return String::new();
    };
    json!({
        "options": state.effective,
        "settings": state.policy.as_ref().map(|p| p.settings.clone()).unwrap_or_default(),
    })
    .to_string()
}

pub fn on_ipc_get(name: &str) -> Option<String> {
    (name == IPC_EFFECTIVE).then(effective_json)
}

/// UI desktop: allinea le impostazioni gestite a quelle del servizio, al più
/// ogni 5 secondi. Senza servizio non cambia nulla.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn sync_ui() {
    if crate::is_server() {
        return;
    }
    {
        let Ok(mut state) = applied().lock() else {
            return;
        };
        if state
            .last_ui_sync
            .map_or(false, |t| t.elapsed() < UI_SYNC_INTERVAL)
        {
            return;
        }
        state.last_ui_sync = Some(Instant::now());
    }
    let Ok(Some(text)) = crate::ipc::get_config(IPC_EFFECTIVE) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let options: HashMap<String, String> = v
        .get("options")
        .and_then(|o| serde_json::from_value::<HashMap<String, String>>(o.clone()).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|(k, _)| policy_key_allowed(k))
        .collect();
    let settings: PolicySettings = v
        .get("settings")
        .and_then(|s| serde_json::from_value(s.clone()).ok())
        .unwrap_or_default();
    let policy = (!options.is_empty() || settings != PolicySettings::default()).then(|| Policy {
        settings,
        ..Default::default()
    });
    apply_effective(options, policy);
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn sync_ui() {}

/// Chiavi gestite dal criterio in questo processo.
pub fn managed_keys() -> HashSet<String> {
    applied()
        .lock()
        .map(|a| a.effective.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enrollment_with(pk: &sign::PublicKey) -> enrollment::Enrollment {
        enrollment::Enrollment {
            device_id: "dev-1".to_owned(),
            tenant_id: "ten-1".to_owned(),
            tenant_key: crate::encode64(pk.0),
            ..Default::default()
        }
    }

    fn signed(policy: &Value, sk: &sign::SecretKey) -> SignedPolicy {
        let payload = serde_json::to_vec(policy).unwrap();
        SignedPolicy {
            payload: crate::encode64(&payload),
            signature: crate::encode64(sign::sign_detached(&payload, sk).to_bytes()),
        }
    }

    #[test]
    fn restrictive_value_wins() {
        assert_eq!(merge("enable-clipboard", Some("Y"), "N"), "N");
        assert_eq!(merge("enable-clipboard", Some("N"), "Y"), "N");
        assert_eq!(merge("clanto-require-2fa", Some("N"), "Y"), "Y");
        assert_eq!(merge("clanto-lock-settings", Some(""), "Y"), "Y");
        assert_eq!(merge("approve-mode", Some("password"), "click"), "password");
        assert_eq!(merge("approve-mode", None, "click"), "click");
    }

    #[test]
    fn enrollment_keys_never_come_from_policy() {
        assert!(!policy_key_allowed(enrollment::OPT_ENROLL_TOKEN));
        assert!(!policy_key_allowed(enrollment::OPT_UNENROLL));
        assert!(!policy_key_allowed("api-server"));
        assert!(!policy_key_allowed("custom-rendezvous-server"));
        assert!(policy_key_allowed("enable-file-transfer"));
    }

    #[test]
    fn signature_device_and_tenant_are_checked() {
        let (pk, sk) = sign::gen_keypair();
        let e = enrollment_with(&pk);
        let good = json!({"tenant_id": "ten-1", "device_id": "dev-1", "version": 3,
            "options": {"enable-clipboard": "N"}, "settings": {"enable_account": true}});
        let policy = verify(&signed(&good, &sk), &e).unwrap();
        assert_eq!(policy.version, 3);
        assert!(policy.settings.enable_account);

        let other_device = json!({"tenant_id": "ten-1", "device_id": "dev-2", "version": 3});
        assert!(verify(&signed(&other_device, &sk), &e).is_err());

        let (_, other_sk) = sign::gen_keypair();
        assert!(verify(&signed(&good, &other_sk), &e).is_err());

        let mut tampered = signed(&good, &sk);
        tampered.payload = crate::encode64(
            serde_json::to_vec(&json!({"tenant_id": "ten-1", "device_id": "dev-1", "version": 9}))
                .unwrap(),
        );
        assert!(verify(&tampered, &e).is_err());
    }

    #[test]
    fn removed_keys_restore_gpo_values() {
        let baseline = HashMap::from([("enable-clipboard".to_owned(), "Y".to_owned())]);
        let mut overwrite = baseline.clone();
        let first = HashMap::from([
            ("enable-clipboard".to_owned(), "N".to_owned()),
            ("enable-audio".to_owned(), "N".to_owned()),
        ]);
        write_overwrite(&mut overwrite, &baseline, &HashMap::new(), &first);
        assert_eq!(overwrite.get("enable-audio").map(String::as_str), Some("N"));
        write_overwrite(&mut overwrite, &baseline, &first, &HashMap::new());
        assert_eq!(overwrite.get("enable-clipboard").map(String::as_str), Some("Y"));
        assert!(!overwrite.contains_key("enable-audio"));
    }
}
