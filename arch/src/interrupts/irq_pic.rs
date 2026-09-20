use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::structures::idt::InterruptStackFrame;

use crate::syscall::TrapFrame;

use super::{SwapGsGuard, KEYBOARD_HANDLER_HOOK, TIMER_HANDLER_HOOK};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });
static KBD_IRQ_COUNT: AtomicUsize = AtomicUsize::new(0);
static KBD_IRQ_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
const KBD_IRQ_LOG_LIMIT: usize = 16;
static IOAPIC_ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
}

impl InterruptIndex {
    pub(super) fn as_u8(self) -> u8 {
        self as u8
    }

    pub(super) fn as_usize(self) -> usize {
        self as usize
    }
}

pub fn set_ioapic_enabled(enabled: bool) {
    IOAPIC_ENABLED.store(enabled, Ordering::Release);
}

pub fn mask_all_pic() {
    unsafe {
        let mut pics = PICS.lock();
        pics.write_masks(0xFF, 0xFF);
    }
}

pub(super) fn init_pic() {
    unsafe {
        let mut pics = PICS.lock();
        pics.initialize();
        // Unmask Timer (IRQ0) and Keyboard (IRQ1) on Master PIC.
        // Slave PIC is fully masked (0xFF).
        pics.write_masks(0xFC, 0xFF);
    }
}

#[no_mangle]
extern "C" fn timer_interrupt_handler_inner(trap_frame: &mut TrapFrame) {
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
    if let Some(handler) = TIMER_HANDLER_HOOK.get() {
        handler(trap_frame);
    }
}

pub(super) extern "x86-interrupt" fn keyboard_interrupt_handler(stack_frame: InterruptStackFrame) {
    let user_mode = (stack_frame.code_segment & 3) == 3;
    let _swapgs = SwapGsGuard::new(user_mode);
    use x86_64::instructions::port::Port;
    let mut port = Port::<u8>::new(0x60);
    let scancode = unsafe { port.read() };
    let cnt = KBD_IRQ_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let log_idx = KBD_IRQ_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if log_idx < KBD_IRQ_LOG_LIMIT {
        logger::println!(
            "[KBD][IRQ] #{} cnt={} mode={} scancode={:#x}",
            log_idx + 1,
            cnt,
            if user_mode { "user" } else { "kernel" },
            scancode
        );
    }
    if let Some(handler) = KEYBOARD_HANDLER_HOOK.get() {
        handler(scancode);
    }

    if IOAPIC_ENABLED.load(Ordering::Relaxed) {
        crate::apic::eoi();
    } else {
        unsafe {
            PICS.lock()
                .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
        }
    }
}
