// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

pub(super) fn update(text: Option<&str>) {
    if is_wayland() {
        wayland::update(text);
    } else {
        x11_overlay::update(text);
    }
}

fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE")
        .map(|value| value.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

mod wayland {
    use gtk::{gdk, glib, prelude::*, Label, Window, WindowType};
    use std::cell::RefCell;

    thread_local! {
        static WINDOWS: RefCell<Vec<Window>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn update(text: Option<&str>) {
        let text = text.map(str::to_owned);
        glib::MainContext::default().invoke(move || apply(text.as_deref()));
    }

    fn apply(text: Option<&str>) {
        WINDOWS.with(|windows| {
            for window in windows.borrow_mut().drain(..) {
                // SAFETY: these windows are created here and referenced only by this list.
                unsafe { window.destroy() };
            }

            let Some(text) = text else {
                return;
            };
            let Some(display) = gdk::Display::default() else {
                return;
            };
            for index in 0..display.n_monitors() {
                let Some(monitor) = display.monitor(index) else {
                    continue;
                };
                let geometry = monitor.geometry();
                let window = Window::new(WindowType::Toplevel);
                window.set_title("ClantoDesk host watermark");
                window.set_decorated(false);
                window.set_resizable(false);
                window.set_keep_above(true);
                window.set_accept_focus(false);
                window.set_focus_on_map(false);
                window.set_skip_taskbar_hint(true);
                window.set_skip_pager_hint(true);
                window.set_type_hint(gdk::WindowTypeHint::Notification);
                let height = 24 + 8;
                window.set_default_size(geometry.width(), height);
                window.move_(geometry.x(), geometry.y());
                window.stick();
                window.set_opacity(0.55);
                // The local user must not be able to close it (e.g. Alt+F4).
                window.connect_delete_event(|_, _| glib::Propagation::Stop);

                let label = Label::new(Some(text));
                label.set_margin_start(12);
                label.set_margin_end(12);
                label.set_xalign(0.5);
                label.set_yalign(0.5);
                window.add(&label);
                window.show_all();
                windows.borrow_mut().push(window);
            }
        });
    }
}

mod x11_overlay {
    use std::{
        ffi::{c_int, c_uint, c_ulong, c_void, CString},
        mem::zeroed,
        ptr::{null, null_mut},
        sync::{mpsc, Mutex},
        time::Duration,
    };
    use x11::{xinerama, xlib};

    const SHAPE_INPUT: c_int = 2;
    const SHAPE_SET: c_int = 0;
    const UNSORTED: c_int = 0;
    const WATERMARK_LINE_HEIGHT: c_uint = 24;
    const WATERMARK_VERTICAL_PADDING: c_uint = 8;

    #[link(name = "Xext")]
    extern "C" {
        fn XShapeCombineRectangles(
            display: *mut xlib::Display,
            destination: xlib::Window,
            destination_kind: c_int,
            x_offset: c_int,
            y_offset: c_int,
            rectangles: *mut xlib::XRectangle,
            rectangle_count: c_int,
            operation: c_int,
            ordering: c_int,
        );
    }

    lazy_static::lazy_static! {
        static ref COMMAND_SENDER: Mutex<Option<mpsc::Sender<Option<String>>>> = Mutex::new(None);
    }

    pub(super) fn update(text: Option<&str>) {
        let command = text.map(str::to_owned);
        let Ok(mut sender) = COMMAND_SENDER.lock() else {
            return;
        };
        if sender.is_none() {
            if command.is_none() {
                return;
            }
            let (tx, rx) = mpsc::channel();
            if let Err(err) = std::thread::Builder::new()
                .name("host-watermark-x11".to_owned())
                .spawn(move || window_thread(rx))
            {
                hbb_common::throttled_log!(
                    Duration::from_secs(60),
                    error,
                    "Failed to start the X11 host watermark thread: {err}"
                );
                return;
            }
            *sender = Some(tx);
        }
        if sender
            .as_ref()
            .is_some_and(|sender| sender.send(command.clone()).is_err())
        {
            *sender = None;
            drop(sender);
            if command.is_some() {
                update(command.as_deref());
            }
        }
    }

    fn window_thread(rx: mpsc::Receiver<Option<String>>) {
        unsafe {
            let display = xlib::XOpenDisplay(null());
            if display.is_null() {
                hbb_common::throttled_log!(
                    Duration::from_secs(60),
                    error,
                    "Failed to open the X11 display for the host watermark"
                );
                return;
            }
            let screen = xlib::XDefaultScreen(display);
            let root = xlib::XRootWindow(display, screen);
            let gc = xlib::XCreateGC(display, root, 0, null_mut());
            xlib::XSetForeground(display, gc, xlib::XWhitePixel(display, screen));
            let mut missing_charsets = null_mut();
            let mut missing_charset_count = 0;
            let mut default_string = null_mut();
            let font_set = xlib::XCreateFontSet(
                display,
                b"-*-*-medium-r-normal--14-*-*-*-*-*-*-*\0".as_ptr().cast(),
                &mut missing_charsets,
                &mut missing_charset_count,
                &mut default_string,
            );
            if !missing_charsets.is_null() {
                xlib::XFreeStringList(missing_charsets);
            }
            if font_set.is_null() {
                hbb_common::throttled_log!(
                    Duration::from_secs(60),
                    warn,
                    "Failed to create an X11 font set for the host watermark; using ASCII fallback"
                );
            }
            let mut windows = Vec::new();
            let mut text = String::new();

            loop {
                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(Some(new_text)) => {
                        text = new_text;
                        recreate_windows(display, screen, &mut windows, watermark_height(&text));
                    }
                    Ok(None) => destroy_windows(display, &mut windows),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }

                for &window in &windows {
                    xlib::XRaiseWindow(display, window);
                    xlib::XClearWindow(display, window);
                    if let Some(line) = text.lines().next() {
                        let fallback = font_set.is_null().then(|| ascii_fallback(line));
                        let bytes = fallback.as_deref().unwrap_or(line).as_bytes();
                        let length = bytes.len().min(c_int::MAX as usize) as c_int;
                        let baseline = 20;
                        if font_set.is_null() {
                            xlib::XDrawString(
                                display,
                                window,
                                gc,
                                12,
                                baseline,
                                bytes.as_ptr().cast(),
                                length,
                            );
                        } else {
                            xlib::Xutf8DrawString(
                                display,
                                window,
                                font_set,
                                gc,
                                12,
                                baseline,
                                bytes.as_ptr().cast(),
                                length,
                            );
                        }
                    }
                }
                xlib::XFlush(display);
            }

            destroy_windows(display, &mut windows);
            if !font_set.is_null() {
                xlib::XFreeFontSet(display, font_set);
            }
            xlib::XFreeGC(display, gc);
            xlib::XCloseDisplay(display);
        }
    }

    fn ascii_fallback(text: &str) -> String {
        text.chars()
            .map(|character| {
                if character.is_ascii_graphic() || character == ' ' {
                    character
                } else {
                    '?'
                }
            })
            .collect()
    }

    fn watermark_height(_text: &str) -> c_uint {
        WATERMARK_LINE_HEIGHT + WATERMARK_VERTICAL_PADDING
    }

    unsafe fn recreate_windows(
        display: *mut xlib::Display,
        screen: c_int,
        windows: &mut Vec<xlib::Window>,
        height: c_uint,
    ) {
        destroy_windows(display, windows);
        let root = xlib::XRootWindow(display, screen);
        let mut count = 0;
        let screens = if xinerama::XineramaIsActive(display) != 0 {
            xinerama::XineramaQueryScreens(display, &mut count)
        } else {
            null_mut()
        };
        let monitors = if !screens.is_null() && count > 0 {
            std::slice::from_raw_parts(screens, count as usize)
                .iter()
                .map(|info| {
                    (
                        info.x_org as c_int,
                        info.y_org as c_int,
                        info.width.max(1) as c_uint,
                    )
                })
                .collect::<Vec<_>>()
        } else {
            vec![(0, 0, xlib::XDisplayWidth(display, screen).max(1) as c_uint)]
        };
        if !screens.is_null() {
            xlib::XFree(screens.cast::<c_void>());
        }

        for (x, y, width) in monitors {
            let mut attributes: xlib::XSetWindowAttributes = zeroed();
            attributes.background_pixel = xlib::XBlackPixel(display, screen);
            attributes.override_redirect = xlib::True;
            let window = xlib::XCreateWindow(
                display,
                root,
                x,
                y,
                width,
                height,
                0,
                0,
                xlib::InputOutput as c_uint,
                null_mut(),
                xlib::CWBackPixel | xlib::CWOverrideRedirect,
                &mut attributes,
            );
            if window == 0 {
                hbb_common::throttled_log!(
                    Duration::from_secs(60),
                    error,
                    "Failed to create an X11 host watermark window"
                );
                continue;
            }
            XShapeCombineRectangles(
                display,
                window,
                SHAPE_INPUT,
                0,
                0,
                null_mut(),
                0,
                SHAPE_SET,
                UNSORTED,
            );
            set_opacity(display, window);
            if let Ok(title) = CString::new("ClantoDesk host watermark") {
                xlib::XStoreName(display, window, title.as_ptr());
            }
            xlib::XMapRaised(display, window);
            windows.push(window);
        }
        xlib::XFlush(display);
    }

    unsafe fn set_opacity(display: *mut xlib::Display, window: xlib::Window) {
        let Ok(name) = CString::new("_NET_WM_WINDOW_OPACITY") else {
            return;
        };
        let Ok(cardinal_name) = CString::new("CARDINAL") else {
            return;
        };
        let property = xlib::XInternAtom(display, name.as_ptr(), xlib::False);
        let cardinal = xlib::XInternAtom(display, cardinal_name.as_ptr(), xlib::False);
        let opacity: c_ulong = 0x8CCC_CCCC;
        xlib::XChangeProperty(
            display,
            window,
            property,
            cardinal,
            32,
            xlib::PropModeReplace,
            (&opacity as *const c_ulong).cast(),
            1,
        );
    }

    unsafe fn destroy_windows(display: *mut xlib::Display, windows: &mut Vec<xlib::Window>) {
        for window in windows.drain(..) {
            xlib::XDestroyWindow(display, window);
        }
        xlib::XFlush(display);
    }
}
