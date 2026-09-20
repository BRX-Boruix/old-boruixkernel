extern crate alloc;

use alloc::vec::Vec;
use super::Command;
use user_lib::{sys_getdents, sys_write, DirEntry};

const COMMAND_ABOUT: &str = "ls\n\nList directory entries.\nUsage: ls <path>";

pub const CMD: Command = Command {
    name: b"ls",
    usage: "ls <path>",
    desc: "List directory entries",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    let path = if args.len() >= 2 { args[1] } else { b"/" };
    let path_c = cstr_from_bytes(path);

    let mut entries: [DirEntry; 64] = [DirEntry { name_len: 0, name: [0u8; 256] }; 64];
    let n = sys_getdents(path_c.as_ptr(), entries.as_mut_ptr(), entries.len());
    if n < 0 {
        println3("ls failed");
        return;
    }
    if n == 0 {
        return;
    }

    let mut names: Vec<(Vec<u8>, bool)> = Vec::new();
    for i in 0..(n as usize) {
        let e = &entries[i];
        if e.name_len == 0 {
            continue;
        }
        let mut v = Vec::new();
        v.extend_from_slice(&e.name[..e.name_len as usize]);
        let is_dir = is_dir_entry(path, &v);
        names.push((v, is_dir));
    }

    names.sort_by(|a, b| cmp_bytes(&a.0, &b.0));

    let mut max_len = 0usize;
    for (n, is_dir) in &names {
        let len = if *is_dir { n.len() + 1 } else { n.len() };
        if len > max_len {
            max_len = len;
        }
    }
    let col_width = if max_len + 2 < 4 { 4 } else { max_len + 2 };
    let term_width = 80usize;
    let cols = core::cmp::max(1, term_width / col_width);

    for (i, (name, is_dir)) in names.iter().enumerate() {
        let _ = sys_write(3, name.as_ptr(), name.len());
        if *is_dir {
            let _ = sys_write(3, "/".as_ptr(), 1);
        }
        let is_last_in_row = (i + 1) % cols == 0;
        if is_last_in_row {
            let _ = sys_write(3, "\n".as_ptr(), 1);
            continue;
        }
        let shown_len = if *is_dir { name.len() + 1 } else { name.len() };
        let pad = col_width.saturating_sub(shown_len);
        for _ in 0..pad {
            let _ = sys_write(3, " ".as_ptr(), 1);
        }
    }
    if names.len() % cols != 0 {
        let _ = sys_write(3, "\n".as_ptr(), 1);
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

fn cmp_bytes(a: &[u8], b: &[u8]) -> core::cmp::Ordering {
    let len = core::cmp::min(a.len(), b.len());
    let mut i = 0usize;
    while i < len {
        if a[i] != b[i] {
            return a[i].cmp(&b[i]);
        }
        i += 1;
    }
    a.len().cmp(&b.len())
}

fn is_dir_entry(base: &[u8], name: &[u8]) -> bool {
    let mut path = Vec::new();
    if base.is_empty() || base == b"/" {
        path.push(b'/');
    } else {
        path.extend_from_slice(base);
        if !base.ends_with(b"/") {
            path.push(b'/');
        }
    }
    path.extend_from_slice(name);
    path.push(0);

    let mut st = user_lib::Stat {
        st_mode: 0,
        st_size: 0,
        st_type: 0,
    };
    let ret = user_lib::sys_stat(path.as_ptr(), &mut st as *mut _);
    if ret < 0 {
        return false;
    }
    st.st_type == 1
}
