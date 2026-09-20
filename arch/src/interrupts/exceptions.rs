use logger::println;
use x86_64::structures::idt::{InterruptStackFrame, PageFaultErrorCode};

use super::{SwapGsGuard, PAGE_FAULT_HANDLER_HOOK, PAGE_FAULT_KILL_HOOK, PAGE_FAULT_REPORT_HOOK};

pub(super) extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    let user_mode = (stack_frame.code_segment & 3) == 3;
    let _swapgs = SwapGsGuard::new(user_mode);
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

pub(super) extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    // Double fault is fatal, force unlock serial to ensure message is printed
    unsafe {
        logger::force_unlock();
    }
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

pub(super) extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let user_mode = (stack_frame.code_segment & 3) == 3;
    let _swapgs = SwapGsGuard::new(user_mode);
    use logger::{error, println};
    use x86_64::registers::control::Cr2;
    use x86_64::registers::control::Cr3;
    use x86_64::registers::control::{Cr0, Cr4};
    use x86_64::registers::model_specific::Efer;

    let addr = Cr2::read();

    // Try to handle the page fault using the registered hook
    if let Some(handler) = PAGE_FAULT_HANDLER_HOOK.get() {
        if handler(addr, error_code).is_ok() {
            return;
        }
    }

    if let Some(reporter) = PAGE_FAULT_REPORT_HOOK.get() {
        reporter(&stack_frame, addr, error_code);
    }
    if user_mode {
        if let Some(kill_handler) = PAGE_FAULT_KILL_HOOK.get() {
            kill_handler(addr, error_code);
        }
    }

    unsafe {
        logger::force_unlock();
    }

    error!("EXCEPTION: PAGE FAULT");
    let mode = if user_mode { "USER" } else { "KERNEL" };
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    let (cr3_frame, cr3_flags) = Cr3::read();
    let cr0 = Cr0::read();
    let cr4 = Cr4::read();
    let efer = Efer::read();
    let reason = if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        "protection violation"
    } else {
        "non-present page"
    };
    let access = if error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH) {
        "instruction fetch"
    } else if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
        "write"
    } else {
        "read"
    };
    println!("Mode: {}", mode);
    println!("RIP: {:#x}  RSP: {:#x}", rip, rsp);
    println!("CR2 (fault addr): {:?}", addr);
    println!(
        "CR3 (pml4): {:#x}  flags: {:?}",
        cr3_frame.start_address().as_u64(),
        cr3_flags
    );
    println!("Access: {}  Reason: {}", access, reason);
    println!(
        "CR0: {:#x}  CR4: {:#x}  EFER: {:#x}",
        cr0.bits(),
        cr4.bits(),
        efer.bits()
    );
    println!(
        "Error Code: {:?} (bits: {:#x})",
        error_code,
        error_code.bits()
    );
    println!(
        "EC Decode: P={} W/R={} U/S={} RSVD={} I/D={}",
        if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
            1
        } else {
            0
        },
        if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
            1
        } else {
            0
        },
        if error_code.contains(PageFaultErrorCode::USER_MODE) {
            1
        } else {
            0
        },
        if error_code.contains(PageFaultErrorCode::MALFORMED_TABLE) {
            1
        } else {
            0
        },
        if error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH) {
            1
        } else {
            0
        },
    );
    const KERNEL_BASE: u64 = 0xffffffff80200000;
    if rip >= KERNEL_BASE {
        println!("RIP (KernelBase + {:#x})", rip - KERNEL_BASE);
    }
    println!("{:#?}", stack_frame);

    if user_mode {
        panic!(
            "Unhandled USER page fault: addr={:?} error={:?}",
            addr, error_code
        );
    }

    panic!(
        "Unhandled KERNEL page fault: addr={:?} error={:?}\n{:#?}",
        addr, error_code, stack_frame
    );
}

