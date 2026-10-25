// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Motore simmetrico dei canali di forwarding, uguale su host e controller.
//!
//! Riusa senza modificarle le primitive di upstream (`crate::port_forward_mux`:
//! finestre, crediti, `run_channel`): i frame `PortForwardChannel` che quelle
//! producono vengono incapsulati in `ClantoForward` da un convertitore, così
//! non si confondono mai con il port forward di upstream. Entrambi i lati
//! possono aprire canali: id dispari dal controller, pari dall'host. Cosa
//! autorizzare e cosa registrare lo decide il lato, tramite `Hooks`.

use crate::port_forward_mux::{
    charge, close_msg, effective_window, open_msg, opened_msg, run_channel, FrameSink, Inbound,
    RecvWindow, SendCredit, CHANNEL_WINDOW, INITIAL_WINDOW, MAX_CHANNELS,
};
use base::message_proto::*;
use hbb_common::{
    bytes::Bytes,
    futures::future::BoxFuture,
    log,
    tokio::{
        self,
        net::{TcpListener, TcpStream},
        sync::{mpsc, watch},
        task::JoinHandle,
        time::Instant,
    },
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Host di un canale aperto dall'host per un listener: `port` = listen_id.
pub const LISTEN_HOST: &str = "clanto-listen";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_LISTENERS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Host,
    Controller,
}

impl Side {
    fn first_id(self) -> i32 {
        match self {
            Side::Controller => 1,
            Side::Host => 2,
        }
    }

    /// Il peer apre solo id della propria parità.
    fn peer_owns(self, id: i32) -> bool {
        match self {
            Side::Host => id > 0 && id % 2 == 1,
            Side::Controller => id > 0 && id % 2 == 0,
        }
    }
}

/// Le decisioni del lato. Le future non devono tenere lock.
pub trait Hooks: Send + Sync + 'static {
    /// Un canale chiesto dal peer: dove collegarsi, oppure il motivo del rifiuto.
    fn authorize_open(&self, host: String, port: i32) -> BoxFuture<'static, Result<Vec<SocketAddr>, String>>;
    /// Solo host: il peer chiede un listener locale.
    fn authorize_listen(&self, _req: &ClantoForwardListen) -> Result<(), String> {
        Err(super::policy::DENIED_REVERSE.to_owned())
    }
    fn on_listening(&self, _msg: ClantoForwardListening) {}
    fn on_refused(&self, _channel_id: i32, _message: String) {}
}

struct Entry {
    inbound: mpsc::UnboundedSender<Inbound>,
    credit: Arc<SendCredit>,
    window: Arc<Mutex<RecvWindow>>,
    opened: bool,
}

struct State {
    channels: HashMap<i32, Entry>,
    next_id: i32,
    listeners: HashMap<i32, JoinHandle<()>>,
    /// Canali aperti da ogni listener: si chiudono con il listener.
    listener_channels: HashMap<i32, Vec<i32>>,
}

pub struct Engine {
    side: Side,
    raw: mpsc::UnboundedSender<(Instant, Arc<Message>)>,
    out: Out,
    state: Mutex<State>,
    teardown: watch::Sender<bool>,
    hooks: Arc<dyn Hooks>,
}

/// Come raggiunge il peer un messaggio già incapsulato.
pub type Out = Arc<dyn Fn(Message) + Send + Sync>;

pub fn wrap(union: clanto_forward::Union) -> Message {
    let mut cf = ClantoForward::new();
    cf.union = Some(union);
    let mut msg = Message::new();
    msg.set_clanto_forward(cf);
    msg
}

impl Engine {
    /// Va chiamata dentro un runtime tokio: avvia il convertitore.
    pub fn new(side: Side, out: Out, hooks: Arc<dyn Hooks>) -> Arc<Self> {
        let (raw, mut raw_rx) = mpsc::unbounded_channel::<(Instant, Arc<Message>)>();
        let forward = out.clone();
        tokio::spawn(async move {
            while let Some((_, msg)) = raw_rx.recv().await {
                let msg = Arc::try_unwrap(msg).unwrap_or_else(|m| (*m).clone());
                match msg.union {
                    Some(message::Union::PortForwardChannel(ch)) => {
                        forward(wrap(clanto_forward::Union::Channel(ch)))
                    }
                    _ => forward(msg),
                }
            }
        });
        Arc::new(Self {
            side,
            raw,
            out,
            state: Mutex::new(State {
                channels: HashMap::new(),
                next_id: side.first_id(),
                listeners: HashMap::new(),
                listener_channels: HashMap::new(),
            }),
            teardown: watch::channel(false).0,
            hooks,
        })
    }

