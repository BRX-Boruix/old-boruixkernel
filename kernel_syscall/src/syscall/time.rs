use core::cmp::min;
use core::convert::TryInto;

use kernel_driver_hub::driver_hub::cmos;

use super::user_ptr::validate_user_range;

pub(super) fn sys_time_human(buf: *mut u8, len: usize) -> isize {
    if buf.is_null() || len == 0 {
        return -1;
    }
    if !validate_user_range(buf as usize, len, true) {
        return -1;
    }

    let time = match cmos::read_rtc_time() {
        Some(t) => t,
        None => return -1,
    };
    let human = cmos::format_human(&time);
    let bytes = human.as_bytes();
    let to_copy = min(bytes.len(), len);
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, to_copy);
    }
    to_copy as isize
}

pub(super) fn sys_time_seconds() -> isize {
    let time = match cmos::read_rtc_time() {
        Some(t) => t,
        None => return -1,
    };
    let secs = cmos::unix_seconds(&time);
    match secs.try_into() {
        Ok(v) => v,
        Err(_) => -1,
    }
}
