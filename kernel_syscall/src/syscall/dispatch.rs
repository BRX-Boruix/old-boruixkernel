use kernel_platform::hal::arch;
use kernel_task::task;
use arch::TrapFrame;

use super::debug::{sys_ipi_stat, sys_kill, sys_plog, sys_ps};
use super::driver_hub::{sys_driverhub, sys_driverhub_devices, sys_driverhub_drivers};
use super::fs::{
    sys_close, sys_dup, sys_dup2, sys_fcntl, sys_fstat, sys_getdents, sys_lseek, sys_mount,
    sys_open, sys_rename, sys_stat, sys_umount, sys_unlink,
};
use super::fs::sys_pipe;
use super::io::{sys_poll, sys_read, sys_select, sys_write};
#[cfg(feature = "debug-syscall")]
use super::memory::sys_testpmm2m;
use super::memory::{sys_migrate_one, sys_mmcompact, sys_mmstat, sys_rmapcount, sys_sbrk};
use super::net::{
    sys_accept, sys_bind, sys_connect, sys_listen, sys_net_counters, sys_net_mac, sys_net_recv,
    sys_net_regs, sys_net_send, sys_net_status, sys_socket,
};
use super::pci::{sys_pci_count, sys_pci_info, sys_pci_list, sys_pci_mode};
use super::process::{
    sys_cpu_count, sys_exec, sys_exit, sys_fork, sys_get_exec_arg, sys_getpid, sys_waitpid,
    sys_yield,
};
use super::time::{sys_time_human, sys_time_seconds};

pub extern "C" fn syscall_stack_switch(tf: *mut TrapFrame) -> *mut TrapFrame {
    if tf.is_null() {
        return tf;
    }
    let task = match task::current_task() {
        Some(t) => t,
        None => return tf,
    };

    // Single source of truth: always use the task's TrapFrame on its kernel stack.
    let dst = task.trap_frame_mut() as *mut TrapFrame;
    if tf == dst {
        return tf;
    }
    unsafe {
        *dst = *tf;
    }
    if arch::syscall::warn_syscall_tf_moved_once() {
        logger::warn!("syscall tf not on task kernel stack; moved");
    }
    dst
}