    fn sink(&self) -> FrameSink {
        FrameSink::Direct(self.raw.clone())
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn send(&self, union: clanto_forward::Union) {
        (self.out)(wrap(union));
    }

    /// Apre un canale verso `host:port` del peer per un socket locale accettato.
    pub fn open_outgoing(self: &Arc<Self>, host: &str, port: i32, socket: TcpStream) -> Result<i32, String> {
        if self.raw.is_closed() {
            return Err("forwarding session is gone".to_owned());
        }
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let credit = Arc::new(SendCredit::new(INITIAL_WINDOW));
        let window = Arc::new(Mutex::new(RecvWindow::new(CHANNEL_WINDOW)));
        let id = {
            let mut state = self.state.lock().unwrap();
            if state.channels.len() >= MAX_CHANNELS {
                return Err("too many forwarding channels".to_owned());
            }
            let mut id = state.next_id;
            while state.channels.contains_key(&id) {
                id = id.checked_add(2).unwrap_or(self.side.first_id());
            }
            state.next_id = id.checked_add(2).unwrap_or(self.side.first_id());
            state.channels.insert(
                id,
                Entry {
                    inbound: inbound_tx,
                    credit: credit.clone(),
                    window: window.clone(),
                    opened: false,
                },
            );
            id
        };
        let open = open_msg(id, host, port, CHANNEL_WINDOW);
        let (reader, writer) = socket.into_split();
        let sink = self.sink();
        let teardown = self.teardown.subscribe();
        let engine = self.clone();
        tokio::spawn(async move {
            // `open` sulla coda ordinata, prima del primo `data` del canale.
            if sink.send_ordered(open).await.is_ok() {
                run_channel(id, reader, writer, Vec::new(), Vec::new(), credit, window, inbound_rx, sink, teardown).await;
            }
            engine.state.lock().unwrap().channels.remove(&id);
        });
        Ok(id)
    }

    pub fn on_message(self: &Arc<Self>, cf: ClantoForward) {
        match cf.union {
            Some(clanto_forward::Union::Channel(ch)) => self.on_channel(ch),
            Some(clanto_forward::Union::Listen(l)) if self.side == Side::Host => self.on_listen(l),
            Some(clanto_forward::Union::Unlisten(u)) if self.side == Side::Host => {
                let channels = {
                    let mut state = self.state.lock().unwrap();
                    if let Some(task) = state.listeners.remove(&u.listen_id) {
                        task.abort();
                    }
                    state.listener_channels.remove(&u.listen_id).unwrap_or_default()
                };
                // Forward eliminato: anche le connessioni già aperte si chiudono.
                self.close_channels(&channels);
            }
            Some(clanto_forward::Union::Listening(l)) if self.side == Side::Controller => {
                self.hooks.on_listening(l)
            }
            _ => log::debug!("forwarding: unexpected message on {:?}", self.side),
        }
    }

    fn on_channel(self: &Arc<Self>, ch: PortForwardChannel) {
        match ch.union {
            Some(port_forward_channel::Union::Open(o)) => self.on_open(o),
            Some(port_forward_channel::Union::Opened(o)) => {
                let mut state = self.state.lock().unwrap();
                if o.success {
                    if let Some(e) = state.channels.get_mut(&o.channel_id) {
                        // Un `opened` ripetuto non alza di nuovo il credito.
                        if !e.opened {
                            e.opened = true;
                            e.credit.raise_initial(o.window);
                        }
                    }
                } else if let Some(e) = state.channels.remove(&o.channel_id) {
                    e.inbound.send(Inbound::Close).ok();
                    drop(state);
                    self.hooks.on_refused(o.channel_id, o.message);
                }
            }
            Some(port_forward_channel::Union::Data(d)) => {
                let mut state = self.state.lock().unwrap();
                let Some(e) = state.channels.get(&d.channel_id) else {
                    return;
                };
                let accepted = e.window.lock().unwrap().accept(d.data.len());
                if accepted && e.inbound.send(Inbound::Data(d.data)).is_ok() {
                    return;
                }
                // Tolto subito: il peer non può accodare altro su questo id.
                if let Some(e) = state.channels.remove(&d.channel_id) {
                    if !accepted {
                        log::warn!("forwarding channel {} overran its window", d.channel_id);
                        e.inbound.send(Inbound::Violation).ok();
                    }
                }
            }
            Some(port_forward_channel::Union::Close(c)) => {
                if let Some(e) = self.state.lock().unwrap().channels.remove(&c.channel_id) {
                    e.inbound.send(Inbound::Close).ok();
                }
            }
            Some(port_forward_channel::Union::WindowUpdate(u)) => {
                if let Some(e) = self.state.lock().unwrap().channels.get(&u.channel_id) {
                    e.credit.add(u.add);
                }
            }
            _ => {}
        }
    }

    fn reply(&self, msg: Message) {
        self.raw.send((Instant::now(), Arc::new(msg))).ok();
    }

    fn on_open(self: &Arc<Self>, open: PortForwardOpen) {
        let id = open.channel_id;
        if !self.side.peer_owns(id) {
            log::warn!("forwarding: peer opened channel {} with the wrong parity", id);
            return;
        }
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let credit = Arc::new(SendCredit::new(effective_window(open.window)));
        let window = Arc::new(Mutex::new(RecvWindow::new(INITIAL_WINDOW)));
        {
            let mut state = self.state.lock().unwrap();
            state.channels.retain(|_, e| !e.inbound.is_closed());
            if state.channels.contains_key(&id) {
                return;
            }
            if state.channels.len() >= MAX_CHANNELS {
                drop(state);
                self.reply(opened_msg(id, false, "Too many forwarding channels", 0));
                return;
            }
            state.channels.insert(
                id,
                Entry {
                    inbound: inbound_tx,
                    credit: credit.clone(),
                    window: window.clone(),
                    opened: true,
                },
            );
        }
        let authorize = self.hooks.authorize_open(open.host, open.port);
        let sink = self.sink();
        let teardown = self.teardown.subscribe();
        let engine = self.clone();
        tokio::spawn(async move {
            run_accepted(id, authorize, credit, window, inbound_rx, sink, teardown).await;
            engine.state.lock().unwrap().channels.remove(&id);
        });
    }

    fn on_listen(self: &Arc<Self>, req: ClantoForwardListen) {
        let id = req.listen_id;
        let reply = |success: bool, message: &str, port: i32| {
            self.send(clanto_forward::Union::Listening(ClantoForwardListening {
                listen_id: id,
                success,
                message: message.to_owned(),
                port,
                ..Default::default()
            }))
        };
        if let Err(e) = self.hooks.authorize_listen(&req) {
            reply(false, &e, 0);
            return;
        }
        // Porta libera scelta dal sistema o non privilegiata: il controller non
        // può prendere le porte basse dei servizi locali dell'host.
        let port = match u16::try_from(req.port) {
            Ok(p) if p == 0 || p >= 1024 => p,
            _ => return reply(false, "Invalid port", 0),
        };
        if self.state.lock().unwrap().listeners.len() >= MAX_LISTENERS {
            return reply(false, "Too many forwarding listeners", 0);
        }
        // Solo loopback: il listener serve a chi è sull'host, non alla sua rete.
        let listener = match std::net::TcpListener::bind(("127.0.0.1", port))
            .and_then(|l| l.set_nonblocking(true).map(|_| l))
            .and_then(TcpListener::from_std)
        {
            Ok(l) => l,
            Err(e) => return reply(false, &format!("Cannot listen on port {port}: {e}"), 0),
        };
        let bound = listener.local_addr().map(|a| a.port() as i32).unwrap_or(0);
        let engine = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let Some(engine) = engine.upgrade() else {
                    break;
                };
                match engine.open_outgoing(LISTEN_HOST, id, socket) {
                    Ok(channel) => {
                        let mut state = engine.state.lock().unwrap();
                        let live: Vec<i32> = state.channels.keys().copied().collect();
                        let list = state.listener_channels.entry(id).or_default();
                        list.retain(|c| live.contains(c));
                        list.push(channel);
                    }
                    Err(e) => log::debug!("forwarding listener {}: {}", id, e),
                }
            }
        });
        if let Some(old) = self.state.lock().unwrap().listeners.insert(id, task) {
            old.abort();
        }
        reply(true, "", bound);
    }

    /// Chiude canali aperti (forward eliminato): il task locale chiude il
    /// socket e il peer riceve `close`.
    pub fn close_channels(&self, ids: &[i32]) {
        let mut state = self.state.lock().unwrap();
        for id in ids {
            if let Some(e) = state.channels.remove(id) {
                e.inbound.send(Inbound::Close).ok();
                self.raw.send((Instant::now(), Arc::new(close_msg(*id)))).ok();
            }
        }
    }

    pub fn has_channel(&self, id: i32) -> bool {
        self.state.lock().unwrap().channels.contains_key(&id)
    }

    pub fn close_all(&self) {
        let mut state = self.state.lock().unwrap();
        state.channels.clear();
        for (_, task) in state.listeners.drain() {
            task.abort();
        }
        self.teardown.send_replace(true);
    }

    #[cfg(test)]
    pub fn live_channels(&self) -> usize {
        self.state.lock().unwrap().channels.len()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.close_all();
    }
}

