use super::Command;
use user_lib::{sys_fork, sys_exec, sys_write, sys_exit};

const COMMAND_ABOUT: &str = "testsmp1\n\nRun SMP test #1 in user space.\nUsage: testsmp1 [count]";

pub const CMD: Command = Command {
    name: b"testsmp1",
    usage: "testsmp1 [count]",
    desc: "Run SMP test #1 (user-space)",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(args: &[&[u8]]) {
    let mut buf = [0u8; 32];
    let mut len = 0usize;
    buf[..8].copy_from_slice(b"testsmp1");
    len += 8;

    if args.len() >= 2 {
        let count = parse_int(args[1]);
        if count > 0 {
            buf[len] = b':';
            len += 1;
            len += write_num(&mut buf[len..], count as usize);
        }
    }
    buf[len] = 0;

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

fn parse_int(s: &[u8]) -> isize {
    let mut i = 0usize;
    let mut val: isize = 0;
    while i < s.len() {
        let b = s[i];
        if b < b'0' || b > b'9' {
            return -1;
        }
        val = val * 10 + (b - b'0') as isize;
        i += 1;
    }
    val
}

fn write_num(buf: &mut [u8], mut n: usize) -> usize {
    if n == 0 {
        if !buf.is_empty() {
            buf[0] = b'0';
            return 1;
        }
        return 0;
    }
    let mut tmp = [0u8; 20];
    let mut t = 0usize;
    while n > 0 && t < tmp.len() {
        tmp[t] = b'0' + (n % 10) as u8;
        t += 1;
        n /= 10;
    }
    let mut i = 0usize;
    while t > 0 && i < buf.len() {
        t -= 1;
        buf[i] = tmp[t];
        i += 1;
    }
    i
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