pub extern "C" fn syscall_dispatch(tf: &mut TrapFrame) {
    let syscall_id = tf.rax;
    // Don't log syscalls, too noisy
    // println!("SYSCALL: id={}, arg0={}, arg1={}, arg2={}", syscall_id, tf.rdi, tf.rsi, tf.rdx);
    match syscall_id {
        0 => {
            // sys_read
            tf.rax = sys_read(tf.rdi, tf.rsi, tf.rdx) as usize;
        }
        1 => {
            // sys_write
            tf.rax = sys_write(tf.rdi, tf.rsi, tf.rdx) as usize;
        }
        2 => {
            // sys_open
            tf.rax = sys_open(tf.rdi as *const u8, tf.rsi as u32, tf.rdx as u32) as usize;
        }
        3 => {
            // sys_close
            tf.rax = sys_close(tf.rdi) as usize;
        }
        4 => {
            // sys_stat
            tf.rax = sys_stat(tf.rdi as *const u8, tf.rsi as *mut super::fs::Stat) as usize;
        }
        5 => {
            // sys_fstat
            tf.rax = sys_fstat(tf.rdi, tf.rsi as *mut super::fs::Stat) as usize;
        }
        6 => {
            // sys_getdents
            tf.rax = sys_getdents(tf.rdi as *const u8, tf.rsi as *mut super::fs::DirEntry, tf.rdx)
                as usize;
        }
        8 => {
            // sys_lseek
            tf.rax = sys_lseek(tf.rdi, tf.rsi) as usize;
        }
        7 => {
            // sys_poll
            tf.rax = sys_poll(tf.rdi, tf.rsi, tf.rdx as isize) as usize;
        }
        22 => {
            // sys_pipe
            tf.rax = sys_pipe(tf.rdi) as usize;
        }
        23 => {
            // sys_select
            tf.rax = sys_select(tf.rdi, tf.rsi, tf.rdx, tf.r10, tf.r8) as usize;
        }
        24 | 124 => {
            // sys_yield
            tf.rax = sys_yield() as usize;
        }
        39 => {
            // sys_getpid
            tf.rax = sys_getpid() as usize;
        }
        32 => {
            // sys_dup
            tf.rax = sys_dup(tf.rdi) as usize;
        }
        33 => {
            // sys_dup2
            tf.rax = sys_dup2(tf.rdi, tf.rsi) as usize;
        }
        41 => {
            // sys_socket
            tf.rax = sys_socket(tf.rdi, tf.rsi, tf.rdx) as usize;
        }
        42 => {
            // sys_connect
            tf.rax = sys_connect(tf.rdi, tf.rsi, tf.rdx) as usize;
        }
        43 => {
            // sys_accept
            tf.rax = sys_accept(tf.rdi, tf.rsi, tf.rdx) as usize;
        }
        57 => {
            // sys_fork
            tf.rax = sys_fork() as usize;
        }
        49 => {
            // sys_bind
            tf.rax = sys_bind(tf.rdi, tf.rsi, tf.rdx) as usize;
        }
        50 => {
            // sys_listen
            tf.rax = sys_listen(tf.rdi, tf.rsi) as usize;
        }
        72 => {
            // sys_fcntl
            tf.rax = sys_fcntl(tf.rdi, tf.rsi, tf.rdx) as usize;
        }
        59 => {
            // sys_exec
            tf.rax = sys_exec(tf.rdi as *const u8) as usize;
        }
        60 => {
            // sys_exit
            sys_exit(tf.rdi as i32);
        }
        61 => {
            // sys_waitpid
            tf.rax = sys_waitpid(tf.rdi as isize, tf.rsi as *mut i32) as usize;
        }
        200 => {
            // sys_ps
            tf.rax = sys_ps(tf.rdi as *mut u8, tf.rsi as usize) as usize;
        }
        201 => {
            // sys_kill
            tf.rax = sys_kill(tf.rdi as usize, tf.rsi as i32) as usize;
        }
        210 => {
            // sys_testmm1
            tf.rax = crate::test_vm::test_mm1() as usize;
        }
        211 => {
            // sys_plog
            tf.rax = sys_plog(tf.rdi, tf.rsi as *const u8, tf.rdx as usize) as usize;
        }
        212 => {
            // sys_mmstat
            tf.rax = sys_mmstat(tf.rdi as *mut u8, tf.rsi as usize, tf.rdx as usize) as usize;
        }
        213 => {
            // sys_get_exec_arg
            tf.rax = sys_get_exec_arg() as usize;
        }
        214 => {
            // sys_cpu_count
            tf.rax = sys_cpu_count() as usize;
        }
        215 => {
            // sys_ipi_stat
            tf.rax = sys_ipi_stat(tf.rdi as *mut u8, tf.rsi as usize) as usize;
        }
        216 => {
            // sys_mmcompact
            tf.rax = sys_mmcompact() as usize;
        }
        217 => {
            // sys_sbrk
            tf.rax = sys_sbrk(tf.rdi as isize) as usize;
        }
        218 => {
            // sys_testpmm2m
            #[cfg(feature = "debug-syscall")]
            {
                tf.rax = sys_testpmm2m(tf.rdi as usize) as usize;
            }
            #[cfg(not(feature = "debug-syscall"))]
            {
                tf.rax = (-1isize) as usize;
            }
        }
        221 => {
            tf.rax = sys_time_human(tf.rdi as *mut u8, tf.rsi as usize) as usize;
        }
        222 => {
            tf.rax = sys_time_seconds() as usize;
        }
        223 => {
            tf.rax = sys_pci_list(
                tf.rdi as *mut kernel_driver_hub::driver_hub::pci::PciDeviceInfo,
                tf.rsi as usize,
            ) as usize;
        }
        224 => {
            tf.rax = sys_pci_info(
                tf.rdi as u8,
                tf.rsi as u8,
                tf.rdx as u8,
                tf.r10 as *mut kernel_driver_hub::driver_hub::pci::PciDeviceInfo,
            ) as usize;
        }
        225 => {
            tf.rax = sys_pci_mode() as usize;
        }
        226 => {
            tf.rax = sys_pci_count() as usize;
        }
        227 => {
            tf.rax = sys_driverhub(tf.rdi as *mut u8, tf.rsi as usize, tf.rdx as usize) as usize;
        }
        228 => {
            tf.rax = sys_driverhub_drivers(
                tf.rdi as *mut kernel_driver_hub::driver_hub::DriverInfoRaw,
                tf.rsi as usize,
            ) as usize;
        }
        229 => {
            tf.rax = sys_driverhub_devices(
                tf.rdi as *mut kernel_driver_hub::driver_hub::DeviceInfoRaw,
                tf.rsi as usize,
            ) as usize;
        }
        230 => {
            tf.rax = sys_net_send(tf.rdi as *const u8, tf.rsi as usize) as usize;
        }
        231 => {
            tf.rax = sys_net_recv(tf.rdi as *mut u8, tf.rsi as usize) as usize;
        }
        232 => {
            tf.rax = sys_net_mac(tf.rdi as *mut u8, tf.rsi as usize) as usize;
        }
        233 => {
            tf.rax = sys_net_status() as usize;
        }
        234 => {
            tf.rax = sys_net_regs(tf.rdi as *mut u32, tf.rsi as usize) as usize;
        }
        235 => {
            tf.rax = sys_net_counters(tf.rdi as *mut u64, tf.rsi as usize) as usize;
        }
        219 => {
            // sys_migrate_one
            tf.rax = sys_migrate_one() as usize;
        }
        165 => {
            // sys_mount
            tf.rax = sys_mount(tf.rdi as *const u8, tf.rsi as *const u8, tf.rdx as *const u8)
                as usize;
        }
        166 => {
            // sys_umount
            tf.rax = sys_umount(tf.rdi as *const u8) as usize;
        }
        167 => {
            // sys_unlink
            tf.rax = sys_unlink(tf.rdi as *const u8) as usize;
        }
        168 => {
            // sys_rename
            tf.rax = sys_rename(tf.rdi as *const u8, tf.rsi as *const u8) as usize;
        }
        220 => {
            // sys_rmapcount
            tf.rax = sys_rmapcount() as usize;
        }
        _ => {
            // Unknown syscall
            tf.rax = usize::MAX; // -1
        }
    }

    // Deferred preemption: only switch at safe point on syscall exit.
    task::check_preempt_from_syscall(tf);
}
