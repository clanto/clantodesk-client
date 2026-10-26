// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Destinazioni aggiuntive per gli eventi di audit.
//!
//! Gli eventi nascono sulla macchina controllata e vengono duplicati verso le
//! destinazioni configurate dall'amministratore:
//!
//!   - syslog RFC 5424 o CEF, su UDP, TCP o TLS
//!   - Windows Event Log, canale Application
//!
//! Entrambe sono disattivate per default e indipendenti dall'API server. La
//! consegna syslog usa un solo worker e non rallenta la sessione.

#[cfg(target_os = "windows")]
use hbb_common::log;
use hbb_common::{config::Config, tokio};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, OnceLock,
    },
    time::Duration,
};

// Chiavi non registrate in KEYS_SETTINGS (submodule upstream): Config le salva
// e le legge comunque, e si possono imporre via config firmata o GPO.
pub const OPT_SYSLOG_ENABLED: &str = "clanto-syslog-enabled";
pub const OPT_SYSLOG_HOST: &str = "clanto-syslog-host";
pub const OPT_SYSLOG_PORT: &str = "clanto-syslog-port";
pub const OPT_SYSLOG_PROTOCOL: &str = "clanto-syslog-protocol";
pub const OPT_SYSLOG_FACILITY: &str = "clanto-syslog-facility";
pub const OPT_SYSLOG_TLS_CA: &str = "clanto-syslog-tls-ca";
pub const OPT_SYSLOG_FORMAT: &str = "clanto-syslog-format";
pub const OPT_EVENTLOG_ENABLED: &str = "clanto-eventlog-enabled";
pub const KIND_TEMP_PASSWORD_ROTATION: &str = "temp_password_rotation";

const DEFAULT_PORT: u16 = 514;
const DEFAULT_TLS_PORT: u16 = 6514;
const DEFAULT_FACILITY: u8 = 16;
const QUEUE_CAPACITY: usize = 1000;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const EVENT_SOURCE: &str = "ClantoDesk";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum EventKey {
    ConnectionOpened,
    SessionOpened,
    ConnectionClosed,
    SessionCompleted,
    ControllerIdentityReceived,
    FileRemoteSend,
    FileRemoteReceive,
    AlarmIpWhitelist,
    AlarmExceedThirtyAttempts,
    AlarmSixAttemptsWithinOneMinute,
    AlarmExceedIpv6PrefixAttempts,
    AlarmTerminalOsLoginBackoff,
    AlarmTerminalOsLoginConcurrency,
    AlarmSessionScopeViolation,
    AlarmIdWhitelist,
    AlarmPostureRejected,
    AlarmPasswordWrong,
    AlarmTwoFactorWrong,
    PosturePassed,
    PostureBypass,
    TemporaryPasswordRotated,
    FileMalwareDetected,
    FileUnscannable,
    FileDirectionBlocked,
    FileExtensionBlocked,
    FileScannedClean,
    BinarySignatureInvalid,
    ForwardOpened,
    ForwardDenied,
    ForwardListenOpened,
    ForwardListenDenied,
    ForwardCameraOpened,
    ForwardCameraDenied,
    ForwardCameraClosed,
    Unknown,
}

#[derive(Clone, Copy, Debug)]
struct EventDefinition {
    key: EventKey,
    id: u32,
    name: &'static str,
    syslog_severity: u8,
    cef_severity: u8,
}

