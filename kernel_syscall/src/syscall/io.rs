use core::mem::size_of;
use core::slice;
use core::sync::atomic::{AtomicUsize, Ordering};

use kernel_driver_hub::driver_hub::device;
use kernel_driver_hub::driver_hub::terminal;
use kernel_task::task;

use super::user_ptr::validate_user_range;

static SYS_READ_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
const SYS_READ_LOG_LIMIT: usize = 16;

pub(super) fn sys_read(fd: usize, buf: usize, len: usize) -> isize {
    if len == 0 {
        return 0;
    }
    if len == 0 {
        return 0;
    }
    if !validate_user_range(buf, len, true) {
        return -1;
    }
    let out = unsafe { slice::from_raw_parts_mut(buf as *mut u8, len) };
    if let Some(cur) = task::current_task() {
        if let Some(handle) = cur.fds.lock().get_fd(fd) {
            if let Ok(n) = handle.read(out) {
                return n as isize;
            }
        }
    }

    if fd != 0 {
        return -1;
    }

    let dev = device::stdin_device();
    let irq_active = kernel_driver_hub::driver_hub::keyboard::irq_input_active();
    let log_idx = SYS_READ_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if log_idx < SYS_READ_LOG_LIMIT {
        logger::println!(
            "[KBD][READ] enter #{} pid={} len={} irq_active={}",
            log_idx + 1,
            task::current_task().map(|t| t.pid).unwrap_or(0),
            len,
            irq_active
        );
    }
    x86_64::instructions::interrupts::enable();

    loop {
        if kernel_driver_hub::driver_hub::keyboard::irq_input_active() {
            let n = dev.read(out);
            if log_idx < SYS_READ_LOG_LIMIT {
                logger::println!(
                    "[KBD][READ] wake #{} pid={} n={} src=kbd_waitq",
                    log_idx + 1,
                    task::current_task().map(|t| t.pid).unwrap_or(0),
                    n
                );
            }
            return n as isize;
        }

        let n = kernel_driver_hub::driver_hub::keyboard::poll_hardware_nonblock(out);
        if n > 0 {
            if log_idx < SYS_READ_LOG_LIMIT {
                logger::println!(
                    "[KBD][READ] wake #{} pid={} n={} src=ps2poll",
                    log_idx + 1,
                    task::current_task().map(|t| t.pid).unwrap_or(0),
                    n
                );
            }
            return n as isize;
        }

        // Fallback path: when keyboard IRQ is not delivered, avoid infinite sleep.
        task::suspend_current_and_run_next();
    }
}

pub(super) fn sys_write(fd: usize, buf: usize, len: usize) -> isize {
    if len == 0 {
        return 0;
    }

    if !validate_user_range(buf, len, false) {
        return -1;
    }

    let bytes = unsafe { slice::from_raw_parts(buf as *const u8, len) };
    if let Some(cur) = task::current_task() {
        if let Some(handle) = cur.fds.lock().get_fd(fd) {
            if let Ok(n) = handle.write(bytes) {
                return n as isize;
            }
        }
    }

    if fd != 1 && fd != 2 && fd != 3 {
        return -1;
    }
    if fd == 3 {
        terminal::write_bytes(bytes);
    } else {
        serial::write_bytes(bytes);
    }
    len as isize
}

