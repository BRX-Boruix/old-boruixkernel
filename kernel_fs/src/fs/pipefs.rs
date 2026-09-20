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

const PIPE_CAPACITY: usize = 4096;

struct PipeInner {
    buf: Vec<u8>,
    head: usize,
    len: usize,
    readers: usize,
    writers: usize,
}

impl PipeInner {
    fn new() -> Self {
        Self {
            buf: vec![0; PIPE_CAPACITY],
            head: 0,
            len: 0,
            readers: 0,
            writers: 0,
        }
    }

    fn read_available(&self) -> usize {
        self.len
    }

    fn write_available(&self) -> usize {
        PIPE_CAPACITY - self.len
    }
}

struct Pipe {
    inner: Mutex<PipeInner>,
    read_wait: WaitQueue,
    write_wait: WaitQueue,
}

impl Pipe {
    fn new() -> Self {
        Self {
            inner: Mutex::new(PipeInner::new()),
            read_wait: WaitQueue::new(),
            write_wait: WaitQueue::new(),
        }
    }
}

struct PipeEnd {
    pipe: Arc<Pipe>,
    write: bool,
}

fn set_handle(node: &Arc<VfsNode>, end: PipeEnd) {
    let ptr = Box::into_raw(Box::new(end));
    node.meta.lock().handle = Some(ptr as usize);
}

fn get_handle(node: &Arc<VfsNode>) -> VfsResult<&'static mut PipeEnd> {
    let ptr = node.meta.lock().handle.ok_or(VfsError::Invalid)? as *mut PipeEnd;
    if ptr.is_null() {
        return Err(VfsError::Invalid);
    }
    Ok(unsafe { &mut *ptr })
}

fn drop_handle(node: &Arc<VfsNode>) {
    let ptr = node.meta.lock().handle.take().unwrap_or(0) as *mut PipeEnd;
    if !ptr.is_null() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    }
}

pub fn pipefs_create_pair() -> (Arc<VfsNode>, Arc<VfsNode>) {
    let pipe = Arc::new(Pipe::new());
    let read_end = vfs_node_alloc(None, "pipe_r");
    let write_end = vfs_node_alloc(None, "pipe_w");

    {
        let mut inner = pipe.inner.lock();
        inner.readers += 1;
        inner.writers += 1;
    }

    {
        let mut meta = read_end.meta.lock();
        meta.node_type = VfsNodeType::Pipe;
        meta.flags |= VFS_NODE_PRIVATE_FD;
    }
    {
        let mut meta = write_end.meta.lock();
        meta.node_type = VfsNodeType::Pipe;
        meta.flags |= VFS_NODE_PRIVATE_FD;
    }

    set_handle(&read_end, PipeEnd { pipe: pipe.clone(), write: false });
    set_handle(&write_end, PipeEnd { pipe, write: true });

    (read_end, write_end)
}

pub struct PipeFs;

impl PipeFs {
    pub fn new() -> Self {
        Self
    }
}

impl VfsOps for PipeFs {
    fn read(&self, node: &Arc<VfsNode>, _offset: usize, buf: &mut [u8]) -> VfsResult<usize> {
        let end = get_handle(node)?;
        if end.write {
            return Err(VfsError::Invalid);
        }
        let pipe = &end.pipe;
        loop {
            let mut inner = pipe.inner.lock();
            if inner.read_available() > 0 {
                let n = min(buf.len(), inner.read_available());
                for i in 0..n {
                    buf[i] = inner.buf[inner.head];
                    inner.head = (inner.head + 1) % PIPE_CAPACITY;
                }
                inner.len -= n;
                drop(inner);
                pipe.write_wait.wake_one();
                return Ok(n);
            }
            if inner.writers == 0 {
                return Ok(0);
            }
            drop(inner);
            pipe.read_wait.wait();
        }
    }

    fn write(&self, node: &Arc<VfsNode>, _offset: usize, buf: &[u8]) -> VfsResult<usize> {
        let end = get_handle(node)?;
        if !end.write {
            return Err(VfsError::Invalid);
        }
        let pipe = &end.pipe;
        let mut written = 0usize;
        while written < buf.len() {
            let mut inner = pipe.inner.lock();
            if inner.readers == 0 {
                return Err(VfsError::Io);
            }
            let avail = inner.write_available();
            if avail == 0 {
                drop(inner);
                pipe.write_wait.wait();
                continue;
            }
            let n = min(avail, buf.len() - written);
            for i in 0..n {
                let idx = (inner.head + inner.len + i) % PIPE_CAPACITY;
                inner.buf[idx] = buf[written + i];
            }
            inner.len += n;
            written += n;
            drop(inner);
            pipe.read_wait.wake_one();
        }
        Ok(written)
    }

    fn close(&self, node: &Arc<VfsNode>) -> VfsResult {
        let end = get_handle(node)?;
        let pipe = &end.pipe;
        let mut inner = pipe.inner.lock();
        if end.write {
            if inner.writers > 0 {
                inner.writers -= 1;
            }
        } else if inner.readers > 0 {
            inner.readers -= 1;
        }
        drop(inner);
        pipe.read_wait.wake_all();
        pipe.write_wait.wake_all();
        Ok(())
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
