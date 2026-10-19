// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use base::message_proto::PeerInfo;
use hbb_common::{config::Config, log};
use serde_json::{json, Map, Value};

pub const OPT_REQUIRE_CONTROLLER_WATERMARK: &str = "clanto-require-controller-watermark";
pub const PEER_INFO_REQUIRE_CONTROLLER_WATERMARK: &str = "clanto-require-controller-watermark";

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
