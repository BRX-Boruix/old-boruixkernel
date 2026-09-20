use core::alloc::{GlobalAlloc, Layout};
use core::cmp::max;
use core::mem::{align_of, size_of};
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

use buddy_system_allocator::linked_list::LinkedList;
use buddy_system_allocator::{Heap, LockedHeap};

use crate::addr::{PhysAddr, VirtAddr};
use crate::frame_allocator;
use crate::mapper::{MapError, Mapper};
use crate::page_table::PageTableFlags;

const HEAP_SHARDS: usize = 8;
const HEAP_GLOBAL_RATIO_NUM: usize = 1;
const HEAP_GLOBAL_RATIO_DEN: usize = 2; // global heap takes 1/2
const HEAP_GROW_CHUNK: usize = 2 * 1024 * 1024; // grow by 2MiB

#[repr(C)]
struct AllocHeader {
    magic: u32,
    shard: u16,
    _pad: u16,
    base: usize,
    size: u32,
    align: u32,
}

const HEADER_MAGIC: u32 = 0x4D4D4845; // "MMHE"

struct ShardedHeap {
    shards: [LockedHeap<32>; HEAP_SHARDS],
    global: LockedHeap<32>,
    alloc_calls: AtomicUsize,
    alloc_fallback_global: AtomicUsize,
    alloc_fallback_other: AtomicUsize,
    alloc_fail: AtomicUsize,
    dealloc_calls: AtomicUsize,
    grow_attempts: AtomicUsize,
    grow_success: AtomicUsize,
    heap_start: AtomicUsize,
    heap_end: AtomicUsize,
    heap_max: AtomicUsize,
}

impl ShardedHeap {
    const fn new() -> Self {
        Self {
            shards: [
                LockedHeap::empty(),
                LockedHeap::empty(),
                LockedHeap::empty(),
                LockedHeap::empty(),
                LockedHeap::empty(),
                LockedHeap::empty(),
                LockedHeap::empty(),
                LockedHeap::empty(),
            ],
            global: LockedHeap::empty(),
            alloc_calls: AtomicUsize::new(0),
            alloc_fallback_global: AtomicUsize::new(0),
            alloc_fallback_other: AtomicUsize::new(0),
            alloc_fail: AtomicUsize::new(0),
            dealloc_calls: AtomicUsize::new(0),
            grow_attempts: AtomicUsize::new(0),
            grow_success: AtomicUsize::new(0),
            heap_start: AtomicUsize::new(0),
            heap_end: AtomicUsize::new(0),
            heap_max: AtomicUsize::new(0),
        }
    }

    fn local_shard(&self) -> usize {
        frame_allocator::current_cpu_id() % HEAP_SHARDS
    }

    fn alloc_from_heap(heap: &LockedHeap<32>, layout: Layout) -> *mut u8 {
        unsafe { heap.alloc(layout) }
    }

    unsafe fn dealloc_to_heap(heap: &LockedHeap<32>, ptr: *mut u8, layout: Layout) {
        heap.dealloc(ptr, layout);
    }

    fn alloc_with_header(&self, shard: usize, layout: Layout) -> *mut u8 {
        let align = max(layout.align(), align_of::<AllocHeader>());
        let header_size = size_of::<AllocHeader>();
        let size = layout.size();
        let new_size = match size.checked_add(header_size + align) {
            Some(v) => v,
            None => return ptr::null_mut(),
        };
        let new_layout = match Layout::from_size_align(new_size, align) {
            Ok(l) => l,
            Err(_) => return ptr::null_mut(),
        };

        let base = if shard == HEAP_SHARDS {
            Self::alloc_from_heap(&self.global, new_layout)
        } else {
            Self::alloc_from_heap(&self.shards[shard], new_layout)
        };

        if base.is_null() {
            return ptr::null_mut();
        }

        let base_addr = base as usize;
        let aligned = (base_addr + header_size + (align - 1)) & !(align - 1);
        let user_ptr = aligned as *mut u8;
        let header_ptr = (aligned - header_size) as *mut AllocHeader;

        unsafe {
            ptr::write_unaligned(
                header_ptr,
                AllocHeader {
                    magic: HEADER_MAGIC,
                    shard: shard as u16,
                    _pad: 0,
                    base: base_addr,
                    size: new_layout.size() as u32,
                    align: new_layout.align() as u32,
                },
            );
        }

        user_ptr
    }

