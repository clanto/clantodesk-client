// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Canale dei comandi del PC iscritto (`clantodesk-server/docs/api-future.md`,
//! `GET /device/commands`, `POST /device/commands/{id}/result`).
//!
//! - Long-poll firmato; consegna almeno una volta, il PC scarta gli `id` già
//!   eseguiti.
//! - Ogni comando arriva firmato dal tenant (`{"payload","signature"}`): un
//!   comando non firmato, per un altro PC o scaduto non si esegue.
//! - Tipi: `update_now`, `resync_policy`, `resync_sysinfo`, `close_session`,
//!   `collect_logs`, `block_sessions`, `unblock_sessions`; gli altri →
//!   `unsupported`.

use super::{device_api, enrollment, tenant_keys};
use hbb_common::{log, tokio};
use serde_derive::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const WAIT_SECS: u64 = 50;
const MIN_EMPTY_INTERVAL: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(300);
const NOT_ENROLLED_SLEEP: Duration = Duration::from_secs(60);
const REMEMBERED_IDS: usize = 256;

static FORCE_SYSINFO: AtomicBool = AtomicBool::new(false);

/// Heartbeat: vero una volta dopo `resync_sysinfo`.
pub fn take_force_sysinfo() -> bool {
    FORCE_SYSINFO.swap(false, Ordering::Relaxed)
}

#[derive(Debug, Deserialize)]
struct Command {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    args: Value,
    device_id: String,
    #[serde(default)]
    expires_at: i64,
}

#[derive(Debug, PartialEq)]
struct Outcome {
    state: &'static str,
    detail: String,
    data: Value,
}

impl Outcome {
    fn new(state: &'static str, detail: impl Into<String>) -> Self {
        Self {
            state,
            detail: detail.into(),
            data: Value::Null,
        }
    }
}

/// Verifica firma, destinatario e scadenza. `Err((id, esito))` se il comando
/// è leggibile ma non va eseguito; `Err((None, …))` se non è nemmeno leggibile.
fn open(raw: &Value, device_id: &str, now: i64) -> Result<Command, (Option<String>, Outcome)> {
    let unsigned_id = raw.get("id").and_then(Value::as_str).map(str::to_owned);
    let Ok(signed) = serde_json::from_value::<tenant_keys::Signed>(raw.clone()) else {
        return Err((unsigned_id, Outcome::new("rejected", "unsigned")));
    };
    let payload = match tenant_keys::verify(&signed, None) {
        Ok(p) => p,
        Err(_) => return Err((unsigned_id, Outcome::new("rejected", "signature"))),
    };
    let cmd: Command = serde_json::from_slice(&payload)
        .map_err(|_| (unsigned_id.clone(), Outcome::new("rejected", "invalid_payload")))?;
    check(cmd, device_id, now)
}

fn check(cmd: Command, device_id: &str, now: i64) -> Result<Command, (Option<String>, Outcome)> {
    if cmd.device_id != device_id {
        return Err((Some(cmd.id), Outcome::new("rejected", "device_mismatch")));
    }
    if cmd.expires_at > 0 && cmd.expires_at < now {
        return Err((Some(cmd.id), Outcome::new("expired", "")));
    }
    Ok(cmd)
}

fn execute(cmd: &Command) -> Outcome {
    match cmd.kind.as_str() {
        "update_now" => {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            let started = super::service_update::request();
            #[cfg(any(target_os = "android", target_os = "ios"))]
            let started = false;
            if started {
                Outcome::new("done", "update check started")
            } else {
                Outcome::new("failed", "updater not available")
            }
        }
        "resync_policy" => {
            super::policy::request_resync();
            Outcome::new("done", "")
        }
        "resync_sysinfo" => {
            FORCE_SYSINFO.store(true, Ordering::Relaxed);
            Outcome::new("done", "")
        }
        "close_session" => {
            let session_id = cmd.args["session_id"].as_str().unwrap_or_default();
            if super::session_grant::close_session(session_id) {
                Outcome::new("done", "")
            } else {
                Outcome::new("failed", "session not open on this device")
            }
        }
        "collect_logs" => match super::device_report::send_diagnostics("tenant_command", Some(&cmd.id)) {
            Ok(id) => Outcome {
                state: "done",
                detail: String::new(),
                data: json!({"diagnostic_id": id}),
            },
            Err(err) => Outcome::new("failed", err),
        },
        "block_sessions" | "unblock_sessions" => {
            if super::session_grant::set_sessions_blocked(cmd.kind == "block_sessions") {
                Outcome::new("done", "")
            } else {
                Outcome::new("failed", "state not saved")
            }
        }
        _ => Outcome::new("unsupported", ""),
    }
}

