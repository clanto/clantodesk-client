// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use chrono::{DateTime, FixedOffset, Local};
use hbb_common::config::LocalConfig;
use std::{collections::HashMap, sync::Mutex};

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub const OPTION_HOST_WATERMARK: &str = "clanto-host-watermark";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatermarkSession {
    pub controller_id: String,
    pub controller_name: String,
    pub account_user: Option<String>,
    pub started_at: DateTime<FixedOffset>,
}

#[derive(Clone)]
struct TrackedSession {
    controller_id: String,
    controller_name: String,
    account_user: Option<String>,
    authorized: bool,
    eligible: bool,
    started_at: Option<DateTime<FixedOffset>>,
}

lazy_static::lazy_static! {
    static ref SESSIONS: Mutex<HashMap<i32, TrackedSession>> = Mutex::new(HashMap::new());
    static ref DISPLAYED: Mutex<bool> = Mutex::new(false);
}

pub fn controller_account_user() -> String {
    serde_json::from_str::<serde_json::Value>(&LocalConfig::get_option("user_info"))
        .ok()
        .and_then(|info| {
            info.get("name")
                .and_then(|name| name.as_str())
                .map(str::to_owned)
        })
        .filter(|user| !user.trim().is_empty())
        .unwrap_or_default()
}

pub fn is_watermark_session(
    is_file_transfer: bool,
    is_view_camera: bool,
    is_terminal: bool,
    is_port_forward: bool,
) -> bool {
    let excluded = is_file_transfer || is_terminal || is_port_forward;
    let is_camera = is_view_camera && !excluded;
    let is_desktop = !is_view_camera && !excluded;
    is_desktop || is_camera
}

pub fn should_show(enabled: bool, sessions: &[WatermarkSession]) -> bool {
    enabled && !sessions.is_empty()
}

pub fn should_request_account_user(enabled: bool, authorized: bool, eligible: bool) -> bool {
    enabled && authorized && eligible
}

pub fn watermark_text(sessions: &[WatermarkSession]) -> Option<String> {
    let first_started = sessions.iter().map(|session| session.started_at).min()?;
    let mut controllers = sessions
        .iter()
        .map(|session| {
            let identity = if session.controller_name.trim().is_empty()
                || session.controller_name == session.controller_id
            {
                session.controller_id.clone()
            } else {
                format!(
                    "{} ({})",
                    session.controller_name.trim(),
                    session.controller_id.trim()
                )
            };
            match session
                .account_user
                .as_deref()
                .map(str::trim)
                .filter(|user| !user.is_empty())
            {
                Some(user) => format!("{identity} - account {user}"),
                None => identity,
            }
        })
        .collect::<Vec<_>>();
    controllers.sort();
    controllers.dedup();
    Some(format!(
        "Remote session - Controller: {} - Started: {} - Active sessions: {}",
        controllers.join(", "),
        first_started.format("%Y-%m-%d %H:%M"),
        sessions.len()
    ))
}

pub fn connection_added(client: &crate::ui_cm_interface::Client) {
    let eligible = is_watermark_session(
        client.is_file_transfer,
        client.is_view_camera,
        client.is_terminal,
        !client.port_forward.is_empty(),
    );
    let now = Local::now().fixed_offset();
    let mut sessions = SESSIONS.lock().unwrap();
    let session = sessions.entry(client.id).or_insert_with(|| TrackedSession {
        controller_id: client.peer_id.clone(),
        controller_name: client.name.clone(),
        account_user: None,
        authorized: client.authorized,
        eligible,
        started_at: (client.authorized && eligible).then_some(now),
    });
    session.controller_id.clone_from(&client.peer_id);
    session.controller_name.clone_from(&client.name);
    session.eligible = eligible;
    if client.authorized && !session.authorized {
        session.started_at = eligible.then_some(now);
    }
    session.authorized = client.authorized;
    if !eligible {
        session.started_at = None;
    }
    drop(sessions);
    refresh_if_enabled();
}

pub fn connection_authorized(id: i32) {
    let mut sessions = SESSIONS.lock().unwrap();
    if let Some(session) = sessions.get_mut(&id) {
        if !session.authorized {
            session.authorized = true;
            session.started_at = session.eligible.then(|| Local::now().fixed_offset());
        }
    }
    drop(sessions);
    refresh_if_enabled();
}

