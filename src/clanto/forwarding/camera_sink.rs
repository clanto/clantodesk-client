// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Host: dove finisce la webcam del controller. Una camera virtuale che le
//! applicazioni di riunione vedono come una webcam qualsiasi.
//!
//! - Linux: un dispositivo v4l2loopback già presente (non lo installiamo noi).
//! - Windows 11: camera virtuale di Media Foundation (`libs/clanto-vcam`), con
//!   ClantoDesk installato. La crea sempre il servizio (registra la media
//!   source in HKLM): sull'host direttamente, sul PC del tecnico su richiesta
//!   IPC dell'interfaccia, che poi scrive i frame nella memoria condivisa.
//! - macOS 13+: Camera Extension (flutter/macos/CameraExtension), attivata
//!   dall'utente dalle impostazioni; l'host scrive nel suo stream sink.

use super::media::I420;

pub const NO_SINK: &str = "No virtual camera available on this device";

pub trait Sink: Send {
    fn name(&self) -> String;
    fn write(&mut self, frame: &I420) -> Result<(), String>;
}

pub fn open(width: usize, height: usize) -> Result<Box<dyn Sink>, String> {
    // Solo nelle build di debug e senza dispositivo scelto: la prova E2E non ha
    // una camera virtuale.
    if cfg!(debug_assertions)
        && std::env::var("CLANTO_E2E_CAMERA_SINK").as_deref() == Ok("null")
        && hbb_common::config::Config::get_option(super::policy::OPT_CAMERA_DEVICE).is_empty()
    {
        return Ok(Box::new(NullSink { width, height }));
    }
    platform_open(width, height)
}

struct NullSink {
    width: usize,
    height: usize,
}

impl Sink for NullSink {
    fn name(&self) -> String {
        "null".to_owned()
    }

