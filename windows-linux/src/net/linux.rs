//! Linux: NetworkManager via `nmcli`.
//!
//! A static address goes into a dedicated connection profile, "RackIP <device>",
//! so the user's own profiles are never edited. Going back to DHCP deletes it
//! and lets NetworkManager reconnect the device the way it did before.

use super::{run, Adapter, Mode};
use crate::config::{parse_ipv4, ManualConfig};

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
