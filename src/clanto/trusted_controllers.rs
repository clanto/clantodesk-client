// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Controller fidati: esenzione dal 2FA decisa dall'amministratore dell'host.
//!
//! Ogni utente di un controller ha una chiave Ed25519 propria, generata al primo
//! uso e salvata cifrata con la chiave utente del keystore. A ogni login firma
//! ID dell'host, challenge della connessione, ID del controller e chiave
//! pubblica. L'host salta il 2FA solo se la coppia ID/chiave e' nell'elenco
//! `clanto-trusted-controllers` e la firma e' valida: l'ID da solo, dichiarato
//! dal controller, non basta.

use hbb_common::{
    config::{Config, LocalConfig},
    log,
    password_security::{
        decrypt_str_or_original_scoped, encrypt_str_or_original_scoped, is_scoped_encrypted,
        KeyScope,
    },
    sodiumoxide::{
        base64::{self, Variant},
        crypto::sign,
    },
};
use std::sync::Mutex;

pub const OPT_TRUSTED_CONTROLLERS: &str = "clanto-trusted-controllers";
const LOCAL_OPT_DEVICE_KEY: &str = "clanto-device-key";
const SIGNATURE_DOMAIN: &str = "clanto-controller-v1";
const MAX_ENTRIES: usize = 500;

static DEVICE_KEY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub key: Vec<u8>,
}

impl Entry {
    fn to_option(&self) -> String {
        format!("{}={}", self.id, encode_key(&self.key))
    }
}

pub fn encode_key(key: &[u8]) -> String {
    base64::encode(key, Variant::UrlSafeNoPadding)
}

/// Chiave pubblica nel formato dell'elenco: 43 caratteri base64url, 32 byte.
pub fn decode_key(s: &str) -> Option<Vec<u8>> {
    let key = base64::decode(s.trim(), Variant::UrlSafeNoPadding).ok()?;
    (key.len() == sign::PUBLICKEYBYTES).then_some(key)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Voci `ID=CHIAVE` separate da virgole, punti e virgola o spazi. Le voci non
/// valide sono restituite a parte e non danno mai fiducia.
pub fn parse(list: &str) -> (Vec<Entry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut invalid = Vec::new();
    for item in list
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|s| !s.is_empty())
    {
        let parsed = item.split_once('=').and_then(|(id, key)| {
            let id = id.trim();
            valid_id(id)
                .then(|| decode_key(key))
                .flatten()
                .map(|key| Entry {
                    id: id.to_owned(),
                    key,
                })
        });
        match parsed {
            Some(entry) if entries.len() < MAX_ENTRIES => {
                if !entries.contains(&entry) {
                    entries.push(entry);
                }
            }
            _ => invalid.push(item.to_owned()),
        }
    }
    (entries, invalid)
}

pub fn format(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(Entry::to_option)
        .collect::<Vec<_>>()
        .join(",")
}

/// Aggiunge una voce all'elenco; la stessa coppia ID/chiave non viene duplicata.
pub fn add(list: &str, id: &str, key: &str) -> Result<String, String> {
    let id = id.trim();
    if !valid_id(id) {
        return Err(format!("Invalid controller ID: {id}"));
    }
    let key = decode_key(key).ok_or_else(|| "Invalid device key".to_owned())?;
    let (mut entries, _) = parse(list);
    let entry = Entry {
        id: id.to_owned(),
        key,
    };
    if !entries.contains(&entry) {
        if entries.len() >= MAX_ENTRIES {
            return Err(format!("At most {MAX_ENTRIES} trusted controllers"));
        }
        entries.push(entry);
    }
    Ok(format(&entries))
}

/// Toglie tutte le chiavi di un ID, oppure solo quella indicata.
pub fn remove(list: &str, id: &str, key: Option<&str>) -> String {
    let key = key.and_then(decode_key);
    let (mut entries, _) = parse(list);
    entries.retain(|e| e.id != id.trim() || key.as_ref().map_or(false, |k| &e.key != k));
    format(&entries)
}

fn canonical_message(host_id: &str, challenge: &str, controller_id: &str, key: &[u8]) -> Vec<u8> {
    format!(
        "{SIGNATURE_DOMAIN}\n{host_id}\n{challenge}\n{controller_id}\n{}\n",
        encode_key(key)
    )
    .into_bytes()
}

fn store_seed(seed: &sign::Seed) -> bool {
    let plain = base64::encode(&seed.0, Variant::Original);
    let stored = encrypt_str_or_original_scoped(&plain, KeyScope::User, 128);
    // Senza chiave utente il valore tornerebbe in chiaro: meglio nessuna chiave.
    if !is_scoped_encrypted(stored.as_bytes()) {
        log::warn!("Controller device key not stored: user key unavailable");
        return false;
    }
    LocalConfig::set_option(LOCAL_OPT_DEVICE_KEY.to_owned(), stored);
    true
}

