//! Linux: NetworkManager via `nmcli`.
//!
//! A static address goes into a dedicated connection profile, "RackIP <device>",
//! so the user's own profiles are never edited. Going back to DHCP deletes it
//! and lets NetworkManager reconnect the device the way it did before.

use super::{run, Adapter, DhcpServer, Mode};
use crate::config::{parse_ipv4, ManualConfig};
use crate::dhcp::{DhcpConfig, Lease};
use std::time::{Duration, Instant};

/// NetworkManager's "shared" mode runs dnsmasq for the port, so the app needs
/// no root rights. It also offers this computer as gateway and DNS.
pub fn start_dhcp(adapter: &Adapter, config: &DhcpConfig) -> Result<Box<dyn DhcpServer>, String> {
    let profile = profile_name(adapter);
    ensure_profile(adapter, &profile)?;
    let addresses = format!("{}/{}", config.server, config.prefix);
    let range = format!("{},{}", config.start, config.end);
    let lease = config.lease_secs.to_string();
    let base = [
        "connection", "modify", profile.as_str(),
        "ipv4.method", "shared",
        "ipv4.addresses", addresses.as_str(),
        "ipv4.gateway", "",
        "connection.autoconnect", "yes",
        "connection.autoconnect-priority", "100",
    ];
    let mut with_range = base.to_vec();
    with_range.extend(["ipv4.shared-dhcp-range", range.as_str(), "ipv4.shared-dhcp-lease-time", lease.as_str()]);
    let mut note = None;
    if run("nmcli", &with_range).is_err() {
        // Range and lease time arrived in NetworkManager 1.42.
        run("nmcli", &base)?;
        note = Some("This NetworkManager is older than 1.42, so it picks the range and lease time itself.".to_string());
    }
    if let Err(e) = run("nmcli", &["connection", "up", &profile, "ifname", &adapter.id]) {
        let _ = run("nmcli", &["connection", "modify", &profile, "ipv4.method", "manual"]);
        return Err(if e.to_lowercase().contains("dnsmasq") {
            "The DHCP server needs dnsmasq. Install it (package dnsmasq-base or dnsmasq) and try again.".into()
        } else {
            e
        });
    }
    Ok(Box::new(SharedServer { device: adapter.id.clone(), profile, note, leases: Vec::new(), read_at: None }))
}

struct SharedServer {
    device: String,
    profile: String,
    note: Option<String>,
    leases: Vec<Lease>,
    read_at: Option<Instant>,
}

impl DhcpServer for SharedServer {
    fn leases(&mut self) -> Vec<Lease> {
        if self.read_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            let path = format!("/var/lib/NetworkManager/dnsmasq-{}.leases", self.device);
            let text = std::fs::read_to_string(path).unwrap_or_default();
            self.leases = parse_dnsmasq_leases(&text, crate::store::now());
            self.read_at = Some(Instant::now());
        }
        self.leases.clone()
    }

    fn problem(&self) -> Option<String> {
        self.note.clone()
    }

    fn stop(self: Box<Self>) -> Result<(), String> {
        run("nmcli", &["connection", "modify", &self.profile, "ipv4.method", "manual"])?;
        run("nmcli", &["connection", "up", &self.profile, "ifname", &self.device]).map(|_| ())
    }
}

/// dnsmasq lease lines: `<expiry> <mac> <ip> <hostname or *> <client id>`.
pub(crate) fn parse_dnsmasq_leases(text: &str, now: u64) -> Vec<Lease> {
    let mut list: Vec<Lease> = text
        .lines()
        .filter_map(|line| {
            let mut f = line.split_whitespace();
            let expires: u64 = f.next()?.parse().ok()?;
            let mac = f.next()?.to_lowercase();
            let ip = parse_ipv4(f.next()?)?;
            let hostname = f.next().filter(|h| *h != "*").unwrap_or("").to_string();
            // 0 means the lease never expires.
            let expires = if expires == 0 { u64::MAX } else { expires };
            (expires > now).then_some(Lease { mac, ip, hostname, expires, bound: true })
        })
        .collect();
    list.sort_by(|a, b| b.expires.cmp(&a.expires));
    list
}

