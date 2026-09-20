pub mod node;
pub mod path;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, Ordering};
use serial;
use spin::Mutex;

pub use node::{VfsNode, VfsNodeMeta, VfsNodeType};
use path::{normalize_path, path_join};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    Invalid,
    NotFound,
    NotDir,
    Exists,
    NoDev,
    Busy,
    NotEmpty,
    NotSupported,
    Io,
}

pub type VfsResult<T = ()> = core::result::Result<T, VfsError>;

pub const VFS_NODE_PRIVATE_FD: u64 = 1 << 63;

pub trait VfsOps: Send + Sync {
    fn mount(&self, _src: Option<&str>, _node: &Arc<VfsNode>) -> VfsResult {
        Err(VfsError::NotSupported)
    }
    fn unmount(&self, _node: &Arc<VfsNode>) -> VfsResult {
        Err(VfsError::NotSupported)
    }
    fn open(&self, _parent: Option<&Arc<VfsNode>>, _name: &str, _node: &Arc<VfsNode>) -> VfsResult {
        Err(VfsError::NotSupported)
    }
    fn close(&self, _node: &Arc<VfsNode>) -> VfsResult {
        Ok(())
    }
    fn read(&self, _node: &Arc<VfsNode>, _offset: usize, _buf: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::NotSupported)
    }
    fn write(&self, _node: &Arc<VfsNode>, _offset: usize, _buf: &[u8]) -> VfsResult<usize> {
        Err(VfsError::NotSupported)
    }
    fn readlink(&self, _node: &Arc<VfsNode>, _buf: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::NotSupported)
    }
    fn mkdir(&self, _parent: &Arc<VfsNode>, _name: &str, _node: &Arc<VfsNode>) -> VfsResult {
        Ok(())
    }
    fn mkfile(&self, _parent: &Arc<VfsNode>, _name: &str, _node: &Arc<VfsNode>) -> VfsResult {
        Ok(())
    }
    fn link(&self, _parent: &Arc<VfsNode>, _name: &str, _node: &Arc<VfsNode>) -> VfsResult {
        Ok(())
    }
    fn symlink(&self, _parent: &Arc<VfsNode>, _name: &str, _node: &Arc<VfsNode>) -> VfsResult {
        Ok(())
    }
    fn stat(&self, _node: &Arc<VfsNode>) -> VfsResult {
        Ok(())
    }
    fn ioctl(&self, _node: &Arc<VfsNode>, _req: usize, _arg: usize) -> VfsResult {
        Err(VfsError::NotSupported)
    }
    fn dup(&self, _node: &Arc<VfsNode>) -> VfsResult<Arc<VfsNode>> {
        Err(VfsError::NotSupported)
    }
    fn poll(&self, _node: &Arc<VfsNode>, _events: u32) -> VfsResult<u32> {
        Err(VfsError::NotSupported)
    }
    fn map(&self, _node: &Arc<VfsNode>, _addr: usize, _len: usize, _offset: usize) -> VfsResult {
        Err(VfsError::NotSupported)
    }
    fn delete(&self, _parent: &Arc<VfsNode>, _node: &Arc<VfsNode>) -> VfsResult {
        Ok(())
    }
    fn rename(&self, _node: &Arc<VfsNode>, _new_name: &str) -> VfsResult {
        Ok(())
    }
    fn free(&self, _node: &Arc<VfsNode>) -> VfsResult {
        Ok(())
    }
    fn mknod(
        &self,
        _parent: &Arc<VfsNode>,
        _name: &str,
        _node: &Arc<VfsNode>,
        _mode: u16,
        _dev: u64,
    ) -> VfsResult {
        Ok(())
    }
    fn chmod(&self, _node: &Arc<VfsNode>, _mode: u16) -> VfsResult {
        Ok(())
    }
}

pub struct VfsFs {
    pub name: String,
    pub fsid: u16,
    pub magic: u64,
    pub flags: u64,
    pub ops: Arc<dyn VfsOps>,
}

static FS_REGISTRY: Mutex<Vec<Arc<VfsFs>>> = Mutex::new(Vec::new());
static NEXT_FSID: AtomicU16 = AtomicU16::new(1);
static ROOT: Mutex<Option<Arc<VfsNode>>> = Mutex::new(None);

pub fn vfs_init() -> Arc<VfsNode> {
    let root = Arc::new(VfsNode::new_root());
    *ROOT.lock() = Some(root.clone());
    root
}

pub fn vfs_root() -> Arc<VfsNode> {
    ROOT.lock().as_ref().cloned().expect("vfs not initialized")
}

