// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Grant di sessione (`clantodesk-server/docs/api-future.md`, «Grant di
//! sessione», `POST /sessions`, `POST /device/sessions/{id}/ack|consent`).
//!
//! Host (PC iscritto):
//! - la modalità di accesso del criterio (`settings.access_mode`) decide se
//!   serve l'account: senza grant valido il login è rifiutato;
//! - il grant si verifica con le chiavi fidate del tenant (`tenant_keys`),
//!   per questo PC, nella finestra di validità, una sola volta, fuori dalle
//!   revoche e con le sessioni non bloccate;
//! - password e conferma dell'utente si chiedono se le vuole il grant **o** il
//!   criterio locale: il PC non si fida solo del grant;
//! - esito (`ack`) e consenso vanno al server; la sessione si chiude allo
//!   scadere di `max_duration_s` o con il comando `close_session`.
//!
//! Controller: con account attivo, prima di collegarsi chiede la sessione al
//! server e allega il grant al login; alla chiusura la segnala.

use super::{device_api, enrollment, policy, tenant_keys};
use hbb_common::{config::Config, config::LocalConfig, log, tokio};
use serde_derive::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Tolleranza sull'orologio del PC per `not_before` ed `expires_at`.
const CLOCK_SKEW_SECS: i64 = 120;
const BLOCKED_FILE: &str = "clanto-sessions-blocked";
const CONTROLLER_ID_OPTION: &str = "clanto-controller-device-id";
const PREFETCH_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROLLER_GRANT_TTL: Duration = Duration::from_secs(280);
const CONTROLLER_POSTURE_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Messaggi di rifiuto al controller (tradotti in `src/lang`).
pub const MSG_GRANT_REQUIRED: &str = "clanto_grant_required";
pub const MSG_GRANT_INVALID: &str = "clanto_grant_invalid";
pub const MSG_SESSIONS_BLOCKED: &str = "clanto_sessions_blocked";

