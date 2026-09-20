use super::Command;
use user_lib::{sys_net_recv, sys_time_seconds, sys_write, sys_yield};

const COMMAND_ABOUT: &str =
    "netdump\n\nDump basic RX packet info.\nUsage: netdump [count]";

pub const CMD: Command = Command {
    name: b"netdump",
    usage: "netdump [count]",
    desc: "Dump basic RX packet info",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    let count = if args.len() > 1 {
        parse_u32(args[1]).unwrap_or(10).min(200)
    } else {
        10
    };
    let mut seen = 0u32;
    let start = sys_time_seconds();
    let mut buf = [0u8; 1514];
    while seen < count && sys_time_seconds().saturating_sub(start) < 5 {
        let n = sys_net_recv(buf.as_mut_ptr(), buf.len());
        if n <= 0 {
            let _ = sys_yield();
            continue;
        }
        let n = n as usize;
        if n < 14 {
            continue;
        }
        let eth = u16::from_be_bytes([buf[12], buf[13]]);
        if eth == 0x0806 {
            print_line(b"arp");
        } else if eth == 0x0800 && n >= 34 {
            let proto = buf[23];
            if proto == 1 {
                print_line(b"ip icmp");
            } else if proto == 6 {
                if n >= 54 {
                    let src_port = u16::from_be_bytes([buf[34], buf[35]]);
                    let dst_port = u16::from_be_bytes([buf[36], buf[37]]);
                    let flags = buf[47];
                    print_tcp(src_port, dst_port, flags);
                } else {
                    print_line(b"ip tcp");
                }
            } else {
                print_line(b"ip other");
            }
        } else {
            print_line(b"other");
        }
        seen += 1;
    }
}

fn print_line(s: &[u8]) {
    let _ = sys_write(3, s.as_ptr(), s.len());
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn print_tcp(src: u16, dst: u16, flags: u8) {
    let _ = sys_write(3, b"tcp ".as_ptr(), 4);
    write_u16(src);
    let _ = sys_write(3, b"->".as_ptr(), 2);
    write_u16(dst);
    let _ = sys_write(3, b" flags=".as_ptr(), 7);
    write_hex8(flags);
    let _ = sys_write(3, b"\n".as_ptr(), 1);
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
