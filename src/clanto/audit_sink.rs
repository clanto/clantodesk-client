//! Destinazioni aggiuntive per gli eventi di audit.
//!
//! Gli eventi nascono in `src/server/connection.rs` (`post_conn_audit`,
//! `post_file_audit`) sulla macchina controllata. Upstream li manda solo a
//! `{api_server}/api/audit/*`. Qui li duplichiamo verso destinazioni che
//! l'amministratore configura nelle impostazioni:
//!
//!   - syslog RFC 5424, UDP o TCP
//!   - Windows Event Log, canale Application
//!
//! Entrambe sono disattivate per default e completamente indipendenti
//! dall'API server: funzionano anche se `/api/audit` non esiste.
//!
//! Vincolo: non deve mai rallentare o interrompere una sessione. Ogni errore
//! viene solo loggato, l'I/O TCP gira in un task separato.

// tokio non e' dipendenza diretta del crate: si usa quello riesportato da hbb_common.
use hbb_common::{config::Config, log, tokio};
use serde_json::Value;

// Le chiavi non sono registrate in KEYS_SETTINGS di hbb_common (che e' un
// submodule upstream): `is_option_can_save()` salva comunque qualsiasi chiave,
// e `Config::get_option()` legge da OVERWRITE > CONFIG2 > DEFAULT. Quindi si
// possono anche imporre centralmente via config firmata.
pub const OPT_SYSLOG_ENABLED: &str = "clanto-syslog-enabled";
pub const OPT_SYSLOG_HOST: &str = "clanto-syslog-host";
pub const OPT_SYSLOG_PORT: &str = "clanto-syslog-port";
pub const OPT_SYSLOG_PROTOCOL: &str = "clanto-syslog-protocol"; // "udp" | "tcp"
pub const OPT_SYSLOG_FACILITY: &str = "clanto-syslog-facility"; // 0-23, default 16 (local0)
pub const OPT_EVENTLOG_ENABLED: &str = "clanto-eventlog-enabled";

const DEFAULT_PORT: u16 = 514;
const DEFAULT_FACILITY: u8 = 16; // local0
const SEVERITY_INFO: u8 = 6;
const EVENT_SOURCE: &str = "ClantoDesk";

/// NON usare `config::option2bool`: per una chiave che non inizia con "enable-"
/// o "allow-" ritorna `value != "N"`, quindi una chiave mai impostata risulta
/// **attiva**. Qui vogliamo il contrario: attivo solo se esplicitamente "Y".
fn is_on(key: &str) -> bool {
    Config::get_option(key) == "Y"
}

/// Punto di ingresso unico, chiamato da `post_conn_audit`/`post_file_audit`.
/// `kind` finisce nel MSGID del syslog ("conn", "file", "alarm").
pub fn emit(kind: &str, v: &Value) {
    let syslog = is_on(OPT_SYSLOG_ENABLED);
    let eventlog = is_on(OPT_EVENTLOG_ENABLED);
    if !syslog && !eventlog {
        return;
    }
    let payload = v.to_string();
    if syslog {
        send_syslog(kind, &payload);
    }
    if eventlog {
        write_event_log(kind, &payload);
    }
}

fn facility() -> u8 {
    Config::get_option(OPT_SYSLOG_FACILITY)
        .parse::<u8>()
        .ok()
        .filter(|f| *f <= 23)
        .unwrap_or(DEFAULT_FACILITY)
}

/// RFC 5424: <PRI>1 TIMESTAMP HOSTNAME APP-NAME PROCID MSGID STRUCTURED-DATA MSG
/// Il MSG e' il JSON dell'evento: i SIEM lo estraggono senza parser dedicati.
fn format_rfc5424(kind: &str, payload: &str) -> String {
    let pri = facility() as u16 * 8 + SEVERITY_INFO as u16;
    let ts = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let host = crate::common::hostname();
    let host = if host.is_empty() { "-".to_owned() } else { host };
    format!(
        "<{}>1 {} {} {} {} {} - {}",
        pri,
        ts,
        host,
        EVENT_SOURCE,
        std::process::id(),
        kind,
        payload
    )
}

fn send_syslog(kind: &str, payload: &str) {
    let host = Config::get_option(OPT_SYSLOG_HOST);
    if host.is_empty() {
        return;
    }
    let port = Config::get_option(OPT_SYSLOG_PORT)
        .parse::<u16>()
        .unwrap_or(DEFAULT_PORT);
    let target = if host.contains(':') && !host.starts_with('[') {
        format!("[{}]:{}", host, port) // IPv6 nudo
    } else {
        format!("{}:{}", host, port)
    };
    let msg = format_rfc5424(kind, payload);

    if Config::get_option(OPT_SYSLOG_PROTOCOL).eq_ignore_ascii_case("tcp") {
        // TCP: connect e write possono bloccare, quindi task separato con timeout.
        // RFC 6587 octet-stuffing: messaggio terminato da newline.
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let fut = async {
                let mut stream = tokio::net::TcpStream::connect(&target).await?;
                stream.write_all(msg.as_bytes()).await?;
                stream.write_all(b"\n").await?;
                stream.flush().await
            };
            match tokio::time::timeout(std::time::Duration::from_secs(5), fut).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::debug!("syslog tcp: {}", e),
                Err(_) => log::debug!("syslog tcp: timeout"),
            }
        });
    } else {
        // UDP: fire-and-forget, nessun handshake, si puo' fare inline.
        match std::net::UdpSocket::bind("0.0.0.0:0") {
            Ok(sock) => {
                if let Err(e) = sock.send_to(msg.as_bytes(), &target) {
                    log::debug!("syslog udp: {}", e);
                }
            }
            Err(e) => log::debug!("syslog udp bind: {}", e),
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn write_event_log(_kind: &str, _payload: &str) {}

/// Scrive nel canale Application dell'Event Log.
///
/// Nota: senza un message file registrato, il Visualizzatore eventi mostra
/// "impossibile trovare la descrizione", ma le stringhe dell'evento restano
/// leggibili e gli agent SIEM le raccolgono comunque. La sorgente va registrata
/// dall'MSI in HKLM\SYSTEM\CurrentControlSet\Services\EventLog\Application\ClantoDesk.
#[cfg(target_os = "windows")]
fn write_event_log(kind: &str, payload: &str) {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::winbase::{DeregisterEventSource, RegisterEventSourceW, ReportEventW};
    use winapi::um::winnt::EVENTLOG_INFORMATION_TYPE;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    // Event ID per tipo: regole di alerting SIEM semplici da scrivere.
    // Tenuti sotto 1000: l'MSI registra EventMessageFile = EventCreate.exe, il cui
    // message table copre un intervallo basso di ID. Fuori da quello il
    // Visualizzatore eventi mostrerebbe "impossibile trovare la descrizione"
    // (i dati restano leggibili, ma l'operatore vede un errore).
    // Soluzione definitiva: una DLL di messaggi nostra.
    let event_id: u32 = match kind {
        "conn" => 100,
        "file" => 101,
        "alarm" => 102,
        _ => 199,
    };

    unsafe {
        let source = wide(EVENT_SOURCE);
        let handle = RegisterEventSourceW(std::ptr::null(), source.as_ptr());
        if handle.is_null() {
            log::debug!("RegisterEventSourceW fallita: {}", std::io::Error::last_os_error());
            return;
        }
        let msg = wide(payload);
        let mut strings = [msg.as_ptr()];
        let ok = ReportEventW(
            handle,
            EVENTLOG_INFORMATION_TYPE,
            0,
            event_id,
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
