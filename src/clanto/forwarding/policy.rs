// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Chi può aprire cosa. Ogni lato decide solo su ciò che espone: l'host sulle
//! destinazioni della sua rete e sui listener locali, il controller sulle
//! destinazioni della propria rete. Tutte le opzioni sono spente di default e
//! gestibili via GPO.
//!
//! Elenco destinazioni: voci `indirizzo:porte` separate da virgole, punti e
//! virgola o spazi. Indirizzo = nome host, IPv4, `[IPv6]`, CIDR o `*`; porte =
//! `N`, `N-M` o `*`. `localhost` vale anche per 127.0.0.1 e ::1.

use super::catalog::{self, Direction};
use hbb_common::config::Config;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub const OPT_ENABLED: &str = "clanto-forward-enabled";
/// Servizi del catalogo spuntati su questo PC, `id=destinazione` separati da
/// `;`: come host (raggiungibili dal controller sulla rete di questo PC) e come
/// controller (raggiungibili dal PC controllato sulla rete di questo PC).
pub const OPT_SERVICES_HOST: &str = "clanto-forward-services-host";
pub const OPT_SERVICES_CONTROLLER: &str = "clanto-forward-services-controller";
pub const OPT_CUSTOM_HOST: &str = "clanto-forward-custom-host";
pub const OPT_CUSTOM_HOST_TARGETS: &str = "clanto-forward-custom-host-targets";
pub const OPT_REVERSE: &str = "clanto-forward-reverse";
pub const OPT_CUSTOM_CONTROLLER: &str = "clanto-forward-custom-controller";
pub const OPT_CUSTOM_CONTROLLER_TARGETS: &str = "clanto-forward-custom-controller-targets";
/// Forwarding webcam, separato da quello di rete: va acceso su entrambi i PC.
pub const OPT_WEBCAM: &str = "clanto-forward-webcam";
/// Controller: può inviare la propria webcam al PC controllato.
pub const OPT_WEBCAM_SEND: &str = "clanto-forward-webcam-send";
/// Controller: può usare la webcam del PC controllato (con il consenso di chi è lì).
pub const OPT_WEBCAM_RECEIVE: &str = "clanto-forward-webcam-receive";
/// Host Linux: dispositivo v4l2loopback da usare (vuoto = il primo trovato).
pub const OPT_CAMERA_DEVICE: &str = "clanto-forward-camera-device";

pub const GPO_KEYS: &[&str] = &[
    OPT_ENABLED,
    OPT_SERVICES_HOST,
    OPT_SERVICES_CONTROLLER,
    OPT_CUSTOM_HOST,
    OPT_CUSTOM_HOST_TARGETS,
    OPT_REVERSE,
    OPT_CUSTOM_CONTROLLER,
    OPT_CUSTOM_CONTROLLER_TARGETS,
    OPT_WEBCAM,
    OPT_WEBCAM_SEND,
    OPT_WEBCAM_RECEIVE,
    OPT_CAMERA_DEVICE,
];

pub const DENIED_DISABLED: &str = "Forwarding is disabled on this device";
pub const DENIED_TARGET: &str = "Forwarding target not allowed";
pub const DENIED_REVERSE: &str = "Forwarding towards the controller is not allowed on this device";
pub const DENIED_WEBCAM: &str = "Webcam forwarding is disabled on this device";
pub const DENIED_WEBCAM_SEND: &str = "Sending this PC's webcam is not allowed on this device";
pub const DENIED_WEBCAM_RECEIVE: &str = "Using the remote PC's webcam is not allowed on this device";
pub const DENIED_WEBCAM_CONSENT: &str = "The remote user did not allow the webcam";
pub const DENIED_PERMISSION: &str = "Forwarding is not allowed for this connection";

fn on(key: &str) -> bool {
    Config::get_option(key) == "Y"
}

/// Servizi attivi: `(id, pattern)` dalle voci valide; le altre si ignorano.
pub fn parse_services(value: &str) -> Vec<(String, Pattern)> {
    value
        .split(';')
        .filter_map(|entry| {
            let (id, target) = entry.trim().split_once('=')?;
            let id = id.trim();
            catalog::valid_id(id).then_some(())?;
            Some((id.to_owned(), parse_pattern(target.trim())?))
        })
        .collect()
}

