// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Lato host: un motore per connessione autorizzata, creato al primo messaggio.
//! Decide la policy dell'host; ogni canale e listener, concesso o negato, va
//! nell'audit.

use super::{
    camera_receiver::{self, Key},
    catalog::Direction,
    engine::{wrap, Engine, Hooks, Out, Side, LISTEN_HOST},
    media::{SendStream, DEFAULT_FPS},
    policy,
};
use base::message_proto::*;
use hbb_common::{futures::future::BoxFuture, log, tokio::time::Instant};
use serde_json::json;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

/// Chi c'è dall'altra parte, per l'audit.
#[derive(Clone, Debug, Default)]
pub struct Peer {
    pub conn_id: i32,
    pub id: String,
    pub name: String,
    pub ip: String,
}

struct HostHooks {
    peer: Peer,
}

pub(super) fn audit(kind: &str, peer: &Peer, extra: serde_json::Value) {
    let mut value = json!({
        "conn_id": peer.conn_id,
        "id": peer.id,
        "name": peer.name,
        "ip": peer.ip,
    });
    if let (Some(map), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
        map.extend(extra.clone());
    }
    crate::clanto::audit_sink::emit(kind, &value);
}

impl Hooks for HostHooks {
    fn authorize_open(&self, host: String, port: i32) -> BoxFuture<'static, Result<Vec<SocketAddr>, String>> {
        let peer = self.peer.clone();
        Box::pin(async move {
            if host == LISTEN_HOST {
                return Err(policy::DENIED_TARGET.to_owned());
            }
            let decision = match allowed(peer.conn_id) {
                Ok(()) => policy::authorize_target(Direction::ToHost, &host, port).await,
                Err(e) => Err(e),
            };
            match decision {
                Ok(decision) => {
                    let policy::Target::Addrs(addrs) = decision.target;
                    audit(
                        "clanto_forward_opened",
                        &peer,
                        json!({"direction": "to_host", "target": format!("{host}:{port}"), "service": decision.service}),
                    );
                    Ok(addrs)
                }
                Err(reason) => {
                    log::warn!("forwarding to {host}:{port} denied for {}: {reason}", peer.id);
                    audit_denied(
                        "clanto_forward_denied",
                        &peer,
                        json!({"direction": "to_host", "target": format!("{host}:{port}"), "reason": reason}),
                    );
                    Err(reason)
                }
            }
        })
    }

    fn authorize_listen(&self, req: &ClantoForwardListen) -> Result<(), String> {
        let result = allowed(self.peer.conn_id).and_then(|_| policy::authorize_listen());
        let detail = json!({
            "direction": "to_controller",
            "listen_port": req.port,
            "description": req.description,
            "reason": result.as_ref().err().cloned().unwrap_or_default(),
        });
        match &result {
            Ok(()) => audit("clanto_forward_listen", &self.peer, detail),
            Err(_) => audit_denied("clanto_forward_listen_denied", &self.peer, detail),
        }
        result
    }
}

/// Permessi della connessione dal profilo per ID, aggiornati a ogni messaggio:
/// `forward` (Tunnel) per la rete, `camera` (Fotocamera) per la webcam.
#[derive(Clone, Copy, Debug, Default)]
pub struct Permissions {
    pub forward: bool,
    pub camera: bool,
}

static ALLOWED: Mutex<Option<HashMap<i32, Permissions>>> = Mutex::new(None);

fn permissions(conn_id: i32) -> Permissions {
    ALLOWED.lock().unwrap().as_ref().and_then(|m| m.get(&conn_id).copied()).unwrap_or_default()
}

pub(super) fn allowed(conn_id: i32) -> Result<(), String> {
    if permissions(conn_id).forward {
        Ok(())
    } else {
        Err(policy::DENIED_PERMISSION.to_owned())
    }
}

/// Webcam (in entrambi i sensi): permesso del profilo e forwarding webcam acceso.
fn camera_allowed(conn_id: i32) -> Result<(), String> {
    if !permissions(conn_id).camera {
        return Err(policy::DENIED_PERMISSION.to_owned());
    }
    policy::authorize_camera()
}

/// Eventi di rifiuto per connessione e minuto: oltre, solo log (un peer non
/// può riempire l'audit con richieste rifiutate).
const DENIED_AUDITS_PER_MINUTE: u32 = 30;
static DENIED_AUDITS: Mutex<Option<HashMap<i32, (Instant, u32)>>> = Mutex::new(None);

pub(super) fn audit_denied(kind: &str, peer: &Peer, extra: serde_json::Value) {
    let emit = {
        let mut guard = DENIED_AUDITS.lock().unwrap();
        let entry = guard.get_or_insert_with(HashMap::new).entry(peer.conn_id).or_insert((Instant::now(), 0));
        if entry.0.elapsed() >= std::time::Duration::from_secs(60) {
            *entry = (Instant::now(), 0);
        }
        entry.1 += 1;
        entry.1 <= DENIED_AUDITS_PER_MINUTE
    };
    if emit {
        audit(kind, peer, extra);
    } else {
        log::debug!("forwarding: denied audit for {} rate-limited", peer.id);
    }
}

