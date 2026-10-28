// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Iscrizione del dispositivo a un tenant dell'api-server.
//!
//! Contratto: `clantodesk-server/docs/protocollo.md`.
//!
//! - Senza iscrizione il dispositivo non manda nulla all'api-server
//!   (heartbeat, sysinfo, audit, registrazioni): `device_api()` è `None`.
//! - L'iscrizione vale per l'api-server su cui è stata fatta: se il build
//!   punta altrove i dati non partono.
//! - Solo HTTPS (loopback ammesso per i test).
//! - Le richieste del dispositivo sono firmate con la chiave Ed25519 del
//!   rendezvous; il server risponde `403 revoked` a un dispositivo revocato e
//!   il PC cancella l'iscrizione.
//! - Lo stato vive nella configurazione del servizio: la UI lo legge via IPC.

use hbb_common::{
    bail,
    config::{self, Config},
    log,
    sodiumoxide::crypto::{hash::sha256, sign},
    ResultType,
};
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicI64, Ordering},
        Mutex, OnceLock,
    },
    time::Duration,
};

/// Criteri (GPO, profilo macOS, policy.json Linux).
pub const OPT_ENROLL_TOKEN: &str = "clanto-enroll-token";
pub const OPT_UNENROLL: &str = "clanto-unenroll";

/// IPC: stato (lettura), iscrizione con token e flusso QR (scrittura).
pub const IPC_STATE: &str = "clanto-enrollment";
pub const IPC_ENROLL: &str = "clanto-enroll";
pub const IPC_QR: &str = "clanto-enroll-qr";

const STATE_FILE: &str = "clanto-enrollment.json";
const API_PREFIX: &str = "/api/clanto/v1";
const GPO_RETRY: Duration = Duration::from_secs(10 * 60);
const QR_POLL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Enrollment {
    pub device_id: String,
    pub tenant_id: String,
    pub tenant_name: String,
    /// Chiave pubblica Ed25519 del tenant (base64): verifica criteri e gettoni.
    pub tenant_key: String,
    pub enrolled_at: i64,
    /// api-server su cui è stata fatta l'iscrizione.
    pub api: String,
}

/// Esito visibile all'interfaccia e alla CLI.
#[derive(Clone, Debug, Default, Serialize)]
struct Activity {
    busy: bool,
    error: String,
}

/// Flusso QR come lo vede l'interfaccia.
#[derive(Clone, Debug, Default, Serialize)]
struct QrView {
    /// idle | starting | waiting | claimed | done | expired | rejected | error
    state: String,
    code: String,
    qr: String,
    code_expires_at: i64,
    expires_at: i64,
    tenant_name: String,
    claimed_by: String,
    error: String,
}

#[derive(Default)]
struct QrSession {
    view: QrView,
    session: String,
    generation: u64,
    decision: Option<bool>,
}

static STATE: OnceLock<Mutex<Option<Enrollment>>> = OnceLock::new();
static ACTIVITY: Mutex<Activity> = Mutex::new(Activity {
    busy: false,
    error: String::new(),
});
static QR: OnceLock<Mutex<QrSession>> = OnceLock::new();
/// Correzione dell'orologio (secondi) suggerita dal server con `401 clock`.
static CLOCK_OFFSET: AtomicI64 = AtomicI64::new(0);

fn state() -> &'static Mutex<Option<Enrollment>> {
    STATE.get_or_init(|| Mutex::new(load()))
}

fn qr() -> &'static Mutex<QrSession> {
    QR.get_or_init(|| Mutex::new(QrSession::default()))
}

fn load() -> Option<Enrollment> {
    let text = std::fs::read_to_string(Config::path(STATE_FILE)).ok()?;
    match serde_json::from_str::<Enrollment>(&text) {
        Ok(e) if !e.device_id.is_empty() && decode_key(&e.tenant_key).is_some() => Some(e),
        Ok(_) => None,
        Err(err) => {
            log::error!("enrollment state unreadable: {err}");
            None
        }
    }
}

