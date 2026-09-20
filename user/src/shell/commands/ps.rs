use super::Command;
use user_lib::{sys_ps, sys_write};

const COMMAND_ABOUT: &str = "ps\n\nList processes (friendly view).\nUsage: ps";

pub const CMD: Command = Command {
    name: b"ps",
    usage: "ps",
    desc: "List processes (friendly)",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(_args: &[&[u8]]) {
    let mut buf = [0u8; 256];
    let n = sys_ps(buf.as_mut_ptr(), buf.len());
    if n > 0 {
        print_header();
        format_and_print(&buf[..n as usize]);
    }
}

fn print_header() {
    let _ = sys_write(3, "PID   STATE\n".as_ptr(), 12);
    let _ = sys_write(3, "----- -----\n".as_ptr(), 12);
}

fn format_and_print(data: &[u8]) {
    let mut i = 0usize;
    while i < data.len() {
        // parse pid
        let mut pid: usize = 0;
        let mut has_pid = false;
        while i < data.len() && data[i] >= b'0' && data[i] <= b'9' {
            pid = pid * 10 + (data[i] - b'0') as usize;
            has_pid = true;
            i += 1;
        }
        if !has_pid {
            // skip to next line
            while i < data.len() && data[i] != b'\n' { i += 1; }
            if i < data.len() { i += 1; }
            continue;
        }

        // skip spaces
        while i < data.len() && data[i] == b' ' { i += 1; }

        // read status token
        let status_start = i;
        while i < data.len() && data[i] != b'\n' { i += 1; }
        let status = &data[status_start..i];
        if i < data.len() { i += 1; }

        print_row(pid, status);
    }
}

fn print_row(pid: usize, status: &[u8]) {
    let mut pid_buf = [b' '; 5];
    let mut tmp = [0u8; 10];
    let mut t = 0usize;
    let mut x = pid;
    if x == 0 {
        tmp[t] = b'0';
        t += 1;
    } else {
        while x > 0 && t < tmp.len() {
            tmp[t] = b'0' + (x % 10) as u8;
            t += 1;
            x /= 10;
        }
    }
    let mut idx = 5;
    while t > 0 {
        t -= 1;
        if idx == 0 { break; }
        idx -= 1;
        pid_buf[idx] = tmp[t];
    }

    let _ = sys_write(3, pid_buf.as_ptr(), pid_buf.len());
    let _ = sys_write(3, " ".as_ptr(), 1);
    let _ = sys_write(3, status.as_ptr(), status.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}
