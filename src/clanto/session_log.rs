// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Registro locale, non bloccante, delle sessioni in entrata.

use chrono::{DateTime, Local, Utc};
use hbb_common::{
    config::Config,
    log,
    password_security::{decrypt_str_or_original_scoped, encrypt_str_or_original_scoped, KeyScope},
};
use serde_derive::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Mutex, Once, OnceLock,
    },
    time::{Duration, SystemTime},
};

pub const OPT_SESSION_LOG_DAYS: &str = "clanto-session-log-days";
const MAX_FILES: usize = 50;
const MAX_RECORD_CHARS: usize = 256 * 1024;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Default, Serialize)]
struct SessionRecord {
    record_type: String,
    session_key: String,
    session_code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    ticket: String,
    conn_id: i32,
    started_at: String,
    ended_at: String,
    duration_secs: Option<u64>,
    peer_id: String,
    peer_name: String,
    account_user: String,
    os_user: String,
    pc_name: String,
    ip: String,
    connection_type: String,
    id_profile: String,
    primary_auth: Option<i64>,
    two_factor: Option<i64>,
    posture_result: String,
    posture_bypasses: Vec<String>,
    permissions_used: HashSet<String>,
    files: Vec<FileRecord>,
    close_reason: String,
    closed: bool,
}

#[derive(Clone, Serialize)]
struct FileRecord {
    direction: String,
    name: String,
    size: i64,
    /// Esito del controllo antimalware sui file ricevuti, vuoto se non fatto.
    #[serde(skip_serializing_if = "String::is_empty")]
    scan: String,
}

#[derive(Default)]
struct Registry {
    sessions: HashMap<String, SessionRecord>,
    active_by_conn: HashMap<i32, String>,
}

#[derive(Debug)]
enum WriterMessage {
    Append(Value),
    Clear,
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
static LAST_CLEANUP: OnceLock<Mutex<Option<SystemTime>>> = OnceLock::new();
static DISABLED_CLEARED: AtomicBool = AtomicBool::new(false);
static WRITER: OnceLock<mpsc::Sender<WriterMessage>> = OnceLock::new();
static MAINTENANCE: Once = Once::new();

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

fn retention_days() -> u64 {
    Config::get_option(OPT_SESSION_LOG_DAYS)
        .parse::<u64>()
        .ok()
        .map(|days| days.min(3650))
        .unwrap_or(180)
}

fn enabled() -> bool {
    logging_enabled_for(retention_days())
}

fn logging_enabled_for(days: u64) -> bool {
    days != 0
}

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn key(conn_id: i32, started_at: &str) -> String {
    format!("{}:{}", conn_id, started_at)
}

fn value_str(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn value_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn connection_type(value: &Value) -> String {
    match value.get("type").and_then(Value::as_i64) {
        Some(1) => "file".to_owned(),
        Some(2) => "tunnel".to_owned(),
        Some(3) => "camera".to_owned(),
        Some(4) => "terminal".to_owned(),
        _ => "control".to_owned(),
    }
}

fn session_for<'a>(registry: &'a mut Registry, conn_id: i32) -> Option<&'a mut SessionRecord> {
    let key = registry.active_by_conn.get(&conn_id)?.clone();
    registry.sessions.get_mut(&key)
}

fn start(value: &Value) -> Option<SessionRecord> {
    let conn_id = value.get("conn_id")?.as_i64()? as i32;
    let started_at = now().to_rfc3339();
    Some(SessionRecord {
        record_type: "session".to_owned(),
        session_key: key(conn_id, &started_at),
        conn_id,
        started_at,
        ip: value_str(value, "ip"),
        posture_result: "not_checked".to_owned(),
        ..Default::default()
    })
}

/// Riceve gli eventi già emessi dall'audit upstream. Non esegue I/O nel thread
/// della sessione: l'apertura e la chiusura vengono scritte da un thread breve.
pub fn on_audit(kind: &str, value: &Value) {
    if !enabled() {
        clear_if_disabled();
        return;
    }
    DISABLED_CLEARED.store(false, Ordering::Release);
    cleanup_if_due();
    match kind {
        "conn" => on_connection(value),
        "file" => on_file(value),
        "alarm" => on_alarm(value),
        "clanto_file_clean" => on_file_scan(kind, value),
        _ if is_file_security_kind(kind) => {
            on_file_scan(kind, value);
            on_file_security(kind, value);
        }
        _ => {}
    }
}

