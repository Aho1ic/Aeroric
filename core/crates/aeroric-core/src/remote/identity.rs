//! 主机身份与局域网地址面（P8-03 刀三自宿主 `remote/mod.rs` 上收）。
//!
//! 纯逻辑：主机名读取、本机 IPv4 收集与优先级排序（常规 LAN 段排在
//! 隧道/代理地址之前）、配对端点快照构造。relay 连接 URL 构造与宿主
//! 同源（`remote-protocol` 同一 path 依赖）。

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};

use serde::Serialize;

/// 生成配对码时公布的 relay 连接 URL 与宿主同一构造器。
use remote_protocol::client_connect_url;

pub fn host_name() -> String {
    #[cfg(target_os = "windows")]
    {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` 是合法可写字节数组且长度即容量。`gethostname` 最多写
        // 入这么多字节；结果先按字节处理、找到 NUL 结尾后再按 UTF-8 校验。
        let ok = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if ok == 0 {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            if let Ok(name) = std::str::from_utf8(&buf[..end]) {
                if !name.is_empty() {
                    return name.trim_end_matches(".local").to_string();
                }
            }
        }
        "Mac".to_string()
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteNetworkAddress {
    pub interface_name: String,
    pub ip: String,
}

pub fn address_priority(ip: Ipv4Addr, interface_name: &str) -> (u8, u8, String, u32) {
    let [a, b, _, _] = ip.octets();
    let range_priority = if a == 192 && b == 168 {
        0
    } else if a == 172 && (16..=31).contains(&b) {
        1
    } else if a == 10 {
        2
    } else if a == 100 && (64..=127).contains(&b) {
        3
    } else if a == 198 && (18..=19).contains(&b) {
        // RFC 2544 benchmark ranges are commonly used by transparent proxies.
        9
    } else if ip.is_link_local() {
        10
    } else {
        4
    };
    let interface_priority = if interface_name.starts_with("utun")
        || interface_name.starts_with("tun")
        || interface_name.starts_with("tap")
        || interface_name.starts_with("wg")
    {
        1
    } else {
        0
    };
    (
        range_priority,
        interface_priority,
        interface_name.to_ascii_lowercase(),
        u32::from(ip),
    )
}

/// 1124e：虚拟机和容器的网桥（`bridge100` 等）只在本机内部可达，随虚拟机
/// 起停出现或消失；手机连不到，不能当成局域网地址公布。
fn is_virtual_bridge(interface_name: &str) -> bool {
    const PREFIXES: [&str; 7] = [
        "bridge", "vmnet", "vboxnet", "docker", "veth", "virbr", "br-",
    ];
    let name = interface_name.to_ascii_lowercase();
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// 返回所有可用于手机直连的本机 IPv4，并将常规 LAN 地址排在隧道/代理地址之前。
pub fn lan_addresses() -> Vec<RemoteNetworkAddress> {
    ranked_addresses(
        if_addrs::get_if_addrs()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|interface| match interface.ip() {
                IpAddr::V4(ip) => Some((interface.name, ip)),
                IpAddr::V6(_) => None,
            }),
    )
}

/// `lan_addresses` 的纯逻辑：去掉回环、网桥和重复的地址，再排序。
fn ranked_addresses(
    interfaces: impl IntoIterator<Item = (String, Ipv4Addr)>,
) -> Vec<RemoteNetworkAddress> {
    let mut seen = HashSet::new();
    let mut addresses = interfaces
        .into_iter()
        .filter_map(|(name, ip)| {
            if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
                return None;
            }
            if is_virtual_bridge(&name) || !seen.insert(ip) {
                return None;
            }
            Some((
                address_priority(ip, &name),
                RemoteNetworkAddress {
                    interface_name: name,
                    ip: ip.to_string(),
                },
            ))
        })
        .collect::<Vec<_>>();
    addresses.sort_by(|left, right| left.0.cmp(&right.0));
    addresses.into_iter().map(|(_, address)| address).collect()
}

/// 1124e：配对链接和 `hello` 都公布的局域网地址。选中的（首选或排第一的）
/// 在前，其余手机可能连得到的跟在后面：一个地址失效时手机还有别的可试。
/// 透明代理的 198.18/15 和链路本地地址只在被选中时才公布。
pub fn lan_endpoints(
    port: u16,
    preferred_lan_ip: Option<&str>,
    addresses: &[RemoteNetworkAddress],
) -> Vec<String> {
    let mut ips = Vec::new();
    if let Some(selected) = selected_lan_ip(preferred_lan_ip, addresses) {
        ips.push(selected);
    }
    for address in addresses {
        let reachable = address
            .ip
            .parse::<Ipv4Addr>()
            .is_ok_and(|ip| address_priority(ip, &address.interface_name).0 < 9);
        if reachable && !ips.contains(&address.ip) {
            ips.push(address.ip.clone());
        }
    }
    ips.into_iter()
        .map(|ip| format!("ws://{ip}:{port}"))
        .collect()
}

pub fn selected_lan_ip(
    preferred_lan_ip: Option<&str>,
    addresses: &[RemoteNetworkAddress],
) -> Option<String> {
    preferred_lan_ip
        .and_then(|preferred| {
            addresses
                .iter()
                .find(|address| address.ip == preferred)
                .map(|address| address.ip.clone())
        })
        .or_else(|| addresses.first().map(|address| address.ip.clone()))
}

/// 生成配对码时要公布的端点快照。
///
/// `includes_lan` 不是仅用于展示：它表示 QR 中包含一个依赖本机 TCP
/// listener 的地址，因此在签发邀请码前必须确认 listener 已宽绑定。相反，
/// 只有 relay / 用户配置的公网地址时无需为了配对而扩大本地监听范围。
#[derive(Debug, PartialEq, Eq)]
pub struct PairingEndpoints {
    pub endpoints: Vec<String>,
    pub includes_lan: bool,
}

pub fn pairing_endpoints(
    port: u16,
    addresses: &[RemoteNetworkAddress],
    config: &crate::remote::config::RemoteConfig,
    host_id: &str,
) -> PairingEndpoints {
    let mut endpoints = lan_endpoints(port, config.preferred_lan_ip.as_deref(), addresses);
    let includes_lan = !endpoints.is_empty();
    endpoints.extend(config.public_endpoints.iter().cloned());
    if let Some(relay_url) = config
        .relay_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
    {
        endpoints.push(client_connect_url(relay_url, host_id));
    }
    PairingEndpoints {
        includes_lan,
        endpoints,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::config::RemoteConfig;

    #[test]
    fn preferred_lan_ip_wins_only_while_still_present() {
        let addresses = vec![
            RemoteNetworkAddress {
                interface_name: "en0".to_string(),
                ip: "192.168.1.10".to_string(),
            },
            RemoteNetworkAddress {
                interface_name: "utun4".to_string(),
                ip: "10.8.0.2".to_string(),
            },
        ];
        assert_eq!(
            selected_lan_ip(Some("192.168.1.10"), &addresses).as_deref(),
            Some("192.168.1.10")
        );
        // 首选地址下线后回落到排序第一的地址。
        assert_eq!(
            selected_lan_ip(Some("192.168.1.99"), &addresses).as_deref(),
            Some("192.168.1.10")
        );
    }

    #[test]
    fn pairing_endpoints_cover_lan_public_and_relay() {
        let config = RemoteConfig {
            public_endpoints: vec!["ws://tailscale-host:6790".to_string()],
            relay_url: Some("wss://relay.example.com".to_string()),
            ..Default::default()
        };
        let addresses = vec![RemoteNetworkAddress {
            interface_name: "en0".to_string(),
            ip: "192.168.1.10".to_string(),
        }];
        let snapshot = pairing_endpoints(6790, &addresses, &config, "host-1");
        assert!(snapshot.includes_lan);
        assert_eq!(snapshot.endpoints[0], "ws://192.168.1.10:6790");
        assert!(
            snapshot
                .endpoints
                .iter()
                .any(|e| e.contains("relay.example.com")),
            "relay 连接 URL 应出现在端点里: {:?}",
            snapshot.endpoints
        );
    }

    /// 1124e 目检时这台 Mac 的网卡：虚拟机网桥 `bridge100` 按名字排在
    /// `en0` 前面，配对链接只写了它，虚拟机一停手机就连不上了。
    fn mac_in_review_1124e() -> Vec<RemoteNetworkAddress> {
        ranked_addresses([
            ("lo0".to_string(), Ipv4Addr::new(127, 0, 0, 1)),
            ("bridge100".to_string(), Ipv4Addr::new(192, 168, 64, 1)),
            ("en0".to_string(), Ipv4Addr::new(192, 168, 0, 159)),
            ("utun4".to_string(), Ipv4Addr::new(100, 125, 106, 127)),
            ("utun5".to_string(), Ipv4Addr::new(10, 0, 0, 2)),
            ("utun1024".to_string(), Ipv4Addr::new(198, 18, 0, 1)),
        ])
    }

    #[test]
    fn virtual_machine_bridges_are_not_lan_addresses() {
        let names: Vec<_> = mac_in_review_1124e()
            .into_iter()
            .map(|address| address.interface_name)
            .collect();
        assert_eq!(names, ["en0", "utun5", "utun4", "utun1024"]);
        for name in [
            "bridge100",
            "vmnet8",
            "vboxnet0",
            "docker0",
            "veth1a2b",
            "virbr0",
            "br-5e1f",
        ] {
            assert!(is_virtual_bridge(name), "{name}");
        }
        for name in ["en0", "en7", "utun4", "wg0", "eth0"] {
            assert!(!is_virtual_bridge(name), "{name}");
        }
    }

    #[test]
    fn pairing_lists_every_reachable_lan_address_selected_first() {
        let addresses = mac_in_review_1124e();
        let snapshot = pairing_endpoints(6790, &addresses, &RemoteConfig::default(), "host-1");
        assert!(snapshot.includes_lan);
        assert_eq!(
            snapshot.endpoints,
            [
                "ws://192.168.0.159:6790",
                "ws://10.0.0.2:6790",
                "ws://100.125.106.127:6790",
            ],
            "198.18/15 is a transparent proxy: listed only when selected"
        );
        // A preferred address comes first; the others still follow.
        assert_eq!(
            lan_endpoints(6790, Some("10.0.0.2"), &addresses)[..2],
            ["ws://10.0.0.2:6790", "ws://192.168.0.159:6790"]
        );
        // The only address is listed even if a phone may not reach it.
        let proxy_only = ranked_addresses([("utun1024".to_string(), Ipv4Addr::new(198, 18, 0, 1))]);
        assert_eq!(
            lan_endpoints(6790, None, &proxy_only),
            ["ws://198.18.0.1:6790"]
        );
    }

    #[test]
    fn relay_only_pairing_does_not_claim_lan_exposure() {
        let config = RemoteConfig {
            relay_url: Some("wss://relay.example.com".to_string()),
            ..Default::default()
        };
        let snapshot = pairing_endpoints(6790, &[], &config, "host-1");
        assert!(!snapshot.includes_lan);
    }
}
