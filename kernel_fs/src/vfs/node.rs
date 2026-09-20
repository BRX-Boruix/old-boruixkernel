use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsNodeType {
    None,
    Dir,
    Block,
    Stream,
    Symlink,
    Pipe,
    Socket,
}

pub struct VfsNodeMeta {
    pub name: String,
    pub node_type: VfsNodeType,
    pub fsid: u16,
    pub size: u64,
    pub mode: u16,
    pub flags: u64,
    pub dev: u64,
    pub rdev: u64,
    pub parent: Option<Weak<VfsNode>>,
    pub children: Vec<Arc<VfsNode>>,
    pub linkto: Option<Weak<VfsNode>>,
    pub linkto_path: Option<String>,
    pub handle: Option<usize>,
    pub is_mount: bool,
}

pub struct VfsNode {
    pub meta: Mutex<VfsNodeMeta>,
}

impl VfsNode {
    pub fn new_root() -> Self {
        Self {
            meta: Mutex::new(VfsNodeMeta {
                name: String::from("/"),
                node_type: VfsNodeType::Dir,
                fsid: 0,
                size: 0,
                mode: 0o755,
                flags: 0,
                dev: 0,
                rdev: 0,
                parent: None,
                children: Vec::new(),
                linkto: None,
                linkto_path: None,
                handle: None,
                is_mount: false,
            }),
        }
    }
}
