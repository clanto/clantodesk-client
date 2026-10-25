// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Lato host: un motore per connessione autorizzata, creato al primo messaggio.
//! Decide la policy dell'host; ogni canale e listener, concesso o negato, va
//! nell'audit.

use super::{
    catalog::Direction,
    engine::{Engine, Hooks, Out, Side, LISTEN_HOST},
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
            match policy::authorize_target(Direction::ToHost, &host, port).await {
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
                    audit(
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
        let result = policy::authorize_listen();
        let detail = json!({
            "direction": "to_controller",
            "listen_port": req.port,
            "description": req.description,
            "reason": result.as_ref().err().cloned().unwrap_or_default(),
        });
        match &result {
            Ok(()) => audit("clanto_forward_listen", &self.peer, detail),
            Err(_) => audit("clanto_forward_listen_denied", &self.peer, detail),
        }
        result
    }
}

static ENGINES: Mutex<Option<HashMap<i32, Arc<Engine>>>> = Mutex::new(None);

/// Messaggio `ClantoForward` da una connessione autorizzata. `tx` è il canale
/// della connessione verso il peer.
pub fn on_message(
    peer: Peer,
    tx: &hbb_common::tokio::sync::mpsc::UnboundedSender<(Instant, Arc<Message>)>,
    msg: ClantoForward,
) {
    let media_out = || -> Out {
        let tx = tx.clone();
        Arc::new(move |m: Message| {
            tx.send((Instant::now(), Arc::new(m))).ok();
        })
    };
    let msg = match msg.union {
        Some(clanto_forward::Union::MediaOpen(m)) => return super::camera_host::on_open(&peer, media_out(), m),
        Some(clanto_forward::Union::MediaFrame(m)) => return super::camera_host::on_frame(peer.conn_id, m),
        Some(clanto_forward::Union::MediaClose(m)) => return super::camera_host::on_close(peer.conn_id, m),
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
}

/// Chiusura della connessione: chiude canali e listener.
pub fn close(conn_id: i32) {
    super::camera_host::close(conn_id);
    let engine = ENGINES
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|engines| engines.remove(&conn_id));
    if let Some(engine) = engine {
        engine.close_all();
    }
}