fn services_key(direction: Direction) -> &'static str {
    match direction {
        Direction::ToHost => OPT_SERVICES_HOST,
        Direction::ToController => OPT_SERVICES_CONTROLLER,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Addr {
    Any,
    Name(String),
    Net(IpAddr, u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    addr: Addr,
    ports: (u16, u16),
}

fn parse_ports(s: &str) -> Option<(u16, u16)> {
    if s == "*" {
        return Some((1, u16::MAX));
    }
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let (a, b) = (a.trim().parse::<u16>().ok()?, b.trim().parse::<u16>().ok()?);
    (a >= 1 && a <= b).then_some((a, b))
}

fn parse_addr(s: &str) -> Option<Addr> {
    if s == "*" {
        return Some(Addr::Any);
    }
    let bare = s.trim_start_matches('[').trim_end_matches(']');
    if let Some((ip, bits)) = bare.split_once('/') {
        let ip: IpAddr = ip.parse().ok()?;
        let bits: u8 = bits.parse().ok()?;
        let max = if ip.is_ipv4() { 32 } else { 128 };
        return (bits <= max).then_some(Addr::Net(ip, bits));
    }
    if let Ok(ip) = bare.parse::<IpAddr>() {
        let bits = if ip.is_ipv4() { 32 } else { 128 };
        return Some(Addr::Net(ip, bits));
    }
    let name = bare.to_ascii_lowercase();
    let valid = !name.is_empty()
        && name.len() <= 253
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    valid.then_some(Addr::Name(name))
}

pub fn parse_pattern(item: &str) -> Option<Pattern> {
    let (addr, ports) = item.trim().rsplit_once(':')?;
    Some(Pattern {
        addr: parse_addr(addr.trim())?,
        ports: parse_ports(ports.trim())?,
    })
}

/// Voci valide e voci scartate (che non danno mai accesso).
pub fn parse_list(list: &str) -> (Vec<Pattern>, Vec<String>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    for item in list
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|s| !s.is_empty())
    {
        match parse_pattern(item) {
            Some(p) => ok.push(p),
            None => bad.push(item.to_owned()),
        }
    }
    (ok, bad)
}

fn in_net(ip: IpAddr, net: IpAddr, bits: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits as u32) };
            u32::from(a) & mask == u32::from(b) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let mask = if bits == 0 { 0 } else { u128::MAX << (128 - bits as u32) };
            u128::from(a) & mask == u128::from(b) & mask
        }
        _ => false,
    }
}

fn is_loopback_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("localhost")
}

impl Pattern {
    fn port_ok(&self, port: u16) -> bool {
        self.ports.0 <= port && port <= self.ports.1
    }

    fn name_ok(&self, name: &str) -> bool {
        match &self.addr {
            Addr::Any => true,
            Addr::Name(n) => n.eq_ignore_ascii_case(name),
            Addr::Net(..) => false,
        }
    }

    fn ip_ok(&self, ip: IpAddr) -> bool {
        match &self.addr {
            Addr::Any => true,
            Addr::Net(net, bits) => in_net(ip, *net, *bits),
            Addr::Name(n) => is_loopback_name(n) && ip.is_loopback(),
        }
    }
}

/// Confronto senza risoluzione DNS: nome uguale, IP letterale nella rete o
/// alias di loopback. Serve a trovare il servizio del catalogo richiesto.
pub fn literal_match(p: &Pattern, host: &str, port: u16) -> bool {
    if !p.port_ok(port) {
        return false;
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return p.ip_ok(ip);
    }
    p.name_ok(host) || (is_loopback_name(host) && p.ip_ok(IpAddr::V4(Ipv4Addr::LOCALHOST)))
}

/// Serve risolvere `host` per questa regola? Solo se potrebbe corrispondere: per
/// nome, o per una rete (o `*`) sulla porta giusta. Così un peer non fa
/// interrogare il DNS dell'host per nomi che nessuna regola ammette.
fn may_match(p: &Pattern, host: &str, port: u16) -> bool {
    literal_match(p, host, port) || (p.port_ok(port) && matches!(p.addr, Addr::Any | Addr::Net(..)))
}

