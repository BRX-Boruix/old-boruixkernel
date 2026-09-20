use core::slice;
use core::str;

use alloc::string::String;

use kernel_driver_hub::driver_hub::terminal;
use kernel_task::task::{self, TaskStatus};

use super::user_ptr::validate_user_range;

pub(super) fn sys_ps(buf: *mut u8, len: usize) -> isize {
    if buf.is_null() || len == 0 {
        return -1;
    }
    if !validate_user_range(buf as usize, len, true) {
        return -1;
    }

    let list = task::list_tasks();
    let mut out_idx = 0usize;

    for (pid, status) in list {
        let status_str = match status {
            TaskStatus::Ready => "READY",
            TaskStatus::Running => "RUN",
            TaskStatus::Exited => "EXIT",
            TaskStatus::Waiting => "WAIT",
        };

        let mut line = [0u8; 32];
        let mut i = 0usize;
        // pid
        let mut tmp = [0u8; 20];
        let mut t = 0usize;
        let mut x = pid;
        if x == 0 {
            tmp[t] = b'0';
            t += 1;
        } else {
            while x > 0 {
                tmp[t] = b'0' + (x % 10) as u8;
                t += 1;
                x /= 10;
            }
        }
        while t > 0 {
            t -= 1;
            line[i] = tmp[t];
            i += 1;
        }
        line[i] = b' ';
        i += 1;
        for &b in status_str.as_bytes() {
            line[i] = b;
            i += 1;
        }
        line[i] = b'\n';
        i += 1;

        if out_idx + i > len {
            break;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(line.as_ptr(), buf.add(out_idx), i);
        }
        out_idx += i;
    }
    out_idx as isize
}

pub(super) fn sys_kill(pid: usize, code: i32) -> isize {
    if pid == 0 {
        return -1;
    }
    if task::kill_task(pid, code) {
        0
    } else {
        -1
    }
}

pub(super) fn sys_plog(ty: usize, buf: *const u8, len: usize) -> isize {
    if len == 0 {
        return 0;
    }
    let max_len = if len > 1024 { 1024 } else { len };
    if !validate_user_range(buf as usize, max_len, false) {
        return -1;
    }
    let bytes = unsafe { slice::from_raw_parts(buf, max_len) };
    let msg = str::from_utf8(bytes).unwrap_or("<invalid utf8>");
    let all = (ty & (1 << 16)) != 0;
    let ty = ty & 0xffff;
    match ty {
        0 => logger::info!("{}", msg),
        1 => logger::warn!("{}", msg),
        2 => logger::error!("{}", msg),
        3 => logger::tip!("{}", msg),
        4 => logger::println!("{}", msg),
        _ => logger::warn!("plog: unknown type {}", ty),
    }

    if all {
        let mut out = String::new();
        match ty {
            0 => out.push_str("\x1b[36m[INFO]\x1b[0m "),
            1 => out.push_str("\x1b[33m[WARN]\x1b[0m "),
            2 => out.push_str("\x1b[31m[ERROR]\x1b[0m "),
            3 => out.push_str("\x1b[32m[TIP]\x1b[0m "),
            _ => {}
        }
        out.push_str(msg);
        out.push('\n');
        terminal::write_bytes(out.as_bytes());
    }
    max_len as isize
}

pub(super) fn sys_ipi_stat(buf: *mut u8, len: usize) -> isize {
    if buf.is_null() || len == 0 {
        return -1;
    }
    let s = kernel_task::smp::ipi_stat_string();
    let bytes = s.as_bytes();
    let n = core::cmp::min(bytes.len(), len);
    if !validate_user_range(buf as usize, n, true) {
        return -1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n);
    }
    n as isize
}
