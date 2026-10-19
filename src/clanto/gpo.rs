// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Criteri di gruppo applicati una volta all'avvio del processo.

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
    "clanto-outgoing-file-transfer",
    "clanto-outgoing-clipboard",
    "clanto-outgoing-tunnel",
    "clanto-outgoing-terminal",
    "clanto-outgoing-remote-printer",
    "clanto-outgoing-record",
    "clanto-controller-watermark",
    "clanto-require-controller-watermark",
    "clanto-host-watermark",
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

#[cfg(not(windows))]
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
}
