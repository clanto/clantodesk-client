// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! 2FA obbligatorio per policy (`clanto-require-2fa=Y`, di norma via GPO).
//!
//! Il segreto TOTP e' per macchina e va registrato dall'utente con l'app di
//! autenticazione: non si distribuisce. La policy quindi non attiva il 2FA ma
//! impedisce di accedere a un host che non lo ha configurato.

use hbb_common::config::Config;

pub const OPT_REQUIRE_2FA: &str = "clanto-require-2fa";
const LOGIN_ERROR: &str = "clanto_2fa_policy_error";

pub fn required() -> bool {
    Config::get_option(OPT_REQUIRE_2FA) == "Y"
}

/// Vero se la policy chiede il 2FA ma sull'host non e' configurato. Un
/// controller fidato non cambia l'esito: l'esenzione presuppone un 2FA attivo.
pub fn blocks_login() -> bool {
    required() && crate::auth_2fa::get_2fa(None).is_none()
}

/// Testo gia' tradotto dall'host, come per i rifiuti della postura.
pub fn login_error() -> String {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    return crate::lang::translate(LOGIN_ERROR.to_owned());
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return crate::lang::translate_locale(LOGIN_ERROR.to_owned(), "");
}
