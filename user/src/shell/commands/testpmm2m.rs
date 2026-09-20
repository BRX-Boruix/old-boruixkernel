use super::Command;
use user_lib::{sys_testpmm2m, sys_write};

const COMMAND_ABOUT: &str =
    "testpmm2m\n\nTry allocating 2M pages in PMM.\nUsage: testpmm2m [count]";

pub const CMD: Command = Command {
    name: b"testpmm2m",
    usage: "testpmm2m [count]",
    desc: "Try allocate 2M pages in PMM",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    let mut count = 1usize;
    if args.len() >= 2 {
        let v = parse_int(args[1]);
        if v > 0 {
            count = v as usize;
        }
    }
    let ok = sys_testpmm2m(count);
    print_num("testpmm2m ok=", ok);
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