fn load_keypair() -> Option<(sign::PublicKey, sign::SecretKey)> {
    let stored = LocalConfig::get_option(LOCAL_OPT_DEVICE_KEY);
    if stored.is_empty() {
        return None;
    }
    let (plain, success, _) = decrypt_str_or_original_scoped(&stored, KeyScope::User);
    if !success || !is_scoped_encrypted(stored.as_bytes()) {
        return None;
    }
    let seed = base64::decode(plain, Variant::Original).ok()?;
    let seed = sign::Seed::from_slice(&seed)?;
    Some(sign::keypair_from_seed(&seed))
}

/// Chiave del controller, generata al primo uso. `None` se la chiave utente
/// del keystore non e' disponibile.
fn device_keypair() -> Option<(sign::PublicKey, sign::SecretKey)> {
    let _guard = DEVICE_KEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pair) = load_keypair() {
        return Some(pair);
    }
    if !LocalConfig::get_option(LOCAL_OPT_DEVICE_KEY).is_empty() {
        // Esiste ma non si decifra (chiave utente bloccata): non sovrascriverla.
        return None;
    }
    let seed = sign::Seed::from_slice(&hbb_common::rand::random::<[u8; 32]>())?;
    if !store_seed(&seed) {
        return None;
    }
    load_keypair()
}

/// Chiave pubblica da comunicare all'amministratore dell'host.
pub fn device_public_key() -> Option<String> {
    device_keypair().map(|(pk, _)| encode_key(&pk.0))
}

/// Firma del login lato controller: `(chiave pubblica, firma)`.
pub fn sign_login(host_id: &str, challenge: &str, controller_id: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    if host_id.is_empty() || challenge.is_empty() || controller_id.is_empty() {
        return None;
    }
    let (pk, sk) = device_keypair()?;
    let message = canonical_message(host_id, challenge, controller_id, &pk.0);
    let signature = sign::sign_detached(&message, &sk);
    Some((pk.0.to_vec(), signature.to_bytes().to_vec()))
}

fn verify_with_list(
    list: &str,
    host_id: &str,
    challenge: &str,
    controller_id: &str,
    key: &[u8],
    signature: &[u8],
) -> bool {
    if host_id.is_empty() || challenge.is_empty() || controller_id.is_empty() {
        return false;
    }
    let (entries, invalid) = parse(list);
    if !invalid.is_empty() {
        log::warn!("{} invalid entries ignored in {OPT_TRUSTED_CONTROLLERS}", invalid.len());
    }
    if !entries.iter().any(|e| e.id == controller_id && e.key == key) {
        return false;
    }
    let (Some(pk), Ok(signature)) = (
        sign::PublicKey::from_slice(key),
        sign::Signature::from_bytes(signature),
    ) else {
        return false;
    };
    sign::verify_detached(
        &signature,
        &canonical_message(host_id, challenge, controller_id, key),
        &pk,
    )
}

