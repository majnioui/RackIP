//! A small DHCP server (RFC 2131) for the device on the cable. Same rules as
//! the macOS app (Sources/RackIPCore/DHCP.swift).
//!
//! It hands out addresses from a range with the subnet mask only: no gateway,
//! no DNS, so devices reach the laptop and nothing else.

use crate::config::{mask_from_prefix, parse_ipv4, ManualConfig};
use std::net::Ipv4Addr;

pub const DEFAULT_SERVER: Ipv4Addr = Ipv4Addr::new(192, 168, 100, 1);
pub const DEFAULT_LEASE_MINUTES: u32 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DhcpConfig {
    pub server: Ipv4Addr,
    pub prefix: u8,
    pub start: Ipv4Addr,
    pub end: Ipv4Addr,
    pub lease_secs: u32,
}

impl DhcpConfig {
    pub fn mask(&self) -> Ipv4Addr {
        mask_from_prefix(self.prefix)
    }

    /// .100 to .200 when the subnet is big enough, otherwise every host address.
    pub fn default_range(server: Ipv4Addr, prefix: u8) -> (Ipv4Addr, Ipv4Addr) {
        let mask = u32::from(mask_from_prefix(prefix));
        let network = u32::from(server) & mask;
        let size = u64::from(!mask) + 1;
        if size >= 256 {
            (Ipv4Addr::from(network + 100), Ipv4Addr::from(network + 200))
        } else {
            (Ipv4Addr::from(network + 1), Ipv4Addr::from(network + size as u32 - 2))
        }
    }

    /// Empty fields fall back to the defaults shown as placeholders.
    pub fn parse(server: &str, mask: &str, start: &str, end: &str, lease_minutes: &str) -> Result<Self, String> {
        let server = if server.trim().is_empty() { DEFAULT_SERVER.to_string() } else { server.to_string() };
        let manual = ManualConfig::parse(&server, mask, "")?;
        if manual.prefix > 30 {
            return Err("The subnet is too small to hand out addresses. Use /30 or larger.".into());
        }
        let (default_start, default_end) = Self::default_range(manual.address, manual.prefix);
        let addr = |text: &str, fallback: Ipv4Addr, label: &str| -> Result<Ipv4Addr, String> {
            let t = text.trim();
            if t.is_empty() {
                return Ok(fallback);
            }
            parse_ipv4(t).ok_or_else(|| format!("“{t}” is not a valid {label} address."))
        };
        let start = addr(start, default_start, "range start")?;
        let end = addr(end, default_end, "range end")?;
        let mask = u32::from(manual.mask());
        let network = u32::from(manual.address) & mask;
        let broadcast = network | !mask;
        for ip in [start, end] {
            let v = u32::from(ip);
            if v & mask != network || v == network || v == broadcast {
                return Err(format!("{ip} is not a host address in {}/{}.", Ipv4Addr::from(network), manual.prefix));
            }
        }
        if u32::from(start) > u32::from(end) {
            return Err("The range starts after it ends.".into());
        }
        if start == end && start == manual.address {
            return Err("The range only holds the server's own address.".into());
        }
        let minutes = match lease_minutes.trim() {
            "" => DEFAULT_LEASE_MINUTES,
            t => t.parse().unwrap_or(0),
        };
        if !(1..=10080).contains(&minutes) {
            return Err("Lease time is in minutes, from 1 to 10080 (a week).".into());
        }
        Ok(DhcpConfig { server: manual.address, prefix: manual.prefix, start, end, lease_secs: minutes * 60 })
    }

    fn in_range(&self, ip: Ipv4Addr) -> bool {
        (u32::from(self.start)..=u32::from(self.end)).contains(&u32::from(ip)) && ip != self.server
    }
}

/// An address handed to a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    /// "aa:bb:cc:dd:ee:ff". Empty for an address a device reported as taken.
    pub mac: String,
    pub ip: Ipv4Addr,
    pub hostname: String,
    /// Unix seconds.
    pub expires: u64,
    /// False while only offered, true once the device accepted it.
    pub bound: bool,
}

