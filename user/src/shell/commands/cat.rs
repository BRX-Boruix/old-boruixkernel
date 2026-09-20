use super::Command;
use user_lib::{sys_close, sys_open, sys_read, sys_write};

const COMMAND_ABOUT: &str = "cat\n\nPrint file contents.\nUsage: cat <path>";

pub const CMD: Command = Command {
    name: b"cat",
    usage: "cat <path>",
    desc: "Print file contents",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        println3("usage: cat <path>");
        return;
    }
    let path = cstr_from_bytes(args[1]);
    let fd = sys_open(path.as_ptr(), 0, 0);
    if fd < 0 {
        println3("open failed");
        return;
    }

    let mut buf = [0u8; 256];
    loop {
        let n = sys_read(fd as usize, buf.as_mut_ptr(), buf.len());
        if n <= 0 {
            break;
        }
        let _ = sys_write(3, buf.as_ptr(), n as usize);
    }
    let _ = sys_close(fd as usize);
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
