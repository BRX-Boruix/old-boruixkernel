use alloc::sync::Arc;
use kernel_driver_hub::driver_hub::e1000;
use kernel_fs::fd_adapter::VfsFile;
use kernel_fs::fs::sockfs::{
    sockfs_accept, sockfs_bind, sockfs_connect, sockfs_create_socket, sockfs_listen, SockType,
};
use kernel_task::fd::{FdError, FileHandle};
use kernel_task::task;

use super::user_ptr::validate_user_range;

pub(super) fn sys_net_send(buf: *const u8, len: usize) -> isize {
    if buf.is_null() || len == 0 {
        return -1;
    }
    if !validate_user_range(buf as usize, len, false) {
        return -1;
    }
    e1000::send(buf, len)
}

pub(super) fn sys_net_recv(buf: *mut u8, len: usize) -> isize {
    if buf.is_null() || len == 0 {
        return -1;
    }
    if !validate_user_range(buf as usize, len, true) {
        return -1;
    }
    e1000::recv(buf, len)
}

pub(super) fn sys_net_mac(buf: *mut u8, len: usize) -> isize {
    if buf.is_null() || len < 6 {
        return -1;
    }
    if !validate_user_range(buf as usize, 6, true) {
        return -1;
    }
    let Some(mac) = e1000::mac_addr() else {
        return -1;
    };
    unsafe {
        core::ptr::copy_nonoverlapping(mac.as_ptr(), buf, 6);
    }
    6
}

pub(super) fn sys_net_status() -> isize {
    match e1000::status() {
        Some(v) => v as isize,
        None => -1,
    }
}

pub(super) fn sys_net_regs(buf: *mut u32, len: usize) -> isize {
    if buf.is_null() || len < 8 {
        return -1;
    }
    let bytes = len * core::mem::size_of::<u32>();
    if !validate_user_range(buf as usize, bytes, true) {
        return -1;
    }
    let out = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    let mut regs = [0u32; 8];
    if !e1000::regs(&mut regs) {
        return -1;
    }
    out[..8].copy_from_slice(&regs);
    8
}

pub(super) fn sys_net_counters(buf: *mut u64, len: usize) -> isize {
    if buf.is_null() || len < 2 {
        return -1;
    }
    let bytes = len * core::mem::size_of::<u64>();
    if !validate_user_range(buf as usize, bytes, true) {
        return -1;
    }
    let out = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    let (tx, rx) = e1000::counters();
    out[0] = tx;
    out[1] = rx;
    2
}

fn map_fd_err(err: FdError) -> isize {
    match err {
        FdError::NotFound => -1,
        FdError::NoSpace => -1,
        FdError::Invalid => -1,
        FdError::Io => -1,
    }
}

pub(super) fn sys_socket(domain: usize, kind: usize, _protocol: usize) -> isize {
    if domain != 1 {
        return -1;
    }
    let ty = match kind {
        1 => SockType::Stream,
        2 => SockType::Dgram,
        _ => return -1,
    };

    let node = sockfs_create_socket(ty);
    let handle = FileHandle::new(VfsFile::new(node), 0);
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let mut fds = current.fds.lock();
    match fds.alloc_fd(0, Arc::new(handle), false) {
        Ok(v) => v as isize,
        Err(e) => map_fd_err(e),
    }
}

pub(super) fn sys_bind(fd: usize, _addr: usize, _len: usize) -> isize {
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let fds = current.fds.lock();
    let Some(handle) = fds.get_fd(fd) else { return -1 };
    let vfs = handle.node.as_ref();
    let node = match vfs.as_any().downcast_ref::<VfsFile>() {
        Some(v) => v,
        None => return -1,
    };
    if sockfs_bind(&node.node).is_err() {
        return -1;
    }
    0
}

pub(super) fn sys_listen(fd: usize, _backlog: usize) -> isize {
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let fds = current.fds.lock();
    let Some(handle) = fds.get_fd(fd) else { return -1 };
    let vfs = handle.node.as_ref();
    let node = match vfs.as_any().downcast_ref::<VfsFile>() {
        Some(v) => v,
        None => return -1,
    };
    if sockfs_listen(&node.node).is_err() {
        return -1;
    }
    0
}

pub(super) fn sys_accept(fd: usize, addr: usize, len: usize) -> isize {
    if addr != 0 && !validate_user_range(addr, len, true) {
        return -1;
    }
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let fds = current.fds.lock();
    let Some(handle) = fds.get_fd(fd) else { return -1 };
    let vfs = handle.node.as_ref();
    let node = match vfs.as_any().downcast_ref::<VfsFile>() {
        Some(v) => v,
        None => return -1,
    };
    let new_node = match sockfs_accept(&node.node) {
        Ok(n) => n,
        Err(_) => return -1,
    };
    drop(fds);
    let mut fds = current.fds.lock();
    let handle = FileHandle::new(VfsFile::new(new_node), 0);
    match fds.alloc_fd(0, Arc::new(handle), false) {
        Ok(v) => v as isize,
        Err(e) => map_fd_err(e),
    }
}

pub(super) fn sys_connect(fd: usize, addr: usize, len: usize) -> isize {
    if addr == 0 || len < core::mem::size_of::<i32>() {
        return -1;
    }
    if !validate_user_range(addr, core::mem::size_of::<i32>(), true) {
        return -1;
    }
    let peer_fd = unsafe { core::ptr::read_unaligned(addr as *const i32) } as usize;

    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let fds = current.fds.lock();
    let Some(handle) = fds.get_fd(fd) else { return -1 };
    let Some(peer_handle) = fds.get_fd(peer_fd) else { return -1 };

    let vfs = handle.node.as_ref();
    let peer_vfs = peer_handle.node.as_ref();
    let node = match vfs.as_any().downcast_ref::<VfsFile>() {
        Some(v) => v,
        None => return -1,
    };
    let peer = match peer_vfs.as_any().downcast_ref::<VfsFile>() {
        Some(v) => v,
        None => return -1,
    };

    if sockfs_connect(&node.node, &peer.node).is_err() {
        return -1;
    }
    0
}
