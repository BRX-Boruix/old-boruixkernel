extern crate alloc;

use alloc::vec::Vec;

use super::Command;
use user_lib::{sys_time_human, sys_time_seconds, sys_write};

const COMMAND_ABOUT: &str =
    "time\n\nRead CMOS/BIOS time.\nUse 'h' for human-readable, 's' for seconds since 2000-01-01.\nUsage: time (h|s)";

pub const CMD: Command = Command {
    name: b"time",
    usage: "time (h|s)",
    desc: "Show BIOS time (human or seconds since 2000)",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(args: &[&[u8]]) {
    if args.len() != 2 {
        print_usage();
        return;
    }
    match args[1] {
        b"h" => print_human(),
        b"s" => print_seconds(),
        _ => print_usage(),
    }
}

fn print_human() {
    let mut buffer = [0u8; 64];
    let n = sys_time_human(buffer.as_mut_ptr(), buffer.len()) as isize;
    if n <= 0 {
        print_err("failed to read BIOS time");
        return;
    }
    let bytes = &buffer[..n as usize];
    let _ = sys_write(3, bytes.as_ptr(), bytes.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}

fn print_seconds() {
    let secs = sys_time_seconds();
    if secs < 0 {
        print_err("failed to read BIOS time");
        return;
    }
    print_num(secs, "seconds since 2000=");
}

fn print_err(msg: &str) {
    let _ = sys_write(3, msg.as_ptr(), msg.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}

fn print_usage() {
    print_err("usage: time h|s");
}

fn print_num(value: isize, prefix: &str) {
    let _ = sys_write(3, prefix.as_ptr(), prefix.len());
    let mut buf = Vec::new();
    if value == 0 {
        buf.push(b'0');
    } else {
        let mut x = value;
        if x < 0 {
            buf.push(b'-');
            x = -x;
        }
        let mut digits = Vec::new();
        while x > 0 {
            digits.push(b'0' + (x % 10) as u8);
            x /= 10;
        }
        while let Some(d) = digits.pop() {
            buf.push(d);
        }
    }
    let _ = sys_write(3, buf.as_ptr(), buf.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}
