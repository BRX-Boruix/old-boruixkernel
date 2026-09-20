use crate::addr::{PhysAddr, VirtAddr};
use crate::frame_allocator;
use crate::page_table::{dec_table_count, inc_table_count, table_count, PageTable, PageTableFlags};
use crate::rmap::{migration_read_lock, rmap_add, rmap_remove};
use x86_64::instructions::tlb;
use x86_64::VirtAddr as X86VirtAddr;

#[inline(always)]
fn shootdown(p4_phys: PhysAddr) {
    crate::notify_tlb_shootdown(p4_phys.as_u64());
}

/// Error type for page map operations
#[derive(Debug)]
pub enum MapError {
    FrameAllocationFailed,
    PageAlreadyMapped,
    PageNotMapped,
    ParentEntryHugePage,
    HugePageNotAligned,
    InvalidHugePageUnmap,
    InvalidAccess,
}

/// Trait for mapping pages
pub trait Mapper {
    /// Map a virtual page to a physical frame
    unsafe fn map_page(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<(), MapError>;

    /// Map a virtual page without flushing the TLB (for batching)
    unsafe fn map_page_noflush(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<(), MapError>;

    /// Unmap a virtual page
    unsafe fn unmap_page(&mut self, page: VirtAddr) -> Result<PhysAddr, MapError>;

    /// Unmap a virtual page without flushing the TLB (for batching)
    unsafe fn unmap_page_noflush(&mut self, page: VirtAddr) -> Result<PhysAddr, MapError>;

    /// Translate a virtual address to physical address
    unsafe fn translate(&self, addr: VirtAddr) -> Option<PhysAddr>;

    /// Flush all TLB entries (for batched mappings)
    fn flush_all(&self);
}

/// A mapper that uses a direct map offset to access physical frames
pub struct OffsetMapper<'a> {
    p4: &'a mut PageTable,
    phys_offset: u64,
    p4_phys: PhysAddr,
}

impl<'a> OffsetMapper<'a> {
    /// Create a new OffsetMapper
    ///
    /// # Safety
    /// The caller must ensure that `phys_offset` is correct and `p4` is the active top-level page table.
    pub unsafe fn new(p4: &'a mut PageTable, phys_offset: u64, p4_phys: PhysAddr) -> Self {
        Self {
            p4,
            phys_offset,
            p4_phys,
        }
    }

    fn phys_to_virt(phys: PhysAddr, phys_offset: u64) -> VirtAddr {
        VirtAddr::new(phys.as_u64() + phys_offset)
    }

    /// Get a mutable reference to the next level table, creating it if necessary
    unsafe fn get_next_table_by_index(
        table: &mut PageTable,
        index: usize,
        phys_offset: u64,
    ) -> Result<&mut PageTable, MapError> {
        let entry = &mut table[index];
        if entry.flags().contains(PageTableFlags::PRESENT) {
            if entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                return Err(MapError::ParentEntryHugePage);
            }
            let phys = entry.addr();
            let virt = Self::phys_to_virt(phys, phys_offset);
            Ok(&mut *virt.as_mut_ptr())
        } else {
            // Allocate a new frame for the page table
            let frame = frame_allocator::allocate_frame().ok_or(MapError::FrameAllocationFailed)?;
            let phys = PhysAddr::new(frame.start_address().as_u64());
            if phys.as_u64() == 0 {
                panic!("OffsetMapper: allocator returned frame at paddr 0");
            }

            // Zero the new table
            let virt = Self::phys_to_virt(phys, phys_offset);
            let table = &mut *virt.as_mut_ptr::<PageTable>();
            table.zero();

            // Set the entry to point to the new table
            // We give generous permissions to the intermediate table entries
            // The actual permissions are controlled by the leaf entry (P1)
            entry.set_addr(
                phys,
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE,
            );
            let parent_phys = Self::table_phys(table, phys_offset);
            inc_table_count(parent_phys);

            Ok(table)
        }
    }

    fn table_empty(table_phys: PhysAddr, table: &PageTable) -> bool {
        table_count(table_phys).unwrap_or_else(|| table.used_count()) == 0
    }

