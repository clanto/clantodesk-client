// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Restrizioni applicate dal controller alle proprie sessioni in uscita.

use hbb_common::config::Config;

pub const OPT_FILE_TRANSFER: &str = "clanto-outgoing-file-transfer";
pub const OPT_CLIPBOARD: &str = "clanto-outgoing-clipboard";
pub const OPT_TUNNEL: &str = "clanto-outgoing-tunnel";
pub const OPT_TERMINAL: &str = "clanto-outgoing-terminal";
pub const OPT_REMOTE_PRINTER: &str = "clanto-outgoing-remote-printer";
pub const OPT_RECORD: &str = "clanto-outgoing-record";

pub fn allowed(key: &str) -> bool {
    allowed_value(&Config::get_option(key))
}

fn allowed_value(value: &str) -> bool {
    value != "N"
}

#[cfg(test)]
mod tests {
    use super::allowed_value;

    #[test]
    fn absent_or_yes_allows_and_no_blocks() {
        assert!(allowed_value(""));
        assert!(allowed_value("Y"));
        assert!(!allowed_value("N"));
    }
}
