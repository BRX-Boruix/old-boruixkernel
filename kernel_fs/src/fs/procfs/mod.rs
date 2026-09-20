use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Write;

use kernel_task::task;
use kernel_platform::memory;

use crate::vfs::{vfs_child_append, VfsError, VfsNode, VfsNodeType, VfsOps, VfsResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcKind {
    Dir,
    Tasks,
    MemInfo,
    CpuInfo,
    SelfLink,
    PidDir,
    PidStatus,
}

struct ProcHandle {
    kind: ProcKind,
    pid: Option<usize>,
}

fn set_handle(node: &Arc<VfsNode>, handle: ProcHandle) {
    let ptr = Box::into_raw(Box::new(handle));
    node.meta.lock().handle = Some(ptr as usize);
}

fn get_handle(node: &Arc<VfsNode>) -> VfsResult<&'static mut ProcHandle> {
    let ptr = node.meta.lock().handle.ok_or(VfsError::Invalid)? as *mut ProcHandle;
    if ptr.is_null() {
        return Err(VfsError::Invalid);
    }
    Ok(unsafe { &mut *ptr })
}

fn drop_handle(node: &Arc<VfsNode>) {
    let ptr = node.meta.lock().handle.take().unwrap_or(0) as *mut ProcHandle;
    if !ptr.is_null() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    }
}

fn build_tasks() -> Vec<u8> {
    let list = task::list_tasks();
    let mut out = String::new();
    for (pid, status) in list {
        out.push_str(&format!("{} {:?}\n", pid, status));
    }
    out.into_bytes()
}

fn build_meminfo() -> Vec<u8> {
    let stats = memory::pmm::frame_stats();
    let frag = memory::pmm::frag_stats();
    let free_global: usize = frag.free_global_by_order.iter().sum();
    let free_percpu: usize = frag.free_percpu_by_order.iter().sum();
    let free_frames: usize = free_global + free_percpu + frag.uninit_frames;
    let total_frames = stats.allocated_frames + free_frames;
    let used_frames = stats.allocated_frames;
    let mem_total_kb = total_frames * 4;
    let mem_free_kb = free_frames * 4;
    let mem_used_kb = used_frames * 4;
    let mut out = String::new();
    let _ = writeln!(out, "MemTotal: {} kB", mem_total_kb);
    let _ = writeln!(out, "MemFree: {} kB", mem_free_kb);
    let _ = writeln!(out, "MemAvailable: {} kB", mem_free_kb);
    let _ = writeln!(out, "MemUsed: {} kB", mem_used_kb);
    let _ = writeln!(out, "FramesTotal: {}", total_frames);
    let _ = writeln!(out, "FramesAllocated: {}", stats.allocated_frames);
    let _ = writeln!(out, "FramesFreeGlobal: {}", free_global);
    let _ = writeln!(out, "FramesFreePerCpu: {}", free_percpu);
    let _ = writeln!(out, "FramesUninit: {}", frag.uninit_frames);
    let _ = writeln!(out, "AllocCalls: {}", stats.alloc_calls);
    let _ = writeln!(out, "AllocFail: {}", stats.alloc_fail);
    let _ = writeln!(out, "CompactCalls: {}", stats.compact_calls);
    let _ = writeln!(out, "CompactSuccess: {}", stats.compact_success);
    out.into_bytes()
}

fn build_cpuinfo() -> Vec<u8> {
    let count = kernel_task::smp::cpu_count();
    let mut out = String::new();
    for i in 0..count {
        let _ = writeln!(out, "processor\t: {}", i);
        let _ = writeln!(out, "model name\t: unknown");
        let _ = writeln!(out, "cpu cores\t: {}", count);
        let _ = writeln!(out, "");
    }
    out.into_bytes()
}

fn build_status(pid: usize) -> Vec<u8> {
    let list = task::list_tasks();
    let mut state = "Unknown";
    for (p, status) in list {
        if p == pid {
            state = match status {
                kernel_task::task::TaskStatus::Ready => "Ready",
                kernel_task::task::TaskStatus::Running => "Running",
                kernel_task::task::TaskStatus::Exited => "Exited",
                kernel_task::task::TaskStatus::Waiting => "Waiting",
            };
            break;
        }
    }
    let mut out = String::new();
    let _ = writeln!(out, "Name:\tpid{}", pid);
    let _ = writeln!(out, "Pid:\t{}", pid);
    let _ = writeln!(out, "State:\t{}", state);
    out.into_bytes()
}

fn parse_pid(name: &str) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    let mut pid: usize = 0;
    for b in name.as_bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        pid = pid.saturating_mul(10).saturating_add((b - b'0') as usize);
    }
    Some(pid)
}

fn current_pid() -> usize {
    task::current_task().map(|t| t.pid).unwrap_or(1)
}

pub struct ProcFs;

impl ProcFs {
    pub fn new() -> Self {
        Self
    }
}

impl VfsOps for ProcFs {
    fn mount(&self, _src: Option<&str>, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(
            node,
            ProcHandle {
                kind: ProcKind::Dir,
                pid: None,
            },
        );
        node.meta.lock().node_type = VfsNodeType::Dir;
        let tasks = vfs_child_append(node, "tasks");
        set_handle(
            &tasks,
            ProcHandle {
                kind: ProcKind::Tasks,
                pid: None,
            },
        );
        tasks.meta.lock().node_type = VfsNodeType::Stream;
        let meminfo = vfs_child_append(node, "meminfo");
        set_handle(
            &meminfo,
            ProcHandle {
                kind: ProcKind::MemInfo,
                pid: None,
            },
        );
        meminfo.meta.lock().node_type = VfsNodeType::Stream;
        let cpuinfo = vfs_child_append(node, "cpuinfo");
        set_handle(
            &cpuinfo,
            ProcHandle {
                kind: ProcKind::CpuInfo,
                pid: None,
            },
        );
        cpuinfo.meta.lock().node_type = VfsNodeType::Stream;
        let self_node = vfs_child_append(node, "self");
        set_handle(
            &self_node,
            ProcHandle {
                kind: ProcKind::SelfLink,
                pid: None,
            },
        );
        {
            let mut meta = self_node.meta.lock();
            meta.node_type = VfsNodeType::Symlink;
            meta.linkto_path = Some(format!("{}", current_pid()));
        }
        Ok(())
    }

