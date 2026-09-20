#![no_std]

/// The main name of the system.
pub const MAIN_NAME: &str = "Boruix";

/// The name of the kernel.
/// This constant is used throughout the kernel to display the system name.
pub const KERNEL_NAME: &str = "Boruix Kernel";

/// The version of the kernel.
pub const KERNEL_VERSION: &str = "0.1.0";

/// Enable interrupt-time preemption (timer/IPI) once preemption is enabled.
pub const ENABLE_INTERRUPT_PREEMPT: bool = false;

/// Kernel heap start address (virtual)
/// We choose a high address range that doesn't conflict with other mappings
pub const KERNEL_HEAP_START: u64 = 0xFFFF_FF00_0000_0000;
/// Kernel heap size (initial size)
pub const KERNEL_HEAP_SIZE: u64 = 16 * 1024 * 1024; // 16 MiB
/// Kernel heap max size (reserved range)
pub const KERNEL_HEAP_MAX: u64 = 64 * 1024 * 1024; // 64 MiB
