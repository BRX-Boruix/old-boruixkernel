#![allow(dead_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use user_lib::{sys_read, sys_write};
mod commands;
use commands::all_commands;

pub fn run_shell() -> ! {
    let mut input_buf = [0u8; 64];
    let mut line_buf: Vec<u8> = Vec::new();
    let mut prompt_shown = false;

    loop {
        if line_buf.is_empty() && !prompt_shown {
            let _ = sys_write(3, "> ".as_ptr(), 2);
            prompt_shown = true;
        }

        let n = sys_read(0, input_buf.as_mut_ptr(), input_buf.len());
        if n <= 0 {
            continue;
        }

        let n = n as usize;
        for &b in &input_buf[..n] {
            if b == b'\r' {
                continue;
            }
            if b == b'\n' {
                let _ = sys_write(3, "\n".as_ptr(), 1);
                if !line_buf.is_empty() {
                    handle_command(&line_buf);
                }
                line_buf.clear();
                prompt_shown = false;
            } else if b == 0x08 {
                if !line_buf.is_empty() {
                    line_buf.pop();
                    let _ = sys_write(3, "\x08 \x08".as_ptr(), 3);
                }
            } else {
                line_buf.push(b);
                let _ = sys_write(3, &b as *const u8, 1);
            }
        }
    }
}

fn handle_command(line: &[u8]) {
    let mut args: Vec<&[u8]> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;

    while i <= line.len() {
        let end = i == line.len();
        if end || line[i] == b' ' || line[i] == b'\t' || line[i] == b'\r' {
            if i > start {
                args.push(&line[start..i]);
            }
            start = i + 1;
        }
        i += 1;
    }

    if args.is_empty() {
        return;
    }

    let cmd = args[0];
    dispatch(cmd, &args);
}

fn dispatch(cmd: &[u8], args: &[&[u8]]) {
    let list = all_commands();
    for c in list {
        if cmd_eq(cmd, c.name) {
            if args.len() > 1 && (cmd_eq(args[1], b"-h") || cmd_eq(args[1], b"--help")) {
                println3(c.usage);
                println3(c.desc);
                return;
            }
            let (cmd_args, redirect) = parse_redirect(args);
            if let Some(path) = redirect {
                if let Some(fd) = open_redirect(path) {
                    user_lib::stdout_redirect_set(fd as isize);
                    (c.run)(cmd_args);
                    user_lib::stdout_redirect_clear();
                    let _ = user_lib::sys_close(fd);
                } else {
                    println3("redirection failed");
                }
            } else {
                (c.run)(cmd_args);
            }
            return;
        }
    }
    println3("unknown command");
}

fn cmd_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] { return false; }
        i += 1;
    }
    true
}

fn parse_redirect<'a>(args: &'a [&'a [u8]]) -> (&'a [&'a [u8]], Option<&'a [u8]>) {
    let mut i = 0usize;
    while i < args.len() {
        if cmd_eq(args[i], b">") {
            if i + 1 < args.len() {
                let mut tmp: Vec<&[u8]> = Vec::new();
                for (idx, a) in args.iter().enumerate() {
                    if idx == i || idx == i + 1 {
                        continue;
                    }
                    tmp.push(*a);
                }
                // Leak to keep lifetime simple in this no_std environment.
                let leaked: &'a [&'a [u8]] = Box::leak(tmp.into_boxed_slice());
                return (leaked, Some(args[i + 1]));
            }
            break;
        }
        i += 1;
    }
    (args, None)
}

fn open_redirect(path: &[u8]) -> Option<usize> {
    let path_c = cstr_from_bytes(path);
    let is_dev = path.starts_with(b"/dev/");
    if is_dev {
        let fd = user_lib::sys_open(path_c.as_ptr(), 0, 0);
        if fd < 0 {
            return None;
        }
        return Some(fd as usize);
    }
    // best-effort truncate: unlink then create
    let _ = user_lib::sys_unlink(path_c.as_ptr());
    let fd = user_lib::sys_open(path_c.as_ptr(), user_lib::O_CREAT, 0);
    if fd < 0 {
        return None;
    }
    Some(fd as usize)
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