fn store(e: &Enrollment) -> ResultType<()> {
    let path = Config::path(STATE_FILE);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(e)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn current() -> Option<Enrollment> {
    state().lock().ok()?.clone()
}

pub fn is_enrolled() -> bool {
    current().is_some()
}

fn set_current(e: Option<Enrollment>) {
    if let Ok(mut s) = state().lock() {
        *s = e;
    }
}

/// Cancella l'iscrizione locale e i criteri che ne derivano.
pub fn clear() {
    let path = Config::path(STATE_FILE);
    if path.exists() {
        if let Err(err) = std::fs::remove_file(&path) {
            log::error!("cannot remove enrollment state: {err}");
        }
    }
    set_current(None);
    super::policy::clear();
    super::audit_queue::clear();
    log::info!("device enrollment removed");
}

pub(crate) fn decode_key(b64: &str) -> Option<sign::PublicKey> {
    sign::PublicKey::from_slice(&crate::decode64(b64).ok()?)
}

/// Solo HTTPS; HTTP solo verso loopback (test).
pub fn is_secure_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    match parsed.scheme() {
        "https" => parsed.has_host(),
        "http" => match parsed.host() {
            Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        },
        _ => false,
    }
}

/// api-server configurato, se utilizzabile (HTTPS, non pubblico RustDesk).
pub fn configured_api() -> Option<String> {
    let url = crate::common::get_api_server(
        Config::get_option("api-server"),
        Config::get_option("custom-rendezvous-server"),
    );
    (!url.is_empty() && !crate::is_public(&url) && is_secure_url(&url)).then_some(url)
}

/// api-server a cui il dispositivo può mandare dati: solo da iscritto, e solo
/// a quello dell'iscrizione.
pub fn device_api() -> Option<String> {
    let enrolled = current()?;
    let api = configured_api()?;
    (enrolled.api == api).then_some(api)
}

fn now_secs() -> i64 {
    hbb_common::get_time() / 1000 + CLOCK_OFFSET.load(Ordering::Relaxed)
}

fn secret_key() -> Option<sign::SecretKey> {
    sign::SecretKey::from_slice(&Config::get_key_pair().0)
}

fn public_key_b64() -> Option<String> {
    let pk = Config::get_key_pair().1;
    (pk.len() == sign::PUBLICKEYBYTES).then(|| crate::encode64(pk))
}

fn path_and_query(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => match u.query() {
            Some(q) => format!("{}?{}", u.path(), q),
            None => u.path().to_owned(),
        },
        Err(_) => String::new(),
    }
}

pub(crate) fn device_message(method: &str, path_and_query: &str, ts: i64, body: &[u8]) -> Vec<u8> {
    let digest = crate::encode64(sha256::hash(body).0);
    let ts = ts.to_string();
    let mut msg = Vec::with_capacity(64 + path_and_query.len() + digest.len());
    for (i, part) in [
        "clanto-device-v1",
        method,
        path_and_query,
        ts.as_str(),
        digest.as_str(),
    ]
    .iter()
    .enumerate()
    {
        if i > 0 {
            msg.push(0);
        }
        msg.extend_from_slice(part.as_bytes());
    }
    msg
}

/// Valore di `Authorization` firmato per una richiesta del dispositivo.
/// `None` se il PC non è iscritto o la chiave non è leggibile.
pub fn auth_value(method: &str, url: &str, body: &[u8]) -> Option<String> {
    let (e, sk) = (current()?, secret_key()?);
    let ts = now_secs();
    let sig = sign::sign_detached(&device_message(method, &path_and_query(url), ts, body), &sk);
    Some(format!(
        "ClantoDevice {}:{}:{}",
        e.device_id,
        ts,
        crate::encode64(sig.to_bytes())
    ))
}

/// Intestazione per `post_request*` (formato `Nome: valore`); vuota se il PC
/// non è iscritto.
pub fn auth_header(method: &str, url: &str, body: &[u8]) -> String {
    auth_value(method, url, body)
        .map(|v| format!("Authorization: {v}"))
        .unwrap_or_default()
}

fn error_code(body: &str) -> Option<(String, Value)> {
    let v: Value = serde_json::from_str(body).ok()?;
    let code = v.get("error")?.as_str()?.to_owned();
    Some((code, v))
}

/// Reazioni comuni alle risposte del server alle richieste firmate.
pub fn observe_response(status: u16, body: &str) {
    match (status, error_code(body)) {
        (401, Some((code, v))) if code == "clock" => {
            if let Some(server) = v.get("server_time").and_then(Value::as_i64) {
                let offset = server - hbb_common::get_time() / 1000;
                CLOCK_OFFSET.store(offset, Ordering::Relaxed);
                log::warn!("api-server clock offset {offset}s");
            }
        }
        (403, Some((code, _))) if code == "revoked" => {
            log::warn!("device revoked by the tenant");
            clear();
        }
        _ => {}
    }
}

