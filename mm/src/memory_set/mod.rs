mod address_space;
mod elf_loader;
mod fault;
mod vma_tree;

pub const STACK_MAX_SIZE: u64 = 8 * 1024 * 1024;

pub use address_space::MemorySet;
pub use vma_tree::{reset_vma_stats, vma_stats, Backing, MemoryArea, VmaStats, VmaTreeStats};
