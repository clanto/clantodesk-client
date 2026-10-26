// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Harness E2E del login 2FA. NON fa parte del prodotto: docker/e2e/run.sh lo
//! copia in src/ solo dentro il container di test e aggiunge `--e2e` a
//! core_main. Il controller è headless e costruisce il LoginRequest da sé,
//! così può comportarsi anche da client modificato (ID falso, hwid).
//!
//!   --e2e host-setup <password> <totp-hex>   host: password, 2FA, approvazione
//!   --e2e host-trust <elenco>                host: clanto-trusted-controllers via IPC
//!   --e2e host-option <chiave> <valore>      host: qualsiasi opzione via IPC
//!   --e2e local-option <chiave> <valore>     controller: opzione dell'utente
//!   --e2e forward <host-id> <password> to-host:<ip>:<porta> | reverse:<ip>:<porta>:<secondi>
//!   --e2e camera <host-id> <password> <frame>  webcam di prova verso l'host
//!   --e2e device-key | my-id                 controller
//!   --e2e login <host-id> <password> <totp-hex|-> [hwid] [no-key] [as-id=<ID>]
//!
//! L'esito del login è una riga `RESULT=ok|2fa-required|error:<testo>`.

use crate::client::*;
use async_trait::async_trait;
use base::message_proto::*;
use hbb_common::{
    config::Config,
    protobuf::Message as _,
    rand,
    rendezvous_proto::ConnType,
    sha2::{Digest, Sha256},
    tokio, Stream,
};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
struct Harness {
    lc: Arc<RwLock<LoginConfigHandler>>,
}

#[async_trait]
impl Interface for Harness {
    fn send(&self, _data: Data) {}
    fn msgbox(&self, msgtype: &str, title: &str, text: &str, _link: &str) {
        eprintln!("msgbox {msgtype}: {title}: {text}");
    }
    fn handle_login_error(&self, _err: &str) -> bool {
        false
    }
    fn handle_peer_info(&self, _pi: PeerInfo) {}
    fn set_multiple_windows_session(&self, _sessions: Vec<WindowsSession>) {}
    async fn handle_hash(&self, _pass: &str, _hash: Hash, _peer: &mut Stream) -> bool {
        true
    }
    async fn handle_login_from_ui(
        &self,
        _os_username: String,
        _os_password: String,
        _password: String,
        _remember: bool,
        _peer: &mut Stream,
    ) {
    }
    async fn handle_test_delay(&self, _t: TestDelay, _peer: &mut Stream) {}
    fn get_lch(&self) -> Arc<RwLock<LoginConfigHandler>> {
        self.lc.clone()
    }
}

fn decode_hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn totp_code(secret: &[u8]) -> String {
    totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret.to_vec(),
        None,
        "e2e".to_owned(),
    )
    .expect("totp")
    .generate_current()
    .expect("totp code")
}

