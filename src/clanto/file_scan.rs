// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Policy e controllo antimalware dei file ricevuti dall'host.

use base::{
    fs::{self, CompletedDownload, TransferJob},
    message_proto::{file_action, FileAction},
};
use hbb_common::{
    anyhow::anyhow,
    config::Config,
    tokio::{sync::Semaphore, task::spawn_blocking, time::timeout},
    ResultType,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{
    io::Write,
    net::{TcpStream, ToSocketAddrs},
};

pub const OPT_FILE_SCAN: &str = "clanto-file-scan";
pub const OPT_FILE_SCAN_CLAMD: &str = "clanto-file-scan-clamd";
pub const OPT_FILE_SCAN_MAX_MB: &str = "clanto-file-scan-max-mb";
pub const OPT_FILE_DIRECTION: &str = "clanto-file-direction";
pub const OPT_FILE_BLOCKED_EXT: &str = "clanto-file-blocked-ext";

const DEFAULT_MAX_MB: u64 = 512;
const BLOCK_SIZE: usize = 1024 * 1024;
const ENGINE_TIMEOUT: Duration = Duration::from_secs(30);
const BLOCKED_SCAN_MESSAGE: &str = "File bloccato dal controllo antimalware";
const BLOCKED_DIRECTION_MESSAGE: &str = "Trasferimento file bloccato dalla policy dell'host";
const BLOCKED_EXTENSION_MESSAGE: &str = "File bloccato per estensione non consentita";
const QUARANTINE_BASENAME_MAX: usize = 120;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct JobOwner {
    conn_id: i32,
    job_id: i32,
}

struct RegisteredJob {
    mode: ScanMode,
    reserved_paths: Vec<String>,
}

#[derive(Default)]
struct JobRegistry {
    jobs: HashMap<JobOwner, RegisteredJob>,
    reservations: HashMap<String, JobOwner>,
}

static JOB_REGISTRY: OnceLock<Mutex<JobRegistry>> = OnceLock::new();
static PREP_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
static SCAN_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScanMode {
    Off,
    Warn,
    Block,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileDirection {
    Both,
    ToHost,
    FromHost,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransferDirection {
    ToHost,
    FromHost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScanOutcomeKind {
    Clean,
    Malware,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScanDecision {
    Deliver,
    Quarantine,
}

struct ScanResult {
    kind: ScanOutcomeKind,
    engine: &'static str,
}

struct ScanPipelineResult {
    size: u64,
    sha256: String,
    scan: ScanResult,
}

pub struct PolicyRejection {
    pub id: i32,
    pub file_num: i32,
    pub message: &'static str,
}

fn scan_mode_from(value: &str) -> ScanMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "warn" => ScanMode::Warn,
        "block" => ScanMode::Block,
        _ => ScanMode::Off,
    }
}

fn scan_mode() -> ScanMode {
    scan_mode_from(&Config::get_option(OPT_FILE_SCAN))
}

fn registry() -> &'static Mutex<JobRegistry> {
    JOB_REGISTRY.get_or_init(|| Mutex::new(JobRegistry::default()))
}

fn lock_registry() -> MutexGuard<'static, JobRegistry> {
    registry().lock().unwrap_or_else(|error| error.into_inner())
}

fn job_owner(job: &TransferJob) -> JobOwner {
    JobOwner {
        conn_id: job.conn_id,
        job_id: job.id(),
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .unwrap_or_else(|_| path.to_owned())
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn reservation_key(path: &Path) -> String {
    let normalized = normalize_path(path);
    #[cfg(target_os = "windows")]
    return normalized.to_string_lossy().to_ascii_lowercase();
    #[cfg(not(target_os = "windows"))]
    normalized.to_string_lossy().into_owned()
}

fn scan_mode_for(owner: JobOwner) -> ScanMode {
    lock_registry()
        .jobs
        .get(&owner)
        .map(|job| job.mode)
        .unwrap_or(ScanMode::Off)
}

pub fn is_scan_enabled_for_job(job: &TransferJob) -> bool {
    scan_mode_for(job_owner(job)) != ScanMode::Off
}

pub fn register_scan_job(job: &TransferJob) -> ResultType<()> {
    let owner = job_owner(job);
    let mode = scan_mode();
    if mode == ScanMode::Off {
        return Ok(());
    }
    let paths = match job.download_paths() {
        Ok(paths) => paths,
        Err(_) => {
            let name = job
                .files()
                .first()
                .map(|entry| audit_basename(&entry.name))
                .unwrap_or_default();
            emit_policy_event(
                "clanto_file_unscannable",
                owner.conn_id,
                &name,
                job.total_size(),
                "reservation-failed",
            );
            return Err(anyhow!(BLOCKED_SCAN_MESSAGE));
        }
    };
    let keys = paths
        .iter()
        .map(|path| reservation_key(path))
        .collect::<Vec<_>>();
    let has_internal_collision = {
        let mut unique_keys = HashSet::with_capacity(keys.len());
        keys.iter().any(|key| !unique_keys.insert(key))
    };
    let mut registry = lock_registry();
    if has_internal_collision
        || registry.jobs.contains_key(&owner)
        || keys.iter().any(|key| {
            registry
                .reservations
                .get(key)
                .map(|reserved_by| *reserved_by != owner)
                .unwrap_or(false)
        })
    {
        drop(registry);
        let name = paths
            .first()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(audit_basename)
            .unwrap_or_default();
        emit_policy_event(
            "clanto_file_unscannable",
            owner.conn_id,
            &name,
            0,
            "destination-reserved",
        );
        return Err(anyhow!(BLOCKED_SCAN_MESSAGE));
    }
    for key in &keys {
        registry.reservations.insert(key.clone(), owner);
    }
    registry.jobs.insert(
        owner,
        RegisteredJob {
            mode,
            reserved_paths: keys,
        },
    );
    Ok(())
}

pub fn release_scan_job(job: &TransferJob) {
    let owner = job_owner(job);
    let mut registry = lock_registry();
    if let Some(job) = registry.jobs.remove(&owner) {
        for path in job.reserved_paths {
            if registry.reservations.get(&path) == Some(&owner) {
                registry.reservations.remove(&path);
            }
        }
    }
}

pub fn release_scan_jobs(jobs: &[TransferJob]) {
    for job in jobs {
        release_scan_job(job);
    }
}

fn file_direction_from(value: &str) -> FileDirection {
    match value.trim().to_ascii_lowercase().as_str() {
        "to-host" => FileDirection::ToHost,
        "from-host" => FileDirection::FromHost,
        "none" => FileDirection::None,
        _ => FileDirection::Both,
    }
}

fn direction_allowed(policy: FileDirection, direction: TransferDirection) -> bool {
    matches!(
        (policy, direction),
        (FileDirection::Both, _)
            | (FileDirection::ToHost, TransferDirection::ToHost)
            | (FileDirection::FromHost, TransferDirection::FromHost)
    )
}

fn normalized_extension(name: &str) -> Option<String> {
    let name = name
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(name)
        .trim_end_matches(|c| c == '.' || c == ' ');
    let (_, extension) = name.rsplit_once('.')?;
    (!extension.is_empty()).then(|| extension.to_ascii_lowercase())
}

fn audit_basename(name: &str) -> String {
    name.rsplit(|character| character == '/' || character == '\\')
        .find(|component| !component.is_empty())
        .unwrap_or_default()
        .to_owned()
}

fn extension_blocked(name: &str, configured: &str) -> bool {
    let Some(extension) = normalized_extension(name) else {
        return false;
    };
    configured.split(',').any(|candidate| {
        candidate
            .trim()
            .trim_start_matches('.')
            .trim_end_matches(|c| c == '.' || c == ' ')
            .eq_ignore_ascii_case(&extension)
    })
}

fn incoming_name<'a>(path: &'a str, entry_name: &'a str) -> &'a str {
    if entry_name.is_empty() {
        path
    } else {
        entry_name
    }
}

fn emit_policy_event(kind: &str, conn_id: i32, name: &str, size: u64, result: &str) {
    let name = audit_basename(name);
    crate::clanto::audit_sink::emit(
        kind,
        &json!({
            "conn_id": conn_id,
            "name": name,
            "size": size,
            "sha256": "",
            "engine": "policy",
            "result": result,
        }),
    );
}

#[cfg(target_os = "ios")]
fn emit_ios_unscannable(conn_id: i32, name: &str, size: u64) {
    let name = audit_basename(name);
    crate::clanto::audit_sink::emit(
        "clanto_file_unscannable",
        &json!({
            "conn_id": conn_id,
            "name": name,
            "size": size,
            "sha256": "",
            "engine": "none",
            "result": "unscannable",
        }),
    );
}

pub fn reject_file_action(action: &FileAction, conn_id: i32) -> Option<PolicyRejection> {
    #[cfg(target_os = "ios")]
    if let Some(file_action::Union::Receive(request)) = action.union.as_ref() {
        let mode = scan_mode();
        if mode != ScanMode::Off {
            let name = request
                .files
                .first()
                .map(|entry| incoming_name(&request.path, &entry.name))
                .unwrap_or(&request.path);
            emit_ios_unscannable(conn_id, name, request.total_size);
            if mode == ScanMode::Block {
                return Some(PolicyRejection {
                    id: request.id,
                    file_num: request.file_num,
                    message: BLOCKED_SCAN_MESSAGE,
                });
            }
        }
    }
    let policy = file_direction_from(&Config::get_option(OPT_FILE_DIRECTION));
    match action.union.as_ref()? {
        file_action::Union::Send(request)
            if fs::JobType::from_proto(request.file_type) == fs::JobType::Generic
                && !direction_allowed(policy, TransferDirection::FromHost) =>
        {
            emit_policy_event(
                "clanto_file_direction_blocked",
                conn_id,
                &request.path,
                0,
                "from-host",
            );
            Some(PolicyRejection {
                id: request.id,
                file_num: request.file_num,
                message: BLOCKED_DIRECTION_MESSAGE,
            })
        }
        file_action::Union::Receive(request)
            if !direction_allowed(policy, TransferDirection::ToHost) =>
        {
            emit_policy_event(
                "clanto_file_direction_blocked",
                conn_id,
                &request.path,
                request.total_size,
                "to-host",
            );
            Some(PolicyRejection {
                id: request.id,
                file_num: request.file_num,
                message: BLOCKED_DIRECTION_MESSAGE,
            })
        }
        file_action::Union::Receive(request) => {
            let configured = Config::get_option(OPT_FILE_BLOCKED_EXT);
            let blocked = request.files.iter().find_map(|entry| {
                let name = incoming_name(&request.path, &entry.name);
                extension_blocked(name, &configured).then(|| {
                    emit_policy_event(
                        "clanto_file_extension_blocked",
                        conn_id,
                        name,
                        entry.size,
                        "blocked",
                    );
                    PolicyRejection {
                        id: request.id,
                        file_num: request.file_num,
                        message: BLOCKED_EXTENSION_MESSAGE,
                    }
                })
            });
            blocked.or_else(|| {
                (request.files.is_empty() && extension_blocked(&request.path, &configured)).then(
                    || {
                        emit_policy_event(
                            "clanto_file_extension_blocked",
                            conn_id,
                            &request.path,
                            request.total_size,
                            "blocked",
                        );
                        PolicyRejection {
                            id: request.id,
                            file_num: request.file_num,
                            message: BLOCKED_EXTENSION_MESSAGE,
                        }
                    },
                )
            })
        }
        _ => None,
    }
}

fn scan_decision(mode: ScanMode, outcome: ScanOutcomeKind) -> ScanDecision {
    match (mode, outcome) {
        (ScanMode::Block, ScanOutcomeKind::Malware | ScanOutcomeKind::Unavailable) => {
            ScanDecision::Quarantine
        }
        _ => ScanDecision::Deliver,
    }
}

fn max_scan_bytes() -> u64 {
    Config::get_option(OPT_FILE_SCAN_MAX_MB)
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|mb| *mb > 0)
        .unwrap_or(DEFAULT_MAX_MB)
        .saturating_mul(1024 * 1024)
}

fn engine_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "AMSI"
    } else if cfg!(any(target_os = "linux", target_os = "macos")) {
        "clamd"
    } else {
        "none"
    }
}