/// Dove collegarsi davvero: un indirizzo già verificato, così un nome che
/// cambia risoluzione dopo il controllo non porta altrove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Addrs(Vec<SocketAddr>),
}

fn loopback_addrs(port: u16) -> Vec<SocketAddr> {
    vec![
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
    ]
}

/// Verifica `host:port` contro i pattern. `resolved` sono gli indirizzi del
/// nome (vuoto per un IP letterale); servono per i pattern su IP e CIDR.
pub fn check(
    patterns: &[Pattern],
    host: &str,
    port: u16,
    resolved: &[IpAddr],
) -> Option<Target> {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return patterns
            .iter()
            .any(|p| p.port_ok(port) && p.ip_ok(ip))
            .then(|| Target::Addrs(vec![SocketAddr::new(ip, port)]));
    }
    if is_loopback_name(host) {
        let allowed = patterns
            .iter()
            .any(|p| p.port_ok(port) && (p.name_ok(host) || p.ip_ok(IpAddr::V4(Ipv4Addr::LOCALHOST))));
        return allowed.then(|| Target::Addrs(loopback_addrs(port)));
    }
    if patterns.iter().any(|p| p.port_ok(port) && p.name_ok(host)) {
        let addrs: Vec<SocketAddr> = resolved.iter().map(|ip| SocketAddr::new(*ip, port)).collect();
        return (!addrs.is_empty()).then_some(Target::Addrs(addrs));
    }
    let vetted: Vec<SocketAddr> = resolved
        .iter()
        .filter(|ip| patterns.iter().any(|p| p.port_ok(port) && p.ip_ok(**ip)))
        .map(|ip| SocketAddr::new(*ip, port))
        .collect();
    (!vetted.is_empty()).then_some(Target::Addrs(vetted))
}

async fn resolve(host: &str, port: u16) -> Vec<IpAddr> {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare.parse::<IpAddr>().is_ok() || is_loopback_name(bare) {
        return Vec::new();
    }
    match hbb_common::tokio::net::lookup_host((bare, port)).await {
        Ok(addrs) => addrs.map(|a| a.ip()).collect(),
        Err(_) => Vec::new(),
    }
}

pub struct Decision {
    pub target: Target,
    /// Id del servizio del catalogo, vuoto per un forward personalizzato.
    pub service: String,
}

/// Lato che espone la propria rete: l'host per `ToHost`, il controller per
/// `ToController`. Autorizzano i servizi spuntati, con la destinazione salvata
/// quando sono stati attivati, e l'elenco personalizzato.
pub async fn authorize_target(direction: Direction, host: &str, port: i32) -> Result<Decision, String> {
    if !on(OPT_ENABLED) {
        return Err(DENIED_DISABLED.to_owned());
    }
    let port = u16::try_from(port).ok().filter(|p| *p > 0).ok_or(DENIED_TARGET)?;
    let services = parse_services(&Config::get_option(services_key(direction)));
    let service = services.into_iter().find(|(_, p)| literal_match(p, host, port));
    let service_patterns: Vec<Pattern> = service.iter().map(|(_, p)| p.clone()).collect();
    let (custom_on, list_key) = match direction {
        Direction::ToHost => (on(OPT_CUSTOM_HOST), OPT_CUSTOM_HOST_TARGETS),
        Direction::ToController => (on(OPT_CUSTOM_CONTROLLER), OPT_CUSTOM_CONTROLLER_TARGETS),
    };
    let custom_patterns = if custom_on { parse_list(&Config::get_option(list_key)).0 } else { Vec::new() };
    if !service_patterns.iter().chain(custom_patterns.iter()).any(|p| may_match(p, host, port)) {
        return Err(DENIED_TARGET.to_owned());
    }
    let resolved = resolve(host, port).await;
    if let (Some((id, _)), Some(target)) = (service, check(&service_patterns, host, port, &resolved)) {
        return Ok(Decision { target, service: id });
    }
    if let Some(target) = check(&custom_patterns, host, port, &resolved) {
        return Ok(Decision { target, service: String::new() });
    }
    Err(DENIED_TARGET.to_owned())
}