const EVENTS: &[EventDefinition] = &[
    EventDefinition {
        key: EventKey::ConnectionOpened,
        id: 61100,
        name: "Connection opened",
        syslog_severity: 6,
        cef_severity: 3,
    },
    EventDefinition {
        key: EventKey::SessionOpened,
        id: 61101,
        name: "Session opened",
        syslog_severity: 6,
        cef_severity: 3,
    },
    EventDefinition {
        key: EventKey::ConnectionClosed,
        id: 61102,
        name: "Connection closed",
        syslog_severity: 6,
        cef_severity: 3,
    },
    EventDefinition {
        key: EventKey::SessionCompleted,
        id: 61103,
        name: "Session completed",
        syslog_severity: 6,
        cef_severity: 3,
    },
    EventDefinition {
        key: EventKey::ControllerIdentityReceived,
        id: 61104,
        name: "Controller identity received",
        syslog_severity: 6,
        cef_severity: 3,
    },
    EventDefinition {
        key: EventKey::FileRemoteSend,
        id: 61110,
        name: "File sent to peer",
        syslog_severity: 5,
        cef_severity: 5,
    },
    EventDefinition {
        key: EventKey::FileRemoteReceive,
        id: 61111,
        name: "File received from peer",
        syslog_severity: 5,
        cef_severity: 5,
    },
    EventDefinition {
        key: EventKey::AlarmIpWhitelist,
        id: 61120,
        name: "IP whitelist rejected",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::AlarmExceedThirtyAttempts,
        id: 61121,
        name: "Login attempt limit exceeded",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::AlarmSixAttemptsWithinOneMinute,
        id: 61122,
        name: "Login rate limit exceeded",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::AlarmExceedIpv6PrefixAttempts,
        id: 61123,
        name: "IPv6 prefix attempt limit exceeded",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::AlarmTerminalOsLoginBackoff,
        id: 61124,
        name: "Terminal OS login backoff",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::AlarmTerminalOsLoginConcurrency,
        id: 61125,
        name: "Terminal OS login concurrency rejected",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::AlarmSessionScopeViolation,
        id: 61126,
        name: "Session scope violation",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::AlarmIdWhitelist,
        id: 61127,
        name: "ID whitelist rejected",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::AlarmPostureRejected,
        id: 61128,
        name: "Device posture rejected",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::AlarmPasswordWrong,
        id: 61129,
        name: "Password authentication failed",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::AlarmTwoFactorWrong,
        id: 61130,
        name: "Two-factor authentication failed",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::PosturePassed,
        id: 61140,
        name: "Device posture passed",
        syslog_severity: 6,
        cef_severity: 3,
    },
    EventDefinition {
        key: EventKey::PostureBypass,
        id: 61141,
        name: "Device posture bypassed",
        syslog_severity: 5,
        cef_severity: 5,
    },
    EventDefinition {
        key: EventKey::TemporaryPasswordRotated,
        id: 61150,
        name: "Temporary password rotated",
        syslog_severity: 5,
        cef_severity: 5,
    },
    EventDefinition {
        key: EventKey::FileMalwareDetected,
        id: 61160,
        name: "File malware detected",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::FileUnscannable,
        id: 61161,
        name: "File could not be scanned",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::FileDirectionBlocked,
        id: 61162,
        name: "File transfer direction blocked",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::FileExtensionBlocked,
        id: 61163,
        name: "File extension blocked",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::FileScannedClean,
        id: 61164,
        name: "File scanned clean",
        syslog_severity: 6,
        cef_severity: 3,
    },
    EventDefinition {
        key: EventKey::BinarySignatureInvalid,
        id: 61180,
        name: "ClantoDesk binary not signed by Clanto",
        syslog_severity: 3,
        cef_severity: 8,
    },
    EventDefinition {
        key: EventKey::ForwardOpened,
        id: 61170,
        name: "Forwarding channel opened",
        syslog_severity: 5,
        cef_severity: 5,
    },
    EventDefinition {
        key: EventKey::ForwardDenied,
        id: 61171,
        name: "Forwarding channel denied",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::ForwardListenOpened,
        id: 61172,
        name: "Forwarding listener opened",
        syslog_severity: 5,
        cef_severity: 5,
    },
    EventDefinition {
        key: EventKey::ForwardListenDenied,
        id: 61173,
        name: "Forwarding listener denied",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::ForwardCameraOpened,
        id: 61174,
        name: "Controller camera opened",
        syslog_severity: 5,
        cef_severity: 5,
    },
    EventDefinition {
        key: EventKey::ForwardCameraDenied,
        id: 61175,
        name: "Controller camera denied",
        syslog_severity: 4,
        cef_severity: 7,
    },
    EventDefinition {
        key: EventKey::ForwardCameraClosed,
        id: 61176,
        name: "Controller camera closed",
        syslog_severity: 6,
        cef_severity: 3,
    },
    EventDefinition {
        key: EventKey::Unknown,
        id: 61999,
        name: "Unknown audit event",
        syslog_severity: 5,
        cef_severity: 5,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Protocol {
    Udp,
    Tcp,
    Tls,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputFormat {
    Rfc5424,
    Cef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DeliveryConfig {
    host: String,
    port: u16,
    protocol: Protocol,
    format: OutputFormat,
    facility: u8,
    tls_ca: String,
}

#[derive(Clone)]
struct PendingRecord {
    sequence: u64,
    event: &'static EventDefinition,
    value: Value,
}

struct AuditQueue {
    records: VecDeque<PendingRecord>,
    capacity: usize,
    next_sequence: u64,
    lost: u64,
}

impl AuditQueue {
    fn new(capacity: usize) -> Self {
        Self {
            records: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
            next_sequence: 0,
            lost: 0,
        }
    }

    fn push(&mut self, event: &'static EventDefinition, value: Value) {
        if self.records.len() == self.capacity {
            self.records.pop_front();
            self.lost = self.lost.saturating_add(1);
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.records.push_back(PendingRecord {
            sequence,
            event,
            value,
        });
    }

    fn front(&self) -> Option<(PendingRecord, u64)> {
        self.records
            .front()
            .cloned()
            .map(|record| (record, self.lost))
    }

    fn finish(&mut self, sequence: u64, reported_lost: u64, delivered: bool) {
        let still_front = self.records.front().map(|record| record.sequence) == Some(sequence);
        if still_front {
            self.records.pop_front();
        }
        if delivered {
            let delivered_after_overflow = u64::from(!still_front);
            self.lost = self
                .lost
                .saturating_sub(reported_lost.saturating_add(delivered_after_overflow));
        }
    }

    fn is_front(&self, sequence: u64) -> bool {
        self.records.front().map(|record| record.sequence) == Some(sequence)
    }
}

struct SinkState {
    queue: Mutex<AuditQueue>,
    notify: tokio::sync::Notify,
    worker_started: AtomicBool,
}

impl SinkState {
    fn new() -> Self {
        Self {
            queue: Mutex::new(AuditQueue::new(QUEUE_CAPACITY)),
            notify: tokio::sync::Notify::new(),
            worker_started: AtomicBool::new(false),
        }
    }
}

static SINK: OnceLock<SinkState> = OnceLock::new();

/// Non usare `option2bool`: per queste chiavi ritorna true se mai impostate.
fn is_on(key: &str) -> bool {
    Config::get_option(key) == "Y"
}

/// Punto di ingresso unico per session log, syslog ed Event Log.
pub fn emit(kind: &str, value: &Value) {
    crate::clanto::session_log::on_audit(kind, value);
    let event = event_definition(kind, value);
    if is_on(OPT_EVENTLOG_ENABLED) {
        write_event_log(event, &value.to_string());
    }
    if active_delivery_config(is_on(OPT_SYSLOG_ENABLED), delivery_config()).is_none() {
        return;
    }
    let sink = SINK.get_or_init(SinkState::new);
    lock_queue(sink).push(event, value.clone());
    start_worker(sink);
    sink.notify.notify_one();
}

fn event_definition(kind: &str, value: &Value) -> &'static EventDefinition {
    let key = match kind {
        "conn" if value.get("action").and_then(Value::as_str) == Some("new") => {
            EventKey::ConnectionOpened
        }
        "conn" if value.get("action").and_then(Value::as_str) == Some("close") => {
            EventKey::ConnectionClosed
        }
        "conn" if value.get("action").and_then(Value::as_str) == Some("controller_identity") => {
            EventKey::ControllerIdentityReceived
        }
        "conn" if value.get("type").and_then(Value::as_str) == Some("clanto_posture") => {
            EventKey::PosturePassed
        }
        "conn" if value.get("type").and_then(Value::as_str) == Some("clanto_posture_bypass") => {
            EventKey::PostureBypass
        }
        "conn" if value.get("peer").is_some() => EventKey::SessionOpened,
        "session" => EventKey::SessionCompleted,
        "file" => match value.get("type").and_then(Value::as_i64) {
            Some(0) => EventKey::FileRemoteSend,
            Some(1) => EventKey::FileRemoteReceive,
            _ => EventKey::Unknown,
        },
        "alarm" => match value.get("typ").and_then(Value::as_i64) {
            Some(0) => EventKey::AlarmIpWhitelist,
            Some(1) => EventKey::AlarmExceedThirtyAttempts,
            Some(2) => EventKey::AlarmSixAttemptsWithinOneMinute,
            Some(6) => EventKey::AlarmExceedIpv6PrefixAttempts,
            Some(7) => EventKey::AlarmTerminalOsLoginBackoff,
            Some(8) => EventKey::AlarmTerminalOsLoginConcurrency,
            Some(9) => EventKey::AlarmSessionScopeViolation,
            Some(10) => EventKey::AlarmIdWhitelist,
            Some(11) => EventKey::AlarmPostureRejected,
            Some(12) => EventKey::AlarmPasswordWrong,
            Some(13) => EventKey::AlarmTwoFactorWrong,
            _ => EventKey::Unknown,
        },
        KIND_TEMP_PASSWORD_ROTATION => EventKey::TemporaryPasswordRotated,
        "clanto_file_malware" => EventKey::FileMalwareDetected,
        "clanto_file_unscannable" => EventKey::FileUnscannable,
        "clanto_file_direction_blocked" => EventKey::FileDirectionBlocked,
        "clanto_file_extension_blocked" => EventKey::FileExtensionBlocked,
        "clanto_file_clean" => EventKey::FileScannedClean,
        "clanto_binary_signature" => EventKey::BinarySignatureInvalid,
        "clanto_forward_opened" => EventKey::ForwardOpened,
        "clanto_forward_denied" => EventKey::ForwardDenied,
        "clanto_forward_listen" => EventKey::ForwardListenOpened,
        "clanto_forward_listen_denied" => EventKey::ForwardListenDenied,
        "clanto_forward_camera_opened" => EventKey::ForwardCameraOpened,
        "clanto_forward_camera_denied" => EventKey::ForwardCameraDenied,
        "clanto_forward_camera_closed" => EventKey::ForwardCameraClosed,
        _ => EventKey::Unknown,
    };
    EVENTS
        .iter()
        .find(|definition| definition.key == key)
        .unwrap_or(&EVENTS[EVENTS.len() - 1])
}

/// TLS e' il predefinito: UDP e TCP mandano in chiaro IP, ID e nomi dei peer.
fn warn_unencrypted_once() {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| hbb_common::log::warn!("Syslog audit is sent unencrypted: use clanto-syslog-protocol=tls"));
}

fn delivery_config() -> DeliveryConfig {
    let protocol = match Config::get_option(OPT_SYSLOG_PROTOCOL)
        .to_ascii_lowercase()
        .as_str()
    {
        "udp" => Protocol::Udp,
        "tcp" => Protocol::Tcp,
        _ => Protocol::Tls,
    };
    if protocol != Protocol::Tls {
        warn_unencrypted_once();
    }
    let configured_port = Config::get_option(OPT_SYSLOG_PORT);
    let default_port = if protocol == Protocol::Tls {
        DEFAULT_TLS_PORT
    } else {
        DEFAULT_PORT
    };
    DeliveryConfig {
        host: Config::get_option(OPT_SYSLOG_HOST),
        port: configured_port.parse::<u16>().unwrap_or(default_port),
        protocol,
        format: if Config::get_option(OPT_SYSLOG_FORMAT).eq_ignore_ascii_case("cef") {
            OutputFormat::Cef
        } else {
            OutputFormat::Rfc5424
        },
        facility: Config::get_option(OPT_SYSLOG_FACILITY)
            .parse::<u8>()
            .ok()
            .filter(|facility| *facility <= 23)
            .unwrap_or(DEFAULT_FACILITY),
        tls_ca: Config::get_option(OPT_SYSLOG_TLS_CA),
    }
}

fn active_delivery_config(enabled: bool, config: DeliveryConfig) -> Option<DeliveryConfig> {
    (enabled && !config.host.is_empty()).then_some(config)
}

fn lock_queue(sink: &SinkState) -> MutexGuard<'_, AuditQueue> {
    sink.queue.lock().unwrap_or_else(|error| error.into_inner())
}

fn start_worker(sink: &'static SinkState) {
    if sink.worker_started.load(Ordering::Acquire) {
        return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        hbb_common::throttled_log!(
            Duration::from_secs(30),
            error,
            "syslog audit worker: Tokio runtime non disponibile"
        );
        return;
    };
    if sink
        .worker_started
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        runtime.spawn(worker(sink));
    }
}

async fn next_record(sink: &'static SinkState) -> (PendingRecord, u64) {
    loop {
        let notified = sink.notify.notified();
        if let Some(snapshot) = {
            let queue = lock_queue(sink);
            queue.front()
        } {
            return snapshot;
        }
        notified.await;
    }
}

fn finish_record(sink: &SinkState, sequence: u64, reported_lost: u64, delivered: bool) {
    lock_queue(sink).finish(sequence, reported_lost, delivered);
}

fn is_front(sink: &SinkState, sequence: u64) -> bool {
    lock_queue(sink).is_front(sequence)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TransportKey {
    host: String,
    port: u16,
    protocol: Protocol,
    tls_ca: String,
}

impl From<&DeliveryConfig> for TransportKey {
    fn from(config: &DeliveryConfig) -> Self {
        Self {
            host: config.host.clone(),
            port: config.port,
            protocol: config.protocol,
            tls_ca: config.tls_ca.clone(),
        }
    }
}

enum ConnectedStream {
    Tcp(tokio::net::TcpStream),
    Tls(tokio_rustls::client::TlsStream<tokio::net::TcpStream>),
}

enum DeliveryError {
    Permanent(String),
    Retryable(String),
}

impl DeliveryError {
    fn message(&self) -> &str {
        match self {
            Self::Permanent(message) | Self::Retryable(message) => message,
        }
    }
}

struct ActiveConnection {
    key: TransportKey,
    stream: ConnectedStream,
}

async fn worker(sink: &'static SinkState) {
    let mut connection: Option<ActiveConnection> = None;
    let mut backoff = Duration::from_secs(1);
    loop {
        let (record, lost) = next_record(sink).await;
        let Some(config) = active_delivery_config(is_on(OPT_SYSLOG_ENABLED), delivery_config())
        else {
            finish_record(sink, record.sequence, lost, false);
            connection = None;
            backoff = Duration::from_secs(1);
            continue;
        };
        let message = format_record(record.event, &record.value, &config, lost);
        if config.protocol == Protocol::Udp {
            let result = tokio::time::timeout(IO_TIMEOUT, send_udp(&config, &message)).await;
            let sent = matches!(result, Ok(Ok(())));
            if !sent {
                log_delivery_error("udp", timeout_result(result));
            }
            finish_record(sink, record.sequence, lost, sent);
            connection = None;
            backoff = Duration::from_secs(1);
            continue;
        }

        let result =
            tokio::time::timeout(IO_TIMEOUT, send_stream(&config, &message, &mut connection)).await;
        match result {
            Ok(Ok(())) => {
                finish_record(sink, record.sequence, lost, true);
                backoff = Duration::from_secs(1);
            }
            Ok(Err(error)) => {
                connection = None;
                log_delivery_error(
                    if config.protocol == Protocol::Tls {
                        "tls"
                    } else {
                        "tcp"
                    },
                    error.message().to_owned(),
                );
                match error {
                    DeliveryError::Permanent(_) => {
                        finish_record(sink, record.sequence, lost, false);
                        backoff = Duration::from_secs(1);
                    }
                    DeliveryError::Retryable(_) => {
                        if is_front(sink, record.sequence) {
                            tokio::time::sleep(backoff).await;
                            backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);
                        } else {
                            backoff = Duration::from_secs(1);
                        }
                    }
                }
            }
            Err(_) => {
                connection = None;
                log_delivery_error(
                    if config.protocol == Protocol::Tls {
                        "tls"
                    } else {
                        "tcp"
                    },
                    "timeout".to_owned(),
                );
                if is_front(sink, record.sequence) {
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);
                } else {
                    backoff = Duration::from_secs(1);
                }
            }
        }
    }
}

