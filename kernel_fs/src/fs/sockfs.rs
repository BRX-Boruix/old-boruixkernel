use alloc::boxed::Box;
use alloc::vec;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cmp::min;

use spin::Mutex;

use kernel_task::waitqueue::WaitQueue;

use crate::vfs::{
    vfs_node_alloc, VfsError, VfsNode, VfsNodeType, VfsOps, VfsResult, VFS_NODE_PRIVATE_FD,
};

const SOCK_CAPACITY: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SockType {
    Stream,
    Dgram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SockState {
    Unconnected,
    Listening,
    Connected,
    Closed,
}

struct RingBuf {
    buf: Vec<u8>,
    head: usize,
    len: usize,
}

impl RingBuf {
    fn new() -> Self {
        Self {
            buf: vec![0; SOCK_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    fn read_available(&self) -> usize {
        self.len
    }

    fn write_available(&self) -> usize {
        SOCK_CAPACITY - self.len
    }

    fn read(&mut self, out: &mut [u8]) -> usize {
        let n = min(out.len(), self.len);
        for i in 0..n {
            out[i] = self.buf[self.head];
            self.head = (self.head + 1) % SOCK_CAPACITY;
        }
        self.len -= n;
        n
    }

    fn write(&mut self, input: &[u8]) -> usize {
        let n = min(input.len(), self.write_available());
        for i in 0..n {
            let idx = (self.head + self.len + i) % SOCK_CAPACITY;
            self.buf[idx] = input[i];
        }
        self.len += n;
        n
    }
}

struct SockInner {
    ty: SockType,
    state: SockState,
    recv: RingBuf,
    peer: Option<Arc<Sock>>,
    backlog: Vec<Arc<Sock>>,
}

impl SockInner {
    fn new(ty: SockType) -> Self {
        Self {
            ty,
            state: SockState::Unconnected,
            recv: RingBuf::new(),
            peer: None,
            backlog: Vec::new(),
        }
    }
}

struct Sock {
    inner: Mutex<SockInner>,
    recv_wait: WaitQueue,
    accept_wait: WaitQueue,
}

impl Sock {
    fn new(ty: SockType) -> Self {
        Self {
            inner: Mutex::new(SockInner::new(ty)),
            recv_wait: WaitQueue::new(),
            accept_wait: WaitQueue::new(),
        }
    }
}

struct SockHandle {
    sock: Arc<Sock>,
}

fn set_handle(node: &Arc<VfsNode>, handle: SockHandle) {
    let ptr = Box::into_raw(Box::new(handle));
    node.meta.lock().handle = Some(ptr as usize);
}

fn get_handle(node: &Arc<VfsNode>) -> VfsResult<&'static mut SockHandle> {
    let ptr = node.meta.lock().handle.ok_or(VfsError::Invalid)? as *mut SockHandle;
    if ptr.is_null() {
        return Err(VfsError::Invalid);
    }
    Ok(unsafe { &mut *ptr })
}

fn drop_handle(node: &Arc<VfsNode>) {
    let ptr = node.meta.lock().handle.take().unwrap_or(0) as *mut SockHandle;
    if !ptr.is_null() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    }
}

pub fn sockfs_create_socket(ty: SockType) -> Arc<VfsNode> {
    let sock = Arc::new(Sock::new(ty));
    let node = vfs_node_alloc(None, "socket");
    node.meta.lock().node_type = VfsNodeType::Socket;
    node.meta.lock().flags |= VFS_NODE_PRIVATE_FD;
    set_handle(&node, SockHandle { sock });
    node
}

pub fn sockfs_bind(node: &Arc<VfsNode>) -> VfsResult {
    let handle = get_handle(node)?;
    let mut inner = handle.sock.inner.lock();
    if inner.state != SockState::Unconnected {
        return Err(VfsError::Invalid);
    }
    inner.state = SockState::Connected;
    Ok(())
}

pub fn sockfs_listen(node: &Arc<VfsNode>) -> VfsResult {
    let handle = get_handle(node)?;
    let mut inner = handle.sock.inner.lock();
    if inner.state != SockState::Unconnected && inner.state != SockState::Connected {
        return Err(VfsError::Invalid);
    }
    inner.state = SockState::Listening;
    Ok(())
}

pub fn sockfs_connect(client: &Arc<VfsNode>, server: &Arc<VfsNode>) -> VfsResult {
    let client_handle = get_handle(client)?;
    let server_handle = get_handle(server)?;

    let mut s_inner = server_handle.sock.inner.lock();
    if s_inner.state != SockState::Listening {
        return Err(VfsError::Invalid);
    }

    let accepted = Arc::new(Sock::new(s_inner.ty));
    {
        let mut a_inner = accepted.inner.lock();
        a_inner.state = SockState::Connected;
        a_inner.peer = Some(client_handle.sock.clone());
    }

    {
        let mut c_inner = client_handle.sock.inner.lock();
        c_inner.state = SockState::Connected;
        c_inner.peer = Some(accepted.clone());
    }

    s_inner.backlog.push(accepted.clone());
    drop(s_inner);
    server_handle.sock.accept_wait.wake_one();
    Ok(())
}

pub fn sockfs_accept(server: &Arc<VfsNode>) -> VfsResult<Arc<VfsNode>> {
    let handle = get_handle(server)?;
    loop {
        let mut inner = handle.sock.inner.lock();
        if let Some(sock) = inner.backlog.pop() {
            let node = vfs_node_alloc(None, "socket");
            node.meta.lock().node_type = VfsNodeType::Socket;
            node.meta.lock().flags |= VFS_NODE_PRIVATE_FD;
            set_handle(&node, SockHandle { sock });
            return Ok(node);
        }
        drop(inner);
        handle.sock.accept_wait.wait();
    }
}

pub struct SockFs;

impl SockFs {
    pub fn new() -> Self {
        Self
    }
}

impl VfsOps for SockFs {
    fn read(&self, node: &Arc<VfsNode>, _offset: usize, buf: &mut [u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        loop {
            let mut inner = handle.sock.inner.lock();
            if inner.recv.read_available() > 0 {
                let n = inner.recv.read(buf);
                drop(inner);
                return Ok(n);
            }
            if inner.state == SockState::Closed {
                return Ok(0);
            }
            drop(inner);
            handle.sock.recv_wait.wait();
        }
    }

    fn write(&self, node: &Arc<VfsNode>, _offset: usize, buf: &[u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        let peer = {
            let inner = handle.sock.inner.lock();
            inner.peer.clone()
        };
        let Some(peer) = peer else { return Err(VfsError::Invalid) };
        let mut p_inner = peer.inner.lock();
        let n = p_inner.recv.write(buf);
        drop(p_inner);
        peer.recv_wait.wake_one();
        Ok(n)
    }

    fn delete(&self, _parent: &Arc<VfsNode>, node: &Arc<VfsNode>) -> VfsResult {
        drop_handle(node);
        Ok(())
    }

    fn free(&self, node: &Arc<VfsNode>) -> VfsResult {
        drop_handle(node);
        Ok(())
    }
}
