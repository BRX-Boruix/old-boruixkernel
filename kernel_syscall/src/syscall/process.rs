use core::slice;

use kernel_platform::memory as mem;
use kernel_modules as modules;
use kernel_task::task::{self, TaskStatus};

use super::user_ptr::{read_cstring_from_user, validate_user_range};

pub(super) fn sys_yield() -> isize {
    task::suspend_current_and_run_next();
    0
}

pub(super) fn sys_getpid() -> isize {
    task::current_task().map(|t| t.pid as isize).unwrap_or(-1)
}

pub(super) fn sys_exit(code: i32) -> ! {
    task::exit_current_and_run_next(code);
    loop {
        // Should not reach here
        x86_64::instructions::hlt();
    }
}

pub(super) fn sys_fork() -> isize {
    let current_task = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };
    let new_pid = task::allocate_pid();
    match current_task.fork(new_pid) {
        Ok(child_task) => {
            task::add_task_arc(child_task);
            new_pid as isize
        }
        Err(_) => -1,
    }
}

pub(super) fn sys_exec(path: *const u8) -> isize {
    let path_str = match read_cstring_from_user(path, 256) {
        Some(s) => s,
        None => return -1,
    };
    let (base_path, exec_arg) = split_exec_arg(&path_str);

    let modules = mem::get_modules();
    for module_ptr in modules {
        let module = unsafe { &*module_ptr.as_ptr() };
        let module_path = match modules::module_path_str(module) {
            Some(p) => p,
            None => "",
        };

        if modules::module_matches(module_path, base_path) {
            let base = match module.base.as_ptr() {
                Some(p) => p,
                None => return -1,
            };
            let len = module.length as usize;
            let elf_data = unsafe { slice::from_raw_parts(base, len) };

            let current_task = match task::current_task() {
                Some(t) => t,
                None => return -1,
            };
            if current_task.exec(elf_data).is_ok() {
                current_task.set_exec_arg(exec_arg);
                return 0;
            }
            return -1;
        }
    }
    -1
}

fn split_exec_arg<'a>(s: &'a str) -> (&'a str, usize) {
    if let Some(idx) = s.find(':') {
        let (base, arg) = s.split_at(idx);
        let arg = &arg[1..];
        if arg.is_empty() {
            return (base, 0);
        }
        let mut val: usize = 0;
        for b in arg.as_bytes() {
            if *b < b'0' || *b > b'9' {
                return (base, 0);
            }
            val = val.saturating_mul(10).saturating_add((b - b'0') as usize);
        }
        return (base, val);
    }
    (s, 0)
}

pub(super) fn sys_get_exec_arg() -> isize {
    let current_task = match task::current_task() {
        Some(t) => t,
        None => return 0,
    };
    current_task.get_exec_arg() as isize
}

pub(super) fn sys_cpu_count() -> isize {
    kernel_task::smp::cpu_count() as isize
}

pub(super) fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let current_task = match task::current_task() {
        Some(t) => t,
        None => return -1,
    };

    loop {
        let mut exit_pid: Option<usize> = None;
        let mut exit_code: i32 = 0;
        let mut no_children = false;
        let mut should_block = false;

        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut children = current_task.children.lock();
            let mut found_child = false;
            let mut remove_idx = 0;

            for (idx, child) in children.iter().enumerate() {
                if pid == -1 || child.pid as isize == pid {
                    found_child = true;
                    let status = child.task_status.lock();
                    if *status == TaskStatus::Exited {
                        exit_pid = Some(child.pid);
                        exit_code = *child.exit_code.lock();
                        remove_idx = idx;
                        break;
                    }
                }
            }

            if let Some(_) = exit_pid {
                children.remove(remove_idx);
            } else if !found_child {
                no_children = true;
            } else {
                // Found children, none exited. Block.
                *current_task.task_status.lock() = TaskStatus::Waiting;
                should_block = true;
            }
        });

        if let Some(pid) = exit_pid {
            if !exit_code_ptr.is_null() {
                if !validate_user_range(exit_code_ptr as usize, core::mem::size_of::<i32>(), true) {
                    return -1;
                }
                unsafe {
                    *exit_code_ptr = exit_code;
                }
            }
            return pid as isize;
        }

        if no_children {
            return -1;
        }

        if should_block {
            let task_cx_ptr = current_task.task_cx.get();
            task::schedule(task_cx_ptr);
        }
    }
}
