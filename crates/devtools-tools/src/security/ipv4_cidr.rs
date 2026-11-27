//! #28 IPv4 / CIDR 子网计算（Ipv4CidrCalculator）。
//!
//! 三格式自动解析：CIDR（`192.168.1.0/24`）、IP + 掩码
//! （`192.168.1.10 255.255.255.0`）、起止范围（`192.168.1.0-192.168.1.255`
//! 聚合为 CIDR）；`/31`（RFC 3021）与 `/32` 特例；网络 / 广播 / 可用主机
//! 范围与数量、地址属性、二/十/十六进制；只读 IPv4-mapped IPv6。

use std::net::{Ipv4Addr, Ipv6Addr};

use ipnet::Ipv4Net;

use crate::{ToolError, ToolErrorKind};

/// 网段分析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CidrResult {
    /// 聚合后的 CIDR 列表（范围输入可能产生多个）。
    pub networks: Vec<String>,
    pub network: Ipv4Addr,
    pub broadcast: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub prefix_len: u8,
    /// 可用首个主机；`/32` 与 `/31` 按下述规则。
    pub first_host: Ipv4Addr,
    pub last_host: Ipv4Addr,
    pub usable_hosts: u64,
    pub total_addresses: u64,
    pub is_private: bool,
    pub is_loopback: bool,
    pub is_link_local: bool,
    pub is_multicast: bool,
    pub is_reserved: bool,
    /// IPv4-mapped IPv6（只读展示）。
    pub ipv4_mapped_ipv6: String,
}

fn u32_to_ip(v: u32) -> Ipv4Addr {
    Ipv4Addr::from(v)
}

fn address_attributes(ip: Ipv4Addr) -> (bool, bool, bool, bool, bool) {
    (
        ip.is_private(),
        ip.is_loopback(),
        ip.is_link_local(),
        ip.is_multicast(),
        ip.is_broadcast() || ip.is_unspecified() || ip.is_documentation(),
    )
}

/// 分析单个 IPv4 网段。
pub fn analyze_net(net: Ipv4Net) -> CidrResult {
    let prefix = net.prefix_len();
    let network = net.network();
    let broadcast = net.broadcast();
    let mask = net.netmask();
    let total = if prefix == 0 {
        u64::from(u32::MAX) + 1
    } else {
        1u64 << (32 - prefix)
    };
    let net_num = u32::from(network);
    let broadcast_num = u32::from(broadcast);

    let (first_host, last_host, usable) = match prefix {
        32 => (network, network, 1),   // /32 单点，视为一台主机
        31 => (network, broadcast, 2), // RFC 3021：两台主机，无网络/广播语义
        _ => (
            u32_to_ip(net_num.saturating_add(1)),
            u32_to_ip(broadcast_num.saturating_sub(1)),
            total.saturating_sub(2),
        ),
    };
    let (private, loopback, link_local, multicast, reserved) = address_attributes(network);
    let mapped = Ipv6Addr::from([
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0xff,
        0xff,
        octet(net_num, 1),
        octet(net_num, 0),
    ]);

    CidrResult {
        networks: vec![format!("{}/{}", net.network(), prefix)],
        network,
        broadcast,
        netmask: mask,
        prefix_len: prefix,
        first_host,
        last_host,
        usable_hosts: usable,
        total_addresses: total,
        is_private: private,
        is_loopback: loopback,
        is_link_local: link_local,
        is_multicast: multicast,
        is_reserved: reserved,
        ipv4_mapped_ipv6: mapped.to_string(),
    }
}

fn octet(v: u32, index: u8) -> u8 {
    ((v >> (8 * index)) & 0xff) as u8
}

