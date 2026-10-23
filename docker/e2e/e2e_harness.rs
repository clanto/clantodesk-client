// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Harness E2E del login 2FA. NON fa parte del prodotto: docker/e2e/run.sh lo
//! copia in src/ solo dentro il container di test e aggiunge `--e2e` a
//! core_main. Il controller e' headless e costruisce il LoginRequest da se',
//! cosi' puo' comportarsi anche da client modificato (ID falso, hwid).
//!
//!   --e2e host-setup <password> <totp-hex>   host: password, 2FA, approvazione
//!   --e2e host-trust <elenco>                host: clanto-trusted-controllers via IPC
//!   --e2e device-key | my-id                 controller
//!   --e2e login <host-id> <password> <totp-hex|-> [hwid] [no-key] [as-id=<ID>]
//!
//! L'esito del login e' una riga `RESULT=ok|2fa-required|error:<testo>`.

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
    let lc: Arc<RwLock<LoginConfigHandler>> = Default::default();
    lc.write().unwrap().initialize(
        a.host.clone(),
        ConnType::FILE_TRANSFER,
        None,
        false,
        None,
        None,
        None,
    );
    let key = Config::get_option("key");
    let ((mut stream, ..), _) =
        match Client::start(&a.host, &key, "", ConnType::FILE_TRANSFER, Harness { lc }).await {
            Ok(v) => v,
            Err(e) => return format!("error:connect {e}"),
        };
    let my_id = a.as_id.clone().unwrap_or_else(Config::get_id);
    let hwid = if a.hwid {
        crate::get_hwid()
    } else {
        Default::default()
    };
    loop {
        let Some(Ok(bytes)) = stream.next_timeout(20_000).await else {
            return "error:timeout".to_owned();
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
                lr.set_file_transfer(FileTransfer {
                    dir: "/tmp".to_owned(),
                    ..Default::default()
                });
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
                if let Err(e) = stream.send(&m).await {
                    return format!("error:send {e}");
                }
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
                            if let Err(e) = stream.send(&m).await {
                                return format!("error:send {e}");
                            }
                            continue;
                        }
                        return "2fa-required".to_owned();
                    }
                    return format!("error:{err}");
                }
                Some(login_response::Union::PeerInfo(_)) => return "ok".to_owned(),
                _ => {}
            },
            _ => {}
        }
    }
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
        "device-key" => println!(
            "DEVICE_KEY={}",
            crate::clanto::trusted_controllers::device_public_key().unwrap_or_default()
        ),
        "my-id" => println!("MY_ID={}", Config::get_id()),
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