/// POST firmato. Errore per ogni stato diverso da 2xx.
pub async fn post_signed(url: String, body: String) -> ResultType<String> {
    let header = auth_header("POST", &url, body.as_bytes());
    if header.is_empty() {
        bail!("device not enrolled");
    }
    let (status, text) = crate::post_request_with_status(url, body, &header).await?;
    observe_response(status, &text);
    if !(200..300).contains(&status) {
        bail!("HTTP {status}: {text}");
    }
    Ok(text)
}

/// Campi Clanto dell'heartbeat.
pub fn heartbeat_extend(v: &mut Value) {
    v["clanto_policy_ver"] = json!(super::policy::version());
    let dropped = super::audit_queue::dropped();
    if dropped > 0 {
        v["clanto_audit_dropped"] = json!(dropped);
    }
}

/// Risposta all'heartbeat consegnato: criterio nuovo e conteggi ricevuti.
pub fn heartbeat_response(clanto: Option<Value>, sent: &Value) {
    if let Some(dropped) = sent["clanto_audit_dropped"].as_u64() {
        super::audit_queue::dropped_reported(dropped);
    }
    let policy = clanto
        .and_then(|c| c.get("policy").cloned())
        .filter(|p| !p.is_null())
        .and_then(|p| serde_json::from_value::<super::policy::SignedPolicy>(p).ok());
    if let Some(policy) = policy {
        super::policy::receive(&policy);
    }
}

fn device_info() -> Value {
    json!({
        "id": Config::get_id(),
        "pk": public_key_b64().unwrap_or_default(),
        "uuid": crate::encode64(hbb_common::get_uuid()),
        "ver": crate::VERSION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "hostname": crate::common::hostname(),
        "brand": super::brand::UPDATE_REPO,
    })
}

#[derive(Deserialize)]
struct EnrollResponse {
    device_id: String,
    tenant_id: String,
    tenant_name: String,
    tenant_key: String,
    #[serde(default)]
    policy: Option<super::policy::SignedPolicy>,
}

fn finish_enrollment(api: String, body: &str) -> ResultType<Enrollment> {
    if let Some((code, _)) = error_code(body) {
        bail!("{code}");
    }
    let rsp: EnrollResponse = serde_json::from_str(body)?;
    if rsp.device_id.is_empty() || decode_key(&rsp.tenant_key).is_none() {
        bail!("invalid enrollment response");
    }
    let enrollment = Enrollment {
        device_id: rsp.device_id,
        tenant_id: rsp.tenant_id,
        tenant_name: rsp.tenant_name,
        tenant_key: rsp.tenant_key,
        enrolled_at: hbb_common::get_time() / 1000,
        api,
    };
    store(&enrollment)?;
    set_current(Some(enrollment.clone()));
    log::info!("device enrolled in tenant {}", enrollment.tenant_id);
    if let Some(policy) = rsp.policy {
        super::policy::receive(&policy);
    }
    Ok(enrollment)
}

fn precheck() -> ResultType<String> {
    if is_enrolled() {
        bail!("already_enrolled");
    }
    if config::is_outgoing_only() {
        bail!("outgoing_only");
    }
    let Some(api) = configured_api() else {
        bail!("api_server_not_configured");
    };
    if public_key_b64().is_none() || secret_key().is_none() {
        bail!("device_key_unavailable");
    }
    Ok(api)
}

/// Iscrizione con token (CLI, GPO). Bloccante: va chiamata fuori dalla UI.
pub fn enroll_with_token(token: &str) -> ResultType<Enrollment> {
    let api = precheck()?;
    let token = token.trim();
    if token.is_empty() || token.len() > 512 {
        bail!("invalid_token");
    }
    let mut body = device_info();
    body["token"] = json!(token);
    let text = crate::post_request_sync(format!("{api}{API_PREFIX}/enroll"), body.to_string(), "")?;
    finish_enrollment(api, &text)
}

fn unenroll() {
    if let Some(api) = device_api() {
        let url = format!("{api}{API_PREFIX}/device/unenroll");
        let header = auth_header("POST", &url, b"{}");
        if let Err(err) = crate::post_request_sync(url, "{}".to_owned(), &header) {
            log::warn!("unenroll notice not delivered: {err}");
        }
    }
    clear();
}

fn set_activity(busy: bool, error: String) {
    if let Ok(mut a) = ACTIVITY.lock() {
        a.busy = busy;
        a.error = error;
    }
}

