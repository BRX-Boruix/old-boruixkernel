use x86_64::structures::idt::{InterruptStackFrame, PageFaultErrorCode};
use x86_64::VirtAddr;

use kernel_platform::hal::arch;
use kernel_platform::memory as mem;

use super::current_task;
use super::scheduler::exit_current_and_run_next;

fn handle_page_fault(addr: VirtAddr, error_code: PageFaultErrorCode) -> Result<(), ()> {
    if let Some(task) = current_task() {
        let mm_addr = mem::addr_space::VirtAddr::new(addr.as_u64());
        unsafe {
            if let Ok(_) = (*task.memory_set.get()).handle_page_fault(mm_addr, error_code) {
                return Ok(());
            }
        }
    }
    Err(())
}

fn handle_page_fault_kill(addr: VirtAddr, error_code: PageFaultErrorCode) -> ! {
    logger::error!("User page fault at {:?}, error {:?}", addr, error_code);
    // Terminate the current task and switch to the next runnable task.
    exit_current_and_run_next(-1);
    loop {
        x86_64::instructions::hlt();
    }
}

fn page_fault_report(
    stack_frame: &InterruptStackFrame,
    addr: VirtAddr,
    error_code: PageFaultErrorCode,
) {
    use logger::println;
    use mem::addr_space::VirtAddr as MmVirtAddr;
    use x86_64::registers::control::Cr3;

    println!("---- Page Fault Details ----");
    println!(
        "PF Error Code: {:?} (bits: {:#x})",
        error_code,
        error_code.bits()
    );

    let task = current_task();
    if let Some(task) = task.as_ref() {
        let status = *task.task_status.lock();
        let kstack_lo = task.kernel_stack_bottom;
        let kstack_hi = task.kernel_stack;
        let (cr3_frame, _) = Cr3::read();
        let cr3 = cr3_frame.start_address().as_u64();
        let mem = unsafe { &*task.memory_set.get() };
        println!(
            "Task: PID={} Status={:?} Priority={} Stride={}",
            task.pid, status, task.priority, task.stride
        );
        println!("Kernel Stack: [{:#x}, {:#x})", kstack_lo, kstack_hi);
        println!("MemorySet P4: {:#x}  CR3: {:#x}", mem.token(), cr3);
    } else {
        println!("Task: <none>");
    }

    // VMA lookup
    if let Some(task) = task.as_ref() {
        let mm_addr = MmVirtAddr::new(addr.as_u64());
        let memory_set = unsafe { &*task.memory_set.get() };
        let (hit, prev, next) = memory_set.debug_vma_lookup(mm_addr);
        match hit {
            Some(a) => println!(
                "VMA: HIT [{:#x}, {:#x}) flags={:?}",
                a.start.as_u64(),
                a.end.as_u64(),
                a.flags
            ),
            None => println!("VMA: MISS"),
        }
        if let Some(p) = prev {
            println!(
                "VMA: PREV [{:#x}, {:#x}) flags={:?}",
                p.start.as_u64(),
                p.end.as_u64(),
                p.flags
            );
        }
        if let Some(n) = next {
            println!(
                "VMA: NEXT [{:#x}, {:#x}) flags={:?}",
                n.start.as_u64(),
                n.end.as_u64(),
                n.flags
            );
        }
    }

    // Page table walk and RIP bytes
    let phys_offset = match mem::addr_space::PHYS_OFFSET.get() {
        Some(v) => *v,
        None => {
            println!("PHYS_OFFSET not initialized");
            return;
        }
    };

    let (cr3_frame, _) = Cr3::read();
    let cr3 = cr3_frame.start_address().as_u64();
    let mm_addr = MmVirtAddr::new(addr.as_u64());
    dump_page_walk("FaultAddr", cr3, mm_addr, phys_offset);

    let rip = MmVirtAddr::new(stack_frame.instruction_pointer.as_u64());
    dump_page_walk("RIP", cr3, rip, phys_offset);
    dump_rip_bytes(rip, cr3, phys_offset);

    println!("---- End Page Fault Details ----");
}

