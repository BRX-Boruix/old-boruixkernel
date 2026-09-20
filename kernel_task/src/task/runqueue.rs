use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::{Lazy, Mutex};

use super::preempt::with_preempt_disabled;
use super::{current_cpu_id, TaskControlBlock, TaskStatus, MAX_CPUS};

pub struct TaskManager {
    pub(super) ready_queue: alloc::collections::VecDeque<Arc<TaskControlBlock>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            ready_queue: alloc::collections::VecDeque::new(),
        }
    }

    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }

    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        if self.ready_queue.is_empty() {
            return None;
        }

        let mut min_pass = usize::MAX;
        let mut min_idx = 0;

        for (idx, task) in self.ready_queue.iter().enumerate() {
            let pass = *task.pass.lock();
            if pass < min_pass {
                min_pass = pass;
                min_idx = idx;
            }
        }

        let task = match self.ready_queue.remove(min_idx) {
            Some(t) => t,
            None => return None,
        };
        task.in_run_queue.store(false, Ordering::Release);

        let stride = task.stride;
        {
            let mut pass = task.pass.lock();
            *pass = pass.wrapping_add(stride);
        }

        Some(task)
    }

}

pub static TASK_MANAGERS: Lazy<Vec<Mutex<TaskManager>>> = Lazy::new(|| {
    let mut v = Vec::with_capacity(MAX_CPUS);
    for _ in 0..MAX_CPUS {
        v.push(Mutex::new(TaskManager::new()));
    }
    v
});

pub(super) static READY_COUNTS: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];

#[inline(always)]
pub(super) fn task_manager() -> &'static Mutex<TaskManager> {
    &TASK_MANAGERS[current_cpu_id()]
}

#[inline(always)]
pub(super) fn inc_ready(cpu: usize, n: usize) {
    if cpu < MAX_CPUS {
        READY_COUNTS[cpu].fetch_add(n, Ordering::Relaxed);
    }
}

#[inline(always)]
pub(super) fn dec_ready(cpu: usize, n: usize) {
    if cpu < MAX_CPUS {
        READY_COUNTS[cpu].fetch_sub(n, Ordering::Relaxed);
    }
}

#[inline(always)]
fn get_ready(cpu: usize) -> usize {
    if cpu < MAX_CPUS {
        READY_COUNTS[cpu].load(Ordering::Relaxed)
    } else {
        0
    }
}

pub(super) fn enqueue_ready(task: Arc<TaskControlBlock>) -> bool {
    if task.in_run_queue.swap(true, Ordering::AcqRel) {
        return false;
    }
    let mut status = task.task_status.lock();
    if *status == TaskStatus::Exited {
        task.in_run_queue.store(false, Ordering::Release);
        return false;
    }
    *status = TaskStatus::Ready;
    drop(status);
    with_preempt_disabled(|| {
        task_manager().lock().add(task);
    });
    inc_ready(current_cpu_id(), 1);
    true
}

pub(super) fn fetch_next_task() -> Option<Arc<TaskControlBlock>> {
    with_preempt_disabled(|| {
        if let Some(t) = task_manager().lock().fetch() {
            dec_ready(current_cpu_id(), 1);
            return Some(t);
        }

        // Work stealing: pick the most loaded CPU and steal a batch
        let cpu = current_cpu_id();
        let count = crate::smp::cpu_count();
        let max = count.min(MAX_CPUS);
        let mut best_cpu: Option<usize> = None;
        let mut best_len: usize = 0;

        for i in 0..max {
            if i == cpu {
                continue;
            }
            let len = get_ready(i);
            if len > best_len {
                best_len = len;
                best_cpu = Some(i);
            }
        }

        if let Some(i) = best_cpu {
            // Only steal if the victim is significantly loaded
            if best_len >= 2 {
                if let Some(mut victim) = TASK_MANAGERS[i].try_lock() {
                    let mut to_steal = best_len / 2;
                    if to_steal == 0 {
                        to_steal = 1;
                    }
                    let mut first: Option<Arc<TaskControlBlock>> = None;
                    let mut local = task_manager().lock();
                    for _ in 0..to_steal {
                        if let Some(t) = victim.fetch() {
                            dec_ready(i, 1);
                            let status = *t.task_status.lock();
                            if status == TaskStatus::Ready {
                                if first.is_none() {
                                    first = Some(t);
                                } else {
                                    local.add(t);
                                    inc_ready(cpu, 1);
                                }
                            } else {
                                // Not runnable; put back to victim queue
                                victim.add(t);
                                inc_ready(i, 1);
                            }
                        } else {
                            break;
                        }
                    }
                    if let Some(t) = first {
                        return Some(t);
                    }
                }
            }
        }
        None
    })
}

pub(super) fn fetch_next_task_try() -> Option<Arc<TaskControlBlock>> {
    if let Some(mut mgr) = task_manager().try_lock() {
        if let Some(t) = mgr.fetch() {
            dec_ready(current_cpu_id(), 1);
            return Some(t);
        }
    } else {
        return None;
    }

    // Work stealing: pick the most loaded CPU and steal a batch
    let cpu = current_cpu_id();
    let count = crate::smp::cpu_count();
    let max = count.min(MAX_CPUS);
    let mut best_cpu: Option<usize> = None;
    let mut best_len: usize = 0;

    for i in 0..max {
        if i == cpu {
            continue;
        }
        let len = get_ready(i);
        if len > best_len {
            best_len = len;
            best_cpu = Some(i);
        }
    }

    if let Some(i) = best_cpu {
        if best_len >= 2 {
            if let Some(mut victim) = TASK_MANAGERS[i].try_lock() {
                let mut to_steal = best_len / 2;
                if to_steal == 0 {
                    to_steal = 1;
                }
                for _ in 0..to_steal {
                    if let Some(t) = victim.fetch() {
                        dec_ready(i, 1);
                        let status = *t.task_status.lock();
                        if status == TaskStatus::Ready {
                            return Some(t);
                        } else {
                            victim.add(t);
                            inc_ready(i, 1);
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    }
    None
}