fn timeout_result(result: Result<Result<(), String>, tokio::time::error::Elapsed>) -> String {
    match result {
        Ok(Ok(())) => String::new(),
        Ok(Err(error)) => error,
        Err(_) => "timeout".to_owned(),
    }
}

fn log_delivery_error(protocol: &str, error: String) {
    hbb_common::throttled_log!(
        Duration::from_secs(30),
        error,
        "syslog {}: {}",
        protocol,
        error
    );
}

async fn send_udp(config: &DeliveryConfig, message: &str) -> Result<(), String> {
    let bind = if config.host.contains(':') {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let socket = tokio::net::UdpSocket::bind(bind)
        .await
        .map_err(|error| format!("bind: {error}"))?;
    socket
        .send_to(message.as_bytes(), target(config))
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn send_stream(
    config: &DeliveryConfig,
    message: &str,
    connection: &mut Option<ActiveConnection>,
) -> Result<(), DeliveryError> {
    let key = TransportKey::from(config);
    if connection.as_ref().is_some_and(|active| active.key != key) {
        *connection = None;
    }
    if connection.is_none() {
        *connection = Some(connect_stream(config, key).await?);
    }
    let frame = octet_counting_frame(message);
    let result = match &mut connection
        .as_mut()
        .ok_or_else(|| DeliveryError::Retryable("connessione non disponibile".to_owned()))?
        .stream
    {
        ConnectedStream::Tcp(stream) => write_frame(stream, &frame).await,
        ConnectedStream::Tls(stream) => write_frame(stream, &frame).await,
    };
    result.map_err(|error| DeliveryError::Retryable(error.to_string()))
}

async fn write_frame<W>(stream: &mut W, frame: &[u8]) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    stream.write_all(frame).await?;
    stream.flush().await
}

async fn connect_stream(
    config: &DeliveryConfig,
    key: TransportKey,
) -> Result<ActiveConnection, DeliveryError> {
    if config.protocol == Protocol::Tls {
        return connect_tls(config, key).await;
    }
    let stream = tokio::net::TcpStream::connect(target(config))
        .await
        .map_err(|error| DeliveryError::Retryable(error.to_string()))?;
    Ok(ActiveConnection {
        key,
        stream: ConnectedStream::Tcp(stream),
    })
}

async fn connect_tls(
    config: &DeliveryConfig,
    key: TransportKey,
) -> Result<ActiveConnection, DeliveryError> {
    use tokio_rustls::rustls::pki_types::ServerName;

    if config.tls_ca.is_empty() {
        return Err(DeliveryError::Permanent(
            "CA TLS non configurata".to_owned(),
        ));
    }
    let pem = tokio::fs::read(&config.tls_ca).await.map_err(|error| {
        DeliveryError::Permanent(format!("CA TLS '{}': {error}", config.tls_ca))
    })?;
    let tls_config = tls_config_from_pem(&pem).map_err(DeliveryError::Permanent)?;
    let server_name =
        ServerName::try_from(tls_server_name(&config.host).to_owned()).map_err(|error| {
            DeliveryError::Permanent(format!("nome server TLS '{}': {error}", config.host))
        })?;
    let stream = tokio::net::TcpStream::connect(target(config))
        .await
        .map_err(|error| DeliveryError::Retryable(error.to_string()))?;
    let stream = tokio_rustls::TlsConnector::from(Arc::new(tls_config))
        .connect(server_name, stream)
        .await
        .map_err(|error| DeliveryError::Retryable(error.to_string()))?;
    Ok(ActiveConnection {
        key,
        stream: ConnectedStream::Tls(stream),
    })
}

fn tls_config_from_pem(pem: &[u8]) -> Result<tokio_rustls::rustls::ClientConfig, String> {
    use tokio_rustls::rustls::{
        crypto::ring,
        pki_types::{pem::PemObject, CertificateDer},
        ClientConfig, RootCertStore,
    };

    let certificates = CertificateDer::pem_slice_iter(pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("lettura PEM della CA TLS: {error}"))?;
    let mut roots = RootCertStore::empty();
    let (valid, invalid) = roots.add_parsable_certificates(certificates);
    if valid == 0 {
        return Err("il file CA TLS non contiene certificati validi".to_owned());
    }
    if invalid > 0 {
        hbb_common::throttled_log!(
            Duration::from_secs(30),
            warn,
            "file CA TLS: {} certificati non validi ignorati",
            invalid
        );
    }
    ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("configurazione versioni TLS: {error}"))
        .map(|builder| builder.with_root_certificates(roots).with_no_client_auth())
}