static ENGINES: Mutex<Option<HashMap<i32, Arc<Engine>>>> = Mutex::new(None);

/// Webcam dell'host verso il controller: richiesta in attesa di consenso,
/// consenso dato (vale per la sessione) e flusso in uscita.
struct Share {
    pending: Option<(i32, Out, Instant)>,
    granted: bool,
    sender: Option<SendStream>,
}

static SHARES: Mutex<Option<HashMap<i32, Share>>> = Mutex::new(None);
/// Senza risposta entro questo tempo la richiesta è negata.
const CONSENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

fn with_share<R>(conn_id: i32, f: impl FnOnce(&mut Share) -> R) -> R {
    let mut guard = SHARES.lock().unwrap();
    let share = guard
        .get_or_insert_with(HashMap::new)
        .entry(conn_id)
        .or_insert(Share { pending: None, granted: false, sender: None });
    f(share)
}

fn grant(out: &Out, request_id: i32, result: Result<Vec<ClantoCameraInfo>, String>) {
    let (accepted, message, cameras) = match result {
        Ok(cameras) => (true, String::new(), cameras),
        Err(e) => (false, e, Vec::new()),
    };
    out(wrap(clanto_forward::Union::CameraGrant(ClantoCameraGrant {
        request_id,
        accepted,
        message,
        cameras,
        ..Default::default()
    })));
}

/// Cosa deve fare la connessione dopo un messaggio.
pub enum Action {
    None,
    /// Chiedere all'utente dell'host il consenso alla webcam (finestra di gestione).
    AskCameraConsent,
}

/// Messaggio `ClantoForward` da una connessione autorizzata. `tx` è il canale
/// della connessione verso il peer.
pub fn on_message(
    peer: Peer,
    perms: Permissions,
    tx: &hbb_common::tokio::sync::mpsc::UnboundedSender<(Instant, Arc<Message>)>,
    msg: ClantoForward,
) -> Action {
    ALLOWED.lock().unwrap().get_or_insert_with(HashMap::new).insert(peer.conn_id, perms);
    let media_out = || -> Out {
        let tx = tx.clone();
        Arc::new(move |m: Message| {
            tx.send((Instant::now(), Arc::new(m))).ok();
        })
    };
    let key = Key::Host(peer.conn_id);
    // Id dispari: flussi del controller (webcam ricevuta); pari: dell'host.
    let from_controller = |id: i32| id % 2 != 0;
    let msg = match msg.union {
        Some(clanto_forward::Union::MediaOpen(m)) if from_controller(m.stream_id) => {
            camera_receiver::on_open(key, Some(&peer), camera_allowed(peer.conn_id), media_out(), m);
            return Action::None;
        }
        Some(clanto_forward::Union::MediaFrame(m)) if from_controller(m.stream_id) => {
            camera_receiver::on_frame(key, m);
            return Action::None;
        }
        Some(clanto_forward::Union::MediaClose(m)) => {
            if from_controller(m.stream_id) {
                camera_receiver::on_close(key, m.stream_id);
            } else {
                with_share(peer.conn_id, |s| {
                    if s.sender.as_ref().is_some_and(|x| x.stream_id == m.stream_id) {
                        if let Some(sender) = s.sender.take() {
                            sender.on_close(&m);
                        }
                    }
                });
            }
            return Action::None;
        }
        Some(clanto_forward::Union::MediaOpened(m)) => {
            with_share(peer.conn_id, |s| {
                if let Some(sender) = s.sender.as_ref().filter(|x| x.stream_id == m.stream_id) {
                    sender.on_opened(&m);
                }
            });
            return Action::None;
        }
        Some(clanto_forward::Union::MediaAck(m)) => {
            with_share(peer.conn_id, |s| {
                if let Some(sender) = s.sender.as_ref().filter(|x| x.stream_id == m.stream_id) {
                    sender.on_ack(&m);
                }
            });
            return Action::None;
        }
        Some(clanto_forward::Union::CameraRequest(r)) => return on_camera_request(&peer, media_out(), r),
        Some(clanto_forward::Union::CameraPull(p)) => {
            on_camera_pull(&peer, media_out(), p);
            return Action::None;
        }
        Some(clanto_forward::Union::MediaOpen(_) | clanto_forward::Union::MediaFrame(_)) => return Action::None,
        union => ClantoForward { union, ..Default::default() },
    };
    let engine = {
        let mut engines = ENGINES.lock().unwrap();
        let engines = engines.get_or_insert_with(HashMap::new);
        engines
            .entry(peer.conn_id)
            .or_insert_with(|| {
                let tx = tx.clone();
                let out: Out = Arc::new(move |m: Message| {
                    tx.send((Instant::now(), Arc::new(m))).ok();
                });
                Engine::new(Side::Host, out, Arc::new(HostHooks { peer: peer.clone() }))
            })
            .clone()
    };
    engine.on_message(msg);
    Action::None
}

