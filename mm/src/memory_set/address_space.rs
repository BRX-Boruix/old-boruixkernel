use alloc::vec::Vec;
use core::slice;

use crate::addr::{PhysAddr, VirtAddr};
use crate::error::MmError;
use crate::frame_allocator;
use crate::mapper::{MapError, Mapper, OffsetMapper};
use crate::page_table::{PageTable, PageTableFlags};
use crate::PHYS_OFFSET;

use super::vma_tree::{MemoryArea, VmaTree, VmaTreeStats};

pub struct MemorySet {
    p4_phys: PhysAddr,
    vma: VmaTree,
    pub(crate) stack_bottom: Option<VirtAddr>,
    pub(crate) stack_top: Option<VirtAddr>,
    pub(crate) stack_limit: Option<VirtAddr>,
}

impl MemorySet {
    pub fn vma_tree_stats(&self) -> VmaTreeStats {
        self.vma.vma_tree_stats()
    }

    /// Create a new empty MemorySet with a new P4 table
    pub fn new_bare() -> Result<Self, MmError> {
        use x86_64::registers::control::Cr3;

        let frame = frame_allocator::allocate_frame().ok_or(MmError::FrameAllocationFailed)?;
        let phys = PhysAddr::new(frame.start_address().as_u64());

        // Initialize the new page table
        let phys_offset = *PHYS_OFFSET.get().ok_or(MmError::PhysOffsetMissing)?;
        let virt = VirtAddr::new(phys.as_u64() + phys_offset);
        // SAFETY: `virt` is a valid, aligned HHDM mapping for the new P4 frame.
        let table = unsafe { &mut *virt.as_mut_ptr::<PageTable>() };
        table.zero();

        // Copy kernel mappings from the boot kernel P4 (stable even if current CR3 is a task)
        let current_p4_phys = if let Some(p) = crate::KERNEL_P4_PHYS.get() {
            PhysAddr::new(*p)
        } else {
            let (current_p4_frame, _) = Cr3::read();
            PhysAddr::new(current_p4_frame.start_address().as_u64())
        };
        let current_p4_virt = VirtAddr::new(current_p4_phys.as_u64() + phys_offset);
        let current_p4_table = unsafe { &*current_p4_virt.as_ptr::<PageTable>() };

        // Copy high half entries (256-511)
        for i in 256..512 {
            table[i] = current_p4_table[i].clone();
        }

        // Sanity check: ensure HHDM entry is present in the new table
        let hhdm_p4 = VirtAddr::new(phys_offset).p4_index();
        if !table[hhdm_p4].flags().contains(PageTableFlags::PRESENT) {
            panic!(
                "MemorySet: missing HHDM mapping in new P4 (index {})",
                hhdm_p4
            );
        }

        Ok(Self {
            p4_phys: phys,
            vma: VmaTree::new(),
            stack_bottom: None,
            stack_top: None,
            stack_limit: None,
        })
    }

    /// Get the physical address of the P4 table
    pub fn token(&self) -> u64 {
        self.p4_phys.as_u64()
    }

    /// Activate this MemorySet (write to CR3)
    pub unsafe fn activate(&self) {
        use x86_64::registers::control::Cr3;
        use x86_64::structures::paging::PhysFrame;
        use x86_64::PhysAddr as X86PhysAddr;

        if let Ok(frame) = PhysFrame::from_start_address(X86PhysAddr::new(self.p4_phys.as_u64())) {
            // SAFETY: Caller ensures the page table is valid and safe to activate.
            let (_, flags) = Cr3::read();
            Cr3::write(frame, flags);
        }
    }

