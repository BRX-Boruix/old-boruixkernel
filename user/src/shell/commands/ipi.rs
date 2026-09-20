use super::Command;
use user_lib::{sys_fork, sys_exec, sys_write, sys_exit};

const COMMAND_ABOUT: &str = "ipi\n\nRun IPI test (user-space stub).\nUsage: ipi";

pub const CMD: Command = Command {
    name: b"ipi",
    usage: "ipi",
    desc: "Run ipi test (user-space stub)",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(_args: &[&[u8]]) {
    let mut buf = [0u8; 8];
    buf[..3].copy_from_slice(b"ipi");
    buf[3] = 0;

    let pid = sys_fork();
    if pid == 0 {
        let r = sys_exec(buf.as_ptr());
        if r != 0 {
            println3("exec failed");
        }
        sys_exit(-1);
    } else if pid > 0 {
        print_num("run pid=", pid);
    } else {
        println3("fork failed");
    }
}

fn println3(s: &str) {
    let _ = sys_write(3, s.as_ptr(), s.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
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
