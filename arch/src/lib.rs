#![no_std]
#![feature(abi_x86_interrupt)]

pub mod apic;
pub mod gdt;
pub mod interrupts;
pub mod syscall;

pub fn init() {
    gdt::init();
    interrupts::init_idt();
    syscall::init();
}