    /// Create a temporary mapper for this MemorySet
    /// Note: This is unsafe because it creates multiple mutable references to the page table if called repeatedly.
    /// The caller must ensure exclusivity.
    pub unsafe fn mapper(&mut self) -> Result<OffsetMapper<'_>, MmError> {
        let phys_offset = *PHYS_OFFSET.get().ok_or(MmError::PhysOffsetMissing)?;
        let virt = VirtAddr::new(self.p4_phys.as_u64() + phys_offset);
        // SAFETY: caller ensures exclusive access to page tables for this MemorySet.
        let table = &mut *virt.as_mut_ptr::<PageTable>();
        Ok(OffsetMapper::new(table, phys_offset, self.p4_phys))
    }

    pub fn push_lazy(&mut self, area: MemoryArea) {
        self.vma.insert_area(area).expect("push_lazy overlap");
    }

    pub(crate) fn insert_area(&mut self, area: MemoryArea) -> Result<(), MapError> {
        self.vma.insert_area(area)
    }

    /// Check if a memory range is valid (contained in mapped areas)
    pub fn check_validity(&self, addr: VirtAddr, size: usize) -> bool {
        self.check_range(addr, size, PageTableFlags::empty())
    }

    /// Check if a memory range is valid and satisfies required flags
    pub fn check_range(&self, addr: VirtAddr, size: usize, required: PageTableFlags) -> bool {
        if size == 0 {
            return true;
        }
        let start = addr.as_u64();
        let end = match start.checked_add(size as u64) {
            Some(v) => v,
            None => return false,
        };

        let mut cur = start;
        let mut current_area = self.vma.find_containing(cur);
        if current_area.is_none() {
            current_area = self.vma.find_next(cur);
        }

        while cur < end {
            let area = match current_area {
                Some(a) => a,
                None => return false,
            };
            if area.start.as_u64() > cur {
                return false;
            }
            if !area.flags.contains(required) {
                return false;
            }

            let next = core::cmp::min(end, area.end.as_u64());
            cur = next;
            if cur < end {
                current_area = self.vma.find_next(cur);
            }
        }

        true
    }

    /// Check if a memory range is backed by present page-table entries and
    /// satisfies required access bits for each mapped page.
    pub fn check_range_mapped(
        &self,
        addr: VirtAddr,
        size: usize,
        required: PageTableFlags,
    ) -> bool {
        if !self.check_range(addr, size, required) {
            return false;
        }
        if size == 0 {
            return true;
        }

        let start = addr.as_u64();
        let end = match start.checked_add(size as u64) {
            Some(v) => v,
            None => return false,
        };

        let hierarchy_required =
            required & (PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE);
        let mut page = VirtAddr::new(start).align_down(4096u64).as_u64();
        let last = VirtAddr::new(end - 1).align_down(4096u64).as_u64();

        loop {
            if !self.page_access_ok(VirtAddr::new(page), hierarchy_required, required) {
                return false;
            }
            if page == last {
                break;
            }
            page += 4096;
        }

        true
    }

    fn page_access_ok(
        &self,
        page: VirtAddr,
        hierarchy_required: PageTableFlags,
        leaf_required: PageTableFlags,
    ) -> bool {
        let phys_offset = match PHYS_OFFSET.get() {
            Some(v) => *v,
            None => return false,
        };

        let p4_virt = VirtAddr::new(self.p4_phys.as_u64() + phys_offset);
        // SAFETY: p4_virt is a valid HHDM mapping for the P4 frame.
        let p4 = unsafe { &*p4_virt.as_ptr::<PageTable>() };
        let p4_entry = &p4[page.p4_index()];
        if !entry_allows(p4_entry.flags(), hierarchy_required) {
            return false;
        }

        let p3 = unsafe {
            &*VirtAddr::new(p4_entry.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
        };
        let p3_entry = &p3[page.p3_index()];
        if !entry_allows(p3_entry.flags(), hierarchy_required) {
            return false;
        }
        if p3_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            return entry_allows(p3_entry.flags(), leaf_required);
        }

        let p2 = unsafe {
            &*VirtAddr::new(p3_entry.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
        };
        let p2_entry = &p2[page.p2_index()];
        if !entry_allows(p2_entry.flags(), hierarchy_required) {
            return false;
        }
        if p2_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            return entry_allows(p2_entry.flags(), leaf_required);
        }

        let p1 = unsafe {
            &*VirtAddr::new(p2_entry.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
        };
        let p1_entry = &p1[page.p1_index()];
        entry_allows(p1_entry.flags(), leaf_required)
    }

    /// Debug helper: locate VMA containing addr and nearest neighbors.
    pub fn debug_vma_lookup(
        &self,
        addr: VirtAddr,
    ) -> (Option<MemoryArea>, Option<MemoryArea>, Option<MemoryArea>) {
        self.vma.debug_vma_lookup(addr)
    }

    /// Translate virtual address to physical address in this MemorySet (read-only)
    pub fn translate(&self, addr: VirtAddr) -> Option<PhysAddr> {
        let phys_offset = *PHYS_OFFSET.get()?;
        let p4_virt = VirtAddr::new(self.p4_phys.as_u64() + phys_offset);
        // SAFETY: p4_virt is a valid HHDM mapping for the P4 frame.
        let p4 = unsafe { &*p4_virt.as_ptr::<PageTable>() };

        let p4_entry = &p4[addr.p4_index()];
        if !p4_entry.flags().contains(PageTableFlags::PRESENT) {
            return None;
        }

        let p3 = unsafe {
            &*VirtAddr::new(p4_entry.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
        };
        let p3_entry = &p3[addr.p3_index()];
        if !p3_entry.flags().contains(PageTableFlags::PRESENT) {
            return None;
        }

        if p3_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            let offset = addr.as_u64() & 0x3fff_ffff;
            return Some(PhysAddr::new(p3_entry.addr().as_u64() + offset));
        }

        let p2 = unsafe {
            &*VirtAddr::new(p3_entry.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
        };
        let p2_entry = &p2[addr.p2_index()];
        if !p2_entry.flags().contains(PageTableFlags::PRESENT) {
            return None;
        }

        if p2_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            let offset = addr.as_u64() & 0x1f_ffff;
            return Some(PhysAddr::new(p2_entry.addr().as_u64() + offset));
        }

        let p1 = unsafe {
            &*VirtAddr::new(p2_entry.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
        };
        let p1_entry = &p1[addr.p1_index()];
        if !p1_entry.flags().contains(PageTableFlags::PRESENT) {
            return None;
        }

        let offset = addr.as_u64() & 0xfff;
        Some(PhysAddr::new(p1_entry.addr().as_u64() + offset))
    }

    /// Map a user range [start, end) with flags and insert VMA.
    pub fn map_user_range(
        &mut self,
        start: VirtAddr,
        end: VirtAddr,
        flags: PageTableFlags,
    ) -> Result<(), MapError> {
        if start >= end {
            return Ok(());
        }
        let area = MemoryArea::new(start, end, flags);
        self.push(area, None)
    }

    /// Map a memory area
    pub fn push(&mut self, area: MemoryArea, data: Option<&[u8]>) -> Result<(), MapError> {
        let mut mapper = unsafe { self.mapper().map_err(|_| MapError::InvalidAccess)? };
        let mut mapped: Vec<(VirtAddr, PhysAddr)> = Vec::new();

        let rollback = |mapper: &mut OffsetMapper<'_>, mapped: &[(VirtAddr, PhysAddr)]| {
            for (page, phys) in mapped.iter().rev() {
                unsafe {
                    if mapper.unmap_page_noflush(*page).is_ok() {
                        let frame = x86_64::structures::paging::PhysFrame::containing_address(
                            x86_64::PhysAddr::new(phys.as_u64()),
                        );
                        frame_allocator::deallocate_frame(frame);
                    }
                }
            }
            mapper.flush_all();
        };

        let start_page = area.start.align_down(4096u64);
        let end_page = area.end.align_up(4096u64);

        for page_addr in (start_page.as_u64()..end_page.as_u64()).step_by(4096) {
            let page = VirtAddr::new(page_addr);
            let frame = frame_allocator::allocate_frame().ok_or(MapError::FrameAllocationFailed)?;
            let phys = PhysAddr::new(frame.start_address().as_u64());

            unsafe {
                if let Err(e) = mapper.map_page_noflush(page, phys, area.flags) {
                    rollback(&mut mapper, &mapped);
                    return Err(e);
                }
            }
            mapped.push((page, phys));

            // Always zero the frame to prevent data leaks, then optionally copy data.
            let phys_offset = *PHYS_OFFSET.get().ok_or(MapError::InvalidAccess)?;
            let dst_virt = VirtAddr::new(phys.as_u64() + phys_offset);
            unsafe {
                dst_virt.as_mut_ptr::<u8>().write_bytes(0, 4096);
            }

            if let Some(data) = data {
                let offset = (page_addr - area.start.as_u64()) as usize;
                if offset < data.len() {
                    let len = core::cmp::min(4096, data.len() - offset);
                    let src = &data[offset..offset + len];

                    let dst =
                        unsafe { slice::from_raw_parts_mut(dst_virt.as_mut_ptr::<u8>(), len) };
                    dst.copy_from_slice(src);
                }
            }
        }

        mapper.flush_all();
        self.vma.insert_area(area)?;
        Ok(())
    }

    pub fn fork(&self) -> Result<Self, MmError> {
        let mut new_set = Self::new_bare()?;
        new_set.stack_bottom = self.stack_bottom;
        new_set.stack_top = self.stack_top;
        new_set.stack_limit = self.stack_limit;
        let phys_offset = *PHYS_OFFSET.get().ok_or(MmError::PhysOffsetMissing)?;

        let mut areas: Vec<MemoryArea> = Vec::new();
        self.vma.for_each_in_order(|area| areas.push(*area));
        for area in areas.iter() {
            new_set.push_lazy(*area);
        }

        for area in areas.iter() {
            let start_page = area.start.align_down(4096u64);
            let end_page = area.end.align_up(4096u64);

            let mut mapper = unsafe { new_set.mapper()? };
            for page_addr in (start_page.as_u64()..end_page.as_u64()).step_by(4096) {
                let page = VirtAddr::new(page_addr);

                let frame =
                    frame_allocator::allocate_frame().ok_or(MmError::FrameAllocationFailed)?;
                let new_phys = PhysAddr::new(frame.start_address().as_u64());

                unsafe {
                    mapper
                        .map_page_noflush(page, new_phys, area.flags)
                        .map_err(MmError::Map)?;
                }

                let dst_virt = VirtAddr::new(new_phys.as_u64() + phys_offset);
                let dst_ptr = dst_virt.as_mut_ptr::<u8>();

                if let Some(src_phys) = self.translate(page) {
                    let src_virt = VirtAddr::new(src_phys.as_u64() + phys_offset);
                    let src_ptr = src_virt.as_ptr::<u8>();
                    unsafe {
                        core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 4096);
                    }
                } else {
                    unsafe {
                        core::ptr::write_bytes(dst_ptr, 0, 4096);
                    }
                }
            }
            mapper.flush_all();
        }

        Ok(new_set)
    }

    pub(crate) fn find_containing_area(&self, addr: VirtAddr) -> Option<MemoryArea> {
        self.vma.find_containing(addr.as_u64())
    }
}