fn start_token_enrollment(token: String) -> bool {
    {
        let Ok(mut a) = ACTIVITY.lock() else {
            return false;
        };
        if a.busy {
            return false;
        }
        a.busy = true;
        a.error.clear();
    }
    std::thread::Builder::new()
        .name("clanto-enroll".to_owned())
        .spawn(move || {
            let error = match enroll_with_token(&token) {
                Ok(_) => String::new(),
                Err(err) => err.to_string(),
            };
            set_activity(false, error);
        })
        .is_ok()
}

// --- QR ("Collega al server") ---

fn qr_signature(action: &str, session: &str, ts: i64) -> Option<String> {
    let sk = secret_key()?;
    let msg = format!("clanto-qr-v1\0{action}\0{session}\0{ts}");
    Some(crate::encode64(sign::sign_detached(msg.as_bytes(), &sk).to_bytes()))
}

fn qr_body(action: &str, session: &str) -> Option<Value> {
    let ts = now_secs();
    // `ts` viaggia come stringa: è la stessa sequenza di byte che si firma.
    Some(json!({"session": session, "ts": ts.to_string(), "sig": qr_signature(action, session, ts)?}))
}

fn qr_update(generation: u64, f: impl FnOnce(&mut QrSession)) -> bool {
    let Ok(mut q) = qr().lock() else {
        return false;
    };
    if q.generation != generation {
        return false;
    }
    f(&mut q);
    true
}

fn qr_fail(generation: u64, error: String) {
    qr_update(generation, |q| {
        q.view.state = "error".to_owned();
        q.view.error = error;
    });
}

fn apply_poll(view: &mut QrView, v: &Value) {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    let n = |k: &str| v.get(k).and_then(Value::as_i64).unwrap_or_default();
    view.state = s("state");
    for (field, key) in [(&mut view.code, "code"), (&mut view.qr, "qr")] {
        let value = s(key);
        if !value.is_empty() {
            *field = value;
        }
    }
    for (field, key) in [
        (&mut view.code_expires_at, "code_expires_at"),
        (&mut view.expires_at, "expires_at"),
    ] {
        let value = n(key);
        if value > 0 {
            *field = value;
        }
    }
    view.tenant_name = s("tenant_name");
    view.claimed_by = s("claimed_by");
}

fn qr_run(generation: u64) {
    let api = match precheck() {
        Ok(api) => api,
        Err(err) => return qr_fail(generation, err.to_string()),
    };
    let start = match crate::post_request_sync(
        format!("{api}{API_PREFIX}/enroll/qr/start"),
        device_info().to_string(),
        "",
    ) {
        Ok(text) => text,
        Err(err) => return qr_fail(generation, err.to_string()),
    };
    if let Some((code, _)) = error_code(&start) {
        return qr_fail(generation, code);
    }
    let Ok(v) = serde_json::from_str::<Value>(&start) else {
        return qr_fail(generation, "invalid response".to_owned());
    };
    let session = v
        .get("session")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if session.is_empty() {
        return qr_fail(generation, "invalid response".to_owned());
    }
    if !qr_update(generation, |q| {
        q.session = session.clone();
        apply_poll(&mut q.view, &v);
        q.view.state = "waiting".to_owned();
    }) {
        return;
    }
    loop {
        std::thread::sleep(QR_POLL);
        let decision = match qr().lock() {
            Ok(q) if q.generation == generation => q.decision,
            _ => return,
        };
        if let Some(accept) = decision {
            let Some(mut body) = qr_body("confirm", &session) else {
                return qr_fail(generation, "device_key_unavailable".to_owned());
            };
            body["accept"] = json!(accept);
            let result = crate::post_request_sync(
                format!("{api}{API_PREFIX}/enroll/qr/confirm"),
                body.to_string(),
                "",
            );
            match (accept, result) {
                (false, _) => {
                    qr_update(generation, |q| q.view.state = "rejected".to_owned());
                }
                (true, Ok(text)) => match finish_enrollment(api.clone(), &text) {
                    Ok(e) => {
                        qr_update(generation, |q| {
                            q.view.state = "done".to_owned();
                            q.view.tenant_name = e.tenant_name;
                        });
                    }
                    Err(err) => qr_fail(generation, err.to_string()),
                },
                (true, Err(err)) => qr_fail(generation, err.to_string()),
            }
            return;
        }
        let Some(body) = qr_body("poll", &session) else {
            return qr_fail(generation, "device_key_unavailable".to_owned());
        };
        let text = match crate::post_request_sync(
            format!("{api}{API_PREFIX}/enroll/qr/poll"),
            body.to_string(),
            "",
        ) {
            Ok(text) => text,
            // Rete instabile: la sessione dura 10 minuti, si riprova.
            Err(err) => {
                log::debug!("qr poll failed: {err}");
                continue;
            }
        };
        if let Some((code, _)) = error_code(&text) {
            // Orologio fuori finestra: si corregge e si riprova al giro dopo.
            if code == "clock" {
                observe_response(401, &text);
                continue;
            }
            return qr_fail(generation, code);
        }
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let mut finished = false;
        if !qr_update(generation, |q| {
            apply_poll(&mut q.view, &v);
            finished = matches!(q.view.state.as_str(), "expired" | "rejected" | "done");
        }) || finished
        {
            return;
        }
    }
}