struct LoginArgs {
    host: String,
    password: String,
    totp: Option<Vec<u8>>,
    hwid: bool,
    no_key: bool,
    as_id: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn login(a: LoginArgs) -> String {
    match open_session(&a, ConnType::FILE_TRANSFER).await {
        Ok(_) => "ok".to_owned(),
        Err(result) => result,
    }
}

/// Login completo; `Err` porta gia' l'esito (`2fa-required`, `error:...`).
async fn open_session(a: &LoginArgs, conn: ConnType) -> Result<Stream, String> {
    let lc: Arc<RwLock<LoginConfigHandler>> = Default::default();
    lc.write()
        .unwrap()
        .initialize(a.host.clone(), conn, None, false, None, None, None);
    let key = Config::get_option("key");
    let ((mut stream, ..), _) = Client::start(&a.host, &key, "", conn, Harness { lc })
        .await
        .map_err(|e| format!("error:connect {e}"))?;
    let my_id = a.as_id.clone().unwrap_or_else(Config::get_id);
    let hwid = if a.hwid {
        crate::get_hwid()
    } else {
        Default::default()
    };
    loop {
        let Some(Ok(bytes)) = stream.next_timeout(20_000).await else {
            return Err("error:timeout".to_owned());
        };
        let Ok(msg) = Message::parse_from_bytes(&bytes) else {
            continue;
        };
        match msg.union {
            Some(message::Union::Hash(hash)) => {
                let mut h = Sha256::new();
                h.update(&a.password);
                h.update(&hash.salt);
                let salted = h.finalize();
                let mut h = Sha256::new();
                h.update(&salted[..]);
                h.update(&hash.challenge);
                let mut lr = LoginRequest {
                    username: a.host.clone(),
                    password: h.finalize()[..].to_vec().into(),
                    my_id: my_id.clone(),
                    my_name: "e2e".to_owned(),
                    my_platform: "Linux".to_owned(),
                    version: crate::VERSION.to_owned(),
                    session_id: rand::random(),
                    hwid: hwid.clone(),
                    ..Default::default()
                };
                if conn == ConnType::FILE_TRANSFER {
                    lr.set_file_transfer(FileTransfer {
                        dir: "/tmp".to_owned(),
                        ..Default::default()
                    });
                }
                if !a.no_key {
                    if let Some((k, s)) = crate::clanto::trusted_controllers::sign_login(
                        &a.host,
                        &hash.challenge,
                        &my_id,
                    ) {
                        lr.clanto_device_key = k.into();
                        lr.clanto_device_sig = s.into();
                    }
                }
                let mut m = Message::new();
                m.set_login_request(lr);
                stream.send(&m).await.map_err(|e| format!("error:send {e}"))?;
            }
            Some(message::Union::LoginResponse(res)) => match res.union {
                Some(login_response::Union::Error(err)) => {
                    if err == REQUIRE_2FA {
                        if let Some(secret) = &a.totp {
                            let mut m = Message::new();
                            m.set_auth_2fa(Auth2FA {
                                code: totp_code(secret),
                                hwid: hwid.clone(),
                                ..Default::default()
                            });
                            stream.send(&m).await.map_err(|e| format!("error:send {e}"))?;
                            continue;
                        }
                        return Err("2fa-required".to_owned());
                    }
                    eprintln!("LOGIN_ERROR {err:?}");
                    return Err(format!("error:{err}"));
                }
                Some(login_response::Union::PeerInfo(_)) => return Ok(stream),
                _ => {}
            },
            _ => {}
        }
    }
}

/// Hook del controller per l'harness: la policy è quella vera
/// (`authorize_target` con le opzioni locali di questo utente).
struct ForwardHooks {
    reverse_target: (String, i32),
    listening: std::sync::Mutex<Option<ClantoForwardListening>>,
    refused: std::sync::Mutex<Vec<String>>,
}

impl crate::clanto::forwarding::engine::Hooks for ForwardHooks {
    fn authorize_open(
        &self,
        host: String,
        _port: i32,
    ) -> hbb_common::futures::future::BoxFuture<'static, Result<Vec<std::net::SocketAddr>, String>> {
        use crate::clanto::forwarding::{catalog::Direction, engine::LISTEN_HOST, policy};
        let (t_host, t_port) = self.reverse_target.clone();
        Box::pin(async move {
            if host != LISTEN_HOST {
                return Err(policy::DENIED_TARGET.to_owned());
            }
            let d = policy::authorize_target(Direction::ToController, &t_host, t_port).await?;
            let policy::Target::Addrs(addrs) = d.target;
            Ok(addrs)
        })
    }
    fn on_listening(&self, msg: ClantoForwardListening) {
        *self.listening.lock().unwrap() = Some(msg);
    }
    fn on_refused(&self, _id: i32, message: String) {
        self.refused.lock().unwrap().push(message);
    }
}

