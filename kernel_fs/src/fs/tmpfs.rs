use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::vfs::{VfsError, VfsNode, VfsNodeType, VfsOps, VfsResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TmpfsKind {
    Dir,
    File,
    Symlink,
    Char,
    Block,
}

struct TmpfsFile {
    kind: TmpfsKind,
    data: Vec<u8>,
    link_count: usize,
}

impl TmpfsFile {
    fn new(kind: TmpfsKind) -> Self {
        Self {
            kind,
            data: Vec::new(),
            link_count: 1,
        }
    }
}

fn get_handle(node: &Arc<VfsNode>) -> VfsResult<&'static mut TmpfsFile> {
    let ptr = node.meta.lock().handle.ok_or(VfsError::Invalid)? as *mut TmpfsFile;
    if ptr.is_null() {
        return Err(VfsError::Invalid);
    }
    Ok(unsafe { &mut *ptr })
}

fn set_handle(node: &Arc<VfsNode>, file: TmpfsFile) {
    let ptr = Box::into_raw(Box::new(file));
    node.meta.lock().handle = Some(ptr as usize);
}

fn drop_handle(node: &Arc<VfsNode>) {
    let ptr = node.meta.lock().handle.take().unwrap_or(0) as *mut TmpfsFile;
    if !ptr.is_null() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    }
}

fn dec_link(file: &mut TmpfsFile) -> bool {
    if file.link_count > 1 {
        file.link_count -= 1;
        false
    } else {
        true
    }
}

pub struct Tmpfs;

impl Tmpfs {
    pub fn new() -> Self {
        Self
    }
}

impl VfsOps for Tmpfs {
    fn mount(&self, _src: Option<&str>, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(node, TmpfsFile::new(TmpfsKind::Dir));
        let mut meta = node.meta.lock();
        meta.node_type = VfsNodeType::Dir;
        Ok(())
    }

    fn unmount(&self, node: &Arc<VfsNode>) -> VfsResult {
        // Best-effort recursive cleanup of handles.
        fn walk(n: &Arc<VfsNode>) {
            let children = n.meta.lock().children.clone();
            for c in children {
                walk(&c);
            }
            let mut meta = n.meta.lock();
            if let Some(handle) = meta.handle {
                let file = unsafe { &mut *(handle as *mut TmpfsFile) };
                if dec_link(file) {
                    meta.handle = None;
                    unsafe {
                        drop(Box::from_raw(handle as *mut TmpfsFile));
                    }
                }
            }
        }
        walk(node);
        Ok(())
    }

    fn mkdir(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(node, TmpfsFile::new(TmpfsKind::Dir));
        node.meta.lock().node_type = VfsNodeType::Dir;
        Ok(())
    }

    fn mkfile(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(node, TmpfsFile::new(TmpfsKind::File));
        node.meta.lock().node_type = VfsNodeType::None;
        Ok(())
    }

    fn symlink(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(node, TmpfsFile::new(TmpfsKind::Symlink));
        node.meta.lock().node_type = VfsNodeType::Symlink;
        Ok(())
    }

    fn stat(&self, node: &Arc<VfsNode>) -> VfsResult {
        let file = get_handle(node)?;
        let mut meta = node.meta.lock();
        meta.node_type = match file.kind {
            TmpfsKind::Dir => VfsNodeType::Dir,
            TmpfsKind::File => VfsNodeType::None,
            TmpfsKind::Symlink => VfsNodeType::Symlink,
            TmpfsKind::Char => VfsNodeType::Stream,
            TmpfsKind::Block => VfsNodeType::Block,
        };
        meta.size = match file.kind {
            TmpfsKind::Dir => 0,
            _ => file.data.len() as u64,
        };
        Ok(())
    }

    fn read(&self, node: &Arc<VfsNode>, offset: usize, buf: &mut [u8]) -> VfsResult<usize> {
        let file = get_handle(node)?;
        if file.kind != TmpfsKind::File {
            return Err(VfsError::Invalid);
        }
        if offset >= file.data.len() {
            return Ok(0);
        }
        let end = core::cmp::min(file.data.len(), offset + buf.len());
        let src = &file.data[offset..end];
        buf[..src.len()].copy_from_slice(src);
        Ok(src.len())
    }

    fn write(&self, node: &Arc<VfsNode>, offset: usize, buf: &[u8]) -> VfsResult<usize> {
        let file = get_handle(node)?;
        if file.kind != TmpfsKind::File {
            return Err(VfsError::Invalid);
        }
        let end = offset + buf.len();
        if end > file.data.len() {
            file.data.resize(end, 0);
        }
        file.data[offset..end].copy_from_slice(buf);
        node.meta.lock().size = file.data.len() as u64;
        Ok(buf.len())
    }

    fn readlink(&self, node: &Arc<VfsNode>, buf: &mut [u8]) -> VfsResult<usize> {
        let target = node.meta.lock().linkto_path.clone().unwrap_or_else(|| String::from(""));
        let bytes = target.as_bytes();
        let n = core::cmp::min(buf.len(), bytes.len());
        buf[..n].copy_from_slice(&bytes[..n]);
        Ok(n)
    }

    fn delete(&self, _parent: &Arc<VfsNode>, node: &Arc<VfsNode>) -> VfsResult {
        let mut meta = node.meta.lock();
        if let Some(handle) = meta.handle {
            let file = unsafe { &mut *(handle as *mut TmpfsFile) };
            if dec_link(file) {
                meta.handle = None;
                unsafe {
                    drop(Box::from_raw(handle as *mut TmpfsFile));
                }
            }
        }
        Ok(())
    }

    fn rename(&self, node: &Arc<VfsNode>, new_name: &str) -> VfsResult {
        node.meta.lock().name = String::from(new_name);
        Ok(())
    }

    fn free(&self, node: &Arc<VfsNode>) -> VfsResult {
        drop_handle(node);
        Ok(())
    }

    fn chmod(&self, node: &Arc<VfsNode>, mode: u16) -> VfsResult {
        node.meta.lock().mode = mode;
        Ok(())
    }

    fn mknod(
        &self,
        _parent: &Arc<VfsNode>,
        _name: &str,
        node: &Arc<VfsNode>,
        mode: u16,
        dev: u64,
    ) -> VfsResult {
        let kind = match mode & 0o170000 {
            0o020000 => TmpfsKind::Char,
            0o060000 => TmpfsKind::Block,
            _ => TmpfsKind::File,
        };
        set_handle(node, TmpfsFile::new(kind));
        let mut meta = node.meta.lock();
        meta.node_type = match kind {
            TmpfsKind::Char => VfsNodeType::Stream,
            TmpfsKind::Block => VfsNodeType::Block,
            TmpfsKind::File => VfsNodeType::None,
            TmpfsKind::Dir => VfsNodeType::Dir,
            TmpfsKind::Symlink => VfsNodeType::Symlink,
        };
        meta.mode = mode;
        meta.dev = dev;
        meta.rdev = dev;
        Ok(())
    }

    fn map(&self, _node: &Arc<VfsNode>, _addr: usize, _len: usize, _offset: usize) -> VfsResult {
        Err(VfsError::NotSupported)
    }
}