    fn table_phys(table: &PageTable, phys_offset: u64) -> PhysAddr {
        let virt = table as *const PageTable as u64;
        PhysAddr::new(virt - phys_offset)
    }

    unsafe fn map_page_inner(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        flags: PageTableFlags,
        flush: bool,
    ) -> Result<(), MapError> {
        let phys_offset = self.phys_offset;
        let p4 = &mut self.p4;
        let p4_index = page.p4_index();
        let p4_ptr: *mut PageTable = &mut **p4;
        let p3 = {
            let table = &mut *p4_ptr;
            Self::get_next_table_by_index(table, p4_index, phys_offset)?
        };

        if flags.contains(PageTableFlags::HUGE_PAGE) {
            // Map as 1GiB huge page if aligned, else 2MiB huge page
            if page.is_aligned(0x4000_0000) && frame.is_aligned(0x4000_0000) {
                let entry = &mut p3[page.p3_index()];
                if !entry.is_unused() {
                    return Err(MapError::PageAlreadyMapped);
                }
                entry.set_addr(
                    frame,
                    flags | PageTableFlags::PRESENT | PageTableFlags::HUGE_PAGE,
                );
                let p3_phys = Self::table_phys(p3, phys_offset);
                inc_table_count(p3_phys);
                if flush {
                    tlb::flush(X86VirtAddr::new(page.as_u64()));
                    shootdown(self.p4_phys);
                }
                return Ok(());
            }
        }

        let p3_index = page.p3_index();
        let p3_ptr = p3 as *mut PageTable;
        let p2 = {
            let table = &mut *p3_ptr;
            Self::get_next_table_by_index(table, p3_index, phys_offset)?
        };

        if flags.contains(PageTableFlags::HUGE_PAGE) {
            // Map as 2MiB huge page
            let entry = &mut p2[page.p2_index()];
            if !entry.is_unused() {
                return Err(MapError::PageAlreadyMapped);
            }
            // Ensure address is aligned
            if !page.is_aligned(0x200_000) || !frame.is_aligned(0x200_000) {
                return Err(MapError::HugePageNotAligned);
            }
            entry.set_addr(
                frame,
                flags | PageTableFlags::PRESENT | PageTableFlags::HUGE_PAGE,
            );
            let p2_phys = Self::table_phys(p2, phys_offset);
            inc_table_count(p2_phys);
            if flush {
                tlb::flush(X86VirtAddr::new(page.as_u64()));
                shootdown(self.p4_phys);
            }
            return Ok(());
        }

        let p2_index = page.p2_index();
        let p2_ptr = p2 as *mut PageTable;
        let p1 = {
            let table = &mut *p2_ptr;
            Self::get_next_table_by_index(table, p2_index, phys_offset)?
        };

        let entry = &mut p1[page.p1_index()];
        if !entry.is_unused() {
            return Err(MapError::PageAlreadyMapped);
        }

        entry.set_addr(frame, flags | PageTableFlags::PRESENT);
        let p1_phys = Self::table_phys(p1, phys_offset);
        inc_table_count(p1_phys);
        if flags.contains(PageTableFlags::USER_ACCESSIBLE) {
            rmap_add(frame, self.p4_phys, page, flags);
        }

        if flush {
            tlb::flush(X86VirtAddr::new(page.as_u64()));
            shootdown(self.p4_phys);
        }

        Ok(())
    }
}