    fn write(&mut self, frame: &I420) -> Result<(), String> {
        if (frame.width, frame.height) != (self.width, self.height) || frame.data.len() != I420::len(self.width, self.height) {
            return Err("frame size mismatch".to_owned());
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn platform_open(width: usize, height: usize) -> Result<Box<dyn Sink>, String> {
    let device = hbb_common::config::Config::get_option(super::policy::OPT_CAMERA_DEVICE);
    v4l2::Loopback::open(device.trim(), width, height).map(|s| Box::new(s) as Box<dyn Sink>)
}

#[cfg(windows)]
fn vcam_dll() -> Result<std::path::PathBuf, String> {
    // La DLL registrata in HKLM è quella della cartella d'installazione (protetta),
    // non quella accanto a un eseguibile qualsiasi.
    if !crate::platform::is_installed() {
        return Err(NO_SINK.to_owned());
    }
    let (_, install_dir, _, _) = crate::platform::windows::get_install_info();
    Ok(std::path::Path::new(&install_dir).join(clanto_vcam::DLL_NAME))
}

#[cfg(windows)]
fn platform_open(width: usize, height: usize) -> Result<Box<dyn Sink>, String> {
    let target = if crate::platform::is_root() {
        VcamTarget::Own(clanto_vcam::control::VirtualCamera::start(&vcam_dll()?, false)?)
    } else {
        vcam_dll()?;
        VcamTarget::Service(ServiceCamera::open()?)
    };
    Ok(Box::new(WindowsSink { target, width, height, buf: Vec::new() }))
}

#[cfg(windows)]
enum VcamTarget {
    /// Questo processo è il servizio: camera e memoria sono sue.
    Own(clanto_vcam::control::VirtualCamera),
    /// Interfaccia del tecnico: camera del servizio, frame scritti qui.
    Service(ServiceCamera),
}

#[cfg(windows)]
struct WindowsSink {
    target: VcamTarget,
    width: usize,
    height: usize,
    buf: Vec<u8>,
}

#[cfg(windows)]
impl Sink for WindowsSink {
    fn name(&self) -> String {
        clanto_vcam::FRIENDLY_NAME.to_owned()
    }

    fn write(&mut self, frame: &I420) -> Result<(), String> {
        if (frame.width, frame.height) != (self.width, self.height) {
            return Err("frame size mismatch".to_owned());
        }
        frame.to_nv12(&mut self.buf);
        let ok = match &self.target {
            VcamTarget::Own(camera) => camera.write(frame.width, frame.height, &self.buf),
            VcamTarget::Service(camera) => camera.mapping.view.write(frame.width, frame.height, &self.buf),
        };
        if !ok {
            return Err("Unsupported camera format".to_owned());
        }
        Ok(())
    }
}

#[cfg(windows)]
const SERVICE_UNAVAILABLE: &str = "The ClantoDesk service did not start the virtual camera";

/// Camera virtuale tenuta dal servizio per l'interfaccia: un thread tiene
/// aperta la connessione IPC, chiuderla (drop di `stop`) spegne la camera.
#[cfg(windows)]
struct ServiceCamera {
    mapping: clanto_vcam::shared::Mapping,
    _stop: std::sync::mpsc::Sender<()>,
}

#[cfg(windows)]
impl ServiceCamera {
    fn open() -> Result<Self, String> {
        use crate::ipc::Data;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        std::thread::Builder::new()
            .name("clanto-vcam-ipc".to_owned())
            .spawn(move || {
                let runtime = match hbb_common::tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                runtime.block_on(async move {
                    let mut conn = match crate::ipc::connect(1000, "").await {
                        Ok(c) => c,
                        Err(e) => {
                            hbb_common::log::warn!("virtual camera: service not reachable: {e}");
                            let _ = ready_tx.send(Err(SERVICE_UNAVAILABLE.to_owned()));
                            return;
                        }
                    };
                    if conn.send(&Data::ClantoVcamStart).await.is_err() {
                        let _ = ready_tx.send(Err(SERVICE_UNAVAILABLE.to_owned()));
                        return;
                    }
                    match conn.next_timeout(15_000).await {
                        Ok(Some(Data::ClantoVcamStarted(error))) if error.is_empty() => {
                            let _ = ready_tx.send(Ok(()));
                        }
                        Ok(Some(Data::ClantoVcamStarted(error))) => {
                            let _ = ready_tx.send(Err(error));
                            return;
                        }
                        _ => {
                            let _ = ready_tx.send(Err(SERVICE_UNAVAILABLE.to_owned()));
                            return;
                        }
                    }
                    // Fino al drop di ServiceCamera.
                    let _ = stop_rx.recv();
                    let _ = conn.send(&Data::ClantoVcamStop).await;
                });
            })
            .map_err(|e| e.to_string())?;
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .map_err(|_| SERVICE_UNAVAILABLE.to_owned())??;
        let mapping = clanto_vcam::shared::Mapping::open_write()?;
        Ok(Self { mapping, _stop: stop_tx })
    }
}

/// Servizio: camera virtuale per l'interfaccia del tecnico, attiva finché la
/// connessione IPC resta aperta o arriva `ClantoVcamStop`.
#[cfg(windows)]
pub async fn serve_ipc(stream: &mut crate::ipc::Connection) {
    use crate::ipc::Data;
    use hbb_common::tokio::task::spawn_blocking;
    let started = spawn_blocking(|| {
        clanto_vcam::control::VirtualCamera::start(&vcam_dll()?, true)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    let error = started.as_ref().err().cloned().unwrap_or_default();
    if !error.is_empty() {
        hbb_common::log::warn!("virtual camera for the interface: {error}");
    }
    let replied = stream.send(&Data::ClantoVcamStarted(error)).await.is_ok();
    let Ok(camera) = started else {
        return;
    };
    if replied {
        loop {
            match stream.next().await {
                Ok(Some(Data::ClantoVcamStop)) | Ok(None) | Err(_) => break,
                Ok(Some(_)) => {}
            }
        }
    }
    // Il drop rimuove la camera: chiamate COM bloccanti, fuori dal runtime.
    let _ = spawn_blocking(move || drop(camera)).await;
}

#[cfg(target_os = "macos")]
fn platform_open(width: usize, height: usize) -> Result<Box<dyn Sink>, String> {
    let camera = clanto_vcam::mac::CameraSink::open(clanto_vcam::FRIENDLY_NAME)?;
    Ok(Box::new(MacSink { camera, width, height, buf: Vec::new() }))
}

#[cfg(target_os = "macos")]
struct MacSink {
    camera: clanto_vcam::mac::CameraSink,
    width: usize,
    height: usize,
    buf: Vec<u8>,
}

#[cfg(target_os = "macos")]
impl Sink for MacSink {
    fn name(&self) -> String {
        clanto_vcam::FRIENDLY_NAME.to_owned()
    }

    fn write(&mut self, frame: &I420) -> Result<(), String> {
        if (frame.width, frame.height) != (self.width, self.height) {
            return Err("frame size mismatch".to_owned());
        }
        frame.to_nv12(&mut self.buf);
        self.camera.write(frame.width, frame.height, &self.buf)
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn platform_open(_width: usize, _height: usize) -> Result<Box<dyn Sink>, String> {
    Err(NO_SINK.to_owned())
}

#[cfg(target_os = "linux")]
mod v4l2 {
    use super::{Sink, I420, NO_SINK};
    use hbb_common::libc;
    use std::{
        fs::{File, OpenOptions},
        io::Write,
        os::fd::AsRawFd,
    };

    const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
    const V4L2_FIELD_NONE: u32 = 1;
    const V4L2_COLORSPACE_SRGB: u32 = 8;
    const V4L2_PIX_FMT_YUYV: u32 = fourcc(b"YUYV");
    const LOOPBACK_DRIVER: &[u8] = b"v4l2 loopback";

    const fn fourcc(c: &[u8; 4]) -> u32 {
        (c[0] as u32) | (c[1] as u32) << 8 | (c[2] as u32) << 16 | (c[3] as u32) << 24
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct Capability {
        pub driver: [u8; 16],
        pub card: [u8; 32],
        pub bus_info: [u8; 32],
        pub version: u32,
        pub capabilities: u32,
        pub device_caps: u32,
        pub reserved: [u32; 3],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct PixFormat {
        pub width: u32,
        pub height: u32,
        pub pixelformat: u32,
        pub field: u32,
        pub bytesperline: u32,
        pub sizeimage: u32,
        pub colorspace: u32,
        pub priv_: u32,
        pub flags: u32,
        pub ycbcr_enc: u32,
        pub quantization: u32,
        pub xfer_func: u32,
    }

    /// L'unione del kernel contiene puntatori: stesso allineamento qui.
    #[repr(C)]
    pub union FmtUnion {
        pub pix: PixFormat,
        pub raw: [u8; 200],
        _align: *mut libc::c_void,
    }

    #[repr(C)]
    pub struct Format {
        pub type_: u32,
        pub fmt: FmtUnion,
    }

    const fn ioc(dir: u64, nr: u64, size: usize) -> u64 {
        (dir << 30) | ((size as u64) << 16) | ((b'V' as u64) << 8) | nr
    }
    pub const VIDIOC_QUERYCAP: u64 = ioc(2, 0, std::mem::size_of::<Capability>());
    pub const VIDIOC_S_FMT: u64 = ioc(3, 5, std::mem::size_of::<Format>());

    fn driver(file: &File) -> Option<Vec<u8>> {
        let mut cap = Capability::default();
        let ret = unsafe { libc::ioctl(file.as_raw_fd(), VIDIOC_QUERYCAP as _, &mut cap as *mut Capability) };
        (ret == 0).then(|| cap.driver.iter().copied().take_while(|&b| b != 0).collect())
    }

    fn open_loopback(path: &str) -> Option<File> {
        let file = OpenOptions::new().write(true).open(path).ok()?;
        (driver(&file)? == LOOPBACK_DRIVER).then_some(file)
    }

    pub struct Loopback {
        file: File,
        path: String,
        buf: Vec<u8>,
    }

    impl Loopback {
        /// `device` vuoto: il primo v4l2loopback in /dev.
        pub fn open(device: &str, width: usize, height: usize) -> Result<Self, String> {
            let (file, path) = if device.is_empty() {
                let mut nodes: Vec<String> = std::fs::read_dir("/sys/class/video4linux")
                    .map(|d| d.filter_map(|e| e.ok()).map(|e| format!("/dev/{}", e.file_name().to_string_lossy())).collect())
                    .unwrap_or_default();
                nodes.sort_by_key(|p| p.trim_start_matches("/dev/video").parse::<u32>().unwrap_or(u32::MAX));
                nodes
                    .into_iter()
                    .find_map(|p| open_loopback(&p).map(|f| (f, p)))
                    .ok_or_else(|| NO_SINK.to_owned())?
            } else {
                let file = open_loopback(device).ok_or_else(|| format!("{device} is not a v4l2loopback device"))?;
                (file, device.to_owned())
            };
            let mut fmt = Format {
                type_: V4L2_BUF_TYPE_VIDEO_OUTPUT,
                fmt: FmtUnion { raw: [0; 200] },
            };
            fmt.fmt.pix = PixFormat {
                width: width as _,
                height: height as _,
                pixelformat: V4L2_PIX_FMT_YUYV,
                field: V4L2_FIELD_NONE,
                bytesperline: (width * 2) as _,
                sizeimage: (width * height * 2) as _,
                colorspace: V4L2_COLORSPACE_SRGB,
                ..Default::default()
            };
            let ret = unsafe { libc::ioctl(file.as_raw_fd(), VIDIOC_S_FMT as _, &mut fmt as *mut Format) };
            if ret != 0 {
                return Err(format!("{path}: format not accepted: {}", std::io::Error::last_os_error()));
            }
            Ok(Self { file, path, buf: Vec::new() })
        }
    }

    impl Sink for Loopback {
        fn name(&self) -> String {
            self.path.clone()
        }

        fn write(&mut self, frame: &I420) -> Result<(), String> {
            frame.to_yuy2(&mut self.buf);
            self.file.write_all(&self.buf).map_err(|e| format!("{}: {e}", self.path))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn kernel_abi_matches() {
            assert_eq!(std::mem::size_of::<Capability>(), 104);
            assert_eq!(VIDIOC_QUERYCAP, 0x8068_5600);
            if cfg!(target_pointer_width = "64") {
                assert_eq!(std::mem::size_of::<Format>(), 208);
                assert_eq!(VIDIOC_S_FMT, 0xc0d0_5605);
            }
            assert_eq!(V4L2_PIX_FMT_YUYV, 0x5659_5559);
        }

        #[test]
        fn missing_device_is_reported() {
            assert!(Loopback::open("/dev/null", 640, 480).is_err());
        }
    }
}
