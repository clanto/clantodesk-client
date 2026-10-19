// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use base::message_proto::ClantoControllerIdentity;
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
    pub os_user: Option<String>,
    pub pc_name: Option<String>,
    pub session_code: String,
    pub access_type: WatermarkAccessType,
    pub permissions: Vec<WatermarkPermission>,
    pub started_at: DateTime<FixedOffset>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatermarkAccessType {
    FullControl,
    ViewOnly,
    Camera,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatermarkPermission {
    Keyboard,
    Clipboard,
    File,
    Audio,
    Restart,
    Recording,
    BlockInput,
    PrivacyMode,
}

#[derive(Clone)]
struct TrackedSession {
    controller_id: String,
    controller_name: String,
    account_user: Option<String>,
    os_user: Option<String>,
    pc_name: Option<String>,
    session_code: String,
    authorized: bool,
    eligible: bool,
    is_view_camera: bool,
    keyboard: bool,
    clipboard: bool,
    audio: bool,
    file: bool,
    restart: bool,
    recording: bool,
    block_input: bool,
    privacy_mode: bool,
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

pub fn controller_identity() -> ClantoControllerIdentity {
    ClantoControllerIdentity {
        account_user: controller_account_user(),
        os_user: controller_os_user(),
        pc_name: crate::common::hostname(),
        ..Default::default()
    }
}

#[cfg(target_os = "windows")]
pub fn controller_os_user() -> String {
    let username = crate::platform::get_active_username();
    let username = if username.trim().is_empty() {
        std::env::var("USERNAME").unwrap_or_default()
    } else {
        username
    };
    let username = username.trim();
    if username.is_empty() || username.contains('\\') {
        return username.to_owned();
    }
    std::env::var("USERDOMAIN")
        .ok()
        .map(|domain| domain.trim().to_owned())
        .filter(|domain| !domain.is_empty())
        .map(|domain| format!("{domain}\\{username}"))
        .unwrap_or_else(|| username.to_owned())
}

#[cfg(not(any(target_os = "windows", target_os = "android", target_os = "ios")))]
pub fn controller_os_user() -> String {
    crate::platform::get_active_username().trim().to_owned()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn controller_os_user() -> String {
    String::new()
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

pub fn should_request_controller_identity(
    host_watermark: bool,
    controller_watermark: bool,
    authorized: bool,
    eligible: bool,
) -> bool {
    (host_watermark || controller_watermark) && authorized && eligible
}

fn translate(key: &str) -> String {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    return crate::lang::translate(key.to_owned());
    #[cfg(any(target_os = "android", target_os = "ios"))]
    crate::lang::translate_locale(key.to_owned(), "")
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn technician(session: &WatermarkSession) -> String {
    match (
        nonempty(Some(&session.controller_name)),
        nonempty(Some(&session.controller_id)),
    ) {
        (Some(name), Some(id)) if name != id => format!("{name} ({id})"),
        (Some(name), _) => name.to_owned(),
        (_, Some(id)) => id.to_owned(),
        _ => String::new(),
    }
}

fn access_type(access_type: WatermarkAccessType, translate: &impl Fn(&str) -> String) -> String {
    translate(match access_type {
        WatermarkAccessType::FullControl => "Full control",
        WatermarkAccessType::ViewOnly => "View only",
        WatermarkAccessType::Camera => "Camera",
    })
}

fn permission_name(permission: WatermarkPermission, translate: &impl Fn(&str) -> String) -> String {
    translate(match permission {
        WatermarkPermission::Keyboard => "Keyboard permission",
        WatermarkPermission::Clipboard => "Clipboard permission",
        WatermarkPermission::File => "File permission",
        WatermarkPermission::Audio => "Audio permission",
        WatermarkPermission::Restart => "Remote restart permission",
        WatermarkPermission::Recording => "Recording permission",
        WatermarkPermission::BlockInput => "Block input permission",
        WatermarkPermission::PrivacyMode => "Privacy mode permission",
    })
}

fn watermark_line(
    session: &WatermarkSession,
    active_sessions: usize,
    translate: &impl Fn(&str) -> String,
) -> String {
    let mut parts = Vec::new();
    let code = session.session_code.trim();
    if !code.is_empty() {
        parts.push(format!("{} {code}", translate("Remote session")));
    }
    parts.push(access_type(session.access_type, translate));
    if !session.permissions.is_empty() {
        parts.push(
            session
                .permissions
                .iter()
                .copied()
                .map(|permission| permission_name(permission, translate))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    let technician = technician(session);
    if !technician.is_empty() {
        parts.push(format!("{}: {technician}", translate("Technician")));
    }
    if let Some(account) = nonempty(session.account_user.as_deref()) {
        parts.push(format!("{} {account}", translate("Account label")));
    }
    match (
        nonempty(session.os_user.as_deref()),
        nonempty(session.pc_name.as_deref()),
    ) {
        (Some(os_user), Some(pc_name)) => {
            parts.push(format!("{os_user} {} {pc_name}", translate("On label")))
        }
        (Some(os_user), None) => parts.push(os_user.to_owned()),
        (None, Some(pc_name)) => parts.push(pc_name.to_owned()),
        _ => {}
    }
    let started = session.started_at.format("%H:%M").to_string();
    parts.push(format!("{} {started}", translate("Since label")));
    parts.push(format!(
        "{active_sessions} {}",
        translate(if active_sessions == 1 {
            "Active session label"
        } else {
            "Active sessions label"
        })
    ));
    parts.join(" · ")
}

pub fn watermark_text(sessions: &[WatermarkSession]) -> Option<String> {
    watermark_text_with(sessions, &translate)
}

fn watermark_text_with(
    sessions: &[WatermarkSession],
    translate: &impl Fn(&str) -> String,
) -> Option<String> {
    if sessions.is_empty() {
        return None;
    }
    let mut sessions = sessions.iter().collect::<Vec<_>>();
    sessions.sort_by_key(|session| (session.started_at, session.controller_id.as_str()));
    let count = sessions.len();
    let mut lines = sessions
        .into_iter()
        .take(3)
        .map(|session| watermark_line(session, count, translate))
        .collect::<Vec<_>>();
    if count > 3 {
        lines.push(format!(
            "+{} {}",
            count - 3,
            translate("More sessions label")
        ));
    }
    Some(lines.join("\n"))
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
        os_user: None,
        pc_name: None,
        session_code: client.session_code.clone(),
        authorized: client.authorized,
        eligible,
        is_view_camera: client.is_view_camera,
        keyboard: client.keyboard,
        clipboard: client.clipboard,
        audio: client.audio,
        file: client.file,
        restart: client.restart,
        recording: client.recording,
        block_input: client.block_input,
        privacy_mode: client.privacy_mode,
        started_at: (client.authorized && eligible).then_some(now),
    });
    session.controller_id.clone_from(&client.peer_id);
    session.controller_name.clone_from(&client.name);
    if !client.session_code.is_empty() {
        session.session_code.clone_from(&client.session_code);
    }
    session.eligible = eligible;
    session.is_view_camera = client.is_view_camera;
    session.keyboard = client.keyboard;
    session.clipboard = client.clipboard;
    session.audio = client.audio;
    session.file = client.file;
    session.restart = client.restart;
    session.recording = client.recording;
    session.block_input = client.block_input;
    session.privacy_mode = client.privacy_mode;
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

pub fn connection_identity(id: i32, identity: ClantoControllerIdentity) {
    let mut sessions = SESSIONS.lock().unwrap();
    if let Some(session) = sessions.get_mut(&id) {
        session.account_user = nonempty(Some(&identity.account_user)).map(str::to_owned);
        session.os_user = nonempty(Some(&identity.os_user)).map(str::to_owned);
        session.pc_name = nonempty(Some(&identity.pc_name)).map(str::to_owned);
    }
    drop(sessions);
    refresh_if_enabled();
}

pub fn connection_session_code(id: i32, session_code: String) {
    let mut sessions = SESSIONS.lock().unwrap();
    if let Some(session) = sessions.get_mut(&id) {
        session.session_code = session_code;
    }
    drop(sessions);
    refresh_if_enabled();
}

pub fn connection_permission(id: i32, name: &str, enabled: bool) {
    let mut sessions = SESSIONS.lock().unwrap();
    if let Some(session) = sessions.get_mut(&id) {
        match name {
            "keyboard" => session.keyboard = enabled,
            "clipboard" => session.clipboard = enabled,
            "audio" => session.audio = enabled,
            "file" => session.file = enabled,
            "restart" => session.restart = enabled,
            "recording" => session.recording = enabled,
            "block_input" => session.block_input = enabled,
            "privacy_mode" => session.privacy_mode = enabled,
            _ => return,
        }
    }
    drop(sessions);
    refresh_if_enabled();
}

pub fn connection_effective_permissions(
    id: i32,
    keyboard: bool,
    clipboard: bool,
    audio: bool,
    file: bool,
) {
    let mut sessions = SESSIONS.lock().unwrap();
    if let Some(session) = sessions.get_mut(&id) {
        session.keyboard = keyboard;
        session.clipboard = clipboard;
        session.audio = audio;
        session.file = file;
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
                let permissions = [
                    (session.keyboard, WatermarkPermission::Keyboard),
                    (session.clipboard, WatermarkPermission::Clipboard),
                    (session.file, WatermarkPermission::File),
                    (session.audio, WatermarkPermission::Audio),
                    (session.restart, WatermarkPermission::Restart),
                    (session.recording, WatermarkPermission::Recording),
                    (session.block_input, WatermarkPermission::BlockInput),
                    (session.privacy_mode, WatermarkPermission::PrivacyMode),
                ]
                .into_iter()
                .filter_map(|(enabled, permission)| enabled.then_some(permission))
                .collect();
                Some(WatermarkSession {
                    controller_id: session.controller_id.clone(),
                    controller_name: session.controller_name.clone(),
                    account_user: session.account_user.clone(),
                    os_user: session.os_user.clone(),
                    pc_name: session.pc_name.clone(),
                    session_code: session.session_code.clone(),
                    access_type: if session.is_view_camera {
                        WatermarkAccessType::Camera
                    } else if session.keyboard {
                        WatermarkAccessType::FullControl
                    } else {
                        WatermarkAccessType::ViewOnly
                    },
                    permissions,
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

    fn session(
        id: &str,
        name: &str,
        account: Option<&str>,
        os_user: Option<&str>,
        pc_name: Option<&str>,
        code: &str,
        access_type: WatermarkAccessType,
        permissions: Vec<WatermarkPermission>,
        timestamp: i64,
    ) -> WatermarkSession {
        WatermarkSession {
            controller_id: id.to_owned(),
            controller_name: name.to_owned(),
            account_user: account.map(str::to_owned),
            os_user: os_user.map(str::to_owned),
            pc_name: pc_name.map(str::to_owned),
            session_code: code.to_owned(),
            access_type,
            permissions,
            started_at: Utc
                .timestamp_opt(timestamp, 0)
                .single()
                .unwrap()
                .fixed_offset(),
        }
    }

    fn english(key: &str) -> String {
        match key {
            "Account label" => "account",
            "On label" => "on",
            "Since label" => "since",
            "Active session label" => "active session",
            "Active sessions label" => "active sessions",
            "More sessions label" => "more",
            _ => key,
        }
        .to_owned()
    }

    fn italian(key: &str) -> String {
        crate::lang::translate_locale(key.to_owned(), "it-IT")
    }

    #[test]
    fn formats_access_permissions_identity_and_active_count() {
        let sessions = vec![
            session(
                "111",
                "Alice laptop",
                Some("alice@example.test"),
                Some("ACME\\alice"),
                Some("PC-ALICE"),
                "S-0123-4567",
                WatermarkAccessType::FullControl,
                vec![
                    WatermarkPermission::Keyboard,
                    WatermarkPermission::Clipboard,
                ],
                1_704_110_400,
            ),
            session(
                "222",
                "",
                None,
                None,
                None,
                "S-89AB-CDEF",
                WatermarkAccessType::Camera,
                vec![WatermarkPermission::Audio],
                1_704_114_000,
            ),
        ];

        assert_eq!(
            watermark_text_with(&sessions, &english).as_deref(),
            Some(
                "Remote session S-0123-4567 · Full control · Keyboard permission, Clipboard permission · Technician: Alice laptop (111) · account alice@example.test · ACME\\alice on PC-ALICE · since 12:00 · 2 active sessions\nRemote session S-89AB-CDEF · Camera · Audio permission · Technician: 222 · since 13:00 · 2 active sessions"
            )
        );
    }

    #[test]
    fn formats_view_only_and_omits_missing_identity_parts() {
        let sessions = vec![session(
            "",
            "Technician",
            None,
            Some("alice"),
            None,
            "S-0123-4567",
            WatermarkAccessType::ViewOnly,
            Vec::new(),
            1_704_110_400,
        )];

        assert_eq!(
            watermark_text_with(&sessions, &english).as_deref(),
            Some(
                "Remote session S-0123-4567 · View only · Technician: Technician · alice · since 12:00 · 1 active session"
            )
        );
    }

    #[test]
    fn formats_the_host_watermark_in_italian() {
        let sessions = vec![session(
            "111",
            "Alice",
            Some("account"),
            Some("ACME\\alice"),
            Some("PC-ALICE"),
            "S-0123-4567",
            WatermarkAccessType::FullControl,
            vec![
                WatermarkPermission::Keyboard,
                WatermarkPermission::Clipboard,
            ],
            1_704_110_400,
        )];

        assert_eq!(
            watermark_text_with(&sessions, &italian).as_deref(),
            Some(
                "Sessione remota S-0123-4567 · Controllo completo · tastiera, appunti · Tecnico: Alice (111) · account account · ACME\\alice su PC-ALICE · dalle 12:00 · 1 sessione attiva"
            )
        );
    }

    #[test]
    fn limits_visible_sessions_to_three_and_reports_the_rest() {
        let sessions = (0..5)
            .map(|index| {
                session(
                    &index.to_string(),
                    "",
                    None,
                    None,
                    None,
                    "S-0123-4567",
                    WatermarkAccessType::ViewOnly,
                    Vec::new(),
                    1_704_110_400 + index,
                )
            })
            .collect::<Vec<_>>();
        let text = watermark_text_with(&sessions, &english).unwrap();
        assert_eq!(text.lines().count(), 4);
        assert!(text.ends_with("+2 more"));
    }

    #[test]
    fn show_decision_requires_option_and_active_session() {
        let sessions = vec![session(
            "111",
            "Alice",
            None,
            None,
            None,
            "S-0123-4567",
            WatermarkAccessType::ViewOnly,
            Vec::new(),
            1_704_110_400,
        )];
        assert!(!should_show(false, &sessions));
        assert!(!should_show(true, &[]));
        assert!(should_show(true, &sessions));
    }

    #[test]
    fn controller_identity_is_requested_for_either_watermark_option() {
        assert!(should_request_controller_identity(true, false, true, true));
        assert!(should_request_controller_identity(false, true, true, true));
        assert!(!should_request_controller_identity(
            false, false, true, true
        ));
        assert!(!should_request_controller_identity(true, true, false, true));
        assert!(!should_request_controller_identity(true, true, true, false));
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
