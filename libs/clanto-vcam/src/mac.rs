// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! macOS: scrive i frame nello stream "sink" della Camera Extension di ClantoDesk
//! (flutter/macos/CameraExtension), che li ripubblica come webcam. Si usa la API C
//! di CoreMediaIO: il dispositivo esiste solo dopo che l'utente ha attivato
//! l'estensione dalle impostazioni di ClantoDesk.

use crate::frame::{fit, nv12_len};
use std::{
    ffi::{c_char, c_void},
    ptr::null,
};

pub const NOT_INSTALLED: &str = "Activate the ClantoDesk camera extension in the settings (Forwarding)";
/// Formato fisso della camera dell'estensione.
pub const WIDTH: usize = 1280;
pub const HEIGHT: usize = 720;

type OSStatus = i32;
type ObjectID = u32;
type CFTypeRef = *const c_void;
type CMSimpleQueueRef = *mut c_void;

#[repr(C)]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

#[repr(C)]
struct CMSampleTimingInfo {
    duration: CMTime,
    presentation: CMTime,
    decode: CMTime,
}

const fn fourcc(c: &[u8; 4]) -> u32 {
    (c[0] as u32) << 24 | (c[1] as u32) << 16 | (c[2] as u32) << 8 | c[3] as u32
}

const SYSTEM_OBJECT: ObjectID = 1;
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const ELEMENT_MAIN: u32 = 0;
const HARDWARE_DEVICES: u32 = fourcc(b"dev#");
const OBJECT_NAME: u32 = fourcc(b"lnam");
const DEVICE_STREAMS: u32 = fourcc(b"stm#");
const STREAM_DIRECTION: u32 = fourcc(b"sdir");
/// kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange (NV12).
const PIXEL_NV12: u32 = fourcc(b"420v");
const UTF8: u32 = 0x0800_0100;
const TIME_VALID: u32 = 1;