/// Avvia la manutenzione una volta che i default e le policy sono stati caricati.
pub fn initialize() {
    if enabled() {
        cleanup_if_due();
    } else {
        clear_if_disabled();
    }
    MAINTENANCE.call_once(|| {
        if let Err(err) = std::thread::Builder::new()
            .name("clanto-session-log-maintenance".to_owned())
            .spawn(|| loop {
                std::thread::sleep(CLEANUP_INTERVAL);
                if enabled() {
                    DISABLED_CLEARED.store(false, Ordering::Release);
                    cleanup_old_files(&log_dir(), retention_days());
                } else {
                    clear_if_disabled();
                }
            })
        {
            log::debug!("session log maintenance: {}", err);
        }
    });
}

pub fn retention_changed() {
    if enabled() {
        DISABLED_CLEARED.store(false, Ordering::Release);
        cleanup_if_due();
    } else {
        DISABLED_CLEARED.store(false, Ordering::Release);
        clear_if_disabled();
    }
}

fn on_connection(value: &Value) {
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    let to_write = apply_connection(&mut registry, value);
    drop(registry);
    if let Some(record) = to_write {
        write_async(record);
    }
}

fn apply_connection(registry: &mut Registry, value: &Value) -> Option<SessionRecord> {
    if value.get("action").and_then(Value::as_str) == Some("new") {
        let record = start(value)?;
        let opening = record.clone();
        registry
            .active_by_conn
            .insert(record.conn_id, record.session_key.clone());
        registry.sessions.insert(record.session_key.clone(), record);
        return Some(opening);
    }
    let conn_id = value
        .get("conn_id")
        .and_then(Value::as_i64)
        .map(|id| id as i32)?;
    let mut to_write = None;
    if let Some(record) = session_for(registry, conn_id) {
        let mut metadata_changed = false;
        for (key, target) in [
            ("session_code", &mut record.session_code),
            ("ticket", &mut record.ticket),
            ("id_profile", &mut record.id_profile),
            ("account_user", &mut record.account_user),
            ("os_user", &mut record.os_user),
            ("pc_name", &mut record.pc_name),
        ] {
            let value = value_str(value, key);
            if !value.is_empty() && *target != value {
                *target = value;
                metadata_changed = true;
            }
        }
        if value.get("action").and_then(Value::as_str) == Some("close") {
            record.ended_at = now().to_rfc3339();
            record.duration_secs = value.get("duration_secs").and_then(Value::as_u64);
            record.closed = true;
            record.close_reason = value_str(value, "reason");
            if record.close_reason.is_empty() {
                record.close_reason = "end".to_owned();
            }
            to_write = Some(record.clone());
        } else if let Some(peer) = value.get("peer").and_then(Value::as_array) {
            record.peer_id = peer
                .first()
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            record.peer_name = peer
                .get(1)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            record.connection_type = connection_type(value);
            record.primary_auth = value_i64(value, "primary_auth");
            record.two_factor = value_i64(value, "two_factor");
            if let Some(result) = value.get("posture_result").and_then(Value::as_str) {
                record.posture_result = result.to_owned();
            }
        }
        if let Some(bypasses) = value.get("posture_bypasses").and_then(Value::as_array) {
            record.posture_result = "bypassed".to_owned();
            record.posture_bypasses = bypasses
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
        }
        if metadata_changed && to_write.is_none() {
            to_write = Some(record.clone());
        }
    }
    if to_write.as_ref().is_some_and(|record| record.closed) {
        registry.active_by_conn.remove(&conn_id);
    }
    to_write
}

fn on_file(value: &Value) {
    let Some(conn_id) = value
        .get("conn_id")
        .and_then(Value::as_i64)
        .map(|id| id as i32)
    else {
        return;
    };
    let direction = match value.get("type").and_then(Value::as_i64) {
        Some(0) => "to_peer",
        _ => "from_peer",
    }
    .to_owned();
    let info = value
        .get("info")
        .and_then(Value::as_str)
        .and_then(|info| serde_json::from_str::<Value>(info).ok());
    // Un file singolo arriva con il nome vuoto e il percorso in `path`.
    let path_name = basename(&value_str(value, "path")).to_owned();
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    let Some(record) = session_for(&mut registry, conn_id) else {
        return;
    };
    record.permissions_used.insert("file_transfer".to_owned());
    if let Some(files) = info
        .and_then(|info| info.get("files").cloned())
        .and_then(|files| files.as_array().cloned())
    {
        for file in files {
            if record.files.len() == MAX_FILES {
                break;
            }
            if let Some(values) = file.as_array() {
                let name = values.first().and_then(Value::as_str).unwrap_or_default();
                record.files.push(FileRecord {
                    direction: direction.clone(),
                    name: if name.is_empty() { path_name.clone() } else { name.to_owned() },
                    size: values.get(1).and_then(Value::as_i64).unwrap_or_default(),
                    scan: String::new(),
                });
            }
        }
    }
}

