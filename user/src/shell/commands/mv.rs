use super::Command;
use user_lib::{sys_rename, sys_write};

const COMMAND_ABOUT: &str = "mv\n\nRename file in place.\nUsage: mv <old> <new_name>";

pub const CMD: Command = Command {
    name: b"mv",
    usage: "mv <old> <new_name>",
    desc: "Rename file (same directory only)",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 3 {
        println3("usage: mv <old> <new_name>");
        return;
    }
    let old = cstr_from_bytes(args[1]);
    let new = cstr_from_bytes(args[2]);
    let ret = sys_rename(old.as_ptr(), new.as_ptr());
    if ret < 0 {
        println3("mv failed");
    }
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
