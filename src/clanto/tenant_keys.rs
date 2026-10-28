// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Chiavi di firma del tenant (`clantodesk-server/docs/protocollo.md`,
//! «Rotazione delle chiavi»; `api-future.md`, `GET /device/grant-keys`).
//!
//! - La prima chiave è quella ricevuta all'iscrizione.
//! - Una chiave nuova entra solo con un documento firmato da una chiave già
//!   fidata (rotazione ordinaria: la precedente resta valida 7 giorni) o dalla
//!   chiave radice Clanto fissata al build (`CLANTO_ROOT_KEY`, compromissione:
//!   le precedenti sono revocate subito).
//! - L'elenco chiavi e revoche dei grant ha versione monotona ed è salvato per
//!   l'uso offline.
//! - `kid` = base64 dei primi 8 byte dello SHA-256 della chiave pubblica.

use super::{enrollment, policy::SignedPolicy};
use hbb_common::{
    bail,
    config::Config,
    log,
    sodiumoxide::crypto::{hash::sha256, sign},
    ResultType,
};
use serde_derive::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

const STATE_FILE: &str = "clanto-tenant-keys.json";
/// Sovrapposizione della chiave precedente dopo una rotazione ordinaria.
pub const RETIRING_SECS: i64 = 7 * 24 * 60 * 60;