/// Lato host: vero solo se il controller prova di possedere una chiave
/// registrata per il suo ID, su questo host e su questa connessione.
pub fn verify_login(challenge: &str, controller_id: &str, key: &[u8], signature: &[u8]) -> bool {
    let list = Config::get_option(OPT_TRUSTED_CONTROLLERS);
    if list.trim().is_empty() || key.is_empty() || signature.is_empty() {
        return false;
    }
    verify_with_list(
        &list,
        &Config::get_id(),
        challenge,
        controller_id,
        key,
        signature,
    )
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
const CLI_COMMANDS: &[&str] = &["--device-key", "--trust-controller", "--untrust-controller"];

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn is_cli_command(arg: &str) -> bool {
    CLI_COMMANDS.contains(&arg)
}

/// `--device-key` stampa la chiave dell'utente corrente e non richiede privilegi.
/// `--trust-controller <ID> <CHIAVE>` e `--untrust-controller <ID> [CHIAVE]`
/// modificano l'elenco dell'host: installazione e privilegi di amministratore.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn run_cli(args: &[String], settings_disabled: bool, admin: bool) {
    use super::settings_lock::{user_option_write_allowed, MANAGED_SETTINGS_MESSAGE};

    match args[0].as_str() {
        "--device-key" => match device_public_key() {
            Some(key) => println!("{key}"),
            None => println!("Device key unavailable: the user key store is locked"),
        },
        command => {
            let usage = if command == "--trust-controller" {
                "Usage: --trust-controller <ID> <DEVICE-KEY>"
            } else {
                "Usage: --untrust-controller <ID> [DEVICE-KEY]"
            };
            let arity_ok = match command {
                "--trust-controller" => args.len() == 3,
                _ => args.len() == 2 || args.len() == 3,
            };
            if !arity_ok {
                println!("{usage}");
                return;
            }
            if !user_option_write_allowed(OPT_TRUSTED_CONTROLLERS)
                || hbb_common::config::OVERWRITE_SETTINGS
                    .read()
                    .unwrap()
                    .contains_key(OPT_TRUSTED_CONTROLLERS)
            {
                println!("{MANAGED_SETTINGS_MESSAGE}");
                return;
            }
            if settings_disabled {
                println!("Settings are disabled!");
                return;
            }
            if !admin {
                println!("Installation and administrative privileges required!");
                return;
            }
            let current = crate::ipc::get_options()
                .get(OPT_TRUSTED_CONTROLLERS)
                .cloned()
                .unwrap_or_default();
            let updated = if command == "--trust-controller" {
                match add(&current, &args[1], &args[2]) {
                    Ok(list) => list,
                    Err(err) => {
                        println!("{err}");
                        return;
                    }
                }
            } else {
                remove(&current, &args[1], args.get(2).map(String::as_str))
            };
            crate::ipc::set_option(OPT_TRUSTED_CONTROLLERS, &updated);
            let stored = crate::ipc::get_options()
                .get(OPT_TRUSTED_CONTROLLERS)
                .cloned()
                .unwrap_or_default();
            if stored == updated {
                println!("Done!");
            } else {
                println!("Failed to update {OPT_TRUSTED_CONTROLLERS}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (sign::PublicKey, sign::SecretKey) {
        sign::gen_keypair()
    }

    fn signed(sk: &sign::SecretKey, pk: &sign::PublicKey, host: &str, ch: &str, id: &str) -> Vec<u8> {
        sign::sign_detached(&canonical_message(host, ch, id, &pk.0), sk)
            .to_bytes()
            .to_vec()
    }

    #[test]
    fn valid_signature_from_listed_key_is_trusted() {
        let (pk, sk) = pair();
        let list = format!("111222333={}", encode_key(&pk.0));
        let sig = signed(&sk, &pk, "host1", "abc123", "111222333");
        assert!(verify_with_list(&list, "host1", "abc123", "111222333", &pk.0, &sig));
    }

    #[test]
    fn declared_id_without_the_key_is_not_trusted() {
        let (pk, _) = pair();
        let (other_pk, other_sk) = pair();
        let list = format!("111222333={}", encode_key(&pk.0));
        // Stesso ID, chiave diversa: e' il caso del client modificato.
        let sig = signed(&other_sk, &other_pk, "host1", "abc123", "111222333");
        assert!(!verify_with_list(&list, "host1", "abc123", "111222333", &other_pk.0, &sig));
        // Chiave pubblica giusta ma firma con un'altra chiave privata.
        assert!(!verify_with_list(&list, "host1", "abc123", "111222333", &pk.0, &sig));
    }

    #[test]
    fn signature_is_bound_to_host_challenge_and_id() {
        let (pk, sk) = pair();
        let list = format!("111222333={}", encode_key(&pk.0));
        let sig = signed(&sk, &pk, "host1", "abc123", "111222333");
        assert!(!verify_with_list(&list, "host2", "abc123", "111222333", &pk.0, &sig));
        assert!(!verify_with_list(&list, "host1", "zzz999", "111222333", &pk.0, &sig));
        let list2 = format!("{list},444555666={}", encode_key(&pk.0));
        assert!(!verify_with_list(&list2, "host1", "abc123", "444555666", &pk.0, &sig));
    }

    #[test]
    fn key_listed_for_another_id_is_not_trusted() {
        let (pk, sk) = pair();
        let list = format!("999888777={}", encode_key(&pk.0));
        let sig = signed(&sk, &pk, "host1", "abc123", "111222333");
        assert!(!verify_with_list(&list, "host1", "abc123", "111222333", &pk.0, &sig));
    }

    #[test]
    fn parse_add_remove() {
        let (pk1, _) = pair();
        let (pk2, _) = pair();
        let (k1, k2) = (encode_key(&pk1.0), encode_key(&pk2.0));
        let list = add("", "111", &k1).unwrap();
        let list = add(&list, "111", &k1).unwrap();
        let list = add(&list, "111", &k2).unwrap();
        let list = add(&list, "222", &k1).unwrap();
        assert_eq!(parse(&list).0.len(), 3);
        assert_eq!(parse(&remove(&list, "111", Some(&k2))).0.len(), 2);
        assert_eq!(remove(&list, "111", None), format!("222={k1}"));
        assert!(add("", "1 1", &k1).is_err());
        assert!(add("", "111", "not-a-key").is_err());
        let (entries, invalid) = parse(&format!("111={k1}; bad, 222=short\n333={k2}"));
        assert_eq!(entries.len(), 2);
        assert_eq!(invalid, vec!["bad".to_owned(), "222=short".to_owned()]);
    }
}
