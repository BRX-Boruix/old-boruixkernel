use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;

use kernel_driver_hub::driver_hub::{self, device::DeviceKind};
use kernel_driver_hub::driver_hub::device::DeviceOps;

use crate::vfs::{
    vfs_child_append, vfs_child_find, VfsError, VfsNode, VfsNodeType, VfsOps, VfsResult,
    VFS_NODE_PRIVATE_FD,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DevTmpKind {
    Dir,
    File,
    Symlink,
    Device,
}

struct DevTmpHandle {
    kind: DevTmpKind,
    dev: Option<&'static dyn DeviceOps>,
    open_fn: Option<fn() -> &'static dyn DeviceOps>,
}

impl DevTmpHandle {
    fn new_dir() -> Self {
        Self {
            kind: DevTmpKind::Dir,
            dev: None,
            open_fn: None,
        }
    }

    fn new_dev(dev: &'static dyn DeviceOps) -> Self {
        Self {
            kind: DevTmpKind::Device,
            dev: Some(dev),
            open_fn: None,
        }
    }
}

fn set_handle(node: &Arc<VfsNode>, handle: DevTmpHandle) {
    let ptr = Box::into_raw(Box::new(handle));
    node.meta.lock().handle = Some(ptr as usize);
}

fn get_handle(node: &Arc<VfsNode>) -> VfsResult<&'static mut DevTmpHandle> {
    let ptr = node.meta.lock().handle.ok_or(VfsError::Invalid)? as *mut DevTmpHandle;
    if ptr.is_null() {
        return Err(VfsError::Invalid);
    }
    Ok(unsafe { &mut *ptr })
}

fn drop_handle(node: &Arc<VfsNode>) {
    let ptr = node.meta.lock().handle.take().unwrap_or(0) as *mut DevTmpHandle;
    if !ptr.is_null() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    }
}

pub struct DevTmpFs;

impl DevTmpFs {
    pub fn new() -> Self {
        Self
    }

    fn create_device_node(parent: &Arc<VfsNode>, name: &str, dev: &'static dyn DeviceOps) {
        if vfs_child_find(parent, name).is_some() {
            return;
        }
        let node = vfs_child_append(parent, name);
        set_handle(&node, DevTmpHandle::new_dev(dev));
        let kind = dev.kind();
        let mut meta = node.meta.lock();
        meta.node_type = match kind {
            DeviceKind::Char => VfsNodeType::Stream,
            DeviceKind::Block => VfsNodeType::Block,
            _ => VfsNodeType::None,
        };
    }

    fn create_devices(root: &Arc<VfsNode>) {
        let count = driver_hub::device_count();
        for idx in 0..count {
            let info = driver_hub::device_info_at(idx);
            let dev = driver_hub::device_at(idx);
            let Some(info) = info else { continue };
            let Some(dev) = dev else { continue };
            Self::create_device_node(root, info.name, dev);
        }
    }

    pub fn create_device_node_ex(
        root: &Arc<VfsNode>,
        name: &str,
        dev: &'static dyn DeviceOps,
        open_fn: Option<fn() -> &'static dyn DeviceOps>,
    ) -> VfsResult {
        if vfs_child_find(root, name).is_some() {
            return Err(VfsError::Exists);
        }
        let node = vfs_child_append(root, name);
        let mut handle = DevTmpHandle::new_dev(dev);
        handle.open_fn = open_fn;
        set_handle(&node, handle);
        Ok(())
    }

    pub fn make_per_open_node(node: &Arc<VfsNode>) -> Option<Arc<VfsNode>> {
        let handle = get_handle(node).ok()?;
        if handle.open_fn.is_none() {
            return None;
        }
        let open_fn = handle.open_fn?;
        let dev = open_fn();
        let parent = node.meta.lock().parent.as_ref().and_then(|p| p.upgrade())?;
        let private = vfs_child_append(&parent, &node.meta.lock().name);
        set_handle(&private, DevTmpHandle::new_dev(dev));
        {
            let mut meta = private.meta.lock();
            meta.flags |= VFS_NODE_PRIVATE_FD;
        }
        Some(private)
    }
}

impl VfsOps for DevTmpFs {
    fn mount(&self, _src: Option<&str>, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(node, DevTmpHandle::new_dir());
        node.meta.lock().node_type = VfsNodeType::Dir;
        Self::create_devices(node);
        Ok(())
    }

    fn unmount(&self, node: &Arc<VfsNode>) -> VfsResult {
        drop_handle(node);
        Ok(())
    }

    fn mkdir(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(node, DevTmpHandle::new_dir());
        node.meta.lock().node_type = VfsNodeType::Dir;
        Ok(())
    }

    fn mkfile(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(
            node,
            DevTmpHandle {
                kind: DevTmpKind::File,
                dev: None,
                open_fn: None,
            },
        );
        node.meta.lock().node_type = VfsNodeType::None;
        Ok(())
    }

    fn symlink(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(
            node,
            DevTmpHandle {
                kind: DevTmpKind::Symlink,
                dev: None,
                open_fn: None,
            },
        );
        node.meta.lock().node_type = VfsNodeType::Symlink;
        Ok(())
    }

    fn stat(&self, node: &Arc<VfsNode>) -> VfsResult {
        let handle = get_handle(node)?;
        let mut meta = node.meta.lock();
        meta.node_type = match handle.kind {
            DevTmpKind::Dir => VfsNodeType::Dir,
            DevTmpKind::Symlink => VfsNodeType::Symlink,
            DevTmpKind::Device => {
                if let Some(dev) = handle.dev {
                    match dev.kind() {
                        DeviceKind::Char => VfsNodeType::Stream,
                        DeviceKind::Block => VfsNodeType::Block,
                        _ => VfsNodeType::None,
                    }
                } else {
                    VfsNodeType::None
                }
            }
            DevTmpKind::File => VfsNodeType::None,
        };
        Ok(())
    }

    fn read(&self, node: &Arc<VfsNode>, _offset: usize, buf: &mut [u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        let Some(dev) = handle.dev else { return Err(VfsError::Invalid) };
        Ok(dev.read_at(_offset as u64, buf))
    }

    fn write(&self, node: &Arc<VfsNode>, _offset: usize, buf: &[u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        let Some(dev) = handle.dev else { return Err(VfsError::Invalid) };
        Ok(dev.write_at(_offset as u64, buf))
    }

    fn poll(&self, node: &Arc<VfsNode>, _events: u32) -> VfsResult<u32> {
        let handle = get_handle(node)?;
        let Some(dev) = handle.dev else { return Err(VfsError::Invalid) };
        Ok(if dev.poll() { 1 } else { 0 })
    }

    fn ioctl(&self, node: &Arc<VfsNode>, req: usize, arg: usize) -> VfsResult {
        let handle = get_handle(node)?;
        let Some(dev) = handle.dev else { return Err(VfsError::Invalid) };
        let ret = dev.ioctl(req, arg);
        if ret < 0 {
            Err(VfsError::Io)
        } else {
            Ok(())
        }
    }

    fn delete(&self, _parent: &Arc<VfsNode>, node: &Arc<VfsNode>) -> VfsResult {
        drop_handle(node);
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

    fn map(&self, _node: &Arc<VfsNode>, _addr: usize, _len: usize, _offset: usize) -> VfsResult {
        Err(VfsError::NotSupported)
    }
}