fn target(config: &DeliveryConfig) -> String {
    if config.host.contains(':') && !config.host.starts_with('[') {
        format!("[{}]:{}", config.host, config.port)
    } else {
        format!("{}:{}", config.host, config.port)
    }
}

fn tls_server_name(host: &str) -> &str {
    let Some(unbracketed) = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    else {
        return host;
    };
    if unbracketed.parse::<std::net::IpAddr>().is_ok() {
        unbracketed
    } else {
        host
    }
}

fn octet_counting_frame(message: &str) -> Vec<u8> {
    format!("{} {}", message.as_bytes().len(), message).into_bytes()
}

fn format_record(
    event: &EventDefinition,
    value: &Value,
    config: &DeliveryConfig,
    lost: u64,
) -> String {
    let value = value_with_lost(value, lost);
    match config.format {
        OutputFormat::Rfc5424 => format_rfc5424(event, &value, config.facility, lost),
        OutputFormat::Cef => format_cef(event, &value),
    }
}

fn value_with_lost(value: &Value, lost: u64) -> Value {
    let mut value = value.clone();
    if lost > 0 {
        if let Some(object) = value.as_object_mut() {
            object.insert("lost_records".to_owned(), Value::from(lost));
        }
    }
    value
}

fn format_rfc5424(event: &EventDefinition, value: &Value, facility: u8, lost: u64) -> String {
    let pri = facility as u16 * 8 + event.syslog_severity as u16;
    let timestamp = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let hostname = crate::common::hostname();
    let hostname = if hostname.is_empty() { "-" } else { &hostname };
    let structured_data = format!(
        "[clanto@32473 eventId=\"{}\" eventName=\"{}\" severity=\"{}\" lostRecords=\"{}\"]",
        event.id,
        escape_structured_data(event.name),
        event.syslog_severity,
        lost
    );
    format!(
        "<{}>1 {} {} {} {} {} {} {}",
        pri,
        timestamp,
        hostname,
        EVENT_SOURCE,
        std::process::id(),
        event.id,
        structured_data,
        value
    )
}

