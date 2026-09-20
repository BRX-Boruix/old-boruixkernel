use super::Command;
use user_lib::net::{get_config, parse_ipv4_str, set_config};
use user_lib::sys_write;

const COMMAND_ABOUT: &str =
    "netcfg\n\nShow or set network config (IP/Mask/GW/DNS).\nUsage: netcfg [show|set <ip> <mask> <gw> <dns>]";

pub const CMD: Command = Command {
    name: b"netcfg",
    usage: "netcfg [show|set <ip> <mask> <gw> <dns>]",
    desc: "Get or set network config",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() == 1 || args[1] == b"show" {
        let (ip, mask, gw, dns) = get_config();
        print_line(b"ip ", ip);
        print_line(b"mask ", mask);
        print_line(b"gw ", gw);
        print_line(b"dns ", dns);
        return;
    }
    if args[1] != b"set" || args.len() < 6 {
        let msg = b"usage: netcfg [show|set <ip> <mask> <gw> <dns>]\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    let Some(ip) = parse_ipv4_str(args[2]) else { return err("ip"); };
    let Some(mask) = parse_ipv4_str(args[3]) else { return err("mask"); };
    let Some(gw) = parse_ipv4_str(args[4]) else { return err("gw"); };
    let Some(dns) = parse_ipv4_str(args[5]) else { return err("dns"); };
    set_config(ip, mask, gw, dns);
    let msg = b"netcfg: updated\n";
    let _ = sys_write(3, msg.as_ptr(), msg.len());
}

fn err(field: &str) {
    let _ = sys_write(3, b"netcfg: invalid ".as_ptr(), 16);
    let _ = sys_write(3, field.as_ptr(), field.len());
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn print_line(prefix: &[u8], ip: [u8; 4]) {
    let _ = sys_write(3, prefix.as_ptr(), prefix.len());
    print_ip(ip);
    let _ = sys_write(3, b"\n".as_ptr(), 1);
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
