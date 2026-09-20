use core::fmt::Write;

use alloc::string::String;

use kernel_platform::memory as mem;
use kernel_task::task;
use mem::addr_space::PageTableFlags;

use super::user_ptr::validate_user_range;

pub(super) fn sys_mmcompact() -> isize {
    mem::pmm::compact_now();
    0
}

pub(super) fn sys_sbrk(increment: isize) -> isize {
    let current_task = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let heap_start = current_task.heap_start.lock();
    let mut heap_end = current_task.heap_end.lock();
    let heap_max = *current_task.heap_max.lock();

    let old = *heap_end;
    if increment == 0 {
        return old as isize;
    }
    let new_end = if increment > 0 {
        let inc = increment as usize;
        old.saturating_add(inc)
    } else {
        let dec = (-increment) as usize;
        old.saturating_sub(dec)
    };
    if new_end < *heap_start || new_end > heap_max {
        return -1;
    }

    if increment > 0 {
        let map_start = mem::addr_space::VirtAddr::new(old as u64).align_up(4096u64);
        let map_end = mem::addr_space::VirtAddr::new(new_end as u64).align_up(4096u64);
        if map_end > map_start {
            let flags = PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE
                | PageTableFlags::NO_EXECUTE;
            unsafe {
                let ms = &mut *current_task.memory_set.get();
                if ms.map_user_range(map_start, map_end, flags).is_err() {
                    return -1;
                }
            }
        }
        *heap_end = new_end;
        return old as isize;
    }

    let unmap_start = mem::addr_space::VirtAddr::new(new_end as u64).align_up(4096u64);
    let unmap_end = mem::addr_space::VirtAddr::new(old as u64).align_up(4096u64);
    if unmap_end > unmap_start {
        use mem::addr_space::VirtAddr;
        use mem::mapper::Mapper;
        use x86_64::structures::paging::PhysFrame;
        use x86_64::PhysAddr as X86PhysAddr;

        unsafe {
            let ms = &mut *current_task.memory_set.get();
            let mut mapper = match ms.mapper() {
                Ok(m) => m,
                Err(_) => return -1,
            };

            let mut unmapped: alloc::vec::Vec<VirtAddr> = alloc::vec::Vec::new();
            let mut current = unmap_start;
            while current < unmap_end {
                if let Ok(phys) = mapper.unmap_page_noflush(current) {
                    let frame = PhysFrame::containing_address(X86PhysAddr::new(phys.as_u64()));
                    mem::pmm::frame_allocator::deallocate_frame(frame);
                    unmapped.push(current);
                } else {
                    // rollback: remap already-unmapped pages as empty pages
                    for page in unmapped.into_iter().rev() {
                        if let Some(frame) = mem::pmm::frame_allocator::allocate_frame() {
                            let phys =
                                mem::addr_space::PhysAddr::new(frame.start_address().as_u64());
                            let phys_offset = match mem::addr_space::PHYS_OFFSET.get() {
                                Some(v) => *v,
                                None => return -1,
                            };
                            let virt = VirtAddr::new(phys.as_u64() + phys_offset);
                            virt.as_mut_ptr::<u8>().write_bytes(0, 4096);
                            let _ = mapper.map_page_noflush(
                                page,
                                phys,
                                PageTableFlags::PRESENT
                                    | PageTableFlags::WRITABLE
                                    | PageTableFlags::USER_ACCESSIBLE
                                    | PageTableFlags::NO_EXECUTE,
                            );
                        } else {
                            return -1;
                        }
                    }
                    mapper.flush_all();
                    return -1;
                }
                current = VirtAddr::new(current.as_u64() + 4096);
            }
            mapper.flush_all();
        }
    }

    *heap_end = new_end;
    old as isize
}

#[cfg(feature = "debug-syscall")]
pub(super) fn sys_testpmm2m(count: usize) -> isize {
    use mem::pmm::frame_allocator::{allocate_frames, deallocate_frame, ORDER_2M};
    let times = if count == 0 { 1 } else { count.min(256) };
    let mut ok = 0usize;
    let mut frames = [None; 256];
    for i in 0..times {
        if let Some(f) = allocate_frames(ORDER_2M) {
            frames[i] = Some(f);
            ok += 1;
        } else {
            break;
        }
    }
    for i in 0..times {
        if let Some(f) = frames[i] {
            deallocate_frame(f);
        }
    }
    ok as isize
}

pub(super) fn sys_migrate_one() -> isize {
    if mem::pmm::migrate_one_user_page() {
        1
    } else {
        0
    }
}

pub(super) fn sys_rmapcount() -> isize {
    mem::rmap::rmap_count() as isize
}

