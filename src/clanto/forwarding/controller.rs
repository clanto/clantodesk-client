// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Lato controller: i forward di una sessione, creati dall'interfaccia.
//!
//! - `to_host`: il controller ascolta su 127.0.0.1 e porta le connessioni a una
//!   destinazione dalla parte dell'host (decide l'host).
//! - `to_controller`: l'host ascolta su 127.0.0.1 e porta le connessioni a una
//!   destinazione dalla parte del controller (decide il controller, qui).
//!
//! I motori girano su un runtime proprio: le chiamate arrivano anche
//! dall'interfaccia, fuori da qualsiasi runtime.

use super::{
    camera_capture,
    catalog::{self, Direction},
    engine::{Engine, Hooks, Out, Side, LISTEN_HOST},
    media::{SendInfo, SendStream, CAMERA_STREAM_ID, DEFAULT_FPS},
    policy,
};
use crate::client::Data;
use base::message_proto::*;
use hbb_common::{
    futures::future::BoxFuture,
    log,
    tokio::{self, net::TcpListener, runtime::Runtime, sync::mpsc::UnboundedSender, task::JoinHandle},
};
use serde_derive::Serialize;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex, OnceLock},
};

const MAX_FORWARDS: usize = 32;

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("clanto-forwarding")
            .enable_all()
            .build()
            .expect("forwarding runtime")
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct ForwardInfo {
    pub id: i32,
    pub direction: Direction,
    /// to_host: porta del controller; to_controller: porta dell'host.
    pub listen_port: i32,
    pub target_host: String,
    pub target_port: i32,
    pub service: String,
    pub state: &'static str,
    pub message: String,
}

struct Forward {
    info: ForwardInfo,
    task: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Forwards {
    items: HashMap<i32, Forward>,
    next_id: i32,
    /// canale -> forward, per mostrare il motivo di un rifiuto.
    channels: HashMap<i32, i32>,
}

struct SessionState {
    engine: Arc<Engine>,
    forwards: Arc<Mutex<Forwards>>,
    out: Out,
    camera: Option<SendStream>,
}

struct ControllerHooks {
    forwards: Arc<Mutex<Forwards>>,
}

impl Hooks for ControllerHooks {
    fn authorize_open(&self, host: String, port: i32) -> BoxFuture<'static, Result<Vec<SocketAddr>, String>> {
        // L'host apre solo canali per i listener chiesti da questo controller.
        let target = (host == LISTEN_HOST)
            .then(|| {
                self.forwards.lock().unwrap().items.get(&port).and_then(|f| {
                    (f.info.direction == Direction::ToController)
                        .then(|| (f.info.target_host.clone(), f.info.target_port))
                })
            })
            .flatten();
        Box::pin(async move {
            let (host, port) = target.ok_or_else(|| policy::DENIED_TARGET.to_owned())?;
            let decision = policy::authorize_target(Direction::ToController, &host, port).await?;
            let policy::Target::Addrs(addrs) = decision.target;
            Ok(addrs)
        })
    }

    fn on_listening(&self, msg: ClantoForwardListening) {
        let mut forwards = self.forwards.lock().unwrap();
        if let Some(f) = forwards.items.get_mut(&msg.listen_id) {
            if msg.success {
                f.info.state = "active";
                f.info.listen_port = msg.port;
                f.info.message.clear();
            } else {
                f.info.state = "error";
                f.info.message = msg.message;
            }
        }
    }

    fn on_refused(&self, channel_id: i32, message: String) {
        let mut forwards = self.forwards.lock().unwrap();
        if let Some(fid) = forwards.channels.remove(&channel_id) {
            if let Some(f) = forwards.items.get_mut(&fid) {
                f.info.message = message;
            }
        }
    }
}

static SESSIONS: Mutex<Option<HashMap<u64, SessionState>>> = Mutex::new(None);