fn escape_structured_data(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(']', "\\]")
}

fn format_cef(event: &EventDefinition, value: &Value) -> String {
    let extensions = cef_extensions(value)
        .into_iter()
        .map(|(key, value)| format!("{}={}", key, escape_cef_extension(&value)))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "CEF:0|{}|{}|{}|{}|{}|{}|{}",
        escape_cef_header("Clanto Services"),
        escape_cef_header(EVENT_SOURCE),
        escape_cef_header(env!("CARGO_PKG_VERSION")),
        event.id,
        escape_cef_header(event.name),
        event.cef_severity,
        extensions
    )
}

fn cef_extensions(value: &Value) -> BTreeMap<String, String> {
    let mut extensions = BTreeMap::new();
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            let key = if key == "ip" { "src" } else { key };
            extensions.insert(cef_extension_key(key), json_value_text(value));
        }
        if let Some(peer_id) = object.get("peer_id") {
            extensions
                .entry("peer".to_owned())
                .or_insert_with(|| json_value_text(peer_id));
        }
        if let Some(info) = object
            .get("info")
            .and_then(Value::as_str)
            .and_then(|info| serde_json::from_str::<Value>(info).ok())
            .and_then(|info| info.as_object().cloned())
        {
            if let Some(ip) = info.get("ip") {
                extensions
                    .entry("src".to_owned())
                    .or_insert_with(|| json_value_text(ip));
            }
            if let Some(id) = info.get("id") {
                extensions
                    .entry("peer".to_owned())
                    .or_insert_with(|| json_value_text(id));
            }
            if let Some(reason) = info.get("reason") {
                extensions
                    .entry("reason".to_owned())
                    .or_insert_with(|| json_value_text(reason));
            }
        }
    }
    extensions
}