fn on_camera_request(peer: &Peer, out: Out, req: ClantoCameraRequest) -> Action {
    if let Err(e) = camera_allowed(peer.conn_id) {
        audit_denied("clanto_forward_camera_denied", peer, json!({"direction": "to_controller", "reason": e}));
        grant(&out, req.request_id, Err(e));
        return Action::None;
    }
    // Telefoni come host: la cattura nativa parte dall'interfaccia, non da qui.
    if cfg!(any(target_os = "android", target_os = "ios")) {
        grant(&out, req.request_id, Err("Not supported on this device".to_owned()));
        return Action::None;
    }
    if with_share(peer.conn_id, |s| s.granted) {
        let cameras = camera_list();
        grant(&out, req.request_id, cameras);
        return Action::None;
    }
    with_share(peer.conn_id, |s| s.pending = Some((req.request_id, out, Instant::now())));
    // Nessuna risposta: negata dopo il timeout.
    let conn_id = peer.conn_id;
    std::thread::spawn(move || {
        std::thread::sleep(CONSENT_TIMEOUT);
        let expired = with_share(conn_id, |s| {
            let stale = s.pending.as_ref().is_some_and(|p| p.0 == req.request_id && p.2.elapsed() >= CONSENT_TIMEOUT);
            if stale {
                s.pending.take()
            } else {
                None
            }
        });
        if let Some((request_id, out, _)) = expired {
            grant(&out, request_id, Err(policy::DENIED_WEBCAM_CONSENT.to_owned()));
        }
    });
    Action::AskCameraConsent
}

fn camera_list() -> Result<Vec<ClantoCameraInfo>, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        Ok(super::camera_capture::list()?
            .into_iter()
            .map(|c| ClantoCameraInfo { index: c.index as i32, name: c.name, ..Default::default() })
            .collect())
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    Err("Not supported on this device".to_owned())
}

/// Risposta dell'utente dell'host (finestra di gestione) alla richiesta webcam.
pub fn camera_consent(conn_id: i32, peer: &Peer, accepted: bool) {
    let pending = with_share(conn_id, |s| {
        let p = s.pending.take();
        if p.is_some() && accepted {
            s.granted = true;
        }
        p
    });
    let Some((request_id, out, _)) = pending else { return };
    if accepted {
        audit("clanto_forward_camera_opened", peer, json!({"direction": "to_controller", "consent": true}));
        grant(&out, request_id, camera_list());
    } else {
        audit_denied(
            "clanto_forward_camera_denied",
            peer,
            json!({"direction": "to_controller", "reason": policy::DENIED_WEBCAM_CONSENT}),
        );
        grant(&out, request_id, Err(policy::DENIED_WEBCAM_CONSENT.to_owned()));
    }
}

fn on_camera_pull(peer: &Peer, out: Out, pull: ClantoCameraPull) {
    let refuse = |out: &Out, e: String| {
        out(wrap(clanto_forward::Union::MediaClose(ClantoMediaClose {
            stream_id: pull.stream_id,
            message: e,
            ..Default::default()
        })));
    };
    if let Err(e) = camera_allowed(peer.conn_id) {
        return refuse(&out, e);
    }
    if !with_share(peer.conn_id, |s| s.granted) {
        return refuse(&out, policy::DENIED_WEBCAM_CONSENT.to_owned());
    }
    if pull.stream_id <= 0 || pull.stream_id % 2 != 0 || pull.index < 0 {
        return refuse(&out, "Invalid camera".to_owned());
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let index = pull.index as u32;
        audit(
            "clanto_forward_camera_opened",
            peer,
            json!({"direction": "to_controller", "camera": index}),
        );
        let factory = Box::new(move || super::camera_capture::open(index));
        let sender = SendStream::start(pull.stream_id, DEFAULT_FPS, factory, out);
        let previous = with_share(peer.conn_id, |s| s.sender.replace(sender));
        drop(previous);
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    refuse(&out, "Not supported on this device".to_owned());
}

/// Chiusura della connessione: chiude canali, listener e webcam.
pub fn close(conn_id: i32) {
    camera_receiver::close(Key::Host(conn_id));
    let share = SHARES.lock().unwrap().as_mut().and_then(|m| m.remove(&conn_id));
    drop(share);
    if let Some(m) = ALLOWED.lock().unwrap().as_mut() {
        m.remove(&conn_id);
    }
    if let Some(m) = DENIED_AUDITS.lock().unwrap().as_mut() {
        m.remove(&conn_id);
    }
    let engine = ENGINES
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|engines| engines.remove(&conn_id));
    if let Some(engine) = engine {
        engine.close_all();
    }
}
