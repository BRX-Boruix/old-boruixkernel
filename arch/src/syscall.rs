use crate::gdt;
use core::arch::{asm, global_asm};
use core::mem::offset_of;
use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;

const USER_DATA_SEL: u64 = ((gdt::USER_DATA_SELECTOR_INDEX as u64) << 3) | 3;
const USER_CODE_SEL: u64 = ((gdt::USER_CODE_SELECTOR_INDEX as u64) << 3) | 3;

global_asm!(r#"
.global syscall_entry
syscall_entry:
    # 1. Swap GS (we are in kernel now, GS points to kernel data)
    swapgs

    # 2. Save user stack
    mov gs:[0x0], rsp

    # 3. Load kernel stack
    mov rsp, gs:[0x8]

    # 4. Save user context (TrapFrame)
    push {user_data_sel}         # SS (User Data Selector | 3)
    push gs:[0x0]     # RSP (User Stack)
    push r11          # RFLAGS (Saved by syscall)
    push {user_code_sel}         # CS (User Code Selector | 3)
    push rcx          # RIP (Saved by syscall)

    # Push general registers (reverse order of TrapFrame layout)
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

    # 5. Move TrapFrame to task kernel stack if needed
    # First argument: &mut TrapFrame (stack pointer)
    mov rdi, rsp
    call syscall_stack_switch
    mov rsp, rax

    # 6. Call Rust handler
    mov rdi, rsp
    
    # We need to make sure stack is aligned
    # push rax aligns stack to 16 bytes?
    # 5 pushes (SS, RSP, RFLAGS, CS, RIP) = 40 bytes.
    # 15 pushes = 120 bytes.
    # Total 160 bytes. 160 % 16 == 0. So stack is aligned.
    
    call syscall_dispatch

    # Disable interrupts to prevent interruption during stack switch
    cli

    # 7. Restore context
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
    pop rax  # Return value from syscall

    # 8. Restore stack for sysret
    # Stack layout: [RIP, CS, RFLAGS, RSP, SS]
    
    # We need RCX = RIP, R11 = RFLAGS
    
    pop rcx      # RIP
    add rsp, 8   # Skip CS (8 bytes)
    pop r11      # RFLAGS
    pop rsp      # User RSP (This switches stack back to user stack!)
    
    # Skip SS (8 bytes) - Wait, we popped RSP, so SS is still on kernel stack?
    # No, we popped RSP from kernel stack into RSP register.
    # So we are now on user stack.
    # Wait, the SS is still on kernel stack, but we don't care about it.
    # We just abandoned the kernel stack frame.
    
    # 9. Swap GS back
    swapgs
    
    # 10. Return to user mode
    sysretq
"#, user_data_sel = const USER_DATA_SEL, user_code_sel = const USER_CODE_SEL);

/// CPU register snapshot used by syscall and interrupt entry.
/// Layout must match the push/pop order in assembly (see `interrupts/vectors.S`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TrapFrame {
    pub r15: usize,
    pub r14: usize,
    pub r13: usize,
    pub r12: usize,
    pub rbp: usize,
    pub rbx: usize,
    pub r11: usize,
    pub r10: usize,
    pub r9: usize,
    pub r8: usize,
    pub rsi: usize,
    pub rdi: usize,
    pub rdx: usize,
    pub rcx: usize,
    pub rax: usize,
    pub rip: usize,
    pub cs: usize,
    pub rflags: usize,
    pub rsp: usize,
    pub ss: usize,
}

extern "C" {
    #[allow(dead_code)]
    fn syscall_stack_switch(tf: *mut TrapFrame) -> *mut TrapFrame;
}

// TrapFrame layout sanity checks (must match assembly push/pop order)
const _: () = {
    assert!(offset_of!(TrapFrame, r15) == 0x00);
    assert!(offset_of!(TrapFrame, r14) == 0x08);
    assert!(offset_of!(TrapFrame, r13) == 0x10);
    assert!(offset_of!(TrapFrame, r12) == 0x18);
    assert!(offset_of!(TrapFrame, rbp) == 0x20);
    assert!(offset_of!(TrapFrame, rbx) == 0x28);
    assert!(offset_of!(TrapFrame, r11) == 0x30);
    assert!(offset_of!(TrapFrame, r10) == 0x38);
    assert!(offset_of!(TrapFrame, r9) == 0x40);
    assert!(offset_of!(TrapFrame, r8) == 0x48);
    assert!(offset_of!(TrapFrame, rsi) == 0x50);
    assert!(offset_of!(TrapFrame, rdi) == 0x58);
    assert!(offset_of!(TrapFrame, rdx) == 0x60);
    assert!(offset_of!(TrapFrame, rcx) == 0x68);
    assert!(offset_of!(TrapFrame, rax) == 0x70);
    assert!(offset_of!(TrapFrame, rip) == 0x78);
    assert!(offset_of!(TrapFrame, cs) == 0x80);
    assert!(offset_of!(TrapFrame, rflags) == 0x88);
    assert!(offset_of!(TrapFrame, rsp) == 0x90);
    assert!(offset_of!(TrapFrame, ss) == 0x98);
    assert!(core::mem::size_of::<TrapFrame>() == 0xa0);
};

