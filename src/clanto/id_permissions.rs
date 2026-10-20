// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use serde_derive::Deserialize;
use std::{collections::BTreeMap, time::Duration};

pub use base::config::keys::OPTION_CLANTO_ID_PERMISSIONS as OPTION_ID_PERMISSIONS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permission {
    Keyboard,
    Clipboard,
    File,
    Audio,
    Camera,
    Terminal,
    Tunnel,
    Restart,
    Recording,
    BlockInput,
    PrivacyMode,
    RemotePrinter,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
struct CustomPermissions {
    #[serde(default)]
    keyboard: bool,
    #[serde(default)]
    clipboard: bool,
    #[serde(default)]
    file: bool,
    #[serde(default)]
    audio: bool,
    #[serde(default)]
    camera: bool,
    #[serde(default)]
    terminal: bool,
    #[serde(default)]
    tunnel: bool,
    #[serde(default)]
    restart: bool,
    #[serde(default)]
    recording: bool,
    #[serde(default)]
    block_input: bool,
    #[serde(default)]
    privacy_mode: bool,
    #[serde(default)]
    remote_printer: bool,
}

impl CustomPermissions {
    fn allows(self, permission: Permission) -> bool {
        match permission {
            Permission::Keyboard => self.keyboard,
            Permission::Clipboard => self.clipboard,
            Permission::File => self.file,
            Permission::Audio => self.audio,
            Permission::Camera => self.camera,
            Permission::Terminal => self.terminal,
            Permission::Tunnel => self.tunnel,
            Permission::Restart => self.restart,
            Permission::Recording => self.recording,
            Permission::BlockInput => self.block_input,
            Permission::PrivacyMode => self.privacy_mode,
            Permission::RemotePrinter => self.remote_printer,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    View,
    Support,
    Full,
    Custom,
}

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Support => "support",
            Self::Full => "full",
            Self::Custom => "custom",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ProfileConfig {
    #[serde(default)]
    name: String,
    profile: Profile,
    #[serde(default)]
    perms: CustomPermissions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedProfile {
    pub name: String,
    pub profile: Profile,
    permissions: CustomPermissions,
}

impl ResolvedProfile {
    fn new(name: String, profile: Profile, permissions: CustomPermissions) -> Self {
        Self {
            name,
            profile,
            permissions,
        }
    }

    fn view() -> Self {
        Self::new(String::new(), Profile::View, CustomPermissions::default())
    }

    fn full() -> Self {
        Self::new(String::new(), Profile::Full, CustomPermissions::default())
    }

    pub fn allows(&self, permission: Permission) -> bool {
        match self.profile {
            Profile::View => false,
            Profile::Support => matches!(
                permission,
                Permission::Keyboard
                    | Permission::Clipboard
                    | Permission::File
                    | Permission::Audio
                    | Permission::Restart
                    | Permission::BlockInput
                    | Permission::PrivacyMode
                    | Permission::RemotePrinter
            ),
            Profile::Full => true,
            Profile::Custom => self.permissions.allows(permission),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resolution {
    pub profile: Option<ResolvedProfile>,
    pub invalid_json: bool,
}

pub fn resolve_profile(id_whitelist: &[String], json: &str, controller_id: &str) -> Resolution {
    let Some(entry) = matching_entry(id_whitelist, controller_id) else {
        return Resolution {
            profile: None,
            invalid_json: false,
        };
    };
    if json.trim().is_empty() {
        return Resolution {
            profile: Some(ResolvedProfile::full()),
            invalid_json: false,
        };
    }
    let profiles = match serde_json::from_str::<BTreeMap<String, ProfileConfig>>(json) {
        Ok(profiles) => profiles,
        Err(_) => {
            return Resolution {
                profile: Some(ResolvedProfile::view()),
                invalid_json: true,
            };
        }
    };
    let Some(config) = profiles
        .iter()
        .find_map(|(key, config)| key.eq_ignore_ascii_case(entry).then_some(config))
    else {
        return Resolution {
            profile: Some(ResolvedProfile::full()),
            invalid_json: false,
        };
    };
    Resolution {
        profile: Some(ResolvedProfile::new(
            config.name.clone(),
            config.profile,
            config.perms,
        )),
        invalid_json: false,
    }
}

pub fn resolve_profile_logged(
    id_whitelist: &[String],
    json: &str,
    controller_id: &str,
) -> Option<ResolvedProfile> {
    let resolution = resolve_profile(id_whitelist, json, controller_id);
    if resolution.invalid_json {
        hbb_common::throttled_log!(
            Duration::from_secs(60),
            error,
            "Invalid {OPTION_ID_PERMISSIONS}; applying the view profile"
        );
    }
    resolution.profile
}

fn matching_entry<'a>(id_whitelist: &'a [String], controller_id: &str) -> Option<&'a str> {
    let controller_id = controller_id.trim();
    let bare_id = controller_id.split('@').next().unwrap_or(controller_id);
    let mut best: Option<(&str, (u8, usize))> = None;
    for entry in id_whitelist {
        let entry = entry.trim();
        let Some(rank) = match_rank(entry, controller_id, bare_id) else {
            continue;
        };
        if best.map_or(true, |(_, best_rank)| rank > best_rank) {
            best = Some((entry, rank));
        }
    }
    best.map(|(entry, _)| entry)
}

fn match_rank(pattern: &str, controller_id: &str, bare_id: &str) -> Option<(u8, usize)> {
    let has_wildcard = pattern
        .chars()
        .any(|character| matches!(character, '*' | '?'));
    if !has_wildcard {
        if pattern.eq_ignore_ascii_case(controller_id) {
            return Some((3, pattern.chars().count()));
        }
        if pattern.eq_ignore_ascii_case(bare_id) {
            return Some((2, pattern.chars().count()));
        }
        return None;
    }
    if !wildcard_match(pattern, controller_id) && !wildcard_match(pattern, bare_id) {
        return None;
    }
    if pattern == "*" {
        return Some((0, 0));
    }
    Some((
        1,
        pattern
            .chars()
            .filter(|character| !matches!(character, '*' | '?'))
            .count(),
    ))
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.trim().to_lowercase().chars().collect();
    let text: Vec<char> = text.trim().to_lowercase().chars().collect();
    let (mut pattern_index, mut text_index) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while text_index < text.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star = Some((pattern_index + 1, text_index));
            pattern_index += 1;
        } else if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if let Some((after_star, star_text_index)) = star {
            pattern_index = after_star;
            text_index = star_text_index + 1;
            star = Some((after_star, star_text_index + 1));
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERMISSIONS: [Permission; 12] = [
        Permission::Keyboard,
        Permission::Clipboard,
        Permission::File,
        Permission::Audio,
        Permission::Camera,
        Permission::Terminal,
        Permission::Tunnel,
        Permission::Restart,
        Permission::Recording,
        Permission::BlockInput,
        Permission::PrivacyMode,
        Permission::RemotePrinter,
    ];

    fn list(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| (*entry).to_owned()).collect()
    }

    fn resolved(entries: &[&str], json: &str, controller_id: &str) -> ResolvedProfile {
        resolve_profile(&list(entries), json, controller_id)
            .profile
            .expect("matching profile")
    }

    #[test]
    fn exact_entry_wins_over_the_most_specific_wildcard_and_catch_all() {
        let json = r#"{
            "*": {"profile":"view"},
            "12*": {"profile":"support"},
            "123*": {"profile":"custom","perms":{"camera":true}},
            "12345": {"profile":"full"},
            "12345@example.com": {"profile":"view"}
        }"#;
        let entries = ["*", "12*", "123*", "12345", "12345@example.com"];
        assert_eq!(resolved(&entries, json, "12345").profile, Profile::Full);
        assert_eq!(
            resolved(&entries, json, "12345@example.com").profile,
            Profile::View
        );
        assert_eq!(resolved(&entries, json, "12399").profile, Profile::Custom);
        assert_eq!(resolved(&entries, json, "12999").profile, Profile::Support);
        assert_eq!(resolved(&entries, json, "99999").profile, Profile::View);
    }

    #[test]
    fn missing_profile_is_full_and_invalid_json_is_view() {
        let entries = ["12345"];
        assert_eq!(
            resolved(&entries, r#"{"other":{"profile":"view"}}"#, "12345").profile,
            Profile::Full
        );
        let invalid = resolve_profile(&list(&entries), "not json", "12345");
        assert!(invalid.invalid_json);
        assert_eq!(
            invalid.profile.expect("fail-closed profile").profile,
            Profile::View
        );
        assert_eq!(resolve_profile(&[], "not json", "12345").profile, None);
    }

    #[test]
    fn profiles_grant_exactly_their_twelve_permissions() {
        let json = r#"{
            "view": {"profile":"view"},
            "support": {"profile":"support"},
            "full": {"profile":"full"},
            "custom": {"profile":"custom","perms":{
                "keyboard":true,"file":true,"camera":true,"tunnel":true,
                "recording":true,"privacy_mode":true
            }}
        }"#;
        let support_expected = [
            true, true, true, true, false, false, false, true, false, true, true, true,
        ];
        let custom_expected = [
            true, false, true, false, true, false, true, false, true, false, true, false,
        ];
        for (id, expected) in [
            ("view", [false; 12]),
            ("support", support_expected),
            ("full", [true; 12]),
            ("custom", custom_expected),
        ] {
            let profile = resolved(&[id], json, id);
            for (permission, expected) in PERMISSIONS.into_iter().zip(expected) {
                assert_eq!(profile.allows(permission), expected, "{id}: {permission:?}");
            }
        }
    }
}
