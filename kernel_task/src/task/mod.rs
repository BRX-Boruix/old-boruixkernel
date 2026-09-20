pub mod context;
pub mod switch;
pub mod task;

mod fault_hooks;
mod preempt;
mod runqueue;
mod scheduler;
mod switch_path;

use kernel_platform::hal::arch;

pub use task::{TaskControlBlock, TaskStatus};

pub use scheduler::{
    add_task, add_task_arc, allocate_pid, block_current_and_schedule, current_task,
    exit_current_and_run_next, kill_task, list_tasks, run_tasks, schedule, set_kill_hook,
    suspend_current_and_run_next,
};

pub use preempt::{
    check_interrupt_trapframe, check_preempt_from_syscall, enable_preempt, handle_resched_ipi,
    handle_timer, mark_need_resched_current, preempt_disable, preempt_enable, preempt_enabled,
};

pub fn init() {
    fault_hooks::init();
    preempt::init();
}

pub(super) const MAX_CPUS: usize = 64;

#[inline(always)]
pub(super) fn current_cpu_id() -> usize {
    let id = arch::current_cpu_id_raw();
    if id < MAX_CPUS {
        id
    } else {
        0
    }
}
