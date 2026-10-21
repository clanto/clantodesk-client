// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use hbb_common::config;

pub const OPT_LOCK_SETTINGS: &str = "clanto-lock-settings";
pub const MANAGED_SETTINGS_MESSAGE: &str = "Settings are managed by policy";
pub const LOCKED_LOCAL_FLUTTER_OPTIONS: &[&str] =
    &["clanto-local-password", "clanto-local-password-scope"];

pub fn user_write_allowed(lock_value: &str) -> bool {
    lock_value != "Y"
}

pub fn user_write_allowed_now() -> bool {
    let overwrite = config::OVERWRITE_SETTINGS.read().unwrap();
    user_write_allowed(
        overwrite
            .get(OPT_LOCK_SETTINGS)
            .map(String::as_str)
            .unwrap_or_default(),
    )
}

pub fn user_option_write_allowed(key: &str) -> bool {
    key != OPT_LOCK_SETTINGS && user_write_allowed_now()
}

pub fn user_local_flutter_option_write_allowed(key: &str, lock_value: &str) -> bool {
    !LOCKED_LOCAL_FLUTTER_OPTIONS.contains(&key) || user_write_allowed(lock_value)
}

pub fn user_local_flutter_option_write_allowed_now(key: &str) -> bool {
    let overwrite = config::OVERWRITE_SETTINGS.read().unwrap();
    user_local_flutter_option_write_allowed(
        key,
        overwrite
            .get(OPT_LOCK_SETTINGS)
            .map(String::as_str)
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::{user_local_flutter_option_write_allowed, user_write_allowed};

    #[test]
    fn absent_or_disabled_policy_allows_user_writes() {
        assert!(user_write_allowed(""));
        assert!(user_write_allowed("N"));
    }

    #[test]
    fn enabled_policy_rejects_user_writes() {
        assert!(!user_write_allowed("Y"));
    }

    #[test]
    fn enabled_policy_only_rejects_protected_local_settings_password() {
        for lock_value in ["", "N", "Y"] {
            for key in ["access_token", "user_info", "theme", "lang"] {
                assert!(user_local_flutter_option_write_allowed(key, lock_value));
            }
        }
        assert!(!user_local_flutter_option_write_allowed(
            "clanto-local-password",
            "Y"
        ));
        assert!(!user_local_flutter_option_write_allowed(
            "clanto-local-password-scope",
            "Y"
        ));
        assert!(user_local_flutter_option_write_allowed(
            "clanto-local-password",
            "N"
        ));
    }
}
