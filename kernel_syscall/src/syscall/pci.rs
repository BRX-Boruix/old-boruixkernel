use core::mem;
use core::slice;

use super::user_ptr::validate_user_range;
use kernel_driver_hub::driver_hub::pci::{self, PciDeviceInfo};

pub(super) fn sys_pci_list(buf: *mut PciDeviceInfo, capacity: usize) -> isize {
    if capacity == 0 {
        return pci::device_count() as isize;
    }
    if buf.is_null() {
        return -1;
    }
    let buf_size = match capacity.checked_mul(mem::size_of::<PciDeviceInfo>()) {
        Some(v) => v,
        None => return -1,
    };
    if !validate_user_range(buf as usize, buf_size, true) {
        return -1;
    }
    let slice = unsafe { slice::from_raw_parts_mut(buf, capacity) };
    pci::fill_device_list(slice) as isize
}

pub(super) fn sys_pci_info(bus: u8, device: u8, function: u8, out: *mut PciDeviceInfo) -> isize {
    if out.is_null() {
        return -1;
    }
    if !validate_user_range(out as usize, mem::size_of::<PciDeviceInfo>(), true) {
        return -1;
    }
    match pci::device_by_address(bus, device, function) {
        Some(info) => {
            unsafe {
                core::ptr::write(out, info);
            }
            0
        }
        None => -1,
    }
}

pub(super) fn sys_pci_mode() -> isize {
    pci::mode() as isize
}

pub(super) fn sys_pci_count() -> isize {
    pci::device_count() as isize
}
