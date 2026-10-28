// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Audit verso l'api-server: numerazione e coda su disco.
//!
//! - Ogni evento porta `seq` (contatore del dispositivo, mai riusato),
//!   `seq_epoch` (UUID del contatore, nuovo quando riparte da zero) e `ts`
//!   (millisecondi): il server scarta i doppioni e segnala i buchi.
//! - Un evento non consegnato dopo i tentativi in memoria finisce in coda,
//!   cifrata con la chiave macchina, e riparte quando il server risponde. Il
//!   server scarta i doppioni sul `nonce`, quindi ritentare è sicuro.
//! - Al massimo `MAX_QUEUED` eventi: oltre si scarta il più vecchio e il numero
//!   degli scartati va al server con l'heartbeat (`clanto_audit_dropped`).
//! - Lo scollegamento svuota la coda e fa ripartire il contatore.

use super::enrollment;
use hbb_common::{
    config::Config,
    log,
    password_security::{decrypt_str_or_original_scoped, encrypt_str_or_original_scoped, KeyScope},
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Duration,
};

const STATE_FILE: &str = "clanto-audit-state.json";
const QUEUE_FILE: &str = "clanto-audit-queue";
const MAX_QUEUED: usize = 10_000;
const MAX_RECORD_CHARS: usize = 256 * 1024;
const FLUSH_INTERVAL: Duration = Duration::from_secs(60);
const KINDS: &[&str] = &["conn", "file", "alarm"];

#[derive(Default)]
struct Queue {
    loaded: bool,
    epoch: String,
    next_seq: u64,
    dropped: u64,
    items: VecDeque<(String, Value)>,
}

static QUEUE: OnceLock<Mutex<Queue>> = OnceLock::new();

fn queue() -> &'static Mutex<Queue> {
    QUEUE.get_or_init(Default::default)
}

fn queue_path() -> PathBuf {
    Config::path(QUEUE_FILE)
}

fn load(q: &mut Queue) {
    if q.loaded {
        return;
    }
    q.loaded = true;
    if let Ok(text) = std::fs::read_to_string(Config::path(STATE_FILE)) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            q.next_seq = v["next_seq"].as_u64().unwrap_or(0);
            q.dropped = v["dropped"].as_u64().unwrap_or(0);
            q.epoch = v["seq_epoch"].as_str().unwrap_or_default().to_owned();
        }
    }
    // Contatore nuovo o stato perso: nuova epoca, così il server non scambia
    // i nuovi eventi per doppioni.
    if q.epoch.is_empty() {
        q.epoch = uuid::Uuid::new_v4().to_string();
        q.next_seq = 0;
        save_state(q);
    }
    if let Ok(text) = std::fs::read_to_string(queue_path()) {
        for line in text.lines().filter(|l| !l.is_empty()) {
            let (plain, ok, _) = decrypt_str_or_original_scoped(line, KeyScope::Machine);
            let parsed = ok
                .then(|| serde_json::from_str::<Value>(&plain).ok())
                .flatten();
            match parsed {
                Some(v) => {
                    let kind = v["kind"].as_str().unwrap_or_default().to_owned();
                    if KINDS.contains(&kind.as_str()) {
                        q.items.push_back((kind, v["body"].clone()));
                    }
                }
                None => q.dropped += 1,
            }
        }
    }
}

fn save_state(q: &Queue) {
    let state = json!({"seq_epoch": q.epoch, "next_seq": q.next_seq, "dropped": q.dropped}).to_string();
    if let Err(err) = std::fs::write(Config::path(STATE_FILE), state) {
        log::error!("audit sequence not saved: {err}");
    }
}

fn save_queue(q: &Queue) {
    let path = queue_path();
    if q.items.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path).ok();
        }
        return;
    }
    let tmp = path.with_extension("tmp");
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        for (kind, body) in &q.items {
            let plain = json!({"kind": kind, "body": body}).to_string();
            let sealed = encrypt_str_or_original_scoped(&plain, KeyScope::Machine, MAX_RECORD_CHARS);
            // Senza chiave macchina l'evento non si scrive in chiaro.
            if sealed.is_empty() || sealed == plain {
                continue;
            }
            writeln!(file, "{sealed}")?;
        }
        file.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if let Err(err) = result {
        log::error!("audit queue not saved: {err}");
    }
}