fn on_alarm(value: &Value) {
    write_value_async(alarm_record(value));
}

fn alarm_record(value: &Value) -> Value {
    let mut record = json!({
        "record_type": "rejected_attempt",
        "occurred_at": now().to_rfc3339(),
        "conn_id": value.get("conn_id").cloned().unwrap_or(Value::Null),
        "alarm_type": value.get("typ").cloned().unwrap_or(Value::Null),
        "details": value.get("info").cloned().unwrap_or(Value::Null),
    });
    if let Some(info) = value
        .get("info")
        .and_then(Value::as_str)
        .and_then(|v| serde_json::from_str::<Value>(v).ok())
    {
        record["ip"] = info.get("ip").cloned().unwrap_or(Value::Null);
        record["peer_id"] = info.get("id").cloned().unwrap_or(Value::Null);
        record["peer_name"] = info.get("name").cloned().unwrap_or(Value::Null);
        if let Some(reason_class) = info.get("reason_class") {
            record["reason_class"] = reason_class.clone();
        }
        if let Some(reason) = info.get("reason") {
            record["reason_detail"] = reason.clone();
        }
        if let Some(failed_rules) = info.get("failed_rules") {
            record["failed_rules"] = failed_rules.clone();
        }
    }
    record
}

fn is_file_security_kind(kind: &str) -> bool {
    matches!(
        kind,
        "clanto_file_malware"
            | "clanto_file_unscannable"
            | "clanto_file_direction_blocked"
            | "clanto_file_extension_blocked"
    )
}

fn basename(path: &str) -> &str {
    path.rsplit(|character| character == '/' || character == '\\')
        .next()
        .unwrap_or_default()
}

fn scan_label(kind: &str) -> &'static str {
    match kind {
        "clanto_file_clean" => "clean",
        "clanto_file_malware" => "malware",
        "clanto_file_unscannable" => "unscannable",
        "clanto_file_direction_blocked" => "blocked_direction",
        "clanto_file_extension_blocked" => "blocked_extension",
        _ => "",
    }
}

/// Esito del controllo sul file della sessione: il primo ricevuto con quel
/// nome e ancora senza esito, altrimenti una voce nuova.
fn on_file_scan(kind: &str, value: &Value) {
    let Some(conn_id) = value.get("conn_id").and_then(Value::as_i64).map(|id| id as i32) else {
        return;
    };
    let name = basename(&value_str(value, "name")).to_owned();
    let scan = scan_label(kind).to_owned();
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    let Some(record) = session_for(&mut registry, conn_id) else {
        return;
    };
    apply_file_scan(&mut record.files, name, value.get("size").and_then(Value::as_i64), scan);
}

fn apply_file_scan(files: &mut Vec<FileRecord>, name: String, size: Option<i64>, scan: String) {
    if let Some(file) = files
        .iter_mut()
        .find(|f| f.direction == "from_peer" && f.scan.is_empty() && basename(&f.name) == name)
    {
        file.scan = scan;
    } else if files.len() < MAX_FILES {
        files.push(FileRecord {
            direction: "from_peer".to_owned(),
            name,
            size: size.unwrap_or_default(),
            scan,
        });
    }
}

fn on_file_security(kind: &str, value: &Value) {
    write_value_async(file_security_record(kind, value));
}

fn file_security_record(kind: &str, value: &Value) -> Value {
    let name = value_str(value, "name");
    let name = name
        .rsplit(|character| character == '/' || character == '\\')
        .next()
        .unwrap_or_default();
    json!({
        "record_type": "file_security",
        "occurred_at": now().to_rfc3339(),
        "conn_id": value.get("conn_id").and_then(Value::as_i64),
        "event": kind,
        "name": name,
        "size": value.get("size").and_then(Value::as_u64),
        "sha256": value_str(value, "sha256"),
        "engine": value_str(value, "engine"),
        "result": value_str(value, "result"),
    })
}

