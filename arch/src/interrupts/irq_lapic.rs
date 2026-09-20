use crate::syscall::TrapFrame;

use super::{LAPIC_RESCHED_HANDLER_HOOK, LAPIC_TIMER_HANDLER_HOOK, LAPIC_TLB_HANDLER_HOOK};

pub const LAPIC_TIMER_VECTOR: u8 = 48;
pub const LAPIC_RESCHED_VECTOR: u8 = 49;
pub const LAPIC_TLB_VECTOR: u8 = 50;

#[no_mangle]
extern "C" fn lapic_timer_interrupt_handler_inner(trap_frame: &mut TrapFrame) {
    crate::apic::eoi();
    if let Some(handler) = LAPIC_TIMER_HANDLER_HOOK.get() {
        handler(trap_frame);
    }
}

#[no_mangle]
extern "C" fn lapic_resched_interrupt_handler_inner(trap_frame: &mut TrapFrame) {
    crate::apic::eoi();
    if let Some(handler) = LAPIC_RESCHED_HANDLER_HOOK.get() {
        handler(trap_frame);
    }
}

#[no_mangle]
extern "C" fn lapic_tlb_interrupt_handler_inner(trap_frame: &mut TrapFrame) {
    crate::apic::eoi();
    if let Some(handler) = LAPIC_TLB_HANDLER_HOOK.get() {
        handler(trap_frame);
    }
}
