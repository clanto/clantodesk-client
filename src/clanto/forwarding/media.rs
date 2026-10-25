// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Flussi media del forwarding: oggi la webcam del controller verso l'host.
//!
//! Il controller cattura, codifica in VP8 (codec di scrap) e invia; l'host
//! decodifica e scrive su una camera virtuale (`camera_sink`). Controllo di
//! flusso: al massimo `MAX_IN_FLIGHT` frame non confermati; se il collegamento
//! è lento il controller salta i frame prima di codificarli, così il VP8 non
//! perde riferimenti. Un errore di decodifica chiede un keyframe.

use super::engine::{wrap, Out};
use base::message_proto::*;
use hbb_common::{bytes::Bytes, log};
use scrap::{
    codec::{EncoderApi, EncoderCfg},
    GoogleImage, VpxDecoder, VpxDecoderConfig, VpxEncoder, VpxEncoderConfig, VpxVideoCodecId,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub const KIND_CAMERA: &str = "camera";
pub const CODEC_VP8: &str = "vp8";
pub const MAX_IN_FLIGHT: i64 = 3;
/// Lato lungo e lato corto massimi: vale anche in verticale (telefoni).
pub const MAX_LONG: usize = 1920;
pub const MAX_SHORT: usize = 1080;
pub const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_FPS: i32 = 15;
pub const MAX_FPS: i32 = 30;
/// Id dei flussi del controller: dispari, come i canali.
pub const CAMERA_STREAM_ID: i32 = 1;

/// Dimensioni accettate: pari, non nulle, entro 1920x1080 in orizzontale o in verticale.
pub fn size_ok(w: usize, h: usize) -> bool {
    w > 0 && h > 0 && w % 2 == 0 && h % 2 == 0 && w.max(h) <= MAX_LONG && w.min(h) <= MAX_SHORT
}
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
const NO_FRAMES_TIMEOUT: Duration = Duration::from_secs(5);

/// Immagine I420 compatta: piano Y, poi U e V a metà risoluzione.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct I420 {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl I420 {
    pub fn len(width: usize, height: usize) -> usize {
        width * height + 2 * (width / 2) * (height / 2)
    }

    pub fn planes(&self) -> (&[u8], &[u8], &[u8]) {
        let y = self.width * self.height;
        let uv = (self.width / 2) * (self.height / 2);
        (&self.data[..y], &self.data[y..y + uv], &self.data[y + uv..y + 2 * uv])
    }

    /// Da RGBA (byte R,G,B,A), con dimensioni ridotte a numeri pari.
    pub fn from_rgba(rgba: &[u8], width: usize, height: usize) -> Result<Self, String> {
        if rgba.len() < width * height * 4 {
            return Err("short RGBA frame".to_owned());
        }
        let (w, h) = (width & !1, height & !1);
        if w == 0 || h == 0 {
            return Err("empty frame".to_owned());
        }
        let mut data = vec![0u8; Self::len(w, h)];
        let (y, rest) = data.split_at_mut(w * h);
        let (u, v) = rest.split_at_mut((w / 2) * (h / 2));
        // libyuv chiama "ABGR" i byte R,G,B,A in memoria.
        let ret = unsafe {
            scrap::ABGRToI420(
                rgba.as_ptr(),
                (width * 4) as _,
                y.as_mut_ptr(),
                w as _,
                u.as_mut_ptr(),
                (w / 2) as _,
                v.as_mut_ptr(),
                (w / 2) as _,
                w as _,
                h as _,
            )
        };
        if ret != 0 {
            return Err(format!("ABGRToI420 failed: {ret}"));
        }
        Ok(Self { width: w, height: h, data })
    }

    /// YUY2 (YUYV), il formato che quasi tutte le applicazioni accettano.
    pub fn to_yuy2(&self, out: &mut Vec<u8>) {
        let (y, u, v) = self.planes();
        out.resize(self.width * self.height * 2, 0);
        unsafe {
            scrap::I420ToYUY2(
                y.as_ptr(),
                self.width as _,
                u.as_ptr(),
                (self.width / 2) as _,
                v.as_ptr(),
                (self.width / 2) as _,
                out.as_mut_ptr(),
                (self.width * 2) as _,
                self.width as _,
                self.height as _,
            );
        }
    }

    /// NV12, il formato delle camere virtuali di Windows.
    pub fn to_nv12(&self, out: &mut Vec<u8>) {
        let (y, u, v) = self.planes();
        let ylen = self.width * self.height;
        out.resize(ylen + ylen / 2, 0);
        let (dy, duv) = out.split_at_mut(ylen);
        unsafe {
            scrap::I420ToNV12(
                y.as_ptr(),
                self.width as _,
                u.as_ptr(),
                (self.width / 2) as _,
                v.as_ptr(),
                (self.width / 2) as _,
                dy.as_mut_ptr(),
                self.width as _,
                duv.as_mut_ptr(),
                self.width as _,
                self.width as _,
                self.height as _,
            );
        }
    }
}

