use kernel_driver_hub::driver_hub;

use super::user_ptr::validate_user_range;

pub(super) fn sys_driverhub(buf: *mut u8, len: usize, flags: usize) -> isize {
    if buf.is_null() || len == 0 {
        return -1;
    }
    if !validate_user_range(buf as usize, len, true) {
        return -1;
    }

    let out = driver_hub::summary_string(flags as u32);
    let bytes = out.as_bytes();
    let n = core::cmp::min(bytes.len(), len);
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n);
    }
    n as isize
}

pub(super) fn sys_driverhub_drivers(buf: *mut driver_hub::DriverInfoRaw, len: usize) -> isize {
    if buf.is_null() || len == 0 {
        return -1;
    }
    let bytes = len * core::mem::size_of::<driver_hub::DriverInfoRaw>();
    if !validate_user_range(buf as usize, bytes, true) {
        return -1;
    }
    let out = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    driver_hub::fill_driver_list(out) as isize
}

pub(super) fn sys_driverhub_devices(buf: *mut driver_hub::DeviceInfoRaw, len: usize) -> isize {
    if buf.is_null() || len == 0 {
        return -1;
    }
    let bytes = len * core::mem::size_of::<driver_hub::DeviceInfoRaw>();
    if !validate_user_range(buf as usize, bytes, true) {
        return -1;
    }
    let out = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    driver_hub::fill_device_list(out) as isize
}
