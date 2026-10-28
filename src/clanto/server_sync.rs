// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Limiti del canale heartbeat/sysinfo verso `api-server` (src/hbbs_http/sync.rs).
//!
//! - In uscita: hostname e nome utente raccolti dal sistema sono dati personali e
//!   non partono, salvo `clanto-share-device-identity=Y` sull'host. Le etichette
//!   impostate dall'amministratore (`preset-device-name`, `preset-device-username`)
//!   restano: sono una scelta esplicita.
//! - In entrata: la `strategy` del server scrive opzioni sull'host. Il server non
//!   può mai toccare le chiavi che proteggono l'accesso, e con il blocco delle
//!   impostazioni (GPO) può cambiare solo le chiavi gestite (`SERVER_MANAGED_KEYS`).
//!   Una chiave impostata anche dalla GPO resta comunque quella della GPO.

use hbb_common::{config::Config, log};
use serde_json::Value;
use std::collections::HashMap;

pub const OPT_SHARE_DEVICE_IDENTITY: &str = "clanto-share-device-identity";
const IDENTITY_FIELDS: &[&str] = &["hostname", "username"];

/// Chiavi che una `strategy` remota non può impostare: un server compromesso
/// non deve poter spegnere 2FA, allowlist, postura o filigrane degli host.
pub const SERVER_PROTECTED_KEYS: &[&str] = &[
    "2fa",
    "bot",
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
    "clanto-access-hours",
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
    "clanto-forward-custom-host",
    "clanto-forward-custom-host-targets",
    "clanto-forward-reverse",
    "clanto-forward-custom-controller",
    "clanto-forward-custom-controller-targets",
    "clanto-forward-webcam",
    "clanto-forward-webcam-send",
    "clanto-forward-webcam-receive",
    "clanto-forward-camera-device",
    "clanto-enroll-token",
    "clanto-unenroll",
];

/// Chiavi che la `strategy` può impostare sempre, anche con il blocco delle
/// impostazioni: i servizi del forwarding si gestiscono da GPO e da API. Da
/// soli non aprono nulla: serve `clanto-forward-enabled`, che resta protetta.
pub const SERVER_MANAGED_KEYS: &[&str] = &[
    "clanto-forward-services-host",
    "clanto-forward-services-controller",
];

fn strategy_may_set(key: &str, locked: bool) -> bool {
    if SERVER_MANAGED_KEYS.contains(&key) {
        return true;
    }
    !locked && !SERVER_PROTECTED_KEYS.contains(&key)
}

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
    let locked = !super::settings_lock::user_write_allowed_now();
    options.retain(|key, _| {
        let allowed = strategy_may_set(key, locked);
        if !allowed {
            log::warn!("Server strategy cannot set option {key}");
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

    #[test]
    fn forward_services_are_always_managed_by_the_server() {
        for locked in [false, true] {
            assert!(strategy_may_set("clanto-forward-services-host", locked));
            assert!(strategy_may_set("clanto-forward-services-controller", locked));
            assert!(!strategy_may_set("clanto-forward-enabled", locked));
        }
        assert!(strategy_may_set("enable-clipboard", false));
        assert!(!strategy_may_set("enable-clipboard", true));
    }
}
