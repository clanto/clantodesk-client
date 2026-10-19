// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Branding e configurazione obbligatoria dell'infrastruttura ClantoDesk.

use super::{
    outgoing::{self, OPT_CLIPBOARD, OPT_FILE_TRANSFER},
    posture::{OPT_POSTURE_BYPASS, OPT_REQUIRE_PROTECTED_DEVICE},
    session_log::OPT_SESSION_LOG_DAYS,
};
use base::config::keys;
use hbb_common::{
    config::{self, Config},
    log,
};

/// WebRTC resta spento finche' `ice-servers` non punta a STUN/TURN Clanto.
pub fn webrtc_allowed() -> bool {
    false
}

/// Nessun probe verso STUN pubblici: manderebbe l'IP del dispositivo a terzi.
/// Il probe IPv6 tornerà con uno STUN Clanto.
pub fn public_stun_allowed() -> bool {
    false
}

/// Predefiniti dei dispositivi nuovi (chiavi di `KEYS_DISPLAY_SETTINGS`).
/// Finiscono in `DEFAULT_DISPLAY_SETTINGS`, non in OVERWRITE: la preferenza
/// salvata dall'utente in `_default` vince, i peer gia' salvati non cambiano.
const NEW_PEER_DEFAULTS: [(&str, &str); 5] = [
    (keys::OPTION_VIEW_STYLE, "adaptive"),
    (keys::OPTION_DISABLE_CLIPBOARD, "Y"),
    (keys::OPTION_ENABLE_FILE_COPY_PASTE, "N"),
    (keys::OPTION_SHOW_REMOTE_CURSOR, "Y"),
    (keys::OPTION_DISABLE_AUDIO, "Y"),
];

fn apply_new_peer_defaults() {
    let mut defaults = config::DEFAULT_DISPLAY_SETTINGS.write().unwrap();
    for (k, v) in NEW_PEER_DEFAULTS {
        defaults.insert(k.to_owned(), v.to_owned());
    }
}

/// Predefiniti lato PC controllato (chiavi di `KEYS_SETTINGS`), in
/// `DEFAULT_SETTINGS`: un valore salvato dall'utente vince. Il timeout e' in minuti.
const HOST_DEFAULTS: [(&str, &str); 9] = [
    (keys::OPTION_ALLOW_AUTO_DISCONNECT, "Y"),
    (keys::OPTION_AUTO_DISCONNECT_TIMEOUT, "10"),
    (keys::OPTION_ENABLE_LAN_DISCOVERY, "N"),
    (keys::OPTION_ENABLE_TUNNEL, "N"),
    (keys::OPTION_ENABLE_REMOTE_RESTART, "N"),
    (keys::OPTION_ENABLE_TERMINAL, "N"),
    (OPT_REQUIRE_PROTECTED_DEVICE, "N"),
    (OPT_POSTURE_BYPASS, ""),
    (OPT_SESSION_LOG_DAYS, "180"),
];

fn apply_host_defaults() {
    let mut defaults = config::DEFAULT_SETTINGS.write().unwrap();
    for (k, v) in HOST_DEFAULTS {
        defaults.insert(k.to_owned(), v.to_owned());
    }
}