const DISCOVER: u8 = 1;
const OFFER: u8 = 2;
const REQUEST: u8 = 3;
const DECLINE: u8 = 4;
const ACK: u8 = 5;
const NAK: u8 = 6;
const RELEASE: u8 = 7;
const INFORM: u8 = 8;
const MAGIC: [u8; 4] = [99, 130, 83, 99];

/// The protocol without sockets: packet in, packet out.
pub struct Engine {
    pub config: DhcpConfig,
    leases: Vec<Lease>,
    /// Another DHCP server seen on the cable. The engine goes quiet once set.
    pub foreign_server: Option<Ipv4Addr>,
}

struct Request<'a> {
    raw: &'a [u8],
    kind: u8,
    ciaddr: Ipv4Addr,
    giaddr: Ipv4Addr,
    mac: String,
    requested: Option<Ipv4Addr>,
    server_id: Option<Ipv4Addr>,
    hostname: Option<String>,
}

impl<'a> Request<'a> {
    fn parse(p: &'a [u8]) -> Option<Self> {
        if p.len() < 240 || p[0] != 1 || p[236..240] != MAGIC {
            return None;
        }
        let mut options: std::collections::HashMap<u8, Vec<u8>> = Default::default();
        let mut i = 240;
        while i < p.len() {
            let code = p[i];
            if code == 255 {
                break;
            }
            if code == 0 {
                i += 1;
                continue;
            }
            let Some(&len) = p.get(i + 1) else { break };
            let end = i + 2 + len as usize;
            if end > p.len() {
                break;
            }
            options.entry(code).or_default().extend_from_slice(&p[i + 2..end]);
            i = end;
        }
        let kind = *options.get(&53)?.first()?;
        let hlen = (p[2] as usize).min(16);
        if hlen == 0 {
            return None;
        }
        let ip = |code: u8| options.get(&code).filter(|d| d.len() == 4).map(|d| Ipv4Addr::new(d[0], d[1], d[2], d[3]));
        let at = |i: usize| Ipv4Addr::new(p[i], p[i + 1], p[i + 2], p[i + 3]);
        let hostname = options
            .get(&12)
            .map(|d| String::from_utf8_lossy(&d.iter().copied().filter(|b| *b != 0).collect::<Vec<_>>()).trim().to_string())
            .filter(|s| !s.is_empty());
        Some(Request {
            raw: p,
            kind,
            ciaddr: at(12),
            giaddr: at(24),
            mac: p[28..28 + hlen].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"),
            requested: ip(50),
            server_id: ip(54),
            hostname,
        })
    }
}

impl Engine {
    pub fn new(config: DhcpConfig) -> Self {
        Engine { config, leases: Vec::new(), foreign_server: None }
    }

    /// Devices holding an address now, newest first.
    pub fn active_leases(&self, now: u64) -> Vec<Lease> {
        let mut v: Vec<Lease> = self.leases.iter().filter(|l| l.bound && !l.mac.is_empty() && l.expires > now).cloned().collect();
        v.sort_by(|a, b| b.expires.cmp(&a.expires));
        v
    }