pub fn vfs_register_fs(name: &str, ops: Arc<dyn VfsOps>, magic: u64, flags: u64) -> u16 {
    let fsid = NEXT_FSID.fetch_add(1, Ordering::Relaxed);
    let fs = Arc::new(VfsFs {
        name: String::from(name),
        fsid,
        magic,
        flags,
        ops,
    });
    FS_REGISTRY.lock().push(fs);
    fsid
}

pub fn get_filesystem(name: &str) -> Option<Arc<VfsFs>> {
    FS_REGISTRY
        .lock()
        .iter()
        .find(|fs| fs.name == name)
        .cloned()
}

pub fn get_filesystem_by_id(fsid: u16) -> Option<Arc<VfsFs>> {
    FS_REGISTRY
        .lock()
        .iter()
        .find(|fs| fs.fsid == fsid)
        .cloned()
}

pub fn vfs_node_alloc(parent: Option<&Arc<VfsNode>>, name: &str) -> Arc<VfsNode> {
    let fsid = parent.map(|p| p.meta.lock().fsid).unwrap_or(0);
    let node = Arc::new(VfsNode {
        meta: Mutex::new(VfsNodeMeta {
            name: String::from(name),
            node_type: VfsNodeType::None,
            fsid,
            size: 0,
            mode: 0o777,
            flags: 0,
            dev: 0,
            rdev: 0,
            parent: parent.map(Arc::downgrade),
            children: Vec::new(),
            linkto: None,
            linkto_path: None,
            handle: None,
            is_mount: false,
        }),
    });
    if let Some(p) = parent {
        p.meta.lock().children.push(node.clone());
    }
    node
}

pub fn vfs_child_append(parent: &Arc<VfsNode>, name: &str) -> Arc<VfsNode> {
    vfs_node_alloc(Some(parent), name)
}

pub fn vfs_child_find(parent: &Arc<VfsNode>, name: &str) -> Option<Arc<VfsNode>> {
    parent
        .meta
        .lock()
        .children
        .iter()
        .find(|n| n.meta.lock().name == name)
        .cloned()
}

pub fn vfs_list_dir(node: &Arc<VfsNode>) -> VfsResult<Vec<String>> {
    let meta = node.meta.lock();
    if meta.node_type != VfsNodeType::Dir {
        return Err(VfsError::NotDir);
    }
    Ok(meta.children.iter().map(|c| c.meta.lock().name.clone()).collect())
}

pub fn vfs_free(node: &Arc<VfsNode>) {
    if let Some(parent) = node
        .meta
        .lock()
        .parent
        .as_ref()
        .and_then(|p| p.upgrade())
    {
        parent
            .meta
            .lock()
            .children
            .retain(|c| !Arc::ptr_eq(c, node));
    }
    node.meta.lock().children.clear();
    node.meta.lock().handle = None;
}

pub fn vfs_get_fullpath(node: &Arc<VfsNode>) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cur: Option<Arc<VfsNode>> = Some(node.clone());
    while let Some(n) = cur {
        let meta = n.meta.lock();
        let name = meta.name.clone();
        cur = meta.parent.as_ref().and_then(|p| p.upgrade());
        if name != "/" {
            parts.push(name);
        }
    }
    parts.reverse();
    let mut out = String::from("/");
    out.push_str(&parts.join("/"));
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

pub fn vfs_cwd_path_build(path: &str) -> String {
    if path.starts_with('/') {
        return normalize_path(path);
    }
    // TODO: replace "/" with per-task CWD
    path_join("/", path)
}

pub fn vfs_open(path: &str) -> VfsResult<Arc<VfsNode>> {
    let path = normalize_path(path);
    let mut current = vfs_root();
    if path == "/" {
        return Ok(current);
    }
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let mut follow_budget = 8usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let next = match vfs_child_find(&current, part) {
            Some(n) => n,
            None => {
                let fs = get_filesystem_by_id(current.meta.lock().fsid).ok_or(VfsError::NoDev)?;
                let node = vfs_child_append(&current, part);
                match fs.ops.open(Some(&current), part, &node) {
                    Ok(()) => node,
                    Err(VfsError::NotSupported) | Err(VfsError::NotFound) => {
                        vfs_free(&node);
                        return Err(VfsError::NotFound);
                    }
                    Err(e) => {
                        vfs_free(&node);
                        return Err(e);
                    }
                }
            }
        };
        let meta = next.meta.lock();
        let is_last = i == parts.len() - 1;
        if meta.node_type == VfsNodeType::Symlink {
            drop(meta);
            if follow_budget == 0 {
                return Err(VfsError::Invalid);
            }
            follow_budget -= 1;
            if let Some(target) = vfs_resolve_symlink(&next)? {
                current = target;
                if is_last {
                    return Ok(current);
                }
                continue;
            } else {
                return Err(VfsError::NotFound);
            }
        }
        drop(meta);
        current = next;
    }
    Ok(current)
}

