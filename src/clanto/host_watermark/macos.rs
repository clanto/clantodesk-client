// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use cocoa::{
    appkit::{
        NSBackingStoreType, NSColor, NSScreen, NSTextField, NSView, NSWindow,
        NSWindowCollectionBehavior, NSWindowStyleMask,
    },
    base::{id, nil, NO, YES},
    foundation::{NSArray, NSAutoreleasePool, NSInteger, NSPoint, NSRect, NSSize, NSString},
};
use dispatch::Queue;
use objc::{class, msg_send, sel, sel_impl};
use std::cell::RefCell;

const WATERMARK_LINE_HEIGHT: f64 = 24.0;
const WATERMARK_VERTICAL_PADDING: f64 = 8.0;
const NS_WINDOW_SHARING_NONE: usize = 0;
const CG_SCREEN_SAVER_WINDOW_LEVEL_KEY: i32 = 13;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    #[link_name = "CGWindowLevelForKey"]
    fn cg_window_level_for_key(key: i32) -> i32;
}

thread_local! {
    static WINDOWS: RefCell<Vec<id>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn update(text: Option<&str>) {
    let text = text.map(str::to_owned);
    Queue::main().exec_async(move || unsafe {
        apply(text.as_deref());
    });
}

unsafe fn apply(text: Option<&str>) {
    let pool = NSAutoreleasePool::new(nil);
    WINDOWS.with(|windows| {
        for window in windows.borrow_mut().drain(..) {
            window.orderOut_(nil);
            let _: () = msg_send![window, release];
        }

        let Some(text) = text else {
            return;
        };
        let height = WATERMARK_LINE_HEIGHT + WATERMARK_VERTICAL_PADDING;
        let screens = NSScreen::screens(nil);
        for index in 0..screens.count() {
            let screen = screens.objectAtIndex(index);
            let screen_frame = NSScreen::frame(screen);
            let frame = NSRect::new(
                NSPoint::new(
                    screen_frame.origin.x,
                    screen_frame.origin.y + screen_frame.size.height - height,
                ),
                NSSize::new(screen_frame.size.width, height),
            );
            let window = NSWindow::alloc(nil).initWithContentRect_styleMask_backing_defer_screen_(
                frame,
                NSWindowStyleMask::NSBorderlessWindowMask,
                NSBackingStoreType::NSBackingStoreBuffered,
                NO,
                screen,
            );
            window.setReleasedWhenClosed_(NO);
            window.setOpaque_(NO);
            window.setHasShadow_(NO);
            window.setCanHide_(NO);
            window.setHidesOnDeactivate_(NO);
            window.setIgnoresMouseEvents_(YES);
            window.setBackgroundColor_(NSColor::colorWithCalibratedRed_green_blue_alpha_(
                nil, 0.08, 0.08, 0.08, 1.0,
            ));
            window.setAlphaValue_(0.55);
            window.setLevel_(
                (cg_window_level_for_key(CG_SCREEN_SAVER_WINDOW_LEVEL_KEY) + 1) as NSInteger,
            );
            window.setCollectionBehavior_(
                NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
                    | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
                    | NSWindowCollectionBehavior::NSWindowCollectionBehaviorIgnoresCycle
                    | NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary,
            );
            let _: () = msg_send![window, setSharingType: NS_WINDOW_SHARING_NONE];

            let label_frame = NSRect::new(
                NSPoint::new(12.0, (height - 20.0) / 2.0),
                NSSize::new(frame.size.width - 24.0, 20.0),
            );
            let label = NSTextField::initWithFrame_(NSTextField::alloc(nil), label_frame);
            label.setEditable_(NO);
            let value = NSString::alloc(nil).init_str(text);
            label.setStringValue_(value);
            let _: () = msg_send![value, release];
            let _: () = msg_send![label, setSelectable: NO];
            let _: () = msg_send![label, setBezeled: NO];
            let _: () = msg_send![label, setDrawsBackground: NO];
            let _: () = msg_send![label, setUsesSingleLineMode: YES];
            let text_color =
                NSColor::colorWithCalibratedRed_green_blue_alpha_(nil, 1.0, 1.0, 1.0, 1.0);
            let _: () = msg_send![label, setTextColor: text_color];
            let font: id = msg_send![class!(NSFont), boldSystemFontOfSize: 15.0f64];
            let _: () = msg_send![label, setFont: font];
            let _: () = msg_send![label, setAlignment: 1isize];
            window.contentView().addSubview_(label);
            let _: () = msg_send![label, release];

            window.orderFrontRegardless();
            windows.borrow_mut().push(window);
        }
    });
    pool.drain();
}