    /// Handles one packet received on port 67. Returns the reply and where to
    /// send it (port 68), or None when there is nothing to answer.
    pub fn handle(&mut self, packet: &[u8], now: u64) -> Option<(Vec<u8>, Ipv4Addr)> {
        if self.foreign_server.is_some() {
            return None;
        }
        let req = Request::parse(packet)?;
        // Relayed requests come from other networks; this server is only for the cable.
        if !req.giaddr.is_unspecified() {
            return None;
        }
        match req.kind {
            DISCOVER => {
                let ip = self.pick(&req, now)?;
                self.record(&req, ip, false, now + 60, now);
                Some(self.reply(&req, OFFER, ip))
            }
            REQUEST => {
                if let Some(chosen) = req.server_id.filter(|s| *s != self.config.server) {
                    // The device took another server's offer.
                    self.foreign_server = Some(chosen);
                    self.leases.retain(|l| l.mac != req.mac || l.bound);
                    return None;
                }
                let wanted = req.requested.or((!req.ciaddr.is_unspecified()).then_some(req.ciaddr))?;
                if self.config.in_range(wanted) && !self.is_held(wanted, &req.mac, now) {
                    self.record(&req, wanted, true, now + u64::from(self.config.lease_secs), now);
                    Some(self.reply(&req, ACK, wanted))
                } else {
                    // Refuse so the device starts over and gets an address from the range.
                    Some(self.reply(&req, NAK, Ipv4Addr::UNSPECIFIED))
                }
            }
            DECLINE => {
                // Something else on the cable already uses it; keep it out of the pool for a while.
                let ip = req.requested?;
                self.leases.retain(|l| l.ip != ip);
                self.leases.push(Lease { mac: String::new(), ip, hostname: String::new(), expires: now + 600, bound: false });
                None
            }
            RELEASE => {
                if let Some(l) = self.leases.iter_mut().find(|l| l.mac == req.mac && l.ip == req.ciaddr) {
                    l.expires = now;
                    l.bound = false;
                }
                None
            }
            INFORM if !req.ciaddr.is_unspecified() => Some(self.reply(&req, ACK, Ipv4Addr::UNSPECIFIED)),
            _ => None,
        }
    }

    fn is_held(&self, ip: Ipv4Addr, mac: &str, now: u64) -> bool {
        self.leases.iter().any(|l| l.ip == ip && l.mac != mac && l.expires > now)
    }

    fn pick(&self, req: &Request, now: u64) -> Option<Ipv4Addr> {
        if let Some(prev) = self.leases.iter().find(|l| l.mac == req.mac) {
            if self.config.in_range(prev.ip) && !self.is_held(prev.ip, &req.mac, now) {
                return Some(prev.ip);
            }
        }
        if let Some(w) = req.requested.filter(|w| self.config.in_range(*w) && !self.is_held(*w, &req.mac, now)) {
            return Some(w);
        }
        let range = || (u32::from(self.config.start)..=u32::from(self.config.end)).map(Ipv4Addr::from).filter(|ip| self.config.in_range(*ip));
        // Never-used addresses first, so a device that comes back finds its old one free.
        range()
            .find(|ip| !self.leases.iter().any(|l| l.ip == *ip))
            .or_else(|| range().find(|ip| !self.is_held(*ip, &req.mac, now)))
    }

    fn record(&mut self, req: &Request, ip: Ipv4Addr, bound: bool, expires: u64, now: u64) {
        let prev = self.leases.iter().find(|l| l.mac == req.mac).cloned();
        self.leases.retain(|l| l.mac != req.mac && !(l.ip == ip && l.expires <= now));
        let mut lease = Lease {
            mac: req.mac.clone(),
            ip,
            hostname: req.hostname.clone().or(prev.as_ref().map(|p| p.hostname.clone())).unwrap_or_default(),
            expires,
            bound,
        };
        // A DISCOVER from a device that still holds its lease does not shorten it.
        if let Some(p) = prev.filter(|p| !bound && p.bound && p.ip == ip && p.expires > expires) {
            lease.bound = true;
            lease.expires = p.expires;
        }
        self.leases.push(lease);
    }

