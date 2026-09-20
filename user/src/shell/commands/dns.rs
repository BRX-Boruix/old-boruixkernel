use super::Command;
use user_lib::{sys_write};
use user_lib::net::{dns_query, dns_query_default, parse_ipv4_str};

const COMMAND_ABOUT: &str =
    "dns\n\nResolve hostname to IPv4 address.\nUsage: dns <name> [server_ip]";

pub const CMD: Command = Command {
    name: b"dns",
    usage: "dns <name> [server_ip]",
    desc: "Resolve hostname to IPv4",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        let msg = b"usage: dns <name> [server_ip]\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    let name = args[1];
    if args.len() > 2 {
        let server_ip = match parse_ipv4_str(args[2]) {
            Some(ip) => ip,
            None => {
                let msg = b"dns: invalid server ip\n";
                let _ = sys_write(3, msg.as_ptr(), msg.len());
                return;
            }
        };
        match dns_query(name, server_ip) {
            Some(ip) => {
                print_ip(ip);
                let _ = sys_write(3, b"\n".as_ptr(), 1);
            }
            None => {
                let msg = b"dns: failed\n";
                let _ = sys_write(3, msg.as_ptr(), msg.len());
            }
        }
        return;
    }
    match dns_query_default(name) {
        Some(ip) => {
            print_ip(ip);
            let _ = sys_write(3, b"\n".as_ptr(), 1);
        }
        None => {
            let msg = b"dns: failed\n";
            let _ = sys_write(3, msg.as_ptr(), msg.len());
        }
    }
}

fn print_ip(ip: [u8; 4]) {
    let mut buf = [0u8; 16];
    let mut n = 0usize;
    n += write_u8(&mut buf[n..], ip[0]);
    buf[n] = b'.';
    n += 1;
    n += write_u8(&mut buf[n..], ip[1]);
    buf[n] = b'.';
    n += 1;
    n += write_u8(&mut buf[n..], ip[2]);
    buf[n] = b'.';
    n += 1;
    n += write_u8(&mut buf[n..], ip[3]);
    let _ = sys_write(3, buf.as_ptr(), n);
}

fn write_u8(dst: &mut [u8], value: u8) -> usize {
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
        dst[j] = buf[i - 1 - j];
    }
    i
}