    unsafe fn dealloc_with_header(&self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }
        let header_size = size_of::<AllocHeader>();
        let header_ptr = ptr.sub(header_size) as *mut AllocHeader;
        let header = ptr::read_unaligned(header_ptr);

        if header.magic != HEADER_MAGIC {
            return;
        }

        let layout = match Layout::from_size_align(header.size as usize, header.align as usize) {
            Ok(l) => l,
            Err(_) => return,
        };

        let base_ptr = header.base as *mut u8;
        if header.shard as usize == HEAP_SHARDS {
            Self::dealloc_to_heap(&self.global, base_ptr, layout);
        } else {
            Self::dealloc_to_heap(&self.shards[header.shard as usize], base_ptr, layout);
        }
    }

    fn grow_heap(&self) -> bool {
        self.grow_attempts.fetch_add(1, Ordering::Relaxed);

        let heap_start = self.heap_start.load(Ordering::Relaxed);
        let heap_end = self.heap_end.load(Ordering::Relaxed);
        let heap_max = self.heap_max.load(Ordering::Relaxed);

        if heap_end >= heap_max {
            return false;
        }

        let grow = if heap_max - heap_end >= HEAP_GROW_CHUNK {
            HEAP_GROW_CHUNK
        } else {
            heap_max - heap_end
        };

        if grow == 0 {
            return false;
        }

        let start = heap_end;
        let end = heap_end + grow;

        let page_range = (start..end).step_by(4096);
        unsafe {
            let (p4, p4_phys) = self.kernel_p4_and_phys();
            let mut mapper = crate::mapper::OffsetMapper::new(p4, self.phys_offset(), p4_phys);
            for page_addr in page_range {
                let page = VirtAddr::new(page_addr as u64);
                let frame = match frame_allocator::allocate_frame() {
                    Some(f) => f,
                    None => return false,
                };
                let phys_addr = PhysAddr::new(frame.start_address().as_u64());
                let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
                if mapper.map_page_noflush(page, phys_addr, flags).is_err() {
                    return false;
                }
            }
            mapper.flush_all();
        }

        // Add new range to global heap only
        unsafe {
            self.global.lock().add_to_heap(start, end);
        }
        self.heap_end.store(end, Ordering::Relaxed);
        self.grow_success.fetch_add(1, Ordering::Relaxed);
        let _ = heap_start; // silence unused if not used later
        true
    }

    fn phys_offset(&self) -> u64 {
        *crate::PHYS_OFFSET
            .get()
            .expect("PHYS_OFFSET not initialized")
    }

    fn kernel_p4_and_phys(&self) -> (&'static mut crate::page_table::PageTable, PhysAddr) {
        use x86_64::registers::control::Cr3;
        let phys_offset = self.phys_offset();
        let (level_4_table_frame, _) = Cr3::read();
        let p4_phys = PhysAddr::new(level_4_table_frame.start_address().as_u64());
        let p4_virt = VirtAddr::new(p4_phys.as_u64() + phys_offset);
        let p4 = unsafe { &mut *p4_virt.as_mut_ptr::<crate::page_table::PageTable>() };
        (p4, p4_phys)
    }

    fn heap_frag_stats(&self) -> (usize, usize) {
        let mut free_blocks = 0usize;
        let mut max_free = 0usize;

        for i in 0..HEAP_SHARDS {
            let heap = self.shards[i].lock();
            let (blocks, max_block) = heap_free_stats(&*heap);
            free_blocks = free_blocks.saturating_add(blocks);
            if max_block > max_free {
                max_free = max_block;
            }
        }

        let heap = self.global.lock();
        let (blocks, max_block) = heap_free_stats(&*heap);
        free_blocks = free_blocks.saturating_add(blocks);
        if max_block > max_free {
            max_free = max_block;
        }

        (free_blocks, max_free)
    }
}

