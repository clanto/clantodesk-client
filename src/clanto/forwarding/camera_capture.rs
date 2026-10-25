// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Controller: webcam locale con nokhwa (Media Foundation, V4L2, AVFoundation).
//! Formato scelto qui: il più grande fino a 1280x720, a 15-30 fps se c'è.

use super::media::{I420, Source};
use scrap::nokhwa::{
    pixel_format::{FormatDecoder, RgbAFormat},
    query,
    utils::{ApiBackend, CameraFormat, CameraIndex, RequestedFormat, RequestedFormatType},
    Camera,
};
use serde_derive::Serialize;

const PREFERRED_MAX: (u32, u32) = (1280, 720);

#[derive(Clone, Debug, Serialize)]
pub struct CameraInfo {
    pub index: u32,
    pub name: String,
}

/// macOS chiede il permesso alla prima apertura. Su Windows nokhwa inizializza
/// COM (STA) solo nel primo thread che lo usa e rifiuta un thread già in un
/// altro modo: qui lo si prepara nello stesso modo. Va chiamata solo in thread
/// creati apposta (`list` e il thread di invio), mai in quelli di Flutter.
fn prepare_thread() -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
        // S_FALSE se nokhwa l'ha già fatto in questo thread.
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        if hr.is_err() {
            return Err(format!("Camera: COM initialization failed (0x{:08X})", hr.0 as u32));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if !scrap::nokhwa::nokhwa_check() {
            let (tx, rx) = std::sync::mpsc::channel();
            let tx = std::sync::Mutex::new(tx);
            scrap::nokhwa::nokhwa_initialize(move |granted| {
                tx.lock().unwrap().send(granted).ok();
            });
            let granted = rx.recv_timeout(std::time::Duration::from_secs(120)).unwrap_or(false);
            if !granted {
                return Err("Camera access denied: allow ClantoDesk in System Settings, Privacy & Security, Camera".to_owned());
            }
        }
    }
    Ok(())
}

/// In un thread nuovo: quello della chiamata (Flutter) può avere COM in un altro modo.
pub fn list() -> Result<Vec<CameraInfo>, String> {
    std::thread::Builder::new()
        .name("clanto-camera-list".to_owned())
        .spawn(list_on_this_thread)
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "Camera list failed".to_owned())?
}

fn list_on_this_thread() -> Result<Vec<CameraInfo>, String> {
    prepare_thread()?;
    let cameras = query(ApiBackend::Auto).map_err(|e| format!("Query cameras error: {e}"))?;
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut out: Vec<CameraInfo> = cameras
        .iter()
        .filter_map(|c| match c.index() {
            CameraIndex::Index(i) => Some(CameraInfo { index: *i, name: c.human_name() }),
            CameraIndex::String(_) => None,
        })
        .collect();
    // nokhwa su Linux può elencare due volte la stessa camera (issue 171);
    // altrove due webcam uguali hanno davvero lo stesso nome.
    #[cfg(target_os = "linux")]
    out.dedup_by(|a, b| a.name == b.name);
    Ok(out)
}

/// Il formato da chiedere alla camera, se ce n'è uno decodificabile.
pub fn choose(formats: &[CameraFormat]) -> Option<CameraFormat> {
    let decodable = |f: &&CameraFormat| RgbAFormat::FORMATS.contains(&f.format());
    let fits = |f: &&CameraFormat| f.resolution().width() <= PREFERRED_MAX.0 && f.resolution().height() <= PREFERRED_MAX.1;
    let area = |f: &CameraFormat| f.resolution().width() as u64 * f.resolution().height() as u64;
    // A parità di area: fps fra 15 e 30, poi più vicino a 30.
    let fps_rank = |f: &CameraFormat| {
        let fps = f.frame_rate();
        ((15..=30).contains(&fps), u32::MAX - fps.abs_diff(30))
    };
    formats
        .iter()
        .filter(decodable)
        .filter(fits)
        .max_by_key(|f| (area(f), fps_rank(f)))
        .or_else(|| formats.iter().filter(decodable).min_by_key(|f| area(f)))
        .copied()
}

pub struct NokhwaSource {
    camera: Camera,
    name: String,
}

pub fn open(index: u32) -> Result<Box<dyn Source>, String> {
    prepare_thread()?;
    let mut camera = Camera::new(
        CameraIndex::Index(index),
        RequestedFormat::new::<RgbAFormat>(RequestedFormatType::None),
    )
    .map_err(|e| format!("Cannot open camera {index}: {e}"))?;
    if let Some(format) = camera.compatible_camera_formats().ok().as_deref().and_then(choose) {
        if let Err(e) = camera.set_camera_requset(RequestedFormat::new::<RgbAFormat>(RequestedFormatType::Exact(format))) {
            hbb_common::log::warn!("camera {index}: format {format} not applied: {e}");
        }
    }
    camera.open_stream().map_err(|e| format!("Camera open stream error: {e}"))?;
    let name = camera.info().human_name();
    Ok(Box::new(NokhwaSource { camera, name }))
}

impl Source for NokhwaSource {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn next(&mut self) -> Result<Option<I420>, String> {
        let buffer = self.camera.frame().map_err(|e| format!("Camera frame error: {e}"))?;
        let image = buffer
            .decode_image::<RgbAFormat>()
            .map_err(|e| format!("Camera frame decode error: {e}"))?;
        I420::from_rgba(image.as_raw(), image.width() as usize, image.height() as usize).map(Some)
    }
}

impl Drop for NokhwaSource {
    fn drop(&mut self) {
        self.camera.stop_stream().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrap::nokhwa::utils::{FrameFormat, Resolution};

    fn f(w: u32, h: u32, fmt: FrameFormat, fps: u32) -> CameraFormat {
        CameraFormat::new(Resolution::new(w, h), fmt, fps)
    }

    #[test]
    fn chooses_largest_up_to_720p_with_sane_fps() {
        let formats = [
            f(1920, 1080, FrameFormat::MJPEG, 30),
            f(1280, 720, FrameFormat::MJPEG, 60),
            f(1280, 720, FrameFormat::MJPEG, 30),
            f(1280, 720, FrameFormat::YUYV, 10),
            f(640, 480, FrameFormat::YUYV, 30),
        ];
        assert_eq!(choose(&formats), Some(f(1280, 720, FrameFormat::MJPEG, 30)));
        // Solo formati grandi: il più piccolo.
        let big = [f(3840, 2160, FrameFormat::MJPEG, 30), f(1920, 1080, FrameFormat::MJPEG, 30)];
        assert_eq!(choose(&big), Some(f(1920, 1080, FrameFormat::MJPEG, 30)));
        assert_eq!(choose(&[]), None);
    }
}
