use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};

use x86_64::instructions::interrupts;

use kernel_platform::hal::arch;

use super::current_cpu_id;
use super::runqueue::{enqueue_ready, fetch_next_task_try};
use super::scheduler::{processor, set_current_task, suspend_current_and_run_next};
use super::switch_path::arch_task_switch;
use super::MAX_CPUS;

static PREEMPT_ENABLED: AtomicBool = AtomicBool::new(false);
static NEED_RESCHED: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];
static WARN_IRQ_TF_MISMATCH: AtomicBool = AtomicBool::new(false);
static INTERRUPT_PREEMPT_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn enable_preempt() {
    PREEMPT_ENABLED.store(true, Ordering::SeqCst);
    INTERRUPT_PREEMPT_ENABLED.store(config::ENABLE_INTERRUPT_PREEMPT, Ordering::SeqCst);
}

pub fn preempt_enabled() -> bool {
    PREEMPT_ENABLED.load(Ordering::Relaxed)
}

pub fn preempt_disable() {
    arch::preempt_disable();
}

pub fn preempt_enable() {
    arch::preempt_enable();
}

fn preempt_count() -> usize {
    arch::preempt_count()
}

fn set_need_resched(cpu: usize) {
    if cpu < MAX_CPUS {
        NEED_RESCHED[cpu].store(true, Ordering::Relaxed);
    }
}

fn take_need_resched(cpu: usize) -> bool {
    if cpu < MAX_CPUS {
        NEED_RESCHED[cpu].swap(false, Ordering::Relaxed)
    } else {
        false
    }
}

pub fn mark_need_resched_current() {
    set_need_resched(current_cpu_id());
}

pub(super) fn notify_resched() {
    let count = crate::smp::cpu_count();
    for cpu in 0..count {
        if cpu < MAX_CPUS {
            NEED_RESCHED[cpu].store(true, Ordering::Relaxed);
        }
    }
    crate::smp::send_resched_ipi_all();
}

#[inline(always)]
pub(super) fn with_preempt_disabled<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    preempt_disable();
    let r = interrupts::without_interrupts(f);
    preempt_enable();
    r
}

pub fn handle_timer(tf: &mut arch::TrapFrame) {
    check_interrupt_trapframe(tf);
    if !preempt_enabled() {
        return;
    }
    if preempt_count() != 0 {
        return;
    }
    set_need_resched(current_cpu_id());
    if INTERRUPT_PREEMPT_ENABLED.load(Ordering::Relaxed) {
        preempt_from_interrupt(tf);
    }
}

pub fn handle_resched_ipi(tf: &mut arch::TrapFrame) {
    check_interrupt_trapframe(tf);
    if !preempt_enabled() {
        return;
    }
    if preempt_count() != 0 {
        return;
    }
    set_need_resched(current_cpu_id());
    if INTERRUPT_PREEMPT_ENABLED.load(Ordering::Relaxed) {
        preempt_from_interrupt(tf);
    }
}

pub fn check_interrupt_trapframe(tf: &arch::TrapFrame) {
    if (tf.cs & 3) != 3 {
        return;
    }
    let Some(proc) = processor().try_lock() else {
        return;
    };
    let Some(task) = proc.current() else {
        return;
    };
    let expected = task.trap_frame() as *const _ as usize;
    let actual = tf as *const _ as usize;
    if expected != actual && !WARN_IRQ_TF_MISMATCH.swap(true, Ordering::SeqCst) {
        logger::warn!(
            "interrupt tf not on task kernel stack: expected={:#x} actual={:#x}",
            expected,
            actual
        );
    }
}

pub fn check_preempt_from_syscall(tf: &arch::TrapFrame) {
    if !preempt_enabled() {
        return;
    }
    if preempt_count() != 0 {
        return;
    }
    if (tf.cs & 3) != 3 {
        return;
    }
    let cpu = current_cpu_id();
    if take_need_resched(cpu) {
        suspend_current_and_run_next();
    }
}

// NOTE: interrupt-time preemption enabled; use try_lock and fallback on contention.
fn preempt_from_interrupt(tf: &mut arch::TrapFrame) {
    if (tf.cs & 3) != 3 {
        return;
    }
    let cpu = current_cpu_id();
    if !take_need_resched(cpu) {
        return;
    }
    let Some(proc) = processor().try_lock() else {
        set_need_resched(cpu);
        return;
    };
    let Some(current) = proc.current() else {
        set_need_resched(cpu);
        return;
    };
    drop(proc);

    let next = match fetch_next_task_try() {
        Some(t) => t,
        None => {
            set_need_resched(cpu);
            return;
        }
    };
    if Arc::ptr_eq(&current, &next) {
        return;
    }

    enqueue_ready(current.clone());

    with_preempt_disabled(|| {
        set_current_task(next.clone());
    });

    arch_task_switch(&current, &next, tf);
}

pub fn init() {
    arch::interrupts::set_timer_handler(handle_timer);
}
