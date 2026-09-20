use core::fmt;

use kernel_platform::memory::addr_space::{ErrorKind, MmError};

#[derive(Debug)]
pub enum TaskError {
    Memory(MmError),
    KernelStackLayout,
    KernelStackAlloc,
    InvalidTrapFrame,
}

impl TaskError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            TaskError::Memory(e) => e.kind(),
            TaskError::KernelStackLayout
            | TaskError::KernelStackAlloc
            | TaskError::InvalidTrapFrame => ErrorKind::Fatal,
        }
    }
}

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskError::Memory(e) => write!(f, "memory error: {}", e),
            TaskError::KernelStackLayout => write!(f, "invalid kernel stack layout"),
            TaskError::KernelStackAlloc => write!(f, "kernel stack allocation failed"),
            TaskError::InvalidTrapFrame => write!(f, "invalid trap frame stack pointer"),
        }
    }
}