pub type Signed = SignedPolicy;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TenantKey {
    pub kid: String,
    pub public_key: String,
    /// active | retiring | revoked
    pub state: String,
    #[serde(default)]
    pub not_after: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Revocation {
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    pub until: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Store {
    tenant_id: String,
    keys: Vec<TenantKey>,
    #[serde(default)]
    rotation_seq: u64,
    #[serde(default)]
    grant_keys_version: u64,
    #[serde(default)]
    revocations: Vec<Revocation>,
}

static STORE: OnceLock<Mutex<Option<Store>>> = OnceLock::new();

fn store() -> &'static Mutex<Option<Store>> {
    STORE.get_or_init(|| Mutex::new(None))
}

fn now() -> i64 {
    hbb_common::get_time() / 1000
}

pub fn kid_of(public_key: &[u8]) -> String {
    crate::encode64(&sha256::hash(public_key).0[..8])
}

fn root_key() -> Option<sign::PublicKey> {
    let b64 = option_env!("CLANTO_ROOT_KEY").filter(|v| !v.is_empty())?;
    sign::PublicKey::from_slice(&crate::decode64(b64).ok()?)
}

fn initial(e: &enrollment::Enrollment) -> Option<Store> {
    let pk = crate::decode64(&e.tenant_key).ok()?;
    Some(Store {
        tenant_id: e.tenant_id.clone(),
        keys: vec![TenantKey {
            kid: kid_of(&pk),
            public_key: e.tenant_key.clone(),
            state: "active".to_owned(),
            not_after: None,
        }],
        ..Default::default()
    })
}

fn load(e: &enrollment::Enrollment) -> Option<Store> {
    let saved = std::fs::read_to_string(Config::path(STATE_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Store>(&text).ok())
        .filter(|s| s.tenant_id == e.tenant_id && !s.keys.is_empty());
    saved.or_else(|| initial(e))
}

fn save(s: &Store) {
    match serde_json::to_vec_pretty(s) {
        Ok(bytes) => {
            if let Err(err) = std::fs::write(Config::path(STATE_FILE), bytes) {
                log::error!("tenant keys not saved: {err}");
            }
        }
        Err(err) => log::error!("tenant keys not encoded: {err}"),
    }
}

/// Esegue `f` sullo stato delle chiavi del tenant corrente.
fn with_store<T>(f: impl FnOnce(&mut Store) -> T) -> Option<T> {
    let e = enrollment::current()?;
    let mut guard = store().lock().ok()?;
    if guard.as_ref().map_or(true, |s| s.tenant_id != e.tenant_id) {
        *guard = load(&e);
    }
    let s = guard.as_mut()?;
    prune(s, now());
    Some(f(s))
}

/// Nuova iscrizione o scollegamento: si riparte dalla chiave dell'iscrizione.
pub fn reset() {
    if let Ok(mut guard) = store().lock() {
        *guard = None;
    }
    let path = Config::path(STATE_FILE);
    if path.exists() {
        std::fs::remove_file(&path).ok();
    }
}

fn prune(s: &mut Store, now: i64) {
    for key in s.keys.iter_mut() {
        if key.state == "retiring" && key.not_after.map_or(false, |t| t <= now) {
            key.state = "revoked".to_owned();
        }
    }
    s.revocations.retain(|r| r.until > now);
}

fn usable(key: &TenantKey, now: i64) -> bool {
    match key.state.as_str() {
        "active" => true,
        "retiring" => key.not_after.map_or(true, |t| t > now),
        _ => false,
    }
}

fn verify_with(pk_b64: &str, signed: &Signed) -> Option<Vec<u8>> {
    let pk = sign::PublicKey::from_slice(&crate::decode64(pk_b64).ok()?)?;
    let payload = crate::decode64(&signed.payload).ok()?;
    let signature = sign::Signature::from_bytes(&crate::decode64(&signed.signature).ok()?).ok()?;
    sign::verify_detached(&signature, &payload, &pk).then_some(payload)
}

fn verify_in(s: &Store, signed: &Signed, kid: Option<&str>, now: i64) -> Option<Vec<u8>> {
    s.keys
        .iter()
        .filter(|k| usable(k, now) && kid.map_or(true, |kid| kid == k.kid))
        .find_map(|k| verify_with(&k.public_key, signed))
}

/// Verifica un documento firmato da una chiave fidata del tenant e ne
/// restituisce il payload. `kid` limita la verifica a quella chiave.
pub fn verify(signed: &Signed, kid: Option<&str>) -> ResultType<Vec<u8>> {
    match with_store(|s| verify_in(s, signed, kid, now())) {
        Some(Some(payload)) => Ok(payload),
        Some(None) => bail!("signature not valid for any trusted tenant key"),
        None => bail!("device not enrolled"),
    }
}

fn verify_root(signed: &Signed) -> Option<Vec<u8>> {
    let pk = root_key()?;
    verify_with(&crate::encode64(pk.0), signed)
}

pub fn current_kid() -> Option<String> {
    with_store(|s| {
        s.keys
            .iter()
            .rev()
            .find(|k| k.state == "active")
            .map(|k| k.kid.clone())
    })
    .flatten()
}

#[derive(Deserialize)]
struct Rotation {
    #[serde(rename = "type", default)]
    kind: String,
    tenant_id: String,
    new_key: String,
    new_key_id: String,
    #[serde(default)]
    valid_from: i64,
    seq: u64,
}

fn apply_rotation(s: &mut Store, r: Rotation, by_root: bool, now: i64) -> ResultType<()> {
    if !r.kind.is_empty() && r.kind != "tenant_key_rotation" {
        bail!("not a key rotation");
    }
    if r.tenant_id != s.tenant_id {
        bail!("rotation for another tenant");
    }
    if r.seq <= s.rotation_seq {
        bail!("rotation {} already applied", r.seq);
    }
    let pk = crate::decode64(&r.new_key)?;
    if sign::PublicKey::from_slice(&pk).is_none() || kid_of(&pk) != r.new_key_id {
        bail!("invalid new tenant key");
    }
    if r.valid_from > now + 300 {
        bail!("rotation not yet valid");
    }
    for key in s.keys.iter_mut().filter(|k| k.state != "revoked") {
        if by_root {
            key.state = "revoked".to_owned();
        } else if key.state == "active" {
            key.state = "retiring".to_owned();
            key.not_after = Some(now + RETIRING_SECS);
        }
    }
    s.keys.retain(|k| k.kid != r.new_key_id);
    s.keys.push(TenantKey {
        kid: r.new_key_id,
        public_key: r.new_key,
        state: "active".to_owned(),
        not_after: None,
    });
    s.rotation_seq = r.seq;
    Ok(())
}

/// Documento `tenant_key_rotation` dalla risposta all'heartbeat.
pub fn receive_rotation(signed: &Signed) {
    let result = with_store(|s| -> ResultType<()> {
        let now = now();
        let (payload, by_root) = match verify_in(s, signed, None, now) {
            Some(payload) => (payload, false),
            None => match verify_root(signed) {
                Some(payload) => (payload, true),
                None => bail!("rotation signature not trusted"),
            },
        };
        apply_rotation(s, serde_json::from_slice(&payload)?, by_root, now)?;
        save(s);
        log::info!(
            "tenant key rotated{}",
            if by_root { " by the Clanto root key" } else { "" }
        );
        Ok(())
    });
    if let Some(Err(err)) = result {
        log::error!("tenant key rotation rejected: {err}");
    }
}

#[derive(Deserialize)]
struct GrantKeys {
    version: u64,
    #[serde(default)]
    keys: Vec<ListedKey>,
    #[serde(default)]
    revocations: Vec<Revocation>,
}

#[derive(Deserialize)]
struct ListedKey {
    kid: String,
    public_key: String,
    #[serde(default)]
    not_after: Option<i64>,
    state: String,
}

fn apply_grant_keys(s: &mut Store, list: GrantKeys, by_root: bool) -> ResultType<()> {
    if list.version <= s.grant_keys_version {
        bail!("grant keys v{} not newer than v{}", list.version, s.grant_keys_version);
    }
    let mut keys = Vec::new();
    for k in list.keys {
        let pk = crate::decode64(&k.public_key)?;
        if sign::PublicKey::from_slice(&pk).is_none() || kid_of(&pk) != k.kid {
            bail!("invalid key {}", k.kid);
        }
        if !matches!(k.state.as_str(), "active" | "retiring" | "revoked") {
            bail!("invalid key state {}", k.state);
        }
        keys.push(TenantKey {
            kid: k.kid,
            public_key: k.public_key,
            state: k.state,
            not_after: k.not_after,
        });
    }
    // Un elenco firmato da una chiave fidata non può togliere l'ultima chiave
    // attiva: il PC resterebbe senza ancora. Solo la chiave radice può farlo.
    if !by_root && !keys.iter().any(|k| k.state == "active") {
        bail!("grant keys without an active key");
    }
    if !keys.is_empty() {
        s.keys = keys;
    }
    s.revocations = list.revocations;
    s.grant_keys_version = list.version;
    Ok(())
}

/// Elenco firmato di `GET /device/grant-keys`.
pub fn receive_grant_keys(signed: &Signed) -> ResultType<()> {
    with_store(|s| -> ResultType<()> {
        let now = now();
        let (payload, by_root) = match verify_in(s, signed, None, now) {
            Some(payload) => (payload, false),
            None => match verify_root(signed) {
                Some(payload) => (payload, true),
                None => bail!("grant keys signature not trusted"),
            },
        };
        apply_grant_keys(s, serde_json::from_slice(&payload)?, by_root)?;
        prune(s, now);
        save(s);
        Ok(())
    })
    .unwrap_or_else(|| bail!("device not enrolled"))
}

pub fn grant_keys_version() -> u64 {
    with_store(|s| s.grant_keys_version).unwrap_or(0)
}

pub fn is_revoked(nonce: &str, session_id: &str, user_id: &str) -> bool {
    with_store(|s| {
        s.revocations.iter().any(|r| {
            r.nonce.as_deref() == Some(nonce)
                || (!session_id.is_empty() && r.session_id.as_deref() == Some(session_id))
                || (!user_id.is_empty() && r.user_id.as_deref() == Some(user_id))
        })
    })
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed(payload: &serde_json::Value, sk: &sign::SecretKey) -> Signed {
        let bytes = serde_json::to_vec(payload).unwrap();
        Signed {
            payload: crate::encode64(&bytes),
            signature: crate::encode64(sign::sign_detached(&bytes, sk).to_bytes()),
        }
    }

    fn store_with(pk: &sign::PublicKey) -> Store {
        Store {
            tenant_id: "t1".to_owned(),
            keys: vec![TenantKey {
                kid: kid_of(&pk.0),
                public_key: crate::encode64(pk.0),
                state: "active".to_owned(),
                not_after: None,
            }],
            ..Default::default()
        }
    }

    fn rotation(pk: &sign::PublicKey, seq: u64) -> Rotation {
        Rotation {
            kind: "tenant_key_rotation".to_owned(),
            tenant_id: "t1".to_owned(),
            new_key: crate::encode64(pk.0),
            new_key_id: kid_of(&pk.0),
            valid_from: 0,
            seq,
        }
    }

    #[test]
    fn ordinary_rotation_keeps_old_key_for_seven_days() {
        let (old_pk, old_sk) = sign::gen_keypair();
        let (new_pk, new_sk) = sign::gen_keypair();
        let mut s = store_with(&old_pk);
        apply_rotation(&mut s, rotation(&new_pk, 1), false, 1000).unwrap();
        let doc = signed(&serde_json::json!({"x": 1}), &old_sk);
        assert!(verify_in(&s, &doc, None, 1000 + RETIRING_SECS - 1).is_some());
        assert!(verify_in(&s, &doc, None, 1000 + RETIRING_SECS).is_none());
        let doc = signed(&serde_json::json!({"x": 2}), &new_sk);
        assert!(verify_in(&s, &doc, Some(&kid_of(&new_pk.0)), 1000).is_some());
        assert!(apply_rotation(&mut s, rotation(&new_pk, 1), false, 1000).is_err());
    }

    #[test]
    fn root_rotation_revokes_previous_keys() {
        let (old_pk, old_sk) = sign::gen_keypair();
        let (new_pk, _) = sign::gen_keypair();
        let mut s = store_with(&old_pk);
        apply_rotation(&mut s, rotation(&new_pk, 1), true, 1000).unwrap();
        let doc = signed(&serde_json::json!({"x": 1}), &old_sk);
        assert!(verify_in(&s, &doc, None, 1000).is_none());
    }

    #[test]
    fn rotation_checks_tenant_and_kid() {
        let (old_pk, _) = sign::gen_keypair();
        let (new_pk, _) = sign::gen_keypair();
        let mut s = store_with(&old_pk);
        let mut r = rotation(&new_pk, 1);
        r.tenant_id = "other".to_owned();
        assert!(apply_rotation(&mut s, r, false, 0).is_err());
        let mut r = rotation(&new_pk, 1);
        r.new_key_id = "AAAAAAAAAAA=".to_owned();
        assert!(apply_rotation(&mut s, r, false, 0).is_err());
    }

    #[test]
    fn grant_keys_need_an_active_key_and_a_newer_version() {
        let (pk, _) = sign::gen_keypair();
        let mut s = store_with(&pk);
        let revoked_only = GrantKeys {
            version: 1,
            keys: vec![ListedKey {
                kid: kid_of(&pk.0),
                public_key: crate::encode64(pk.0),
                not_after: None,
                state: "revoked".to_owned(),
            }],
            revocations: vec![],
        };
        assert!(apply_grant_keys(&mut s, revoked_only, false).is_err());
        let ok = GrantKeys {
            version: 2,
            keys: vec![ListedKey {
                kid: kid_of(&pk.0),
                public_key: crate::encode64(pk.0),
                not_after: None,
                state: "active".to_owned(),
            }],
            revocations: vec![Revocation {
                nonce: Some("n1".to_owned()),
                session_id: None,
                user_id: None,
                until: i64::MAX,
            }],
        };
        apply_grant_keys(&mut s, ok, false).unwrap();
        assert_eq!(s.grant_keys_version, 2);
        let stale = GrantKeys {
            version: 2,
            keys: vec![],
            revocations: vec![],
        };
        assert!(apply_grant_keys(&mut s, stale, false).is_err());
    }
}
