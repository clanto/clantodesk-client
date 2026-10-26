// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Configurazione gestita da MDM (Intune, Workspace ONE, Jamf...): le stesse
//! chiavi e lo stesso effetto della GPO di Windows (`gpo::GPO_KEYS`), su
//! Android (managed configurations, `res/xml/app_restrictions.xml`, lette da
//! `ClantoManagedConfig.kt`) e iOS (managed app configuration,
//! `com.apple.configuration.managed`, letta in `AppDelegate.swift`).
//! I valori finiscono in `OVERWRITE_SETTINGS`: valgono subito e l'interfaccia
//! li mostra come fissati. Una chiave tolta dall'MDM torna modificabile.

use super::gpo::GPO_KEYS;
use hbb_common::{config, log};
use serde_json::Value;
use std::sync::Mutex;

/// Chiavi applicate dall'ultima configurazione, per toglierle se spariscono.
static APPLIED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Bool(b) => Some(if *b { "Y" } else { "N" }.to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Coppie valide della configurazione: solo chiavi della GPO, valori non vuoti.
fn managed_pairs(json: &str) -> Vec<(String, String)> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(json) else {
        log::warn!("managed configuration is not a JSON object");
        return Vec::new();
    };
    map.iter()
        .filter(|(key, _)| GPO_KEYS.contains(&key.as_str()))
        .filter_map(|(key, value)| Some((key.clone(), value_string(value)?)))
        .collect()
}

pub fn apply_json(json: &str) {
    let pairs = managed_pairs(json);
    let mut overwrite = config::OVERWRITE_SETTINGS.write().unwrap();
    let mut applied = APPLIED.lock().unwrap_or_else(|e| e.into_inner());
    for key in applied.drain(..) {
        overwrite.remove(&key);
    }
    for (key, value) in pairs {
        overwrite.insert(key.clone(), value);
        applied.push(key);
    }
    log::info!("managed configuration applied: {} keys", applied.len());
}

/// `ClantoManagedConfig.apply` (Kotlin): JSON delle restrizioni dell'app.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_it_clanto_clantodesk_clanto_ClantoManagedConfig_apply(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    json: jni::objects::JString,
) {
    if let Ok(json) = env.get_string(&json) {
        apply_json(&String::from(json));
    }
}

/// `AppDelegate.swift`: JSON di `com.apple.configuration.managed`.
///
/// # Safety
/// `json` è una stringa C terminata da zero, valida per la durata della chiamata.
#[cfg(target_os = "ios")]
#[no_mangle]
pub unsafe extern "C" fn clanto_apply_managed_config(json: *const std::os::raw::c_char) {
    if json.is_null() {
        return;
    }
    if let Ok(json) = std::ffi::CStr::from_ptr(json).to_str() {
        apply_json(json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_gpo_keys_with_values_are_managed() {
        let pairs = managed_pairs(
            r#"{"clanto-require-2fa": true, "enable-clipboard": "N", "clanto-access-hours": " ",
                "custom-rendezvous-server": "evil.example", "auto-disconnect-timeout": 10}"#,
        );
        let mut pairs = pairs;
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("auto-disconnect-timeout".to_owned(), "10".to_owned()),
                ("clanto-require-2fa".to_owned(), "Y".to_owned()),
                ("enable-clipboard".to_owned(), "N".to_owned()),
            ]
        );
        assert!(managed_pairs("[1]").is_empty());
    }

    #[test]
    fn android_restrictions_cover_every_gpo_key() {
        let xml = include_str!("../../flutter/android/app/src/main/res/xml/app_restrictions.xml");
        for key in GPO_KEYS {
            assert!(xml.contains(&format!("android:key=\"{key}\"")), "{key}");
        }
    }
}