/// Sessione di controllo remoto con un forward.
///   to-host:<ip>:<porta>          -> ok | error:<motivo>
///   reverse:<ip>:<porta>:<secondi> -> stampa LISTEN=<porta host>, tiene aperto
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn forward(a: LoginArgs, mode: String) -> String {
    use crate::clanto::forwarding::engine::{Engine, Side};
    use hbb_common::tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::mpsc,
    };
    use std::time::Duration;

    let mut stream = match open_session(&a, ConnType::DEFAULT_CONN).await {
        Ok(s) => s,
        Err(result) => return result,
    };
    let parts: Vec<&str> = mode.split(':').collect();
    let reverse_target = match parts.as_slice() {
        ["reverse", ip, port, _] => (ip.to_string(), port.parse().unwrap_or(0)),
        _ => (String::new(), 0),
    };
    let hooks = Arc::new(ForwardHooks {
        reverse_target,
        listening: Default::default(),
        refused: Default::default(),
    });
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
    let engine = Engine::new(
        Side::Controller,
        Arc::new(move |m| {
            out_tx.send(m).ok();
        }),
        hooks.clone(),
    );
    let scenario = {
        let engine = engine.clone();
        let hooks = hooks.clone();
        let parts: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
        hbb_common::tokio::spawn(async move {
            match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                ["to-host", ip, port] => {
                    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let mut client = TcpStream::connect(l.local_addr().unwrap()).await.unwrap();
                    let (server, _) = l.accept().await.unwrap();
                    if let Err(e) = engine.open_outgoing(ip, port.parse().unwrap_or(0), server) {
                        return format!("error:{e}");
                    }
                    client.write_all(b"e2e-forward").await.ok();
                    let mut buf = [0u8; 11];
                    match hbb_common::tokio::time::timeout(Duration::from_secs(15), client.read_exact(&mut buf)).await {
                        Ok(Ok(_)) if &buf == b"e2e-forward" => "ok".to_owned(),
                        _ => format!(
                            "error:{}",
                            hooks.refused.lock().unwrap().last().cloned().unwrap_or_else(|| "closed".to_owned())
                        ),
                    }
                }
                ["reverse", _, _, secs] => {
                    engine.send(clanto_forward::Union::Listen(ClantoForwardListen {
                        listen_id: 1,
                        port: 0,
                        description: "e2e".to_owned(),
                        ..Default::default()
                    }));
                    for _ in 0..150 {
                        let listening = hooks.listening.lock().unwrap().clone();
                        if let Some(l) = listening {
                            if !l.success {
                                return format!("error:listen {}", l.message);
                            }
                            println!("LISTEN={}", l.port);
                            hbb_common::tokio::time::sleep(Duration::from_secs(secs.parse().unwrap_or(30))).await;
                            return "ok".to_owned();
                        }
                        hbb_common::tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    "error:no listening reply".to_owned()
                }
                _ => "error:bad mode".to_owned(),
            }
        })
    };
    hbb_common::tokio::pin!(scenario);
    loop {
        hbb_common::tokio::select! {
            r = &mut scenario => {
                engine.close_all();
                return r.unwrap_or_else(|e| format!("error:{e}"));
            }
            Some(m) = out_rx.recv() => {
                if let Err(e) = stream.send(&m).await {
                    return format!("error:send {e}");
                }
            }
            res = stream.next_timeout(60_000) => {
                let Some(Ok(bytes)) = res else {
                    eprintln!("STREAM_END {:?}", res.map(|r| r.map(|b| b.len())));
                    return "error:session closed".to_owned();
                };
                if let Ok(msg) = Message::parse_from_bytes(&bytes) {
                    if let Some(message::Union::ClantoForward(cf)) = msg.union {
                        engine.on_message(cf);
                    }
                }
            }
        }
    }
}

