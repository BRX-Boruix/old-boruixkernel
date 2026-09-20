.text

.global __timer_interrupt_handler
__timer_interrupt_handler:
    # 1. Check if we came from user mode (CS & 3 == 3)
    # CS is at offset 8 from RSP (RIP is at 0)
    test byte ptr [rsp + 8], 3
    jz 1f

    # Came from user mode -> swapgs
    swapgs

1:
    # 2. Save full register state to build TrapFrame
    # Stack layout after pushes will match TrapFrame (r15..rax then CPU frame)
    push rax
    push rcx
    push rdx
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    # Prepare argument for inner function (pointer to TrapFrame)
    mov rdi, rsp

    call timer_interrupt_handler_inner

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

    # 3. Swapgs back if needed
    test byte ptr [rsp + 8], 3
    jz 2f
    swapgs
2:
    iretq

.global __lapic_timer_interrupt_handler
__lapic_timer_interrupt_handler:
    # If came from user mode, swapgs
    test byte ptr [rsp + 8], 3
    jz 1f
    swapgs
1:
    # Save full register state to build TrapFrame
    push rax
    push rcx
    push rdx
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp
    call lapic_timer_interrupt_handler_inner

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
    # Swapgs back if needed
    test byte ptr [rsp + 8], 3
    jz 2f
    swapgs
2:
    iretq

.global __lapic_resched_interrupt_handler
__lapic_resched_interrupt_handler:
    # If came from user mode, swapgs
    test byte ptr [rsp + 8], 3
    jz 1f
    swapgs
1:
    # Save full register state to build TrapFrame
    push rax
    push rcx
    push rdx
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp
    call lapic_resched_interrupt_handler_inner

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
    # Swapgs back if needed
    test byte ptr [rsp + 8], 3
    jz 2f
    swapgs
2:
    iretq

.global __lapic_tlb_interrupt_handler
__lapic_tlb_interrupt_handler:
    # If came from user mode, swapgs
    test byte ptr [rsp + 8], 3
    jz 1f
    swapgs
1:
    # Save full register state to build TrapFrame
    push rax
    push rcx
    push rdx
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp
    call lapic_tlb_interrupt_handler_inner

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
    # Swapgs back if needed
    test byte ptr [rsp + 8], 3
    jz 2f
    swapgs
2:
    iretq