impl<'a> Mapper for OffsetMapper<'a> {
    unsafe fn map_page(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<(), MapError> {
        let _migration_guard = migration_read_lock();
        self.map_page_inner(page, frame, flags, true)
    }

    unsafe fn map_page_noflush(
        &mut self,
        page: VirtAddr,
        frame: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<(), MapError> {
        let _migration_guard = migration_read_lock();
        self.map_page_inner(page, frame, flags, false)
    }

    unsafe fn unmap_page(&mut self, page: VirtAddr) -> Result<PhysAddr, MapError> {
        let _migration_guard = migration_read_lock();
        self.unmap_page_inner(page, true)
    }

    unsafe fn unmap_page_noflush(&mut self, page: VirtAddr) -> Result<PhysAddr, MapError> {
        let _migration_guard = migration_read_lock();
        self.unmap_page_inner(page, false)
    }

    unsafe fn translate(&self, addr: VirtAddr) -> Option<PhysAddr> {
        let phys_offset = self.phys_offset;
        let p4 = &self.p4;

        let p4_entry = &p4[addr.p4_index()];
        if !p4_entry.flags().contains(PageTableFlags::PRESENT) {
            return None;
        }

        let p3 =
            unsafe { &*Self::phys_to_virt(p4_entry.addr(), phys_offset).as_ptr::<PageTable>() };
        let p3_entry = &p3[addr.p3_index()];
        if !p3_entry.flags().contains(PageTableFlags::PRESENT) {
            return None;
        }

        if p3_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            // 1GiB page
            let offset = addr.as_u64() & 0x3fff_ffff;
            return Some(PhysAddr::new(p3_entry.addr().as_u64() + offset));
        }

        let p2 =
            unsafe { &*Self::phys_to_virt(p3_entry.addr(), phys_offset).as_ptr::<PageTable>() };
        let p2_entry = &p2[addr.p2_index()];
        if !p2_entry.flags().contains(PageTableFlags::PRESENT) {
            return None;
        }

        if p2_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            // 2MiB page
            let offset = addr.as_u64() & 0x1f_ffff;
            return Some(PhysAddr::new(p2_entry.addr().as_u64() + offset));
        }

        let p1 =
            unsafe { &*Self::phys_to_virt(p2_entry.addr(), phys_offset).as_ptr::<PageTable>() };
        let p1_entry = &p1[addr.p1_index()];
        if !p1_entry.flags().contains(PageTableFlags::PRESENT) {
            return None;
        }

        let offset = addr.as_u64() & 0xfff;
        Some(PhysAddr::new(p1_entry.addr().as_u64() + offset))
    }

    fn flush_all(&self) {
        tlb::flush_all();
        shootdown(self.p4_phys);
    }
}

impl<'a> OffsetMapper<'a> {
    unsafe fn unmap_page_inner(
        &mut self,
        page: VirtAddr,
        flush: bool,
    ) -> Result<PhysAddr, MapError> {
        use x86_64::structures::paging::PhysFrame;
        use x86_64::PhysAddr as X86PhysAddr;
        let phys_offset = self.phys_offset;
        let p4 = &mut self.p4;

        // We need to walk carefully, if any level is missing, we can't unmap
        let p4_entry = &mut p4[page.p4_index()];
        if !p4_entry.flags().contains(PageTableFlags::PRESENT) {
            return Err(MapError::PageNotMapped);
        }

        let p3 = &mut *Self::phys_to_virt(p4_entry.addr(), phys_offset).as_mut_ptr::<PageTable>();
        let p3_entry = &mut p3[page.p3_index()];
        if !p3_entry.flags().contains(PageTableFlags::PRESENT) {
            return Err(MapError::PageNotMapped);
        }

        if p3_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            // 1GiB page
            let frame = p3_entry.frame();
            p3_entry.set_unused();
            let p3_phys = Self::table_phys(p3, phys_offset);
            dec_table_count(p3_phys);
            if flush {
                tlb::flush(X86VirtAddr::new(page.as_u64()));
                shootdown(self.p4_phys);
            }
            if Self::table_empty(p3_phys, p3) {
                let p3_frame =
                    PhysFrame::containing_address(X86PhysAddr::new(p4_entry.addr().as_u64()));
                frame_allocator::deallocate_frame(p3_frame);
                p4_entry.set_unused();
                let p4_phys = Self::table_phys(p4, phys_offset);
                dec_table_count(p4_phys);
            }
            return Ok(frame);
        }

        let p2 = &mut *Self::phys_to_virt(p3_entry.addr(), phys_offset).as_mut_ptr::<PageTable>();
        let p2_entry = &mut p2[page.p2_index()];
        if !p2_entry.flags().contains(PageTableFlags::PRESENT) {
            return Err(MapError::PageNotMapped);
        }

