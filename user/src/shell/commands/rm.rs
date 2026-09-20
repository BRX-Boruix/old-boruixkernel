extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use super::Command;
use user_lib::{sys_getdents, sys_stat, sys_unlink, sys_write, DirEntry, Stat};

const COMMAND_ABOUT: &str =
    "rm\n\nRemove file or directory.\nUsage: rm [-r] [-f] [-l] [--yes-i-do] <path> [path...]";

pub const CMD: Command = Command {
    name: b"rm",
    usage: "rm <path>",
    desc: "Remove file or empty directory",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        println3("usage: rm [-r] [-f] <path> [path...]");
        return;
    }
    let mut recursive = false;
    let mut force = false;
    let mut log_each = false;
    let mut allow_root = false;
    let mut paths: Vec<&[u8]> = Vec::new();

    for a in args.iter().skip(1) {
        if *a == b"--yes-i-do" {
            allow_root = true;
            continue;
        }
        if a.starts_with(b"-") && a.len() > 1 {
            for &b in &a[1..] {
                if b == b'r' {
                    recursive = true;
                } else if b == b'f' {
                    force = true;
                } else if b == b'l' {
                    log_each = true;
                }
            }
            continue;
        }
        paths.push(*a);
    }

    if paths.is_empty() {
        println3("usage: rm [-r] [-f] <path> [path...]");
        return;
    }

    for p in paths {
        let ok = rm_path(p, recursive, force, log_each, allow_root);
        if !ok && !force {
            println3("rm failed");
        }
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

fn is_dir_path(path: &[u8]) -> Option<bool> {
    let mut st = Stat {
        st_mode: 0,
        st_size: 0,
        st_type: 0,
    };
    let path_c = cstr_from_bytes(path);
    let ret = sys_stat(path_c.as_ptr(), &mut st as *mut _);
    if ret < 0 {
        return None;
    }
    // VfsNodeType::Dir == 1
    Some(st.st_type == 1)
}

fn rm_path(path: &[u8], recursive: bool, force: bool, log_each: bool, allow_root: bool) -> bool {
    let mut clean = trim_trailing_slash(path);
    if clean == b"/" {
        if !allow_root {
            if !force {
                println3("rm: refusing to remove / (use --yes-i-do)");
            } else if log_each {
                println3("rm: refusing to remove / (use --yes-i-do)");
            }
            return false;
        }
    }
    let is_dir = match is_dir_path(clean) {
        Some(v) => v,
        None => {
            return force;
        }
    };

    if !is_dir {
        return unlink_path_force(clean, force, log_each);
    }

    if !recursive {
        return unlink_path_force(clean, force, log_each);
    }

    let mut entries: [DirEntry; 64] = [DirEntry { name_len: 0, name: [0u8; 256] }; 64];
    let path_c = cstr_from_bytes(clean);
    loop {
        let n = sys_getdents(path_c.as_ptr(), entries.as_mut_ptr(), entries.len());
        if n < 0 {
            return force;
        }
        if n == 0 {
            break;
        }
        let mut deleted_any = false;
        for i in 0..(n as usize) {
            let e = &entries[i];
            if e.name_len == 0 {
                continue;
            }
            let name = &e.name[..e.name_len as usize];
            let child = join_path(clean, name);
            if rm_path(&child, recursive, force, log_each, allow_root) {
                deleted_any = true;
            } else if !force {
                return false;
            }
        }
        if !deleted_any {
            // Avoid infinite loop if we can't delete anything.
            if !force {
                return false;
            }
            break;
        }
    }
    unlink_path_force(clean, force, log_each)
}

fn unlink_path_force(path: &[u8], force: bool, log_each: bool) -> bool {
    let path_c = cstr_from_bytes(path);
    let ret = sys_unlink(path_c.as_ptr());
    if ret < 0 { return force; }
    if log_each {
        println3_path("rm", path);
    }
    true
}

fn trim_trailing_slash(path: &[u8]) -> &[u8] {
    if path.len() > 1 && path[path.len() - 1] == b'/' {
        &path[..path.len() - 1]
    } else {
        path
    }
}

fn join_path(base: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(base);
    if !base.ends_with(b"/") {
        out.push(b'/');
    }
    out.extend_from_slice(name);
    out
}

fn println3_path(prefix: &str, path: &[u8]) {
    let _ = sys_write(3, prefix.as_ptr(), prefix.len());
    let _ = sys_write(3, " ".as_ptr(), 1);
    let _ = sys_write(3, path.as_ptr(), path.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}
