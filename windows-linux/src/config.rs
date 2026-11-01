//! Validation of a typed address. Same rules as the macOS app
//! (Sources/RackIPCore/ManualConfig.swift).

use std::net::Ipv4Addr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualConfig {
    pub address: Ipv4Addr,
    pub prefix: u8,
    pub gateway: Option<Ipv4Addr>,
    /// Allowed, but probably a mistake.
    pub warnings: Vec<String>,
}

pub const DEFAULT_PREFIX: u8 = 24;

impl ManualConfig {
    pub fn mask(&self) -> Ipv4Addr {
        mask_from_prefix(self.prefix)
    }

    /// - `address` may carry a prefix ("10.0.0.5/24").
    /// - `mask` takes dotted form or a prefix ("24" or "/24"); empty means /24.
    /// - `gateway` is optional.
    pub fn parse(address: &str, mask: &str, gateway: &str) -> Result<Self, String> {
        let (address, mask, gateway) = (address.trim(), mask.trim(), gateway.trim());
        if address.is_empty() {
            return Err("Enter an IP address.".into());
        }

        let (addr_text, prefix_from_addr) = match address.split_once('/') {
            Some((a, p)) => match p.parse::<u8>().ok().filter(|p| *p <= 32) {
                Some(p) => (a, Some(p)),
                None => return Err(format!("“{p}” is not a valid subnet mask.")),
            },
            None => (address, None),
        };
        let ip = parse_ipv4(addr_text).ok_or_else(|| format!("“{addr_text}” is not a valid IPv4 address."))?;

        let prefix = if mask.is_empty() {
            prefix_from_addr.unwrap_or(DEFAULT_PREFIX)
        } else {
            let p = parse_mask(mask).ok_or_else(|| format!("“{mask}” is not a valid subnet mask."))?;
            if let Some(from_addr) = prefix_from_addr {
                if from_addr != p {
                    return Err(format!(
                        "The /prefix on the address ({}) does not match the subnet mask ({}).",
                        mask_from_prefix(from_addr),
                        mask_from_prefix(p)
                    ));
                }
            }
            p
        };
        if prefix == 0 {
            return Err("0.0.0.0 is not a valid subnet mask.".into());
        }

        let v = u32::from(ip);
        if v == 0 || v >> 24 == 127 || v >> 28 >= 14 {
            return Err(format!("{ip} cannot be assigned to an interface."));
        }
        let m = u32::from(mask_from_prefix(prefix));
        if prefix <= 30 {
            let host = v & !m;
            if host == 0 {
                return Err(format!("{ip} is the network address of its /{prefix} subnet."));
            }
            if host == !m {
                return Err(format!("{ip} is the broadcast address of its /{prefix} subnet."));
            }
        }

        let mut warnings = Vec::new();
        let gateway = if gateway.is_empty() {
            None
        } else {
            let g = parse_ipv4(gateway).ok_or_else(|| format!("“{gateway}” is not a valid gateway address."))?;
            if g == ip {
                return Err("The gateway cannot be the same as the IP address.".into());
            }
            if u32::from(g) & m != v & m {
                warnings.push(format!(
                    "Gateway {g} is outside {}/{prefix}, so it will not be reachable.",
                    Ipv4Addr::from(v & m)
                ));
            }
            Some(g)
        };

        Ok(ManualConfig { address: ip, prefix, gateway, warnings })
    }
}

/// Strict dotted quad: no leading zeros beyond 3 digits, no signs or spaces.
pub fn parse_ipv4(s: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        octets[i] = p.parse::<u16>().ok().filter(|v| *v <= 255)? as u8;
    }
    Some(Ipv4Addr::from(octets))
}

pub fn mask_from_prefix(prefix: u8) -> Ipv4Addr {
    Ipv4Addr::from(if prefix == 0 { 0 } else { u32::MAX << (32 - prefix as u32) })
}

pub fn prefix_from_mask(mask: Ipv4Addr) -> Option<u8> {
    let ones = u32::from(mask).count_ones() as u8;
    (mask_from_prefix(ones) == mask).then_some(ones)
}

fn parse_mask(text: &str) -> Option<u8> {
    let bare = text.strip_prefix('/').unwrap_or(text);
    if let Ok(p) = bare.parse::<u8>() {
        return (p <= 32).then_some(p);
    }
    prefix_from_mask(parse_ipv4(text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(a: &str, m: &str, g: &str) -> ManualConfig {
        ManualConfig::parse(a, m, g).unwrap_or_else(|e| panic!("{a} {m} {g}: {e}"))
    }
    fn err(a: &str, m: &str, g: &str) -> bool {
        ManualConfig::parse(a, m, g).is_err()
    }

    #[test]
    fn ipv4() {
        assert_eq!(parse_ipv4("10.0.0.5"), Some(Ipv4Addr::new(10, 0, 0, 5)));
        for bad in ["", "1.2.3", "1.2.3.4.5", "256.1.1.1", "1..2.3", " 1.2.3.4", "1.2.3.-4", "0001.2.3.4", "+1.2.3.4"] {
            assert_eq!(parse_ipv4(bad), None, "{bad}");
        }
        assert_eq!(prefix_from_mask(Ipv4Addr::new(255, 255, 252, 0)), Some(22));
        assert_eq!(prefix_from_mask(Ipv4Addr::new(255, 255, 0, 255)), None);
    }

    #[test]
    fn configs() {
        assert_eq!(ok("192.168.1.10", "", "").prefix, 24);
        assert_eq!(ok(" 10.1.2.3/16 ", "", "").prefix, 16);
        assert_eq!(ok("10.1.2.3", "/22", "").prefix, 22);
        assert_eq!(ok("10.1.2.3", "255.255.252.0", "").prefix, 22);
        assert_eq!(ok("10.0.0.5", "", "10.0.0.1").gateway, Some(Ipv4Addr::new(10, 0, 0, 1)));
        assert_eq!(ok("10.0.0.5", "", "10.0.1.1").warnings.len(), 1);
        assert!(ok("10.0.0.0", "255.255.255.254", "").warnings.is_empty());
        assert_eq!(ok("10.0.0.5", "", "").mask(), Ipv4Addr::new(255, 255, 255, 0));
    }

    #[test]
    fn rejects() {
        assert!(err("", "", ""));
        assert!(err("10.0.0", "", ""));
        assert!(err("10.0.0.5", "255.0.255.0", ""));
        assert!(err("10.0.0.5/33", "", ""));
        assert!(err("10.0.0.5", "", "x"));
        assert!(err("10.0.0.5", "", "10.0.0.5"));
        assert!(err("10.0.0.0", "", ""));
        assert!(err("10.0.0.255", "", ""));
        assert!(err("127.0.0.1", "255.0.0.0", ""));
        assert!(err("224.0.0.1", "", ""));
        assert!(err("10.0.0.5/16", "255.255.255.0", ""));
    }
}