/// Segna il primo utilizzo del permesso, senza memorizzare contenuti sensibili.
pub fn mark_permission(conn_id: i32, permission: &'static str) {
    if !enabled() {
        return;
    }
    if let Ok(mut registry) = registry().try_lock() {
        mark_permission_in(&mut registry, conn_id, permission);
    }
}

fn mark_permission_in(registry: &mut Registry, conn_id: i32, permission: &str) {
    if let Some(record) = session_for(registry, conn_id) {
        record.permissions_used.insert(permission.to_owned());
    }
}

/// Restituisce il riepilogo completo da inoltrare a syslog/Event Log alla chiusura.
pub fn completed_session(conn_id: i32) -> Option<Value> {
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    let key = registry
        .sessions
        .iter()
        .find(|(_, record)| record.conn_id == conn_id && record.closed)
        .map(|(key, _)| key.clone())?;
    serde_json::to_value(registry.sessions.remove(&key)?).ok()
}

fn log_dir() -> PathBuf {
    Config::path("session-reports")
}

fn write_async(record: SessionRecord) {
    if let Ok(value) = serde_json::to_value(record) {
        write_value_async(value);
    }
}

fn write_value_async(value: Value) {
    if let Err(err) = writer().send(WriterMessage::Append(value)) {
        log::debug!("session log queue: {}", err);
    }
}

fn writer() -> &'static mpsc::Sender<WriterMessage> {
    WRITER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        if let Err(err) = std::thread::Builder::new()
            .name("clanto-session-log".to_owned())
            .spawn(move || {
                // Ultimo anello della catena, letto dai file al primo record.
                let mut chain: Option<Option<ChainLink>> = None;
                for message in receiver {
                    match message {
                        WriterMessage::Append(mut value) if enabled() => {
                            let dir = log_dir();
                            let last = chain.get_or_insert_with(|| last_link(&dir));
                            let link = chain_value(&mut value, last.as_ref());
                            match append_record(&dir, &value) {
                                Ok(()) => *last = Some(link),
                                Err(err) => log::debug!("session log: {}", err),
                            }
                        }
                        WriterMessage::Append(_) => {}
                        WriterMessage::Clear => {
                            clear_files(&log_dir());
                            chain = Some(None);
                        }
                    }
                }
            })
        {
            log::debug!("session log writer: {}", err);
        }
        sender
    })
}

/// Ogni riga e' cifrata con la chiave macchina: i record contengono IP, ID e
/// nomi dei peer. Senza chiave il record non viene scritto in chiaro.
fn seal_line(value: &Value) -> std::io::Result<String> {
    let plain = serde_json::to_string(value)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    let sealed = encrypt_str_or_original_scoped(&plain, KeyScope::Machine, MAX_RECORD_CHARS);
    if sealed.is_empty() || sealed == plain {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "machine key unavailable, session record not written",
        ));
    }
    Ok(sealed)
}

/// Catena d'integrità: ogni record porta numero progressivo (`_seq`), impronta
/// del precedente (`_prev`) e propria (`_hash`, SHA-256 di `_prev` e del record
/// senza `_hash`). Una riga modificata, tolta o spostata rompe la catena dal
/// punto in cui manca. Il primo record rimasto dopo la pulizia per conservazione
/// fa da ancora. Non ferma chi ha la chiave macchina: per quello serve la copia
/// esterna (SIEM).
#[derive(Clone, Debug, PartialEq, Eq)]
struct ChainLink {
    seq: u64,
    hash: String,
}

const CHAIN_GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn chain_hash(prev: &str, value: &Value) -> String {
    use sha2::{Digest, Sha256};
    let mut body = value.clone();
    if let Some(map) = body.as_object_mut() {
        map.remove("_hash");
    }
    let mut digest = Sha256::new();
    digest.update(prev.as_bytes());
    digest.update(b"\n");
    digest.update(serde_json::to_string(&body).unwrap_or_default().as_bytes());
    hex::encode(digest.finalize())
}

fn chain_value(value: &mut Value, last: Option<&ChainLink>) -> ChainLink {
    let seq = last.map_or(1, |l| l.seq + 1);
    let prev = last.map_or(CHAIN_GENESIS.to_owned(), |l| l.hash.clone());
    value["_seq"] = json!(seq);
    value["_prev"] = json!(prev);
    let hash = chain_hash(&prev, value);
    value["_hash"] = json!(hash);
    ChainLink { seq, hash }
}