pub(super) fn sys_poll(fds: usize, nfds: usize, timeout: isize) -> isize {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    const POLLIN: i16 = 0x0001;
    const POLLOUT: i16 = 0x0004;
    const POLLNVAL: i16 = 0x0020;

    #[inline(always)]
    fn poll_revents(fd: i32, events: i16) -> i16 {
        match fd {
            0 => {
                if (events & POLLIN) != 0 && device::stdin_device().poll() {
                    POLLIN
                } else {
                    0
                }
            }
            1 | 2 | 3 => {
                if (events & POLLOUT) != 0 {
                    POLLOUT
                } else {
                    0
                }
            }
            _ => POLLNVAL,
        }
    }

    if timeout != 0 {
        // Minimal semantics for now: only timeout=0 non-blocking probe.
        return -1;
    }

    if nfds == 0 {
        return 0;
    }

    let bytes = match nfds.checked_mul(size_of::<PollFd>()) {
        Some(v) => v,
        None => return -1,
    };
    if !validate_user_range(fds, bytes, false) || !validate_user_range(fds, bytes, true) {
        return -1;
    }

    let mut ready = 0usize;
    for i in 0..nfds {
        let p = unsafe { (fds as *mut PollFd).add(i) };
        let mut entry = unsafe { core::ptr::read_unaligned(p) };
        let mut revents = 0i16;
        if let Some(cur) = task::current_task() {
            if let Some(handle) = cur.fds.lock().get_fd(entry.fd as usize) {
                if let Ok(mask) = handle.node.poll(entry.events as u32) {
                    if mask != 0 {
                        revents = entry.events;
                    }
                }
            } else {
                revents = poll_revents(entry.fd, entry.events);
            }
        } else {
            revents = poll_revents(entry.fd, entry.events);
        }
        entry.revents = revents;
        if entry.revents != 0 {
            ready += 1;
        }
        unsafe {
            core::ptr::write_unaligned(p, entry);
        }
    }

    ready as isize
}

pub(super) fn sys_select(
    nfds: usize,
    readfds: usize,
    writefds: usize,
    exceptfds: usize,
    timeout: usize,
) -> isize {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TimeVal {
        tv_sec: isize,
        tv_usec: isize,
    }

    #[inline(always)]
    fn valid_fdset_ptr(ptr: usize) -> bool {
        validate_user_range(ptr, size_of::<usize>(), false)
            && validate_user_range(ptr, size_of::<usize>(), true)
    }

    // Minimal semantics for now: support non-blocking probe only.
    if timeout != 0 {
        if !validate_user_range(timeout, size_of::<TimeVal>(), false) {
            return -1;
        }
        let tv = unsafe { core::ptr::read_unaligned(timeout as *const TimeVal) };
        if tv.tv_sec != 0 || tv.tv_usec != 0 {
            return -1;
        }
    }

    if nfds == 0 {
        return 0;
    }

    let bits_per_word = usize::BITS as usize;
    if nfds > bits_per_word {
        // Current minimal implementation only handles one fd_set word.
        return -1;
    }
    let mask = if nfds == bits_per_word {
        usize::MAX
    } else {
        (1usize << nfds) - 1
    };

    let stdin_ready = device::stdin_device().poll();
    const WRITABLE_FDS: usize = (1usize << 1) | (1usize << 2) | (1usize << 3);

    let mut ready_mask = 0usize;

    if readfds != 0 {
        if !valid_fdset_ptr(readfds) {
            return -1;
        }
        let ptr = readfds as *mut usize;
        let in_mask = unsafe { core::ptr::read_unaligned(ptr) } & mask;
        let mut out_mask = 0usize;
        if stdin_ready && (in_mask & 1usize) != 0 {
            out_mask |= 1usize;
        }
        ready_mask |= out_mask;
        unsafe {
            core::ptr::write_unaligned(ptr, out_mask);
        }
    }

    if writefds != 0 {
        if !valid_fdset_ptr(writefds) {
            return -1;
        }
        let ptr = writefds as *mut usize;
        let in_mask = unsafe { core::ptr::read_unaligned(ptr) } & mask;
        let out_mask = in_mask & WRITABLE_FDS & mask;
        ready_mask |= out_mask;
        unsafe {
            core::ptr::write_unaligned(ptr, out_mask);
        }
    }

    if exceptfds != 0 {
        if !valid_fdset_ptr(exceptfds) {
            return -1;
        }
        unsafe {
            core::ptr::write_unaligned(exceptfds as *mut usize, 0usize);
        }
    }

    ready_mask.count_ones() as isize
}
