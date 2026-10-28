// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Rapporti del PC iscritto (`clantodesk-server/docs/api-future.md`):
//!
//! - `POST /device/posture`: all'avvio, quando cambia (controllo ogni ora) e
//!   almeno ogni 24 ore. Solo stati, nessun nome di prodotto;
//! - `GET /device/update`: versione assegnata dall'anello di aggiornamento,
//!   firmata dal tenant;
//! - `POST /device/diagnostics`: log compressi, solo su comando del tenant o
//!   con il consenso di chi è al PC.

use super::{device_api, enrollment, posture, tenant_keys};
use hbb_common::{config::Config, log};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
const RESEND_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_DIAGNOSTICS_RAW: u64 = 32 * 1024 * 1024;
const DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(300);

fn flag(p: &std::collections::HashMap<String, String>, key: &str) -> Option<bool> {
    match p.get(key).map(String::as_str) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

fn number(p: &std::collections::HashMap<String, String>, key: &str) -> Value {
    p.get(key)
        .and_then(|v| v.parse::<i64>().ok())
        .map_or(Value::Null, |n| json!(n))
}

fn on_off(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "unknown",
    }
}

/// Postura nel formato di `POST /device/posture` (senza `collected_at` e
/// `nonce`, che cambiano a ogni invio).
pub fn posture_body() -> Value {
    let p = posture::collect();
    let s = |key: &str| p.get(key).cloned().unwrap_or_default();
    json!({
        "disk_encryption": on_off(flag(&p, "disk_encrypted")),
        "antivirus": {
            "present": flag(&p, "av_enabled").is_some(),
            "enabled": flag(&p, "av_enabled").unwrap_or(false),
            "signatures_age_days": number(&p, "av_signature_days"),
        },
        "firewall": on_off(flag(&p, "firewall_enabled")),
        "os": {
            "name": s("platform"),
            "version": s("os_version"),
            "build": s("os_build"),
            "supported": Value::Null,
        },
        "pending_updates": {
            "count": Value::Null,
            "security": Value::Null,
            "oldest_days": number(&p, "os_patch_days"),
        },
        "reboot_pending": flag(&p, "pending_reboot").unwrap_or(false),
    })
}

fn send_posture(body: &Value) -> bool {
    let Some(url) = device_api::url("/device/posture") else {
        return false;
    };
    let mut body = body.clone();
    body["collected_at"] = json!(hbb_common::get_time() / 1000);
    body["nonce"] = json!(uuid::Uuid::new_v4().to_string());
    match device_api::post_json_sync(&url, &body) {
        Ok(_) => true,
        Err(err) => {
            log::warn!("posture report not delivered: {err}");
            false
        }
    }
}

/// Servizio: invio periodico della postura finché il PC è iscritto.
pub fn start() {
    std::thread::Builder::new()
        .name("clanto-posture-report".to_owned())
        .spawn(|| {
            let mut last: Option<(Value, Instant)> = None;
            loop {
                if enrollment::device_api().is_some() {
                    enrollment::refresh_grant_keys();
                    let body = posture_body();
                    let due = last.as_ref().map_or(true, |(prev, at)| {
                        *prev != body || at.elapsed() >= RESEND_INTERVAL
                    });
                    if due && send_posture(&body) {
                        last = Some((body, Instant::now()));
                    }
                } else {
                    last = None;
                }
                std::thread::sleep(CHECK_INTERVAL);
            }
        })
        .ok();
}

