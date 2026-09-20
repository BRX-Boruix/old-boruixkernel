use alloc::sync::Arc;
use kernel_fs::fd_adapter::VfsFile;
use kernel_fs::fs::pipefs::pipefs_create_pair;
use kernel_fs::vfs::{
    vfs_delete, vfs_list_dir, vfs_mkdir, vfs_mkfile, vfs_mount, vfs_open, vfs_rename,
    vfs_unmount, VfsError,
};
use kernel_task::fd::{FdError, FileHandle};
use kernel_task::task;
use serial;

use super::user_ptr::{read_cstring_from_user, validate_user_range};

const O_CREAT: u32 = 0x40;
const O_DIRECTORY: u32 = 0x10000;

const F_GETFD: usize = 1;
const F_SETFD: usize = 2;
const F_GETFL: usize = 3;
const F_SETFL: usize = 4;
const FD_CLOEXEC: u32 = 1;

#[repr(C)]
pub struct Stat {
    pub st_mode: u16,
    pub st_size: u64,
    pub st_type: u32,
}

#[repr(C)]
pub struct DirEntry {
    pub name_len: u16,
    pub name: [u8; 256],
}

fn map_vfs_err(err: VfsError) -> isize {
    match err {
        VfsError::NotFound => -2,
        VfsError::NotDir => -3,
        VfsError::Exists => -4,
        VfsError::NoDev => -5,
        VfsError::Busy => -6,
        VfsError::NotEmpty => -7,
        VfsError::NotSupported => -8,
        VfsError::Invalid => -9,
        VfsError::Io => -10,
    }
}

fn map_fd_err(err: FdError) -> isize {
    match err {
        FdError::NotFound => -20,
        FdError::NoSpace => -21,
        FdError::Invalid => -22,
        FdError::Io => -23,
    }
}

pub(super) fn sys_open(path: *const u8, flags: u32, _mode: u32) -> isize {
    let path = match read_cstring_from_user(path, 4096) {
        Some(s) => s,
        None => return -11,
    };
    if path.starts_with("/volumes/") {
        serial::write_bytes(b"sys_open ");
        serial::write_bytes(path.as_bytes());
        serial::write_bytes(b"\n");
    }
    if path.as_bytes().iter().any(|&b| b == b'\r') {
        logger::warn!("sys_open path contains CR: {:?}", path);
    }

    let node = match vfs_open(&path) {
        Ok(n) => n,
        Err(VfsError::NotFound) if (flags & O_CREAT) != 0 => {
            serial::write_bytes(b"sys_open create ");
            serial::write_bytes(path.as_bytes());
            serial::write_bytes(b"\n");
            if (flags & O_DIRECTORY) != 0 {
                if let Err(e) = vfs_mkdir(&path) {
                    return map_vfs_err(e);
                }
            } else if let Err(e) = vfs_mkfile(&path) {
                return map_vfs_err(e);
            }
            serial::write_bytes(b"sys_open create ok\n");
            match vfs_open(&path) {
                Ok(n) => n,
                Err(e) => {
                    logger::warn!("sys_open failed after create: path='{}' err={:?}", path, e);
                    return map_vfs_err(e);
                }
            }
        }
        Err(e) => {
            logger::warn!("sys_open failed: path='{}' err={:?}", path, e);
            return map_vfs_err(e);
        }
    };

    let handle = FileHandle::new(VfsFile::new(node), flags);
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let mut fds = current.fds.lock();
    match fds.alloc_fd(0, Arc::new(handle), false) {
        Ok(fd) => fd as isize,
        Err(e) => map_fd_err(e),
    }
}

pub(super) fn sys_close(fd: usize) -> isize {
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let mut fds = current.fds.lock();
    match fds.close_fd(fd) {
        Ok(()) => 0,
        Err(e) => map_fd_err(e),
    }
}

pub(super) fn sys_dup(fd: usize) -> isize {
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let mut fds = current.fds.lock();
    match fds.dup(fd, 0, false) {
        Ok(n) => n as isize,
        Err(e) => map_fd_err(e),
    }
}

pub(super) fn sys_pipe(pipefd: usize) -> isize {
    if !validate_user_range(pipefd, core::mem::size_of::<[i32; 2]>(), true) {
        return -1;
    }

    let (read_node, write_node) = pipefs_create_pair();
    let read_handle = FileHandle::new(VfsFile::new(read_node), 0);
    let write_handle = FileHandle::new(VfsFile::new(write_node), 0);

    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let mut fds = current.fds.lock();

    let rfd = match fds.alloc_fd(0, Arc::new(read_handle), false) {
        Ok(v) => v as i32,
        Err(e) => return map_fd_err(e),
    };
    let wfd = match fds.alloc_fd(0, Arc::new(write_handle), false) {
        Ok(v) => v as i32,
        Err(e) => {
            let _ = fds.close_fd(rfd as usize);
            return map_fd_err(e);
        }
    };

    unsafe {
        let ptr = pipefd as *mut i32;
        core::ptr::write_unaligned(ptr, rfd);
        core::ptr::write_unaligned(ptr.add(1), wfd);
    }

    0
}

pub(super) fn sys_dup2(oldfd: usize, newfd: usize) -> isize {
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let mut fds = current.fds.lock();
    match fds.dup2(oldfd, newfd, false) {
        Ok(n) => n as isize,
        Err(e) => map_fd_err(e),
    }
}