async fn connect_any(addrs: &[SocketAddr]) -> Result<TcpStream, String> {
    let mut last = "no address".to_owned();
    for addr in addrs {
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
            Ok(Ok(s)) => return Ok(s),
            Ok(Err(e)) => last = e.to_string(),
            Err(_) => last = "timeout".to_owned(),
        }
    }
    Err(last)
}

/// Come il lato controllato di upstream (`run_controlled_channel`), con in più
/// l'autorizzazione: autorizza, collega, concede la finestra, risponde `opened`,
/// poi inoltra. I dati arrivati nel frattempo restano in coda entro
/// `INITIAL_WINDOW`; un `close` già in coda vince sempre.
async fn run_accepted(
    id: i32,
    authorize: BoxFuture<'static, Result<Vec<SocketAddr>, String>>,
    credit: Arc<SendCredit>,
    window: Arc<Mutex<RecvWindow>>,
    mut inbound: mpsc::UnboundedReceiver<Inbound>,
    sink: FrameSink,
    teardown: watch::Receiver<bool>,
) {
    let mut pending: Vec<Bytes> = Vec::new();
    let mut pending_len = 0usize;
    let setup = async move {
        let addrs = authorize.await?;
        connect_any(&addrs).await
    };
    tokio::pin!(setup);
    let socket = loop {
        tokio::select! {
            biased;
            cmd = inbound.recv() => match cmd {
                Some(Inbound::Data(b)) => {
                    if pending_len.saturating_add(charge(b.len()) as usize) > INITIAL_WINDOW as usize {
                        log::warn!("forwarding channel {} sent more than INITIAL_WINDOW before opened", id);
                        sink.send_ordered(close_msg(id)).await.ok();
                        return;
                    }
                    pending_len += charge(b.len()) as usize;
                    pending.push(b);
                }
                Some(Inbound::Close) | None => return,
                Some(Inbound::Violation) => {
                    sink.send_ordered(close_msg(id)).await.ok();
                    return;
                }
            },
            res = &mut setup => match res {
                Ok(s) => break s,
                Err(e) => {
                    log::info!("forwarding channel {} refused: {}", id, e);
                    sink.send_ordered(opened_msg(id, false, &e, 0)).await.ok();
                    return;
                }
            }
        }
    };
    // Concessa prima che `opened` parta: il peer non può esserne davanti.
    window.lock().unwrap().grant(CHANNEL_WINDOW - INITIAL_WINDOW);
    if sink.send_ordered(opened_msg(id, true, "", CHANNEL_WINDOW)).await.is_err() {
        return;
    }
    let (reader, writer) = socket.into_split();
    run_channel(id, reader, writer, Vec::new(), pending, credit, window, inbound, sink, teardown).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbb_common::tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    /// Autorizza solo `allowed`; per i listener mappa listen_id -> `reverse`.
    struct TestHooks {
        allowed: Option<SocketAddr>,
        reverse: Option<SocketAddr>,
        listening: Mutex<Option<ClantoForwardListening>>,
        refused: Mutex<Vec<String>>,
    }

    impl Hooks for TestHooks {
        fn authorize_open(&self, host: String, port: i32) -> BoxFuture<'static, Result<Vec<SocketAddr>, String>> {
            let result = if host == LISTEN_HOST {
                self.reverse.map(|a| vec![a]).ok_or_else(|| "no listener".to_owned())
            } else {
                match self.allowed {
                    Some(a) if a.port() as i32 == port => Ok(vec![a]),
                    _ => Err("Forwarding target not allowed".to_owned()),
                }
            };
            Box::pin(async move { result })
        }
        fn authorize_listen(&self, _req: &ClantoForwardListen) -> Result<(), String> {
            Ok(())
        }
        fn on_listening(&self, msg: ClantoForwardListening) {
            *self.listening.lock().unwrap() = Some(msg);
        }
        fn on_refused(&self, _channel_id: i32, message: String) {
            self.refused.lock().unwrap().push(message);
        }
    }

    fn hooks(allowed: Option<SocketAddr>, reverse: Option<SocketAddr>) -> Arc<TestHooks> {
        Arc::new(TestHooks {
            allowed,
            reverse,
            listening: Mutex::new(None),
            refused: Mutex::new(Vec::new()),
        })
    }

    /// Due motori collegati in memoria, come host e controller di una sessione.
    fn pair(host_hooks: Arc<TestHooks>, ctrl_hooks: Arc<TestHooks>) -> (Arc<Engine>, Arc<Engine>) {
        let (to_host_tx, mut to_host_rx) = mpsc::unbounded_channel::<Message>();
        let (to_ctrl_tx, mut to_ctrl_rx) = mpsc::unbounded_channel::<Message>();
        let host = Engine::new(Side::Host, Arc::new(move |m| { to_ctrl_tx.send(m).ok(); }), host_hooks);
        let ctrl = Engine::new(Side::Controller, Arc::new(move |m| { to_host_tx.send(m).ok(); }), ctrl_hooks);
        let (h, c) = (host.clone(), ctrl.clone());
        tokio::spawn(async move {
            while let Some(m) = to_host_rx.recv().await {
                if let Some(message::Union::ClantoForward(cf)) = m.union {
                    h.on_message(cf);
                }
            }
        });
        tokio::spawn(async move {
            while let Some(m) = to_ctrl_rx.recv().await {
                if let Some(message::Union::ClantoForward(cf)) = m.union {
                    c.on_message(cf);
                }
            }
        });
        (host, ctrl)
    }

    async fn echo_server() -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                tokio::spawn(async move {
                    let (mut r, mut w) = s.split();
                    tokio::io::copy(&mut r, &mut w).await.ok();
                });
            }
        });
        addr
    }

    /// Un socket "locale" collegato al motore: il lato client lo usa il test.
    async fn local_pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TcpStream::connect(l.local_addr().unwrap()).await.unwrap();
        let (server, _) = l.accept().await.unwrap();
        (client, server)
    }

    async fn roundtrip(client: &mut TcpStream, data: &[u8]) -> Vec<u8> {
        let (mut r, mut w) = client.split();
        let send = async {
            w.write_all(data).await.unwrap();
        };
        let mut back = vec![0u8; data.len()];
        let recv = async {
            r.read_exact(&mut back).await.unwrap();
        };
        tokio::time::timeout(Duration::from_secs(20), async { tokio::join!(send, recv) })
            .await
            .expect("roundtrip timed out");
        back
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
    }

    #[test]
    fn to_host_channel_carries_a_megabyte_both_ways() {
        rt().block_on(async {
            let echo = echo_server().await;
            let (_host, ctrl) = pair(hooks(Some(echo), None), hooks(None, None));
            let (mut client, server) = local_pair().await;
            ctrl.open_outgoing("127.0.0.1", echo.port() as i32, server).unwrap();
            let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 251) as u8).collect();
            assert_eq!(roundtrip(&mut client, &data).await, data);
        });
    }

    #[test]
    fn denied_target_closes_the_local_socket() {
        rt().block_on(async {
            let echo = echo_server().await;
            let ctrl_hooks = hooks(None, None);
            let (_host, ctrl) = pair(hooks(None, None), ctrl_hooks.clone());
            let (mut client, server) = local_pair().await;
            ctrl.open_outgoing("127.0.0.1", echo.port() as i32, server).unwrap();
            let mut buf = [0u8; 8];
            let n = tokio::time::timeout(Duration::from_secs(10), client.read(&mut buf))
                .await
                .expect("socket not closed")
                .unwrap_or(0);
            assert_eq!(n, 0);
            assert_eq!(ctrl_hooks.refused.lock().unwrap().as_slice(), ["Forwarding target not allowed"]);
        });
    }

    #[test]
    fn reverse_listener_reaches_the_controller_side() {
        rt().block_on(async {
            let echo = echo_server().await;
            let ctrl_hooks = hooks(None, Some(echo));
            let (_host, ctrl) = pair(hooks(None, None), ctrl_hooks.clone());
            ctrl.send(clanto_forward::Union::Listen(ClantoForwardListen {
                listen_id: 7,
                port: 0,
                ..Default::default()
            }));
            let port = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if let Some(l) = ctrl_hooks.listening.lock().unwrap().clone() {
                        return l;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            assert!(port.success, "{}", port.message);
            let mut client = TcpStream::connect(("127.0.0.1", port.port as u16)).await.unwrap();
            assert_eq!(roundtrip(&mut client, b"ciao dall'host").await, b"ciao dall'host");
        });
    }

    #[test]
    fn peer_cannot_open_ids_of_the_other_parity() {
        assert!(Side::Host.peer_owns(1) && !Side::Host.peer_owns(2));
        assert!(Side::Controller.peer_owns(2) && !Side::Controller.peer_owns(1));
        assert!(!Side::Host.peer_owns(-1));
    }
}