        if p2_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            // 2MiB page
            let frame = p2_entry.frame();
            p2_entry.set_unused();
            let p2_phys = Self::table_phys(p2, phys_offset);
            dec_table_count(p2_phys);
            if flush {
                tlb::flush(X86VirtAddr::new(page.as_u64()));
                shootdown(self.p4_phys);
            }
            if Self::table_empty(p2_phys, p2) {
                let p2_frame =
                    PhysFrame::containing_address(X86PhysAddr::new(p3_entry.addr().as_u64()));
                frame_allocator::deallocate_frame(p2_frame);
                p3_entry.set_unused();
                let p3_phys = Self::table_phys(p3, phys_offset);
                dec_table_count(p3_phys);
                if Self::table_empty(p3_phys, p3) {
                    let p3_frame =
                        PhysFrame::containing_address(X86PhysAddr::new(p4_entry.addr().as_u64()));
                    frame_allocator::deallocate_frame(p3_frame);
                    p4_entry.set_unused();
                    let p4_phys = Self::table_phys(p4, phys_offset);
                    dec_table_count(p4_phys);
                }
            }
            return Ok(frame);
        }

        let p1 = &mut *Self::phys_to_virt(p2_entry.addr(), phys_offset).as_mut_ptr::<PageTable>();
        let p1_entry = &mut p1[page.p1_index()];
        if !p1_entry.flags().contains(PageTableFlags::PRESENT) {
            return Err(MapError::PageNotMapped);
        }

        let frame = p1_entry.frame();
        let old_flags = p1_entry.flags();
        p1_entry.set_unused();
        let p1_phys = Self::table_phys(p1, phys_offset);
        dec_table_count(p1_phys);
        if old_flags.contains(PageTableFlags::USER_ACCESSIBLE) {
            rmap_remove(frame, self.p4_phys, page);
        }

        if flush {
            tlb::flush(X86VirtAddr::new(page.as_u64()));
            shootdown(self.p4_phys);
        }

        if Self::table_empty(p1_phys, p1) {
            let p1_frame =
                PhysFrame::containing_address(X86PhysAddr::new(p2_entry.addr().as_u64()));
            frame_allocator::deallocate_frame(p1_frame);
            p2_entry.set_unused();
            let p2_phys = Self::table_phys(p2, phys_offset);
            dec_table_count(p2_phys);
            if Self::table_empty(p2_phys, p2) {
                let p2_frame =
                    PhysFrame::containing_address(X86PhysAddr::new(p3_entry.addr().as_u64()));
                frame_allocator::deallocate_frame(p2_frame);
                p3_entry.set_unused();
                let p3_phys = Self::table_phys(p3, phys_offset);
                dec_table_count(p3_phys);
                if Self::table_empty(p3_phys, p3) {
                    let p3_frame =
                        PhysFrame::containing_address(X86PhysAddr::new(p4_entry.addr().as_u64()));
                    frame_allocator::deallocate_frame(p3_frame);
                    p4_entry.set_unused();
                    let p4_phys = Self::table_phys(p4, phys_offset);
                    dec_table_count(p4_phys);
                }
            }
        }

        Ok(frame)
    }
}

impl<'a> OffsetMapper<'a> {
    pub unsafe fn migrate_page_nolock(
        &mut self,
        page: VirtAddr,
        new_frame: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<PhysAddr, MapError> {
        let old = self.unmap_page_inner(page, false)?;
        match self.map_page_inner(page, new_frame, flags, true) {
            Ok(()) => Ok(old),
            Err(e) => {
                // Best-effort rollback: restore old mapping.
                let _ = self.map_page_inner(page, old, flags, true);
                Err(e)
            }
        }
    }
}

pub unsafe fn mapper_for_p4(p4_phys: PhysAddr) -> Result<OffsetMapper<'static>, MapError> {
    let phys_offset = *crate::PHYS_OFFSET.get().ok_or(MapError::InvalidAccess)?;
    let p4_virt = VirtAddr::new(p4_phys.as_u64() + phys_offset);
    let p4 = &mut *p4_virt.as_mut_ptr::<PageTable>();
    Ok(OffsetMapper::new(p4, phys_offset, p4_phys))
}
