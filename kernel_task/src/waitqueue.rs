use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use x86_64::instructions::interrupts;

use crate::task::{self, TaskControlBlock, TaskStatus};
use core::sync::atomic::{AtomicUsize, Ordering};

static WAKE_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
const WAKE_LOG_LIMIT: usize = 16;

pub struct WaitQueue {
    waiters: Mutex<Vec<Arc<TaskControlBlock>>>,
}

impl WaitQueue {
    pub fn new() -> Self {
        Self {
            waiters: Mutex::new(Vec::new()),
        }
    }

    pub fn wait(&self) {
        self.wait_while(|| true);
    }

    pub fn wait_while<F: Fn() -> bool>(&self, condition: F) {
        loop {
            let current = match task::current_task() {
                Some(t) => t,
                None => return,
            };
            let mut should_sleep = false;
            interrupts::without_interrupts(|| {
                if condition() {
                    *current.task_status.lock() = TaskStatus::Waiting;
                    current.in_run_queue.store(false, Ordering::Release);
                    let mut waiters = self.waiters.lock();
                    if !waiters.iter().any(|t| t.pid == current.pid) {
                        waiters.push(current.clone());
                    }
                    should_sleep = true;
                }
            });
            if !should_sleep {
                return;
            }
            let task_cx_ptr = current.task_cx.get();
            task::block_current_and_schedule(task_cx_ptr);
        }
    }

    pub fn wake_one(&self) {
        let mut waiters = self.waiters.lock();
        if let Some(task) = waiters.pop() {
            let status = *task.task_status.lock();
            let log_idx = WAKE_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
            if log_idx < WAKE_LOG_LIMIT {
                logger::println!(
                    "[KBD][WQ] wake_one #{} pid={} status={:?}",
                    log_idx + 1,
                    task.pid,
                    status
                );
            }
            drop(waiters);
            task::add_task_arc(task);
        }
    }

    pub fn wake_all(&self) {
        let mut waiters = self.waiters.lock();
        if waiters.is_empty() {
            return;
        }
        let list = core::mem::take(&mut *waiters);
        drop(waiters);
        for t in list {
            task::add_task_arc(t);
        }
    }
}
