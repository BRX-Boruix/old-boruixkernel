use super::Command;
use user_lib::{sys_getpid, sys_net_recv, sys_net_send, sys_time_seconds, sys_write, sys_yield};
use user_lib::net::{build_ipv4_header, checksum16, parse_ipv4_str, resolve_arp, same_subnet, NetConfig, ETH_TYPE_IP, IP_PROTO_ICMP};

const COMMAND_ABOUT: &str =
    "ping\n\nSend ICMP echo requests.\nUsage: ping <ip> [count]";

pub const CMD: Command = Command {
    name: b"ping",
    usage: "ping <ip> [count]",
    desc: "Send ICMP echo requests",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        let msg = b"usage: ping <ip> [count]\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    let Some(dst_ip) = parse_ipv4_str(args[1]) else {
        let msg = b"ping: invalid ip\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    };
    let count = if args.len() > 2 {
        parse_u32(args[2]).unwrap_or(4).min(32)
    } else {
        4
    };

    let cfg = NetConfig::default();
    if cfg.mac.iter().all(|b| *b == 0) {
        let msg = b"ping: net not ready\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    let next_hop = if same_subnet(cfg.ip, cfg.mask, dst_ip) { dst_ip } else { cfg.gw };
    let Some(dst_mac) = resolve_arp(&cfg, next_hop) else {
        let msg = b"ping: arp failed\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    };

    let id = (sys_getpid() as u16).wrapping_add(0x1234);
    let mut seq: u16 = 0;
    for _ in 0..count {
        let mut frame = [0u8; 98];
        frame[0..6].copy_from_slice(&dst_mac);
        frame[6..12].copy_from_slice(&cfg.mac);
        frame[12..14].copy_from_slice(&ETH_TYPE_IP.to_be_bytes());

        let ip_offset = 14;
        let icmp_offset = ip_offset + 20;
        frame[icmp_offset] = 8;
        frame[icmp_offset + 1] = 0;
        frame[icmp_offset + 2] = 0;
        frame[icmp_offset + 3] = 0;
        frame[icmp_offset + 4..icmp_offset + 6].copy_from_slice(&id.to_be_bytes());
        frame[icmp_offset + 6..icmp_offset + 8].copy_from_slice(&seq.to_be_bytes());
        for i in 0..32 {
            frame[icmp_offset + 8 + i] = i as u8;
        }
        let icmp_len = 8 + 32;
        let csum = checksum16(&frame[icmp_offset..icmp_offset + icmp_len]);
        frame[icmp_offset + 2..icmp_offset + 4].copy_from_slice(&csum.to_be_bytes());

        let total_len = (20 + icmp_len) as u16;
        build_ipv4_header(
            &mut frame[ip_offset..ip_offset + 20],
            total_len,
            IP_PROTO_ICMP,
            cfg.ip,
            dst_ip,
            seq,
        );

        let frame_len = 14 + total_len as usize;
        let _ = sys_net_send(frame.as_ptr(), frame_len);

        let start = sys_time_seconds();
        let mut ok = false;
        let mut buf = [0u8; 1514];
        while sys_time_seconds().saturating_sub(start) < 2 {
            let n = sys_net_recv(buf.as_mut_ptr(), buf.len());
            if n <= 0 {
                let _ = sys_yield();
                continue;
            }
            if n < 42 {
                continue;
            }
            if u16::from_be_bytes([buf[12], buf[13]]) != ETH_TYPE_IP {
                continue;
            }
            if buf[23] != IP_PROTO_ICMP {
                continue;
            }
            if buf[30..34] != cfg.ip {
                continue;
            }
            if buf[26..30] != dst_ip {
                continue;
            }
            let icmp = 14 + ((buf[14] & 0x0f) as usize) * 4;
            if buf[icmp] != 0 {
                continue;
            }
            if buf[icmp + 4..icmp + 6] != id.to_be_bytes() {
                continue;
            }
            if buf[icmp + 6..icmp + 8] != seq.to_be_bytes() {
                continue;
            }
            ok = true;
            break;
        }

        if ok {
            let msg = b"ping: ok\n";
            let _ = sys_write(3, msg.as_ptr(), msg.len());
        } else {
            let msg = b"ping: timeout\n";
            let _ = sys_write(3, msg.as_ptr(), msg.len());
        }
        seq = seq.wrapping_add(1);
    }
}

fn parse_u32(s: &[u8]) -> Option<u32> {
    let mut v = 0u32;
    for &b in s {
        if b < b'0' || b > b'9' {
            return None;
        }
        v = v * 10 + (b - b'0') as u32;
    }
    Some(v)
}
