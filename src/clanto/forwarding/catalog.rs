// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Catalogo dei servizi di forwarding pubblicato da Clanto: un JSON con la
//! sola lista dei servizi, `{"version": 1, "services": [...]}`.
//!
//! Il catalogo aiuta a scegliere, non autorizza: un servizio vale su un PC solo
//! se lì è stato spuntato nelle impostazioni, e in quel momento si salva anche
//! la sua destinazione (`policy::OPT_SERVICES_*`). Un catalogo modificato non
//! può quindi allargare ciò che è già attivo.

use hbb_common::{log, tokio};
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
struct Catalog {
    version: u32,
    services: Vec<Service>,
}

/// Verifica versione e forma dei servizi; quelli non validi si scartano.
pub fn parse(body: &str) -> Result<Vec<Service>, String> {
    let catalog: Catalog = serde_json::from_str(body).map_err(|e| format!("catalog: {e}"))?;
    if catalog.version != 1 {
        return Err(format!("catalog: unsupported version {}", catalog.version));
    }
    Ok(catalog
        .services
        .into_iter()
        .filter(|s| {
            let ok = valid_id(&s.id)
                && s.kind == "tcp"
                && super::policy::parse_pattern(&s.target).is_some();
            if !ok {
                log::warn!("catalog: service {:?} ignored", s.id);
            }
            ok
        })
        .take(MAX_SERVICES)
        .collect())
}

/// Id usabile nell'opzione dei servizi attivi (`id=destinazione;...`).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
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

/// Ultimo catalogo scaricato (vuoto se mai riuscito).
pub fn current() -> Vec<Service> {
    CACHE.read().unwrap_or_else(|e| e.into_inner()).services.clone()
}

/// Un solo aggiornamento alla volta: le richieste concorrenti usano la cache.
static REFRESHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

async fn fetch() -> Result<Vec<Service>, String> {
    let client = crate::hbbs_http::create_http_client_async_with_url_strict(CATALOG_URL)
        .await
        .map_err(|e| e.to_string())?;
    let body = tokio::time::timeout(FETCH_TIMEOUT, async {
        client.get(CATALOG_URL).send().await?.error_for_status()?.text().await
    })
    .await
    .map_err(|_| "catalog: timeout".to_owned())?
    .map_err(|e| e.to_string())?;
    parse(&body)
}

/// Aggiorna il catalogo se è vecchio; un errore lascia l'ultimo valido e si
/// ritenta dopo `RETRY_AFTER`.
pub async fn refresh_if_stale() {
    let now = Instant::now();
    {
        let cache = CACHE.read().unwrap_or_else(|e| e.into_inner());
        let fresh = cache.fetched.map_or(false, |t| now.duration_since(t) < REFRESH_AFTER);
        let waiting = cache.next_try.map_or(false, |t| now < t);
        if fresh || waiting {
            return;
        }
    }
    if REFRESHING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let result = fetch().await;
    REFRESHING.store(false, std::sync::atomic::Ordering::SeqCst);
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

    const CATALOG: &str = r#"{"version":1,"services":[
        {"id":"rdp","name":"Desktop remoto","direction":"to_host","target":"localhost:3389"},
        {"id":"bad","name":"x","direction":"to_host","target":"nope"},
        {"id":"a;b=c","name":"x","direction":"to_host","target":"localhost:22"}]}"#;

    #[test]
    fn parses_and_filters_services() {
        let services = parse(CATALOG).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].direction, Direction::ToHost);
        assert!(services[0].matches("127.0.0.1", 3389));
        assert!(!services[0].matches("127.0.0.1", 22));
        assert!(parse(r#"{"version":2,"services":[]}"#).is_err());
        assert!(parse("not json").is_err());
    }
}
