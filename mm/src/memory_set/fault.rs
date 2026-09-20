use crate::addr::{PhysAddr, VirtAddr};
use crate::frame_allocator;
use crate::mapper::{MapError, Mapper};
use crate::page_table::PageTableFlags;
use crate::PHYS_OFFSET;

use super::address_space::MemorySet;

impl MemorySet {
    /// Handle a page fault by allocating a new frame if valid
    pub fn handle_page_fault(
        &mut self,
        addr: VirtAddr,
        error_code: x86_64::structures::idt::PageFaultErrorCode,
    ) -> Result<(), MapError> {
        use x86_64::structures::idt::PageFaultErrorCode;

        if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
            return Err(MapError::InvalidAccess);
        }

        let flags = {
            let area = match self.find_containing_area(addr) {
                Some(area) => area,
                None => {
                    if self.try_grow_stack(addr, error_code)? {
                        return Ok(());
                    }
                    return Err(MapError::InvalidAccess);
                }
            };

            if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE)
                && !area.flags.contains(PageTableFlags::WRITABLE)
            {
                return Err(MapError::InvalidAccess);
            }
            if error_code.contains(PageFaultErrorCode::USER_MODE)
                && !area.flags.contains(PageTableFlags::USER_ACCESSIBLE)
            {
                return Err(MapError::InvalidAccess);
            }
            if error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH)
                && area.flags.contains(PageTableFlags::NO_EXECUTE)
            {
                return Err(MapError::InvalidAccess);
            }

            area.flags
        };

        let page_addr = addr.align_down(4096);
        let frame = frame_allocator::allocate_frame().ok_or(MapError::FrameAllocationFailed)?;
        let phys = PhysAddr::new(frame.start_address().as_u64());

        // Zero the new frame to prevent data leaks
        let phys_offset = *PHYS_OFFSET.get().ok_or(MapError::InvalidAccess)?;
        let virt = VirtAddr::new(phys.as_u64() + phys_offset);
        unsafe {
            virt.as_mut_ptr::<u8>().write_bytes(0, 4096);
        }

        unsafe {
            let mut mapper = self.mapper().map_err(|_| MapError::InvalidAccess)?;
            mapper.map_page(page_addr, phys, flags | PageTableFlags::PRESENT)?;
        }

        Ok(())
    }

    fn try_grow_stack(
        &mut self,
        addr: VirtAddr,
        error_code: x86_64::structures::idt::PageFaultErrorCode,
    ) -> Result<bool, MapError> {
        use x86_64::structures::idt::PageFaultErrorCode;
        if !error_code.contains(PageFaultErrorCode::USER_MODE) {
            return Ok(false);
        }

        let (stack_bottom, stack_top, stack_limit) = match (
            self.stack_bottom,
            self.stack_top,
            self.stack_limit,
        ) {
            (Some(bottom), Some(top), Some(limit)) => (bottom, top, limit),
            _ => return Ok(false),
        };

        if addr >= stack_top || addr < stack_limit || addr >= stack_bottom {
            return Ok(false);
        }

        let page_addr = addr.align_down(4096);
        if page_addr < stack_limit {
            return Ok(false);
        }

        let flags =
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

        let frame = frame_allocator::allocate_frame().ok_or(MapError::FrameAllocationFailed)?;
        let phys = PhysAddr::new(frame.start_address().as_u64());

        let phys_offset = *PHYS_OFFSET.get().ok_or(MapError::InvalidAccess)?;
        let virt = VirtAddr::new(phys.as_u64() + phys_offset);
        unsafe {
            virt.as_mut_ptr::<u8>().write_bytes(0, 4096);
        }

        unsafe {
            let mut mapper = self.mapper().map_err(|_| MapError::InvalidAccess)?;
            if let Err(e) = mapper.map_page(page_addr, phys, flags) {
                return Err(e);
            }
        }

        if self
            .insert_area(super::vma_tree::MemoryArea::new(page_addr, stack_bottom, flags))
            .is_err()
        {
            unsafe {
                let mut mapper = self.mapper().map_err(|_| MapError::InvalidAccess)?;
                if let Ok(p) = mapper.unmap_page(page_addr) {
                    let frame = x86_64::structures::paging::PhysFrame::containing_address(
                        x86_64::PhysAddr::new(p.as_u64()),
                    );
                    frame_allocator::deallocate_frame(frame);
                }
            }
            return Err(MapError::InvalidAccess);
        }

        self.stack_bottom = Some(page_addr);

        Ok(true)
    }
}
