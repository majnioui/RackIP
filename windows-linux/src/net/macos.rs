//! Development only: lets the cross-platform UI run on a Mac. The macOS
//! release is the Swift app at the repository root.

use super::{run, Adapter, Mode};
use crate::config::{parse_ipv4, prefix_from_mask, ManualConfig};

const TOOL: &str = "/usr/sbin/networksetup";

pub fn adapters() -> Result<Vec<Adapter>, String> {
    let order = run(TOOL, &["-listnetworkserviceorder"])?;
    let mut list = Vec::new();
    let mut name: Option<String> = None;
    for line in order.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("(Hardware Port: ") {
            let Some(service) = name.take() else { continue };
            let rest = rest.trim_end_matches(')');
            let (port, device) = rest.rsplit_once(", Device: ").unwrap_or((rest, ""));
            if device.is_empty() {
                continue;
            }
            let lower = port.to_lowercase();
            let wired = device.starts_with("en")
                && !["wi-fi", "bridge", "iphone", "bluetooth"].iter().any(|x| lower.contains(x));
            let connected = run("/sbin/ifconfig", &[device]).map(|o| o.contains("status: active")).unwrap_or(false);
            let info = run(TOOL, &["-getinfo", &service]).unwrap_or_default();
            let field = |k: &str| info.lines().find_map(|l| l.strip_prefix(k)).and_then(|v| parse_ipv4(v.trim()));
            list.push(Adapter {
                id: service.clone(),
                name: service,
                detail: format!("{port} ({device})"),
                wired,
                connected,
                mode: if info.starts_with("DHCP") { Mode::Dhcp } else if info.starts_with("Manual") { Mode::Manual } else { Mode::Unknown },
                address: field("IP address:"),
                prefix: field("Subnet mask:").and_then(prefix_from_mask),
                gateway: field("Router:"),
            });
        } else if let Some(close) = line.find(')').filter(|_| line.starts_with('(')) {
            name = Some(line[close + 1..].trim().to_string());
        }
    }
    Ok(list)
}

pub fn set_static(adapter: &Adapter, c: &ManualConfig) -> Result<(), String> {
    let (ip, mask) = (c.address.to_string(), c.mask().to_string());
    let gw = c.gateway.map(|g| g.to_string());
    let mut args = vec!["-setmanual", adapter.id.as_str(), ip.as_str(), mask.as_str()];
    if let Some(g) = gw.as_deref() {
        args.push(g);
    }
    check(run(TOOL, &args)?)
}

pub fn set_dhcp(adapter: &Adapter) -> Result<(), String> {
    check(run(TOOL, &["-setdhcp", &adapter.id])?)
}

fn check(out: String) -> Result<(), String> {
    if out.to_lowercase().contains("error") { Err(out.trim().to_string()) } else { Ok(()) }
}