fn qr_command(command: &str) -> bool {
    let Ok(mut q) = qr().lock() else {
        return false;
    };
    match command {
        "start" => {
            q.generation += 1;
            q.session.clear();
            q.decision = None;
            q.view = QrView {
                state: "starting".to_owned(),
                ..Default::default()
            };
            let generation = q.generation;
            drop(q);
            std::thread::Builder::new()
                .name("clanto-enroll-qr".to_owned())
                .spawn(move || qr_run(generation))
                .is_ok()
        }
        "accept" | "reject" if q.view.state == "claimed" => {
            q.decision = Some(command == "accept");
            true
        }
        "cancel" => {
            q.generation += 1;
            q.view = QrView {
                state: "idle".to_owned(),
                ..Default::default()
            };
            true
        }
        _ => false,
    }
}

// --- IPC (servizio) ---

fn state_json() -> String {
    let enrollment = current();
    let activity = ACTIVITY.lock().map(|a| a.clone()).unwrap_or_default();
    let view = qr().lock().map(|q| q.view.clone()).unwrap_or_default();
    json!({
        "enrolled": enrollment.is_some(),
        "tenant_name": enrollment.as_ref().map(|e| e.tenant_name.clone()).unwrap_or_default(),
        "enrolled_at": enrollment.as_ref().map(|e| e.enrolled_at).unwrap_or_default(),
        "api_configured": configured_api().is_some(),
        "busy": activity.busy,
        "error": activity.error,
        "qr": view,
    })
    .to_string()
}

pub fn on_ipc_get(name: &str) -> Option<String> {
    (name == IPC_STATE).then(state_json)
}

/// `None` se il nome non è nostro. Con le impostazioni bloccate da criterio
/// l'iscrizione si fa solo con il token della GPO.
pub fn on_ipc_set(name: &str, value: &str) -> Option<bool> {
    if name != IPC_ENROLL && name != IPC_QR {
        return None;
    }
    let starts = name == IPC_ENROLL || value == "start";
    if starts && !super::settings_lock::user_write_allowed_now() {
        return Some(false);
    }
    Some(if name == IPC_ENROLL {
        start_token_enrollment(value.to_owned())
    } else {
        qr_command(value)
    })
}

// --- UI ---

/// Stato per l'interfaccia: dal servizio su desktop, locale su mobile.
pub fn ui_state() -> String {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        crate::ipc::get_config(IPC_STATE)
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    state_json()
}