    fn unmount(&self, node: &Arc<VfsNode>) -> VfsResult {
        drop_handle(node);
        Ok(())
    }

    fn mkdir(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(
            node,
            ProcHandle {
                kind: ProcKind::Dir,
                pid: None,
            },
        );
        node.meta.lock().node_type = VfsNodeType::Dir;
        Ok(())
    }

    fn mkfile(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_handle(
            node,
            ProcHandle {
                kind: ProcKind::Tasks,
                pid: None,
            },
        );
        node.meta.lock().node_type = VfsNodeType::Stream;
        Ok(())
    }

    fn read(&self, node: &Arc<VfsNode>, offset: usize, buf: &mut [u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        match handle.kind {
            ProcKind::Dir => Err(VfsError::Invalid),
            ProcKind::Tasks => {
                let data = build_tasks();
                if offset >= data.len() {
                    return Ok(0);
                }
                let end = core::cmp::min(data.len(), offset + buf.len());
                let slice = &data[offset..end];
                buf[..slice.len()].copy_from_slice(slice);
                Ok(slice.len())
            }
            ProcKind::MemInfo => {
                let data = build_meminfo();
                if offset >= data.len() {
                    return Ok(0);
                }
                let end = core::cmp::min(data.len(), offset + buf.len());
                let slice = &data[offset..end];
                buf[..slice.len()].copy_from_slice(slice);
                Ok(slice.len())
            }
            ProcKind::CpuInfo => {
                let data = build_cpuinfo();
                if offset >= data.len() {
                    return Ok(0);
                }
                let end = core::cmp::min(data.len(), offset + buf.len());
                let slice = &data[offset..end];
                buf[..slice.len()].copy_from_slice(slice);
                Ok(slice.len())
            }
            ProcKind::PidStatus => {
                let pid = handle.pid.unwrap_or(0);
                let data = build_status(pid);
                if offset >= data.len() {
                    return Ok(0);
                }
                let end = core::cmp::min(data.len(), offset + buf.len());
                let slice = &data[offset..end];
                buf[..slice.len()].copy_from_slice(slice);
                Ok(slice.len())
            }
            ProcKind::SelfLink | ProcKind::PidDir => Err(VfsError::Invalid),
        }
    }

    fn readlink(&self, node: &Arc<VfsNode>, buf: &mut [u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        if handle.kind != ProcKind::SelfLink {
            return Err(VfsError::Invalid);
        }
        let pid = current_pid();
        let s = format!("{}", pid);
        let bytes = s.as_bytes();
        let n = core::cmp::min(bytes.len(), buf.len());
        buf[..n].copy_from_slice(&bytes[..n]);
        Ok(n)
    }

    fn open(&self, parent: Option<&Arc<VfsNode>>, name: &str, node: &Arc<VfsNode>) -> VfsResult {
        let Some(parent) = parent else { return Err(VfsError::NotFound) };
        let p = get_handle(parent)?;
        match p.kind {
            ProcKind::Dir => match name {
                "tasks" => {
                    set_handle(
                        node,
                        ProcHandle {
                            kind: ProcKind::Tasks,
                            pid: None,
                        },
                    );
                    node.meta.lock().node_type = VfsNodeType::Stream;
                    Ok(())
                }
                "meminfo" => {
                    set_handle(
                        node,
                        ProcHandle {
                            kind: ProcKind::MemInfo,
                            pid: None,
                        },
                    );
                    node.meta.lock().node_type = VfsNodeType::Stream;
                    Ok(())
                }
                "cpuinfo" => {
                    set_handle(
                        node,
                        ProcHandle {
                            kind: ProcKind::CpuInfo,
                            pid: None,
                        },
                    );
                    node.meta.lock().node_type = VfsNodeType::Stream;
                    Ok(())
                }
                "self" => {
                    set_handle(
                        node,
                        ProcHandle {
                            kind: ProcKind::SelfLink,
                            pid: None,
                        },
                    );
                    let mut meta = node.meta.lock();
                    meta.node_type = VfsNodeType::Symlink;
                    meta.linkto_path = Some(format!("{}", current_pid()));
                    Ok(())
                }
                _ => {
                    if let Some(pid) = parse_pid(name) {
                        set_handle(
                            node,
                            ProcHandle {
                                kind: ProcKind::PidDir,
                                pid: Some(pid),
                            },
                        );
                        node.meta.lock().node_type = VfsNodeType::Dir;
                        Ok(())
                    } else {
                        Err(VfsError::NotFound)
                    }
                }
            },
            ProcKind::PidDir => {
                if name == "status" {
                    set_handle(
                        node,
                        ProcHandle {
                            kind: ProcKind::PidStatus,
                            pid: p.pid,
                        },
                    );
                    node.meta.lock().node_type = VfsNodeType::Stream;
                    Ok(())
                } else {
                    Err(VfsError::NotFound)
                }
            }
            _ => Err(VfsError::NotFound),
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
}
