use super::Command;
use user_lib::{sys_getpid, sys_net_recv, sys_net_send, sys_time_seconds, sys_write, sys_yield};
use user_lib::net::{
    build_ipv4_header, dns_query_cfg, parse_ipv4_str, resolve_arp, same_subnet, tcp_checksum,
    NetConfig, ETH_TYPE_IP, IP_PROTO_TCP,
};

const TCP_FIN: u8 = 0x01;
const TCP_SYN: u8 = 0x02;
const TCP_RST: u8 = 0x04;
const TCP_PSH: u8 = 0x08;
const TCP_ACK: u8 = 0x10;

const COMMAND_ABOUT: &str =
    "getnet\n\nHTTP GET over TCP (no TLS).\nUsage: getnet [-d] <ip|host> <path> [port]";

pub const CMD: Command = Command {
    name: b"getnet",
    usage: "getnet [-d] <ip|host> <path> [port]",
    desc: "HTTP GET over TCP (no TLS)",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 3 {
        let msg = b"usage: getnet [-d] <ip|host> <path> [port]\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    let mut arg_idx = 1usize;
    let debug = args.get(1) == Some(&b"-d".as_ref());
    if debug {
        arg_idx += 1;
    }
    if args.len() <= arg_idx + 1 {
        let msg = b"usage: getnet [-d] <ip|host> <path> [port]\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    let host = args[arg_idx];
    let port = if args.len() > arg_idx + 2 {
        parse_u16(args[arg_idx + 2]).unwrap_or(80)
    } else {
        80
    };
    let path = args[arg_idx + 1];
    if !path.starts_with(b"/") {
        let msg = b"getnet: path must start with '/'\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }

    let cfg = NetConfig::default();
    if cfg.mac.iter().all(|b| *b == 0) {
        let msg = b"getnet: net not ready\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    let dst_ip = match parse_ipv4_str(host) {
        Some(ip) => ip,
        None => {
            match dns_query_cfg(&cfg, host, cfg.dns) {
                Some(ip) => ip,
                None => {
                    let msg = b"getnet: dns failed\n";
                    let _ = sys_write(3, msg.as_ptr(), msg.len());
                    return;
                }
            }
        }
    };
    let next_hop = if same_subnet(cfg.ip, cfg.mask, dst_ip) { dst_ip } else { cfg.gw };
    let Some(dst_mac) = resolve_arp(&cfg, next_hop) else {
        let msg = b"getnet: arp failed\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    };

    let src_port = 40000u16.wrapping_add(sys_getpid() as u16);
    let mut seq = (sys_time_seconds() as u32) ^ ((src_port as u32) << 16);

    let mut syn_ok = None;
    for _ in 0..3 {
        if send_tcp(&cfg, &dst_mac, dst_ip, src_port, port, seq, 0, TCP_SYN) < 0 {
            let msg = b"getnet: send syn failed\n";
            let _ = sys_write(3, msg.as_ptr(), msg.len());
            return;
        }
        match wait_tcp(&cfg, src_port, dst_ip, port, TCP_SYN | TCP_ACK, 2, debug) {
            WaitResult::SynAck(peer_seq, _payload_len) => {
                syn_ok = Some(peer_seq);
                break;
            }
            WaitResult::Rst => {
                let msg = b"getnet: connection refused\n";
                let _ = sys_write(3, msg.as_ptr(), msg.len());
                return;
            }
            WaitResult::Timeout => {}
        }
    }
    let Some(peer_seq) = syn_ok else {
        let msg = b"getnet: syn timeout\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    };
    let mut ack = peer_seq.wrapping_add(1);
    seq = seq.wrapping_add(1);

    let _ = send_tcp(&cfg, &dst_mac, dst_ip, src_port, port, seq, ack, TCP_ACK);

    let mut req = [0u8; 256];
    let mut n = 0usize;
    n += copy_str(&mut req[n..], b"GET ");
    n += copy_str(&mut req[n..], path);
    n += copy_str(&mut req[n..], b" HTTP/1.0\r\nHost: ");
    n += copy_str(&mut req[n..], host);
    n += copy_str(&mut req[n..], b"\r\n\r\n");

    if send_tcp_payload(
        &cfg,
        &dst_mac,
        dst_ip,
        src_port,
        port,
        seq,
        ack,
        TCP_PSH | TCP_ACK,
        &req[..n],
    ) < 0 {
        let msg = b"getnet: send http failed\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    seq = seq.wrapping_add(n as u32);

    let start = sys_time_seconds();
    let mut buf = [0u8; 1514];
    let mut received_any = false;
    let mut last_activity = start;
    let mut resend_left = 2;
    loop {
        let nrecv = sys_net_recv(buf.as_mut_ptr(), buf.len());
        if nrecv <= 0 {
            let now = sys_time_seconds();
            if now.saturating_sub(start) > 8 {
                break;
            }
            if !received_any && resend_left > 0 && now.saturating_sub(last_activity) >= 2 {
                let _ = send_tcp_payload(
                    &cfg,
                    &dst_mac,
                    dst_ip,
                    src_port,
                    port,
                    seq,
                    ack,
                    TCP_PSH | TCP_ACK,
                    &req[..n],
                );
                resend_left -= 1;
                last_activity = now;
            }
            let _ = sys_yield();
            continue;
        }
        received_any = true;
        last_activity = sys_time_seconds();
        if debug {
            if let Some(info) = parse_tcp_brief(&buf[..nrecv as usize]) {
                print_tcp_brief(&info);
            }
        }
        let Some((tcp_off, payload_len, pkt_seq, flags)) =
            parse_tcp(&buf[..nrecv as usize], cfg.ip, dst_ip, src_port, port)
        else {
            continue;
        };
        if flags & TCP_RST != 0 {
            break;
        }
        if pkt_seq != ack {
            let _ = send_tcp(&cfg, &dst_mac, dst_ip, src_port, port, seq, ack, TCP_ACK);
            continue;
        }
        if payload_len > 0 {
            let payload = &buf[tcp_off..tcp_off + payload_len];
            let _ = sys_write(3, payload.as_ptr(), payload.len());
            ack = ack.wrapping_add(payload_len as u32);
            let _ = send_tcp(&cfg, &dst_mac, dst_ip, src_port, port, seq, ack, TCP_ACK);
        }
        if flags & TCP_FIN != 0 {
            ack = ack.wrapping_add(1);
            let _ = send_tcp(&cfg, &dst_mac, dst_ip, src_port, port, seq, ack, TCP_ACK | TCP_FIN);
            break;
        }
    }
}

fn send_tcp(
    cfg: &NetConfig,
    dst_mac: &[u8; 6],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
) -> isize {
    send_tcp_payload(cfg, dst_mac, dst_ip, src_port, dst_port, seq, ack, flags, &[])
}

fn send_tcp_payload(
    cfg: &NetConfig,
    dst_mac: &[u8; 6],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> isize {
    let tcp_len = 20 + payload.len();
    let total_len = 20 + tcp_len;
    let mut frame = [0u8; 14 + 20 + 20 + 256];
    if payload.len() > 256 {
        return -1;
    }
    frame[0..6].copy_from_slice(dst_mac);
    frame[6..12].copy_from_slice(&cfg.mac);
    frame[12..14].copy_from_slice(&ETH_TYPE_IP.to_be_bytes());

    let ip_off = 14;
    build_ipv4_header(
        &mut frame[ip_off..ip_off + 20],
        total_len as u16,
        IP_PROTO_TCP,
        cfg.ip,
        dst_ip,
        (seq & 0xffff) as u16,
    );

    let tcp_off = ip_off + 20;
    frame[tcp_off..tcp_off + 2].copy_from_slice(&src_port.to_be_bytes());
    frame[tcp_off + 2..tcp_off + 4].copy_from_slice(&dst_port.to_be_bytes());
    frame[tcp_off + 4..tcp_off + 8].copy_from_slice(&seq.to_be_bytes());
    frame[tcp_off + 8..tcp_off + 12].copy_from_slice(&ack.to_be_bytes());
    frame[tcp_off + 12] = 5u8 << 4;
    let ack_flag = if ack != 0 { TCP_ACK } else { 0 };
    frame[tcp_off + 13] = flags | ack_flag;
    frame[tcp_off + 14..tcp_off + 16].copy_from_slice(&0x4000u16.to_be_bytes());
    frame[tcp_off + 16..tcp_off + 18].fill(0);
    frame[tcp_off + 18..tcp_off + 20].fill(0);
    frame[tcp_off + 20..tcp_off + 20 + payload.len()].copy_from_slice(payload);

    let csum = tcp_checksum(cfg.ip, dst_ip, &frame[tcp_off..tcp_off + tcp_len]);
    frame[tcp_off + 16..tcp_off + 18].copy_from_slice(&csum.to_be_bytes());

    let frame_len = 14 + total_len;
    sys_net_send(frame.as_ptr(), frame_len)
}

enum WaitResult {
    SynAck(u32, usize),
    Rst,
    Timeout,
}

fn wait_tcp(
    cfg: &NetConfig,
    src_port: u16,
    dst_ip: [u8; 4],
    dst_port: u16,
    flags: u8,
    timeout: isize,
    debug: bool,
) -> WaitResult {
    let start = sys_time_seconds();
    let mut buf = [0u8; 1514];
    loop {
        let n = sys_net_recv(buf.as_mut_ptr(), buf.len());
        if n > 0 {
            if debug {
                if let Some(info) = parse_tcp_brief(&buf[..n as usize]) {
                    print_tcp_brief(&info);
                }
            }
            if let Some((tcp_off, payload_len, seq, f)) =
                parse_tcp(&buf[..n as usize], cfg.ip, dst_ip, src_port, dst_port)
            {
                if (f & TCP_RST) != 0 {
                    return WaitResult::Rst;
                }
                if (f & flags) == flags {
                    let _ = tcp_off;
                    return WaitResult::SynAck(seq, payload_len);
                }
            }
        }
        if sys_time_seconds().saturating_sub(start) >= timeout {
            return WaitResult::Timeout;
        }
        let _ = sys_yield();
    }
}

fn parse_tcp(
    buf: &[u8],
    local_ip: [u8; 4],
    remote_ip: [u8; 4],
    local_port: u16,
    remote_port: u16,
) -> Option<(usize, usize, u32, u8)> {
    if buf.len() < 54 {
        return None;
    }
    if u16::from_be_bytes([buf[12], buf[13]]) != ETH_TYPE_IP {
        return None;
    }
    let ip_off = 14;
    let ihl = (buf[ip_off] & 0x0f) as usize * 4;
    if buf[ip_off + 9] != IP_PROTO_TCP {
        return None;
    }
    if buf[ip_off + 12..ip_off + 16] != remote_ip {
        return None;
    }
    if buf[ip_off + 16..ip_off + 20] != local_ip {
        return None;
    }
    let tcp_off = ip_off + ihl;
    if tcp_off + 20 > buf.len() {
        return None;
    }
    if u16::from_be_bytes([buf[tcp_off], buf[tcp_off + 1]]) != remote_port {
        return None;
    }
    if u16::from_be_bytes([buf[tcp_off + 2], buf[tcp_off + 3]]) != local_port {
        return None;
    }
    let seq = u32::from_be_bytes([
        buf[tcp_off + 4],
        buf[tcp_off + 5],
        buf[tcp_off + 6],
        buf[tcp_off + 7],
    ]);
    let data_off = ((buf[tcp_off + 12] >> 4) as usize) * 4;
    let flags = buf[tcp_off + 13];
    let payload_off = tcp_off + data_off;
    if payload_off > buf.len() {
        return None;
    }
    let total_len = u16::from_be_bytes([buf[ip_off + 2], buf[ip_off + 3]]) as usize;
    let payload_len = total_len.saturating_sub(ihl + data_off);
    Some((payload_off, payload_len, seq, flags))
}

struct TcpBrief {
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    flags: u8,
    seq: u32,
    ack: u32,
    payload_len: usize,
}

fn parse_tcp_brief(buf: &[u8]) -> Option<TcpBrief> {
    if buf.len() < 54 {
        return None;
    }
    if u16::from_be_bytes([buf[12], buf[13]]) != ETH_TYPE_IP {
        return None;
    }
    let ip_off = 14;
    let ihl = (buf[ip_off] & 0x0f) as usize * 4;
    if buf[ip_off + 9] != IP_PROTO_TCP {
        return None;
    }
    let src_ip = [buf[ip_off + 12], buf[ip_off + 13], buf[ip_off + 14], buf[ip_off + 15]];
    let dst_ip = [buf[ip_off + 16], buf[ip_off + 17], buf[ip_off + 18], buf[ip_off + 19]];
    let tcp_off = ip_off + ihl;
    if tcp_off + 20 > buf.len() {
        return None;
    }
    let src_port = u16::from_be_bytes([buf[tcp_off], buf[tcp_off + 1]]);
    let dst_port = u16::from_be_bytes([buf[tcp_off + 2], buf[tcp_off + 3]]);
    let seq = u32::from_be_bytes([
        buf[tcp_off + 4],
        buf[tcp_off + 5],
        buf[tcp_off + 6],
        buf[tcp_off + 7],
    ]);
    let ack = u32::from_be_bytes([
        buf[tcp_off + 8],
        buf[tcp_off + 9],
        buf[tcp_off + 10],
        buf[tcp_off + 11],
    ]);
    let data_off = ((buf[tcp_off + 12] >> 4) as usize) * 4;
    let flags = buf[tcp_off + 13];
    let total_len = u16::from_be_bytes([buf[ip_off + 2], buf[ip_off + 3]]) as usize;
    let payload_len = total_len.saturating_sub(ihl + data_off);
    Some(TcpBrief {
        src_ip,
        dst_ip,
        src_port,
        dst_port,
        flags,
        seq,
        ack,
        payload_len,
    })
}

fn print_tcp_brief(info: &TcpBrief) {
    let _ = sys_write(3, b"tcp ".as_ptr(), 4);
    write_ip(&info.src_ip);
    let _ = sys_write(3, b":".as_ptr(), 1);
    write_u16(info.src_port);
    let _ = sys_write(3, b" -> ".as_ptr(), 4);
    write_ip(&info.dst_ip);
    let _ = sys_write(3, b":".as_ptr(), 1);
    write_u16(info.dst_port);
    let _ = sys_write(3, b" flags=".as_ptr(), 7);
    write_hex8(info.flags);
    let _ = sys_write(3, b" seq=".as_ptr(), 5);
    write_u32(info.seq);
    let _ = sys_write(3, b" ack=".as_ptr(), 5);
    write_u32(info.ack);
    let _ = sys_write(3, b" len=".as_ptr(), 5);
    write_u32(info.payload_len as u32);
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn copy_str(dst: &mut [u8], src: &[u8]) -> usize {
    let n = core::cmp::min(dst.len(), src.len());
    dst[..n].copy_from_slice(&src[..n]);
    n
}

fn write_ip_str(dst: &mut [u8], ip: [u8; 4]) -> usize {
    let mut n = 0usize;
    n += write_u8(dst, n, ip[0]);
    n += write_char(dst, n, b'.');
    n += write_u8(dst, n, ip[1]);
    n += write_char(dst, n, b'.');
    n += write_u8(dst, n, ip[2]);
    n += write_char(dst, n, b'.');
    n += write_u8(dst, n, ip[3]);
    n
}

fn write_u8(dst: &mut [u8], offset: usize, value: u8) -> usize {
    let mut buf = [0u8; 3];
    let mut v = value as u32;
    let mut i = 0usize;
    if v == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        while v > 0 {
            buf[i] = b'0' + (v % 10) as u8;
            i += 1;
            v /= 10;
        }
    }
    for j in 0..i {
        dst[offset + j] = buf[i - 1 - j];
    }
    i
}

fn write_char(dst: &mut [u8], offset: usize, ch: u8) -> usize {
    dst[offset] = ch;
    1
}

fn write_u16(mut value: u16) {
    let mut buf = [0u8; 5];
    let mut i = 0usize;
    if value == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        while value > 0 {
            buf[i] = b'0' + (value % 10) as u8;
            i += 1;
            value /= 10;
        }
    }
    while i > 0 {
        i -= 1;
        let _ = sys_write(3, &buf[i] as *const u8, 1);
    }
}

fn write_u32(mut value: u32) {
    let mut buf = [0u8; 10];
    let mut i = 0usize;
    if value == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        while value > 0 {
            buf[i] = b'0' + (value % 10) as u8;
            i += 1;
            value /= 10;
        }
    }
    while i > 0 {
        i -= 1;
        let _ = sys_write(3, &buf[i] as *const u8, 1);
    }
}

fn write_hex8(value: u8) {
    let mut buf = [0u8; 2];
    buf[0] = nibble((value >> 4) & 0xF);
    buf[1] = nibble(value & 0xF);
    let _ = sys_write(3, buf.as_ptr(), 2);
}

fn nibble(x: u8) -> u8 {
    match x {
        0..=9 => b'0' + x,
        10..=15 => b'a' + (x - 10),
        _ => b'?'
    }
}

fn write_ip(ip: &[u8; 4]) {
    let mut buf = [0u8; 16];
    let mut n = 0usize;
    n += write_u8(&mut buf[n..], 0, ip[0]);
    buf[n] = b'.';
    n += 1;
    n += write_u8(&mut buf[n..], 0, ip[1]);
    buf[n] = b'.';
    n += 1;
    n += write_u8(&mut buf[n..], 0, ip[2]);
    buf[n] = b'.';
    n += 1;
    n += write_u8(&mut buf[n..], 0, ip[3]);
    let _ = sys_write(3, buf.as_ptr(), n);
}

fn parse_u16(s: &[u8]) -> Option<u16> {
    let mut v = 0u16;
    for &b in s {
        if b < b'0' || b > b'9' {
            return None;
        }
        v = v * 10 + (b - b'0') as u16;
    }
    Some(v)
}