fn init_common() {
    // 1. Enable syscall/sysret instruction
    unsafe {
        Efer::update(|flags| {
            flags.insert(EferFlags::SYSTEM_CALL_EXTENSIONS);
        });
    }

    // 2. Setup STAR MSR
    // Kernel CS: 0x8 (Code selector)
    // Kernel SS: 0x10 (Data selector)
    // User CS: 0x20 | 3 (Code selector)
    // User SS: 0x18 | 3 (Data selector)

    // Star::write(cs_sysret, cs_syscall)
    // cs_sysret: 0x10 (Kernel Data) -> User CS = 0x20, User SS = 0x18
    // cs_syscall: 0x8 (Kernel Code) -> Kernel CS = 0x8, Kernel SS = 0x10

    use x86_64::structures::gdt::SegmentSelector;
    use x86_64::PrivilegeLevel;

    let kernel_code = SegmentSelector::new(gdt::KERNEL_CODE_SELECTOR_INDEX, PrivilegeLevel::Ring0);
    let kernel_data = SegmentSelector::new(gdt::KERNEL_DATA_SELECTOR_INDEX, PrivilegeLevel::Ring0);
    let user_data = SegmentSelector::new(gdt::USER_DATA_SELECTOR_INDEX, PrivilegeLevel::Ring3);
    let user_code = SegmentSelector::new(gdt::USER_CODE_SELECTOR_INDEX, PrivilegeLevel::Ring3);

    if Star::write(user_code, user_data, kernel_code, kernel_data).is_err() {
        panic!("Failed to write STAR MSR");
    }

    // 3. Setup LSTAR MSR (Entry point)
    extern "C" {
        fn syscall_entry();
    }
    LStar::write(x86_64::VirtAddr::new(syscall_entry as *const () as u64));

    // 4. Setup SFMASK MSR (Flags to clear)
    // Clear IF (Interrupt Flag) to disable interrupts on entry
    SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::TRAP_FLAG);
}

pub fn init() {
    init_common();
    // Setup GS bases for CPU0
    init_cpu(0);
}

pub fn init_ap(cpu_id: usize) {
    init_common();
    init_cpu(cpu_id);
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct CpuData {
    pub user_rsp: u64,              // Offset 0x0
    pub kernel_rsp: u64,            // Offset 0x8
    pub warn_syscall_tf_moved: u64, // Offset 0x10
    pub cpu_id: u64,                // Offset 0x18
    pub preempt_count: u64,         // Offset 0x20
}

const _: () = {
    assert!(offset_of!(CpuData, user_rsp) == 0x0);
    assert!(offset_of!(CpuData, kernel_rsp) == 0x8);
    assert!(offset_of!(CpuData, warn_syscall_tf_moved) == 0x10);
    assert!(offset_of!(CpuData, cpu_id) == 0x18);
    assert!(offset_of!(CpuData, preempt_count) == 0x20);
    assert!(core::mem::size_of::<CpuData>() == 0x28);
};

const MAX_CPUS: usize = 64;
const SYSCALL_STACK_SIZE: usize = 4096 * 4;

#[repr(align(16))]
#[derive(Copy, Clone)]
struct SyscallStack([u8; SYSCALL_STACK_SIZE]);

static mut SYSCALL_STACKS: [SyscallStack; MAX_CPUS] =
    [SyscallStack([0; SYSCALL_STACK_SIZE]); MAX_CPUS];

static mut CPU_DATA: [CpuData; MAX_CPUS] = [CpuData {
    user_rsp: 0,
    kernel_rsp: 0,
    warn_syscall_tf_moved: 0,
    cpu_id: 0,
    preempt_count: 0,
}; MAX_CPUS];

pub fn set_kernel_stack(stack_top: usize) {
    // Update TSS RSP0 for interrupts
    crate::gdt::set_kernel_stack(x86_64::VirtAddr::new(stack_top as u64));
}

pub fn init_cpu(cpu_id: usize) {
    use x86_64::registers::model_specific::{GsBase, KernelGsBase};
    let cpu = cpu_id % MAX_CPUS;
    unsafe {
        let cpu_data_ptr = core::ptr::addr_of_mut!(CPU_DATA).cast::<CpuData>().add(cpu);
        let stack_ptr = SYSCALL_STACKS[cpu].0.as_mut_ptr();
        (*cpu_data_ptr).kernel_rsp = stack_ptr as u64 + SYSCALL_STACK_SIZE as u64;
        (*cpu_data_ptr).warn_syscall_tf_moved = 0;
        (*cpu_data_ptr).cpu_id = cpu as u64;
        (*cpu_data_ptr).preempt_count = 0;

        // Kernel uses GS for CpuData
        GsBase::write(x86_64::VirtAddr::new(cpu_data_ptr as u64));
        // User GS base is 0
        KernelGsBase::write(x86_64::VirtAddr::new(0));
    }
}

pub fn warn_syscall_tf_moved_once() -> bool {
    use x86_64::registers::model_specific::GsBase;
    let base = GsBase::read().as_u64() as *mut CpuData;
    unsafe {
        let data = &mut *base;
        if data.warn_syscall_tf_moved == 0 {
            data.warn_syscall_tf_moved = 1;
            true
        } else {
            false
        }
    }
}

pub fn current_cpu_id_raw() -> usize {
    const CPU_ID_OFFSET: u64 = offset_of!(CpuData, cpu_id) as u64;
    let id: u64;
    unsafe {
        asm!("mov {}, gs:[{off}]", out(reg) id, off = const CPU_ID_OFFSET);
    }
    id as usize
}

pub fn preempt_count() -> usize {
    const OFF: u64 = offset_of!(CpuData, preempt_count) as u64;
    let v: u64;
    unsafe {
        asm!("mov {}, gs:[{off}]", out(reg) v, off = const OFF);
    }
    v as usize
}

pub fn preempt_disable() {
    const OFF: u64 = offset_of!(CpuData, preempt_count) as u64;
    unsafe {
        asm!("inc qword ptr gs:[{off}]", off = const OFF);
    }
}

pub fn preempt_enable() {
    const OFF: u64 = offset_of!(CpuData, preempt_count) as u64;
    unsafe {
        asm!("dec qword ptr gs:[{off}]", off = const OFF);
    }
}
