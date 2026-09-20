use core::sync::atomic::{AtomicU32, Ordering};

use crate::{sys_getpid, sys_net_mac, sys_net_recv, sys_net_send, sys_time_seconds, sys_yield};

pub const ETH_TYPE_IP: u16 = 0x0800;
pub const ETH_TYPE_ARP: u16 = 0x0806;
pub const IP_PROTO_ICMP: u8 = 1;
pub const IP_PROTO_TCP: u8 = 6;
pub const IP_PROTO_UDP: u8 = 17;

#[derive(Clone, Copy)]
pub struct NetConfig {
    pub ip: [u8; 4],
    pub mask: [u8; 4],
    pub gw: [u8; 4],
    pub dns: [u8; 4],
    pub mac: [u8; 6],
}

impl NetConfig {
    pub fn default() -> Self {
        let mut mac = [0u8; 6];
        let n = sys_net_mac(mac.as_mut_ptr(), mac.len());
        if n != 6 {
            mac = [0u8; 6];
        }
        let ip = unpack_ipv4(NET_IP.load(Ordering::Relaxed));
        let mask = unpack_ipv4(NET_MASK.load(Ordering::Relaxed));
        let gw = unpack_ipv4(NET_GW.load(Ordering::Relaxed));
        let dns = unpack_ipv4(NET_DNS.load(Ordering::Relaxed));
        NetConfig {
            ip,
            mask,
            gw,
            dns,
            mac,
        }
    }
}

const DEFAULT_IP: [u8; 4] = [192, 168, 100, 2];
const DEFAULT_MASK: [u8; 4] = [255, 255, 255, 0];
const DEFAULT_GW: [u8; 4] = [192, 168, 100, 1];
const DEFAULT_DNS: [u8; 4] = [8, 8, 8, 8];

static NET_IP: AtomicU32 = AtomicU32::new(pack_ipv4(DEFAULT_IP));
static NET_MASK: AtomicU32 = AtomicU32::new(pack_ipv4(DEFAULT_MASK));
static NET_GW: AtomicU32 = AtomicU32::new(pack_ipv4(DEFAULT_GW));
static NET_DNS: AtomicU32 = AtomicU32::new(pack_ipv4(DEFAULT_DNS));

pub fn set_config(ip: [u8; 4], mask: [u8; 4], gw: [u8; 4], dns: [u8; 4]) {
    NET_IP.store(pack_ipv4(ip), Ordering::Relaxed);
    NET_MASK.store(pack_ipv4(mask), Ordering::Relaxed);
    NET_GW.store(pack_ipv4(gw), Ordering::Relaxed);
    NET_DNS.store(pack_ipv4(dns), Ordering::Relaxed);
}

pub fn get_config() -> ( [u8; 4], [u8; 4], [u8; 4], [u8; 4] ) {
    (
        unpack_ipv4(NET_IP.load(Ordering::Relaxed)),
        unpack_ipv4(NET_MASK.load(Ordering::Relaxed)),
        unpack_ipv4(NET_GW.load(Ordering::Relaxed)),
        unpack_ipv4(NET_DNS.load(Ordering::Relaxed)),
    )
}

const fn pack_ipv4(ip: [u8; 4]) -> u32 {
    ((ip[0] as u32) << 24) | ((ip[1] as u32) << 16) | ((ip[2] as u32) << 8) | (ip[3] as u32)
}

