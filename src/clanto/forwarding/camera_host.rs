// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Host: riceve la webcam del controller e la scrive sulla camera virtuale.
//! Una sola camera per host: la camera virtuale è una sola. Decodifica in un
//! thread proprio, così il ciclo della connessione non aspetta il codec.

use super::{
    camera_sink,
    engine::{wrap, Out},
    host::{audit, audit_denied, Peer},
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
    time::{Duration, Instant},
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

/// Tempo per cui si aspetta che la camera virtuale di un flusso appena chiuso
/// si liberi (Stop e subito Avvia).
const BUSY_WAIT: Duration = Duration::from_secs(3);

impl Busy {
    fn acquire_within(wait: Duration) -> Option<Self> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(b) = Self::acquire() {
                return Some(b);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Larghezza e altezza dichiarate da un keyframe VP8 (RFC 6386, 9.1), senza
/// decodificarlo: un peer non può far allocare al decoder immagini enormi.
fn vp8_keyframe_size(data: &[u8]) -> Option<(usize, usize)> {
    if data.len() < 10 || data[0] & 1 != 0 || data[3..6] != [0x9d, 0x01, 0x2a] {
        return None;
    }
    let w = u16::from_le_bytes([data[6], data[7]]) & 0x3fff;
    let h = u16::from_le_bytes([data[8], data[9]]) & 0x3fff;
    Some((w as usize, h as usize))
}

pub fn on_open(peer: &Peer, out: Out, msg: ClantoMediaOpen) {
    let denied = |reason: String| {
        log::warn!("controller camera from {} denied: {reason}", peer.id);
        audit_denied(
            "clanto_forward_camera_denied",
            peer,
            json!({"stream": msg.name, "width": msg.width, "height": msg.height, "fps": msg.fps, "reason": reason}),
        );
        reply(&out, msg.stream_id, Err(reason));
    };
    if let Err(e) = super::host::allowed(peer.conn_id).and_then(|_| policy::authorize_camera()) {
        return denied(e);
    }
    let (w, h) = match validate(&msg) {
        Ok(size) => size,
        Err(e) => return denied(e),
    };
    let (tx, rx) = mpsc::sync_channel::<ClantoMediaFrame>(MAX_QUEUED);
    {
        let mut guard = STREAMS.lock().unwrap();
        let streams = guard.get_or_insert_with(HashMap::new);
        if streams.contains_key(&peer.conn_id) {
            drop(guard);
            return denied("Camera already active in this session".to_owned());
        }
        streams.insert(peer.conn_id, Stream { stream_id: msg.stream_id, tx, out: out.clone() });
    }
    // Camera virtuale e decoder in un thread proprio: la creazione (registro,
    // Media Foundation, CoreMediaIO) può richiedere tempo e inizializza COM, e
    // non deve farlo il thread della connessione.
    let (peer, conn_id, stream_id, fps) = (peer.clone(), peer.conn_id, msg.stream_id, msg.fps);
    let name = msg.name.chars().take(128).collect::<String>();
    let spawned = std::thread::Builder::new().name("clanto-camera-recv".to_owned()).spawn(move || {
        let opened = Busy::acquire_within(BUSY_WAIT)
            .ok_or_else(|| "The virtual camera is in use by another session".to_owned())
            .and_then(|busy| camera_sink::open(w, h).map(|sink| (busy, sink)));
        let (_busy, sink) = match opened {
            Ok(v) => v,
            Err(reason) => {
                log::warn!("controller camera from {} denied: {reason}", peer.id);
                remove_stream(conn_id, stream_id);
                audit_denied(
                    "clanto_forward_camera_denied",
                    &peer,
                    json!({"stream": name, "width": w, "height": h, "fps": fps, "reason": reason}),
                );
                reply(&out, stream_id, Err(reason));
                return;
            }
        };
        let sink_name = sink.name();
        audit(
            "clanto_forward_camera_opened",
            &peer,
            json!({"stream": name, "width": w, "height": h, "fps": fps, "sink": sink_name}),
        );
        reply(&out, stream_id, Ok(sink_name));
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
        remove_stream(conn_id, stream_id);
        audit(
            "clanto_forward_camera_closed",
            &peer,
            json!({"frames": frames, "seconds": started.elapsed().as_secs(), "reason": error.unwrap_or_default()}),
        );
    });
    if let Err(e) = spawned {
        log::error!("camera receiver thread: {e}");
        remove_stream(conn_id, stream_id);
    }
}

/// Il flusso può essere già stato sostituito: si toglie solo il proprio.
fn remove_stream(conn_id: i32, stream_id: i32) {
    if let Some(streams) = STREAMS.lock().unwrap().as_mut() {
        if streams.get(&conn_id).is_some_and(|s| s.stream_id == stream_id) {
            streams.remove(&conn_id);
        }
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
        if frame.key {
            match vp8_keyframe_size(&frame.data) {
                Some(dims) if dims == size => {}
                Some(_) => return (frames, Some("The camera changed resolution".to_owned())),
                None => return (frames, Some("Invalid camera frame".to_owned())),
            }
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
    // Frame troppo grande, coda piena (il controller ignora il credito) o
    // ricevitore finito: il flusso si chiude.
    let rejected = if msg.data.len() > MAX_FRAME_BYTES {
        Some("Camera frame too large")
    } else if stream.tx.try_send(msg).is_err() {
        Some("Camera stream overflow")
    } else {
        None
    };
    if let Some(reason) = rejected {
        if let Some(s) = streams.remove(&conn_id) {
            (s.out)(wrap(clanto_forward::Union::MediaClose(ClantoMediaClose {
                stream_id,
                message: reason.to_owned(),
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
        assert!(Busy::acquire_within(Duration::from_millis(120)).is_none());
        // Liberata durante l'attesa (Stop e subito Avvia): la si ottiene.
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(a);
        });
        assert!(Busy::acquire_within(Duration::from_secs(2)).is_some());
        release.join().unwrap();
    }

    #[test]
    fn keyframe_size_is_read_before_decoding() {
        use super::super::media::{Encoder, Source, TestPattern};
        let frame = TestPattern::new(320, 240).next().unwrap().unwrap();
        let packets = Encoder::new(320, 240, 15).unwrap().encode(&frame, 0).unwrap();
        let (data, key) = &packets[0];
        assert!(key);
        assert_eq!(vp8_keyframe_size(data), Some((320, 240)));
        let mut forged = data.clone();
        forged[6..10].copy_from_slice(&[0xff, 0x3f, 0xff, 0x3f]);
        assert_eq!(vp8_keyframe_size(&forged), Some((16383, 16383)));
        assert_eq!(vp8_keyframe_size(&data[..5]), None);
    }
}
