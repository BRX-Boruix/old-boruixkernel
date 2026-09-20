use crate::page_table::PageTableFlags;
use crate::{PhysAddr, VirtAddr};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RmapEntry {
    pub p4_phys: PhysAddr,
    pub vaddr: VirtAddr,
    pub flags: PageTableFlags,
}

static RMAP: Mutex<BTreeMap<u64, Vec<RmapEntry>>> = Mutex::new(BTreeMap::new());
static MIGRATION_LOCK: RwLock<()> = RwLock::new(());

static RMAP_ADD: AtomicUsize = AtomicUsize::new(0);
static RMAP_REMOVE: AtomicUsize = AtomicUsize::new(0);
static RMAP_LOOKUP: AtomicUsize = AtomicUsize::new(0);
static MIG_READ: AtomicUsize = AtomicUsize::new(0);
static MIG_WRITE: AtomicUsize = AtomicUsize::new(0);

pub struct RmapStats {
    pub add: usize,
    pub remove: usize,
    pub lookup: usize,
    pub mig_read: usize,
    pub mig_write: usize,
}

pub fn migration_read_lock<'a>() -> RwLockReadGuard<'a, ()> {
    MIG_READ.fetch_add(1, Ordering::Relaxed);
    MIGRATION_LOCK.read()
}

pub fn migration_write_lock<'a>() -> RwLockWriteGuard<'a, ()> {
    MIG_WRITE.fetch_add(1, Ordering::Relaxed);
    MIGRATION_LOCK.write()
}

pub fn rmap_add(phys: PhysAddr, p4_phys: PhysAddr, vaddr: VirtAddr, flags: PageTableFlags) {
    RMAP_ADD.fetch_add(1, Ordering::Relaxed);
    let mut map = RMAP.lock();
    let key = phys.as_u64();
    let entry = RmapEntry {
        p4_phys,
        vaddr,
        flags,
    };
    map.entry(key).or_insert_with(Vec::new).push(entry);
}

pub fn rmap_remove(phys: PhysAddr, p4_phys: PhysAddr, vaddr: VirtAddr) {
    RMAP_REMOVE.fetch_add(1, Ordering::Relaxed);
    let mut map = RMAP.lock();
    let key = phys.as_u64();
    if let Some(vec) = map.get_mut(&key) {
        vec.retain(|e| !(e.p4_phys == p4_phys && e.vaddr == vaddr));
        if vec.is_empty() {
            map.remove(&key);
        }
    }
}

pub fn rmap_lookup(phys: PhysAddr) -> Vec<RmapEntry> {
    RMAP_LOOKUP.fetch_add(1, Ordering::Relaxed);
    let map = RMAP.lock();
    map.get(&phys.as_u64()).cloned().unwrap_or_default()
}

pub fn rmap_any() -> Option<(PhysAddr, RmapEntry)> {
    let map = RMAP.lock();
    let (key, vec) = map.iter().next()?;
    let entry = *vec.first()?;
    Some((PhysAddr::new(*key), entry))
}

pub fn rmap_count() -> usize {
    let map = RMAP.lock();
    map.values().map(|v| v.len()).sum()
}

pub fn rmap_stats() -> RmapStats {
    RmapStats {
        add: RMAP_ADD.load(Ordering::Relaxed),
        remove: RMAP_REMOVE.load(Ordering::Relaxed),
        lookup: RMAP_LOOKUP.load(Ordering::Relaxed),
        mig_read: MIG_READ.load(Ordering::Relaxed),
        mig_write: MIG_WRITE.load(Ordering::Relaxed),
    }
}

pub fn reset_rmap_stats() {
    RMAP_ADD.store(0, Ordering::Relaxed);
    RMAP_REMOVE.store(0, Ordering::Relaxed);
    RMAP_LOOKUP.store(0, Ordering::Relaxed);
    MIG_READ.store(0, Ordering::Relaxed);
    MIG_WRITE.store(0, Ordering::Relaxed);
}
