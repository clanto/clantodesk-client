// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Limiti del canale heartbeat/sysinfo verso `api-server` (src/hbbs_http/sync.rs).
//!
//! - In uscita: hostname e nome utente raccolti dal sistema sono dati personali e
//!   non partono, salvo `clanto-share-device-identity=Y` sull'host. Le etichette
//!   impostate dall'amministratore (`preset-device-name`, `preset-device-username`)
//!   restano: sono una scelta esplicita.
//! - In entrata: la `strategy` del server scrive opzioni sull'host. Il server non
//!   puo' mai toccare le chiavi che proteggono l'accesso, e con il blocco delle
//!   impostazioni (GPO) non puo' cambiare nulla.

use hbb_common::{config::Config, log};
use serde_json::Value;
use std::collections::HashMap;

pub const OPT_SHARE_DEVICE_IDENTITY: &str = "clanto-share-device-identity";
const IDENTITY_FIELDS: &[&str] = &["hostname", "username"];

/// Chiavi che una `strategy` remota non puo' impostare: un server compromesso
/// non deve poter spegnere 2FA, allowlist, postura o filigrane degli host.
pub const SERVER_PROTECTED_KEYS: &[&str] = &[
    "2fa",
    "verification-method",
    "approve-mode",
    "temporary-password-length",
    "allow-only-conn-window-open",
    "enable-trusted-devices",
    "id-whitelist",
    "whitelist",
    "clanto-id-permissions",
    "clanto-trusted-controllers",
    "clanto-require-2fa",
    "clanto-require-protected-device",
    "clanto-require-controller-watermark",
    "clanto-posture-bypass",
    "clanto-host-watermark",
    "clanto-lock-settings",
    "clanto-share-device-identity",
    "clanto-session-log-days",
    "clanto-file-scan",
    "clanto-file-direction",
    "clanto-file-blocked-ext",
    "clanto-forward-enabled",
    "clanto-forward-catalog",
    "clanto-forward-custom-host",
    "clanto-forward-custom-host-targets",
    "clanto-forward-reverse",
    "clanto-forward-custom-controller",
    "clanto-forward-custom-controller-targets",
];

pub fn filter_sysinfo(v: &mut Value) {
    if Config::get_option(OPT_SHARE_DEVICE_IDENTITY) == "Y" {
        return;
    }
    if let Some(map) = v.as_object_mut() {
        for field in IDENTITY_FIELDS {
            map.remove(*field);
        }
    }
}

pub fn filter_strategy(options: &mut HashMap<String, String>) {
    if !super::settings_lock::user_write_allowed_now() {
        if !options.is_empty() {
            log::warn!("Server strategy ignored: settings are managed by policy");
        }
        options.clear();
        return;
    }
    options.retain(|key, _| {
        let allowed = !SERVER_PROTECTED_KEYS.contains(&key.as_str());
        if !allowed {
            log::warn!("Server strategy cannot set protected option {key}");
        }
        allowed
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identity_is_removed_by_default() {
        let mut v = json!({"hostname": "pc-01", "username": "mario", "os": "Linux"});
        filter_sysinfo(&mut v);
        assert_eq!(v, json!({"os": "Linux"}));
    }

    #[test]
    fn protected_keys_never_come_from_the_server() {
        let mut options: HashMap<String, String> = [
            ("2fa", ""),
            ("id-whitelist", ""),
            ("clanto-trusted-controllers", "1=x"),
            ("enable-clipboard", "N"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        filter_strategy(&mut options);
        assert_eq!(options.len(), 1);
        assert_eq!(options.get("enable-clipboard").map(String::as_str), Some("N"));
    }
}
