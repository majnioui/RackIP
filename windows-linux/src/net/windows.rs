//! Windows: read with PowerShell's NetTCPIP cmdlets (JSON, so not affected by
//! the system language), change with `netsh` (needs administrator rights,
//! which the app's manifest requests at launch).

use super::{run, Adapter, Mode};
use crate::config::{parse_ipv4, ManualConfig};
use serde::{Deserialize, Deserializer};

const SCRIPT: &str = r#"
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$ErrorActionPreference = 'SilentlyContinue'
$a = @(Get-NetAdapter | ForEach-Object { [pscustomobject]@{
    Index = [int]$_.ifIndex; Name = [string]$_.Name; Desc = [string]$_.InterfaceDescription
    Status = [string]$_.Status; Media = [string]$_.PhysicalMediaType; Virtual = [bool]$_.Virtual } })
$ip = @(Get-NetIPAddress -AddressFamily IPv4 | ForEach-Object { [pscustomobject]@{
    Index = [int]$_.InterfaceIndex; Address = [string]$_.IPAddress; Prefix = [int]$_.PrefixLength } })
$gw = @(Get-NetRoute -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' | ForEach-Object { [pscustomobject]@{
    Index = [int]$_.InterfaceIndex; NextHop = [string]$_.NextHop } })
$dh = @(Get-NetIPInterface -AddressFamily IPv4 | ForEach-Object { [pscustomobject]@{
    Index = [int]$_.InterfaceIndex; Dhcp = [string]$_.Dhcp } })
ConvertTo-Json -Compress -Depth 4 -InputObject @{ adapters = $a; ips = $ip; gateways = $gw; dhcp = $dh }
"#;

pub fn adapters() -> Result<Vec<Adapter>, String> {
    let out = run(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT],
    )?;
    parse(&out)
}

pub fn set_static(adapter: &Adapter, c: &ManualConfig) -> Result<(), String> {
    let name = format!("name={}", adapter.id);
    let address = format!("address={}", c.address);
    let mask = format!("mask={}", c.mask());
    let gateway = format!("gateway={}", c.gateway.map(|g| g.to_string()).unwrap_or_else(|| "none".into()));
    netsh(&["interface", "ipv4", "set", "address", &name, "source=static", &address, &mask, &gateway])
}

pub fn set_dhcp(adapter: &Adapter) -> Result<(), String> {
    let name = format!("name={}", adapter.id);
    netsh(&["interface", "ipv4", "set", "address", &name, "source=dhcp"])
}

