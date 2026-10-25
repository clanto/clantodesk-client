// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Host: riceve la webcam del controller e la scrive sulla camera virtuale.
//! Una sola camera per host: la camera virtuale è una sola. Decodifica in un
//! thread proprio, così il ciclo della connessione non aspetta il codec.

use super::{
    camera_sink,
    engine::{wrap, Out},
    host::{audit, Peer},
    media::{size_ok, Decoder, CODEC_VP8, KIND_CAMERA, MAX_FPS, MAX_FRAME_BYTES},
    policy,
};
use base::message_proto::*;
use hbb_common::log;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Mutex,
    },
    time::Instant,
};

/// Frame in coda oltre i quali il controller non rispetta il credito.
const MAX_QUEUED: usize = 16;

static BUSY: AtomicBool = AtomicBool::new(false);

struct Stream {
    stream_id: i32,
    tx: mpsc::SyncSender<ClantoMediaFrame>,
    out: Out,
}

static STREAMS: Mutex<Option<HashMap<i32, Stream>>> = Mutex::new(None);

fn reply(out: &Out, stream_id: i32, result: Result<String, String>) {
    let (success, message, sink) = match result {
        Ok(sink) => (true, String::new(), sink),
        Err(e) => (false, e, String::new()),
    };
    out(wrap(clanto_forward::Union::MediaOpened(ClantoMediaOpened {
        stream_id,
        success,
        message,
        sink,
        ..Default::default()
    })));
}

fn validate(msg: &ClantoMediaOpen) -> Result<(usize, usize), String> {
    if msg.kind != KIND_CAMERA || msg.codec != CODEC_VP8 {
        return Err(format!("Unsupported media {} {}", msg.kind, msg.codec));
    }
    let (w, h) = (msg.width as usize, msg.height as usize);
    if !size_ok(w, h) || !(1..=MAX_FPS).contains(&msg.fps) {
        return Err("Unsupported camera format".to_owned());
    }
    Ok((w, h))
}

/// Riserva la camera virtuale; il flag torna libero al drop.
struct Busy;

