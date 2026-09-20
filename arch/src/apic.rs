#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::registers::model_specific::Msr;

const IA32_APIC_BASE_MSR: u32 = 0x1B;
const APIC_ENABLE: u64 = 1 << 11;

const LAPIC_ID: u64 = 0x20;
const LAPIC_EOI: u64 = 0xB0;
const LAPIC_SVR: u64 = 0xF0;
const LAPIC_ICR_LOW: u64 = 0x300;
const LAPIC_ICR_HIGH: u64 = 0x310;
const LAPIC_LVT_LINT0: u64 = 0x350;
const LAPIC_LVT_LINT1: u64 = 0x360;
const LAPIC_LVT_TIMER: u64 = 0x320;
const LAPIC_TIMER_INIT_CNT: u64 = 0x380;
const LAPIC_TIMER_DIV_CONF: u64 = 0x3E0;

static LAPIC_BASE_VA: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
fn lapic_base() -> u64 {
    LAPIC_BASE_VA.load(Ordering::Relaxed)
}

#[inline(always)]
fn lapic_write(offset: u64, val: u32) {
    let base = lapic_base();
    if base == 0 {
        return;
    }
    unsafe {
        core::ptr::write_volatile((base + offset) as *mut u32, val);
    }
}

#[inline(always)]
fn lapic_read(offset: u64) -> u32 {
    let base = lapic_base();
    if base == 0 {
        return 0;
    }
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

pub fn init(phys_offset: u64) -> u32 {
    let mut msr = Msr::new(IA32_APIC_BASE_MSR);
    let mut val = unsafe { msr.read() };
    let base = val & 0xfffff000;
    if (val & APIC_ENABLE) == 0 {
        val |= APIC_ENABLE;
        unsafe { msr.write(val) };
    }

    LAPIC_BASE_VA.store(base + phys_offset, Ordering::Relaxed);

    // Enable spurious interrupt vector (bit 8)
    let svr = lapic_read(LAPIC_SVR);
    lapic_write(LAPIC_SVR, svr | 0x100);
    // Keep legacy PIC IRQ delivery alive on BSP after LAPIC is enabled:
    // LINT0 = ExtINT (virtual wire), LINT1 = NMI.
    lapic_write(LAPIC_LVT_LINT0, 0x0000_0700);
    lapic_write(LAPIC_LVT_LINT1, 0x0000_0400);
    lapic_id()
}

pub fn lapic_id() -> u32 {
    lapic_read(LAPIC_ID) >> 24
}

pub fn eoi() {
    lapic_write(LAPIC_EOI, 0);
}

pub fn send_ipi(apic_id: u8, icr_low: u32) {
    lapic_write(LAPIC_ICR_HIGH, (apic_id as u32) << 24);
    lapic_write(LAPIC_ICR_LOW, icr_low);
}

pub fn init_timer(vector: u8) {
    // Divide by 1 (0b1011), periodic mode
    lapic_write(LAPIC_TIMER_DIV_CONF, 0b1011);
    lapic_write(LAPIC_LVT_TIMER, 0x20000 | vector as u32);
    lapic_write(LAPIC_TIMER_INIT_CNT, 0x10000);
}