#[repr(C)]
struct HeapLayout<const ORDER: usize> {
    free_list: [LinkedList; ORDER],
    user: usize,
    allocated: usize,
    total: usize,
}

fn heap_free_stats<const ORDER: usize>(heap: &Heap<ORDER>) -> (usize, usize) {
    debug_assert_eq!(
        core::mem::size_of::<HeapLayout<ORDER>>(),
        core::mem::size_of::<Heap<ORDER>>()
    );
    let layout = unsafe { &*(heap as *const Heap<ORDER> as *const HeapLayout<ORDER>) };

    let mut free_blocks = 0usize;
    let mut max_free = 0usize;
    for (i, list) in layout.free_list.iter().enumerate() {
        if !list.is_empty() {
            let size = 1usize << i;
            if size > max_free {
                max_free = size;
            }
        }
        for _ in list.iter() {
            free_blocks = free_blocks.saturating_add(1);
        }
    }
    (free_blocks, max_free)
}

unsafe impl GlobalAlloc for ShardedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.alloc_calls.fetch_add(1, Ordering::Relaxed);

        let shard = self.local_shard();
        let mut ptr = self.alloc_with_header(shard, layout);
        if !ptr.is_null() {
            return ptr;
        }

        self.alloc_fallback_global.fetch_add(1, Ordering::Relaxed);
        ptr = self.alloc_with_header(HEAP_SHARDS, layout);
        if !ptr.is_null() {
            return ptr;
        }

        for other in 0..HEAP_SHARDS {
            if other == shard {
                continue;
            }
            self.alloc_fallback_other.fetch_add(1, Ordering::Relaxed);
            ptr = self.alloc_with_header(other, layout);
            if !ptr.is_null() {
                return ptr;
            }
        }

        if self.grow_heap() {
            ptr = self.alloc_with_header(HEAP_SHARDS, layout);
            if !ptr.is_null() {
                return ptr;
            }
        }

        self.alloc_fail.fetch_add(1, Ordering::Relaxed);
        ptr::null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        self.dealloc_calls.fetch_add(1, Ordering::Relaxed);
        self.dealloc_with_header(ptr);
    }
}

#[global_allocator]
static HEAP_ALLOCATOR: ShardedHeap = ShardedHeap::new();

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

pub fn init_heap(mapper: &mut impl Mapper) -> Result<(), MapError> {
    let heap_start = VirtAddr::new(config::KERNEL_HEAP_START);
    let heap_size = config::KERNEL_HEAP_SIZE;
    let heap_max = config::KERNEL_HEAP_MAX;
    if heap_max < heap_size {
        return Err(MapError::InvalidAccess);
    }
    let heap_end = VirtAddr::new(heap_start.as_u64() + heap_size);

    let page_range = {
        let heap_start_page = heap_start.align_down(4096);
        let heap_end_page = heap_end.align_up(4096);

        let start = heap_start_page.as_u64();
        let end = heap_end_page.as_u64();
        (start..end).step_by(4096)
    };

    logger::info!(
        "HEAP: start={:#x} size={:#x} end={:#x}",
        heap_start.as_u64(),
        heap_size,
        heap_end.as_u64()
    );
    let mut first_page = true;
    for page_addr in page_range {
        let page = VirtAddr::new(page_addr);
        let frame = frame_allocator::allocate_frame().ok_or(MapError::FrameAllocationFailed)?;
        let phys_addr = PhysAddr::new(frame.start_address().as_u64());
        if phys_addr.as_u64() == 0 {
            panic!("HEAP: allocator returned paddr 0");
        }
        if first_page {
            logger::info!(
                "HEAP: first page {:?} -> phys {:#x}",
                page,
                phys_addr.as_u64()
            );
        }
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_page(page, phys_addr, flags)?;
        }
        if first_page {
            validate_heap_mapping(page);
            first_page = false;
        }
    }
    dump_heap_walk(heap_start);

    let global_size = (heap_size as usize * HEAP_GLOBAL_RATIO_NUM) / HEAP_GLOBAL_RATIO_DEN;
    let global_start = heap_start.as_usize();
    let shard_total = heap_size as usize - global_size;
    let per_shard = shard_total / HEAP_SHARDS;

    unsafe {
        HEAP_ALLOCATOR
            .global
            .lock()
            .add_to_heap(global_start, global_start + global_size);
    }

    for i in 0..HEAP_SHARDS {
        let start = global_start + global_size + i * per_shard;
        let size = if i + 1 == HEAP_SHARDS {
            heap_size as usize - (global_size + i * per_shard)
        } else {
            per_shard
        };
        unsafe {
            HEAP_ALLOCATOR.shards[i]
                .lock()
                .add_to_heap(start, start + size);
        }
    }

    let heap_max = config::KERNEL_HEAP_MAX as usize;
    if heap_max < heap_size as usize {
        return Err(MapError::InvalidAccess);
    }
    HEAP_ALLOCATOR
        .heap_start
        .store(global_start, Ordering::Relaxed);
    HEAP_ALLOCATOR
        .heap_end
        .store(global_start + heap_size as usize, Ordering::Relaxed);
    HEAP_ALLOCATOR
        .heap_max
        .store(global_start + heap_max, Ordering::Relaxed);

    Ok(())
}