fn link_of(value: &Value) -> Option<(ChainLink, &str)> {
    Some((
        ChainLink {
            seq: value.get("_seq")?.as_u64()?,
            hash: value.get("_hash")?.as_str()?.to_owned(),
        },
        value.get("_prev")?.as_str()?,
    ))
}

fn session_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("sessions-"))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    // `sessions-AAAA-MM.jsonl`: l'ordine dei nomi è quello cronologico.
    files.sort();
    files
}

fn last_link(dir: &Path) -> Option<ChainLink> {
    let mut last = None;
    for file in session_files(dir) {
        if let Ok(data) = fs::read_to_string(&file) {
            for value in data.lines().filter_map(open_line) {
                if let Some((link, _)) = link_of(&value) {
                    last = Some(link);
                }
            }
        }
    }
    last
}

/// Verifica in ordine di scrittura. `None` per righe senza catena (precedenti
/// alla funzione) o illeggibili; una riga illeggibile rompe l'anello seguente.
#[derive(Default)]
struct ChainVerifier {
    last: Option<ChainLink>,
    gap: bool,
}

impl ChainVerifier {
    fn check(&mut self, value: Option<&Value>) -> Option<bool> {
        let Some(value) = value else {
            self.gap = true;
            return None;
        };
        let (link, prev) = link_of(value)?;
        let linked = match &self.last {
            Some(last) => !self.gap && link.seq == last.seq + 1 && prev == last.hash,
            None => true,
        };
        let ok = linked && chain_hash(prev, value) == link.hash;
        self.last = Some(link);
        self.gap = false;
        Some(ok)
    }
}

/// Le righe in chiaro scritte prima della cifratura restano leggibili.
fn open_line(line: &str) -> Option<Value> {
    if line.starts_with('{') {
        return serde_json::from_str(line).ok();
    }
    let (plain, success, _) = decrypt_str_or_original_scoped(line, KeyScope::Machine);
    success.then(|| serde_json::from_str(&plain).ok()).flatten()
}

fn append_record(dir: &Path, value: &Value) -> std::io::Result<()> {
    let line = seal_line(value)?;
    fs::create_dir_all(dir)?;
    let name = format!("sessions-{}.jsonl", Local::now().format("%Y-%m"));
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(name))?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")
}

fn clear_if_disabled() {
    if DISABLED_CLEARED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    *registry().lock().unwrap_or_else(|e| e.into_inner()) = Registry::default();
    if let Err(err) = writer().send(WriterMessage::Clear) {
        log::debug!("session log cleanup queue: {}", err);
    }
}

fn clear_files(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("sessions-") {
                if let Err(err) = fs::remove_file(entry.path()) {
                    log::debug!("session log cleanup: {}", err);
                }
            }
        }
    }
}

fn cleanup_if_due() {
    let lock = LAST_CLEANUP.get_or_init(|| Mutex::new(None));
    let mut last = lock.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_some_and(|at| {
        at.elapsed()
            .map_or(false, |elapsed| elapsed < CLEANUP_INTERVAL)
    }) {
        return;
    }
    *last = Some(SystemTime::now());
    let days = retention_days();
    std::thread::spawn(move || cleanup_old_files(&log_dir(), days));
}

fn cleanup_old_files(dir: &Path, days: u64) {
    let cutoff = SystemTime::now().checked_sub(Duration::from_secs(days.saturating_mul(86_400)));
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("sessions-") {
            continue;
        }
        let expired = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .zip(cutoff)
            .is_some_and(|(modified, cutoff)| is_expired(modified, cutoff));
        if expired {
            if let Err(err) = fs::remove_file(entry.path()) {
                log::debug!("session log cleanup: {}", err);
            }
        }
    }
}

fn is_expired(modified: SystemTime, cutoff: SystemTime) -> bool {
    modified < cutoff
}