/// Sessione di controllo remoto che invia un pattern di prova come webcam:
/// `ok frames=N` quando l'host ha confermato N frame decodificati.
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn camera(a: LoginArgs, frames: i64) -> String {
    use crate::clanto::forwarding::media::{SendStream, TestPattern, CAMERA_STREAM_ID};
    use hbb_common::tokio::sync::mpsc;
    use std::time::{Duration, Instant};

    let mut stream = match open_session(&a, ConnType::DEFAULT_CONN).await {
        Ok(s) => s,
        Err(result) => return result,
    };
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
    let mut camera = SendStream::start(
        CAMERA_STREAM_ID,
        15,
        Box::new(|| Ok(Box::new(TestPattern::new(320, 240)) as _)),
        Arc::new(move |m| {
            out_tx.send(m).ok();
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut tick = hbb_common::tokio::time::interval(Duration::from_millis(100));
    let result = loop {
        hbb_common::tokio::select! {
            _ = tick.tick() => {
                let info = camera.info.lock().unwrap().clone();
                if info.state == "error" || info.state == "closed" {
                    break format!("error:{}", info.message);
                }
                if info.acked >= frames {
                    break format!("ok frames={} sink={} size={}x{}", info.acked, info.sink, info.width, info.height);
                }
                if Instant::now() > deadline {
                    break format!("error:timeout state={} sent={} acked={}", info.state, info.sent, info.acked);
                }
            }
            Some(m) = out_rx.recv() => {
                if let Err(e) = stream.send(&m).await {
                    break format!("error:send {e}");
                }
            }
            res = stream.next_timeout(60_000) => {
                let Some(Ok(bytes)) = res else {
                    break "error:session closed".to_owned();
                };
                if let Ok(msg) = Message::parse_from_bytes(&bytes) {
                    if let Some(message::Union::ClantoForward(cf)) = msg.union {
                        match cf.union {
                            Some(clanto_forward::Union::MediaOpened(m)) => camera.on_opened(&m),
                            Some(clanto_forward::Union::MediaAck(m)) => camera.on_ack(&m),
                            Some(clanto_forward::Union::MediaClose(m)) => camera.on_close(&m),
                            _ => {}
                        }
                    }
                }
            }
        }
    };
    camera.stop();
    // Il MediaClose del controller arriva all'host prima della chiusura.
    while let Ok(m) = out_rx.try_recv() {
        stream.send(&m).await.ok();
    }
    result
}

/// Webcam dell'host verso il controller, senza finestra di consenso:
///   request -> `granted` | `denied:<motivo>` (risposta dell'host entro 10 s)
///   pull    -> `closed:<motivo>` (scelta della webcam senza consenso)
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn camera_ask(a: LoginArgs, mode: String) -> String {
    use crate::clanto::forwarding::engine::wrap;
    let mut stream = match open_session(&a, ConnType::DEFAULT_CONN).await {
        Ok(s) => s,
        Err(result) => return result,
    };
    let first = match mode.as_str() {
        "request" => clanto_forward::Union::CameraRequest(ClantoCameraRequest { request_id: 7, ..Default::default() }),
        "pull" => clanto_forward::Union::CameraPull(ClantoCameraPull { stream_id: 2, index: 0, ..Default::default() }),
        _ => return "error:bad mode".to_owned(),
    };
    if let Err(e) = stream.send(&wrap(first)).await {
        return format!("error:send {e}");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let Some(Ok(bytes)) = stream.next_timeout(10_000).await else {
            return "error:session closed".to_owned();
        };
        let Ok(msg) = Message::parse_from_bytes(&bytes) else { continue };
        let Some(message::Union::ClantoForward(cf)) = msg.union else { continue };
        match cf.union {
            Some(clanto_forward::Union::CameraGrant(g)) if g.accepted => return "granted".to_owned(),
            Some(clanto_forward::Union::CameraGrant(g)) => return format!("denied:{}", g.message),
            Some(clanto_forward::Union::MediaClose(c)) => return format!("closed:{}", c.message),
            _ => {}
        }
    }
    "error:timeout".to_owned()
}

pub fn run(args: &[String]) {
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    match arg(0).as_str() {
        "host-setup" => {
            let ok = Config::set_permanent_password(&arg(1));
            Config::set_option("verification-method".to_owned(), "use-permanent-password".to_owned());
            Config::set_option("approve-mode".to_owned(), "password".to_owned());
            let info = crate::auth_2fa::TOTPInfo {
                name: Config::get_id(),
                secret: decode_hex(&arg(2)),
                digits: 6,
                created_at: hbb_common::get_time(),
            };
            match info.into_string() {
                Ok(v) => Config::set_option("2fa".to_owned(), v),
                Err(e) => println!("2FA setup failed: {e}"),
            }
            println!(
                "HOST_ID={} PASSWORD_SET={} TFA={}",
                Config::get_id(),
                ok,
                crate::auth_2fa::get_2fa(None).is_some()
            );
        }
        "host-trust" => {
            let key = crate::clanto::trusted_controllers::OPT_TRUSTED_CONTROLLERS;
            crate::ipc::set_option(key, &arg(1));
            let stored = crate::ipc::get_options().get(key).cloned().unwrap_or_default();
            println!("TRUSTED={stored}");
        }
        "host-option" => {
            crate::ipc::set_option(&arg(1), &arg(2));
            let stored = crate::ipc::get_options().get(&arg(1)).cloned().unwrap_or_default();
            println!("OPTION {}={}", arg(1), if arg(1) == "2fa" { "<redacted>".to_owned() } else { stored });
        }
        "device-key" => println!(
            "DEVICE_KEY={}",
            crate::clanto::trusted_controllers::device_public_key().unwrap_or_default()
        ),
        "my-id" => println!("MY_ID={}", Config::get_id()),
        // Opzione nella configurazione di questo utente (lato controller).
        "local-option" => {
            Config::set_option(arg(1), arg(2));
            println!("LOCAL {}={}", arg(1), Config::get_option(&arg(1)));
        }
        // forward <host-id> <password> <modo>
        "forward" => {
            let result = forward(
                LoginArgs {
                    host: arg(1),
                    password: arg(2),
                    totp: None,
                    hwid: false,
                    no_key: false,
                    as_id: None,
                },
                arg(3),
            );
            println!("RESULT={result}");
        }
        // camera <host-id> <password> <frame>
        "camera" => {
            let result = camera(
                LoginArgs {
                    host: arg(1),
                    password: arg(2),
                    totp: None,
                    hwid: false,
                    no_key: false,
                    as_id: None,
                },
                arg(3).parse().unwrap_or(30),
            );
            println!("RESULT={result}");
        }
        // camera-ask <host-id> <password> <request|pull>
        "camera-ask" => {
            let result = camera_ask(
                LoginArgs {
                    host: arg(1),
                    password: arg(2),
                    totp: None,
                    hwid: false,
                    no_key: false,
                    as_id: None,
                },
                arg(3),
            );
            println!("RESULT={result}");
        }
        "login" => {
            let flags = &args[4.min(args.len())..];
            let totp = arg(3);
            let result = login(LoginArgs {
                host: arg(1),
                password: arg(2),
                totp: (!totp.is_empty() && totp != "-").then(|| decode_hex(&totp)),
                hwid: flags.iter().any(|f| f == "hwid"),
                no_key: flags.iter().any(|f| f == "no-key"),
                as_id: flags
                    .iter()
                    .find_map(|f| f.strip_prefix("as-id=").map(str::to_owned)),
            });
            println!("RESULT={result}");
        }
        other => println!("unknown e2e command: {other}"),
    }
}