async fn report(id: &str, outcome: &Outcome) {
    let Some(url) = device_api::url(&format!("/device/commands/{id}/result")) else {
        return;
    };
    let mut detail = outcome.detail.clone();
    detail.truncate(1024);
    let mut body = json!({
        "state": outcome.state,
        "ts": hbb_common::get_time() / 1000,
    });
    if !detail.is_empty() {
        body["detail"] = json!(detail);
    }
    if !outcome.data.is_null() {
        body["data"] = outcome.data.clone();
    }
    if let Err(err) = device_api::post_json(&url, &body).await {
        log::warn!("command {id} result not delivered: {err}");
    }
}

async fn handle(raw: &Value, done: &mut VecDeque<String>) {
    let Some(device_id) = enrollment::current().map(|e| e.device_id) else {
        return;
    };
    let now = hbb_common::get_time() / 1000;
    let cmd = match open(raw, &device_id, now) {
        Ok(cmd) => cmd,
        Err((Some(id), outcome)) => {
            log::warn!("command {id} not executed: {} {}", outcome.state, outcome.detail);
            report(&id, &outcome).await;
            return;
        }
        Err((None, outcome)) => {
            log::warn!("unreadable command: {}", outcome.detail);
            return;
        }
    };
    if done.contains(&cmd.id) {
        return;
    }
    report(&cmd.id, &Outcome::new("accepted", "")).await;
    let kind = cmd.kind.clone();
    // Alcuni comandi bloccano (rete, file): fuori dal runtime del canale.
    let (cmd, outcome) = tokio::task::spawn_blocking(move || {
        let outcome = execute(&cmd);
        (cmd, outcome)
    })
    .await
    .unwrap_or_else(|_| {
        (
            Command {
                id: String::new(),
                kind,
                args: Value::Null,
                device_id: String::new(),
                expires_at: 0,
            },
            Outcome::new("failed", "internal error"),
        )
    });
    if cmd.id.is_empty() {
        return;
    }
    log::info!("command {} ({}) → {}", cmd.id, cmd.kind, outcome.state);
    report(&cmd.id, &outcome).await;
    done.push_back(cmd.id);
    while done.len() > REMEMBERED_IDS {
        done.pop_front();
    }
}

async fn run() {
    let mut cursor = String::new();
    let mut backoff = Duration::from_secs(5);
    let mut done = VecDeque::new();
    loop {
        let Some(base) = device_api::url("/device/commands") else {
            cursor.clear();
            tokio::time::sleep(NOT_ENROLLED_SLEEP).await;
            continue;
        };
        let mut url = format!("{base}?wait={WAIT_SECS}");
        if !cursor.is_empty() {
            url.push_str(&format!("&after={}", urlencode(&cursor)));
        }
        let started = Instant::now();
        match device_api::get_json(&url, Duration::from_secs(WAIT_SECS + 20)).await {
            Ok(v) => {
                backoff = Duration::from_secs(5);
                if let Some(c) = v.get("cursor").and_then(Value::as_str) {
                    cursor = c.to_owned();
                }
                let commands = v
                    .get("commands")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for raw in &commands {
                    handle(raw, &mut done).await;
                }
                if commands.is_empty() && started.elapsed() < MIN_EMPTY_INTERVAL {
                    tokio::time::sleep(MIN_EMPTY_INTERVAL).await;
                }
            }
            Err(err) => {
                log::debug!("command channel: {err}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Servizio: avvia il canale dei comandi.
pub fn start() {
    std::thread::Builder::new()
        .name("clanto-device-commands".to_owned())
        .spawn(|| {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(run());
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(device: &str, expires_at: i64) -> Command {
        Command {
            id: "c1".to_owned(),
            kind: "resync_policy".to_owned(),
            args: Value::Null,
            device_id: device.to_owned(),
            expires_at,
        }
    }

    #[test]
    fn other_device_and_expired_commands_are_not_executed() {
        assert_eq!(
            check(cmd("d2", 0), "d1", 100).unwrap_err().1.detail,
            "device_mismatch"
        );
        assert_eq!(check(cmd("d1", 50), "d1", 100).unwrap_err().1.state, "expired");
        assert!(check(cmd("d1", 0), "d1", 100).is_ok());
        assert!(check(cmd("d1", 200), "d1", 100).is_ok());
    }

    #[test]
    fn unsigned_commands_are_rejected() {
        let raw = json!({"id": "c9", "type": "block_sessions", "device_id": "d1"});
        let (id, outcome) = open(&raw, "d1", 0).unwrap_err();
        assert_eq!(id.as_deref(), Some("c9"));
        assert_eq!(outcome.state, "rejected");
    }

    #[test]
    fn unknown_type_is_unsupported() {
        let mut c = cmd("d1", 0);
        c.kind = "format_disk".to_owned();
        assert_eq!(execute(&c).state, "unsupported");
    }

    #[test]
    fn cursor_is_encoded() {
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
    }
}
