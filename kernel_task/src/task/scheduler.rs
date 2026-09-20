use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::{Lazy, Mutex};

use super::context::TaskContext;
use super::preempt::{notify_resched, with_preempt_disabled};
use super::runqueue::{
    enqueue_ready, fetch_next_task, inc_ready, task_manager, READY_COUNTS, TASK_MANAGERS,
};
use super::switch;
use super::switch_path::{restore_fpu_state, save_fpu_state};
use super::{current_cpu_id, MAX_CPUS};
use super::{TaskControlBlock, TaskStatus};
use crate::error::TaskError;

use x86_64::instructions::interrupts;

static KILL_HOOK: AtomicUsize = AtomicUsize::new(0);

pub fn set_kill_hook(hook: Option<fn()>) {
    let ptr = hook.map(|f| f as usize).unwrap_or(0);
    KILL_HOOK.store(ptr, Ordering::Release);
}

fn run_kill_hook() {
    let ptr = KILL_HOOK.load(Ordering::Acquire);
    if ptr != 0 {
        let hook: fn() = unsafe { core::mem::transmute(ptr) };
        hook();
    }
}

pub struct Processor {
    current: Option<Arc<TaskControlBlock>>,
    idle_task_cx: TaskContext,
}

impl Processor {
    pub fn new() -> Self {
        Self {
            current: None,
            idle_task_cx: TaskContext::zero(),
        }
    }

    pub fn current(&self) -> Option<Arc<TaskControlBlock>> {
        self.current.clone()
    }
}

pub static PROCESSORS: Lazy<Vec<Mutex<Processor>>> = Lazy::new(|| {
    let mut v = Vec::with_capacity(MAX_CPUS);
    for _ in 0..MAX_CPUS {
        v.push(Mutex::new(Processor::new()));
    }
    v
});

pub static ALL_TASKS: Lazy<Mutex<Vec<Arc<TaskControlBlock>>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

static PID_COUNTER: AtomicUsize = AtomicUsize::new(1);

#[inline(always)]
pub(super) fn processor() -> &'static Mutex<Processor> {
    &PROCESSORS[current_cpu_id()]
}

pub(super) fn set_current_task(task: Arc<TaskControlBlock>) {
    *task.task_status.lock() = TaskStatus::Running;
    let mut proc = processor().lock();
    proc.current = Some(task);
}

pub(super) fn clear_current_task() {
    let mut proc = processor().lock();
    proc.current = None;
}

pub fn suspend_current_and_run_next() {
    let task = match current_task() {
        Some(t) => t,
        None => return,
    };

    save_fpu_state(&task);

    // Push back to ready queue
    enqueue_ready(task.clone());
    // We are about to switch to idle; clear current to avoid stale preempt.
    with_preempt_disabled(|| {
        clear_current_task();
    });

    // Schedule
    let task_cx_ptr = task.task_cx.get();
    schedule(task_cx_ptr);
}

pub fn exit_current_and_run_next(exit_code: i32) {
    let task = match current_task() {
        Some(t) => t,
        None => return,
    };
    logger::println!("PID {} Exiting with code {}", task.pid, exit_code);

    save_fpu_state(&task);
    task.fds.lock().close_all();

    // Change status to Exited
    {
        *task.task_status.lock() = TaskStatus::Exited;
        *task.exit_code.lock() = exit_code;
    }

    // Keep in ALL_TASKS for ps/join visibility; status is enough.

    // Wake up parent if waiting
    {
        let parent = task.parent.lock();
        if let Some(parent_weak) = parent.as_ref() {
            if let Some(parent_task) = parent_weak.upgrade() {
                let mut parent_status = parent_task.task_status.lock();
                if *parent_status == TaskStatus::Waiting {
                    *parent_status = TaskStatus::Ready;
                    // Add to ready queue
                    if enqueue_ready(parent_task.clone()) {
                        notify_resched();
                    }
                }
            }
        }
    }

    // Schedule
    with_preempt_disabled(|| {
        clear_current_task();
    });
    let task_cx_ptr = task.task_cx.get();
    schedule(task_cx_ptr);
}

