use spin::Lazy;
use x86_64::structures::idt::InterruptDescriptorTable;
use x86_64::VirtAddr;

use crate::gdt;

use super::exceptions::{
    breakpoint_handler, divide_error_handler, double_fault_handler,
    general_protection_fault_handler, invalid_opcode_handler, page_fault_handler,
    segment_not_present_handler, stack_segment_fault_handler,
};
use super::irq_lapic::{LAPIC_RESCHED_VECTOR, LAPIC_TIMER_VECTOR, LAPIC_TLB_VECTOR};
use super::irq_pic::{init_pic, keyboard_interrupt_handler, InterruptIndex};

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX + 1);
    }
    idt.page_fault.set_handler_fn(page_fault_handler);
    idt.general_protection_fault
        .set_handler_fn(general_protection_fault_handler);
    idt.divide_error.set_handler_fn(divide_error_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt.stack_segment_fault
        .set_handler_fn(stack_segment_fault_handler);
    idt.segment_not_present
        .set_handler_fn(segment_not_present_handler);
    idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);

    unsafe {
        idt[InterruptIndex::Timer.as_usize()].set_handler_addr(VirtAddr::new(
            __timer_interrupt_handler as *const () as usize as u64,
        ));
        idt[LAPIC_TIMER_VECTOR as usize].set_handler_addr(VirtAddr::new(
            __lapic_timer_interrupt_handler as *const () as usize as u64,
        ));
        idt[LAPIC_RESCHED_VECTOR as usize].set_handler_addr(VirtAddr::new(
            __lapic_resched_interrupt_handler as *const () as usize as u64,
        ));
        idt[LAPIC_TLB_VECTOR as usize].set_handler_addr(VirtAddr::new(
            __lapic_tlb_interrupt_handler as *const () as usize as u64,
        ));
    }

    idt
});

pub fn init_idt() {
    load_idt();
    init_pic();
    x86_64::instructions::interrupts::enable();
}

pub fn load_idt() {
    IDT.load();
}

extern "C" {
    fn __timer_interrupt_handler();
    fn __lapic_timer_interrupt_handler();
    fn __lapic_resched_interrupt_handler();
    fn __lapic_tlb_interrupt_handler();
}