fn netsh(args: &[&str]) -> Result<(), String> {
    // netsh reports some failures on stdout with exit code 0.
    let out = run("netsh.exe", args)?;
    let text = out.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("ok.") {
        Ok(())
    } else if text.to_lowercase().contains("elevation") || text.to_lowercase().contains("administrator") {
        Err("Run RackIP as administrator to change network settings.".into())
    } else {
        Err(text.to_string())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    #[serde(default, deserialize_with = "one_or_many")]
    adapters: Vec<RawAdapter>,
    #[serde(default, deserialize_with = "one_or_many")]
    ips: Vec<RawIp>,
    #[serde(default, deserialize_with = "one_or_many")]
    gateways: Vec<RawGateway>,
    #[serde(default, deserialize_with = "one_or_many")]
    dhcp: Vec<RawDhcp>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawAdapter {
    index: i64,
    name: String,
    #[serde(default)]
    desc: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    media: String,
    #[serde(default)]
    r#virtual: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawIp {
    index: i64,
    address: String,
    prefix: u8,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawGateway {
    index: i64,
    next_hop: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawDhcp {
    index: i64,
    dhcp: String,
}

/// PowerShell collapses one-element arrays to a bare object and empty ones to null.
fn one_or_many<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        Many(Vec<T>),
        One(T),
    }
    Ok(match Option::<OneOrMany<T>>::deserialize(d)? {
        None => Vec::new(),
        Some(OneOrMany::Many(v)) => v,
        Some(OneOrMany::One(t)) => vec![t],
    })
}

pub(crate) fn parse(json: &str) -> Result<Vec<Adapter>, String> {
    let json = json.trim_start_matches('\u{feff}').trim();
    let snap: Snapshot = serde_json::from_str(json).map_err(|e| format!("Could not read adapter list: {e}"))?;
    let mut list: Vec<Adapter> = snap
        .adapters
        .iter()
        .map(|a| {
            // Prefer a real address over the 169.254 one Windows assigns while waiting for DHCP.
            let ip = snap
                .ips
                .iter()
                .filter(|i| i.index == a.index)
                .filter_map(|i| parse_ipv4(&i.address).map(|ip| (ip, i.prefix)))
                .min_by_key(|(ip, _)| ip.is_link_local());
            let gateway = snap
                .gateways
                .iter()
                .filter(|g| g.index == a.index)
                .filter_map(|g| parse_ipv4(&g.next_hop))
                .find(|g| !g.is_unspecified());
            let mode = match snap.dhcp.iter().find(|d| d.index == a.index).map(|d| d.dhcp.as_str()) {
                Some("Enabled") | Some("1") => Mode::Dhcp,
                Some("Disabled") | Some("0") => Mode::Manual,
                _ => Mode::Unknown,
            };
            let wired = a.media == "802.3" && !a.r#virtual && !a.desc.to_lowercase().contains("wi-fi");
            Adapter {
                id: a.index.to_string(),
                name: a.name.clone(),
                detail: a.desc.clone(),
                wired,
                connected: a.status.eq_ignore_ascii_case("up"),
                mode,
                address: ip.map(|(ip, _)| ip),
                prefix: ip.map(|(_, p)| p),
                gateway,
            }
        })
        .collect();
    list.sort_by(|a, b| b.wired.cmp(&a.wired).then_with(|| a.name.cmp(&b.name)));
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn parses_snapshot() {
        let json = r#"{"adapters":[{"Index":12,"Name":"Ethernet 2","Desc":"Realtek USB GbE Family Controller","Status":"Up","Media":"802.3","Virtual":false},
            {"Index":7,"Name":"Wi-Fi","Desc":"Intel(R) Wi-Fi 6 AX201","Status":"Up","Media":"Native 802.11","Virtual":false},
            {"Index":20,"Name":"vEthernet (WSL)","Desc":"Hyper-V Virtual Ethernet Adapter","Status":"Up","Media":"802.3","Virtual":true}],
          "ips":[{"Index":12,"Address":"169.254.3.4","Prefix":16},{"Index":12,"Address":"10.0.0.5","Prefix":24},{"Index":7,"Address":"192.168.1.20","Prefix":24}],
          "gateways":{"Index":7,"NextHop":"192.168.1.1"},
          "dhcp":[{"Index":12,"Dhcp":"Disabled"},{"Index":7,"Dhcp":"Enabled"}]}"#;
        let list = parse(json).unwrap();
        assert_eq!(list.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Ethernet 2", "Wi-Fi", "vEthernet (WSL)"]);
        let eth = &list[0];
        assert!(eth.wired && eth.connected);
        assert_eq!(eth.id, "12");
        assert_eq!(eth.mode, Mode::Manual);
        assert_eq!(eth.address, Some(Ipv4Addr::new(10, 0, 0, 5)));
        assert_eq!(eth.prefix, Some(24));
        assert_eq!(eth.gateway, None);
        assert_eq!(list[1].gateway, Some(Ipv4Addr::new(192, 168, 1, 1)));
        assert_eq!(list[1].mode, Mode::Dhcp);
        assert!(!list[1].wired && !list[2].wired);
    }

    #[test]
    fn empty_and_bom() {
        assert!(parse("\u{feff}{\"adapters\":null,\"ips\":null,\"gateways\":null,\"dhcp\":null}").unwrap().is_empty());
    }
}
