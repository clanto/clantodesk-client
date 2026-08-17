// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Branding e configurazione obbligatoria dell'infrastruttura ClantoDesk.

use hbb_common::{
    config::{self, Config},
    log,
};

/// Applica branding e valori server iniettati al build prima che `Config`
/// venga usata per calcolare percorsi o impostazioni predefinite.
pub fn apply_build_config() {
    *config::APP_NAME.write().unwrap() = "ClantoDesk".to_owned();

    // ORG e' dichiarato solo su macOS. Entra sia nel percorso di configurazione
    // sia nei nomi dei plist di servizio generati dai template upstream.
    #[cfg(target_os = "macos")]
    {
        *config::ORG.write().unwrap() = "it.clanto".to_owned();
    }

    if let Some(server) = option_env!("RENDEZVOUS_SERVER").filter(|v| !v.is_empty()) {
        *config::PROD_RENDEZVOUS_SERVER.write().unwrap() = server.to_owned();
        config::DEFAULT_SETTINGS
            .write()
            .unwrap()
            .insert("custom-rendezvous-server".to_owned(), server.to_owned());
    }
    if let Some(key) = option_env!("RS_PUB_KEY").filter(|v| !v.is_empty()) {
        config::DEFAULT_SETTINGS
            .write()
            .unwrap()
            .insert("key".to_owned(), key.to_owned());
    }
    if let Some(server) = option_env!("API_SERVER").filter(|v| !v.is_empty()) {
        config::DEFAULT_SETTINGS
            .write()
            .unwrap()
            .insert("api-server".to_owned(), server.to_owned());
    }
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