/// Messaggio per il controller nella lingua dell'host, come gli altri rifiuti
/// del login Clanto.
pub fn translated(key: &str) -> String {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    return crate::lang::translate(key.to_owned());
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return crate::lang::translate_locale(key.to_owned(), "");
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Requirements {
    pub account: bool,
    pub password: bool,
    pub confirm: bool,
}

/// Requisiti della modalità di accesso del criterio.
pub fn requirements(access_mode: &str) -> Requirements {
    let (account, password, confirm) = match access_mode {
        "account_password" | "account_2fa_password" => (true, true, false),
        "account_2fa" => (true, false, false),
        "account_2fa_confirm" => (true, false, true),
        // `password` e valori sconosciuti: come oggi.
        _ => (false, true, false),
    };
    Requirements {
        account,
        password,
        confirm,
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrantInfo {
    pub session_id: String,
    pub user_id: String,
    pub username: String,
    pub nonce: String,
    pub permissions: Vec<String>,
    pub max_duration_s: u64,
    pub needs: Requirements,
}

impl GrantInfo {
    /// Permessi del grant: `view` sempre; elenco vuoto = nessuna restrizione.
    pub fn allows(&self, permission: &str) -> bool {
        self.permissions.is_empty() || self.permissions.iter().any(|p| p == permission)
    }
}

#[derive(Debug, PartialEq)]
pub enum Decision {
    /// PC non gestito o modalità senza account e nessun grant: flusso di oggi.
    Unmanaged,
    /// Rifiuto con codice per l'ack, messaggio per il controller e, se il
    /// grant era leggibile, sessione e nonce.
    Refuse {
        code: &'static str,
        message: &'static str,
        session_id: String,
        nonce: String,
    },
    Grant(GrantInfo),
}

#[derive(Deserialize)]
struct Constraints {
    #[serde(default)]
    max_duration_s: u64,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    not_before: i64,
    expires_at: i64,
}

#[derive(Deserialize)]
struct GrantPayload {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    kid: Option<String>,
    tenant_id: String,
    device_id: String,
    session_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    requires: Vec<String>,
    constraints: Constraints,
    nonce: String,
}

fn now() -> i64 {
    hbb_common::get_time() / 1000
}

/// Nonce dei grant già usati, fino alla loro scadenza.
static SEEN: Mutex<Vec<(String, i64)>> = Mutex::new(Vec::new());

fn mark_seen(nonce: &str, until: i64, now: i64) -> bool {
    let Ok(mut seen) = SEEN.lock() else {
        return false;
    };
    seen.retain(|(_, t)| *t > now);
    if seen.iter().any(|(n, _)| n == nonce) {
        return false;
    }
    seen.push((nonce.to_owned(), until + CLOCK_SKEW_SECS));
    true
}

fn check_payload(
    p: &GrantPayload,
    e: &enrollment::Enrollment,
    local: Requirements,
    now: i64,
) -> Result<GrantInfo, &'static str> {
    if p.kind != "session_grant" {
        return Err("signature_invalid");
    }
    if p.tenant_id != e.tenant_id || p.device_id != e.device_id {
        return Err("device_mismatch");
    }
    if now + CLOCK_SKEW_SECS < p.constraints.not_before
        || now - CLOCK_SKEW_SECS > p.constraints.expires_at
    {
        return Err("expired");
    }
    if p.nonce.is_empty() || p.session_id.is_empty() {
        return Err("signature_invalid");
    }
    let wants = |r: &str| p.requires.iter().any(|x| x == r);
    Ok(GrantInfo {
        session_id: p.session_id.clone(),
        user_id: p.user_id.clone(),
        username: p.username.clone(),
        nonce: p.nonce.clone(),
        permissions: p.constraints.permissions.clone(),
        max_duration_s: p.constraints.max_duration_s,
        needs: Requirements {
            account: true,
            password: local.password || wants("password"),
            confirm: local.confirm || wants("confirm") || wants("consent"),
        },
    })
}

/// Host: decisione sul login. `grant` è il campo `clanto_grant` del login.
pub fn evaluate(grant: &str) -> Decision {
    let Some(e) = enrollment::current() else {
        return Decision::Unmanaged;
    };
    let local = requirements(&policy::settings().access_mode);
    let refuse_with = |code, message, session_id: &str, nonce: &str| Decision::Refuse {
        code,
        message,
        session_id: session_id.to_owned(),
        nonce: nonce.to_owned(),
    };
    if sessions_blocked() {
        return refuse_with("sessions_blocked", MSG_SESSIONS_BLOCKED, "", "");
    }
    if grant.trim().is_empty() {
        return if local.account {
            refuse_with("grant_required", MSG_GRANT_REQUIRED, "", "")
        } else {
            Decision::Unmanaged
        };
    }
    let refuse = |code| refuse_with(code, MSG_GRANT_INVALID, "", "");
    let Ok(signed) = serde_json::from_str::<tenant_keys::Signed>(grant) else {
        return refuse("signature_invalid");
    };
    let Ok(payload) = tenant_keys::verify(&signed, None) else {
        return refuse("signature_invalid");
    };
    let Ok(p) = serde_json::from_slice::<GrantPayload>(&payload) else {
        return refuse("signature_invalid");
    };
    let refuse_known = |code| refuse_with(code, MSG_GRANT_INVALID, &p.session_id, &p.nonce);
    if let Some(kid) = p.kid.as_deref() {
        if tenant_keys::verify(&signed, Some(kid)).is_err() {
            return refuse_known("kid_unknown");
        }
    }
    let now = now();
    let info = match check_payload(&p, &e, local, now) {
        // Un grant per un altro PC non si conferma: la sessione non è nostra.
        Err("device_mismatch") => return refuse("device_mismatch"),
        Err(code) => return refuse_known(code),
        Ok(info) => info,
    };
    if tenant_keys::is_revoked(&info.nonce, &info.session_id, &info.user_id) {
        return refuse_known("revoked");
    }
    if !mark_seen(&info.nonce, p.constraints.expires_at, now) {
        return refuse_known("replayed");
    }
    Decision::Grant(info)
}

// --- Esiti verso il server ---

fn post_async(path: String, body: Value) {
    let Some(url) = device_api::url(&path) else {
        return;
    };
    std::thread::spawn(move || {
        if let Err(err) = device_api::post_json_sync(&url, &body) {
            log::warn!("{path} not delivered: {err}");
        }
    });
}

/// `POST /device/sessions/{id}/ack`. Senza `session_id` (nessun grant) il
/// rifiuto resta nell'audit di connessione.
pub fn ack(session_id: &str, accepted: bool, reason: &str, grant_nonce: &str) {
    if session_id.is_empty() {
        return;
    }
    let mut body = json!({
        "result": if accepted { "accepted" } else { "refused" },
        "grant_nonce": grant_nonce,
        "ts": now(),
        "nonce": uuid::Uuid::new_v4().to_string(),
    });
    if !accepted {
        body["reason"] = json!(reason);
    }
    post_async(format!("/device/sessions/{session_id}/ack"), body);
}

/// `POST /device/sessions/{id}/consent`: decisione di chi è al PC nella
/// finestra di accettazione.
pub fn consent(info: &GrantInfo, decision: &str) {
    let scope: Vec<&str> = ["view", "control", "file", "clipboard", "record"]
        .into_iter()
        .filter(|p| info.allows(p))
        .collect();
    let notice = "clanto-cm-accept";
    let body = json!({
        "decision": decision,
        "notice_id": notice,
        "notice_version": 1,
        "notice_hash": hex_sha256(notice.as_bytes()),
        "notice_lang": crate::get_local_option("lang"),
        "person": {"verified_by": "os_session"},
        "scope": scope,
        "decided_at": now(),
        "nonce": uuid::Uuid::new_v4().to_string(),
    });
    post_async(format!("/device/sessions/{}/consent", info.session_id), body);
}

fn hex_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// --- Sessioni attive, durata massima, blocco ---

struct Active {
    session_id: String,
    started: Instant,
}

static ACTIVE: OnceLock<Mutex<HashMap<i32, Active>>> = OnceLock::new();

fn active() -> &'static Mutex<HashMap<i32, Active>> {
    ACTIVE.get_or_init(Default::default)
}

/// Connessione autorizzata con un grant: durata massima e chiusura da remoto.
pub fn register(conn_id: i32, info: &GrantInfo) {
    if let Ok(mut map) = active().lock() {
        map.insert(
            conn_id,
            Active {
                session_id: info.session_id.clone(),
                started: Instant::now(),
            },
        );
    }
    if info.max_duration_s == 0 {
        return;
    }
    let limit = Duration::from_secs(info.max_duration_s);
    let session_id = info.session_id.clone();
    std::thread::spawn(move || {
        std::thread::sleep(limit);
        let still_open = active()
            .lock()
            .map(|m| {
                m.get(&conn_id)
                    .map_or(false, |a| a.session_id == session_id && a.started.elapsed() >= limit)
            })
            .unwrap_or(false);
        if still_open {
            log::info!("session {session_id} reached its maximum duration");
            disconnect(vec![conn_id]);
        }
    });
}

pub fn unregister(conn_id: i32) {
    if let Ok(mut map) = active().lock() {
        map.remove(&conn_id);
    }
}

fn disconnect(conns: Vec<i32>) {
    #[cfg(not(target_os = "ios"))]
    crate::hbbs_http::sync::disconnect(conns);
    #[cfg(target_os = "ios")]
    let _ = conns;
}

/// Comando `close_session`. Vero se la sessione era aperta su questo PC.
pub fn close_session(session_id: &str) -> bool {
    let conns: Vec<i32> = active()
        .lock()
        .map(|m| {
            m.iter()
                .filter(|(_, a)| a.session_id == session_id)
                .map(|(id, _)| *id)
                .collect()
        })
        .unwrap_or_default();
    let found = !conns.is_empty();
    if found {
        disconnect(conns);
    }
    found
}

pub fn sessions_blocked() -> bool {
    Config::path(BLOCKED_FILE).exists()
}

/// Comandi `block_sessions` / `unblock_sessions`. Il blocco chiude anche le
/// sessioni aperte.
pub fn set_sessions_blocked(blocked: bool) -> bool {
    let path = Config::path(BLOCKED_FILE);
    let ok = if blocked {
        std::fs::write(&path, b"Y").is_ok()
    } else {
        !path.exists() || std::fs::remove_file(&path).is_ok()
    };
    if blocked {
        let all: Vec<i32> = active()
            .lock()
            .map(|m| m.keys().copied().collect())
            .unwrap_or_default();
        #[allow(unused_mut)]
        let mut conns = all;
        #[cfg(not(target_os = "ios"))]
        conns.extend(crate::server::AUTHED_CONNS.lock().unwrap().iter().map(|c| c.conn_id));
        conns.sort_unstable();
        conns.dedup();
        if !conns.is_empty() {
            disconnect(conns);
        }
    }
    ok
}

// --- Controller ---

struct Fetched {
    at: Instant,
    grant: String,
    session_id: String,
}

static FETCHED: OnceLock<Mutex<HashMap<String, Fetched>>> = OnceLock::new();
static LAST_CONTROLLER_POSTURE: Mutex<Option<Instant>> = Mutex::new(None);

fn fetched() -> &'static Mutex<HashMap<String, Fetched>> {
    FETCHED.get_or_init(Default::default)
}