#[link(name = "CoreMediaIO", kind = "framework")]
extern "C" {
    fn CMIOObjectGetPropertyDataSize(
        object: ObjectID,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> OSStatus;
    fn CMIOObjectGetPropertyData(
        object: ObjectID,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        used: *mut u32,
        data: *mut c_void,
    ) -> OSStatus;
    fn CMIOStreamCopyBufferQueue(
        stream: ObjectID,
        altered: Option<extern "C" fn(ObjectID, *mut c_void, *mut c_void)>,
        refcon: *mut c_void,
        queue: *mut CMSimpleQueueRef,
    ) -> OSStatus;
    fn CMIODeviceStartStream(device: ObjectID, stream: ObjectID) -> OSStatus;
    fn CMIODeviceStopStream(device: ObjectID, stream: ObjectID) -> OSStatus;
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSimpleQueueEnqueue(queue: CMSimpleQueueRef, element: *const c_void) -> OSStatus;
    fn CMSimpleQueueGetCount(queue: CMSimpleQueueRef) -> i32;
    fn CMSimpleQueueGetCapacity(queue: CMSimpleQueueRef) -> i32;
    fn CMVideoFormatDescriptionCreateForImageBuffer(
        allocator: CFTypeRef,
        image: CFTypeRef,
        out: *mut CFTypeRef,
    ) -> OSStatus;
    fn CMSampleBufferCreateForImageBuffer(
        allocator: CFTypeRef,
        image: CFTypeRef,
        data_ready: u8,
        callback: CFTypeRef,
        refcon: *mut c_void,
        format: CFTypeRef,
        timing: *const CMSampleTimingInfo,
        out: *mut CFTypeRef,
    ) -> OSStatus;
    fn CMClockGetHostTimeClock() -> CFTypeRef;
    fn CMClockGetTime(clock: CFTypeRef) -> CMTime;
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    static kCVPixelBufferIOSurfacePropertiesKey: CFTypeRef;
    fn CVPixelBufferCreate(
        allocator: CFTypeRef,
        width: usize,
        height: usize,
        format: u32,
        attributes: CFTypeRef,
        out: *mut CFTypeRef,
    ) -> i32;
    fn CVPixelBufferLockBaseAddress(buffer: CFTypeRef, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(buffer: CFTypeRef, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddressOfPlane(buffer: CFTypeRef, plane: usize) -> *mut u8;
    fn CVPixelBufferGetBytesPerRowOfPlane(buffer: CFTypeRef, plane: usize) -> usize;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    fn CFRelease(cf: CFTypeRef);
    fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFTypeRef;
}

fn address(selector: u32) -> PropertyAddress {
    PropertyAddress { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN }
}

fn ids(object: ObjectID, selector: u32) -> Vec<ObjectID> {
    let addr = address(selector);
    let mut size = 0u32;
    unsafe {
        if CMIOObjectGetPropertyDataSize(object, &addr, 0, null(), &mut size) != 0 || size == 0 {
            return Vec::new();
        }
        let mut out = vec![0 as ObjectID; size as usize / std::mem::size_of::<ObjectID>()];
        let mut used = 0u32;
        if CMIOObjectGetPropertyData(object, &addr, 0, null(), size, &mut used, out.as_mut_ptr() as *mut c_void) != 0 {
            return Vec::new();
        }
        out.truncate(used as usize / std::mem::size_of::<ObjectID>());
        out
    }
}

fn name(object: ObjectID) -> Option<String> {
    let addr = address(OBJECT_NAME);
    let mut cf: CFTypeRef = null();
    let mut used = 0u32;
    unsafe {
        let size = std::mem::size_of::<CFTypeRef>() as u32;
        if CMIOObjectGetPropertyData(object, &addr, 0, null(), size, &mut used, &mut cf as *mut CFTypeRef as *mut c_void) != 0
            || cf.is_null()
        {
            return None;
        }
        let mut buf = [0 as c_char; 256];
        let ok = CFStringGetCString(cf, buf.as_mut_ptr(), buf.len() as isize, UTF8) != 0;
        CFRelease(cf);
        ok.then(|| std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }
}

fn direction(stream: ObjectID) -> Option<u32> {
    let addr = address(STREAM_DIRECTION);
    let (mut value, mut used) = (0u32, 0u32);
    let ok = unsafe { CMIOObjectGetPropertyData(stream, &addr, 0, null(), 4, &mut used, &mut value as *mut u32 as *mut c_void) == 0 };
    ok.then_some(value)
}

/// Richiamata quando l'estensione consuma la coda: non serve, `write` controlla
/// lo spazio libero a ogni frame.
extern "C" fn queue_altered(_stream: ObjectID, _token: *mut c_void, _refcon: *mut c_void) {}

/// Stream sink della camera ClantoDesk. Al drop lo stream si ferma.
pub struct CameraSink {
    device: ObjectID,
    stream: ObjectID,
    queue: CMSimpleQueueRef,
    attributes: CFTypeRef,
    frame: Vec<u8>,
}

// Oggetti CoreMedia usati da un solo thread alla volta.
unsafe impl Send for CameraSink {}

impl CameraSink {
    pub fn open(device_name: &str) -> Result<Self, String> {
        let device = ids(SYSTEM_OBJECT, HARDWARE_DEVICES)
            .into_iter()
            .find(|&d| name(d).as_deref() == Some(device_name))
            .ok_or_else(|| NOT_INSTALLED.to_owned())?;
        let streams = ids(device, DEVICE_STREAMS);
        // Lo stream sink si chiama "<camera>.Sink" (CameraExtension/main.swift) e ha
        // direzione 0: per CoreMediaIO 1 è l'ingresso, cioè lo stream che le app
        // leggono. In mancanza, il secondo stream.
        let stream = streams
            .iter()
            .copied()
            .find(|&s| name(s).map_or(false, |n| n.ends_with(".Sink")))
            .or_else(|| streams.iter().copied().find(|&s| direction(s) == Some(0)))
            .or_else(|| streams.get(1).copied())
            .ok_or_else(|| NOT_INSTALLED.to_owned())?;
        let mut queue: CMSimpleQueueRef = std::ptr::null_mut();
        unsafe {
            let status = CMIOStreamCopyBufferQueue(stream, Some(queue_altered), std::ptr::null_mut(), &mut queue);
            if status != 0 || queue.is_null() {
                return Err(format!("CMIOStreamCopyBufferQueue: {status}"));
            }
            let status = CMIODeviceStartStream(device, stream);
            if status != 0 {
                CFRelease(queue);
                return Err(format!("CMIODeviceStartStream: {status}"));
            }
            // Buffer su IOSurface: passano al processo dell'estensione.
            let empty = CFDictionaryCreate(
                null(),
                null(),
                null(),
                0,
                &kCFTypeDictionaryKeyCallBacks as *const u8 as *const c_void,
                &kCFTypeDictionaryValueCallBacks as *const u8 as *const c_void,
            );
            let keys = [kCVPixelBufferIOSurfacePropertiesKey];
            let values = [empty];
            let attributes = CFDictionaryCreate(
                null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &kCFTypeDictionaryKeyCallBacks as *const u8 as *const c_void,
                &kCFTypeDictionaryValueCallBacks as *const u8 as *const c_void,
            );
            CFRelease(empty);
            Ok(Self { device, stream, queue, attributes, frame: vec![0; nv12_len(WIDTH, HEIGHT)] })
        }
    }

    /// Frame NV12 di qualsiasi dimensione: adattato a 1280x720 con bande nere.
    /// Se l'estensione è indietro il frame viene scartato.
    pub fn write(&mut self, width: usize, height: usize, nv12: &[u8]) -> Result<(), String> {
        if nv12.len() != nv12_len(width, height) {
            return Err("frame size mismatch".to_owned());
        }
        fit(nv12, width, height, &mut self.frame, WIDTH, HEIGHT);
        unsafe {
            if CMSimpleQueueGetCount(self.queue) >= CMSimpleQueueGetCapacity(self.queue) {
                return Ok(());
            }
            let mut pixel: CFTypeRef = null();
            let ret = CVPixelBufferCreate(null(), WIDTH, HEIGHT, PIXEL_NV12, self.attributes, &mut pixel);
            if ret != 0 || pixel.is_null() {
                return Err(format!("CVPixelBufferCreate: {ret}"));
            }
            CVPixelBufferLockBaseAddress(pixel, 0);
            let (y, uv) = self.frame.split_at(WIDTH * HEIGHT);
            for (plane, src, rows) in [(0usize, y, HEIGHT), (1, uv, HEIGHT / 2)] {
                let base = CVPixelBufferGetBaseAddressOfPlane(pixel, plane);
                let stride = CVPixelBufferGetBytesPerRowOfPlane(pixel, plane);
                for row in 0..rows {
                    std::ptr::copy_nonoverlapping(src[row * WIDTH..].as_ptr(), base.add(row * stride), WIDTH);
                }
            }
            CVPixelBufferUnlockBaseAddress(pixel, 0);
            let mut format: CFTypeRef = null();
            let status = CMVideoFormatDescriptionCreateForImageBuffer(null(), pixel, &mut format);
            if status != 0 {
                CFRelease(pixel);
                return Err(format!("CMVideoFormatDescriptionCreateForImageBuffer: {status}"));
            }
            let now = CMClockGetTime(CMClockGetHostTimeClock());
            let timing = CMSampleTimingInfo {
                duration: CMTime { value: 1, timescale: 30, flags: TIME_VALID, epoch: 0 },
                presentation: now,
                decode: CMTime { value: 0, timescale: 0, flags: 0, epoch: 0 },
            };
            let mut sample: CFTypeRef = null();
            let status = CMSampleBufferCreateForImageBuffer(null(), pixel, 1, null(), std::ptr::null_mut(), format, &timing, &mut sample);
            CFRelease(format);
            CFRelease(pixel);
            if status != 0 {
                return Err(format!("CMSampleBufferCreateForImageBuffer: {status}"));
            }
            // La coda prende il riferimento del sample: lo rilascia chi la svuota.
            let status = CMSimpleQueueEnqueue(self.queue, sample);
            if status != 0 {
                CFRelease(sample);
            }
        }
        Ok(())
    }
}

impl Drop for CameraSink {
    fn drop(&mut self) {
        unsafe {
            CMIODeviceStopStream(self.device, self.stream);
            CFRelease(self.queue);
            CFRelease(self.attributes);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fourcc_matches_core_media_io() {
        assert_eq!(super::HARDWARE_DEVICES, 0x6465_7623);
        assert_eq!(super::PIXEL_NV12, 0x3432_3076);
    }
}
