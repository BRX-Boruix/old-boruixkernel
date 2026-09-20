mod allocator_core;
mod compact;
mod percpu_cache;
mod stats;

use limine::{MemmapEntry, NonNullPtr};
use spin::Once;
use x86_64::instructions::interrupts;
use x86_64::structures::paging::PhysFrame;

use allocator_core::{LazyBuddyAllocator, ORDER_4K};
use percpu_cache::{FreeListTable, PerCpuCacheSet};

pub use allocator_core::{ORDER_1G, ORDER_2M};
pub use compact::compact_now;
pub use stats::reset_stats as reset_frame_stats;
pub use stats::{frag_stats, reset_stats, stats, FrameAllocatorStats, PmmFragStats};

pub(crate) use allocator_core::MAX_CPUS;

// Global allocator instance
static ALLOCATOR: LazyBuddyAllocator = LazyBuddyAllocator::new();
static FREE_LISTS: Once<FreeListTable> = Once::new();
static PER_CPU: PerCpuCacheSet = PerCpuCacheSet::new();

pub fn current_cpu_id() -> usize {
    let id = arch::syscall::current_cpu_id_raw();
    if id >= MAX_CPUS {
        logger::warn!("PMM: cpu_id {} out of range, clamping to 0", id);
        0
    } else {
        id
    }
}

unsafe impl Send for LazyBuddyAllocator {}
unsafe impl Sync for LazyBuddyAllocator {}

/// Initialize the global allocator
pub unsafe fn init(mmap: &[NonNullPtr<MemmapEntry>]) {
    interrupts::without_interrupts(|| {
        ALLOCATOR.init(mmap);
    });
}

/// Allocate a physical frame
pub fn allocate_frame() -> Option<PhysFrame> {
    allocate_frames(ORDER_4K)
}

/// Allocate physical frames with specific order
pub fn allocate_frames(order: usize) -> Option<PhysFrame> {
    interrupts::without_interrupts(|| ALLOCATOR.allocate(order))
}

pub fn migrate_one_user_page() -> bool {
    ALLOCATOR.migrate_one_user_page()
}

/// Deallocate a physical frame
pub fn deallocate_frame(frame: PhysFrame) {
    interrupts::without_interrupts(|| {
        ALLOCATOR.deallocate(frame);
    });
}
