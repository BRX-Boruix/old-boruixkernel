use bitflags::bitflags;
use core::fmt;
use core::ops::{Index, IndexMut};
use core::sync::atomic::{AtomicU16, Ordering};

use crate::addr::PhysAddr;
use spin::Once;

bitflags! {
    /// A 64-bit page table entry flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct PageTableFlags: u64 {
        /// Specifies whether the mapped frame or page table is loaded in memory.
        const PRESENT = 1 << 0;
        /// Controls whether writes to the mapped frames are allowed.
        ///
        /// If this bit is unset in a level 1 page table entry, the mapped frame is read-only.
        /// If this bit is unset in a higher level page table entry the complete range of mapped
        /// pages is read-only.
        const WRITABLE = 1 << 1;
        /// Controls whether accesses from userspace (i.e. ring 3) are allowed.
        const USER_ACCESSIBLE = 1 << 2;
        /// If this bit is set, a "write-through" caching policy is used.
        /// In a write-through policy, data is written to both the cache and the main memory.
        const WRITE_THROUGH = 1 << 3;
        /// Disables caching for the pointed entry is cacheable.
        const NO_CACHE = 1 << 4;
        /// Set by the CPU when the mapped frame or page table is accessed.
        const ACCESSED = 1 << 5;
        /// Set by the CPU on a write to the mapped frame.
        const DIRTY = 1 << 6;
        /// Specifies that the entry maps a huge frame instead of a page table.
        /// Only allowed in P2 or P3 tables.
        const HUGE_PAGE = 1 << 7;
        /// Indicates that the mapping is global, typically used for kernel mappings.
        /// Global mappings are not flushed from the TLB on CR3 switch.
        const GLOBAL = 1 << 8;
        /// Forbid code execution from the mapped frame.
        /// Can be only used when the no-execute page protection feature is enabled in the EFER register.
        const NO_EXECUTE = 1 << 63;
    }
}

/// A 64-bit page table entry.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry {
    entry: u64,
}

impl PageTableEntry {
    /// Creates an unused page table entry.
    pub const fn new() -> Self {
        PageTableEntry { entry: 0 }
    }

    /// Returns whether this entry is zero.
    pub const fn is_unused(&self) -> bool {
        self.entry == 0
    }

    /// Sets this entry to zero.
    pub fn set_unused(&mut self) {
        self.entry = 0;
    }

    /// Returns the flags of this entry.
    pub const fn flags(&self) -> PageTableFlags {
        PageTableFlags::from_bits_truncate(self.entry)
    }

    /// Returns the physical address mapped by this entry, might be zero.
    pub const fn addr(&self) -> PhysAddr {
        PhysAddr::new(self.entry & 0x000f_ffff_ffff_f000)
    }

    /// Returns the physical address mapped by this entry, might be zero.
    pub const fn frame(&self) -> PhysAddr {
        self.addr()
    }

    /// Map the entry to the specified physical address with the specified flags.
    pub fn set_addr(&mut self, addr: PhysAddr, flags: PageTableFlags) {
        assert!(addr.is_aligned(4096));
        self.entry = (addr.as_u64()) | flags.bits();
    }

    pub fn set_flags(&mut self, flags: PageTableFlags) {
        self.entry = self.addr().as_u64() | flags.bits();
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut f = f.debug_struct("PageTableEntry");
        f.field("addr", &self.addr());
        f.field("flags", &self.flags());
        f.finish()
    }
}

/// The number of entries in a page table.
pub const ENTRY_COUNT: usize = 512;

/// A 64-bit page table.
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; ENTRY_COUNT],
}

#[derive(Clone, Copy)]
struct PageTableCountTable {
    base: *mut AtomicU16,
    len: usize,
}

unsafe impl Send for PageTableCountTable {}
unsafe impl Sync for PageTableCountTable {}

static PAGE_TABLE_COUNTS: Once<PageTableCountTable> = Once::new();

pub fn init_page_table_counts(base: *mut AtomicU16, len: usize) {
    let _ = PAGE_TABLE_COUNTS.call_once(|| PageTableCountTable { base, len });
}

fn counts() -> Option<PageTableCountTable> {
    PAGE_TABLE_COUNTS.get().copied()
}

fn pfn_of_table(phys: PhysAddr) -> usize {
    (phys.as_u64() as usize) / 4096
}

pub fn inc_table_count(phys: PhysAddr) {
    if let Some(c) = counts() {
        let idx = pfn_of_table(phys);
        if idx < c.len {
            unsafe {
                (*c.base.add(idx)).fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

pub fn dec_table_count(phys: PhysAddr) {
    if let Some(c) = counts() {
        let idx = pfn_of_table(phys);
        if idx < c.len {
            unsafe {
                (*c.base.add(idx)).fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}

pub fn table_count(phys: PhysAddr) -> Option<u16> {
    counts().and_then(|c| {
        let idx = pfn_of_table(phys);
        if idx < c.len {
            unsafe { Some((*c.base.add(idx)).load(Ordering::Relaxed)) }
        } else {
            None
        }
    })
}

impl PageTable {
    /// Creates an empty page table.
    pub const fn new() -> Self {
        const EMPTY: PageTableEntry = PageTableEntry::new();
        PageTable {
            entries: [EMPTY; ENTRY_COUNT],
        }
    }

    /// Clears all entries.
    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.set_unused();
        }
    }

    /// Returns an iterator over the entries of the page table.
    pub fn iter(&self) -> impl Iterator<Item = &PageTableEntry> {
        self.entries.iter()
    }

    /// Returns an iterator that allows modifying the entries of the page table.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut PageTableEntry> {
        self.entries.iter_mut()
    }

    pub fn used_count(&self) -> u16 {
        // Prefer side-table if available; fallback to scanning entries.
        if let Some(_) = counts() {
            // Caller should prefer table_count() with known phys address.
        }
        self.entries
            .iter()
            .filter(|e| e.flags().contains(PageTableFlags::PRESENT))
            .count() as u16
    }

    pub fn inc_used(&mut self) {
        // No-op: used_count is computed on demand to keep PageTable size at 4096 bytes.
    }

    pub fn dec_used(&mut self) {
        // No-op: used_count is computed on demand to keep PageTable size at 4096 bytes.
    }
}

impl Index<usize> for PageTable {
    type Output = PageTableEntry;

    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}

impl IndexMut<usize> for PageTable {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.entries[index]
    }
}

impl fmt::Debug for PageTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.entries[..].fmt(f)
    }
}