impl Busy {
    fn acquire() -> Option<Self> {
        BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| Busy)
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

pub fn on_open(peer: &Peer, out: Out, msg: ClantoMediaOpen) {
    let detail = |reason: &str| {
        json!({"stream": msg.name, "width": msg.width, "height": msg.height, "fps": msg.fps, "reason": reason})
    };
    let denied = |reason: String| {
        log::warn!("controller camera from {} denied: {reason}", peer.id);
        audit("clanto_forward_camera_denied", peer, detail(&reason));
        reply(&out, msg.stream_id, Err(reason));
    };
    if let Err(e) = policy::authorize_camera() {
        return denied(e);
    }
    let (w, h) = match validate(&msg) {
        Ok(size) => size,
        Err(e) => return denied(e),
    };
    if STREAMS.lock().unwrap().as_ref().is_some_and(|s| s.contains_key(&peer.conn_id)) {
        return denied("Camera already active in this session".to_owned());
    }
    let Some(busy) = Busy::acquire() else {
        return denied("The virtual camera is in use by another session".to_owned());
    };
    let sink = match camera_sink::open(w, h) {
        Ok(sink) => sink,
        Err(e) => return denied(e),
    };
    let sink_name = sink.name();
    let (tx, rx) = mpsc::sync_channel::<ClantoMediaFrame>(MAX_QUEUED);
    STREAMS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(peer.conn_id, Stream { stream_id: msg.stream_id, tx, out: out.clone() });
    audit("clanto_forward_camera_opened", peer, json!({"stream": msg.name, "width": w, "height": h, "fps": msg.fps, "sink": sink_name}));
    reply(&out, msg.stream_id, Ok(sink_name));

    let conn_id = peer.conn_id;
    let (peer, stream_id) = (peer.clone(), msg.stream_id);
    let spawned = std::thread::Builder::new().name("clanto-camera-recv".to_owned()).spawn(move || {
        let _busy = busy;
        let started = Instant::now();
        let (frames, error) = receive(rx, sink, (w, h), &out, stream_id);
        if let Some(e) = &error {
            log::warn!("controller camera from {}: {e}", peer.id);
            out(wrap(clanto_forward::Union::MediaClose(ClantoMediaClose {
                stream_id,
                message: e.clone(),
                ..Default::default()
            })));
        }
        // Il flusso può essere già stato sostituito: si toglie solo il proprio.
        if let Some(streams) = STREAMS.lock().unwrap().as_mut() {
            if streams.get(&peer.conn_id).is_some_and(|s| s.stream_id == stream_id) {
                streams.remove(&peer.conn_id);
            }
        }
        audit(
            "clanto_forward_camera_closed",
            &peer,
            json!({"frames": frames, "seconds": started.elapsed().as_secs(), "reason": error.unwrap_or_default()}),
        );
    });
    if let Err(e) = spawned {
        log::error!("camera receiver thread: {e}");
        close(conn_id);
    }
}

fn receive(
    rx: mpsc::Receiver<ClantoMediaFrame>,
    mut sink: Box<dyn camera_sink::Sink>,
    size: (usize, usize),
    out: &Out,
    stream_id: i32,
) -> (u64, Option<String>) {
    let mut decoder = match Decoder::new() {
        Ok(d) => d,
        Err(e) => return (0, Some(e)),
    };
    let mut frames = 0u64;
    let mut waiting_key = true;
    while let Ok(frame) = rx.recv() {
        let mut keyframe = false;
        if frame.data.len() > MAX_FRAME_BYTES {
            return (frames, Some("Camera frame too large".to_owned()));
        }
        if waiting_key && !frame.key {
            keyframe = true;
        } else {
            match decoder.decode(&frame.data) {
                Ok(images) => {
                    waiting_key = false;
                    for img in images {
                        if (img.width, img.height) != size {
                            return (frames, Some("The camera changed resolution".to_owned()));
                        }
                        if let Err(e) = sink.write(&img) {
                            return (frames, Some(e));
                        }
                        frames += 1;
                    }
                }
                Err(e) => {
                    log::debug!("camera decode: {e}");
                    waiting_key = true;
                    keyframe = true;
                }
            }
        }
        out(wrap(clanto_forward::Union::MediaAck(ClantoMediaAck {
            stream_id,
            pts: frame.pts,
            keyframe,
            ..Default::default()
        })));
    }
    (frames, None)
}

pub fn on_frame(conn_id: i32, msg: ClantoMediaFrame) {
    let mut guard = STREAMS.lock().unwrap();
    let Some(streams) = guard.as_mut() else { return };
    let Some(stream) = streams.get(&conn_id).filter(|s| s.stream_id == msg.stream_id) else {
        return;
    };
    let stream_id = stream.stream_id;
    if stream.tx.try_send(msg).is_err() {
        // Coda piena (il controller ignora il credito) o ricevitore finito.
        if let Some(s) = streams.remove(&conn_id) {
            (s.out)(wrap(clanto_forward::Union::MediaClose(ClantoMediaClose {
                stream_id,
                message: "Camera stream overflow".to_owned(),
                ..Default::default()
            })));
        }
    }
}

pub fn on_close(conn_id: i32, msg: ClantoMediaClose) {
    let mut guard = STREAMS.lock().unwrap();
    if let Some(streams) = guard.as_mut() {
        if streams.get(&conn_id).is_some_and(|s| s.stream_id == msg.stream_id) {
            streams.remove(&conn_id);
        }
    }
}

/// Fine della connessione: il ricevitore esce quando la coda si chiude.
pub fn close(conn_id: i32) {
    if let Some(streams) = STREAMS.lock().unwrap().as_mut() {
        streams.remove(&conn_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::media::I420;

    fn open(w: i32, h: i32, fps: i32) -> ClantoMediaOpen {
        ClantoMediaOpen {
            stream_id: 1,
            kind: KIND_CAMERA.to_owned(),
            codec: CODEC_VP8.to_owned(),
            width: w,
            height: h,
            fps,
            ..Default::default()
        }
    }

    #[test]
    fn rejects_bad_formats() {
        assert_eq!(validate(&open(640, 480, 15)), Ok((640, 480)));
        assert_eq!(validate(&open(720, 1280, 15)), Ok((720, 1280)), "telefono in verticale");
        assert!(validate(&open(1280, 1920, 15)).is_err(), "lato corto oltre 1080");
        assert!(validate(&open(641, 480, 15)).is_err());
        assert!(validate(&open(3840, 2160, 15)).is_err());
        assert!(validate(&open(640, 480, 0)).is_err());
        assert!(validate(&open(640, 480, 120)).is_err());
        let mut m = open(640, 480, 15);
        m.codec = "h264".to_owned();
        assert!(validate(&m).is_err());
        assert_eq!(I420::len(640, 480), 640 * 480 * 3 / 2);
    }

    #[test]
    fn virtual_camera_is_exclusive() {
        let a = Busy::acquire().unwrap();
        assert!(Busy::acquire().is_none());
        drop(a);
        assert!(Busy::acquire().is_some());
    }
}