/// Aggiunge `seq` e `ts` all'evento.
pub fn stamp(mut v: Value) -> Value {
    if let Ok(mut q) = queue().lock() {
        load(&mut q);
        v["seq"] = json!(q.next_seq);
        v["seq_epoch"] = json!(q.epoch);
        q.next_seq += 1;
        save_state(&q);
    }
    v["ts"] = json!(hbb_common::get_time());
    v
}

fn kind_of(url: &str) -> Option<&'static str> {
    let path = url::Url::parse(url).ok()?.path().to_owned();
    KINDS
        .iter()
        .copied()
        .find(|k| path.ends_with(&format!("/api/audit/{k}")))
}

/// URL di audit del dispositivo; vuoto se non iscritto.
pub fn audit_url(kind: &str) -> String {
    enrollment::device_api()
        .map(|api| format!("{api}/api/audit/{kind}"))
        .unwrap_or_default()
}

/// Evento non consegnato: in coda per un invio successivo.
pub fn push(url: &str, v: &Value) {
    let Some(kind) = kind_of(url) else {
        return;
    };
    if !enrollment::is_enrolled() {
        return;
    }
    if let Ok(mut q) = queue().lock() {
        load(&mut q);
        while q.items.len() >= MAX_QUEUED {
            q.items.pop_front();
            q.dropped += 1;
        }
        q.items.push_back((kind.to_owned(), v.clone()));
        save_queue(&q);
        save_state(&q);
    }
}

/// Eventi scartati per coda piena o illeggibili, da segnalare al server.
pub fn dropped() -> u64 {
    queue()
        .lock()
        .map(|mut q| {
            load(&mut q);
            q.dropped
        })
        .unwrap_or(0)
}

/// Il server ha ricevuto il conteggio con l'heartbeat.
pub fn dropped_reported(count: u64) {
    if let Ok(mut q) = queue().lock() {
        q.dropped = q.dropped.saturating_sub(count);
        save_state(&q);
    }
}

pub fn clear() {
    if let Ok(mut q) = queue().lock() {
        load(&mut q);
        q.items.clear();
        q.dropped = 0;
        q.epoch = uuid::Uuid::new_v4().to_string();
        q.next_seq = 0;
        save_queue(&q);
        save_state(&q);
    }
}

async fn flush_once() {
    loop {
        let Some(api) = enrollment::device_api() else {
            return;
        };
        let next = queue().lock().ok().and_then(|mut q| {
            load(&mut q);
            q.items.front().cloned()
        });
        let Some((kind, body)) = next else {
            return;
        };
        let url = format!("{api}/api/audit/{kind}");
        let text = body.to_string();
        let header = enrollment::auth_header("POST", &url, text.as_bytes());
        match crate::post_request_with_status(url, text, &header).await {
            Ok((status, rsp)) => {
                enrollment::observe_response(status, &rsp);
                let delivered = (200..300).contains(&status) && rsp.trim().is_empty();
                let rejected = (400..500).contains(&status) && status != 401 && status != 408 && status != 429;
                if !delivered && !rejected {
                    return;
                }
                if rejected {
                    log::error!("queued audit event rejected: {status}");
                }
                if let Ok(mut q) = queue().lock() {
                    q.items.pop_front();
                    save_queue(&q);
                }
            }
            Err(_) => return,
        }
    }
}

/// Servizio: svuota la coda ogni minuto.
pub fn start() {
    std::thread::Builder::new()
        .name("clanto-audit-queue".to_owned())
        .spawn(|| {
            let Ok(rt) = hbb_common::tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(async {
                loop {
                    flush_once().await;
                    hbb_common::tokio::time::sleep(FLUSH_INTERVAL).await;
                }
            });
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_from_url() {
        assert_eq!(kind_of("https://a.example/api/audit/conn"), Some("conn"));
        assert_eq!(kind_of("https://a.example/api/audit/alarm"), Some("alarm"));
        assert_eq!(kind_of("https://a.example/api/audit"), None);
        assert_eq!(kind_of("https://a.example/api/audit/connx"), None);
    }
}