/// YUV_420_888 (piani con stride e pixel stride) -> I420 compatto, ruotato di
/// `rotation` gradi in senso orario. `None` se i parametri non sono coerenti.
#[allow(clippy::too_many_arguments)]
pub fn android420_to_i420(
    y: &[u8],
    u: &[u8],
    v: &[u8],
    y_stride: usize,
    uv_stride: usize,
    uv_pixel_stride: usize,
    width: usize,
    height: usize,
    rotation: i32,
) -> Option<I420> {
    let (w, h) = (width & !1, height & !1);
    if w == 0 || h == 0 || !(1..=2).contains(&uv_pixel_stride) || y_stride < w {
        return None;
    }
    // Ultimo byte letto da libyuv in ogni piano.
    let uv_last = (h / 2 - 1) * uv_stride + (w / 2 - 1) * uv_pixel_stride + 1;
    if y.len() < (h - 1) * y_stride + w || u.len() < uv_last || v.len() < uv_last {
        return None;
    }
    let mut upright = vec![0u8; I420::len(w, h)];
    {
        let (dy, rest) = upright.split_at_mut(w * h);
        let (du, dv) = rest.split_at_mut((w / 2) * (h / 2));
        let ret = unsafe {
            scrap::Android420ToI420(
                y.as_ptr(),
                y_stride as _,
                u.as_ptr(),
                uv_stride as _,
                v.as_ptr(),
                uv_stride as _,
                uv_pixel_stride as _,
                dy.as_mut_ptr(),
                w as _,
                du.as_mut_ptr(),
                (w / 2) as _,
                dv.as_mut_ptr(),
                (w / 2) as _,
                w as _,
                h as _,
            )
        };
        if ret != 0 {
            return None;
        }
    }
    let mode = match rotation.rem_euclid(360) {
        90 => scrap::RotationMode::kRotate90,
        180 => scrap::RotationMode::kRotate180,
        270 => scrap::RotationMode::kRotate270,
        _ => return Some(I420 { width: w, height: h, data: upright }),
    };
    let (rw, rh) = if matches!(mode, scrap::RotationMode::kRotate180) { (w, h) } else { (h, w) };
    let mut data = vec![0u8; I420::len(rw, rh)];
    let src = I420 { width: w, height: h, data: upright };
    let (sy, su, sv) = src.planes();
    let (dy, rest) = data.split_at_mut(rw * rh);
    let (du, dv) = rest.split_at_mut((rw / 2) * (rh / 2));
    let ret = unsafe {
        scrap::I420Rotate(
            sy.as_ptr(),
            w as _,
            su.as_ptr(),
            (w / 2) as _,
            sv.as_ptr(),
            (w / 2) as _,
            dy.as_mut_ptr(),
            rw as _,
            du.as_mut_ptr(),
            (rw / 2) as _,
            dv.as_mut_ptr(),
            (rw / 2) as _,
            w as _,
            h as _,
            mode,
        )
    };
    (ret == 0).then_some(I420 { width: rw, height: rh, data })
}

/// Una sorgente video. Vive nel thread di invio: non serve che sia `Send`.
pub trait Source {
    fn name(&self) -> String;
    /// Il frame successivo; `None` = nessun frame nuovo per ora.
    fn next(&mut self) -> Result<Option<I420>, String>;
}

/// Crea la sorgente dentro il thread di invio (le camere non sono `Send`).
pub type SourceFactory = Box<dyn FnOnce() -> Result<Box<dyn Source>, String> + Send>;

