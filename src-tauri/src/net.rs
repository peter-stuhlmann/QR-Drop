//! Picking the LAN address the phone can reach (CLAUDE.md §5.3).

use std::net::{IpAddr, Ipv4Addr};

use serde::Serialize;

/// Adapter names that are almost never the user's WLAN/LAN.
const VIRTUAL_MARKERS: &[&str] = &[
    "vethernet",
    "virtualbox",
    "vbox",
    "vmware",
    "vmnet",
    "docker",
    "br-",
    "veth",
    "utun",
    "tun",
    "tap",
    "wsl",
    "hyper-v",
    "tailscale",
    "zerotier",
    "wireguard",
    "wg",
    "nordlynx",
    "openvpn",
    "vpn",
    "loopback",
    "bluetooth",
    "ppp",
    "awdl",
    "llw",
    "anpi",
    "bridge",
    "ham",
    "npcap",
    "pseudo",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IfaceKind {
    Wifi,
    Ethernet,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub ip: Ipv4Addr,
    pub name: String,
    pub kind: IfaceKind,
}

fn is_virtual(name: &str) -> bool {
    let n = name.to_lowercase();
    VIRTUAL_MARKERS.iter().any(|m| {
        // Short markers must match a word start ("tun0", "tap-windows"), not
        // any substring ("Ethernet" contains no marker, but e.g. "wg" could
        // appear inside a vendor name).
        if m.len() <= 3 {
            n.starts_with(m)
                || n.split(|c: char| !c.is_ascii_alphanumeric())
                    .any(|w| w.starts_with(m))
        } else {
            n.contains(m)
        }
    })
}

/// Wi-Fi or cable, judged by the adapter name. macOS names both `en0`,
/// `en1` … (on a MacBook `en0` is usually Wi-Fi, on an iMac cable), so those
/// stay `Other` instead of being labelled wrongly.
fn kind_of(name: &str) -> IfaceKind {
    let n = name.to_lowercase();
    if n.contains("wi-fi")
        || n.contains("wifi")
        || n.contains("wlan")
        || n.contains("wireless")
        || n.contains("airport")
        || n.starts_with("wl")
    {
        IfaceKind::Wifi
    } else if n.contains("ethernet") || n.starts_with("eth") || n.contains("lan") {
        IfaceKind::Ethernet
    } else {
        IfaceKind::Other
    }
}

/// 0 = 192.168.x.x, 1 = 10.x.x.x, 2 = 172.16–31.x.x, None = not a LAN address.
fn private_rank(ip: Ipv4Addr) -> Option<u8> {
    let o = ip.octets();
    match o {
        [192, 168, ..] => Some(0),
        [10, ..] => Some(1),
        [172, b, ..] if (16..=31).contains(&b) => Some(2),
        _ => None,
    }
}

/// Filters and orders interfaces, best candidate first.
pub fn rank_candidates(ifaces: Vec<(String, IpAddr)>) -> Vec<Candidate> {
    let mut out: Vec<(Candidate, u8)> = Vec::new();
    for (name, ip) in ifaces {
        let IpAddr::V4(ip) = ip else { continue };
        let Some(rank) = private_rank(ip) else {
            continue;
        };
        if is_virtual(&name) || out.iter().any(|(c, _)| c.ip == ip) {
            continue;
        }
        let kind = kind_of(&name);
        out.push((Candidate { ip, name, kind }, rank));
    }
    out.sort_by(|(a, ra), (b, rb)| {
        a.kind
            .cmp(&b.kind)
            .then(ra.cmp(rb))
            .then(a.name.cmp(&b.name))
    });
    out.into_iter().map(|(c, _)| c).collect()
}

/// All usable LAN addresses of this computer, best first.
pub fn lan_candidates() -> Vec<Candidate> {
    match local_ip_address::list_afinet_netifas() {
        Ok(list) => rank_candidates(list),
        Err(err) => {
            log::warn!("Netzwerkschnittstellen nicht lesbar: {err}");
            Vec::new()
        }
    }
}

/// True for addresses we accept requests from (RFC 1918, link-local, ULA, loopback).
pub fn is_private_client(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_client(IpAddr::V4(v4));
            }
            let seg = v6.segments()[0];
            v6.is_loopback() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn prefers_wifi_and_drops_virtual_adapters() {
        let ifaces = vec![
            ("vEthernet (WSL)".to_string(), v4(172, 28, 16, 1)),
            (
                "VirtualBox Host-Only Network".to_string(),
                v4(192, 168, 56, 1),
            ),
            ("Ethernet".to_string(), v4(10, 0, 0, 12)),
            ("WLAN".to_string(), v4(192, 168, 178, 24)),
            ("Tailscale".to_string(), v4(100, 64, 1, 2)),
            ("Loopback Pseudo-Interface 1".to_string(), v4(127, 0, 0, 1)),
            ("docker0".to_string(), v4(172, 17, 0, 1)),
        ];
        let c = rank_candidates(ifaces);
        let ips: Vec<_> = c.iter().map(|c| c.ip.to_string()).collect();
        assert_eq!(ips, ["192.168.178.24", "10.0.0.12"]);
        assert_eq!(c[0].kind, IfaceKind::Wifi);
    }

    #[test]
    fn range_priority_within_same_kind() {
        let ifaces = vec![
            ("Ethernet 2".to_string(), v4(172, 20, 1, 5)),
            ("Ethernet".to_string(), v4(10, 1, 1, 5)),
            ("Ethernet 3".to_string(), v4(192, 168, 1, 5)),
        ];
        let ips: Vec<_> = rank_candidates(ifaces)
            .iter()
            .map(|c| c.ip.to_string())
            .collect();
        assert_eq!(ips, ["192.168.1.5", "10.1.1.5", "172.20.1.5"]);
    }

    #[test]
    fn macos_names() {
        let ifaces = vec![
            ("utun3".to_string(), v4(10, 8, 0, 2)),
            ("awdl0".to_string(), v4(192, 168, 9, 9)),
            ("bridge100".to_string(), v4(192, 168, 64, 1)),
            ("en0".to_string(), v4(192, 168, 0, 42)),
        ];
        let c = rank_candidates(ifaces);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "en0");
        // Could be Wi-Fi or cable.
        assert_eq!(c[0].kind, IfaceKind::Other);
    }

    #[test]
    fn ignores_public_and_ipv6_and_duplicates() {
        let ifaces = vec![
            ("WLAN".to_string(), v4(8, 8, 8, 8)),
            ("WLAN".to_string(), IpAddr::V6(Ipv6Addr::LOCALHOST)),
            ("WLAN".to_string(), v4(192, 168, 1, 2)),
            ("Wi-Fi".to_string(), v4(192, 168, 1, 2)),
            ("Ethernet".to_string(), v4(169, 254, 3, 3)),
        ];
        let c = rank_candidates(ifaces);
        assert_eq!(c.len(), 1);
        assert!(rank_candidates(vec![]).is_empty());
    }

    #[test]
    fn private_clients_only() {
        assert!(is_private_client(v4(192, 168, 1, 20)));
        assert!(is_private_client(v4(10, 2, 3, 4)));
        assert!(is_private_client(v4(172, 31, 255, 1)));
        assert!(is_private_client(v4(169, 254, 1, 1)));
        assert!(is_private_client(v4(127, 0, 0, 1)));
        assert!(!is_private_client(v4(172, 32, 0, 1)));
        assert!(!is_private_client(v4(8, 8, 8, 8)));
        assert!(!is_private_client(v4(100, 64, 0, 1)));
        assert!(is_private_client("fd12::1".parse().unwrap()));
        assert!(is_private_client("fe80::1".parse().unwrap()));
        assert!(is_private_client("::ffff:192.168.0.3".parse().unwrap()));
        assert!(!is_private_client("2001:db8::1".parse().unwrap()));
        assert!(!is_private_client("::ffff:1.1.1.1".parse().unwrap()));
    }
}
