use alloc::vec::Vec;
use xmas_elf::{program::Type, ElfFile};

use crate::addr::{PhysAddr, VirtAddr};
use crate::error::MmError;
use crate::frame_allocator;
use crate::mapper::Mapper;
use crate::page_table::PageTableFlags;
use crate::PHYS_OFFSET;

use super::address_space::MemorySet;
use super::STACK_MAX_SIZE;
use super::vma_tree::MemoryArea;

impl MemorySet {
    /// Create a MemorySet from ELF data
    pub fn from_elf(elf_data: &[u8]) -> Result<(Self, u64, u64, u64, u64), MmError> {
        let elf = ElfFile::new(elf_data).map_err(|_| MmError::InvalidElf)?;
        let elf_header = elf.header;

        let magic = elf_header.pt1.magic;
        if magic != [0x7f, 0x45, 0x4c, 0x46] {
            return Err(MmError::InvalidElfMagic);
        }
        if elf_header.pt1.class() != xmas_elf::header::Class::SixtyFour {
            return Err(MmError::Not64Bit);
        }
        if elf_header.pt2.type_().as_type() != xmas_elf::header::Type::Executable {
            return Err(MmError::NotExecutable);
        }

        let mut memory_set = Self::new_bare()?;
        let phys_offset = *PHYS_OFFSET.get().ok_or(MmError::PhysOffsetMissing)?;

        let mut max_end: u64 = 0;
        // Iterate program headers
        for ph in elf.program_iter() {
            if let Ok(Type::Load) = ph.get_type() {
                let start_va = VirtAddr::new(ph.virtual_addr());
                let end_va = VirtAddr::new(ph.virtual_addr() + ph.mem_size());
                if end_va.as_u64() > max_end {
                    max_end = end_va.as_u64();
                }
                let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;

                let ph_flags = ph.flags();
                if ph_flags.is_write() {
                    flags |= PageTableFlags::WRITABLE;
                }
                if !ph_flags.is_execute() {
                    flags |= PageTableFlags::NO_EXECUTE;
                }

                let start_page = start_va.align_down(4096u64);
                let end_page = end_va.align_up(4096u64);

                // Allocate and map pages (batched TLB flush)
                let mut mapped: Vec<(VirtAddr, PhysAddr)> = Vec::new();
                {
                    let mut mapper = unsafe { memory_set.mapper()? };
                    let mut current_page = start_page;
                    while current_page < end_page {
                        let frame = frame_allocator::allocate_frame()
                            .ok_or(MmError::FrameAllocationFailed)?;
                        let phys_addr = PhysAddr::new(frame.start_address().as_u64());
                        if phys_addr.as_u64() == 0 {
                            panic!("from_elf: allocator returned paddr 0");
                        }

                        // Zero the frame first
                        let virt_addr = VirtAddr::new(phys_addr.as_u64() + phys_offset);
                        // SAFETY: `virt_addr` is the HHDM mapping of a freshly allocated frame.
                        unsafe {
                            virt_addr.as_mut_ptr::<u8>().write_bytes(0, 4096);
                        }

                        // SAFETY: mapper is exclusive for this MemorySet; mapping new frame.
                        unsafe {
                            if let Err(e) = mapper.map_page_noflush(current_page, phys_addr, flags)
                            {
                                logger::error!("Map failed at {:?}: {:?}", current_page, e);
                                rollback_mapped(&mut mapper, &mapped);
                                return Err(MmError::MapFailed);
                            }
                        }
                        mapped.push((current_page, phys_addr));

                        current_page += 4096u64;
                    }
                    mapper.flush_all();

                    // Copy data
                    let file_size = ph.file_size();
                    let file_offset = ph.offset();

                    let mut current_va = start_va;
                    while current_va < end_va {
                        let page_start = current_va.align_down(4096u64);
                        let page_end = page_start + 4096u64;

                        let phys = match unsafe { mapper.translate(page_start) } {
                            Some(p) => p,
                            None => {
                                rollback_mapped(&mut mapper, &mapped);
                                return Err(MmError::TranslationFailed);
                            }
                        };
                        let dst_base = (phys.as_u64() + phys_offset) as *mut u8;

                        let chunk_start = if current_va > page_start {
                            current_va
                        } else {
                            page_start
                        };
                        let chunk_end = if end_va < page_end { end_va } else { page_end };
                        let chunk_len = (chunk_end.as_u64() - chunk_start.as_u64()) as usize;
                        let dst_offset = (chunk_start.as_u64() - page_start.as_u64()) as usize;

                        let segment_offset = chunk_start.as_u64() - start_va.as_u64();

                        if segment_offset < file_size {
                            let copy_len = if segment_offset + chunk_len as u64 > file_size {
                                (file_size - segment_offset) as usize
                            } else {
                                chunk_len
                            };

                            let src_offset = (file_offset + segment_offset) as usize;
                            // SAFETY: source ELF slice is valid; destination is within mapped frame.
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    elf_data.as_ptr().add(src_offset),
                                    dst_base.add(dst_offset),
                                    copy_len,
                                );
                            }
                        }

                        current_va = chunk_end;
                    }
                }

                if memory_set
                    .insert_area(MemoryArea::new(start_page, end_page, flags))
                    .is_err()
                {
                    let mut mapper = unsafe { memory_set.mapper()? };
                    rollback_mapped(&mut mapper, &mapped);
                    return Err(MmError::AreaOverlap);
                }
            }
        }

        // Map user stack
        let stack_top = VirtAddr::new(0x0000_7fff_ffff_f000); // Below 128TB
        let stack_bottom = VirtAddr::new(0x0000_7fff_fffB_F000); // 256KB stack
        let stack_flags =
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

        let mut mapped_stack: Vec<(VirtAddr, PhysAddr)> = Vec::new();
        {
            let mut mapper = unsafe { memory_set.mapper()? };
            let mut current_page = stack_bottom;
            while current_page < stack_top {
                let frame =
                    frame_allocator::allocate_frame().ok_or(MmError::FrameAllocationFailed)?;
                let phys_addr = PhysAddr::new(frame.start_address().as_u64());
                let virt_addr = VirtAddr::new(phys_addr.as_u64() + phys_offset);
                // SAFETY: `virt_addr` is the HHDM mapping of a freshly allocated frame.
                unsafe {
                    virt_addr.as_mut_ptr::<u8>().write_bytes(0, 4096);
                }

                // SAFETY: mapper is exclusive for this MemorySet; mapping new frame.
                unsafe {
                    match mapper.map_page_noflush(current_page, phys_addr, stack_flags) {
                        Ok(_) => {}
                        Err(e) => {
                            logger::error!("Failed to map stack page {:?}: {:?}", current_page, e);
                            rollback_mapped(&mut mapper, &mapped_stack);
                            return Err(MmError::MapFailed);
                        }
                    }
                }
                mapped_stack.push((current_page, phys_addr));
                current_page += 4096u64;
            }
            mapper.flush_all();
        }
        if memory_set
            .insert_area(MemoryArea::new(stack_bottom, stack_top, stack_flags))
            .is_err()
        {
            let mut mapper = unsafe { memory_set.mapper()? };
            rollback_mapped(&mut mapper, &mapped_stack);
            return Err(MmError::AreaOverlap);
        }
        memory_set.stack_top = Some(stack_top);
        memory_set.stack_bottom = Some(stack_bottom);
        let stack_limit = stack_top
            .as_u64()
            .saturating_sub(STACK_MAX_SIZE)
            .max(0x1000);
        memory_set.stack_limit = Some(VirtAddr::new(stack_limit));

        let mut heap_start = 0u64;
        let mut heap_end = 0u64;
        for sec in elf.section_iter() {
            if let Ok(name) = sec.get_name(&elf) {
                if name == ".heap" {
                    heap_start = sec.address();
                    heap_end = sec.address() + sec.size();
                    break;
                }
            }
        }
        if heap_end == 0 {
            let base = VirtAddr::new(max_end).align_up(4096u64).as_u64();
            heap_start = base;
            heap_end = base;
        }

        Ok((
            memory_set,
            elf.header.pt2.entry_point(),
            stack_top.as_u64(),
            heap_start,
            heap_end,
        ))
    }
}

fn rollback_mapped(mapper: &mut crate::mapper::OffsetMapper<'_>, mapped: &[(VirtAddr, PhysAddr)]) {
    use x86_64::structures::paging::PhysFrame;
    use x86_64::PhysAddr as X86PhysAddr;
    for (page, phys) in mapped.iter().rev() {
        unsafe {
            if mapper.unmap_page_noflush(*page).is_ok() {
                let frame = PhysFrame::containing_address(X86PhysAddr::new(phys.as_u64()));
                crate::frame_allocator::deallocate_frame(frame);
            }
        }
    }
    mapper.flush_all();
}
