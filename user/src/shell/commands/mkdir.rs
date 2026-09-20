use super::Command;
use user_lib::{sys_close, sys_open, sys_write, O_CREAT, O_DIRECTORY};

const COMMAND_ABOUT: &str = "mkdir\n\nCreate directory.\nUsage: mkdir <path>";

pub const CMD: Command = Command {
    name: b"mkdir",
    usage: "mkdir <path>",
    desc: "Create directory",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        println3("usage: mkdir <path>");
        return;
    }
    let path = cstr_from_bytes(args[1]);
    let fd = sys_open(path.as_ptr(), O_CREAT | O_DIRECTORY, 0);
    if fd < 0 {
        println3("mkdir failed");
        return;
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
