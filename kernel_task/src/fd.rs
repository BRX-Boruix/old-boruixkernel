use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdError {
    Invalid,
    NotFound,
    NoSpace,
    Io,
}

pub type FdResult<T = ()> = core::result::Result<T, FdError>;

pub trait FileOps: Send + Sync {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> FdResult<usize> {
        Err(FdError::Io)
    }
    fn write(&self, _offset: usize, _buf: &[u8]) -> FdResult<usize> {
        Err(FdError::Io)
    }
    fn ioctl(&self, _req: usize, _arg: usize) -> FdResult {
        Err(FdError::Io)
    }
    fn poll(&self, _events: u32) -> FdResult<u32> {
        Err(FdError::Io)
    }
    fn size(&self) -> Option<u64> {
        None
    }
    fn stat(&self) -> Option<FileStat> {
        None
    }
    fn as_any(&self) -> &dyn Any;
}

#[derive(Debug, Clone, Copy)]
pub struct FileStat {
    pub size: u64,
    pub mode: u16,
    pub node_type: u32,
}

pub struct FileHandle {
    pub node: Arc<dyn FileOps>,
    pub flags: AtomicU32,
    pub offset: Mutex<usize>,
    pub refcount: AtomicUsize,
}

impl FileHandle {
    pub fn new(node: Arc<dyn FileOps>, flags: u32) -> Self {
        Self {
            node,
            flags: AtomicU32::new(flags),
            offset: Mutex::new(0),
            refcount: AtomicUsize::new(1),
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> FdResult<usize> {
        let mut off = self.offset.lock();
        let n = self.node.read(*off, buf)?;
        *off += n;
        Ok(n)
    }

    pub fn write(&self, buf: &[u8]) -> FdResult<usize> {
        let mut off = self.offset.lock();
        let n = self.node.write(*off, buf)?;
        *off += n;
        Ok(n)
    }

    pub fn seek(&self, offset: usize) -> FdResult {
        *self.offset.lock() = offset;
        Ok(())
    }

    pub fn fstat(&self) -> Option<FileStat> {
        self.node.stat()
    }

    pub fn set_flags(&self, flags: u32) {
        self.flags.store(flags, Ordering::Relaxed);
    }

    pub fn get_flags(&self) -> u32 {
        self.flags.load(Ordering::Relaxed)
    }
}

pub struct FdTable {
    entries: Vec<Option<Arc<FileHandle>>>,
    cloexec: Vec<bool>,
}

impl FdTable {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            cloexec: Vec::new(),
        }
    }

    fn ensure_len(&mut self, len: usize) {
        if self.entries.len() < len {
            self.entries.resize_with(len, || None);
            self.cloexec.resize(len, false);
        }
    }

    pub fn alloc_fd(&mut self, min_fd: usize, handle: Arc<FileHandle>, cloexec: bool) -> FdResult<usize> {
        if min_fd > 1_000_000 {
            return Err(FdError::Invalid);
        }
        let mut idx = min_fd;
        loop {
            self.ensure_len(idx + 1);
            if self.entries[idx].is_none() {
                self.entries[idx] = Some(handle);
                self.cloexec[idx] = cloexec;
                return Ok(idx);
            }
            idx += 1;
            if idx > 1_000_000 {
                return Err(FdError::NoSpace);
            }
        }
    }

    pub fn get_fd(&self, fd: usize) -> Option<Arc<FileHandle>> {
        self.entries.get(fd).and_then(|e| e.as_ref().cloned())
    }

    pub fn close_fd(&mut self, fd: usize) -> FdResult {
        let Some(entry) = self.entries.get_mut(fd) else { return Err(FdError::NotFound) };
        if let Some(handle) = entry.take() {
            handle.refcount.fetch_sub(1, Ordering::Relaxed);
        }
        if let Some(flag) = self.cloexec.get_mut(fd) {
            *flag = false;
        }
        Ok(())
    }

    pub fn dup(&mut self, fd: usize, min_fd: usize, cloexec: bool) -> FdResult<usize> {
        let handle = self.get_fd(fd).ok_or(FdError::NotFound)?;
        handle.refcount.fetch_add(1, Ordering::Relaxed);
        self.alloc_fd(min_fd, handle, cloexec)
    }

    pub fn dup2(&mut self, oldfd: usize, newfd: usize, cloexec: bool) -> FdResult<usize> {
        let handle = self.get_fd(oldfd).ok_or(FdError::NotFound)?;
        self.ensure_len(newfd + 1);
        if oldfd == newfd {
            self.cloexec[newfd] = cloexec;
            return Ok(newfd);
        }
        if self.entries[newfd].is_some() {
            self.close_fd(newfd)?;
        }
        handle.refcount.fetch_add(1, Ordering::Relaxed);
        self.entries[newfd] = Some(handle);
        self.cloexec[newfd] = cloexec;
        Ok(newfd)
    }

    pub fn dup3(&mut self, oldfd: usize, newfd: usize, cloexec: bool) -> FdResult<usize> {
        if oldfd == newfd {
            return Err(FdError::Invalid);
        }
        self.dup2(oldfd, newfd, cloexec)
    }

    pub fn fork_clone(&self) -> Self {
        let mut entries = Vec::with_capacity(self.entries.len());
        for e in &self.entries {
            if let Some(h) = e {
                h.refcount.fetch_add(1, Ordering::Relaxed);
                entries.push(Some(h.clone()));
            } else {
                entries.push(None);
            }
        }
        Self {
            entries,
            cloexec: self.cloexec.clone(),
        }
    }

    pub fn close_on_exec(&mut self) {
        for i in 0..self.entries.len() {
            if self.cloexec.get(i) == Some(&true) {
                let _ = self.close_fd(i);
            }
        }
    }

    pub fn close_all(&mut self) {
        for i in 0..self.entries.len() {
            let _ = self.close_fd(i);
        }
    }

    pub fn is_cloexec(&self, fd: usize) -> bool {
        self.cloexec.get(fd).copied().unwrap_or(false)
    }

    pub fn set_cloexec(&mut self, fd: usize, enabled: bool) {
        if fd < self.cloexec.len() {
            self.cloexec[fd] = enabled;
        }
    }
}