pub fn schedule(switched_task_cx_ptr: *mut TaskContext) {
    let idle_cx_ptr = interrupts::without_interrupts(|| {
        let mut processor = processor().lock();
        &mut processor.idle_task_cx as *mut TaskContext
    });

    unsafe {
        switch::__switch(switched_task_cx_ptr, idle_cx_ptr);
    }
}

pub fn block_current_and_schedule(task_cx_ptr: *mut TaskContext) {
    with_preempt_disabled(|| {
        clear_current_task();
    });
    x86_64::instructions::interrupts::enable();
    schedule(task_cx_ptr);
}

pub fn run_tasks() {
    loop {
        interrupts::enable(); // Ensure interrupts are enabled in idle loop

        let task = fetch_next_task();

        if let Some(task) = task {
            restore_fpu_state(&task);
            with_preempt_disabled(|| {
                set_current_task(task.clone());
            });

            let current_task = &task;

            let idle_cx_ptr = with_preempt_disabled(|| {
                let mut processor = processor().lock();
                &mut processor.idle_task_cx as *mut TaskContext
            });

            let next_cx_ptr = current_task.task_cx.get();

            // Activate address space
            unsafe {
                (*current_task.memory_set.get()).activate();
            }
            let p4 = unsafe { (*current_task.memory_set.get()).token() };
            crate::smp::set_current_cr3(p4);

            // Set kernel stack for syscalls
            kernel_platform::hal::arch::set_kernel_stack(current_task.kernel_stack);

            // logger::println!("Switching to PID {}", current_task.pid);

            unsafe {
                switch::__switch(idle_cx_ptr, next_cx_ptr);
            }

            // Back from task
            with_preempt_disabled(|| {
                clear_current_task();
            });
        } else {
            x86_64::instructions::hlt();
        }
    }
}

pub fn add_task(elf_data: &[u8]) -> Result<usize, TaskError> {
    let pid = allocate_pid();
    let task = Arc::new(TaskControlBlock::new(elf_data, pid)?);
    {
        let mut list = ALL_TASKS.lock();
        if !list.iter().any(|t| t.pid == task.pid) {
            list.push(task.clone());
        }
    }
    with_preempt_disabled(|| {
        task_manager().lock().add(task);
    });
    inc_ready(current_cpu_id(), 1);
    notify_resched();
    Ok(pid)
}

pub fn add_task_arc(task: Arc<TaskControlBlock>) {
    if enqueue_ready(task) {
        notify_resched();
    }
}

pub fn allocate_pid() -> usize {
    PID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

pub fn current_task() -> Option<Arc<TaskControlBlock>> {
    interrupts::without_interrupts(|| processor().lock().current())
}

pub fn list_tasks() -> Vec<(usize, TaskStatus)> {
    let list = ALL_TASKS.lock();
    list.iter()
        .map(|t| (t.pid, *t.task_status.lock()))
        .collect()
}

pub fn kill_task(pid: usize, code: i32) -> bool {
    let current = current_task();
    if let Some(ct) = &current {
        if ct.pid == pid {
            exit_current_and_run_next(code);
            return true;
        }
    }

    let list = ALL_TASKS.lock();
    let target = list.iter().find(|t| t.pid == pid).cloned();
    drop(list);
    let Some(task) = target else {
        return false;
    };

    *task.task_status.lock() = TaskStatus::Exited;
    *task.exit_code.lock() = code;
    task.in_run_queue.store(false, Ordering::Release);
    // Wake readers blocked in I/O waitqueues so kill does not leave
    // waiters parked forever.
    run_kill_hook();

    // Wake parent if waiting
    let parent = task.parent.lock();
    if let Some(parent_weak) = parent.as_ref() {
        if let Some(parent_task) = parent_weak.upgrade() {
            let waiting = {
                let parent_status = parent_task.task_status.lock();
                *parent_status == TaskStatus::Waiting
            };
            if waiting {
                if enqueue_ready(parent_task.clone()) {
                    notify_resched();
                }
            }
        }
    }

    // Remove from ready queue if present
    with_preempt_disabled(|| {
        for (i, mgr) in TASK_MANAGERS.iter().enumerate() {
            let mut mgr = mgr.lock();
            mgr.ready_queue.retain(|t| t.pid != pid);
            READY_COUNTS[i].store(mgr.ready_queue.len(), Ordering::Relaxed);
        }
    });

    true
}