fn account_token() -> Option<String> {
    if hbb_common::config::is_disable_account() {
        return None;
    }
    let token = LocalConfig::get_option("access_token");
    (!token.is_empty()).then_some(token)
}

/// Identificativo stabile del PC del tecnico (non l'ID RustDesk).
pub fn controller_device_id() -> String {
    let id = LocalConfig::get_option(CONTROLLER_ID_OPTION);
    if !id.is_empty() {
        return id;
    }
    let id = uuid::Uuid::new_v4().to_string();
    LocalConfig::set_option(CONTROLLER_ID_OPTION.to_owned(), id.clone());
    id
}

async fn post_bearer(url: String, body: Value, token: &str) -> Option<(u16, Value)> {
    let header = format!("Authorization: Bearer {token}");
    let (status, text) = crate::post_request_with_status(url, body.to_string(), &header)
        .await
        .ok()?;
    Some((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

async fn post_controller_posture(api: &str, token: &str) {
    let due = LAST_CONTROLLER_POSTURE
        .lock()
        .map(|t| t.map_or(true, |t| t.elapsed() >= CONTROLLER_POSTURE_INTERVAL))
        .unwrap_or(false);
    if !due {
        return;
    }
    let mut body = super::device_report::posture_body();
    body["controller_device_id"] = json!(controller_device_id());
    body["app_version"] = json!(crate::VERSION);
    let url = format!("{api}{}/me/device-posture", device_api::API_PREFIX);
    if let Some((status, _)) = post_bearer(url, body, token).await {
        if (200..300).contains(&status) {
            if let Ok(mut t) = LAST_CONTROLLER_POSTURE.lock() {
                *t = Some(Instant::now());
            }
        }
    }
}

async fn request_session(peer_id: String) {
    let (Some(token), Some(api)) = (account_token(), enrollment::configured_api()) else {
        return;
    };
    post_controller_posture(&api, &token).await;
    let mut body = json!({
        "id": peer_id,
        "controller": {"device_id": controller_device_id(), "client_version": crate::VERSION},
    });
    let ticket = super::ticket::for_peer(&peer_id);
    if !ticket.is_empty() {
        body["ticket"] = json!(ticket);
    }
    let url = format!("{api}{}/sessions", device_api::API_PREFIX);
    let Some((status, v)) = post_bearer(url, body, &token).await else {
        return;
    };
    if !(200..300).contains(&status) {
        // 404 = PC non gestito o non visibile: si prosegue senza grant.
        let code = v.get("error").and_then(Value::as_str).unwrap_or_default();
        log::info!("session for {peer_id} not granted: {status} {code}");
        return;
    }
    let (Some(grant), Some(session_id)) = (
        v.get("grant").filter(|g| g.is_object()),
        v.get("session_id").and_then(Value::as_str),
    ) else {
        return;
    };
    if let Ok(mut map) = fetched().lock() {
        map.insert(
            peer_id,
            Fetched {
                at: Instant::now(),
                grant: grant.to_string(),
                session_id: session_id.to_owned(),
            },
        );
    }
}

/// Controller, all'avvio della connessione: chiede la sessione al server
/// (al più 5 secondi, poi si prosegue senza grant).
pub async fn prefetch(peer_id: &str) {
    if account_token().is_none() {
        return;
    }
    let _ = tokio::time::timeout(PREFETCH_TIMEOUT, request_session(peer_id.to_owned())).await;
}

/// Grant da allegare al login verso `peer_id`.
pub fn grant_for(peer_id: &str) -> String {
    fetched()
        .lock()
        .ok()
        .and_then(|map| {
            map.get(peer_id)
                .filter(|f| f.at.elapsed() < CONTROLLER_GRANT_TTL)
                .map(|f| f.grant.clone())
        })
        .unwrap_or_default()
}

/// Controller, fine della connessione: `POST /sessions/{id}/close`.
pub fn controller_closed(peer_id: &str) {
    let Some(session_id) = fetched()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(peer_id).map(|f| f.session_id))
    else {
        return;
    };
    let (Some(token), Some(api)) = (account_token(), enrollment::configured_api()) else {
        return;
    };
    std::thread::spawn(move || {
        let url = format!("{api}{}/sessions/{session_id}/close", device_api::API_PREFIX);
        let header = format!("Authorization: Bearer {token}");
        if let Err(err) = crate::post_request_sync(url, json!({"reason": "completed"}).to_string(), &header) {
            log::warn!("session close not delivered: {err}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enrollment() -> enrollment::Enrollment {
        enrollment::Enrollment {
            device_id: "d1".to_owned(),
            tenant_id: "t1".to_owned(),
            ..Default::default()
        }
    }

    fn payload(requires: &[&str], not_before: i64, expires_at: i64) -> GrantPayload {
        serde_json::from_value(json!({
            "type": "session_grant", "tenant_id": "t1", "device_id": "d1",
            "session_id": "s1", "user_id": "u1", "username": "tech",
            "requires": requires,
            "constraints": {"max_duration_s": 3600, "permissions": ["view", "control"],
                            "not_before": not_before, "expires_at": expires_at},
            "nonce": "n1"
        }))
        .unwrap()
    }

    #[test]
    fn access_modes() {
        assert_eq!(requirements("password"), Requirements { account: false, password: true, confirm: false });
        assert_eq!(requirements(""), requirements("password"));
        assert_eq!(requirements("account_2fa"), Requirements { account: true, password: false, confirm: false });
        assert_eq!(requirements("account_2fa_confirm"), Requirements { account: true, password: false, confirm: true });
        assert_eq!(requirements("account_2fa_password"), Requirements { account: true, password: true, confirm: false });
        assert_eq!(requirements("account_password"), Requirements { account: true, password: true, confirm: false });
    }

    #[test]
    fn local_policy_and_grant_requirements_add_up() {
        let local = requirements("account_2fa");
        let info = check_payload(&payload(&["consent"], 900, 1100), &enrollment(), local, 1000).unwrap();
        assert!(info.needs.confirm);
        assert!(!info.needs.password);
        let local = requirements("account_2fa_password");
        let info = check_payload(&payload(&[], 900, 1100), &enrollment(), local, 1000).unwrap();
        assert!(info.needs.password);
        assert!(info.allows("control"));
        assert!(!info.allows("file"));
    }

    #[test]
    fn grant_for_another_device_or_out_of_time_is_refused() {
        let local = requirements("account_2fa");
        let other = enrollment::Enrollment {
            device_id: "d2".to_owned(),
            ..enrollment()
        };
        assert_eq!(check_payload(&payload(&[], 0, 2000), &other, local, 1000), Err("device_mismatch"));
        assert_eq!(check_payload(&payload(&[], 0, 800), &enrollment(), local, 1000), Err("expired"));
        assert_eq!(check_payload(&payload(&[], 1200, 2000), &enrollment(), local, 1000), Err("expired"));
        assert!(check_payload(&payload(&[], 0, 900), &enrollment(), local, 1000).is_ok());
    }

    #[test]
    fn nonce_is_used_once() {
        assert!(mark_seen("unique-nonce", 2000, 1000));
        assert!(!mark_seen("unique-nonce", 2000, 1000));
        assert!(mark_seen("unique-nonce", 2000, 5000));
    }
}
