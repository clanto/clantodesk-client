// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Windows: aggiornamento tramite il servizio (CLANTO.md, «Aggiornamenti»).
//!
//! Il pulsante «Aggiorna» chiede al processo `--server` (SYSTEM) di scaricare,
//! verificare la firma Clanto e installare la nuova versione, come fa già
//! l'aggiornamento automatico: nessuna richiesta di amministratore, anche per
//! un utente standard. Si installa solo una versione più recente pubblicata
//! nelle release Clanto (`do_check_software_update`), scaricata in una cartella
//! del servizio che gli utenti non possono scrivere. Se il servizio non c'è
//! (copia portable, servizio fermo) la UI usa il percorso con UAC.

use std::{path::PathBuf, sync::Mutex};

pub const IPC_UPDATE: &str = "clanto-update-now";
pub const IPC_STATE: &str = "clanto-update-state";

pub const CHECKING: &str = "checking";
pub const NONE: &str = "none";
pub const DOWNLOADING: &str = "downloading";
pub const INSTALLING: &str = "installing";
pub const ERROR_PREFIX: &str = "error:";

static STATE: Mutex<String> = Mutex::new(String::new());

pub fn set_state(state: &str) {
    if let Ok(mut s) = STATE.lock() {
        *s = state.to_owned();
    }
}

pub fn set_error(err: impl std::fmt::Display) {
    set_state(&format!("{ERROR_PREFIX}{err}"));
}

/// Servizio: stato per la UI.
pub fn on_ipc_get(name: &str) -> Option<String> {
    (name == IPC_STATE).then(|| STATE.lock().map(|s| s.clone()).unwrap_or_default())
}

/// Servizio: richiesta di aggiornamento dalla UI. `None` se il nome non è nostro.
pub fn on_ipc_set(name: &str, _value: &str) -> Option<bool> {
    if name != IPC_UPDATE {
        return None;
    }
    #[cfg(windows)]
    {
        if !(crate::platform::is_installed() && crate::platform::is_root()) {
            return Some(false);
        }
        set_state(CHECKING);
        let ok = crate::updater::manually_check_update().is_ok();
        if !ok {
            set_error("updater not running");
        }
        Some(ok)
    }
    #[cfg(not(windows))]
    Some(false)
}

/// UI: chiede l'aggiornamento al servizio. `false`: servizio non disponibile.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn request() -> bool {
    use crate::ipc::Data;
    let Ok(runtime) = hbb_common::tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    let reply: hbb_common::ResultType<bool> = runtime.block_on(async {
        let mut c = crate::ipc::connect(1000, "").await?;
        c.send(&Data::Config((IPC_UPDATE.to_owned(), Some("Y".to_owned())))).await?;
        if let Some(Data::Config((name, Some(v)))) = c.next_timeout(2000).await? {
            return Ok(name == IPC_UPDATE && v == "Y");
        }
        Ok(false)
    });
    reply.unwrap_or(false)
}

/// UI: stato dell'aggiornamento nel servizio (vuoto se non raggiungibile).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn state() -> String {
    crate::ipc::get_config(IPC_STATE).ok().flatten().unwrap_or_default()
}

/// Servizio Windows (SYSTEM): il file scaricato va nella cartella di
/// configurazione del servizio, non scrivibile dagli utenti, così nessuno può
/// preparare o sostituire il file tra la verifica della firma e l'avvio.
/// Le versioni scaricate in precedenza si cancellano.
pub fn download_path(default: PathBuf) -> PathBuf {
    #[cfg(windows)]
    if crate::platform::is_root() {
        let dir = hbb_common::config::Config::path("update");
        if let (Some(name), Ok(())) = (default.file_name(), std::fs::create_dir_all(&dir)) {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten().filter(|e| e.file_name() != name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
            return dir.join(name);
        }
    }
    default
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_is_exposed_only_under_its_name() {
        set_state(DOWNLOADING);
        assert_eq!(on_ipc_get(IPC_STATE).as_deref(), Some(DOWNLOADING));
        assert_eq!(on_ipc_get("other"), None);
        set_error("boom");
        assert_eq!(on_ipc_get(IPC_STATE).as_deref(), Some("error:boom"));
        assert_eq!(on_ipc_set("other", "Y"), None);
    }
}