pub fn connection_account_user(id: i32, account_user: String) {
    let mut sessions = SESSIONS.lock().unwrap();
    if let Some(session) = sessions.get_mut(&id) {
        session.account_user = (!account_user.trim().is_empty()).then_some(account_user);
    }
    drop(sessions);
    refresh_if_enabled();
}

pub fn connection_removed(id: i32) {
    SESSIONS.lock().unwrap().remove(&id);
    refresh_if_enabled();
}

pub fn clear() {
    let mut displayed = DISPLAYED.lock().unwrap();
    SESSIONS.lock().unwrap().clear();
    hide_locked(&mut displayed);
}

pub fn option_changed(enabled: bool) {
    if enabled {
        refresh();
    } else {
        hide();
    }
}

fn refresh_if_enabled() {
    if crate::ui_interface::get_option(OPTION_HOST_WATERMARK) == "Y" {
        refresh();
    }
}

fn hide() {
    let mut displayed = DISPLAYED.lock().unwrap();
    hide_locked(&mut displayed);
}

fn refresh() {
    let mut displayed = DISPLAYED.lock().unwrap();
    if crate::ui_interface::get_option(OPTION_HOST_WATERMARK) != "Y" {
        hide_locked(&mut displayed);
        return;
    }
    let text = {
        let sessions = SESSIONS.lock().unwrap();
        let active = sessions
            .values()
            .filter(|session| session.authorized && session.eligible)
            .filter_map(|session| {
                Some(WatermarkSession {
                    controller_id: session.controller_id.clone(),
                    controller_name: session.controller_name.clone(),
                    account_user: session.account_user.clone(),
                    started_at: session.started_at?,
                })
            })
            .collect::<Vec<_>>();
        watermark_text(&active)
    };
    if text.is_some() {
        platform_update(text.as_deref());
        *displayed = true;
    } else {
        hide_locked(&mut displayed);
    }
}

fn hide_locked(displayed: &mut bool) {
    if *displayed {
        platform_update(None);
        *displayed = false;
    }
}

fn platform_update(text: Option<&str>) {
    #[cfg(target_os = "android")]
    android::update(text);
    #[cfg(target_os = "linux")]
    linux::update(text);
    #[cfg(target_os = "macos")]
    macos::update(text);
    #[cfg(target_os = "windows")]
    windows::update(text);
    #[cfg(not(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "windows"
    )))]
    let _ = text;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn session(id: &str, name: &str, user: Option<&str>, timestamp: i64) -> WatermarkSession {
        WatermarkSession {
            controller_id: id.to_owned(),
            controller_name: name.to_owned(),
            account_user: user.map(str::to_owned),
            started_at: Utc
                .timestamp_opt(timestamp, 0)
                .single()
                .unwrap()
                .fixed_offset(),
        }
    }

    #[test]
    fn formats_controller_user_start_and_active_count() {
        let sessions = vec![
            session(
                "111",
                "Alice laptop",
                Some("alice@example.test"),
                1_704_110_400,
            ),
            session("222", "", None, 1_704_114_000),
        ];

        assert_eq!(
            watermark_text(&sessions).as_deref(),
            Some(
                "Remote session - Controller: 222, Alice laptop (111) - account alice@example.test - Started: 2024-01-01 12:00 - Active sessions: 2"
            )
        );
    }

    #[test]
    fn show_decision_requires_option_and_active_session() {
        let sessions = vec![session("111", "Alice", None, 1_704_110_400)];
        assert!(!should_show(false, &sessions));
        assert!(!should_show(true, &[]));
        assert!(should_show(true, &sessions));
    }

    #[test]
    fn account_user_is_requested_only_for_enabled_authorized_eligible_sessions() {
        assert!(should_request_account_user(true, true, true));
        assert!(!should_request_account_user(false, true, true));
        assert!(!should_request_account_user(true, false, true));
        assert!(!should_request_account_user(true, true, false));
    }

    #[test]
    fn only_desktop_and_camera_connections_are_watermarked() {
        assert!(is_watermark_session(false, false, false, false));
        assert!(is_watermark_session(false, true, false, false));
        assert!(!is_watermark_session(true, false, false, false));
        assert!(!is_watermark_session(false, false, true, false));
        assert!(!is_watermark_session(false, false, false, true));
    }
}
