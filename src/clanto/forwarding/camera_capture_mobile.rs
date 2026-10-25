// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Android e iOS: la webcam del telefono come webcam del tecnico. La cattura è
//! nativa: Camera2 in Kotlin (`it.clanto.clantodesk.clanto.ClantoCamera`, via JNI)
//! e AVFoundation in Swift (`ClantoCamera.swift`, via `clanto_camera_push_nv12`);
//! qui ogni frame diventa I420 diritto e va al flusso.
//! Stessa interfaccia di `camera_capture` desktop: `list` e `open`.

use super::media::{android420_to_i420, I420, Source};
use serde_derive::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Mutex,
    },
    time::Duration,
};

#[derive(Clone, Debug, Serialize)]
pub struct CameraInfo {
    pub index: u32,
    pub name: String,
}

/// 0 = posteriore, 1 = anteriore: Kotlin sceglie la camera con quella direzione.
pub fn list() -> Result<Vec<CameraInfo>, String> {
    Ok(vec![
        CameraInfo { index: 0, name: "Rear camera".to_owned() },
        CameraInfo { index: 1, name: "Front camera".to_owned() },
    ])
}

static FRAMES: Mutex<Option<mpsc::SyncSender<I420>>> = Mutex::new(None);
/// Un flusso è finito: Kotlin deve chiudere la camera. Prima dell'apertura i
/// frame si scartano senza chiuderla (l'interfaccia avvia Rust e Kotlin insieme).
static ENDED: AtomicBool = AtomicBool::new(false);
/// Sorgente corrente: una sorgente chiusa in ritardo non tocca quella nuova.
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub struct MobileSource {
    rx: mpsc::Receiver<I420>,
    name: String,
    generation: u64,
}

/// Prima di avviare Kotlin: i frame che arrivano prima di `open` non chiudono la camera.
pub fn prepare() {
    // Una sorgente precedente chiusa in ritardo non deve più contare come attuale.
    GENERATION.fetch_add(1, Ordering::SeqCst);
    ENDED.store(false, Ordering::SeqCst);
}

/// Prepara la ricezione; la cattura la avvia l'interfaccia (permesso e Camera2).
pub fn open(index: u32) -> Result<Box<dyn Source>, String> {
    let (tx, rx) = mpsc::sync_channel(2);
    let mut frames = FRAMES.lock().unwrap();
    *frames = Some(tx);
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    ENDED.store(false, Ordering::SeqCst);
    drop(frames);
    let name = list()?
        .into_iter()
        .find(|c| c.index == index)
        .map(|c| c.name)
        .ok_or_else(|| "Invalid camera".to_owned())?;
    Ok(Box::new(MobileSource { rx, name, generation }))
}

impl Source for MobileSource {
    fn name(&self) -> String {
        self.name.clone()
    }

    /// L'ultimo frame arrivato; i precedenti ancora in coda si scartano.
    fn next(&mut self) -> Result<Option<I420>, String> {
        let mut frame = match self.rx.recv_timeout(Duration::from_millis(200)) {
            Ok(f) => f,
            Err(mpsc::RecvTimeoutError::Timeout) => return Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("The camera was closed".to_owned()),
        };
        while let Ok(f) = self.rx.try_recv() {
            frame = f;
        }
        Ok(Some(frame))
    }
}

impl Drop for MobileSource {
    fn drop(&mut self) {
        // La cattura nativa vede `false` al frame successivo e chiude la camera.
        let mut frames = FRAMES.lock().unwrap();
        if GENERATION.load(Ordering::SeqCst) == self.generation {
            *frames = None;
            ENDED.store(true, Ordering::SeqCst);
        }
    }
}

/// Un frame YUV 4:2:0 dalla cattura nativa. `false` = nessun flusso attivo: la
/// cattura chiude la camera.
#[allow(clippy::too_many_arguments)]
fn push(y: &[u8], u: &[u8], v: &[u8], y_stride: i32, uv_stride: i32, uv_pixel_stride: i32, width: i32, height: i32, rotation: i32) -> bool {
    let Some(tx) = FRAMES.lock().unwrap().clone() else {
        return !ENDED.load(Ordering::SeqCst);
    };
    let dims = [y_stride, uv_stride, uv_pixel_stride, width, height].map(|x| usize::try_from(x).unwrap_or(0));
    if let Some(frame) = android420_to_i420(y, u, v, dims[0], dims[1], dims[2], dims[3], dims[4], rotation) {
        // Coda piena: il frame si perde, il prossimo è più recente.
        if let Err(mpsc::TrySendError::Disconnected(_)) = tx.try_send(frame) {
            return false;
        }
    }
    true
}

#[cfg(target_os = "android")]
mod android {
    use jni::{
        objects::{JByteBuffer, JClass},
        sys::{jboolean, jint, JNI_FALSE, JNI_TRUE},
        JNIEnv,
    };

    fn buffer<'a>(env: &JNIEnv, b: &JByteBuffer) -> Option<&'a [u8]> {
        let ptr = env.get_direct_buffer_address(b).ok()?;
        let len = env.get_direct_buffer_capacity(b).ok()?;
        (!ptr.is_null()).then(|| unsafe { std::slice::from_raw_parts(ptr as *const u8, len) })
    }

    /// ClantoCamera.pushFrame (Kotlin): frame YUV_420_888 di Camera2.
    #[no_mangle]
    pub extern "system" fn Java_it_clanto_clantodesk_clanto_ClantoCamera_pushFrame(
        env: JNIEnv,
        _class: JClass,
        y: JByteBuffer,
        u: JByteBuffer,
        v: JByteBuffer,
        y_stride: jint,
        uv_stride: jint,
        uv_pixel_stride: jint,
        width: jint,
        height: jint,
        rotation: jint,
    ) -> jboolean {
        let (Some(y), Some(u), Some(v)) = (buffer(&env, &y), buffer(&env, &u), buffer(&env, &v)) else {
            return JNI_TRUE;
        };
        if super::push(y, u, v, y_stride, uv_stride, uv_pixel_stride, width, height, rotation) {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    }
}

/// ClantoCamera.swift (iOS): frame NV12 di AVFoundation, già diritto. `uv` è il
/// piano CbCr interlacciato. Dichiarata in flutter/ios/Runner/ClantoCamera.h.
///
/// # Safety
/// `y` e `uv` puntano a `y_len` e `uv_len` byte validi per la durata della chiamata.
#[cfg(target_os = "ios")]
#[no_mangle]
pub unsafe extern "C" fn clanto_camera_push_nv12(
    y: *const u8,
    y_len: usize,
    y_stride: i32,
    uv: *const u8,
    uv_len: usize,
    uv_stride: i32,
    width: i32,
    height: i32,
) -> bool {
    if y.is_null() || uv.is_null() || uv_len < 2 {
        return true;
    }
    let y = std::slice::from_raw_parts(y, y_len);
    let uv = std::slice::from_raw_parts(uv, uv_len);
    push(y, uv, &uv[1..], y_stride, uv_stride, 2, width, height, 0)
}