fn dump_heap_walk(addr: VirtAddr) {
    use crate::PageTable;
    use x86_64::registers::control::Cr3;
    let phys_offset = match crate::PHYS_OFFSET.get() {
        Some(v) => *v,
        None => {
            logger::error!("HEAP: PHYS_OFFSET missing");
            return;
        }
    };
    let (cr3_frame, _) = Cr3::read();
    let cr3_phys = PhysAddr::new(cr3_frame.start_address().as_u64());
    let p4_virt = VirtAddr::new(cr3_phys.as_u64() + phys_offset);
    let p4 = unsafe { &*p4_virt.as_ptr::<PageTable>() };

    let p4e = p4[addr.p4_index()];
    logger::info!(
        "HEAP: walk P4 idx={} flags={:?} addr={:#x}",
        addr.p4_index(),
        p4e.flags(),
        p4e.addr().as_u64()
    );
    if !p4e.flags().contains(PageTableFlags::PRESENT) {
        return;
    }

    let p3 = unsafe { &*VirtAddr::new(p4e.addr().as_u64() + phys_offset).as_ptr::<PageTable>() };
    let p3e = p3[addr.p3_index()];
    logger::info!(
        "HEAP: walk P3 idx={} flags={:?} addr={:#x}",
        addr.p3_index(),
        p3e.flags(),
        p3e.addr().as_u64()
    );
    if !p3e.flags().contains(PageTableFlags::PRESENT) {
        return;
    }

    if p3e.flags().contains(PageTableFlags::HUGE_PAGE) {
        return;
    }
    let p2 = unsafe { &*VirtAddr::new(p3e.addr().as_u64() + phys_offset).as_ptr::<PageTable>() };
    let p2e = p2[addr.p2_index()];
    logger::info!(
        "HEAP: walk P2 idx={} flags={:?} addr={:#x}",
        addr.p2_index(),
        p2e.flags(),
        p2e.addr().as_u64()
    );
    if !p2e.flags().contains(PageTableFlags::PRESENT) {
        return;
    }

    if p2e.flags().contains(PageTableFlags::HUGE_PAGE) {
        return;
    }
    let p1 = unsafe { &*VirtAddr::new(p2e.addr().as_u64() + phys_offset).as_ptr::<PageTable>() };
    let p1e = p1[addr.p1_index()];
    logger::info!(
        "HEAP: walk P1 idx={} flags={:?} addr={:#x}",
        addr.p1_index(),
        p1e.flags(),
        p1e.addr().as_u64()
    );
}

