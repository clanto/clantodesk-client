// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use std::{
    mem::zeroed,
    ptr::{null, null_mut},
    sync::{mpsc, Mutex},
    time::Duration,
};
use winapi::{
    shared::{
        minwindef::{BOOL, DWORD, LPARAM, LRESULT, TRUE, UINT, WPARAM},
        windef::{HDC, HMONITOR, HWND, RECT},
    },
    um::{
        libloaderapi::GetModuleHandleW,
        wingdi::{CreateSolidBrush, DeleteObject, SetBkMode, SetTextColor, TRANSPARENT},
        winuser::{
            BeginPaint, CreateWindowExW, DefWindowProcW, DestroyWindow, DrawTextW, EndPaint,
            EnumDisplayMonitors, FillRect, GetClientRect, PeekMessageW, RegisterClassW,
            SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowPos, ShowWindow,
            TranslateMessage, UpdateWindow, CS_HREDRAW, CS_VREDRAW, DT_CENTER, DT_END_ELLIPSIS,
            DT_SINGLELINE, DT_VCENTER, HTTRANSPARENT, HWND_TOPMOST, LWA_ALPHA, MA_NOACTIVATE, MSG,
            PAINTSTRUCT, PM_REMOVE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE, WM_CLOSE,
            WM_DESTROY, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT, WNDCLASSW,
            WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
            WS_POPUP,
        },
    },
};

const WINDOW_CLASS: &str = "ClantoDeskHostWatermark";
const WATERMARK_HEIGHT: i32 = 48;
const WDA_EXCLUDEFROMCAPTURE: DWORD = 0x0000_0011;

lazy_static::lazy_static! {
    static ref COMMAND_SENDER: Mutex<Option<mpsc::Sender<Option<String>>>> = Mutex::new(None);
    static ref CURRENT_TEXT: Mutex<String> = Mutex::new(String::new());
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
            .name("host-watermark".to_owned())
            .spawn(move || window_thread(rx))
        {
            hbb_common::throttled_log!(
                Duration::from_secs(60),
                error,
                "Failed to start the Windows host watermark thread: {err}"
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
    let class = wide(WINDOW_CLASS);
    let instance = unsafe { GetModuleHandleW(null()) };
    let wnd_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: class.as_ptr(),
    };
    if unsafe { RegisterClassW(&wnd_class) } == 0 {
        hbb_common::throttled_log!(
            Duration::from_secs(60),
            error,
            "Failed to register the Windows host watermark class"
        );
        return;
    }

    let mut windows = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Some(text)) => {
                if let Ok(mut current) = CURRENT_TEXT.lock() {
                    *current = text;
                }
                recreate_windows(instance, &class, &mut windows);
            }
            Ok(None) => destroy_windows(&mut windows),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                destroy_windows(&mut windows);
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        let mut message: MSG = unsafe { zeroed() };
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                winapi::um::winuser::DispatchMessageW(&message);
            }
        }
        for &window in &windows {
            unsafe {
                SetWindowPos(
                    window,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE
                        | winapi::um::winuser::SWP_NOMOVE
                        | winapi::um::winuser::SWP_NOSIZE,
                );
            }
        }
    }
}

fn recreate_windows(
    instance: winapi::shared::minwindef::HINSTANCE,
    class: &[u16],
    windows: &mut Vec<HWND>,
) {
    destroy_windows(windows);
    let mut monitors = Vec::<RECT>::new();
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(collect_monitor),
            &mut monitors as *mut Vec<RECT> as LPARAM,
        );
    }
    for monitor in monitors {
        let width = monitor.right - monitor.left;
        let window = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                class.as_ptr(),
                null(),
                WS_POPUP,
                monitor.left,
                monitor.top,
                width,
                WATERMARK_HEIGHT,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            )
        };
        if window.is_null() {
            hbb_common::throttled_log!(
                Duration::from_secs(60),
                error,
                "Failed to create a Windows host watermark window"
            );
            continue;
        }
        unsafe {
            SetLayeredWindowAttributes(window, 0, 205, LWA_ALPHA);
            if SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE) == 0 {
                hbb_common::throttled_log!(
                    Duration::from_secs(60),
                    warn,
                    "Windows could not exclude the host watermark from capture"
                );
            }
            SetWindowPos(
                window,
                HWND_TOPMOST,
                monitor.left,
                monitor.top,
                width,
                WATERMARK_HEIGHT,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            ShowWindow(window, SW_SHOWNOACTIVATE);
            UpdateWindow(window);
        }
        windows.push(window);
    }
}

fn destroy_windows(windows: &mut Vec<HWND>) {
    for window in windows.drain(..) {
        unsafe {
            DestroyWindow(window);
        }
    }
}

unsafe extern "system" fn collect_monitor(
    _monitor: HMONITOR,
    _dc: HDC,
    rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    if !rect.is_null() && data != 0 {
        (*(data as *mut Vec<RECT>)).push(*rect);
    }
    TRUE
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CLOSE => 0,
        WM_DESTROY => 0,
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(window, &mut paint);
            let mut rect: RECT = zeroed();
            GetClientRect(window, &mut rect);
            let brush = CreateSolidBrush(0x0020_2020);
            FillRect(dc, &rect, brush);
            DeleteObject(brush as _);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, 0x00FF_FFFF);
            if let Ok(text) = CURRENT_TEXT.lock() {
                let mut text = wide(&text);
                DrawTextW(
                    dc,
                    text.as_mut_ptr(),
                    -1,
                    &mut rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                );
            }
            EndPaint(window, &paint);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
