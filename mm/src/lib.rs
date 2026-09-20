#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod addr;
pub mod error;
pub mod frame_allocator;
pub mod heap_allocator;
pub mod mapper;
pub mod memory_set;
pub mod page_table;
pub mod rmap;

pub use addr::{PhysAddr, VirtAddr};
pub use error::{ErrorKind, MmError};
pub use frame_allocator::reset_stats as reset_frame_stats;
pub use frame_allocator::{
    compact_now, frag_stats, migrate_one_user_page, stats as frame_stats, FrameAllocatorStats,
    PmmFragStats,
};
pub use heap_allocator::{heap_stats, reset_heap_stats, HeapStats};
pub use mapper::Mapper;
pub use memory_set::{reset_vma_stats, vma_stats, MemoryArea, MemorySet, VmaStats, VmaTreeStats};
pub use page_table::{PageTable, PageTableEntry, PageTableFlags};
pub use rmap::{
    migration_read_lock, migration_write_lock, reset_rmap_stats, rmap_add, rmap_any, rmap_count,
    rmap_lookup, rmap_remove, rmap_stats, RmapEntry, RmapStats,
};

use core::slice;
use limine::{File, HhdmRequest, MemmapRequest, ModuleRequest, NonNullPtr};
use spin::Once;
use x86_64::registers::control::Cr3;

#[used]
pub static HHDM_REQUEST: HhdmRequest = HhdmRequest::new(0);
#[used]
pub static MEMMAP_REQUEST: MemmapRequest = MemmapRequest::new(0);
#[used]
pub static MODULE_REQUEST: ModuleRequest = ModuleRequest::new(0);

pub static PHYS_OFFSET: Once<u64> = Once::new();
pub static KERNEL_P4_PHYS: Once<u64> = Once::new();

pub type TlbShootdownHook = fn(u64);
static TLB_SHOOTDOWN_HOOK: Once<TlbShootdownHook> = Once::new();

pub fn set_tlb_shootdown_hook(hook: TlbShootdownHook) {
    let _ = TLB_SHOOTDOWN_HOOK.call_once(|| hook);
}

pub(crate) fn notify_tlb_shootdown(p4_phys: u64) {
    if let Some(h) = TLB_SHOOTDOWN_HOOK.get() {
        h(p4_phys);
    }
}

pub fn get_modules() -> &'static [NonNullPtr<File>] {
    if let Some(resp) = MODULE_REQUEST.get_response().get() {
        unsafe { core::slice::from_raw_parts(resp.modules.as_ptr(), resp.module_count as usize) }
    } else {
        &[]
    }
}

pub fn init() {
    // 1. Get HHDM offset
    let phys_offset = if let Some(hhdm_resp) = HHDM_REQUEST.get_response().get() {
        *PHYS_OFFSET.call_once(|| hhdm_resp.offset)
    } else {
        panic!("Failed to get HHDM response from Limine");
    };
    logger::info!("HHDM offset: {:#x}", phys_offset);

    // 2. Initialize Frame Allocator
    if let Some(memmap_resp) = MEMMAP_REQUEST.get_response().get() {
        unsafe {
            // Limine provides an array of NonNullPtr<MemmapEntry>.
            // Pass the raw pointer slice into the frame allocator.
            let entries = slice::from_raw_parts(
                memmap_resp.entries.as_ptr(),
                memmap_resp.entry_count as usize,
            );
            frame_allocator::init(entries);
        }
    } else {
        panic!("Failed to get Memory Map from Limine");
    }

    // 3. Initialize shared kernel P4 entries (high half)
    init_kernel_shared_p4();

    // 4. Initialize Heap
    // Get P4 table
    let (level_4_table_frame, _) = Cr3::read();
    let p4_phys = PhysAddr::new(level_4_table_frame.start_address().as_u64());
    let _ = KERNEL_P4_PHYS.call_once(|| p4_phys.as_u64());
    let p4_virt = VirtAddr::new(p4_phys.as_u64() + phys_offset);
    let p4_table = unsafe { &mut *p4_virt.as_mut_ptr::<PageTable>() };
    let hhdm_p4 = VirtAddr::new(phys_offset).p4_index();
    logger::info!(
        "HHDM P4 entry {} present={} addr={:#x}",
        hhdm_p4,
        p4_table[hhdm_p4].flags().contains(PageTableFlags::PRESENT),
        p4_table[hhdm_p4].addr().as_u64()
    );
    logger::info!(
        "CR3 P4 phys={:#x} virt={:#x}",
        p4_phys.as_u64(),
        p4_virt.as_u64()
    );

    let mut mapper = unsafe { mapper::OffsetMapper::new(p4_table, phys_offset, p4_phys) };

    if let Err(_e) = heap_allocator::init_heap(&mut mapper) {
        panic!("Failed to initialize kernel heap");
    }
}

fn init_kernel_shared_p4() {
    let phys_offset = match PHYS_OFFSET.get() {
        Some(v) => *v,
        None => panic!("PHYS_OFFSET not initialized"),
    };

    let (level_4_table_frame, _) = Cr3::read();
    let p4_phys = PhysAddr::new(level_4_table_frame.start_address().as_u64());
    let p4_virt = VirtAddr::new(p4_phys.as_u64() + phys_offset);
    let p4_table = unsafe { &mut *p4_virt.as_mut_ptr::<PageTable>() };

    for i in 256..512 {
        if p4_table[i].flags().contains(PageTableFlags::PRESENT) {
            continue;
        }
        let frame = frame_allocator::allocate_frame()
            .expect("PMM: failed to allocate P3 table for shared kernel mappings");
        let phys = PhysAddr::new(frame.start_address().as_u64());
        let virt = VirtAddr::new(phys.as_u64() + phys_offset);
        let table = unsafe { &mut *virt.as_mut_ptr::<PageTable>() };
        table.zero();
        p4_table[i].set_addr(
            phys,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
        );
    }
}
