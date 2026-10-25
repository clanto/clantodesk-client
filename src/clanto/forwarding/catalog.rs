// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Catalogo dei servizi di forwarding pubblicato da Clanto e firmato Ed25519.
//!
//! File `{"payload": "<base64 del JSON>", "signature": "<base64>"}`: la firma
//! copre i byte esatti del payload, quindi nessuna canonicalizzazione. La
//! chiave pubblica entra al build (`CLANTO_FORWARD_CATALOG_PK`, base64); senza
//! chiave, firma non valida o catalogo scaduto non vale nessun servizio. Il
//! catalogo facilita, non obbliga: i forward personalizzati seguono la policy.

use hbb_common::{
    log,
    sodiumoxide::{
        base64::{self, Variant},
        crypto::sign,
    },
    tokio,
};
use serde_derive::{Deserialize, Serialize};
use std::{
    sync::RwLock,
    time::{Duration, Instant},
};

pub const CATALOG_URL: &str = "https://clanto.it/api/clantodesk-forward-catalog.json";
const REFRESH_AFTER: Duration = Duration::from_secs(60 * 60);
const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_SERVICES: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Il controller raggiunge una destinazione dalla parte dell'host.
    ToHost,
    /// L'host raggiunge una destinazione dalla parte del controller.
    ToController,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    #[serde(default = "tcp")]
    pub kind: String,
    pub direction: Direction,
    /// `host:porta` come nella policy (CIDR e intervalli ammessi).
    pub target: String,
    #[serde(default)]
    pub description: String,
}

fn tcp() -> String {
    "tcp".to_owned()
}

impl Service {
    pub fn matches(&self, host: &str, port: u16) -> bool {
        super::policy::parse_pattern(&self.target)
            .map_or(false, |p| super::policy::literal_match(&p, host, port))
    }
}

#[derive(Deserialize)]
struct Envelope {
    payload: String,
    signature: String,
}

#[derive(Deserialize)]
struct Payload {
    version: u32,
    /// `YYYY-MM-DD`: dopo questa data il catalogo non vale più.
    expires: String,
    services: Vec<Service>,
}

fn public_key() -> Option<sign::PublicKey> {
    let encoded = option_env!("CLANTO_FORWARD_CATALOG_PK").filter(|v| !v.is_empty())?;
    let bytes = base64::decode(encoded, Variant::Original).ok()?;
    sign::PublicKey::from_slice(&bytes)
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// Verifica firma, versione, scadenza e forma dei servizi.
pub fn verify(body: &str, pk: &sign::PublicKey, today: &str) -> Result<Vec<Service>, String> {
    let env: Envelope = serde_json::from_str(body).map_err(|e| format!("catalog: {e}"))?;
    let payload = base64::decode(env.payload.trim(), Variant::Original)
        .map_err(|_| "catalog: payload is not base64".to_owned())?;
    let signature = base64::decode(env.signature.trim(), Variant::Original)
        .ok()
        .and_then(|s| sign::Signature::from_bytes(&s).ok())
        .ok_or("catalog: bad signature encoding")?;
    if !sign::verify_detached(&signature, &payload, pk) {
        return Err("catalog: signature does not match".to_owned());
    }
    let payload: Payload =
        serde_json::from_slice(&payload).map_err(|e| format!("catalog payload: {e}"))?;
    if payload.version != 1 {
        return Err(format!("catalog: unsupported version {}", payload.version));
    }
    if payload.expires.len() != 10 || payload.expires.as_str() < today {
        return Err(format!("catalog: expired on {}", payload.expires));
    }
    let services: Vec<Service> = payload
        .services
        .into_iter()
        .filter(|s| {
            let ok = !s.id.is_empty()
                && s.kind == "tcp"
                && super::policy::parse_pattern(&s.target).is_some();
            if !ok {
                log::warn!("catalog: service {:?} ignored", s.id);
            }
            ok
        })
        .take(MAX_SERVICES)
        .collect();
    Ok(services)
}

struct Cache {
    services: Vec<Service>,
    fetched: Option<Instant>,
    next_try: Option<Instant>,
}

static CACHE: RwLock<Cache> = RwLock::new(Cache {
    services: Vec::new(),
    fetched: None,
    next_try: None,
});

/// Servizi validi in questo momento (vuoto se mai scaricato o non valido).
pub fn current() -> Vec<Service> {
    CACHE.read().unwrap_or_else(|e| e.into_inner()).services.clone()
}

async fn fetch(pk: &sign::PublicKey) -> Result<Vec<Service>, String> {
    let client = crate::hbbs_http::create_http_client_async_with_url_strict(CATALOG_URL)
        .await
        .map_err(|e| e.to_string())?;
    let body = tokio::time::timeout(FETCH_TIMEOUT, async {
        client.get(CATALOG_URL).send().await?.error_for_status()?.text().await
    })
    .await
    .map_err(|_| "catalog: timeout".to_owned())?
    .map_err(|e| e.to_string())?;
    verify(&body, pk, &today())
}

/// Aggiorna il catalogo se è vecchio; un errore lascia l'ultimo valido finché
/// non scade, e si ritenta dopo `RETRY_AFTER`.
pub async fn refresh_if_stale() {
    let Some(pk) = public_key() else {
        return;
    };
    let now = Instant::now();
    {
        let cache = CACHE.read().unwrap_or_else(|e| e.into_inner());
        let fresh = cache.fetched.map_or(false, |t| now.duration_since(t) < REFRESH_AFTER);
        let waiting = cache.next_try.map_or(false, |t| now < t);
        if fresh || waiting {
            return;
        }
    }
    let result = fetch(&pk).await;
    let mut cache = CACHE.write().unwrap_or_else(|e| e.into_inner());
    match result {
        Ok(services) => {
            cache.services = services;
            cache.fetched = Some(now);
            cache.next_try = None;
        }
        Err(err) => {
            log::warn!("forward catalog not updated: {err}");
            cache.next_try = Some(now + RETRY_AFTER);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed(json: &str, sk: &sign::SecretKey) -> String {
        let sig = sign::sign_detached(json.as_bytes(), sk);
        serde_json::json!({
            "payload": base64::encode(json, Variant::Original),
            "signature": base64::encode(sig.to_bytes(), Variant::Original),
        })
        .to_string()
    }

    const CATALOG: &str = r#"{"version":1,"expires":"2027-01-31","services":[
        {"id":"rdp","name":"Desktop remoto","direction":"to_host","target":"localhost:3389"},
        {"id":"bad","name":"x","direction":"to_host","target":"nope"}]}"#;

    #[test]
    fn verifies_signature_and_expiry() {
        let (pk, sk) = sign::gen_keypair();
        let services = verify(&signed(CATALOG, &sk), &pk, "2026-10-25").unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].direction, Direction::ToHost);
        assert!(services[0].matches("127.0.0.1", 3389));
        assert!(!services[0].matches("127.0.0.1", 22));
        assert!(verify(&signed(CATALOG, &sk), &pk, "2027-02-01").is_err());
        let (other_pk, _) = sign::gen_keypair();
        assert!(verify(&signed(CATALOG, &sk), &other_pk, "2026-10-25").is_err());
        let tampered = signed(CATALOG, &sk).replace("\"payload\":\"", "\"payload\":\"A");
        assert!(verify(&tampered, &pk, "2026-10-25").is_err());
    }
}
