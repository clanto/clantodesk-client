// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use base::message_proto::PeerInfo;
use hbb_common::{
    config::Config,
    log,
    rand::{rngs::OsRng, RngCore as _},
};
use serde_json::{json, Map, Value};

pub const OPT_REQUIRE_CONTROLLER_WATERMARK: &str = "clanto-require-controller-watermark";
pub const PEER_INFO_REQUIRE_CONTROLLER_WATERMARK: &str = "clanto-require-controller-watermark";
const SESSION_CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub fn recording_allowed(requested: bool, require_controller_watermark: bool) -> bool {
    requested && !require_controller_watermark
}

pub fn configured_recording_allowed(requested: bool) -> bool {
    recording_allowed(
        requested,
        Config::get_option(OPT_REQUIRE_CONTROLLER_WATERMARK) == "Y",
    )
}

pub fn generate_session_code() -> Result<String, String> {
    let mut random = [0u8; 8];
    OsRng
        .try_fill_bytes(&mut random)
        .map_err(|error| format!("generazione codice sessione: {error}"))?;
    let mut code = String::with_capacity(11);
    code.push_str("S-");
    for (index, byte) in random.iter().enumerate() {
        if index == 4 {
            code.push('-');
        }
        code.push(SESSION_CODE_ALPHABET[(byte & 0x1f) as usize] as char);
    }
    Ok(code)
}

pub fn add_requirement(pi: &mut PeerInfo) {
    if Config::get_option(OPT_REQUIRE_CONTROLLER_WATERMARK) != "Y" {
        return;
    }

    let mut additions = if pi.platform_additions.is_empty() {
        Map::new()
    } else {
        match serde_json::from_str::<Map<String, Value>>(&pi.platform_additions) {
            Ok(value) => value,
            Err(error) => {
                log::error!("lettura dati sessione per requisito filigrana: {error}");
                Map::new()
            }
        }
    };
    additions.insert(
        PEER_INFO_REQUIRE_CONTROLLER_WATERMARK.to_owned(),
        json!(true),
    );
    match serde_json::to_string(&additions) {
        Ok(value) => pi.platform_additions = value,
        Err(error) => log::error!("serializzazione requisito filigrana controller: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn session_code_has_expected_format_and_alphabet() {
        let code = generate_session_code().unwrap();
        assert_eq!(code.len(), 11);
        assert_eq!(&code[..2], "S-");
        assert_eq!(&code[6..7], "-");
        assert!(code
            .chars()
            .enumerate()
            .all(
                |(index, character)| matches!(index, 1 | 6) && character == '-'
                    || index == 0 && character == 'S'
                    || !matches!(index, 0 | 1 | 6)
                        && SESSION_CODE_ALPHABET.contains(&(character as u8))
            ));
    }

    #[test]
    fn generated_session_codes_are_unique() {
        let codes = (0..1_000)
            .map(|_| generate_session_code().unwrap())
            .collect::<HashSet<_>>();
        assert_eq!(codes.len(), 1_000);
    }

    #[test]
    fn controller_watermark_disables_recording() {
        assert!(recording_allowed(true, false));
        assert!(!recording_allowed(false, false));
        assert!(!recording_allowed(true, true));
        assert!(!recording_allowed(false, true));
    }
}