fn session(key: u64, sender: &UnboundedSender<Data>) -> (Arc<Engine>, Arc<Mutex<Forwards>>) {
    let mut sessions = SESSIONS.lock().unwrap();
    let sessions = sessions.get_or_insert_with(HashMap::new);
    let state = sessions.entry(key).or_insert_with(|| {
        let forwards = Arc::new(Mutex::new(Forwards { next_id: 1, ..Default::default() }));
        let sender = sender.clone();
        let out: Out = Arc::new(move |m: Message| {
            sender.send(Data::Message(m)).ok();
        });
        let _guard = runtime().enter();
        let engine = Engine::new(
            Side::Controller,
            out.clone(),
            Arc::new(ControllerHooks { forwards: forwards.clone() }),
        );
        SessionState { engine, forwards, out, camera: None }
    });
    (state.engine.clone(), state.forwards.clone())
}

/// Messaggio `ClantoForward` dall'host (hook in `io_loop`).
pub fn on_message(key: u64, sender: &UnboundedSender<Data>, msg: ClantoForward) {
    let msg = match msg.union {
        Some(clanto_forward::Union::MediaOpened(m)) => return with_camera(key, |c| c.on_opened(&m)),
        Some(clanto_forward::Union::MediaAck(m)) => return with_camera(key, |c| c.on_ack(&m)),
        Some(clanto_forward::Union::MediaClose(m)) => return with_camera(key, |c| c.on_close(&m)),
        union => ClantoForward { union, ..Default::default() },
    };
    let (engine, _) = session(key, sender);
    let _guard = runtime().enter();
    engine.on_message(msg);
}

/// Fine della sessione: chiude canali e listener locali.
pub fn close(key: u64) {
    let state = SESSIONS.lock().unwrap().as_mut().and_then(|s| s.remove(&key));
    if let Some(state) = state {
        drop(state.camera);
        for (_, f) in state.forwards.lock().unwrap().items.drain() {
            if let Some(task) = f.task {
                task.abort();
            }
        }
        state.engine.close_all();
    }
}

pub fn list(key: u64) -> Vec<ForwardInfo> {
    let sessions = SESSIONS.lock().unwrap();
    let Some(state) = sessions.as_ref().and_then(|s| s.get(&key)) else {
        return Vec::new();
    };
    let mut items: Vec<ForwardInfo> = state
        .forwards
        .lock()
        .unwrap()
        .items
        .values()
        .map(|f| f.info.clone())
        .collect();
    items.sort_by_key(|f| f.id);
    items
}

fn reserve(forwards: &Arc<Mutex<Forwards>>, info: ForwardInfo) -> Result<i32, String> {
    let mut f = forwards.lock().unwrap();
    if f.items.len() >= MAX_FORWARDS {
        return Err("Too many forwards".to_owned());
    }
    let id = f.next_id;
    f.next_id += 1;
    f.items.insert(id, Forward { info: ForwardInfo { id, ..info }, task: None });
    Ok(id)
}

/// Nuovo forward. `local_port`: per `to_host` la porta del controller, per
/// `to_controller` quella dell'host (0 = libera). `service` = id del catalogo,
/// vuoto per un forward personalizzato.
pub fn add(
    key: u64,
    sender: &UnboundedSender<Data>,
    direction: Direction,
    local_port: i32,
    target_host: String,
    target_port: i32,
    service: String,
) -> Result<ForwardInfo, String> {
    if hbb_common::config::Config::get_option(policy::OPT_ENABLED) != "Y" {
        return Err(policy::DENIED_DISABLED.to_owned());
    }
    let port = u16::try_from(local_port).map_err(|_| "Invalid port".to_owned())?;
    if target_host.trim().is_empty() || !(1..=65535).contains(&target_port) {
        return Err(policy::DENIED_TARGET.to_owned());
    }
    let (engine, forwards) = session(key, sender);
    let info = ForwardInfo {
        id: 0,
        direction,
        listen_port: local_port,
        target_host: target_host.trim().to_owned(),
        target_port,
        service,
        state: "pending",
        message: String::new(),
    };
    match direction {
        Direction::ToHost => {
            let listener = std::net::TcpListener::bind(("127.0.0.1", port))
                .and_then(|l| l.set_nonblocking(true).map(|_| l))
                .map_err(|e| format!("Cannot listen on port {port}: {e}"))?;
            let bound = listener.local_addr().map(|a| a.port() as i32).unwrap_or(0);
            let id = reserve(&forwards, ForwardInfo { listen_port: bound, state: "active", ..info })?;
            let _guard = runtime().enter();
            let listener = TcpListener::from_std(listener).map_err(|e| e.to_string())?;
            let (host, tport) = {
                let f = forwards.lock().unwrap();
                let i = &f.items[&id].info;
                (i.target_host.clone(), i.target_port)
            };
            let weak = Arc::downgrade(&engine);
            let fw = forwards.clone();
            let task = runtime().spawn(async move {
                while let Ok((socket, _)) = listener.accept().await {
                    let Some(engine) = weak.upgrade() else { break };
                    match engine.open_outgoing(&host, tport, socket) {
                        Ok(channel) => {
                            fw.lock().unwrap().channels.insert(channel, id);
                        }
                        Err(e) => log::debug!("forward {id}: {e}"),
                    }
                }
            });
            let mut f = forwards.lock().unwrap();
            let entry = f.items.get_mut(&id).ok_or("Forward removed")?;
            entry.task = Some(task);
            Ok(entry.info.clone())
        }
        Direction::ToController => {
            // Prima di chiedere all'host, il controller decide su ciò che espone.
            runtime()
                .block_on(policy::authorize_target(Direction::ToController, &info.target_host, target_port))
                .map(|_| ())?;
            let id = reserve(&forwards, info)?;
            let _guard = runtime().enter();
            engine.send(clanto_forward::Union::Listen(ClantoForwardListen {
                listen_id: id,
                port: local_port,
                description: String::new(),
                ..Default::default()
            }));
            Ok(forwards.lock().unwrap().items[&id].info.clone())
        }
    }
}