/// Comando del flusso QR dall'interfaccia: start, accept, reject, cancel.
pub fn ui_qr(command: &str) {
    if !ui_is_admin() {
        log::warn!("QR enrollment requires an elevated interface");
        return;
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if let Err(err) = crate::ipc::set_config(IPC_QR, command.to_owned()) {
        log::error!("QR enrollment command not delivered: {err}");
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        on_ipc_set(IPC_QR, command);
    }
}

/// «Collega al server» si apre solo con l'interfaccia avviata come
/// amministratore (Windows: elevata; macOS e Linux: root). Su Android il
/// dispositivo è dell'utente stesso.
pub fn ui_is_admin() -> bool {
    #[cfg(windows)]
    {
        crate::platform::is_elevated(None).unwrap_or_default() || crate::platform::is_root()
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        crate::platform::is_root()
    }
    #[cfg(target_os = "android")]
    {
        true
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux", target_os = "android")))]
    {
        false
    }
}

/// Servizio: criteri salvati, poi GPO di scollegamento o iscrizione.
pub fn start() {
    super::policy::apply_saved();
    super::audit_queue::start();
    let overwrite = config::OVERWRITE_SETTINGS.read().unwrap().clone();
    let unenroll_requested = overwrite.get(OPT_UNENROLL).map(String::as_str) == Some("Y");
    let token = overwrite
        .get(OPT_ENROLL_TOKEN)
        .cloned()
        .unwrap_or_default();
    if unenroll_requested {
        if is_enrolled() {
            std::thread::spawn(unenroll);
        }
        return;
    }
    if token.is_empty() || is_enrolled() {
        return;
    }
    std::thread::Builder::new()
        .name("clanto-enroll-gpo".to_owned())
        .spawn(move || loop {
            match enroll_with_token(&token) {
                Ok(_) => return,
                Err(err) => {
                    let err = err.to_string();
                    log::warn!("enrollment from policy failed: {err}");
                    let permanent = [
                        "already_enrolled",
                        "invalid_token",
                        "token_expired",
                        "token_exhausted",
                        "outgoing_only",
                    ];
                    if permanent.contains(&err.as_str()) {
                        return;
                    }
                }
            }
            std::thread::sleep(GPO_RETRY);
        })
        .ok();
}

// --- CLI ---

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn is_cli_command(arg: &str) -> bool {
    arg == "--enroll" || arg == "--enrollment"
}

/// `--enroll <token>` e `--enrollment` (stato). Il lavoro lo fa il servizio.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn run_cli(args: &[String], admin: bool) {
    if !admin {
        println!("Installation and administrative privileges required!");
        return;
    }
    let read_state = || -> Option<Value> {
        let text = crate::ipc::get_config(IPC_STATE).ok()??;
        serde_json::from_str(&text).ok()
    };
    if args[0] == "--enrollment" {
        match read_state() {
            Some(v) if v["enrolled"] == json!(true) => {
                println!("Enrolled: {}", v["tenant_name"].as_str().unwrap_or_default())
            }
            Some(_) => println!("Not enrolled"),
            None => println!("Service not running"),
        }
        return;
    }
    let Some(token) = args.get(1) else {
        println!("Usage: --enroll <TOKEN>");
        return;
    };
    if crate::ipc::set_config(IPC_ENROLL, token.to_owned()).is_err() {
        println!("Service not running");
        return;
    }
    // L'iscrizione passa dalla rete: si attende l'esito fino a un minuto.
    for _ in 0..60 {
        std::thread::sleep(Duration::from_secs(1));
        let Some(v) = read_state() else {
            continue;
        };
        if v["busy"] == json!(true) {
            continue;
        }
        if v["enrolled"] == json!(true) {
            println!("Done! Enrolled: {}", v["tenant_name"].as_str().unwrap_or_default());
        } else {
            let error = v["error"].as_str().unwrap_or_default();
            println!("Enrollment failed: {}", if error.is_empty() { "refused" } else { error });
        }
        return;
    }
    println!("Enrollment still running, check with --enrollment");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_loopback() {
        assert!(is_secure_url("https://api.clantodesk.clanto.cloud"));
        assert!(is_secure_url("http://127.0.0.1:8080"));
        assert!(is_secure_url("http://localhost:8080"));
        assert!(is_secure_url("http://[::1]:8080"));
        assert!(!is_secure_url("http://api.clantodesk.clanto.cloud"));
        assert!(!is_secure_url("admin_clantodesk.clanto.cloud"));
        assert!(!is_secure_url("ftp://example.org"));
    }

    #[test]
    fn signed_message_layout() {
        let msg = device_message("POST", "/api/heartbeat", 1700000000, b"");
        let expected = [
            &b"clanto-device-v1\0POST\0/api/heartbeat\01700000000\0"[..],
            b"47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=",
        ]
        .concat();
        assert_eq!(msg, expected);
    }

    #[test]
    fn query_is_signed() {
        assert_eq!(
            path_and_query("https://h.example/api/record?type=part&file=a"),
            "/api/record?type=part&file=a"
        );
        assert_eq!(path_and_query("https://h.example/api/heartbeat"), "/api/heartbeat");
    }

    #[test]
    fn qr_body_sends_the_signed_timestamp_as_text() {
        let ts: i64 = 1700000000;
        let msg = format!("clanto-qr-v1\0poll\0s-1\0{ts}");
        assert!(msg.ends_with("\01700000000"));
        assert_eq!(json!({"ts": ts.to_string()})["ts"], json!("1700000000"));
    }

    #[test]
    fn poll_keeps_last_code_when_missing() {
        let mut view = QrView {
            code: "AAAA".to_owned(),
            ..Default::default()
        };
        apply_poll(&mut view, &json!({"state": "claimed", "tenant_name": "Acme"}));
        assert_eq!(view.state, "claimed");
        assert_eq!(view.code, "AAAA");
        assert_eq!(view.tenant_name, "Acme");
    }
}