fn dump_page_walk(tag: &str, cr3_phys: u64, addr: mem::addr_space::VirtAddr, phys_offset: u64) {
    use logger::println;
    use mem::addr_space::{PageTableFlags, PhysAddr};
    use mem::mapper::PageTable;

    let p4_virt = mem::addr_space::VirtAddr::new(cr3_phys + phys_offset);
    let p4 = unsafe { &*p4_virt.as_ptr::<PageTable>() };

    let p4_idx = addr.p4_index();
    let p3_idx = addr.p3_index();
    let p2_idx = addr.p2_index();
    let p1_idx = addr.p1_index();

    let p4e = p4[p4_idx];
    println!("{} Walk: P4[{}] = {:?}", tag, p4_idx, p4e);
    if !p4e.flags().contains(PageTableFlags::PRESENT) {
        return;
    }

    let p3 = unsafe {
        &*mem::addr_space::VirtAddr::new(p4e.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
    };
    let p3e = p3[p3_idx];
    println!("{} Walk: P3[{}] = {:?}", tag, p3_idx, p3e);
    if !p3e.flags().contains(PageTableFlags::PRESENT) {
        return;
    }
    if p3e.flags().contains(PageTableFlags::HUGE_PAGE) {
        let offset = addr.as_u64() & 0x3fff_ffff;
        let phys = PhysAddr::new(p3e.addr().as_u64() + offset);
        println!("{} Walk: 1GiB page -> {:?}", tag, phys);
        return;
    }

    let p2 = unsafe {
        &*mem::addr_space::VirtAddr::new(p3e.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
    };
    let p2e = p2[p2_idx];
    println!("{} Walk: P2[{}] = {:?}", tag, p2_idx, p2e);
    if !p2e.flags().contains(PageTableFlags::PRESENT) {
        return;
    }
    if p2e.flags().contains(PageTableFlags::HUGE_PAGE) {
        let offset = addr.as_u64() & 0x1f_ffff;
        let phys = PhysAddr::new(p2e.addr().as_u64() + offset);
        println!("{} Walk: 2MiB page -> {:?}", tag, phys);
        return;
    }

    let p1 = unsafe {
        &*mem::addr_space::VirtAddr::new(p2e.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
    };
    let p1e = p1[p1_idx];
    println!("{} Walk: P1[{}] = {:?}", tag, p1_idx, p1e);
    if !p1e.flags().contains(PageTableFlags::PRESENT) {
        return;
    }
    let offset = addr.as_u64() & 0xfff;
    let phys = PhysAddr::new(p1e.addr().as_u64() + offset);
    println!("{} Walk: 4KiB page -> {:?}", tag, phys);
}

fn dump_rip_bytes(rip: mem::addr_space::VirtAddr, cr3_phys: u64, phys_offset: u64) {
    use logger::println;
    use mem::addr_space::{PageTableFlags, PhysAddr};
    use mem::mapper::PageTable;

    let p4_virt = mem::addr_space::VirtAddr::new(cr3_phys + phys_offset);
    let p4 = unsafe { &*p4_virt.as_ptr::<PageTable>() };

    let p4e = p4[rip.p4_index()];
    if !p4e.flags().contains(PageTableFlags::PRESENT) {
        println!("RIP Bytes: <unmapped P4>");
        return;
    }
    let p3 = unsafe {
        &*mem::addr_space::VirtAddr::new(p4e.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
    };
    let p3e = p3[rip.p3_index()];
    if !p3e.flags().contains(PageTableFlags::PRESENT) {
        println!("RIP Bytes: <unmapped P3>");
        return;
    }
    if p3e.flags().contains(PageTableFlags::HUGE_PAGE) {
        let offset = rip.as_u64() & 0x3fff_ffff;
        let phys = PhysAddr::new(p3e.addr().as_u64() + offset);
        print_phys_bytes(phys, phys_offset);
        return;
    }
    let p2 = unsafe {
        &*mem::addr_space::VirtAddr::new(p3e.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
    };
    let p2e = p2[rip.p2_index()];
    if !p2e.flags().contains(PageTableFlags::PRESENT) {
        println!("RIP Bytes: <unmapped P2>");
        return;
    }
    if p2e.flags().contains(PageTableFlags::HUGE_PAGE) {
        let offset = rip.as_u64() & 0x1f_ffff;
        let phys = PhysAddr::new(p2e.addr().as_u64() + offset);
        print_phys_bytes(phys, phys_offset);
        return;
    }
    let p1 = unsafe {
        &*mem::addr_space::VirtAddr::new(p2e.addr().as_u64() + phys_offset).as_ptr::<PageTable>()
    };
    let p1e = p1[rip.p1_index()];
    if !p1e.flags().contains(PageTableFlags::PRESENT) {
        println!("RIP Bytes: <unmapped P1>");
        return;
    }
    let offset = rip.as_u64() & 0xfff;
    let phys = PhysAddr::new(p1e.addr().as_u64() + offset);
    print_phys_bytes(phys, phys_offset);
}

fn print_phys_bytes(phys: mem::addr_space::PhysAddr, phys_offset: u64) {
    use logger::{print, println};
    let ptr = (phys.as_u64() + phys_offset) as *const u8;
    let mut bytes = [0u8; 16];
    unsafe {
        core::ptr::copy_nonoverlapping(ptr, bytes.as_mut_ptr(), bytes.len());
    }
    print!("RIP Bytes:");
    for b in &bytes {
        print!(" {:02x}", b);
    }
    println!();
}

pub fn init() {
    arch::interrupts::set_page_fault_handler(handle_page_fault);
    arch::interrupts::set_page_fault_kill_handler(handle_page_fault_kill);
    arch::interrupts::set_page_fault_report_handler(page_fault_report);
}