    fn reply(&self, req: &Request, kind: u8, yiaddr: Ipv4Addr) -> (Vec<u8>, Ipv4Addr) {
        let mut p = vec![0u8; 236];
        p[0] = 2; // BOOTREPLY
        p[1] = req.raw[1];
        p[2] = req.raw[2];
        p[4..8].copy_from_slice(&req.raw[4..8]); // xid
        p[10..12].copy_from_slice(&req.raw[10..12]); // flags
        if kind != OFFER && kind != NAK {
            p[12..16].copy_from_slice(&req.ciaddr.octets());
        }
        p[16..20].copy_from_slice(&yiaddr.octets());
        p[28..44].copy_from_slice(&req.raw[28..44]); // chaddr
        p.extend_from_slice(&MAGIC);
        let mut option = |code: u8, data: &[u8]| {
            p.push(code);
            p.push(data.len() as u8);
            p.extend_from_slice(data);
        };
        let secs = self.config.lease_secs;
        option(53, &[kind]);
        option(54, &self.config.server.octets());
        if kind == OFFER || kind == ACK {
            if kind == OFFER || !yiaddr.is_unspecified() {
                option(51, &secs.to_be_bytes());
                option(58, &(secs / 2).to_be_bytes());
                option(59, &(secs / 8 * 7).to_be_bytes());
            }
            option(1, &self.config.mask().octets());
        }
        p.push(255);
        p.resize(p.len().max(300), 0);
        // Devices without an address yet can only be reached by broadcast.
        let to = if kind != NAK && !req.ciaddr.is_unspecified() { req.ciaddr } else { Ipv4Addr::BROADCAST };
        (p, to)
    }
}

/// Runs an `Engine` on a UDP socket in a background thread.
#[cfg_attr(not(windows), allow(dead_code))]
pub mod server {
    use super::{DhcpConfig, Engine, Lease};
    use crate::store::now;
    use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::Duration;

    pub struct Server {
        engine: Arc<Mutex<Engine>>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Server {
        /// Binds `bind:67` and starts answering.
        pub fn start(bind: Ipv4Addr, config: DhcpConfig) -> std::io::Result<Server> {
            let socket = UdpSocket::bind(SocketAddrV4::new(bind, 67))?;
            socket.set_broadcast(true)?;
            socket.set_read_timeout(Some(Duration::from_millis(500)))?;
            let engine = Arc::new(Mutex::new(Engine::new(config)));
            let stop = Arc::new(AtomicBool::new(false));
            let thread = {
                let (engine, stop) = (engine.clone(), stop.clone());
                std::thread::spawn(move || {
                    let mut buf = [0u8; 1500];
                    while !stop.load(Ordering::Relaxed) {
                        let Ok((n, _)) = socket.recv_from(&mut buf) else { continue };
                        let reply = engine.lock().unwrap().handle(&buf[..n], now());
                        if let Some((packet, to)) = reply {
                            let _ = socket.send_to(&packet, SocketAddrV4::new(to, 68));
                        }
                    }
                })
            };
            Ok(Server { engine, stop, thread: Some(thread) })
        }

        pub fn leases(&self) -> Vec<Lease> {
            self.engine.lock().unwrap().active_leases(now())
        }

        pub fn foreign_server(&self) -> Option<Ipv4Addr> {
            self.engine.lock().unwrap().foreign_server
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(kind: u8, mac: u8, ciaddr: [u8; 4], options: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8; 236];
        p[0] = 1;
        p[1] = 1;
        p[2] = 6;
        p[4..8].copy_from_slice(&[0xde, 0xad, 0xbe, mac]);
        p[12..16].copy_from_slice(&ciaddr);
        p[28..34].copy_from_slice(&[0, 0x11, 0x22, 0x33, 0x44, mac]);
        p.extend_from_slice(&MAGIC);
        p.extend_from_slice(&[53, 1, kind]);
        p.extend_from_slice(options);
        p.push(255);
        p
    }

    fn yiaddr(r: &[u8]) -> Ipv4Addr {
        Ipv4Addr::new(r[16], r[17], r[18], r[19])
    }

    fn kind(r: &[u8]) -> u8 {
        let mut i = 240;
        while i + 2 < r.len() && r[i] != 255 {
            if r[i] == 53 {
                return r[i + 2];
            }
            i += 2 + r[i + 1] as usize;
        }
        0
    }

    #[test]
    fn config_defaults_and_errors() {
        let c = DhcpConfig::parse("", "", "", "", "").unwrap();
        assert_eq!((c.server, c.prefix, c.lease_secs), (DEFAULT_SERVER, 24, 3600));
        assert_eq!((c.start, c.end), (Ipv4Addr::new(192, 168, 100, 100), Ipv4Addr::new(192, 168, 100, 200)));
        let small = DhcpConfig::parse("10.0.0.1/28", "", "", "", "").unwrap();
        assert_eq!((small.start, small.end), (Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 14)));
        for (s, m, a, b, l) in [
            ("10.0.0.1", "", "10.0.1.5", "", ""),
            ("10.0.0.1", "", "10.0.0.50", "10.0.0.20", ""),
            ("10.0.0.1", "", "", "", "0"),
            ("10.0.0.1", "/31", "", "", ""),
            ("10.0.0.1", "", "10.0.0.255", "", ""),
        ] {
            assert!(DhcpConfig::parse(s, m, a, b, l).is_err(), "{s} {m} {a} {b} {l}");
        }
    }