/// 将连续 IP 范围聚合为最小 CIDR 列表（标准 range-to-CIDR 算法）。
pub fn aggregate_range(start: Ipv4Addr, end: Ipv4Addr) -> Result<Vec<Ipv4Net>, ToolError> {
    let mut current = u32::from(start);
    let last = u32::from(end);
    if current > last {
        return Err(ToolError::new(
            ToolErrorKind::InvalidSyntax,
            "起始 IP 不能大于结束 IP",
        ));
    }
    let mut result = Vec::new();
    while current <= last {
        // 当前地址中最低置位决定最大可用块大小。
        let low_bit = if current == 0 {
            32
        } else {
            current.trailing_zeros() as u8
        };
        // 剩余地址数能容纳的最大块（2 的幂）。
        let remaining = (last - current) + 1;
        let max_size = 32 - remaining.leading_zeros() as u8 - 1;
        let size_bits = low_bit.min(max_size).min(32);
        let prefix = 32 - size_bits;
        let net = Ipv4Net::new(u32_to_ip(current), prefix)
            .map_err(|e| ToolError::new(ToolErrorKind::Internal, format!("CIDR 构造失败：{e}")))?;
        result.push(net);
        let block = 1u64 << size_bits;
        current = current.saturating_add(block as u32);
        if size_bits == 32 {
            break;
        }
    }
    Ok(result)
}

/// 自动解析三种输入格式并分析。
pub fn parse_and_analyze(input: &str) -> Result<CidrResult, ToolError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ToolError::new(ToolErrorKind::InvalidSyntax, "输入为空"));
    }

    // 格式 3：起止范围 `a.b.c.d - e.f.g.h`
    if let Some((left, right)) = input.split_once('-') {
        let start: Ipv4Addr = left.trim().parse().map_err(|_| {
            ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("无效起始 IP：`{}`", left.trim()),
            )
        })?;
        let end: Ipv4Addr = right.trim().parse().map_err(|_| {
            ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("无效结束 IP：`{}`", right.trim()),
            )
        })?;
        let nets = aggregate_range(start, end)?;
        let first = nets.first().expect("至少一个网段");
        let mut result = analyze_net(*first);
        result.networks = nets.iter().map(ToString::to_string).collect();
        result.first_host = start;
        result.last_host = end;
        result.total_addresses = (u32::from(end) as u64) - (u32::from(start) as u64) + 1;
        result.usable_hosts = result.total_addresses;
        return Ok(result);
    }

    // 格式 1：CIDR `a.b.c.d/prefix`
    if let Some((ip, prefix)) = input.split_once('/') {
        let addr: Ipv4Addr = ip.trim().parse().map_err(|_| {
            ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("无效 IP：`{}`", ip.trim()),
            )
        })?;
        let prefix: u8 = prefix.trim().parse().map_err(|_| {
            ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("无效前缀长度：`{}`", prefix.trim()),
            )
        })?;
        let net = Ipv4Net::new(addr, prefix)
            .map_err(|e| ToolError::new(ToolErrorKind::InvalidSyntax, format!("无效 CIDR：{e}")))?;
        return Ok(analyze_net(net));
    }

    // 格式 2：IP + 掩码（空格 / 多空格分隔）
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.len() == 2 {
        let ip: Ipv4Addr = parts[0].parse().map_err(|_| {
            ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("无效 IP：`{}`", parts[0]),
            )
        })?;
        let mask: Ipv4Addr = parts[1].parse().map_err(|_| {
            ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("无效子网掩码：`{}`", parts[1]),
            )
        })?;
        let mask_num = u32::from(mask);
        if !is_contiguous_mask(mask_num) {
            return Err(ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("非连续子网掩码：`{}`", parts[1]),
            ));
        }
        let prefix = mask_num.count_ones() as u8;
        let net = Ipv4Net::new(ip, prefix)
            .map_err(|e| ToolError::new(ToolErrorKind::InvalidSyntax, format!("无效 CIDR：{e}")))?;
        return Ok(analyze_net(net));
    }

    // 仅 IP：视为 /32。
    if let Ok(addr) = input.parse::<Ipv4Addr>() {
        return Ok(analyze_net(Ipv4Net::new(addr, 32).expect("/32 永远有效")));
    }

    Err(ToolError::new(
        ToolErrorKind::InvalidSyntax,
        "无法识别输入：需 CIDR、IP+掩码 或 起止范围",
    ))
}

fn is_contiguous_mask(mask: u32) -> bool {
    if mask == 0 {
        return true;
    }
    let inverted = !mask;
    (inverted & (inverted.wrapping_add(1))) == 0
}