/// Applica branding e valori server iniettati al build prima che `Config`
/// venga usata per calcolare percorsi o impostazioni predefinite.
pub fn apply_build_config() {
    *config::APP_NAME.write().unwrap() = "ClantoDesk".to_owned();
    super::keystore::init();
    apply_new_peer_defaults();
    apply_host_defaults();

    // ClantoDesk usa un'infrastruttura gestita: l'account cloud non e' ancora
    // disponibile e i parametri dei server non devono essere modificabili
    // dall'interfaccia. Queste opzioni sono rispettate sia da Flutter mobile
    // sia dal client desktop.
    config::HARD_SETTINGS
        .write()
        .unwrap()
        .insert("disable-account".to_owned(), "Y".to_owned());
    config::BUILTIN_SETTINGS
        .write()
        .unwrap()
        .insert("hide-server-settings".to_owned(), "Y".to_owned());

    // enable-webrtc e' LocalConfig: fissarlo qui lo rende non modificabile dalla UI.
    if !webrtc_allowed() {
        config::OVERWRITE_LOCAL_SETTINGS
            .write()
            .unwrap()
            .insert("enable-webrtc".to_owned(), "N".to_owned());
    }

    // ORG e' dichiarato solo su macOS. Entra sia nel percorso di configurazione
    // sia nei nomi dei plist di servizio generati dai template upstream.
    #[cfg(target_os = "macos")]
    {
        *config::ORG.write().unwrap() = "it.clanto".to_owned();
    }

    if let Some(server) = option_env!("RENDEZVOUS_SERVER").filter(|v| !v.is_empty()) {
        *config::PROD_RENDEZVOUS_SERVER.write().unwrap() = server.to_owned();
        config::OVERWRITE_SETTINGS
            .write()
            .unwrap()
            .insert("custom-rendezvous-server".to_owned(), server.to_owned());
    }
    if let Some(key) = option_env!("RS_PUB_KEY").filter(|v| !v.is_empty()) {
        config::OVERWRITE_SETTINGS
            .write()
            .unwrap()
            .insert("key".to_owned(), key.to_owned());
    }
    if let Some(server) = option_env!("API_SERVER").filter(|v| !v.is_empty()) {
        config::OVERWRITE_SETTINGS
            .write()
            .unwrap()
            .insert("api-server".to_owned(), server.to_owned());
    }
    // Il relay va bloccato anche quando non e' fissato in build: la chiave
    // presente in OVERWRITE_SETTINGS rende impossibile scriverla e cancella un
    // valore gia' salvato. Vuota significa "quello annunciato dal rendezvous".
    config::OVERWRITE_SETTINGS.write().unwrap().insert(
        "relay-server".to_owned(),
        option_env!("RELAY_SERVER").unwrap_or("").to_owned(),
    );
    super::gpo::apply();
    apply_outgoing_restrictions();

    // Ruolo della build, fissato alla compilazione e non modificabile a runtime.
    // "outgoing" toglie il lato controllato: `RendezvousMediator::start_all` non
    // registra il dispositivo, quindi non ha un ID raggiungibile. "incoming"
    // toglie il lato client. Assente = entrambi.
    //
    // INVARIANTE DEL FORK: questa riga deve restare qui. Spostarla in una
    // sostituzione del workflow la rende invisibile a chi riporta le modifiche
    // upstream, e una sostituzione mancata produce in silenzio la variante
    // permissiva. Vedi CLANTO.md.
    if let Some(role) = option_env!("CONN_TYPE").filter(|v| !v.is_empty()) {
        match role {
            "outgoing" | "incoming" => {
                config::HARD_SETTINGS
                    .write()
                    .unwrap()
                    .insert("conn-type".to_owned(), role.to_owned());
                log::info!("build role: conn-type={}", role);
            }
            other => {
                // Un valore non riconosciuto non deve degradare in "tutto
                // permesso": e' un errore di build, non una preferenza.
                panic!(
                    "CONN_TYPE non valido: '{}'. Valori ammessi: outgoing, incoming.",
                    other
                );
            }
        }
    }
}

fn apply_outgoing_restrictions() {
    let mut overwrite = config::OVERWRITE_DISPLAY_SETTINGS.write().unwrap();
    if !outgoing::allowed(OPT_CLIPBOARD) {
        overwrite.insert(keys::OPTION_DISABLE_CLIPBOARD.to_owned(), "Y".to_owned());
    }
    if !outgoing::allowed(OPT_FILE_TRANSFER) {
        overwrite.insert(
            keys::OPTION_ENABLE_FILE_COPY_PASTE.to_owned(),
            "N".to_owned(),
        );
    }
}

/// Vero se rendezvous server e chiave sono fissati al build. In quel caso nome
/// dell'eseguibile e `custom.txt` non possono sostituirli.
pub fn servers_fixed_at_build() -> bool {
    servers_fixed(option_env!("RENDEZVOUS_SERVER"), option_env!("RS_PUB_KEY"))
}

fn servers_fixed(server: Option<&str>, key: Option<&str>) -> bool {
    [server, key]
        .iter()
        .all(|v| v.is_some_and(|v| !v.is_empty()))
}