/// Barre in movimento: per le prove, senza camera.
pub struct TestPattern {
    width: usize,
    height: usize,
    n: usize,
}

impl TestPattern {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width: width & !1, height: height & !1, n: 0 }
    }
}

impl Source for TestPattern {
    fn name(&self) -> String {
        "Test pattern".to_owned()
    }

    fn next(&mut self) -> Result<Option<I420>, String> {
        self.n += 1;
        let (w, h) = (self.width, self.height);
        let mut rgba = vec![0u8; w * h * 4];
        for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
            let band = ((i % w + self.n * 8) / 64) % 4;
            px.copy_from_slice(&[[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [240, 240, 240, 255]][band]);
        }
        I420::from_rgba(&rgba, w, h).map(Some)
    }
}

pub struct Encoder {
    inner: VpxEncoder,
    width: usize,
    height: usize,
    fps: i32,
}

impl Encoder {
    pub fn new(width: usize, height: usize, fps: i32) -> Result<Self, String> {
        let cfg = VpxEncoderConfig {
            width: width as _,
            height: height as _,
            quality: 1.0,
            codec: VpxVideoCodecId::VP8,
            // Un keyframe ogni 4 secondi recupera anche senza richiesta.
            keyframe_interval: Some((fps * 4) as usize),
        };
        let inner = VpxEncoder::new(EncoderCfg::VPX(cfg), false).map_err(|e| e.to_string())?;
        Ok(Self { inner, width, height, fps })
    }

    /// Ricrea il codificatore: il primo frame dopo è un keyframe.
    pub fn reset(&mut self) -> Result<(), String> {
        *self = Self::new(self.width, self.height, self.fps)?;
        Ok(())
    }

    /// Pacchetti `(dati, keyframe)`; `ms` = tempo del frame in millisecondi.
    pub fn encode(&mut self, frame: &I420, ms: i64) -> Result<Vec<(Vec<u8>, bool)>, String> {
        if frame.width != self.width || frame.height != self.height {
            return Err("frame size changed".to_owned());
        }
        let frames = self.inner.encode(ms, &frame.data, 1).map_err(|e| e.to_string())?;
        Ok(frames.map(|f| (f.data.to_vec(), f.key)).collect())
    }
}

pub struct Decoder {
    inner: VpxDecoder,
}

impl Decoder {
    pub fn new() -> Result<Self, String> {
        let inner = VpxDecoder::new(VpxDecoderConfig { codec: VpxVideoCodecId::VP8 }).map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<I420>, String> {
        let images = self.inner.decode(data).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for img in images {
            let (w, h) = (img.width(), img.height());
            if !size_ok(w, h) {
                return Err(format!("unexpected frame size {w}x{h}"));
            }
            let stride = img.stride();
            let planes = img.planes();
            let mut data = Vec::with_capacity(I420::len(w, h));
            for (p, (pw, ph)) in [(w, h), (w / 2, h / 2), (w / 2, h / 2)].into_iter().enumerate() {
                for row in 0..ph {
                    let line = unsafe { std::slice::from_raw_parts(planes[p].add(row * stride[p] as usize), pw) };
                    data.extend_from_slice(line);
                }
            }
            out.push(I420 { width: w, height: h, data });
        }
        Ok(out)
    }
}

/// Stato di un flusso in uscita, visibile all'interfaccia.
#[derive(Clone, Debug, Default, serde_derive::Serialize)]
pub struct SendInfo {
    pub state: &'static str,
    pub message: String,
    pub source: String,
    pub sink: String,
    pub width: usize,
    pub height: usize,
    pub sent: i64,
    pub acked: i64,
}

/// Flusso in uscita (controller). Si ferma con `stop` o al drop.
pub struct SendStream {
    pub stream_id: i32,
    stop: Arc<AtomicBool>,
    acked: Arc<AtomicI64>,
    keyframe: Arc<AtomicBool>,
    pub info: Arc<Mutex<SendInfo>>,
    thread: Option<JoinHandle<()>>,
    out: Out,
}

fn send(out: &Out, union: clanto_forward::Union) {
    out(wrap(union));
}

impl SendStream {
    pub fn start(stream_id: i32, fps: i32, factory: SourceFactory, out: Out) -> Self {
        let fps = fps.clamp(1, MAX_FPS);
        let stop = Arc::new(AtomicBool::new(false));
        let acked = Arc::new(AtomicI64::new(0));
        let keyframe = Arc::new(AtomicBool::new(false));
        let info = Arc::new(Mutex::new(SendInfo { state: "starting", ..Default::default() }));
        let thread = {
            let (stop, acked, keyframe, info, out) = (stop.clone(), acked.clone(), keyframe.clone(), info.clone(), out.clone());
            std::thread::Builder::new()
                .name("clanto-media-send".to_owned())
                .spawn(move || {
                    if let Err(e) = run_send(stream_id, fps, factory, &out, &stop, &acked, &keyframe, &info) {
                        log::warn!("media stream {stream_id}: {e}");
                        let mut i = info.lock().unwrap();
                        i.state = "error";
                        i.message = e.clone();
                        drop(i);
                        send(&out, clanto_forward::Union::MediaClose(ClantoMediaClose {
                            stream_id,
                            message: e,
                            ..Default::default()
                        }));
                    } else {
                        let mut i = info.lock().unwrap();
                        if matches!(i.state, "starting" | "pending" | "active") {
                            i.state = "stopped";
                        }
                    }
                })
                .ok()
        };
        Self { stream_id, stop, acked, keyframe, info, thread, out }
    }

