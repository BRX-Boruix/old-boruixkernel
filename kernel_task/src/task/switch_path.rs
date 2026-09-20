use alloc::sync::Arc;
use core::arch::asm;

use super::switch;
use super::TaskControlBlock;
use kernel_platform::hal::arch;

pub(super) fn save_fpu_state(task: &TaskControlBlock) {
    unsafe {
        let area = task.fxsave_area.get() as *mut u8;
        asm!("fxsave64 [{}]", in(reg) area, options(nostack, preserves_flags));
    }
}

pub(super) fn restore_fpu_state(task: &TaskControlBlock) {
    unsafe {
        let area = task.fxsave_area.get() as *const u8;
        asm!("fxrstor64 [{}]", in(reg) area, options(nostack, preserves_flags));
    }
}

pub(super) fn arch_task_switch(
    current: &Arc<TaskControlBlock>,
    next: &Arc<TaskControlBlock>,
    tf: &mut arch::TrapFrame,
) -> ! {
    // Save current user context into its canonical TrapFrame.
    *current.trap_frame_mut() = *tf;
    // Update current task context to resume via __restore on next run.
    let ksp = current.trap_frame() as *const _ as usize;
    unsafe {
        let cx = &mut *current.task_cx.get();
        cx.rsp = ksp - core::mem::size_of::<usize>();
        cx.rip = switch::__restore as *const () as usize;
    }

    save_fpu_state(current);
    restore_fpu_state(next);

    // Switch address space and update per-CPU CR3 cache.
    unsafe {
        (*next.memory_set.get()).activate();
    }
    let p4 = unsafe { (*next.memory_set.get()).token() };
    crate::smp::set_current_cr3(p4);

    // Update TSS RSP0 to next task's kernel stack.
    arch::set_kernel_stack(next.kernel_stack);

    // GSBASE is per-CPU (CpuData), not per-task; nothing to sync.

    // Jump to iretq restore path with next task's TrapFrame.
    x86_64::instructions::interrupts::disable();
    let next_tf = next.trap_frame() as *const arch::TrapFrame;
    unsafe { switch::restore_to_trapframe(next_tf) }
}
