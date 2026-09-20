use kernel_platform::hal::arch;
use arch::TrapFrame;
use core::arch::{asm, global_asm};

global_asm!(
    r#"
    .equ OFF_RSP, 0
    .equ OFF_R15, 8
    .equ OFF_R14, 16
    .equ OFF_R13, 24
    .equ OFF_R12, 32
    .equ OFF_RBX, 40
    .equ OFF_RBP, 48
    .equ OFF_RIP, 56

    .global __switch
    __switch:
        # __switch(current_task_cx_ptr: *mut TaskContext, next_task_cx_ptr: *const TaskContext)
        # rdi: current_task_cx_ptr
        # rsi: next_task_cx_ptr

        # Save current context
        mov [rdi + OFF_RSP], rsp
        mov [rdi + OFF_R15], r15
        mov [rdi + OFF_R14], r14
        mov [rdi + OFF_R13], r13
        mov [rdi + OFF_R12], r12
        mov [rdi + OFF_RBX], rbx
        mov [rdi + OFF_RBP], rbp
        mov rax, [rsp]
        mov [rdi + OFF_RIP], rax

        # Restore next context
        mov rsp, [rsi + OFF_RSP]
        mov r15, [rsi + OFF_R15]
        mov r14, [rsi + OFF_R14]
        mov r13, [rsi + OFF_R13]
        mov r12, [rsi + OFF_R12]
        mov rbx, [rsi + OFF_RBX]
        mov rbp, [rsi + OFF_RBP]
        mov rax, [rsi + OFF_RIP]
        mov [rsp], rax

        ret


    .global __restore
    __restore:
        # Disable interrupts to ensure atomicity of context restore and swapgs
        cli
        
        # Expected stack layout (top to bottom):
        # [TrapFrame (r15...rax)]
        # [TrapFrame (rip, cs, rflags, rsp, ss)]
        
        pop r15
        pop r14
        pop r13
        pop r12
        pop rbp
        pop rbx
        pop r11
        pop r10
        pop r9
        pop r8
        pop rsi
        pop rdi
        pop rdx
        pop rcx
        pop rax
        
        # Stack: [RIP, CS, RFLAGS, RSP, SS]
        
        # Check if we are returning to user mode (CS & 3 == 3)
        # If so, swapgs to restore user GS base
        # Note: We can modify RFLAGS here because iretq will restore the saved RFLAGS
        test byte ptr [rsp + 8], 3
        jz 1f
        swapgs
    1:
        iretq
    "#
);

extern "C" {
    pub fn __switch(
        current_task_cx_ptr: *mut super::context::TaskContext,
        next_task_cx_ptr: *const super::context::TaskContext,
    );
    pub fn __restore();
}

#[inline(always)]
pub unsafe fn restore_to_trapframe(tf: *const TrapFrame) -> ! {
    asm!(
        "mov rsp, {0}",
        "jmp {1}",
        in(reg) tf,
        sym __restore,
        options(noreturn)
    );
}