fn unavailable_pipeline(size: u64, sha256: String, engine: &'static str) -> ScanPipelineResult {
    ScanPipelineResult {
        size,
        sha256,
        scan: ScanResult {
            kind: ScanOutcomeKind::Unavailable,
            engine,
        },
    }
}

fn prepare_scan(path: &Path) -> ResultType<(u64, String)> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.sync_all()?;
    let size = file.metadata()?.len();
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; BLOCK_SIZE];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok((size, hex::encode(hasher.finalize())))
}

async fn bounded_scan_pipeline(path: PathBuf, name: String, max_bytes: u64) -> ScanPipelineResult {
    let prep_semaphore = PREP_SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone();
    let prep_permit = match timeout(ENGINE_TIMEOUT, prep_semaphore.acquire_owned()).await {
        Ok(Ok(permit)) => permit,
        _ => return unavailable_pipeline(0, String::new(), "filesystem"),
    };
    let prep_path = path.clone();
    let preparation = match timeout(
        ENGINE_TIMEOUT,
        spawn_blocking(move || {
            let _permit = prep_permit;
            prepare_scan(&prep_path)
        }),
    )
    .await
    {
        Ok(Ok(preparation)) => preparation,
        _ => return unavailable_pipeline(0, String::new(), "filesystem"),
    };
    let (size, sha256) = match preparation {
        Ok(preparation) => preparation,
        Err(_) => return unavailable_pipeline(0, String::new(), "filesystem"),
    };
    if size > max_bytes {
        return unavailable_pipeline(size, sha256, "size-limit");
    }
    let scan_semaphore = SCAN_SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone();
    let scan_permit = match timeout(ENGINE_TIMEOUT, scan_semaphore.acquire_owned()).await {
        Ok(Ok(permit)) => permit,
        _ => return unavailable_pipeline(size, sha256, engine_name()),
    };
    let scan = match timeout(
        ENGINE_TIMEOUT,
        spawn_blocking(move || {
            let _permit = scan_permit;
            scan_with_engine(&path, &name)
        }),
    )
    .await
    {
        Ok(Ok(scan)) => scan,
        _ => ScanResult {
            kind: ScanOutcomeKind::Unavailable,
            engine: engine_name(),
        },
    };
    ScanPipelineResult { size, sha256, scan }
}

