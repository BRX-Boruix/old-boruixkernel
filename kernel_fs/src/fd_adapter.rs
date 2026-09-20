use alloc::sync::Arc;

use kernel_task::fd::{FdError, FdResult, FileOps, FileStat};

use crate::vfs::{
    vfs_ioctl, vfs_poll, vfs_read, vfs_write, VfsError, VfsNode,
};

pub struct VfsFile {
    pub node: Arc<VfsNode>,
}

impl VfsFile {
    pub fn new(node: Arc<VfsNode>) -> Arc<Self> {
        Arc::new(Self { node })
    }
}

fn map_err(err: VfsError) -> FdError {
    match err {
        VfsError::NotFound | VfsError::Invalid => FdError::NotFound,
        _ => FdError::Io,
    }
}

impl FileOps for VfsFile {
    fn read(&self, offset: usize, buf: &mut [u8]) -> FdResult<usize> {
        vfs_read(&self.node, offset, buf).map_err(map_err)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> FdResult<usize> {
        vfs_write(&self.node, offset, buf).map_err(map_err)
    }

    fn ioctl(&self, req: usize, arg: usize) -> FdResult {
        vfs_ioctl(&self.node, req, arg).map_err(map_err)
    }

    fn poll(&self, events: u32) -> FdResult<u32> {
        vfs_poll(&self.node, events).map_err(map_err)
    }

    fn stat(&self) -> Option<FileStat> {
        let meta = self.node.meta.lock();
        Some(FileStat {
            size: meta.size,
            mode: meta.mode,
            node_type: meta.node_type as u32,
        })
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