/// 将 IP 表示为二进制点分字符串。
pub fn to_binary(ip: Ipv4Addr) -> String {
    let v = u32::from(ip);
    (0..4)
        .map(|i| format!("{:08b}", octet(v, 3 - i)))
        .collect::<Vec<_>>()
        .join(".")
}

/// 将 IP 表示为十六进制点分字符串。
pub fn to_hex(ip: Ipv4Addr) -> String {
    let v = u32::from(ip);
    (0..4)
        .map(|i| format!("0x{:02X}", octet(v, 3 - i)))
        .collect::<Vec<_>>()
        .join(".")
}

/// 解析只读 IPv4-mapped IPv6（`::ffff:a.b.c.d`）。
pub fn parse_ipv4_mapped(input: &str) -> Result<Ipv4Addr, ToolError> {
    let addr: Ipv6Addr = input.parse().map_err(|_| {
        ToolError::new(
            ToolErrorKind::InvalidSyntax,
            format!("无效 IPv6：`{input}`"),
        )
    })?;
    addr.to_ipv4_mapped().ok_or_else(|| {
        ToolError::new(
            ToolErrorKind::UnsupportedInput,
            "该 IPv6 地址不是 IPv4-mapped 形式（::ffff:a.b.c.d）",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_input() {
        let r = parse_and_analyze("192.168.1.10/24").unwrap();
        assert_eq!(r.network, Ipv4Addr::new(192, 168, 1, 0));
        assert_eq!(r.broadcast, Ipv4Addr::new(192, 168, 1, 255));
        assert_eq!(r.first_host, Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(r.last_host, Ipv4Addr::new(192, 168, 1, 254));
        assert_eq!(r.usable_hosts, 254);
        assert!(r.is_private);
        assert_eq!(r.networks, vec!["192.168.1.0/24"]);
    }

    #[test]
    fn ip_plus_netmask() {
        let r = parse_and_analyze("10.0.0.5 255.255.0.0").unwrap();
        assert_eq!(r.prefix_len, 16);
        assert_eq!(r.network, Ipv4Addr::new(10, 0, 0, 0));
        assert_eq!(r.usable_hosts, 65534);
        assert!(parse_and_analyze("10.0.0.1 255.0.255.0").is_err());
    }

    #[test]
    fn slash_31_and_32_special_cases() {
        let r31 = parse_and_analyze("10.0.0.0/31").unwrap();
        assert_eq!(r31.first_host, Ipv4Addr::new(10, 0, 0, 0));
        assert_eq!(r31.last_host, Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(r31.usable_hosts, 2);

        let r32 = parse_and_analyze("10.0.0.7/32").unwrap();
        assert_eq!(r32.first_host, Ipv4Addr::new(10, 0, 0, 7));
        assert_eq!(r32.last_host, Ipv4Addr::new(10, 0, 0, 7));
        assert_eq!(r32.usable_hosts, 1);
        assert_eq!(r32.total_addresses, 1);
    }

    #[test]
    fn range_aggregates_to_cidrs() {
        let r = parse_and_analyze("192.168.1.0-192.168.1.255").unwrap();
        assert_eq!(r.networks, vec!["192.168.1.0/24"]);
        assert_eq!(r.total_addresses, 256);

        let r2 = parse_and_analyze("192.168.0.0-192.168.1.255").unwrap();
        assert_eq!(r2.networks, vec!["192.168.0.0/23"]);

        let r3 = parse_and_analyze("10.0.0.0-10.0.0.10").unwrap();
        assert!(r3.networks.len() > 1, "非对齐范围应聚合为多个 CIDR");
        assert!(parse_and_analyze("10.0.0.10-10.0.0.1").is_err());
    }

    #[test]
    fn representations_and_ipv4_mapped() {
        let ip = Ipv4Addr::new(192, 168, 1, 1);
        assert_eq!(to_binary(ip), "11000000.10101000.00000001.00000001");
        assert_eq!(to_hex(ip), "0xC0.0xA8.0x01.0x01");
        assert_eq!(parse_ipv4_mapped("::ffff:192.168.1.1").unwrap(), ip);
        assert!(parse_ipv4_mapped("::1").is_err());
    }
}
