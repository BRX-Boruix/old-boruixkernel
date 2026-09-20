use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::addr::VirtAddr;
use crate::mapper::MapError;
use crate::page_table::PageTableFlags;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryArea {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub flags: PageTableFlags,
    pub backing: Backing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backing {
    Anonymous,
    File,
}

impl MemoryArea {
    pub fn new(start: VirtAddr, end: VirtAddr, flags: PageTableFlags) -> Self {
        Self {
            start,
            end,
            flags,
            backing: Backing::Anonymous,
        }
    }

    pub fn size(&self) -> u64 {
        self.end.as_u64() - self.start.as_u64()
    }

    pub fn contains(&self, addr: VirtAddr) -> bool {
        addr.as_u64() >= self.start.as_u64() && addr.as_u64() < self.end.as_u64()
    }
}

struct IntervalNode {
    key: u64,
    area: MemoryArea,
    max_end: u64,
    prio: u64,
    left: Option<Box<IntervalNode>>,
    right: Option<Box<IntervalNode>>,
}

impl IntervalNode {
    fn new(area: MemoryArea) -> Self {
        let key = area.start.as_u64();
        let end = area.end.as_u64();
        Self {
            key,
            area,
            max_end: end,
            prio: splitmix64(key),
            left: None,
            right: None,
        }
    }

    fn recalc(&mut self) {
        let mut max_end = self.area.end.as_u64();
        if let Some(ref l) = self.left {
            if l.max_end > max_end {
                max_end = l.max_end;
            }
        }
        if let Some(ref r) = self.right {
            if r.max_end > max_end {
                max_end = r.max_end;
            }
        }
        self.max_end = max_end;
    }
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn merge(a: Option<Box<IntervalNode>>, b: Option<Box<IntervalNode>>) -> Option<Box<IntervalNode>> {
    match (a, b) {
        (None, r) => r,
        (l, None) => l,
        (Some(mut l), Some(mut r)) => {
            if l.prio >= r.prio {
                l.right = merge(l.right.take(), Some(r));
                l.recalc();
                Some(l)
            } else {
                r.left = merge(Some(l), r.left.take());
                r.recalc();
                Some(r)
            }
        }
    }
}

fn split(
    root: Option<Box<IntervalNode>>,
    key: u64,
) -> (Option<Box<IntervalNode>>, Option<Box<IntervalNode>>) {
    match root {
        None => (None, None),
        Some(mut node) => {
            if key <= node.key {
                let (l, r) = split(node.left.take(), key);
                node.left = r;
                node.recalc();
                (l, Some(node))
            } else {
                let (l, r) = split(node.right.take(), key);
                node.right = l;
                node.recalc();
                (Some(node), r)
            }
        }
    }
}

fn insert_node(
    root: Option<Box<IntervalNode>>,
    node: Box<IntervalNode>,
) -> Option<Box<IntervalNode>> {
    if root.is_none() {
        return Some(node);
    }
    let (l, r) = split(root, node.key);
    let merged = merge(l, Some(node));
    merge(merged, r)
}

fn remove_node(
    root: Option<Box<IntervalNode>>,
    key: u64,
) -> (Option<Box<IntervalNode>>, Option<MemoryArea>) {
    let (l, r) = split(root, key);
    if key == u64::MAX {
        let area = r.map(|n| n.area);
        return (l, area);
    }
    let (mid, r2) = split(r, key + 1);
    let area = mid.map(|n| n.area);
    (merge(l, r2), area)
}

fn find_prev(root: &Option<Box<IntervalNode>>, key: u64) -> Option<MemoryArea> {
    let mut cur = root.as_ref();
    let mut res: Option<MemoryArea> = None;
    while let Some(node) = cur {
        if key < node.key {
            cur = node.left.as_ref();
        } else {
            res = Some(node.area);
            cur = node.right.as_ref();
        }
    }
    res
}

fn find_next(root: &Option<Box<IntervalNode>>, key: u64) -> Option<MemoryArea> {
    let mut cur = root.as_ref();
    let mut res: Option<MemoryArea> = None;
    while let Some(node) = cur {
        if key <= node.key {
            res = Some(node.area);
            cur = node.left.as_ref();
        } else {
            cur = node.right.as_ref();
        }
    }
    res
}

fn find_containing(root: &Option<Box<IntervalNode>>, addr: u64) -> Option<MemoryArea> {
    let mut cur = root.as_ref();
    while let Some(node) = cur {
        if let Some(left) = node.left.as_ref() {
            if left.max_end > addr {
                cur = node.left.as_ref();
                continue;
            }
        }
        if addr < node.area.start.as_u64() {
            cur = node.right.as_ref();
            continue;
        }
        if addr >= node.area.end.as_u64() {
            cur = node.right.as_ref();
            continue;
        }
        return Some(node.area);
    }
    None
}

fn for_each_in_order(root: &Option<Box<IntervalNode>>, mut f: impl FnMut(&MemoryArea)) {
    let mut stack: Vec<&IntervalNode> = Vec::new();
    let mut cur = root.as_deref();
    while cur.is_some() || !stack.is_empty() {
        while let Some(node) = cur {
            stack.push(node);
            cur = node.left.as_deref();
        }
        let node = stack.pop().unwrap();
        f(&node.area);
        cur = node.right.as_deref();
    }
}

fn max_depth(root: &Option<Box<IntervalNode>>) -> usize {
    let mut max = 0usize;
    if root.is_none() {
        return 0;
    }
    let mut stack: Vec<(&IntervalNode, usize)> = Vec::new();
    if let Some(node) = root.as_deref() {
        stack.push((node, 1));
    }
    while let Some((node, depth)) = stack.pop() {
        if depth > max {
            max = depth;
        }
        if let Some(ref l) = node.left {
            stack.push((l, depth + 1));
        }
        if let Some(ref r) = node.right {
            stack.push((r, depth + 1));
        }
    }
    max
}

fn node_count(root: &Option<Box<IntervalNode>>) -> usize {
    let mut count = 0usize;
    let mut stack: Vec<&IntervalNode> = Vec::new();
    let mut cur = root.as_deref();
    while cur.is_some() || !stack.is_empty() {
        while let Some(node) = cur {
            stack.push(node);
            cur = node.left.as_deref();
        }
        let node = stack.pop().unwrap();
        count += 1;
        cur = node.right.as_deref();
    }
    count
}

pub struct VmaTreeStats {
    pub node_count: usize,
    pub max_depth: usize,
}

fn vma_tree_stats(root: &Option<Box<IntervalNode>>) -> VmaTreeStats {
    VmaTreeStats {
        node_count: node_count(root),
        max_depth: max_depth(root),
    }
}

static VMA_INSERTS: AtomicUsize = AtomicUsize::new(0);
static VMA_MERGES: AtomicUsize = AtomicUsize::new(0);
static VMA_OVERLAP_REJECTS: AtomicUsize = AtomicUsize::new(0);

pub struct VmaStats {
    pub inserts: usize,
    pub merges: usize,
    pub overlap_rejects: usize,
}

pub fn vma_stats() -> VmaStats {
    VmaStats {
        inserts: VMA_INSERTS.load(Ordering::Relaxed),
        merges: VMA_MERGES.load(Ordering::Relaxed),
        overlap_rejects: VMA_OVERLAP_REJECTS.load(Ordering::Relaxed),
    }
}

pub fn reset_vma_stats() {
    VMA_INSERTS.store(0, Ordering::Relaxed);
    VMA_MERGES.store(0, Ordering::Relaxed);
    VMA_OVERLAP_REJECTS.store(0, Ordering::Relaxed);
}

pub(crate) struct VmaTree {
    root: Option<Box<IntervalNode>>,
}

impl VmaTree {
    pub(crate) fn new() -> Self {
        Self { root: None }
    }

    pub(crate) fn vma_tree_stats(&self) -> VmaTreeStats {
        vma_tree_stats(&self.root)
    }

    pub(crate) fn insert_area(&mut self, mut area: MemoryArea) -> Result<(), MapError> {
        if area.start.as_u64() >= area.end.as_u64() {
            return Err(MapError::InvalidAccess);
        }

        VMA_INSERTS.fetch_add(1, Ordering::Relaxed);
        loop {
            let mut merged = false;
            if let Some(prev) = find_prev(&self.root, area.start.as_u64()) {
                if prev.end.as_u64() > area.start.as_u64() {
                    VMA_OVERLAP_REJECTS.fetch_add(1, Ordering::Relaxed);
                    return Err(MapError::InvalidAccess);
                }
                if prev.end.as_u64() == area.start.as_u64() && prev.flags == area.flags {
                    let (new_root, _) = remove_node(self.root.take(), prev.start.as_u64());
                    self.root = new_root;
                    area.start = prev.start;
                    VMA_MERGES.fetch_add(1, Ordering::Relaxed);
                    merged = true;
                }
            }
            if let Some(next) = find_next(&self.root, area.start.as_u64()) {
                if next.start.as_u64() < area.end.as_u64() {
                    VMA_OVERLAP_REJECTS.fetch_add(1, Ordering::Relaxed);
                    return Err(MapError::InvalidAccess);
                }
                if next.start.as_u64() == area.end.as_u64() && next.flags == area.flags {
                    let (new_root, _) = remove_node(self.root.take(), next.start.as_u64());
                    self.root = new_root;
                    area.end = next.end;
                    VMA_MERGES.fetch_add(1, Ordering::Relaxed);
                    merged = true;
                }
            }
            if !merged {
                break;
            }
        }

        self.root = insert_node(self.root.take(), Box::new(IntervalNode::new(area)));
        Ok(())
    }

    pub(crate) fn find_next(&self, key: u64) -> Option<MemoryArea> {
        find_next(&self.root, key)
    }

    pub(crate) fn find_containing(&self, addr: u64) -> Option<MemoryArea> {
        find_containing(&self.root, addr)
    }

    pub(crate) fn for_each_in_order(&self, f: impl FnMut(&MemoryArea)) {
        for_each_in_order(&self.root, f)
    }

    pub(crate) fn debug_vma_lookup(
        &self,
        addr: VirtAddr,
    ) -> (Option<MemoryArea>, Option<MemoryArea>, Option<MemoryArea>) {
        let cur = addr.as_u64();
        let containing = find_containing(&self.root, cur);
        let prev = find_prev(&self.root, cur);
        let next = find_next(&self.root, cur);
        (containing, prev, next)
    }

    pub(crate) fn clear(&mut self) {
        self.root = None;
    }
}