pub(super) extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let user_mode = (stack_frame.code_segment & 3) == 3;
    let _swapgs = SwapGsGuard::new(user_mode);
    unsafe {
        logger::force_unlock();
    }

    // 1. Decode Context
    let cs = stack_frame.code_segment;
    let mode = if cs & 3 == 0 { "KERNEL" } else { "USER" };
    let rip = stack_frame.instruction_pointer.as_u64();

    // 2. Decode RFLAGS
    use x86_64::registers::rflags::RFlags;
    let flags = RFlags::from_bits_truncate(stack_frame.cpu_flags);

    // 3. Decode Error Code
    let ext = (error_code & 1) != 0;
    let idt = (error_code & 2) != 0;
    let ti = (error_code & 4) != 0;
    let index = (error_code >> 3) & 0x1fff;

    // 4. Get Current Task Info
    use x86_64::registers::control::Cr3;
    let (cr3_frame, _) = Cr3::read();
    let cr3 = cr3_frame.start_address().as_u64();

    // 5. Symbol Hint (Offset from Kernel Base)
    const KERNEL_BASE: u64 = 0xffffffff80200000;
    let rel_rip = if rip >= KERNEL_BASE {
        Some(rip - KERNEL_BASE)
    } else {
        None
    };

    println!("\n\x1b[41;37m[EXCEPTION] GENERAL PROTECTION FAULT\x1b[0m");
    println!("----------------------------------------------------------------");
    println!("Context:  {} Mode", mode);
    if let Some(off) = rel_rip {
        println!("RIP:      {:#018x} (KernelBase + {:#x})", rip, off);
    } else {
        println!("RIP:      {:#018x}", rip);
    }
    println!(
        "CS:       {:#06x} (Attributes: {:?})",
        cs,
        if cs & 3 == 0 { "Ring 0" } else { "Ring 3" }
    );
    println!("RFLAGS:   {:#018x}", flags.bits());
    println!(
        "          [ IF:{} | TF:{} | DF:{} | AC:{} ]",
        if flags.contains(RFlags::INTERRUPT_FLAG) {
            1
        } else {
            0
        },
        if flags.contains(RFlags::TRAP_FLAG) {
            1
        } else {
            0
        },
        if flags.contains(RFlags::DIRECTION_FLAG) {
            1
        } else {
            0
        },
        if flags.contains(RFlags::ALIGNMENT_CHECK) {
            1
        } else {
            0
        }
    );
    println!(
        "Stack:    RSP={:#018x} SS={:#06x}",
        stack_frame.stack_pointer.as_u64(),
        stack_frame.stack_segment
    );
    println!("CR3:      {:#018x} (Page Table Phys Addr)", cr3);

    println!("----------------------------------------------------------------");
    println!("Error Code Breakdown (0x{:x}):", error_code);
    println!(
        "  - External (EXT):   {}",
        if ext {
            "Yes (Hardware/External)"
        } else {
            "No (Software/Internal)"
        }
    );
    println!(
        "  - IDT Flag (IDT):   {}",
        if idt {
            "Yes (Gate Descriptor in IDT)"
        } else {
            "No (Segment Descriptor)"
        }
    );
    if !idt {
        println!("  - Table (TI):       {}", if ti { "LDT" } else { "GDT" });
        println!("  - Selector Index:   {} (0x{:x})", index, index);
    }
    println!("----------------------------------------------------------------");
    println!("Raw Interrupt Stack Frame:");
    println!("{:#?}", stack_frame);

    loop {
        x86_64::instructions::hlt();
    }
}

pub(super) extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    let user_mode = (stack_frame.code_segment & 3) == 3;
    let _swapgs = SwapGsGuard::new(user_mode);
    unsafe {
        logger::force_unlock();
    }
    println!("EXCEPTION: DIVIDE ERROR\n{:#?}", stack_frame);
    loop {
        x86_64::instructions::hlt();
    }
}

pub(super) extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    let user_mode = (stack_frame.code_segment & 3) == 3;
    let _swapgs = SwapGsGuard::new(user_mode);
    unsafe {
        logger::force_unlock();
    }
    println!("EXCEPTION: INVALID OPCODE\n{:#?}", stack_frame);
    loop {
        x86_64::instructions::hlt();
    }
}

pub(super) extern "x86-interrupt" fn stack_segment_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let user_mode = (stack_frame.code_segment & 3) == 3;
    let _swapgs = SwapGsGuard::new(user_mode);
    unsafe {
        logger::force_unlock();
    }
    println!("EXCEPTION: STACK SEGMENT FAULT");
    println!("Error Code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    loop {
        x86_64::instructions::hlt();
    }
}

pub(super) extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let user_mode = (stack_frame.code_segment & 3) == 3;
    let _swapgs = SwapGsGuard::new(user_mode);
    unsafe {
        logger::force_unlock();
    }
    println!("EXCEPTION: SEGMENT NOT PRESENT");
    println!("Error Code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    loop {
        x86_64::instructions::hlt();
    }
}
