#![allow(dead_code)]

extern crate alloc;

use linked_list_allocator::LockedHeap;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::{sys_write, sys_sbrk};

struct GrowableHeap {
    inner: LockedHeap,
    heap_end: AtomicUsize,
}

impl GrowableHeap {
    const fn new() -> Self {
        Self {
            inner: LockedHeap::empty(),
            heap_end: AtomicUsize::new(0),
        }
    }

    unsafe fn init(&self, start: usize, end: usize) {
        self.inner.lock().init(start as *mut u8, end - start);
        self.heap_end.store(end, Ordering::Relaxed);
    }

    unsafe fn grow(&self, min_bytes: usize) -> bool {
        let grow = align_up(core::cmp::max(min_bytes, 1024 * 1024), 4096);
        let old = sys_sbrk(grow as isize);
        if old < 0 {
            return false;
        }
        self.inner.lock().extend(grow);
        self.heap_end.fetch_add(grow, Ordering::Relaxed);
        SBRK_COUNT.fetch_add(1, Ordering::Relaxed);
        SBRK_BYTES.fetch_add(grow, Ordering::Relaxed);
        true
    }
}

#[global_allocator]
static ALLOCATOR: GrowableHeap = GrowableHeap::new();

static SBRK_COUNT: AtomicUsize = AtomicUsize::new(0);
static SBRK_BYTES: AtomicUsize = AtomicUsize::new(0);

pub unsafe fn init() {
    extern "C" {
        static __heap_start: u8;
        static __heap_end: u8;
    }
    let start = core::ptr::addr_of!(__heap_start) as usize;
    let end = core::ptr::addr_of!(__heap_end) as usize;
    if end > start {
        ALLOCATOR.init(start, end);
    }
}

#[alloc_error_handler]
fn alloc_error(_layout: Layout) -> ! {
    let _ = sys_write(2, "User OOM\n".as_ptr(), 9);
    crate::sys_exit(12);
}

#[inline(always)]
const fn align_up(x: usize, align: usize) -> usize {
    (x + align - 1) & !(align - 1)
}

unsafe impl GlobalAlloc for GrowableHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.dangling_ptr().as_ptr();
        }
        match self.inner.lock().allocate_first_fit(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(()) => {
                if self.grow(layout.size()) {
                    match self.inner.lock().allocate_first_fit(layout) {
                        Ok(ptr) => ptr.as_ptr(),
                        Err(()) => ptr::null_mut(),
                    }
                } else {
                    ptr::null_mut()
                }
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if layout.size() == 0 {
            return;
        }
        if let Some(nn) = NonNull::new(ptr) {
            self.inner.lock().deallocate(nn, layout);
        }
    }
}

pub fn heap_size() -> usize {
    extern "C" {
        static __heap_start: u8;
        static __heap_end: u8;
    }
    let start = core::ptr::addr_of!(__heap_start) as usize;
    let end = core::ptr::addr_of!(__heap_end) as usize;
    end.saturating_sub(start)
}

pub fn sbrk_stats() -> (usize, usize) {
    (
        SBRK_COUNT.load(Ordering::Relaxed),
        SBRK_BYTES.load(Ordering::Relaxed),
    )
}