/// Legge soltanto i metadati già persistiti, filtrati dall'intervallo UNIX in
/// millisecondi. È chiamata dal processo di servizio attraverso IPC.
pub fn report_json(from_ms: i64, to_ms: i64) -> String {
    let mut sessions: HashMap<String, Value> = HashMap::new();
    let mut broken_sessions: HashSet<String> = HashSet::new();
    let mut records = Vec::new();
    let mut verifier = ChainVerifier::default();
    for file in session_files(&log_dir()) {
        let Ok(data) = fs::read_to_string(&file) else {
            continue;
        };
        for line in data.lines() {
            let opened = open_line(line);
            let verdict = verifier.check(opened.as_ref());
            let Some(mut record) = opened else {
                continue;
            };
            // Esito per l'interfaccia; i campi della catena non servono oltre.
            if let Some(map) = record.as_object_mut() {
                for key in ["_seq", "_prev", "_hash"] {
                    map.remove(key);
                }
            }
            if let Some(ok) = verdict {
                record["integrity"] = json!(if ok { "ok" } else { "broken" });
                if !ok {
                    if let Some(key) = record.get("session_key").and_then(Value::as_str) {
                        broken_sessions.insert(key.to_owned());
                    }
                }
            }
            let at = record
                .get("started_at")
                .or_else(|| record.get("occurred_at"))
                .and_then(Value::as_str)
                .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.timestamp_millis());
            if at.is_some_and(|at| (from_ms <= 0 || at >= from_ms) && (to_ms <= 0 || at <= to_ms)) {
                if let Some(key) = record.get("session_key").and_then(Value::as_str) {
                    let replace = sessions
                        .get(key)
                        .and_then(|old: &Value| old.get("closed").and_then(Value::as_bool))
                        != Some(true)
                        || record.get("closed").and_then(Value::as_bool) == Some(true);
                    if replace {
                        sessions.insert(key.to_owned(), record);
                    }
                } else {
                    records.push(record);
                }
            }
        }
    }
    for (key, record) in sessions.iter_mut() {
        if broken_sessions.contains(key) {
            record["integrity"] = json!("broken");
        }
    }
    records.extend(sessions.into_values());
    records.sort_by(|left, right| {
        right
            .get("started_at")
            .or_else(|| right.get("occurred_at"))
            .and_then(Value::as_str)
            .cmp(
                &left
                    .get("started_at")
                    .or_else(|| left.get("occurred_at"))
                    .and_then(Value::as_str),
            )
    });
    serde_json::to_string(&records).unwrap_or_else(|_| "[]".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_new_auth_and_close_with_duration() {
        let mut registry = Registry::default();
        let new = json!({"conn_id": 7, "ip": "192.0.2.1", "action": "new"});
        apply_connection(&mut registry, &new);
        let active = apply_connection(
            &mut registry,
            &json!({"conn_id": 7, "peer": ["peer", "Peer"], "type": 0, "primary_auth": 2, "session_code": "S-0123-4567", "id_profile": "support"}),
        )
        .unwrap();
        assert_eq!(active.session_code, "S-0123-4567");
        assert_eq!(active.id_profile, "support");
        assert!(!active.closed);
        assert!(registry.active_by_conn.contains_key(&7));
        let record = apply_connection(
            &mut registry,
            &json!({
                "conn_id": 7,
                "action": "close",
                "duration_secs": 42,
                "account_user": "account",
                "os_user": "ACME\\alice",
                "pc_name": "PC-ALICE",
            }),
        )
        .unwrap();
        assert_eq!(record.peer_id, "peer");
        assert_eq!(record.session_code, "S-0123-4567");
        assert_eq!(record.account_user, "account");
        assert_eq!(record.os_user, "ACME\\alice");
        assert_eq!(record.pc_name, "PC-ALICE");
        assert_eq!(record.duration_secs, Some(42));
        assert!(record.closed);
    }

    #[test]
    fn permission_is_recorded_once() {
        let mut registry = Registry::default();
        apply_connection(&mut registry, &json!({"conn_id": 7, "action": "new"}));
        mark_permission_in(&mut registry, 7, "clipboard");
        mark_permission_in(&mut registry, 7, "clipboard");
        assert_eq!(
            session_for(&mut registry, 7)
                .unwrap()
                .permissions_used
                .len(),
            1
        );
    }

    #[test]
    fn excludes_files_beyond_retention() {
        let now = SystemTime::now();
        let cutoff = now.checked_sub(Duration::from_secs(180 * 86_400)).unwrap();
        let older = now.checked_sub(Duration::from_secs(181 * 86_400)).unwrap();
        assert!(is_expired(older, cutoff));
    }

    #[test]
    fn zero_disables_the_log() {
        assert!(!logging_enabled_for(0));
    }

    #[test]
    fn file_security_record_contains_only_allowed_metadata() {
        let record = file_security_record(
            "clanto_file_malware",
            &json!({
                "conn_id": 7,
                "name": r"C:\incoming\bad.exe",
                "size": 42,
                "sha256": "abc123",
                "engine": "AMSI",
                "result": "malware",
                "path": r"C:\incoming\bad.exe.download",
                "content": "not allowed",
            }),
        );
        let occurred_at = record.get("occurred_at").cloned().unwrap();
        assert!(DateTime::parse_from_rfc3339(occurred_at.as_str().unwrap()).is_ok());
        assert_eq!(
            record,
            json!({
                "record_type": "file_security",
                "occurred_at": occurred_at,
                "conn_id": 7,
                "event": "clanto_file_malware",
                "name": "bad.exe",
                "size": 42,
                "sha256": "abc123",
                "engine": "AMSI",
                "result": "malware",
            })
        );
    }

    #[test]
    fn chain_detects_changed_removed_and_reordered_records() {
        let mut values = Vec::new();
        let mut last = None;
        for i in 0..4 {
            let mut v = json!({"record_type": "session", "n": i});
            last = Some(chain_value(&mut v, last.as_ref()));
            values.push(v);
        }
        let verify = |vals: &[Value]| {
            let mut verifier = ChainVerifier::default();
            vals.iter().map(|v| verifier.check(Some(v))).collect::<Vec<_>>()
        };
        assert!(verify(&values).iter().all(|r| *r == Some(true)));
        let mut changed = values.clone();
        changed[1]["n"] = json!(9);
        assert_eq!(verify(&changed)[1], Some(false));
        let removed = [values[0].clone(), values[2].clone(), values[3].clone()];
        assert_eq!(verify(&removed), vec![Some(true), Some(false), Some(true)]);
        let mut verifier = ChainVerifier::default();
        verifier.check(Some(&values[0]));
        verifier.check(None);
        assert_eq!(verifier.check(Some(&values[2])), Some(false));
        assert_eq!(ChainVerifier::default().check(Some(&json!({"legacy": 1}))), None);
    }

    #[test]
    fn scan_result_is_attached_to_the_received_file() {
        let mut files = vec![FileRecord {
            direction: "from_peer".to_owned(),
            name: "dir/a.txt".to_owned(),
            size: 3,
            scan: String::new(),
        }];
        apply_file_scan(&mut files, "a.txt".to_owned(), Some(3), "clean".to_owned());
        assert_eq!(files[0].scan, "clean");
        apply_file_scan(&mut files, "b.exe".to_owned(), Some(9), "malware".to_owned());
        assert_eq!((files.len(), files[1].scan.as_str()), (2, "malware"));
        assert_eq!(scan_label("clanto_file_clean"), "clean");
        assert_eq!(basename("C:\\in\\x.pdf"), "x.pdf");
    }

    #[test]
    fn recognizes_all_file_security_events() {
        for kind in [
            "clanto_file_malware",
            "clanto_file_unscannable",
            "clanto_file_direction_blocked",
            "clanto_file_extension_blocked",
        ] {
            assert!(is_file_security_kind(kind));
        }
        assert!(!is_file_security_kind("file"));
    }

    #[test]
    fn posture_rejection_report_keeps_class_and_technical_detail() {
        let record = alarm_record(&json!({
            "conn_id": 7,
            "typ": 11,
            "info": json!({
                "id": "peer",
                "name": "Peer",
                "ip": "192.0.2.1",
                "reason_class": "outdated",
                "reason": "revoked release [core:revoked]",
                "failed_rules": ["core:revoked"],
            }).to_string(),
        }));

        assert_eq!(record["reason_class"], "outdated");
        assert_eq!(record["reason_detail"], "revoked release [core:revoked]");
        assert_eq!(record["failed_rules"], json!(["core:revoked"]));
    }

    #[test]
    fn records_are_sealed_and_legacy_lines_stay_readable() {
        let value = json!({"peer_id": "123", "ip": "192.0.2.1"});
        let sealed = seal_line(&value).unwrap();
        assert!(!sealed.contains("192.0.2.1"));
        assert_eq!(open_line(&sealed), Some(value.clone()));
        assert_eq!(open_line(&value.to_string()), Some(value));
        assert_eq!(open_line("not a record"), None);
    }
}