fn validate_heap_mapping(addr: VirtAddr) {
    use crate::PageTable;
    use x86_64::registers::control::Cr3;
    let phys_offset = *crate::PHYS_OFFSET.get().expect("HEAP: PHYS_OFFSET missing");
    let (cr3_frame, _) = Cr3::read();
    let cr3_phys = PhysAddr::new(cr3_frame.start_address().as_u64());
    let p4_virt = VirtAddr::new(cr3_phys.as_u64() + phys_offset);
    let p4 = unsafe { &*p4_virt.as_ptr::<PageTable>() };
    let p4e = p4[addr.p4_index()];
    if !p4e.flags().contains(PageTableFlags::PRESENT) {
        panic!("HEAP: validate P4 not present");
    }
    let p3 = unsafe { &*VirtAddr::new(p4e.addr().as_u64() + phys_offset).as_ptr::<PageTable>() };
    let p3e = p3[addr.p3_index()];
    if !p3e.flags().contains(PageTableFlags::PRESENT) {
        panic!("HEAP: validate P3 not present");
    }
    if p3e.flags().contains(PageTableFlags::HUGE_PAGE) {
        return;
    }
    let p2 = unsafe { &*VirtAddr::new(p3e.addr().as_u64() + phys_offset).as_ptr::<PageTable>() };
    let p2e = p2[addr.p2_index()];
    if !p2e.flags().contains(PageTableFlags::PRESENT) {
        panic!("HEAP: validate P2 not present");
    }
    if p2e.flags().contains(PageTableFlags::HUGE_PAGE) {
        return;
    }
    let p1 = unsafe { &*VirtAddr::new(p2e.addr().as_u64() + phys_offset).as_ptr::<PageTable>() };
    let p1e = p1[addr.p1_index()];
    if !p1e.flags().contains(PageTableFlags::PRESENT) {
        panic!("HEAP: validate P1 not present");
    }
}

pub fn heap_test() {
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    let heap_value = Box::new(41);
    assert_eq!(*heap_value, 41);

    let mut vec = Vec::new();
    for i in 0..500 {
        vec.push(i);
    }
    assert_eq!(vec.iter().sum::<i32>(), (0..500).sum());
}

#[derive(Debug, Clone, Copy)]
pub struct HeapStats {
    pub alloc_calls: usize,
    pub alloc_fallback_global: usize,
    pub alloc_fallback_other: usize,
    pub alloc_fail: usize,
    pub dealloc_calls: usize,
    pub grow_attempts: usize,
    pub grow_success: usize,
    pub free_blocks: usize,
    pub max_free: usize,
}

pub fn heap_stats() -> HeapStats {
    let (free_blocks, max_free) = HEAP_ALLOCATOR.heap_frag_stats();
    HeapStats {
        alloc_calls: HEAP_ALLOCATOR.alloc_calls.load(Ordering::Relaxed),
        alloc_fallback_global: HEAP_ALLOCATOR.alloc_fallback_global.load(Ordering::Relaxed),
        alloc_fallback_other: HEAP_ALLOCATOR.alloc_fallback_other.load(Ordering::Relaxed),
        alloc_fail: HEAP_ALLOCATOR.alloc_fail.load(Ordering::Relaxed),
        dealloc_calls: HEAP_ALLOCATOR.dealloc_calls.load(Ordering::Relaxed),
        grow_attempts: HEAP_ALLOCATOR.grow_attempts.load(Ordering::Relaxed),
        grow_success: HEAP_ALLOCATOR.grow_success.load(Ordering::Relaxed),
        free_blocks,
        max_free,
    }
}

pub fn reset_heap_stats() {
    HEAP_ALLOCATOR.alloc_calls.store(0, Ordering::Relaxed);
    HEAP_ALLOCATOR
        .alloc_fallback_global
        .store(0, Ordering::Relaxed);
    HEAP_ALLOCATOR
        .alloc_fallback_other
        .store(0, Ordering::Relaxed);
    HEAP_ALLOCATOR.alloc_fail.store(0, Ordering::Relaxed);
    HEAP_ALLOCATOR.dealloc_calls.store(0, Ordering::Relaxed);
    HEAP_ALLOCATOR.grow_attempts.store(0, Ordering::Relaxed);
    HEAP_ALLOCATOR.grow_success.store(0, Ordering::Relaxed);
}
