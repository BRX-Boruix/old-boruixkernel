use super::Command;
use user_lib::{sys_umount, sys_write};

const COMMAND_ABOUT: &str = "umount\n\nUnmount filesystem.\nUsage: umount <dir>";

pub const CMD: Command = Command {
    name: b"umount",
    usage: "umount <dir>",
    desc: "Unmount filesystem",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        println3("usage: umount <dir>");
        return;
    }
    let dir = cstr_from_bytes(args[1]);
    let ret = sys_umount(dir.as_ptr());
    if ret != 0 {
        println3("umount failed");
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
