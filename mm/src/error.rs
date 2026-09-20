use core::fmt;

use crate::mapper::MapError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Recoverable,
    Fatal,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::Recoverable => write!(f, "recoverable"),
            ErrorKind::Fatal => write!(f, "fatal"),
        }
    }
}

#[derive(Debug)]
pub enum MmError {
    InvalidElf,
    NotExecutable,
    Not64Bit,
    InvalidElfMagic,
    PhysOffsetMissing,
    FrameAllocationFailed,
    Map(MapError),
    AreaOverlap,
    TranslationFailed,
    MapFailed,
}

impl MmError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            MmError::PhysOffsetMissing => ErrorKind::Fatal,
            MmError::FrameAllocationFailed => ErrorKind::Recoverable,
            MmError::Map(MapError::InvalidAccess) => ErrorKind::Recoverable,
            MmError::Map(_) => ErrorKind::Recoverable,
            MmError::InvalidElf
            | MmError::NotExecutable
            | MmError::Not64Bit
            | MmError::InvalidElfMagic
            | MmError::AreaOverlap
            | MmError::TranslationFailed
            | MmError::MapFailed => ErrorKind::Recoverable,
        }
    }
}

impl fmt::Display for MmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MmError::InvalidElf => write!(f, "invalid ELF"),
            MmError::NotExecutable => write!(f, "ELF is not executable"),
            MmError::Not64Bit => write!(f, "ELF is not 64-bit"),
            MmError::InvalidElfMagic => write!(f, "ELF magic mismatch"),
            MmError::PhysOffsetMissing => write!(f, "PHYS_OFFSET not initialized"),
            MmError::FrameAllocationFailed => write!(f, "frame allocation failed"),
            MmError::Map(e) => write!(f, "page map error: {:?}", e),
            MmError::AreaOverlap => write!(f, "VMA overlap"),
            MmError::TranslationFailed => write!(f, "address translation failed"),
            MmError::MapFailed => write!(f, "map operation failed"),
        }
    }
}