pub(super) fn sys_mmstat(buf: *mut u8, len: usize, flags: usize) -> isize {
    if (flags & 1) != 0 {
        mem::pmm::reset_frame_stats();
        mem::heap::reset_heap_stats();
        mem::addr_space::reset_vma_stats();
        mem::rmap::reset_rmap_stats();
        return 0;
    }
    if len == 0 {
        return 0;
    }
    if !validate_user_range(buf as usize, len, true) {
        return -1;
    }

    let fs = mem::pmm::frame_stats();
    let frag = mem::pmm::frag_stats();
    let hs = mem::heap::heap_stats();
    let mut out = String::new();
    let _ = writeln!(out, "PMM stats:");
    let _ = writeln!(out, "  allocated_frames: {}", fs.allocated_frames);
    let _ = writeln!(out, "  alloc_calls: {}", fs.alloc_calls);
    let _ = writeln!(out, "  alloc_hit_percpu: {}", fs.alloc_hit_percpu);
    let _ = writeln!(out, "  alloc_refill: {}", fs.alloc_refill);
    let _ = writeln!(out, "  alloc_hit_global: {}", fs.alloc_hit_global);
    let _ = writeln!(out, "  alloc_hit_uninit: {}", fs.alloc_hit_uninit);
    let _ = writeln!(out, "  alloc_fail: {}", fs.alloc_fail);
    let _ = writeln!(out, "  compact_calls: {}", fs.compact_calls);
    let _ = writeln!(out, "  compact_drained: {}", fs.compact_drained);
    let _ = writeln!(out, "  compact_success: {}", fs.compact_success);
    let _ = writeln!(
        out,
        "  compact_last: {} -> {}",
        fs.compact_last_before, fs.compact_last_after
    );
    let _ = writeln!(out, "  max_free_order: {}", frag.max_order);
    let _ = writeln!(out, "  uninit_frames: {}", frag.uninit_frames);
    let _ = writeln!(out, "  alloc_fail_by_order:");
    for (i, v) in fs.alloc_fail_by_order.iter().enumerate() {
        if *v > 0 {
            let _ = writeln!(out, "    order {}: {}", i, v);
        }
    }
    let _ = writeln!(out, "  free_global_by_order:");
    for (i, v) in frag.free_global_by_order.iter().enumerate() {
        if *v > 0 {
            let _ = writeln!(out, "    order {}: {}", i, v);
        }
    }
    let _ = writeln!(out, "  free_percpu_by_order:");
    for (i, v) in frag.free_percpu_by_order.iter().enumerate() {
        if *v > 0 {
            let _ = writeln!(out, "    order {}: {}", i, v);
        }
    }
    let _ = writeln!(out, "  dealloc_calls: {}", fs.dealloc_calls);
    let _ = writeln!(out, "  global_list_ops: {}", fs.global_list_ops);
    let _ = writeln!(out, "  reserve_count: {}", fs.reserve_count);
    let _ = writeln!(out, "");
    let _ = writeln!(out, "Heap stats:");
    let _ = writeln!(out, "  alloc_calls: {}", hs.alloc_calls);
    let _ = writeln!(out, "  alloc_fallback_global: {}", hs.alloc_fallback_global);
    let _ = writeln!(out, "  alloc_fallback_other: {}", hs.alloc_fallback_other);
    let _ = writeln!(out, "  alloc_fail: {}", hs.alloc_fail);
    let _ = writeln!(out, "  dealloc_calls: {}", hs.dealloc_calls);
    let _ = writeln!(out, "  grow_attempts: {}", hs.grow_attempts);
    let _ = writeln!(out, "  grow_success: {}", hs.grow_success);
    let _ = writeln!(out, "  free_blocks: {}", hs.free_blocks);
    let _ = writeln!(out, "  max_free: {}", hs.max_free);

    let vs = mem::addr_space::vma_stats();
    let rs = mem::rmap::rmap_stats();
    let tree_stats = task::current_task()
        .map(|t| unsafe { (*t.memory_set.get()).vma_tree_stats() })
        .unwrap_or(mem::addr_space::VmaTreeStats {
            node_count: 0,
            max_depth: 0,
        });
    let _ = writeln!(out, "");
    let _ = writeln!(out, "VMA stats:");
    let _ = writeln!(out, "  inserts: {}", vs.inserts);
    let _ = writeln!(out, "  merges: {}", vs.merges);
    let _ = writeln!(out, "  overlap_rejects: {}", vs.overlap_rejects);
    let _ = writeln!(out, "  node_count: {}", tree_stats.node_count);
    let _ = writeln!(out, "  max_depth: {}", tree_stats.max_depth);
    let _ = writeln!(out, "");
    let _ = writeln!(out, "RMAP stats:");
    let _ = writeln!(out, "  add: {}", rs.add);
    let _ = writeln!(out, "  remove: {}", rs.remove);
    let _ = writeln!(out, "  lookup: {}", rs.lookup);
    let _ = writeln!(out, "  mig_read: {}", rs.mig_read);
    let _ = writeln!(out, "  mig_write: {}", rs.mig_write);

    let bytes = out.as_bytes();
    let copy_len = core::cmp::min(len, bytes.len());
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, copy_len);
    }
    copy_len as isize
}
