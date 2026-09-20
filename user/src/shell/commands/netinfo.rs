use super::Command;
use user_lib::{sys_net_counters, sys_net_mac, sys_net_regs, sys_net_status, sys_write};

const COMMAND_ABOUT: &str =
    "netinfo\n\nShow network device status, registers, and counters.\nUsage: netinfo";

pub const CMD: Command = Command {
    name: b"netinfo",
    usage: "netinfo",
    desc: "Show network device status",
    about: COMMAND_ABOUT,
    run,
};

fn run(_args: &[&[u8]]) {
    let mut mac = [0u8; 6];
    let n = sys_net_mac(mac.as_mut_ptr(), mac.len());
    if n != 6 {
        let msg = b"netinfo: no mac (driver not ready)\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    let _ = sys_write(3, b"mac ".as_ptr(), 4);
    write_hex8(mac[0]);
    let _ = sys_write(3, b":".as_ptr(), 1);
    write_hex8(mac[1]);
    let _ = sys_write(3, b":".as_ptr(), 1);
    write_hex8(mac[2]);
    let _ = sys_write(3, b":".as_ptr(), 1);
    write_hex8(mac[3]);
    let _ = sys_write(3, b":".as_ptr(), 1);
    write_hex8(mac[4]);
    let _ = sys_write(3, b":".as_ptr(), 1);
    write_hex8(mac[5]);
    let _ = sys_write(3, b"\n".as_ptr(), 1);

    let status = sys_net_status();
    if status < 0 {
        let msg = b"status: unavailable\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    let _ = sys_write(3, b"status ".as_ptr(), 7);
    write_hex32(status as u32);
    let _ = sys_write(3, b"\n".as_ptr(), 1);

    let mut regs = [0u32; 8];
    let n = sys_net_regs(regs.as_mut_ptr(), regs.len());
    if n == 8 {
        let _ = sys_write(3, b"regs status/ctrl/rctl/tctl/rdh/rdt/tdh/tdt\n".as_ptr(), 55);
        for (i, v) in regs.iter().enumerate() {
            write_hex32(*v);
            if i + 1 == regs.len() {
                let _ = sys_write(3, b"\n".as_ptr(), 1);
            } else {
                let _ = sys_write(3, b" ".as_ptr(), 1);
            }
        }
    }

    let mut ctrs = [0u64; 2];
    let n = sys_net_counters(ctrs.as_mut_ptr(), ctrs.len());
    if n == 2 {
        let _ = sys_write(3, b"tx/rx ".as_ptr(), 6);
        write_u64(ctrs[0]);
        let _ = sys_write(3, b"/".as_ptr(), 1);
        write_u64(ctrs[1]);
        let _ = sys_write(3, b"\n".as_ptr(), 1);
    }
}

fn write_hex8(value: u8) {
    let mut buf = [0u8; 2];
    buf[0] = nibble((value >> 4) & 0xF);
    buf[1] = nibble(value & 0xF);
    let _ = sys_write(3, buf.as_ptr(), 2);
}

fn write_hex16(value: u16) {
    write_hex8((value >> 8) as u8);
    write_hex8((value & 0xFF) as u8);
}

fn write_hex32(value: u32) {
    write_hex16((value >> 16) as u16);
    write_hex16((value & 0xFFFF) as u16);
}

fn write_u64(mut value: u64) {
    let mut buf = [0u8; 20];
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

fn nibble(x: u8) -> u8 {
    match x {
        0..=9 => b'0' + x,
        10..=15 => b'a' + (x - 10),
        _ => b'?'
    }
}