/// Host: può aprire un listener su 127.0.0.1 per un forward verso il controller?
pub fn authorize_listen() -> Result<(), String> {
    if !on(OPT_ENABLED) {
        return Err(DENIED_DISABLED.to_owned());
    }
    if !on(OPT_REVERSE) {
        return Err(DENIED_REVERSE.to_owned());
    }
    Ok(())
}

/// Host: forwarding webcam acceso (ricevere quella del controller, o dare la
/// propria dopo il consenso).
pub fn authorize_camera() -> Result<(), String> {
    if !on(OPT_WEBCAM) {
        return Err(DENIED_WEBCAM.to_owned());
    }
    Ok(())
}

/// Controller: può inviare la propria webcam?
pub fn authorize_webcam_send() -> Result<(), String> {
    authorize_camera()?;
    if !on(OPT_WEBCAM_SEND) {
        return Err(DENIED_WEBCAM_SEND.to_owned());
    }
    Ok(())
}

/// Controller: può usare la webcam del PC controllato?
pub fn authorize_webcam_receive() -> Result<(), String> {
    authorize_camera()?;
    if !on(OPT_WEBCAM_RECEIVE) {
        return Err(DENIED_WEBCAM_RECEIVE.to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn parses_patterns() {
        let (ok, bad) = parse_list("localhost:3389, 192.168.1.0/24:443;[::1]:22 nas.lan:5000-5010 *:8080 bad 10.0.0.1:0 x:99999");
        assert_eq!(ok.len(), 5);
        assert_eq!(bad, vec!["bad", "10.0.0.1:0", "x:99999"]);
    }

    #[test]
    fn loopback_aliases_match_each_other() {
        let (p, _) = parse_list("localhost:3389");
        assert!(check(&p, "127.0.0.1", 3389, &[]).is_some());
        assert!(check(&p, "::1", 3389, &[]).is_some());
        assert!(check(&p, "localhost", 3389, &[]).is_some());
        assert!(check(&p, "localhost", 3390, &[]).is_none());
        let (p, _) = parse_list("127.0.0.1:3389");
        assert!(check(&p, "localhost", 3389, &[]).is_some());
    }

    #[test]
    fn cidr_and_ports() {
        let (p, _) = parse_list("192.168.1.0/24:443,10.0.0.5:8000-8010");
        assert!(check(&p, "192.168.1.77", 443, &[]).is_some());
        assert!(check(&p, "192.168.2.77", 443, &[]).is_none());
        assert!(check(&p, "10.0.0.5", 8005, &[]).is_some());
        assert!(check(&p, "10.0.0.5", 8011, &[]).is_none());
    }

    #[test]
    fn hostname_is_checked_on_its_resolved_addresses() {
        let (p, _) = parse_list("192.168.1.0/24:443");
        // Un nome che risolve fuori dalla rete ammessa non passa.
        assert!(check(&p, "evil.example", 443, &ips(&["203.0.113.9"])).is_none());
        // Si collega solo agli indirizzi verificati.
        assert_eq!(
            check(&p, "nas.lan", 443, &ips(&["203.0.113.9", "192.168.1.10"])),
            Some(Target::Addrs(vec!["192.168.1.10:443".parse().unwrap()]))
        );
        let (p, _) = parse_list("nas.lan:443");
        assert!(check(&p, "NAS.lan", 443, &ips(&["203.0.113.9"])).is_some());
        assert!(check(&p, "other.lan", 443, &ips(&["192.168.1.10"])).is_none());
    }

    #[test]
    fn enabled_services_keep_their_target() {
        let s = parse_services("rdp=localhost:3389; ssh = 10.0.0.5:22;bad;x=nope;a b=localhost:1");
        let ids: Vec<&str> = s.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["rdp", "ssh"]);
        assert!(literal_match(&s[0].1, "127.0.0.1", 3389));
        assert!(!literal_match(&s[0].1, "127.0.0.1", 3390));
        assert!(parse_services("").is_empty());
    }

    #[test]
    fn empty_list_allows_nothing() {
        let (p, _) = parse_list("");
        assert!(check(&p, "localhost", 3389, &[]).is_none());
    }
}
