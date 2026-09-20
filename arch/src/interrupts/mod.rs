use core::arch::asm;

use spin::Once;
use x86_64::structures::idt::{InterruptStackFrame, PageFaultErrorCode};
use x86_64::VirtAddr;

use crate::syscall::TrapFrame;

mod exceptions;
mod idt;
mod irq_lapic;
mod irq_pic;

pub use idt::{init_idt, load_idt};
pub use irq_lapic::{LAPIC_RESCHED_VECTOR, LAPIC_TIMER_VECTOR, LAPIC_TLB_VECTOR};
pub use irq_pic::{mask_all_pic, set_ioapic_enabled, InterruptIndex, PIC_1_OFFSET, PIC_2_OFFSET};

use core::arch::global_asm;
global_asm!(include_str!("vectors.S"));

pub type PageFaultHandler = fn(VirtAddr, PageFaultErrorCode) -> Result<(), ()>;
pub type PageFaultKillHandler = fn(VirtAddr, PageFaultErrorCode) -> !;
pub type PageFaultReportHandler = fn(&InterruptStackFrame, VirtAddr, PageFaultErrorCode);
pub type TimerHandler = fn(&mut TrapFrame);
pub type LapicTimerHandler = fn(&mut TrapFrame);
pub type LapicReschedHandler = fn(&mut TrapFrame);
pub type LapicTlbHandler = fn(&mut TrapFrame);
pub type KeyboardHandler = fn(u8);

pub(super) static PAGE_FAULT_HANDLER_HOOK: Once<PageFaultHandler> = Once::new();
pub(super) static PAGE_FAULT_KILL_HOOK: Once<PageFaultKillHandler> = Once::new();
pub(super) static PAGE_FAULT_REPORT_HOOK: Once<PageFaultReportHandler> = Once::new();
pub(super) static TIMER_HANDLER_HOOK: Once<TimerHandler> = Once::new();
pub(super) static LAPIC_TIMER_HANDLER_HOOK: Once<LapicTimerHandler> = Once::new();
pub(super) static LAPIC_RESCHED_HANDLER_HOOK: Once<LapicReschedHandler> = Once::new();
pub(super) static LAPIC_TLB_HANDLER_HOOK: Once<LapicTlbHandler> = Once::new();
pub(super) static KEYBOARD_HANDLER_HOOK: Once<KeyboardHandler> = Once::new();

pub fn set_page_fault_handler(handler: PageFaultHandler) {
    let _ = PAGE_FAULT_HANDLER_HOOK.call_once(|| handler);
}

pub fn set_page_fault_kill_handler(handler: PageFaultKillHandler) {
    let _ = PAGE_FAULT_KILL_HOOK.call_once(|| handler);
}

pub fn set_page_fault_report_handler(handler: PageFaultReportHandler) {
    let _ = PAGE_FAULT_REPORT_HOOK.call_once(|| handler);
}

pub fn set_timer_handler(handler: TimerHandler) {
    let _ = TIMER_HANDLER_HOOK.call_once(|| handler);
}

pub fn set_lapic_timer_handler(handler: LapicTimerHandler) {
    let _ = LAPIC_TIMER_HANDLER_HOOK.call_once(|| handler);
}

pub fn set_lapic_resched_handler(handler: LapicReschedHandler) {
    let _ = LAPIC_RESCHED_HANDLER_HOOK.call_once(|| handler);
}

pub fn set_lapic_tlb_handler(handler: LapicTlbHandler) {
    let _ = LAPIC_TLB_HANDLER_HOOK.call_once(|| handler);
}

pub fn set_keyboard_handler(handler: KeyboardHandler) {
    let _ = KEYBOARD_HANDLER_HOOK.call_once(|| handler);
}

pub(super) struct SwapGsGuard(bool);

impl SwapGsGuard {
    #[inline(always)]
    fn new(user_mode: bool) -> Self {
        if user_mode {
            unsafe {
                asm!("swapgs");
            }
        }
        SwapGsGuard(user_mode)
    }
}

impl Drop for SwapGsGuard {
    #[inline(always)]
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                asm!("swapgs");
            }
        }
    }
}