#[cfg(target_os = "windows")]
fn scan_with_engine(path: &Path, name: &str) -> ScanResult {
    use windows::{
        core::HSTRING,
        Win32::System::Antimalware::{
            AmsiCloseSession, AmsiInitialize, AmsiOpenSession, AmsiScanBuffer, AmsiUninitialize,
            AMSI_RESULT, AMSI_RESULT_DETECTED,
        },
    };
    use winreg::{
        enums::{HKEY_LOCAL_MACHINE, KEY_READ},
        RegKey,
    };

    // In amsi.h this is a macro rather than an exported function.
    #[allow(non_snake_case)]
    fn AmsiResultIsMalware(result: AMSI_RESULT) -> bool {
        result.0 >= AMSI_RESULT_DETECTED.0
    }

    let providers = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(r"SOFTWARE\Microsoft\AMSI\Providers", KEY_READ);
    if !providers
        .ok()
        .map(|key| key.enum_keys().any(|provider| provider.is_ok()))
        .unwrap_or(false)
    {
        return ScanResult {
            kind: ScanOutcomeKind::Unavailable,
            engine: "AMSI",
        };
    }

    let context = match unsafe { AmsiInitialize(&HSTRING::from("ClantoDesk")) } {
        Ok(context) => context,
        Err(_) => {
            return ScanResult {
                kind: ScanOutcomeKind::Unavailable,
                engine: "AMSI",
            };
        }
    };
    let session = match unsafe { AmsiOpenSession(context) } {
        Ok(session) => session,
        Err(_) => {
            unsafe { AmsiUninitialize(context) };
            return ScanResult {
                kind: ScanOutcomeKind::Unavailable,
                engine: "AMSI",
            };
        }
    };
    let result = (|| -> ResultType<ScanOutcomeKind> {
        let content_name = HSTRING::from(name);
        let mut file = File::open(path)?;
        let mut buffer = vec![0_u8; BLOCK_SIZE];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                return Ok(ScanOutcomeKind::Clean);
            }
            let scan_result = unsafe {
                AmsiScanBuffer(
                    context,
                    buffer.as_ptr().cast(),
                    read as u32,
                    &content_name,
                    Some(session),
                )
            }
            .map_err(|error| anyhow!(error.to_string()))?;
            if AmsiResultIsMalware(scan_result) {
                return Ok(ScanOutcomeKind::Malware);
            }
        }
    })();
    unsafe {
        AmsiCloseSession(context, session);
        AmsiUninitialize(context);
    }
    match result {
        Ok(kind) => ScanResult {
            kind,
            engine: "AMSI",
        },
        Err(_) => ScanResult {
            kind: ScanOutcomeKind::Unavailable,
            engine: "AMSI",
        },
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn scan_with_engine(path: &Path, _name: &str) -> ScanResult {
    let endpoint = Config::get_option(OPT_FILE_SCAN_CLAMD);
    if endpoint.trim().is_empty() {
        return ScanResult {
            kind: ScanOutcomeKind::Unavailable,
            engine: "clamd",
        };
    }
    match clamd_scan(path, endpoint.trim()) {
        Ok(kind) => ScanResult {
            kind,
            engine: "clamd",
        },
        Err(_) => ScanResult {
            kind: ScanOutcomeKind::Unavailable,
            engine: "clamd",
        },
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn scan_with_engine(_path: &Path, _name: &str) -> ScanResult {
    ScanResult {
        kind: ScanOutcomeKind::Unavailable,
        engine: "none",
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
trait ClamdStream: Read + Write {}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl<T: Read + Write> ClamdStream for T {}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn clamd_scan(path: &Path, endpoint: &str) -> ResultType<ScanOutcomeKind> {
    let mut stream: Box<dyn ClamdStream> = if endpoint.starts_with('/') {
        use std::os::unix::net::UnixStream;
        let stream = UnixStream::connect(endpoint)?;
        stream.set_read_timeout(Some(ENGINE_TIMEOUT))?;
        stream.set_write_timeout(Some(ENGINE_TIMEOUT))?;
        Box::new(stream)
    } else {
        let address = endpoint
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| anyhow!("endpoint clamd non valido"))?;
        let stream = TcpStream::connect_timeout(&address, ENGINE_TIMEOUT)?;
        stream.set_read_timeout(Some(ENGINE_TIMEOUT))?;
        stream.set_write_timeout(Some(ENGINE_TIMEOUT))?;
        stream.set_nodelay(true)?;
        Box::new(stream)
    };
    stream.write_all(b"zINSTREAM\0")?;
    let mut file = File::open(path)?;
    let mut buffer = vec![0_u8; BLOCK_SIZE];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        stream.write_all(&(read as u32).to_be_bytes())?;
        stream.write_all(&buffer[..read])?;
    }
    stream.write_all(&0_u32.to_be_bytes())?;
    stream.flush()?;
    let mut response = Vec::with_capacity(256);
    let mut byte = [0_u8; 1];
    while response.len() < 64 * 1024 {
        if stream.read(&mut byte)? == 0 || matches!(byte[0], 0 | b'\n') {
            break;
        }
        response.push(byte[0]);
    }
    let response = String::from_utf8_lossy(&response);
    if response.contains(" FOUND") {
        Ok(ScanOutcomeKind::Malware)
    } else if response.contains(" OK") {
        Ok(ScanOutcomeKind::Clean)
    } else {
        Err(anyhow!("risposta clamd non valida: {}", response.trim()))
    }
}

fn quarantine_path(download: &CompletedDownload, sha256: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let safe_name = safe_quarantine_basename(&download.name);
    let sha256 = if sha256.is_empty() {
        "sha256-unavailable"
    } else {
        sha256
    };
    Config::path("quarantine").join(format!("{timestamp}-{sha256}-{safe_name}"))
}

fn safe_quarantine_basename(name: &str) -> String {
    let safe_name: String = audit_basename(name)
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(QUARANTINE_BASENAME_MAX)
        .collect();
    if safe_name.is_empty() {
        "file".to_owned()
    } else {
        safe_name
    }
}

fn quarantine(download: &CompletedDownload, sha256: &str) -> ResultType<()> {
    let destination = quarantine_path(download, sha256);
    let directory = destination
        .parent()
        .ok_or_else(|| anyhow!("cartella di quarantena non valida"))?;
    std::fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    }
    if let Err(rename_error) = std::fs::rename(&download.download_path, &destination) {
        std::fs::copy(&download.download_path, &destination)
            .map_err(|copy_error| anyhow!("{rename_error}; {copy_error}"))?;
        std::fs::remove_file(&download.download_path)?;
    }
    std::fs::remove_file(format!("{}.digest", download.final_path.to_string_lossy())).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn emit_scan_event(
    kind: ScanOutcomeKind,
    conn_id: i32,
    download: &CompletedDownload,
    size: u64,
    sha256: &str,
    result: &ScanResult,
) {
    let event_kind = match kind {
        ScanOutcomeKind::Malware => "clanto_file_malware",
        ScanOutcomeKind::Unavailable => "clanto_file_unscannable",
        ScanOutcomeKind::Clean => KIND_CLEAN,
    };
    let name = audit_basename(&download.name);
    report_to_host(
        conn_id,
        event_kind,
        &json!({
            "conn_id": conn_id,
            "name": name,
            "size": size,
            "sha256": sha256,
            "engine": result.engine,
            "result": match kind {
                ScanOutcomeKind::Malware => "malware",
                ScanOutcomeKind::Unavailable => "unscannable",
                ScanOutcomeKind::Clean => "clean",
            },
        }),
    );
}

const KIND_CLEAN: &str = "clanto_file_clean";
/// Esiti che la finestra di gestione può riportare alla connessione.
const CM_KINDS: &[&str] = &["clanto_file_malware", "clanto_file_unscannable", KIND_CLEAN];

/// La scansione gira nella finestra di gestione (utente), report e audit nel
/// servizio: l'esito passa dalla connessione IPC della sessione. Senza
/// connessione si registra qui.
fn report_to_host(conn_id: i32, kind: &str, value: &serde_json::Value) {
    #[cfg(not(target_os = "ios"))]
    if crate::ui_cm_interface::clanto_send_to_connection(
        conn_id,
        crate::ipc::Data::ClantoFileScan((kind.to_owned(), value.to_string())),
    ) {
        return;
    }
    crate::clanto::audit_sink::emit(kind, value);
}

/// Lato connessione: esito ricevuto dalla finestra di gestione. Solo i tipi
/// della scansione e sempre con l'id di questa connessione.
pub fn on_cm_report(conn_id: i32, kind: &str, value: &str) {
    if !CM_KINDS.contains(&kind) {
        return;
    }
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(value) else {
        return;
    };
    if !value.is_object() {
        return;
    }
    value["conn_id"] = json!(conn_id);
    crate::clanto::audit_sink::emit(kind, &value);
}

pub async fn finish_completed_download(
    job: &mut TransferJob,
    next_file_num: Option<i32>,
) -> ResultType<bool> {
    let mode = scan_mode_for(job_owner(job));
    if mode == ScanMode::Off || next_file_num == Some(job.file_num()) {
        return Ok(false);
    }
    let Some(download) = job.current_download()? else {
        return Ok(false);
    };
    job.drop_current_download_writer().await?;
    let pipeline = bounded_scan_pipeline(
        download.download_path.clone(),
        download.name.clone(),
        max_scan_bytes(),
    )
    .await;
    let size = pipeline.size;
    let sha256 = pipeline.sha256;
    let result = pipeline.scan;
    emit_scan_event(result.kind, job.conn_id, &download, size, &sha256, &result);
    match scan_decision(mode, result.kind) {
        ScanDecision::Deliver => {
            job.modify_time();
            Ok(true)
        }
        ScanDecision::Quarantine => {
            let quarantine_download = download.clone();
            let quarantine_hash = sha256.clone();
            let quarantine_result =
                spawn_blocking(move || quarantine(&quarantine_download, &quarantine_hash)).await;
            if !matches!(quarantine_result, Ok(Ok(()))) {
                hbb_common::throttled_log!(
                    Duration::from_secs(30),
                    error,
                    "antimalware quarantine failed"
                );
                return Err(anyhow!(
                    "{BLOCKED_SCAN_MESSAGE}: errore durante la quarantena"
                ));
            }
            Err(anyhow!(BLOCKED_SCAN_MESSAGE))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_extensions_cover_double_suffix_case_and_windows_trimming() {
        let configured = "exe,dll,ps1,bat,cmd,vbs,js,msi,scr,lnk";
        assert!(extension_blocked("report.pdf.exe", configured));
        assert!(extension_blocked("A.EXE", configured));
        assert!(extension_blocked("a.exe.", configured));
        assert!(extension_blocked("a.exe ", configured));
        assert!(!extension_blocked("report.exe.pdf", configured));
    }

    #[test]
    fn direction_policy_restricts_only_the_selected_direction() {
        assert!(direction_allowed(
            FileDirection::Both,
            TransferDirection::ToHost
        ));
        assert!(direction_allowed(
            FileDirection::Both,
            TransferDirection::FromHost
        ));
        assert!(direction_allowed(
            FileDirection::ToHost,
            TransferDirection::ToHost
        ));
        assert!(!direction_allowed(
            FileDirection::ToHost,
            TransferDirection::FromHost
        ));
        assert!(direction_allowed(
            FileDirection::FromHost,
            TransferDirection::FromHost
        ));
        assert!(!direction_allowed(
            FileDirection::FromHost,
            TransferDirection::ToHost
        ));
        assert!(!direction_allowed(
            FileDirection::None,
            TransferDirection::ToHost
        ));
        assert!(!direction_allowed(
            FileDirection::None,
            TransferDirection::FromHost
        ));
    }

    #[test]
    fn unavailable_engine_obeys_off_warn_and_block_modes() {
        assert_eq!(
            scan_decision(ScanMode::Off, ScanOutcomeKind::Unavailable),
            ScanDecision::Deliver
        );
        assert_eq!(
            scan_decision(ScanMode::Warn, ScanOutcomeKind::Unavailable),
            ScanDecision::Deliver
        );
        assert_eq!(
            scan_decision(ScanMode::Block, ScanOutcomeKind::Unavailable),
            ScanDecision::Quarantine
        );
    }

    #[test]
    fn quarantine_basename_is_safe_and_bounded() {
        let unsafe_name = format!("directory/{}?.exe", "a".repeat(300));
        let safe_name = safe_quarantine_basename(&unsafe_name);
        assert!(safe_name.len() <= QUARANTINE_BASENAME_MAX);
        assert!(!safe_name.contains('/'));
        assert!(!safe_name.contains('\\'));
        assert!(!safe_name.contains('?'));
    }

    #[test]
    fn audit_uses_only_the_basename() {
        assert_eq!(
            audit_basename("/private/incoming/payload.exe"),
            "payload.exe"
        );
        assert_eq!(
            audit_basename(r"C:\private\incoming\payload.exe"),
            "payload.exe"
        );
    }
}