/// URL della release assegnata al PC dall'anello di aggiornamento.
/// `None`: PC non iscritto o server senza indicazione (vale il controllo
/// pubblico). `Some("")`: nessun aggiornamento permesso adesso.
pub async fn assigned_update_url() -> Option<String> {
    let url = device_api::url("/device/update")?;
    let v = match device_api::get_json(&url, Duration::from_secs(30)).await {
        Ok(v) => v,
        Err(err) => {
            log::debug!("update ring not available: {err}");
            return None;
        }
    };
    if v.get("assigned").map_or(true, Value::is_null) {
        log::info!("update ring: no update allowed now");
        return Some(String::new());
    }
    let signed: tenant_keys::Signed = serde_json::from_value(v.clone()).ok()?;
    let payload: Value = match tenant_keys::verify(&signed, None) {
        Ok(bytes) => serde_json::from_slice(&bytes).ok()?,
        Err(err) => {
            log::error!("update assignment rejected: {err}");
            return None;
        }
    };
    let device_id = enrollment::current()?.device_id;
    let version = payload.get("version").and_then(Value::as_str)?;
    if payload.get("device_id").and_then(Value::as_str) != Some(device_id.as_str())
        || v["assigned"]["version"].as_str() != Some(version)
        || version.is_empty()
        || !version.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        log::error!("update assignment for another device or version");
        return None;
    }
    // Il download resta quello verificato dall'updater (release GitHub del
    // marchio e firma del codice).
    Some(format!(
        "https://github.com/{}/{}/releases/tag/{version}",
        super::brand::UPDATE_OWNER,
        super::brand::UPDATE_REPO
    ))
}

/// Ultimi log, dal più recente, entro `MAX_DIAGNOSTICS_RAW`, compressi.
fn collect_logs() -> std::io::Result<Vec<u8>> {
    let dir = Config::log_path();
    let mut files: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let mut raw = Vec::new();
    for (_, path) in files {
        let bytes = std::fs::read(&path)?;
        if raw.len() as u64 + bytes.len() as u64 > MAX_DIAGNOSTICS_RAW {
            break;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        raw.extend_from_slice(format!("==> {name} <==\n").as_bytes());
        raw.extend_from_slice(&bytes);
        raw.push(b'\n');
    }
    zstd::encode_all(raw.as_slice(), 9)
}

/// `POST /device/diagnostics`. `basis`: `tenant_command` (con `command_id`)
/// o `user_consent`. Bloccante.
pub fn send_diagnostics(basis: &str, command_id: Option<&str>) -> Result<String, String> {
    let mut query = format!("/device/diagnostics?basis={basis}&kind=log&redaction=none");
    match (basis, command_id) {
        ("tenant_command", Some(id)) => query.push_str(&format!("&command_id={id}")),
        ("user_consent", _) => query.push_str(&format!(
            "&consent_ts={}&notice_version=1",
            hbb_common::get_time() / 1000
        )),
        _ => return Err("invalid_basis".to_owned()),
    }
    let url = device_api::url(&query).ok_or("not_enrolled")?;
    let body = collect_logs().map_err(|e| e.to_string())?;
    let runtime = hbb_common::tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let v = runtime
        .block_on(device_api::post_bytes(&url, body, DIAGNOSTICS_TIMEOUT))
        .map_err(|e| e.to_string())?;
    v.get("diagnostic_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "invalid_response".to_owned())
}

pub const IPC_DIAGNOSTICS: &str = "clanto-diagnostics-send";

/// IPC dalla UI: invio volontario della diagnostica (consenso dell'utente).
pub fn on_ipc_set(name: &str) -> Option<bool> {
    if name != IPC_DIAGNOSTICS {
        return None;
    }
    if enrollment::device_api().is_none() {
        return Some(false);
    }
    std::thread::spawn(|| match send_diagnostics("user_consent", None) {
        Ok(id) => log::info!("diagnostics sent with consent: {id}"),
        Err(err) => log::warn!("diagnostics not sent: {err}"),
    });
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posture_values_are_mapped() {
        let p = std::collections::HashMap::from([
            ("disk_encrypted".to_owned(), "true".to_owned()),
            ("firewall_enabled".to_owned(), "unknown".to_owned()),
            ("os_patch_days".to_owned(), "12".to_owned()),
        ]);
        assert_eq!(on_off(flag(&p, "disk_encrypted")), "on");
        assert_eq!(on_off(flag(&p, "firewall_enabled")), "unknown");
        assert_eq!(number(&p, "os_patch_days"), json!(12));
        assert_eq!(number(&p, "missing"), Value::Null);
    }
}
