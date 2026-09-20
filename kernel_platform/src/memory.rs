use limine::{File, NonNullPtr};

pub use crate::hal::mm::set_tlb_shootdown_hook;

pub fn init() {
    crate::hal::mm::init();
}

pub fn get_modules() -> &'static [NonNullPtr<File>] {
    crate::hal::mm::get_modules()
}

pub mod addr_space {
    pub use crate::hal::mm::{
        reset_vma_stats, vma_stats, ErrorKind, MemoryArea, MemorySet, MmError, PageTableFlags,
        PhysAddr, VirtAddr, VmaTreeStats, PHYS_OFFSET,
    };
}

pub mod mapper {
    pub use crate::hal::mm::{Mapper, PageTable, PageTableEntry};
}

pub mod pmm {
    pub use crate::hal::mm::frame_allocator;
    pub use crate::hal::mm::{
        compact_now, frag_stats, frame_stats, migrate_one_user_page, reset_frame_stats,
    };
}

pub mod heap {
    pub use crate::hal::mm::{heap_stats, reset_heap_stats};
}

pub mod rmap {
    pub use crate::hal::mm::{reset_rmap_stats, rmap_count, rmap_stats};
}