pub fn adapters() -> Result<Vec<Adapter>, String> {
    let status = run("nmcli", &["-t", "-e", "yes", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device", "status"])
        .map_err(|e| format!("NetworkManager (nmcli) is required: {e}"))?;
    let mut list = Vec::new();
    for row in status.lines().map(split_terse) {
        let [device, kind, state, connection] = match <[String; 4]>::try_from(row) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if matches!(kind.as_str(), "loopback" | "bridge" | "tun" | "wifi-p2p" | "dummy") || device.is_empty() {
            continue;
        }
        let details = run(
            "nmcli",
            &["-t", "-f", "WIRED-PROPERTIES.CARRIER,IP4.ADDRESS,IP4.GATEWAY", "device", "show", &device],
        )
        .unwrap_or_default();
        let mode = if connection.is_empty() {
            Mode::Unknown
        } else {
            match run("nmcli", &["-g", "ipv4.method", "connection", "show", &connection]).as_deref().map(str::trim) {
                Ok("auto") => Mode::Dhcp,
                Ok("manual") => Mode::Manual,
                _ => Mode::Unknown,
            }
        };
        list.push(adapter_from(&device, &kind, &state, &connection, &details, mode));
    }
    list.sort_by(|a, b| b.wired.cmp(&a.wired).then_with(|| a.name.cmp(&b.name)));
    Ok(list)
}

pub fn set_static(adapter: &Adapter, c: &ManualConfig) -> Result<(), String> {
    let profile = profile_name(adapter);
    ensure_profile(adapter, &profile)?;
    let addresses = format!("{}/{}", c.address, c.prefix);
    let gateway = c.gateway.map(|g| g.to_string()).unwrap_or_default();
    run(
        "nmcli",
        &[
            "connection", "modify", &profile,
            "ipv4.method", "manual",
            "ipv4.addresses", &addresses,
            "ipv4.gateway", &gateway,
            "connection.autoconnect", "yes",
            "connection.autoconnect-priority", "100",
        ],
    )?;
    run("nmcli", &["connection", "up", &profile, "ifname", &adapter.id]).map(|_| ())
}

pub fn set_dhcp(adapter: &Adapter) -> Result<(), String> {
    let profile = profile_name(adapter);
    if profile_exists(&profile)? {
        run("nmcli", &["connection", "delete", &profile])?;
    }
    run("nmcli", &["device", "connect", &adapter.id]).map(|_| ())
}

fn profile_name(adapter: &Adapter) -> String {
    format!("RackIP {}", adapter.id)
}

fn profile_exists(profile: &str) -> Result<bool, String> {
    let names = run("nmcli", &["-t", "-e", "yes", "-f", "NAME", "connection", "show"])?;
    Ok(names.lines().map(|l| split_terse(l).concat()).any(|n| n == profile))
}

fn ensure_profile(adapter: &Adapter, profile: &str) -> Result<(), String> {
    if profile_exists(profile)? {
        return Ok(());
    }
    let kind = if adapter.wired { "ethernet" } else { "wifi" };
    if kind == "wifi" {
        return Err("Static addresses on Wi-Fi are not supported here; use your system network settings.".into());
    }
    run("nmcli", &["connection", "add", "type", kind, "ifname", &adapter.id, "con-name", profile]).map(|_| ())
}

/// Splits one line of `nmcli -t -e yes` output on unescaped colons.
pub(crate) fn split_terse(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    fields.last_mut().unwrap().push(next);
                }
            }
            ':' => fields.push(String::new()),
            _ => fields.last_mut().unwrap().push(c),
        }
    }
    fields
}

pub(crate) fn adapter_from(device: &str, kind: &str, state: &str, connection: &str, details: &str, mode: Mode) -> Adapter {
    let mut carrier = None;
    let mut address = None;
    let mut prefix = None;
    let mut gateway = None;
    for line in details.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        if key == "WIRED-PROPERTIES.CARRIER" {
            carrier = Some(value == "on");
        } else if key.starts_with("IP4.ADDRESS") && address.is_none() {
            if let Some((ip, p)) = value.split_once('/') {
                address = parse_ipv4(ip);
                prefix = p.parse().ok();
            }
        } else if key == "IP4.GATEWAY" {
            gateway = parse_ipv4(value);
        }
    }
    let wired = kind == "ethernet";
    let connected = carrier.unwrap_or(state.starts_with("connected"));
    let name = if connection.is_empty() || connection.starts_with("RackIP ") || !wired {
        device.to_string()
    } else {
        format!("{device} — {connection}")
    };
    Adapter {
        id: device.to_string(),
        name,
        detail: device.to_string(),
        wired,
        connected,
        mode,
        address,
        prefix,
        gateway,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn dnsmasq_leases() {
        let text = "1700000600 AA:BB:CC:DD:EE:01 10.42.0.23 idrac 01:aa:bb:cc:dd:ee:01\n\
                    1700000100 aa:bb:cc:dd:ee:02 10.42.0.24 * *\n\
                    1699999000 aa:bb:cc:dd:ee:03 10.42.0.25 old *\n\
                    garbage\n";
        let l = parse_dnsmasq_leases(text, 1_700_000_000);
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].mac.as_str(), l[0].ip, l[0].hostname.as_str()), ("aa:bb:cc:dd:ee:01", Ipv4Addr::new(10, 42, 0, 23), "idrac"));
        assert_eq!(l[1].hostname, "");
    }

    #[test]
    fn terse_split() {
        assert_eq!(split_terse("enp3s0:ethernet:connected:Wired connection 1"), ["enp3s0", "ethernet", "connected", "Wired connection 1"]);
        assert_eq!(split_terse(r"wlan0:wifi:connected:Cafe\:Guest"), ["wlan0", "wifi", "connected", "Cafe:Guest"]);
        assert_eq!(split_terse("eth1:ethernet:unavailable:"), ["eth1", "ethernet", "unavailable", ""]);
    }

    #[test]
    fn details() {
        let a = adapter_from(
            "enx00e04c680001",
            "ethernet",
            "connected",
            "RackIP enx00e04c680001",
            "WIRED-PROPERTIES.CARRIER:on\nIP4.ADDRESS[1]:10.0.0.5/24\nIP4.GATEWAY:\n",
            Mode::Manual,
        );
        assert!(a.wired && a.connected);
        assert_eq!(a.name, "enx00e04c680001");
        assert_eq!(a.address, Some(Ipv4Addr::new(10, 0, 0, 5)));
        assert_eq!(a.prefix, Some(24));
        assert_eq!(a.gateway, None);

        let b = adapter_from("eth0", "ethernet", "unavailable", "", "WIRED-PROPERTIES.CARRIER:off\n", Mode::Unknown);
        assert!(!b.connected);
        assert_eq!(b.address, None);
    }
}
