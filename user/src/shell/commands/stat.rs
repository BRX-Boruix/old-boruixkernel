use super::Command;
use user_lib::{sys_stat, sys_write, Stat};

const COMMAND_ABOUT: &str = "stat\n\nShow file metadata.\nUsage: stat <path>";

pub const CMD: Command = Command {
    name: b"stat",
    usage: "stat <path>",
    desc: "Show file metadata",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        println3("usage: stat <path>");
        return;
    }
    let path = cstr_from_bytes(args[1]);
    let mut st = Stat { st_mode: 0, st_size: 0, st_type: 0 };
    let ret = sys_stat(path.as_ptr(), &mut st as *mut Stat);
    if ret != 0 {
        println3("stat failed");
        return;
    }
    print_stat(&st);
}

fn print_stat(st: &Stat) {
    let _ = sys_write(3, "size=".as_ptr(), 5);
    print_num(st.st_size as usize);
    let _ = sys_write(3, " mode=".as_ptr(), 6);
    print_num(st.st_mode as usize);
    let _ = sys_write(3, " type=".as_ptr(), 6);
    print_num(st.st_type as usize);
    let _ = sys_write(3, "\n".as_ptr(), 1);
}

fn print_num(mut n: usize) {
    let mut buf = [0u8; 32];
    let mut i = 0usize;
    if n == 0 {
        buf[i] = b'0';
        i += 1;
    } else {
        let mut tmp = [0u8; 32];
        let mut t = 0usize;
        while n > 0 {
            tmp[t] = b'0' + (n % 10) as u8;
            t += 1;
            n /= 10;
        }
        while t > 0 {
            t -= 1;
            buf[i] = tmp[t];
            i += 1;
        }
    }
    let _ = sys_write(3, buf.as_ptr(), i);
}

fn cstr_from_bytes(bytes: &[u8]) -> [u8; 256] {
    let mut out = [0u8; 256];
    let mut i = 0usize;
    for &b in bytes {
        if i + 1 >= out.len() {
            break;
        }
        out[i] = b;
        i += 1;
    }
    out[i] = 0;
    out
}

fn println3(s: &str) {
    let _ = sys_write(3, s.as_ptr(), s.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}