const fn unpack_ipv4(v: u32) -> [u8; 4] {
    [
        ((v >> 24) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        (v & 0xff) as u8,
    ]
}

pub fn parse_ipv4_str(s: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut idx = 0usize;
    let mut acc = 0u16;
    for &b in s {
        if b == b'.' {
            if idx >= 4 || acc > 255 {
                return None;
            }
            out[idx] = acc as u8;
            idx += 1;
            acc = 0;
            continue;
        }
        if b < b'0' || b > b'9' {
            return None;
        }
        acc = acc * 10 + (b - b'0') as u16;
        if acc > 255 {
            return None;
        }
    }
    if idx != 3 || acc > 255 {
        return None;
    }
    out[3] = acc as u8;
    Some(out)
}

pub fn checksum16(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

pub fn same_subnet(ip: [u8; 4], mask: [u8; 4], other: [u8; 4]) -> bool {
    for i in 0..4 {
        if (ip[i] & mask[i]) != (other[i] & mask[i]) {
            return false;
        }
    }
    true
}

pub fn resolve_arp(cfg: &NetConfig, target_ip: [u8; 4]) -> Option<[u8; 6]> {
    let mut frame = [0u8; 42];
    for b in &mut frame[0..6] {
        *b = 0xff;
    }
    frame[6..12].copy_from_slice(&cfg.mac);
    frame[12..14].copy_from_slice(&ETH_TYPE_ARP.to_be_bytes());

    frame[14..16].copy_from_slice(&1u16.to_be_bytes()); // HTYPE Ethernet
    frame[16..18].copy_from_slice(&0x0800u16.to_be_bytes()); // PTYPE IPv4
    frame[18] = 6;
    frame[19] = 4;
    frame[20..22].copy_from_slice(&1u16.to_be_bytes()); // oper request
    frame[22..28].copy_from_slice(&cfg.mac);
    frame[28..32].copy_from_slice(&cfg.ip);
    frame[32..38].fill(0);
    frame[38..42].copy_from_slice(&target_ip);

    let _ = sys_net_send(frame.as_ptr(), frame.len());

    let start = sys_time_seconds();
    let mut buf = [0u8; 1514];
    loop {
        let n = sys_net_recv(buf.as_mut_ptr(), buf.len());
        if n > 0 {
            if n < 42 {
                continue;
            }
            if u16::from_be_bytes([buf[12], buf[13]]) != ETH_TYPE_ARP {
                continue;
            }
            if u16::from_be_bytes([buf[20], buf[21]]) != 2 {
                continue;
            }
            if buf[28..32] != target_ip {
                continue;
            }
            if buf[38..42] != cfg.ip {
                continue;
            }
            let mut mac = [0u8; 6];
            mac.copy_from_slice(&buf[22..28]);
            return Some(mac);
        }
        if sys_time_seconds().saturating_sub(start) >= 3 {
            return None;
        }
        let _ = sys_yield();
    }
}

pub fn build_ipv4_header(
    buf: &mut [u8],
    total_len: u16,
    proto: u8,
    src: [u8; 4],
    dst: [u8; 4],
    ident: u16,
) -> usize {
    buf[0] = 0x45;
    buf[1] = 0;
    buf[2..4].copy_from_slice(&total_len.to_be_bytes());
    buf[4..6].copy_from_slice(&ident.to_be_bytes());
    buf[6..8].copy_from_slice(&0x4000u16.to_be_bytes()); // DF
    buf[8] = 64;
    buf[9] = proto;
    buf[10..12].fill(0);
    buf[12..16].copy_from_slice(&src);
    buf[16..20].copy_from_slice(&dst);
    let csum = checksum16(&buf[..20]);
    buf[10..12].copy_from_slice(&csum.to_be_bytes());
    20
}

pub fn tcp_checksum(src: [u8; 4], dst: [u8; 4], segment: &[u8]) -> u16 {
    let mut sum = 0u32;
    sum += u16::from_be_bytes([src[0], src[1]]) as u32;
    sum += u16::from_be_bytes([src[2], src[3]]) as u32;
    sum += u16::from_be_bytes([dst[0], dst[1]]) as u32;
    sum += u16::from_be_bytes([dst[2], dst[3]]) as u32;
    sum += IP_PROTO_TCP as u32;
    sum += (segment.len() as u32) & 0xffff;

    let mut i = 0usize;
    while i + 1 < segment.len() {
        sum += u16::from_be_bytes([segment[i], segment[i + 1]]) as u32;
        i += 2;
    }
    if i < segment.len() {
        sum += (segment[i] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

pub fn udp_checksum(src: [u8; 4], dst: [u8; 4], segment: &[u8]) -> u16 {
    let mut sum = 0u32;
    sum += u16::from_be_bytes([src[0], src[1]]) as u32;
    sum += u16::from_be_bytes([src[2], src[3]]) as u32;
    sum += u16::from_be_bytes([dst[0], dst[1]]) as u32;
    sum += u16::from_be_bytes([dst[2], dst[3]]) as u32;
    sum += IP_PROTO_UDP as u32;
    sum += (segment.len() as u32) & 0xffff;

    let mut i = 0usize;
    while i + 1 < segment.len() {
        sum += u16::from_be_bytes([segment[i], segment[i + 1]]) as u32;
        i += 2;
    }
    if i < segment.len() {
        sum += (segment[i] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    let csum = !(sum as u16);
    if csum == 0 { 0xffff } else { csum }
}

pub fn dns_query(name: &[u8], server_ip: [u8; 4]) -> Option<[u8; 4]> {
    let cfg = NetConfig::default();
    if cfg.mac.iter().all(|b| *b == 0) {
        return None;
    }
    dns_query_cfg(&cfg, name, server_ip)
}

pub fn dns_query_default(name: &[u8]) -> Option<[u8; 4]> {
    let cfg = NetConfig::default();
    if cfg.mac.iter().all(|b| *b == 0) {
        return None;
    }
    dns_query_cfg(&cfg, name, cfg.dns)
}

pub fn dns_query_cfg(cfg: &NetConfig, name: &[u8], server_ip: [u8; 4]) -> Option<[u8; 4]> {
    if name.is_empty() || name.len() > 255 {
        return None;
    }
    let next_hop = if same_subnet(cfg.ip, cfg.mask, server_ip) { server_ip } else { cfg.gw };
    let dst_mac = resolve_arp(cfg, next_hop)?;

    let mut frame = [0u8; 14 + 20 + 8 + 512];
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&cfg.mac);
    frame[12..14].copy_from_slice(&ETH_TYPE_IP.to_be_bytes());

    let ip_off = 14;
    let udp_off = ip_off + 20;
    let dns_off = udp_off + 8;
    let mut dns_len = 12usize;

    let id = (sys_time_seconds() as u16) ^ (sys_getpid() as u16);
    frame[dns_off..dns_off + 2].copy_from_slice(&id.to_be_bytes());
    frame[dns_off + 2..dns_off + 4].copy_from_slice(&0x0100u16.to_be_bytes());
    frame[dns_off + 4..dns_off + 6].copy_from_slice(&1u16.to_be_bytes());
    frame[dns_off + 6..dns_off + 8].fill(0);
    frame[dns_off + 8..dns_off + 10].fill(0);
    frame[dns_off + 10..dns_off + 12].fill(0);

    let qname_len = encode_qname(name, &mut frame[dns_off + 12..])?;
    dns_len += qname_len;
    frame[dns_off + dns_len..dns_off + dns_len + 2].copy_from_slice(&1u16.to_be_bytes());
    frame[dns_off + dns_len + 2..dns_off + dns_len + 4].copy_from_slice(&1u16.to_be_bytes());
    dns_len += 4;

    let udp_len = (8 + dns_len) as u16;
    let src_port = 40000u16.wrapping_add(sys_getpid() as u16);
    frame[udp_off..udp_off + 2].copy_from_slice(&src_port.to_be_bytes());
    frame[udp_off + 2..udp_off + 4].copy_from_slice(&53u16.to_be_bytes());
    frame[udp_off + 4..udp_off + 6].copy_from_slice(&udp_len.to_be_bytes());
    frame[udp_off + 6..udp_off + 8].fill(0);

    let total_len = (20 + udp_len as usize) as u16;
    build_ipv4_header(
        &mut frame[ip_off..ip_off + 20],
        total_len,
        IP_PROTO_UDP,
        cfg.ip,
        server_ip,
        id,
    );
    let checksum = udp_checksum(cfg.ip, server_ip, &frame[udp_off..udp_off + udp_len as usize]);
    frame[udp_off + 6..udp_off + 8].copy_from_slice(&checksum.to_be_bytes());

    let frame_len = 14 + total_len as usize;
    let _ = sys_net_send(frame.as_ptr(), frame_len);

    let start = sys_time_seconds();
    let mut buf = [0u8; 1514];
    loop {
        let n = sys_net_recv(buf.as_mut_ptr(), buf.len());
        if n > 0 {
            let n = n as usize;
            if n < 42 {
                continue;
            }
            if u16::from_be_bytes([buf[12], buf[13]]) != ETH_TYPE_IP {
                continue;
            }
            if buf[23] != IP_PROTO_UDP {
                continue;
            }
            if buf[30..34] != cfg.ip {
                continue;
            }
            if buf[26..30] != server_ip {
                continue;
            }
            let ip_off_r = 14;
            let ihl = (buf[ip_off_r] & 0x0f) as usize * 4;
            let udp_off_r = ip_off_r + ihl;
            if udp_off_r + 8 > n {
                continue;
            }
            if u16::from_be_bytes([buf[udp_off_r + 2], buf[udp_off_r + 3]]) != src_port {
                continue;
            }
            let dns_off_r = udp_off_r + 8;
            if dns_off_r + 12 > n {
                continue;
            }
            let rid = u16::from_be_bytes([buf[dns_off_r], buf[dns_off_r + 1]]);
            if rid != id {
                continue;
            }
            let flags = u16::from_be_bytes([buf[dns_off_r + 2], buf[dns_off_r + 3]]);
            if (flags & 0x8000) == 0 {
                continue;
            }
            if (flags & 0x000f) != 0 {
                return None;
            }
            let qd = u16::from_be_bytes([buf[dns_off_r + 4], buf[dns_off_r + 5]]) as usize;
            let an = u16::from_be_bytes([buf[dns_off_r + 6], buf[dns_off_r + 7]]) as usize;
            let mut off = dns_off_r + 12;
            for _ in 0..qd {
                off = skip_name(&buf, off)?;
                if off + 4 > n {
                    return None;
                }
                off += 4;
            }
            for _ in 0..an {
                off = skip_name(&buf, off)?;
                if off + 10 > n {
                    return None;
                }
                let typ = u16::from_be_bytes([buf[off], buf[off + 1]]);
                let class = u16::from_be_bytes([buf[off + 2], buf[off + 3]]);
                let rdlen = u16::from_be_bytes([buf[off + 8], buf[off + 9]]) as usize;
                off += 10;
                if off + rdlen > n {
                    return None;
                }
                if typ == 1 && class == 1 && rdlen == 4 {
                    let ip = [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]];
                    return Some(ip);
                }
                off += rdlen;
            }
            return None;
        }
        if sys_time_seconds().saturating_sub(start) >= 3 {
            return None;
        }
        let _ = sys_yield();
    }
}

fn encode_qname(name: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut idx = 0usize;
    let mut start = 0usize;
    for i in 0..=name.len() {
        if i == name.len() || name[i] == b'.' {
            let len = i - start;
            if len == 0 || len > 63 {
                return None;
            }
            if idx + 1 + len >= out.len() {
                return None;
            }
            out[idx] = len as u8;
            idx += 1;
            out[idx..idx + len].copy_from_slice(&name[start..i]);
            idx += len;
            start = i + 1;
        }
    }
    if idx >= out.len() {
        return None;
    }
    out[idx] = 0;
    idx += 1;
    Some(idx)
}

fn skip_name(buf: &[u8], mut off: usize) -> Option<usize> {
    let mut steps = 0usize;
    while off < buf.len() && steps < 255 {
        let len = buf[off];
        if len & 0xC0 == 0xC0 {
            if off + 1 >= buf.len() {
                return None;
            }
            return Some(off + 2);
        }
        if len == 0 {
            return Some(off + 1);
        }
        off += 1 + len as usize;
        steps += 1;
    }
    None
}