fn cef_extension_key(key: &str) -> String {
    key.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn json_value_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn escape_cef_header(value: &str) -> String {
    value.replace('\\', "\\\\").replace('|', "\\|")
}

fn escape_cef_extension(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('=', "\\=")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

#[cfg(not(target_os = "windows"))]
fn write_event_log(_event: &EventDefinition, _payload: &str) {}

/// `EventLogMessages.dll` del .NET definisce gli ID 1-65535 come "%1", quindi
/// rende qualsiasi Event ID. Elenco separato da ";": vince il primo che esiste.
#[cfg(target_os = "windows")]
const EVENT_MESSAGE_FILE: &str = concat!(
    r"%SystemRoot%\Microsoft.NET\Framework64\v4.0.30319\EventLogMessages.dll",
    ";",
    r"%SystemRoot%\Microsoft.NET\Framework\v4.0.30319\EventLogMessages.dll"
);

/// Registra la sorgente eventi in HKLM, una volta per processo. Serve perche'
/// l'MSI non gira su EXE autoestraente e aggiornamento automatico. Senza
/// privilegi l'evento si scrive comunque, solo senza descrizione.
#[cfg(target_os = "windows")]
fn ensure_event_source_registered() {
    use std::sync::Once;
    use winreg::{enums::*, RegKey};

    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let path = format!(
            r"SYSTEM\CurrentControlSet\Services\EventLog\Application\{}",
            EVENT_SOURCE
        );
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

        // Non sovrascrivere: l'MSI o l'amministratore possono avere di meglio.
        if let Ok(key) = hklm.open_subkey(&path) {
            if key
                .get_value::<String, _>("EventMessageFile")
                .map_or(false, |v| !v.is_empty())
            {
                return;
            }
        }

        match hklm.create_subkey(&path) {
            Ok((key, _)) => {
                // REG_EXPAND_SZ: %SystemRoot% deve essere espanso da Windows.
                let mut v = winreg::RegValue {
                    bytes: Vec::new(),
                    vtype: REG_EXPAND_SZ,
                };
                v.bytes = EVENT_MESSAGE_FILE
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .flat_map(|c| c.to_le_bytes())
                    .collect();
                if let Err(e) = key.set_raw_value("EventMessageFile", &v) {
                    log::debug!("EventMessageFile: {}", e);
                }
                // 7 = Error | Warning | Information
                if let Err(e) = key.set_value("TypesSupported", &7u32) {
                    log::debug!("TypesSupported: {}", e);
                }
                log::info!("sorgente eventi '{}' registrata", EVENT_SOURCE);
            }
            Err(e) => log::debug!(
                "registrazione sorgente eventi non riuscita (servono privilegi HKLM): {}",
                e
            ),
        }
    });
}

