use core::slice;
use core::str;

use alloc::string::String;

use kernel_platform::memory as mem;
use kernel_task::task;
use mem::addr_space::PageTableFlags;

pub(super) fn validate_user_range(buf: usize, len: usize, write: bool) -> bool {
    const USER_RANGE_END: usize = 0x0000_8000_0000_0000;
    if len == 0 {
        return true;
    }
    if buf >= USER_RANGE_END {
        return false;
    }
    let end = match buf.checked_add(len) {
        Some(v) => v,
        None => return false,
    };
    if end > USER_RANGE_END {
        return false;
    }

    let current_task = match task::current_task() {
        Some(t) => t,
        None => return false,
    };

    let required = if write {
        PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE
    } else {
        PageTableFlags::USER_ACCESSIBLE
    };
    let memory_set = unsafe { &*current_task.memory_set.get() };
    memory_set.check_range_mapped(mem::addr_space::VirtAddr::new(buf as u64), len, required)
}

pub(super) fn read_cstring_from_user(ptr: *const u8, max_len: usize) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    unsafe {
        while len < max_len {
            let addr = ptr.add(len) as usize;
            if !validate_user_range(addr, 1, false) {
                return None;
            }
            if *ptr.add(len) == 0 {
                break;
            }
            len += 1;
        }
    }
    if len == 0 {
        return Some(String::new());
    }
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    str::from_utf8(bytes).ok().map(String::from)
}