/// Impedisce a una build ClantoDesk priva di configurazione di ricadere in
/// silenzio sui server e sulla chiave pubblica RustDesk.
///
/// Deve essere chiamata dopo il caricamento del custom client, che puo' fornire
/// a sua volta rendezvous server e chiave.
pub fn ensure_own_server_configured() -> bool {
    match check_own_server_configured() {
        Ok(()) => true,
        Err(e) => {
            log::error!("{}", e);
            eprintln!("{}", e);
            false
        }
    }
}

fn check_own_server_configured() -> Result<(), String> {
    let allow_public = option_env!("CLANTO_ALLOW_PUBLIC_SERVER").unwrap_or("") == "1";
    let mut problems = Vec::new();

    if crate::common::using_public_server() {
        problems.push("nessun rendezvous server configurato (RENDEZVOUS_SERVER)");
    }
    let key = Config::get_option("key");
    if key.is_empty() {
        problems.push("nessuna chiave pubblica configurata (RS_PUB_KEY)");
    } else if key == config::RS_PUB_KEY {
        problems.push("la chiave pubblica configurata e' quella pubblica di RustDesk");
    }
    if problems.is_empty() {
        return Ok(());
    }

    let detail = problems.join("; ");
    if allow_public {
        log::warn!(
            "ATTENZIONE: build di sviluppo, si ricade sull'infrastruttura pubblica RustDesk ({}). \
             Non distribuire questo binario.",
            detail
        );
        return Ok(());
    }

    Err(format!(
        "Avvio interrotto: configurazione server Clanto assente o non valida ({}). \
         Questo binario e' stato compilato senza i secret RENDEZVOUS_SERVER/RS_PUB_KEY \
         e ricadrebbe sui server pubblici RustDesk.",
        detail
    ))
}

#[cfg(test)]
mod tests {
    use super::{apply_host_defaults, apply_new_peer_defaults, servers_fixed};
    use base::config::keys;
    use hbb_common::config::UserDefaultConfig;

    #[test]
    fn fixed_only_when_server_and_key_are_set() {
        assert!(servers_fixed(Some("rs.example.org"), Some("pubkey")));
        assert!(!servers_fixed(None, None));
        assert!(!servers_fixed(Some("rs.example.org"), None));
        assert!(!servers_fixed(None, Some("pubkey")));
    }

    #[test]
    fn empty_values_do_not_count_as_set() {
        assert!(!servers_fixed(Some(""), Some("pubkey")));
        assert!(!servers_fixed(Some("rs.example.org"), Some("")));
        assert!(!servers_fixed(Some(""), Some("")));
    }

    #[test]
    fn new_peer_defaults_apply_when_user_has_no_preference() {
        apply_new_peer_defaults();
        let cfg = UserDefaultConfig::default();
        assert_eq!(cfg.get(keys::OPTION_VIEW_STYLE), "adaptive");
        assert_eq!(cfg.get(keys::OPTION_DISABLE_CLIPBOARD), "Y");
        assert_eq!(cfg.get(keys::OPTION_ENABLE_FILE_COPY_PASTE), "N");
        assert_eq!(cfg.get(keys::OPTION_SHOW_REMOTE_CURSOR), "Y");
        assert_eq!(cfg.get(keys::OPTION_DISABLE_AUDIO), "Y");
    }

    #[test]
    fn new_peer_defaults_are_not_locked() {
        apply_new_peer_defaults();
        let overwrite = hbb_common::config::OVERWRITE_DISPLAY_SETTINGS.read().unwrap();
        assert!(super::NEW_PEER_DEFAULTS
            .iter()
            .all(|(k, _)| !overwrite.contains_key(*k)));
    }

    #[test]
    fn host_defaults_are_defaults_not_overwrites() {
        apply_host_defaults();
        let defaults = hbb_common::config::DEFAULT_SETTINGS.read().unwrap();
        assert_eq!(defaults.get(keys::OPTION_AUTO_DISCONNECT_TIMEOUT).map(String::as_str), Some("10"));
        assert_eq!(defaults.get(keys::OPTION_ENABLE_TUNNEL).map(String::as_str), Some("N"));
        let overwrite = hbb_common::config::OVERWRITE_SETTINGS.read().unwrap();
        assert!(super::HOST_DEFAULTS.iter().all(|(k, _)| !overwrite.contains_key(*k)));
    }
}