/// Scrive nel canale Application dell'Event Log.
#[cfg(target_os = "windows")]
fn write_event_log(event: &EventDefinition, payload: &str) {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::{
        winbase::{DeregisterEventSource, RegisterEventSourceW, ReportEventW},
        winnt::{EVENTLOG_ERROR_TYPE, EVENTLOG_INFORMATION_TYPE, EVENTLOG_WARNING_TYPE},
    };

    ensure_event_source_registered();

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let event_type = match event.syslog_severity {
        0..=3 => EVENTLOG_ERROR_TYPE,
        4 => EVENTLOG_WARNING_TYPE,
        _ => EVENTLOG_INFORMATION_TYPE,
    };
    unsafe {
        let source = wide(EVENT_SOURCE);
        let handle = RegisterEventSourceW(std::ptr::null(), source.as_ptr());
        if handle.is_null() {
            log::debug!(
                "RegisterEventSourceW fallita: {}",
                std::io::Error::last_os_error()
            );
            return;
        }
        let msg = wide(payload);
        let mut strings = [msg.as_ptr()];
        let ok = ReportEventW(
            handle,
            event_type,
            0,
            event.id,
            std::ptr::null_mut(),
            strings.len() as u16,
            0,
            strings.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        if ok == 0 {
            log::debug!("ReportEventW fallita: {}", std::io::Error::last_os_error());
        }
        DeregisterEventSource(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    fn test_config() -> DeliveryConfig {
        DeliveryConfig {
            host: "collector.example".to_owned(),
            port: DEFAULT_PORT,
            protocol: Protocol::Tcp,
            format: OutputFormat::Rfc5424,
            facility: DEFAULT_FACILITY,
            tls_ca: String::new(),
        }
    }

    #[test]
    fn octet_counting_uses_message_byte_length() {
        assert_eq!(octet_counting_frame("hello"), b"5 hello");
        assert_eq!(octet_counting_frame("è"), "2 è".as_bytes());
    }

    #[test]
    fn tls_server_name_removes_only_ipv6_brackets() {
        assert_eq!(tls_server_name("[2001:db8::1]"), "2001:db8::1");
        assert_eq!(tls_server_name("2001:db8::1"), "2001:db8::1");
        assert_eq!(
            tls_server_name("[collector.example]"),
            "[collector.example]"
        );

        let mut config = test_config();
        config.host = "[2001:db8::1]".to_owned();
        config.port = DEFAULT_TLS_PORT;
        assert_eq!(target(&config), "[2001:db8::1]:6514");
    }

    #[test]
    fn disabled_or_empty_syslog_config_is_inactive() {
        assert!(active_delivery_config(false, test_config()).is_none());
        let mut config = test_config();
        config.host.clear();
        assert!(active_delivery_config(true, config).is_none());
        assert!(active_delivery_config(true, test_config()).is_some());
    }

    #[test]
    fn pending_record_uses_the_config_of_each_attempt() {
        let event = event_definition("conn", &json!({"action": "new"}));
        let mut queue = AuditQueue::new(1);
        queue.push(event, json!({"action": "new"}));
        let (record, lost) = queue.front().unwrap();

        let rfc5424 = format_record(record.event, &record.value, &test_config(), lost);
        let mut updated = test_config();
        updated.format = OutputFormat::Cef;
        let cef = format_record(record.event, &record.value, &updated, lost);
        assert!(rfc5424.starts_with('<'));
        assert!(cef.starts_with("CEF:0|"));
    }

    #[test]
    fn cef_escapes_header_and_extension_delimiters() {
        let event = EventDefinition {
            key: EventKey::Unknown,
            id: 61998,
            name: r"Name|part\tail",
            syslog_severity: 5,
            cef_severity: 5,
        };
        let record = format_cef(&event, &json!({"reason": "left=right\nnext"}));
        assert!(record.starts_with("CEF:0|Clanto Services|ClantoDesk|"));
        assert!(record.contains(r"|Name\|part\\tail|5|"));
        assert!(record.contains(r"reason=left\=right\nnext"));
    }

    #[test]
    fn cef_includes_session_code_extension() {
        let event = event_definition("conn", &json!({"peer": ["id", "name"]}));
        let record = format_cef(event, &json!({"session_code": "S-0123-4567"}));
        assert!(record.contains("session_code=S-0123-4567"));
    }

    #[test]
    fn rfc5424_and_cef_use_the_same_stable_event_id() {
        let event = event_definition("alarm", &json!({"typ": 11}));
        let value = json!({"reason": "posture"});
        let rfc5424 = format_rfc5424(event, &value, DEFAULT_FACILITY, 0);
        let cef = format_cef(event, &value);
        assert!(rfc5424.contains(" 61128 [clanto@32473 eventId=\"61128\""));
        assert!(cef.contains("|61128|Device posture rejected|"));
    }

    #[test]
    fn event_table_is_unique_and_covers_all_current_variants() {
        let ids = EVENTS.iter().map(|event| event.id).collect::<HashSet<_>>();
        let keys = EVENTS.iter().map(|event| event.key).collect::<HashSet<_>>();
        assert_eq!(ids.len(), EVENTS.len());
        assert_eq!(keys.len(), EVENTS.len());
        for legacy_id in [61000, 61001, 61002, 61099] {
            assert!(!ids.contains(&legacy_id));
        }
        for alarm_type in [0, 1, 2, 6, 7, 8, 9, 10, 11, 12, 13] {
            let event = event_definition("alarm", &json!({"typ": alarm_type}));
            assert_ne!(event.key, EventKey::Unknown);
            assert_ne!(event.syslog_severity, 6);
        }
        for file_type in [0, 1] {
            assert_ne!(
                event_definition("file", &json!({"type": file_type})).key,
                EventKey::Unknown
            );
        }
        assert_eq!(
            event_definition("alarm", &json!({"typ": 11})).key,
            EventKey::AlarmPostureRejected
        );
        assert_eq!(
            event_definition("conn", &json!({"type": "clanto_posture_bypass"})).key,
            EventKey::PostureBypass
        );
        assert_eq!(
            event_definition(KIND_TEMP_PASSWORD_ROTATION, &json!({})).key,
            EventKey::TemporaryPasswordRotated
        );
        assert_eq!(
            event_definition("conn", &json!({"action": "close"})).key,
            EventKey::ConnectionClosed
        );
        assert_eq!(
            event_definition("conn", &json!({"action": "new"})).key,
            EventKey::ConnectionOpened
        );
        assert_eq!(
            event_definition("conn", &json!({"action": "controller_identity"})).key,
            EventKey::ControllerIdentityReceived
        );
        assert_eq!(
            event_definition("conn", &json!({"peer": ["id", "name"], "type": 0})).key,
            EventKey::SessionOpened
        );
        assert_eq!(
            event_definition("session", &json!({})).key,
            EventKey::SessionCompleted
        );
        assert_eq!(
            event_definition("conn", &json!({"type": "clanto_posture"})).key,
            EventKey::PosturePassed
        );
        for (kind, key, id) in [
            ("clanto_file_malware", EventKey::FileMalwareDetected, 61160),
            ("clanto_file_unscannable", EventKey::FileUnscannable, 61161),
            (
                "clanto_file_direction_blocked",
                EventKey::FileDirectionBlocked,
                61162,
            ),
            (
                "clanto_file_extension_blocked",
                EventKey::FileExtensionBlocked,
                61163,
            ),
        ] {
            let event = event_definition(kind, &json!({}));
            assert_eq!(event.key, key);
            assert_eq!(event.id, id);
            assert_eq!(event.syslog_severity, 4);
        }
        for (kind, key, id) in [
            ("clanto_forward_opened", EventKey::ForwardOpened, 61170),
            ("clanto_forward_denied", EventKey::ForwardDenied, 61171),
            ("clanto_forward_listen", EventKey::ForwardListenOpened, 61172),
            ("clanto_forward_listen_denied", EventKey::ForwardListenDenied, 61173),
            ("clanto_forward_camera_opened", EventKey::ForwardCameraOpened, 61174),
            ("clanto_forward_camera_denied", EventKey::ForwardCameraDenied, 61175),
            ("clanto_forward_camera_closed", EventKey::ForwardCameraClosed, 61176),
        ] {
            let event = event_definition(kind, &json!({}));
            assert_eq!(event.key, key);
            assert_eq!(event.id, id);
        }
    }

    #[test]
    fn queue_overflow_drops_oldest_and_counts_loss() {
        let event = event_definition("conn", &json!({"action": "new"}));
        let mut queue = AuditQueue::new(2);
        queue.push(event, json!({"record": 1}));
        queue.push(event, json!({"record": 2}));
        queue.push(event, json!({"record": 3}));
        assert_eq!(queue.records.len(), 2);
        assert_eq!(queue.records.front().map(|record| record.sequence), Some(1));
        assert_eq!(queue.lost, 1);
    }

    #[test]
    fn successful_in_flight_record_is_not_counted_lost_after_overflow() {
        let event = event_definition("conn", &json!({"action": "new"}));
        let mut queue = AuditQueue::new(2);
        queue.push(event, json!({"record": 1}));
        queue.push(event, json!({"record": 2}));

        let (in_flight, reported_lost) = queue.front().unwrap();
        queue.push(event, json!({"record": 3}));
        assert_eq!(queue.lost, 1);

        queue.finish(in_flight.sequence, reported_lost, true);
        assert_eq!(queue.lost, 0);
        assert_eq!(queue.records.front().map(|record| record.sequence), Some(1));
    }

    #[test]
    fn failed_in_flight_record_remains_counted_lost_after_overflow() {
        let event = event_definition("conn", &json!({"action": "new"}));
        let mut queue = AuditQueue::new(2);
        queue.push(event, json!({"record": 1}));
        queue.push(event, json!({"record": 2}));

        let (in_flight, _) = queue.front().unwrap();
        queue.push(event, json!({"record": 3}));
        assert!(!queue.is_front(in_flight.sequence));
        assert_eq!(queue.lost, 1);
    }
}
