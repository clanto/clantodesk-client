// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Text clipboard of the controller through GtkClipboard.
//
// Wayland compositors without ext/zwlr data-control (e.g. ChromeOS sommelier) make
// clipboard-master and arboard fall back to X11. Xwayland only syncs selections with a
// focused X11 window, so changes are never seen and writes never reach Wayland apps.
// GtkClipboard uses the same Wayland connection as the Flutter window instead.
// Every GTK call runs on the GTK main thread through `glib::idle_add_once`.

use arboard::ClipboardData;
use clipboard_master::ClipboardHandler;
use gtk::{gdk, glib, prelude::*};
use hbb_common::log;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{mpsc, Mutex, OnceLock},
    time::Duration,
};

const GTK_TIMEOUT: Duration = Duration::from_secs(2);

static ACTIVE: OnceLock<bool> = OnceLock::new();
// Last text written by us, to not send it back to the peer.
static LAST_SET: Mutex<Option<String>> = Mutex::new(None);

pub fn use_gtk_clipboard(
    wayland_display_set: bool,
    gdk_backend_is_wayland: bool,
    data_control_available: bool,
) -> bool {
    wayland_display_set && gdk_backend_is_wayland && !data_control_available
}

// Controller process only: the server process has no GTK main loop and never calls this.
pub fn init() {
    ACTIVE.get_or_init(detect);
}

pub fn is_active() -> bool {
    ACTIVE.get().copied().unwrap_or(false)
}

fn detect() -> bool {
    let wayland_display_set = std::env::var_os("WAYLAND_DISPLAY").is_some();
    // Same probe as clipboard-master and arboard use to choose between Wayland and X11.
    let data_control_available =
        wayland_display_set && wl_clipboard_rs::utils::is_primary_selection_supported().is_ok();
    let gdk_backend_is_wayland =
        wayland_display_set && !data_control_available && gdk_backend_is_wayland();
    let active = use_gtk_clipboard(
        wayland_display_set,
        gdk_backend_is_wayland,
        data_control_available,
    );
    if active {
        log::info!("clipboard: Wayland without data-control, using GTK clipboard");
    }
    active
}

fn gdk_backend_is_wayland() -> bool {
    let (tx, rx) = mpsc::channel();
    glib::idle_add_once(move || {
        let wayland = gtk::init().is_ok()
            && gdk::Display::default()
                .map(|d| d.type_().name() == "GdkWaylandDisplay")
                .unwrap_or(false);
        let _ = tx.send(wayland);
    });
    rx.recv_timeout(GTK_TIMEOUT).unwrap_or(false)
}

// Calls `on_clipboard_ready` once connected and `on_clipboard_change` on every owner change.
pub fn start<H: ClipboardHandler + Send + 'static>(handler: H) {
    glib::idle_add_once(move || {
        if gtk::init().is_err() {
            log::error!("GTK clipboard: failed to initialize GTK");
            return;
        }
        let handler = Rc::new(RefCell::new(handler));
        let on_change = handler.clone();
        gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).connect_local(
            "owner-change",
            false,
            move |_| {
                log::trace!("GTK clipboard: owner change");
                let _ = on_change.borrow_mut().on_clipboard_change();
                None
            },
        );
        handler.borrow_mut().on_clipboard_ready();
    });
}

// Current text, or None when empty, unavailable or equal to the last text set by us
// (unless `force`). Blocks the calling thread, never the GTK main loop.
pub fn read_text(force: bool) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    glib::idle_add_once(move || {
        gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).request_text(move |_, text| {
            let _ = tx.send(text.map(str::to_owned));
        });
    });
    let text = match rx.recv_timeout(GTK_TIMEOUT) {
        Ok(text) => text?,
        Err(_) => {
            log::debug!("GTK clipboard: timeout reading text");
            return None;
        }
    };
    if text.is_empty() {
        return None;
    }
    if !force {
        let mut last = LAST_SET.lock().unwrap();
        if last.as_deref() == Some(text.as_str()) {
            return None;
        }
        *last = None;
    }
    Some(text)
}

// Sets the text of `data` if there is one and the GTK backend is active.
// Returns false when the caller must use its own path.
pub fn set_text_if_active(data: &[ClipboardData]) -> bool {
    if !is_active() {
        return false;
    }
    let Some(text) = data.iter().find_map(|c| match c {
        ClipboardData::Text(t) => Some(t.clone()),
        _ => None,
    }) else {
        return false;
    };
    *LAST_SET.lock().unwrap() = Some(text.clone());
    glib::idle_add_once(move || {
        gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).set_text(&text);
        log::debug!("GTK clipboard: text set");
    });
    true
}

#[cfg(test)]
mod tests {
    use super::use_gtk_clipboard;

    #[test]
    fn gtk_only_on_wayland_window_without_data_control() {
        assert!(use_gtk_clipboard(true, true, false));
        assert!(!use_gtk_clipboard(true, true, true));
        assert!(!use_gtk_clipboard(true, false, false));
        assert!(!use_gtk_clipboard(false, true, false));
        assert!(!use_gtk_clipboard(false, false, false));
    }
}