pub fn vfs_open_nofollow(path: &str) -> VfsResult<Arc<VfsNode>> {
    let path = normalize_path(path);
    let mut current = vfs_root();
    if path == "/" {
        return Ok(current);
    }
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let next = match vfs_child_find(&current, part) {
            Some(n) => n,
            None => {
                let fs = get_filesystem_by_id(current.meta.lock().fsid).ok_or(VfsError::NoDev)?;
                let node = vfs_child_append(&current, part);
                match fs.ops.open(Some(&current), part, &node) {
                    Ok(()) => node,
                    Err(VfsError::NotSupported) | Err(VfsError::NotFound) => {
                        vfs_free(&node);
                        return Err(VfsError::NotFound);
                    }
                    Err(e) => {
                        vfs_free(&node);
                        return Err(e);
                    }
                }
            }
        };
        if i == parts.len() - 1 {
            return Ok(next);
        }
        let meta = next.meta.lock();
        if meta.node_type == VfsNodeType::Symlink {
            drop(meta);
            if let Some(target) = vfs_resolve_symlink(&next)? {
                current = target;
                continue;
            } else {
                return Err(VfsError::NotFound);
            }
        }
        drop(meta);
        current = next;
    }
    Ok(current)
}

fn vfs_resolve_symlink(node: &Arc<VfsNode>) -> VfsResult<Option<Arc<VfsNode>>> {
    let meta = node.meta.lock();
    if let Some(link) = meta.linkto.as_ref().and_then(|w| w.upgrade()) {
        return Ok(Some(link));
    }
    if let Some(path) = meta.linkto_path.as_ref() {
        let base = meta
            .parent
            .as_ref()
            .and_then(|p| p.upgrade())
            .map(|p| vfs_get_fullpath(&p))
            .unwrap_or_else(|| String::from("/"));
        let full = if path.starts_with('/') {
            path.clone()
        } else {
            path_join(&base, path)
        };
        drop(meta);
        return Ok(Some(vfs_open(&full)?));
    }
    Ok(None)
}

pub fn vfs_mkdir(path: &str) -> VfsResult {
    serial::write_bytes(b"vfs_mkdir ");
    serial::write_bytes(path.as_bytes());
    serial::write_bytes(b"\n");
    let mut path = normalize_path(path);
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    let mut current = vfs_root();
    for part in path.trim_start_matches('/').split('/') {
        if part.is_empty() {
            continue;
        }
        if let Some(existing) = vfs_child_find(&current, part) {
            current = existing;
            let meta = current.meta.lock();
            if meta.node_type != VfsNodeType::Dir {
                return Err(VfsError::NotDir);
            }
            continue;
        }
        let node = vfs_child_append(&current, part);
        {
            let mut meta = node.meta.lock();
            meta.node_type = VfsNodeType::Dir;
        }
        let fsid = current.meta.lock().fsid;
        if let Some(fs) = get_filesystem_by_id(fsid) {
            fs.ops.mkdir(&current, part, &node)?;
        }
        current = node;
    }
    serial::write_bytes(b"vfs_mkdir ok\n");
    Ok(())
}

pub fn vfs_mkfile(path: &str) -> VfsResult<Arc<VfsNode>> {
    serial::write_bytes(b"vfs_mkfile ");
    serial::write_bytes(path.as_bytes());
    serial::write_bytes(b"\n");
    let mut path = normalize_path(path);
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    let mut parts = path.trim_start_matches('/').split('/').collect::<Vec<_>>();
    let name = parts.pop().ok_or(VfsError::Invalid)?;
    let parent_path = String::from("/") + &parts.join("/");
    let parent = vfs_open(&parent_path)?;
    if vfs_child_find(&parent, name).is_some() {
        return Err(VfsError::Exists);
    }
    let node = vfs_child_append(&parent, name);
    let fsid = parent.meta.lock().fsid;
    if let Some(fs) = get_filesystem_by_id(fsid) {
        serial::write_bytes(b"vfs_mkfile fs=");
        serial::write_bytes(fs.name.as_bytes());
        serial::write_bytes(b"\n");
        fs.ops.mkfile(&parent, name, &node)?;
    }
    serial::write_bytes(b"vfs_mkfile ok\n");
    Ok(node)
}