pub(super) fn sys_fcntl(fd: usize, cmd: usize, arg: usize) -> isize {
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let mut fds = current.fds.lock();
    match cmd {
        F_GETFD => {
            if fds.get_fd(fd).is_none() {
                return -1;
            }
            let clo = fds.is_cloexec(fd) as u32;
            clo as isize
        }
        F_SETFD => {
            if fds.get_fd(fd).is_none() {
                return -1;
            }
            fds.set_cloexec(fd, (arg as u32 & FD_CLOEXEC) != 0);
            0
        }
        F_GETFL => {
            let Some(h) = fds.get_fd(fd) else { return -1 };
            h.get_flags() as isize
        }
        F_SETFL => {
            let Some(h) = fds.get_fd(fd) else { return -1 };
            h.set_flags(arg as u32);
            0
        }
        _ => -1,
    }
}

pub(super) fn sys_lseek(fd: usize, offset: usize) -> isize {
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let fds = current.fds.lock();
    let Some(h) = fds.get_fd(fd) else { return -1 };
    match h.seek(offset) {
        Ok(()) => offset as isize,
        Err(_) => -1,
    }
}

pub(super) fn sys_fstat(fd: usize, buf: *mut Stat) -> isize {
    if buf.is_null() {
        return -1;
    }
    if !validate_user_range(buf as usize, core::mem::size_of::<Stat>(), true) {
        return -1;
    }
    let current = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let fds = current.fds.lock();
    let Some(h) = fds.get_fd(fd) else { return -1 };
    let Some(stat) = h.fstat() else { return -1 };
    unsafe {
        core::ptr::write_unaligned(
            buf,
            Stat {
                st_mode: stat.mode,
                st_size: stat.size,
                st_type: stat.node_type,
            },
        );
    }
    0
}

pub(super) fn sys_stat(path: *const u8, buf: *mut Stat) -> isize {
    if buf.is_null() {
        return -12;
    }
    if !validate_user_range(buf as usize, core::mem::size_of::<Stat>(), true) {
        return -12;
    }
    let path = match read_cstring_from_user(path, 4096) {
        Some(s) => s,
        None => return -11,
    };
    let node = match vfs_open(&path) {
        Ok(n) => n,
        Err(e) => return map_vfs_err(e),
    };
    let meta = node.meta.lock();
    unsafe {
        core::ptr::write_unaligned(
            buf,
            Stat {
                st_mode: meta.mode,
                st_size: meta.size,
                st_type: meta.node_type as u32,
            },
        );
    }
    0
}

pub(super) fn sys_getdents(path: *const u8, buf: *mut DirEntry, max: usize) -> isize {
    if buf.is_null() || max == 0 {
        return -12;
    }
    if !validate_user_range(buf as usize, max * core::mem::size_of::<DirEntry>(), true) {
        return -12;
    }
    let path = match read_cstring_from_user(path, 4096) {
        Some(s) => s,
        None => return -11,
    };
    if path.as_bytes().iter().any(|&b| b == b'\r') {
        logger::warn!("sys_getdents path contains CR: {:?}", path);
    }
    let node = match vfs_open(&path) {
        Ok(n) => n,
        Err(e) => {
            logger::warn!("sys_getdents open failed: path='{}' err={:?}", path, e);
            return map_vfs_err(e);
        }
    };
    let list = match vfs_list_dir(&node) {
        Ok(v) => v,
        Err(e) => {
            logger::warn!("sys_getdents list failed: path='{}' err={:?}", path, e);
            return map_vfs_err(e);
        }
    };
    let total = core::cmp::min(max, list.len());
    for (i, name) in list.iter().take(total).enumerate() {
        let entry_ptr = unsafe { buf.add(i) };
        let mut entry = DirEntry {
            name_len: 0,
            name: [0u8; 256],
        };
        let bytes = name.as_bytes();
        let n = core::cmp::min(bytes.len(), entry.name.len() - 1);
        entry.name[..n].copy_from_slice(&bytes[..n]);
        entry.name_len = n as u16;
        unsafe {
            core::ptr::write_unaligned(entry_ptr, entry);
        }
    }
    total as isize
}

pub(super) fn sys_mount(dev: *const u8, dir: *const u8, fstype: *const u8) -> isize {
    let dir = match read_cstring_from_user(dir, 4096) {
        Some(s) => s,
        None => return -1,
    };
    let fstype = match read_cstring_from_user(fstype, 64) {
        Some(s) => s,
        None => return -1,
    };
    let src = if dev.is_null() {
        None
    } else {
        read_cstring_from_user(dev, 4096)
    };

    let node = match vfs_open(&dir) {
        Ok(n) => n,
        Err(e) => return map_vfs_err(e),
    };
    match vfs_mount(src.as_deref(), &fstype, &node) {
        Ok(()) => 0,
        Err(e) => map_vfs_err(e),
    }
}

pub(super) fn sys_umount(path: *const u8) -> isize {
    let path = match read_cstring_from_user(path, 4096) {
        Some(s) => s,
        None => return -1,
    };
    match vfs_unmount(&path) {
        Ok(()) => 0,
        Err(e) => map_vfs_err(e),
    }
}

pub(super) fn sys_unlink(path: *const u8) -> isize {
    let path = match read_cstring_from_user(path, 4096) {
        Some(s) => s,
        None => return -11,
    };
    match vfs_delete(&path) {
        Ok(()) => 0,
        Err(e) => map_vfs_err(e),
    }
}

pub(super) fn sys_rename(old: *const u8, new: *const u8) -> isize {
    let old = match read_cstring_from_user(old, 4096) {
        Some(s) => s,
        None => return -11,
    };
    let new = match read_cstring_from_user(new, 4096) {
        Some(s) => s,
        None => return -11,
    };
    if new.contains('/') {
        return map_vfs_err(VfsError::Invalid);
    }
    match vfs_rename(&old, &new) {
        Ok(()) => 0,
        Err(e) => map_vfs_err(e),
    }
}
