use super::Command;
use user_lib::{sys_getpid, sys_write};

const COMMAND_ABOUT: &str = "pid\n\nShow current PID.\nUsage: pid";

pub const CMD: Command = Command {
    name: b"pid",
    usage: "pid",
    desc: "Show current PID",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(_args: &[&[u8]]) {
    let pid = sys_getpid();
    print_num("pid=", pid);
}

fn print_num(prefix: &str, n: isize) {
    let _ = sys_write(3, prefix.as_ptr(), prefix.len());
    let mut buf = [0u8; 24];
    let mut i = 0usize;
    let mut x = if n < 0 { -n } else { n } as usize;
    if n < 0 {
        buf[i] = b'-';
        i += 1;
    }
    if x == 0 {
        buf[i] = b'0';
        i += 1;
    } else {
        let mut tmp = [0u8; 20];
        let mut t = 0usize;
        while x > 0 {
            tmp[t] = b'0' + (x % 10) as u8;
            t += 1;
            x /= 10;
        }
        while t > 0 {
            t -= 1;
            buf[i] = tmp[t];
            i += 1;
        }
    }
    let _ = sys_write(3, buf.as_ptr(), i);
    let _ = sys_write(3, "\n".as_ptr(), 1);
}