pub fn vfs_link(target: &str, link_path: &str) -> VfsResult<Arc<VfsNode>> {
    let target_node = vfs_open(target)?;
    let link_node = vfs_mkfile(link_path)?;
    {
        let mut meta = link_node.meta.lock();
        meta.linkto = Some(Arc::downgrade(&target_node));
        meta.node_type = target_node.meta.lock().node_type;
        meta.handle = target_node.meta.lock().handle;
        meta.size = target_node.meta.lock().size;
        meta.mode = target_node.meta.lock().mode;
    }
    Ok(link_node)
}

pub fn vfs_symlink(target: &str, link_path: &str) -> VfsResult<Arc<VfsNode>> {
    let link_node = vfs_mkfile(link_path)?;
    {
        let mut meta = link_node.meta.lock();
        meta.node_type = VfsNodeType::Symlink;
        meta.linkto_path = Some(String::from(target));
    }
    Ok(link_node)
}

pub fn vfs_mknod(path: &str, mode: u16, dev: u64) -> VfsResult<Arc<VfsNode>> {
    let node = vfs_mkfile(path)?;
    {
        let mut meta = node.meta.lock();
        meta.mode = mode;
        meta.dev = dev;
        meta.rdev = dev;
    }
    Ok(node)
}

pub fn vfs_delete(path: &str) -> VfsResult {
    let node = vfs_open(path)?;
    let parent = node
        .meta
        .lock()
        .parent
        .as_ref()
        .and_then(|p| p.upgrade())
        .ok_or(VfsError::Invalid)?;
    if node.meta.lock().node_type == VfsNodeType::Dir && !node.meta.lock().children.is_empty() {
        return Err(VfsError::NotEmpty);
    }
    let fsid = parent.meta.lock().fsid;
    if let Some(fs) = get_filesystem_by_id(fsid) {
        fs.ops.delete(&parent, &node)?;
    }
    parent.meta.lock().children.retain(|c| !Arc::ptr_eq(c, &node));
    Ok(())
}

pub fn vfs_rename(path: &str, new_name: &str) -> VfsResult {
    let node = vfs_open(path)?;
    let fsid = node.meta.lock().fsid;
    if let Some(fs) = get_filesystem_by_id(fsid) {
        fs.ops.rename(&node, new_name)?;
    }
    node.meta.lock().name = String::from(new_name);
    Ok(())
}

pub fn vfs_read(node: &Arc<VfsNode>, offset: usize, buf: &mut [u8]) -> VfsResult<usize> {
    let fs = get_filesystem_by_id(node.meta.lock().fsid).ok_or(VfsError::NoDev)?;
    fs.ops.read(node, offset, buf)
}

pub fn vfs_write(node: &Arc<VfsNode>, offset: usize, buf: &[u8]) -> VfsResult<usize> {
    let fs = get_filesystem_by_id(node.meta.lock().fsid).ok_or(VfsError::NoDev)?;
    fs.ops.write(node, offset, buf)
}

pub fn vfs_ioctl(node: &Arc<VfsNode>, req: usize, arg: usize) -> VfsResult {
    let fs = get_filesystem_by_id(node.meta.lock().fsid).ok_or(VfsError::NoDev)?;
    fs.ops.ioctl(node, req, arg)
}

pub fn vfs_poll(node: &Arc<VfsNode>, events: u32) -> VfsResult<u32> {
    let fs = get_filesystem_by_id(node.meta.lock().fsid).ok_or(VfsError::NoDev)?;
    fs.ops.poll(node, events)
}

pub fn vfs_map(node: &Arc<VfsNode>, addr: usize, len: usize, offset: usize) -> VfsResult {
    let fs = get_filesystem_by_id(node.meta.lock().fsid).ok_or(VfsError::NoDev)?;
    fs.ops.map(node, addr, len, offset)
}

pub fn vfs_mount(src: Option<&str>, fstype: &str, mountpoint: &Arc<VfsNode>) -> VfsResult {
    let fs = get_filesystem(fstype).ok_or(VfsError::NoDev)?;
    let (prev_fsid, prev_mount) = {
        let mut meta = mountpoint.meta.lock();
        let old_fsid = meta.fsid;
        let old_mount = meta.is_mount;
        meta.fsid = fs.fsid;
        meta.is_mount = true;
        (old_fsid, old_mount)
    };
    match fs.ops.mount(src, mountpoint) {
        Ok(()) => Ok(()),
        Err(e) => {
            let mut meta = mountpoint.meta.lock();
            meta.fsid = prev_fsid;
            meta.is_mount = prev_mount;
            Err(e)
        }
    }
}

pub fn vfs_unmount(path: &str) -> VfsResult {
    let node = vfs_open(path)?;
    let fs = get_filesystem_by_id(node.meta.lock().fsid).ok_or(VfsError::NoDev)?;
    fs.ops.unmount(&node)?;
    node.meta.lock().is_mount = false;
    Ok(())
}