pub fn remove(key: u64, id: i32) {
    let state = {
        let sessions = SESSIONS.lock().unwrap();
        sessions
            .as_ref()
            .and_then(|s| s.get(&key))
            .map(|s| (s.engine.clone(), s.forwards.clone()))
    };
    let Some((engine, forwards)) = state else {
        return;
    };
    let Some(f) = forwards.lock().unwrap().items.remove(&id) else {
        return;
    };
    if let Some(task) = f.task {
        task.abort();
    }
    if f.info.direction == Direction::ToController {
        let _guard = runtime().enter();
        engine.send(clanto_forward::Union::Unlisten(ClantoForwardUnlisten {
            listen_id: id,
            ..Default::default()
        }));
    }
}

/// Servizi del catalogo, aggiornato se vecchio.
pub fn catalog_services() -> Vec<catalog::Service> {
    runtime().block_on(catalog::refresh_if_stale());
    catalog::current()
}

fn with_camera(key: u64, f: impl FnOnce(&SendStream)) {
    let sessions = SESSIONS.lock().unwrap();
    if let Some(camera) = sessions.as_ref().and_then(|s| s.get(&key)).and_then(|s| s.camera.as_ref()) {
        f(camera);
    }
}

/// Webcam di questo computer.
pub fn cameras() -> Result<Vec<camera_capture::CameraInfo>, String> {
    camera_capture::list()
}

/// Invia la webcam `index` all'host come camera virtuale. Sostituisce quella
/// già attiva nella sessione.
pub fn camera_start(key: u64, sender: &UnboundedSender<Data>, index: u32) -> Result<(), String> {
    if hbb_common::config::Config::get_option(policy::OPT_ENABLED) != "Y" {
        return Err(policy::DENIED_DISABLED.to_owned());
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    camera_capture::prepare();
    session(key, sender);
    let mut sessions = SESSIONS.lock().unwrap();
    let state = sessions
        .as_mut()
        .and_then(|s| s.get_mut(&key))
        .ok_or("Session closed")?;
    let factory = Box::new(move || camera_capture::open(index));
    let previous = state.camera.replace(SendStream::start(CAMERA_STREAM_ID, DEFAULT_FPS, factory, state.out.clone()));
    drop(sessions);
    drop(previous);
    Ok(())
}

pub fn camera_stop(key: u64) {
    let camera = SESSIONS
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|s| s.get_mut(&key))
        .and_then(|s| s.camera.take());
    drop(camera);
}

pub fn camera_status(key: u64) -> Option<SendInfo> {
    let sessions = SESSIONS.lock().unwrap();
    let camera = sessions.as_ref()?.get(&key)?.camera.as_ref()?;
    let info = camera.info.lock().unwrap().clone();
    Some(info)
}