    pub fn on_opened(&self, msg: &ClantoMediaOpened) {
        let mut i = self.info.lock().unwrap();
        if msg.success {
            i.state = "active";
            i.sink = msg.sink.clone();
        } else {
            i.state = "error";
            i.message = msg.message.clone();
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    pub fn on_ack(&self, msg: &ClantoMediaAck) {
        self.acked.fetch_max(msg.pts, Ordering::SeqCst);
        if msg.keyframe {
            self.keyframe.store(true, Ordering::SeqCst);
        }
        self.info.lock().unwrap().acked = self.acked.load(Ordering::SeqCst);
    }

    /// L'host ha chiuso il flusso.
    pub fn on_close(&self, msg: &ClantoMediaClose) {
        let mut i = self.info.lock().unwrap();
        i.state = if msg.message.is_empty() { "closed" } else { "error" };
        i.message = msg.message.clone();
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn stop(&mut self) {
        if !self.stop.swap(true, Ordering::SeqCst) {
            send(&self.out, clanto_forward::Union::MediaClose(ClantoMediaClose {
                stream_id: self.stream_id,
                ..Default::default()
            }));
        }
        // Niente join: il thread può essere fermo sull'apertura della camera o
        // sul permesso di macOS; vede `stop` ed esce da solo.
        self.thread.take();
    }
}

impl Drop for SendStream {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn run_send(
    stream_id: i32,
    fps: i32,
    factory: SourceFactory,
    out: &Out,
    stop: &AtomicBool,
    acked: &AtomicI64,
    keyframe: &AtomicBool,
    info: &Mutex<SendInfo>,
) -> Result<(), String> {
    let mut source = factory()?;
    // Il primo frame fissa le dimensioni del flusso.
    let started = Instant::now();
    let first = loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Some(f) = source.next()? {
            break f;
        }
        if started.elapsed() > OPEN_TIMEOUT {
            return Err("The camera sends no frames".to_owned());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !size_ok(first.width, first.height) {
        return Err(format!("Camera resolution {}x{} not supported", first.width, first.height));
    }
    {
        let mut i = info.lock().unwrap();
        i.state = "pending";
        i.source = source.name();
        i.width = first.width;
        i.height = first.height;
    }
    send(out, clanto_forward::Union::MediaOpen(ClantoMediaOpen {
        stream_id,
        kind: KIND_CAMERA.to_owned(),
        codec: CODEC_VP8.to_owned(),
        width: first.width as _,
        height: first.height as _,
        fps,
        name: source.name(),
        ..Default::default()
    }));
    while info.lock().unwrap().state == "pending" {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        if started.elapsed() > OPEN_TIMEOUT * 2 {
            return Err("The remote computer did not answer".to_owned());
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut encoder = Encoder::new(first.width, first.height, fps)?;
    let interval = Duration::from_millis(1000 / fps as u64);
    let t0 = Instant::now();
    let mut seq: i64 = 0;
    let mut pending = Some(first);
    let mut next_tick = Instant::now();
    let mut last_frame = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        if now < next_tick {
            std::thread::sleep(next_tick - now);
        }
        next_tick += interval;
        if next_tick + interval < Instant::now() {
            next_tick = Instant::now() + interval;
        }
        let frame = match pending.take() {
            Some(f) => f,
            None => match source.next()? {
                Some(f) => f,
                // Cattura nativa ferma (app in background, camera presa da altri).
                None if last_frame.elapsed() > NO_FRAMES_TIMEOUT => {
                    return Err("The camera stopped sending frames".to_owned())
                }
                None => continue,
            },
        };
        last_frame = Instant::now();
        // Collegamento lento: si salta prima di codificare.
        if seq - acked.load(Ordering::SeqCst) >= MAX_IN_FLIGHT {
            continue;
        }
        if frame.width != encoder.width || frame.height != encoder.height {
            return Err("The camera changed resolution".to_owned());
        }
        if keyframe.swap(false, Ordering::SeqCst) {
            encoder.reset()?;
        }
        for (data, key) in encoder.encode(&frame, t0.elapsed().as_millis() as i64)? {
            seq += 1;
            send(out, clanto_forward::Union::MediaFrame(ClantoMediaFrame {
                stream_id,
                data: Bytes::from(data),
                key,
                pts: seq,
                ..Default::default()
            }));
        }
        info.lock().unwrap().sent = seq;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_semi_planar_and_rotates() {
        // 4x2 NV21 come Camera2 su molti telefoni: U e V interlacciati, pixel stride 2.
        let y: Vec<u8> = (0..8).collect();
        let uv = [100u8, 200, 101, 201];
        let f = android420_to_i420(&y, &uv, &uv[1..], 4, 4, 2, 4, 2, 0).unwrap();
        assert_eq!((f.width, f.height), (4, 2));
        assert_eq!(f.planes().0, &y[..]);
        assert_eq!(f.planes().1, &[100, 101]);
        assert_eq!(f.planes().2, &[200, 201]);
        let r = android420_to_i420(&y, &uv, &uv[1..], 4, 4, 2, 4, 2, 90).unwrap();
        assert_eq!((r.width, r.height), (2, 4));
        assert!(android420_to_i420(&y, &uv, &uv[1..], 4, 4, 3, 4, 2, 0).is_none(), "bad pixel stride");
        assert!(android420_to_i420(&y[..4], &uv, &uv[1..], 4, 4, 2, 4, 2, 0).is_none(), "short Y plane");
    }

    #[test]
    fn vp8_roundtrip_keeps_size_and_content() {
        let mut src = TestPattern::new(320, 240);
        let mut enc = Encoder::new(320, 240, 15).unwrap();
        let mut dec = Decoder::new().unwrap();
        let mut decoded = 0;
        for i in 0..10 {
            let frame = src.next().unwrap().unwrap();
            for (data, key) in enc.encode(&frame, i * 66).unwrap() {
                assert!(i > 0 || key, "first packet is a keyframe");
                for img in dec.decode(&data).unwrap() {
                    assert_eq!((img.width, img.height), (320, 240));
                    assert_eq!(img.data.len(), I420::len(320, 240));
                    // Luma media vicina all'originale: il contenuto passa davvero.
                    let mean = |d: &[u8]| d.iter().map(|&x| x as u64).sum::<u64>() / d.len() as u64;
                    let (a, b) = (mean(frame.planes().0), mean(img.planes().0));
                    assert!(a.abs_diff(b) < 12, "luma {a} vs {b}");
                    decoded += 1;
                }
            }
        }
        assert!(decoded >= 8);
    }

    #[test]
    fn conversions_have_expected_sizes() {
        let f = TestPattern::new(64, 48).next().unwrap().unwrap();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        f.to_yuy2(&mut a);
        f.to_nv12(&mut b);
        assert_eq!(a.len(), 64 * 48 * 2);
        assert_eq!(b.len(), 64 * 48 * 3 / 2);
        assert!(I420::from_rgba(&[0; 8], 2, 2).is_err());
    }
}
