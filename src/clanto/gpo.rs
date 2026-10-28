// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Criteri di gruppo (Windows) e policy di macOS e Linux, applicati una volta
//! all'avvio del processo.

pub const GPO_KEYS: &[&str] = &[
    "enable-remote-printer",
    "enable-clipboard",
    "enable-file-transfer",
    "enable-camera",
    "enable-terminal",
    "enable-tunnel",
    "enable-remote-restart",
    "enable-record-session",
    "enable-keyboard",
    "enable-audio",
    "enable-lan-discovery",
    "allow-auto-disconnect",
    "auto-disconnect-timeout",
    "verification-method",
    "approve-mode",
    "id-whitelist",
    "clanto-id-permissions",
    "clanto-trusted-controllers",
    "clanto-require-2fa",
    "clanto-access-hours",
    "clanto-share-device-identity",
    "clanto-forward-enabled",
    "clanto-forward-services-host",
    "clanto-forward-services-controller",
    "clanto-forward-custom-host",
    "clanto-forward-custom-host-targets",
    "clanto-forward-reverse",
    "clanto-forward-custom-controller",
    "clanto-forward-custom-controller-targets",
    "clanto-forward-webcam",
    "clanto-forward-webcam-send",
    "clanto-forward-webcam-receive",
    "clanto-forward-camera-device",
    "clanto-temp-password-rotate-minutes",
    "clanto-require-protected-device",
    "clanto-posture-bypass",
    "clanto-file-scan",
    "clanto-file-scan-clamd",
    "clanto-file-scan-max-mb",
    "clanto-file-direction",
    "clanto-file-blocked-ext",
    "clanto-eventlog-enabled",
    "clanto-syslog-enabled",
    "clanto-syslog-host",
    "clanto-syslog-port",
    "clanto-syslog-protocol",
    "clanto-syslog-facility",
    "clanto-syslog-tls-ca",
    "clanto-syslog-format",
    "clanto-session-log-days",
    "clanto-privacy-prompt",
    "allow-auto-update",
    "clanto-outgoing-file-transfer",
    "clanto-outgoing-clipboard",
    "clanto-outgoing-tunnel",
    "clanto-outgoing-terminal",
    "clanto-outgoing-remote-printer",
    "clanto-outgoing-record",
    "clanto-require-controller-watermark",
    "clanto-host-watermark",
    "clanto-lock-settings",
    "clanto-enroll-token",
    "clanto-unenroll",
];

#[cfg(windows)]
pub fn apply() {
    use hbb_common::{config, log};
    use std::io::ErrorKind;
    use winreg::{enums::*, RegKey, RegValue};

    const POLICY_KEY: &str = r"Software\Policies\Clanto\ClantoDesk";

    fn string_value(value: &RegValue) -> Option<String> {
        if value.vtype != REG_SZ || value.bytes.len() % 2 != 0 {
            return None;
        }
        let chars = value
            .bytes
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .take_while(|c| *c != 0)
            .collect::<Vec<_>>();
        String::from_utf16(&chars).ok()
    }

    fn dword_value(value: &RegValue) -> Option<String> {
        if value.vtype != REG_DWORD || value.bytes.len() != 4 {
            return None;
        }
        Some(u32::from_le_bytes(value.bytes.as_slice().try_into().ok()?).to_string())
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = match hklm.open_subkey(POLICY_KEY) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return,
        Err(error) => {
            log::debug!("lettura criteri di gruppo ClantoDesk: {}", error);
            return;
        }
    };

    let mut overwrite = config::OVERWRITE_SETTINGS.write().unwrap();
    for entry in key.enum_values() {
        let (name, value) = match entry {
            Ok(entry) => entry,
            Err(error) => {
                log::debug!("lettura valore criterio di gruppo ClantoDesk: {}", error);
                continue;
            }
        };
        if !GPO_KEYS.contains(&name.as_str()) {
            continue;
        }
        let value = string_value(&value).or_else(|| dword_value(&value));
        match value {
            Some(value) => {
                overwrite.insert(name, value);
            }
            None => log::debug!(
                "tipo o valore non valido per il criterio di gruppo ClantoDesk '{}'",
                name
            ),
        }
    }
}

/// macOS: profilo di configurazione MDM, dominio `it.clanto.clantodesk`
/// (`/Library/Managed Preferences`). Linux: `/etc/clantodesk/policy.json`.
/// Stesse chiavi e stesso effetto della GPO (`GPO_KEYS`, i nomi delle opzioni);
/// il file deve essere di root e non scrivibile da altri.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn apply() {
    use hbb_common::log;
    use std::os::unix::fs::MetadataExt;

    #[cfg(target_os = "macos")]
    const POLICY_FILE: &str = "/Library/Managed Preferences/it.clanto.clantodesk.plist";
    #[cfg(target_os = "linux")]
    const POLICY_FILE: &str = "/etc/clantodesk/policy.json";

    let Ok(meta) = std::fs::metadata(POLICY_FILE) else {
        return;
    };
    if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
        log::warn!("{POLICY_FILE} ignored: it must be owned by root and not writable by others");
        return;
    }
    #[cfg(target_os = "macos")]
    let json = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-", POLICY_FILE])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok());
    #[cfg(target_os = "linux")]
    let json = std::fs::read_to_string(POLICY_FILE).ok();
    match json {
        Some(json) => super::managed_config::apply_json(&json),
        None => log::warn!("{POLICY_FILE} is not readable"),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn apply() {}

#[cfg(test)]
mod tests {
    use super::GPO_KEYS;

    #[test]
    fn admx_value_names_match_gpo_keys() {
        let admx = include_str!("../../res/admx/clantodesk.admx");
        let mut value_names = admx
            .split("valueName=\"")
            .skip(1)
            .filter_map(|part| part.split_once('"').map(|(name, _)| name))
            .collect::<Vec<_>>();
        let mut gpo_keys = GPO_KEYS.to_vec();
        value_names.sort_unstable();
        gpo_keys.sort_unstable();
        assert_eq!(value_names, gpo_keys);
    }

    #[test]
    fn gpo_cannot_manage_servers() {
        for key in [
            "custom-rendezvous-server",
            "key",
            "api-server",
            "relay-server",
        ] {
            assert!(!GPO_KEYS.contains(&key));
        }
    }

    #[test]
    fn gpo_cannot_reenable_self_trusted_devices() {
        assert!(!GPO_KEYS.contains(&"enable-trusted-devices"));
    }

    #[test]
    fn forwarding_keys_are_gpo_managed_and_server_protected() {
        for key in crate::clanto::forwarding::policy::GPO_KEYS {
            assert!(GPO_KEYS.contains(key), "{key} not in GPO_KEYS");
            // Le liste dei servizi le imposta anche l'API server (SERVER_MANAGED_KEYS).
            if crate::clanto::server_sync::SERVER_MANAGED_KEYS.contains(key) {
                continue;
            }
            assert!(
                crate::clanto::server_sync::SERVER_PROTECTED_KEYS.contains(key),
                "{key} can be set by the server"
            );
        }
    }
}
