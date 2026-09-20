use super::Command;
use user_lib::{sys_mount, sys_write};

const COMMAND_ABOUT: &str = "mount\n\nMount filesystem.\nUsage: mount <type> <dir> [src|-]";

pub const CMD: Command = Command {
    name: b"mount",
    usage: "mount <type> <dir> [src|-]",
    desc: "Mount filesystem",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 3 {
        println3("usage: mount <type> <dir> [src|-]");
        return;
    }
    let fstype = cstr_from_bytes(args[1]);
    let dir = cstr_from_bytes(args[2]);
    let dev = if args.len() >= 4 && args[3] != b"-" {
        Some(cstr_from_bytes(args[3]))
    } else {
        None
    };

    let ret = match dev {
        Some(d) => sys_mount(d.as_ptr(), dir.as_ptr(), fstype.as_ptr()),
        None => sys_mount(core::ptr::null(), dir.as_ptr(), fstype.as_ptr()),
    };
    if ret != 0 {
        println3("mount failed");
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