impl Drop for MemorySet {
    fn drop(&mut self) {
        use x86_64::structures::paging::PhysFrame;
        use x86_64::PhysAddr as X86PhysAddr;

        let mut areas: Vec<MemoryArea> = Vec::new();
        self.vma.for_each_in_order(|area| areas.push(*area));
        self.vma.clear();

        let mut mapper = match unsafe { self.mapper() } {
            Ok(m) => m,
            Err(_) => return,
        };

        // Unmap all user areas; unmap_page now cleans up empty page tables.
        for area in areas.iter() {
            let start_page = area.start.align_down(4096u64);
            let end_page = area.end.align_up(4096u64);

            for page_addr in (start_page.as_u64()..end_page.as_u64()).step_by(4096) {
                let page = VirtAddr::new(page_addr);
                // SAFETY: the mapper owns page table mutations for this MemorySet.
                unsafe {
                    if let Ok(phys) = mapper.unmap_page_noflush(page) {
                        let frame = PhysFrame::containing_address(X86PhysAddr::new(phys.as_u64()));
                        frame_allocator::deallocate_frame(frame);
                    }
                }
            }
        }

        mapper.flush_all();

        // Deallocate P4 table frame
        if let Ok(p4_frame) = PhysFrame::from_start_address(X86PhysAddr::new(self.p4_phys.as_u64()))
        {
            frame_allocator::deallocate_frame(p4_frame);
        }
    }
}

fn entry_allows(flags: PageTableFlags, required: PageTableFlags) -> bool {
    flags.contains(PageTableFlags::PRESENT) && flags.contains(required)
}