    #[test]
    fn engine_flow() {
        let config = DhcpConfig::parse("192.168.100.1", "", "192.168.100.100", "192.168.100.101", "").unwrap();
        let mut e = Engine::new(config);
        let now = 1_000_000;
        let server_id = [54, 4, 192, 168, 100, 1];

        let mut opts = vec![12, 5];
        opts.extend_from_slice(b"idrac");
        let (offer, to) = e.handle(&packet(DISCOVER, 1, [0; 4], &opts), now).unwrap();
        assert_eq!((kind(&offer), yiaddr(&offer), to), (OFFER, Ipv4Addr::new(192, 168, 100, 100), Ipv4Addr::BROADCAST));
        assert_eq!((offer[0], &offer[4..8]), (2, &[0xde, 0xad, 0xbe, 1][..]));
        assert!(e.active_leases(now).is_empty());

        let mut req = vec![50, 4, 192, 168, 100, 100];
        req.extend_from_slice(&server_id);
        let (ack, _) = e.handle(&packet(REQUEST, 1, [0; 4], &req), now).unwrap();
        assert_eq!((kind(&ack), yiaddr(&ack)), (ACK, Ipv4Addr::new(192, 168, 100, 100)));
        let active = e.active_leases(now);
        assert_eq!(active.len(), 1);
        assert_eq!((active[0].mac.as_str(), active[0].hostname.as_str()), ("00:11:22:33:44:01", "idrac"));

        let (second, _) = e.handle(&packet(DISCOVER, 2, [0; 4], &[]), now).unwrap();
        assert_eq!(yiaddr(&second), Ipv4Addr::new(192, 168, 100, 101));
        let mut req2 = vec![50, 4, 192, 168, 100, 101];
        req2.extend_from_slice(&server_id);
        e.handle(&packet(REQUEST, 2, [0; 4], &req2), now).unwrap();
        assert!(e.handle(&packet(DISCOVER, 3, [0; 4], &[]), now).is_none(), "pool exhausted");

        let (again, _) = e.handle(&packet(DISCOVER, 1, [0; 4], &[]), now + 30).unwrap();
        assert_eq!(yiaddr(&again), Ipv4Addr::new(192, 168, 100, 100));

        let (nak, to) = e.handle(&packet(REQUEST, 4, [0; 4], &[50, 4, 10, 0, 0, 9]), now).unwrap();
        assert_eq!((kind(&nak), to), (NAK, Ipv4Addr::BROADCAST));

        let (renew, to) = e.handle(&packet(REQUEST, 1, [192, 168, 100, 100], &[]), now).unwrap();
        assert_eq!((kind(&renew), to), (ACK, Ipv4Addr::new(192, 168, 100, 100)));

        e.handle(&packet(RELEASE, 2, [192, 168, 100, 101], &[]), now);
        assert_eq!(e.active_leases(now).len(), 1);

        assert!(e.handle(&packet(REQUEST, 5, [0; 4], &[50, 4, 10, 1, 1, 9, 54, 4, 10, 1, 1, 1]), now).is_none());
        assert_eq!(e.foreign_server, Some(Ipv4Addr::new(10, 1, 1, 1)));
        assert!(e.handle(&packet(DISCOVER, 6, [0; 4], &[]), now).is_none());
        assert!(e.handle(&[1, 2, 3], now).is_none());
    }
}
