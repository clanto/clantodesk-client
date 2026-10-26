// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Riferimento ticket facoltativo: il software di ticketing apre la sessione
//! con `clantodesk://connect/<ID>?ticket=INC-1234` o `--connect <ID> --ticket
//! INC-1234`; il controller lo manda nel login (`clanto_ticket`) e l'host lo
//! registra nell'audit, nel SIEM e nel report sessioni accanto al codice
//! sessione. Non è obbligatorio e non decide nulla sull'accesso.

use std::{collections::HashMap, sync::Mutex};

const MAX_LEN: usize = 64;

static TICKETS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Testo breve e senza caratteri di controllo: finisce in log e SIEM.
pub fn sanitize(ticket: &str) -> String {
    ticket
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || " _-.:#/".contains(*c))
        .take(MAX_LEN)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// ID senza server (`@...`), chiave (`?key=`) e spazi.
fn peer_key(id: &str) -> String {
    id.split(['@', '?'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Controller: riferimento per le prossime connessioni a `id` (vuoto = nessuno).
pub fn set(id: &str, ticket: &str) {
    let mut tickets = TICKETS.lock().unwrap_or_else(|e| e.into_inner());
    let map = tickets.get_or_insert_with(HashMap::new);
    let ticket = sanitize(ticket);
    if ticket.is_empty() {
        map.remove(&peer_key(id));
    } else {
        map.insert(peer_key(id), ticket);
    }
}

/// Controller: riferimento da mettere nel login verso `id`.
pub fn for_peer(id: &str) -> String {
    TICKETS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|map| map.get(&peer_key(id)).cloned())
        .unwrap_or_default()
}

/// Host: aggiunge il riferimento ricevuto all'evento di audit.
pub fn audit(value: &mut serde_json::Value, ticket: &str) {
    let ticket = sanitize(ticket);
    if !ticket.is_empty() {
        value["ticket"] = serde_json::json!(ticket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tickets_are_sanitized_and_bound_to_the_peer() {
        assert_eq!(sanitize("  INC-1234\n<script>"), "INC-1234script");
        assert_eq!(sanitize(&"x".repeat(100)).len(), MAX_LEN);
        set("123 456 789@srv?key=k", "REQ #42");
        assert_eq!(for_peer("123456789"), "REQ #42");
        set("123456789", "");
        assert_eq!(for_peer("123456789"), "");
        let mut v = serde_json::json!({});
        audit(&mut v, "  ");
        assert!(v.get("ticket").is_none());
        audit(&mut v, "INC-1");
        assert_eq!(v["ticket"], "INC-1");
    }
}
