use kernel_platform::hal::arch;
use kernel_platform::memory as mem;
use arch::TrapFrame;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use limine::{SmpInfo, SmpRequest};
use logger::info;
extern crate alloc;
use crate::task;
use alloc::string::String;
use core::fmt::Write;

#[no_mangle]
#[used]
pub static SMP_REQUEST: SmpRequest = SmpRequest::new(0);

const MAX_CPUS: usize = 64;

static AP_ONLINE: AtomicUsize = AtomicUsize::new(0);
static CPU_COUNT: AtomicUsize = AtomicUsize::new(1);
static LAPIC_TICKS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static APIC_IDS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static IPI_PENDING: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];

const IPI_RESCHED_BIT: usize = 1 << 0;
const IPI_TLB_BIT: usize = 1 << 1;

static RESCHED_SEQ: AtomicU64 = AtomicU64::new(0);
static RESCHED_ACK: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static TLB_SEQ: AtomicU64 = AtomicU64::new(0);
static TLB_ACK: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static TLB_WAIT_READY: AtomicUsize = AtomicUsize::new(0);
static CURRENT_CR3: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];

const RESCHED_THROTTLE_TICKS: u64 = 2;
static RESCHED_LAST_SEND: AtomicU64 = AtomicU64::new(0);

static RESCHED_SENT: AtomicU64 = AtomicU64::new(0);
static RESCHED_SKIPPED_PENDING: AtomicU64 = AtomicU64::new(0);
static RESCHED_SKIPPED_THROTTLE: AtomicU64 = AtomicU64::new(0);
static TLB_SENT: AtomicU64 = AtomicU64::new(0);
static TLB_SKIPPED_PENDING: AtomicU64 = AtomicU64::new(0);
static TLB_SKIPPED_TARGET: AtomicU64 = AtomicU64::new(0);
static TLB_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static TLB_UNRESP_MASK: AtomicU64 = AtomicU64::new(0);

const TLB_DEFER_TICKS: u64 = 2;
static TLB_DEFERRED_CR3: AtomicU64 = AtomicU64::new(0);
static TLB_DEFERRED_DEADLINE: AtomicU64 = AtomicU64::new(0);
static TLB_DEFERRED_PENDING: AtomicUsize = AtomicUsize::new(0);

pub fn init() {
    let phys_offset = match mem::addr_space::PHYS_OFFSET.get() {
        Some(v) => *v,
        None => 0,
    };
    let lapic_id = arch::apic::init(phys_offset);
    info!("SMP: BSP LAPIC ID {}", lapic_id);

    arch::interrupts::set_lapic_timer_handler(lapic_timer_tick);

    let resp_ptr = match SMP_REQUEST.get_response().as_ptr() {
        Some(p) => p,
        None => {
            info!("SMP: no response");
            return;
        }
    };

    let resp = unsafe { &mut *resp_ptr };
    let cpu_count = resp.cpu_count as usize;
    CPU_COUNT.store(cpu_count.max(1), Ordering::SeqCst);
    info!("SMP: reported {} CPU(s)", cpu_count);

    let bsp_lapic = resp.bsp_lapic_id;
    let mut aps = 0usize;

    for (i, cpu_ptr) in resp.cpus().iter_mut().enumerate() {
        let info = unsafe { &mut *cpu_ptr.as_ptr() };
        if i < MAX_CPUS {
            APIC_IDS[i].store(info.lapic_id as u64, Ordering::Relaxed);
        }
        if info.lapic_id == bsp_lapic {
            continue;
        }
        info.extra_argument = i as u64;
        unsafe {
            core::ptr::write_volatile(&mut info.goto_address, ap_entry);
        }
        aps += 1;
    }

    // Wait briefly for APs to report online.
    let expected = aps;
    for _ in 0..1_000_000 {
        if AP_ONLINE.load(Ordering::SeqCst) >= expected {
            break;
        }
        core::hint::spin_loop();
    }
    info!("SMP: {} AP(s) online", AP_ONLINE.load(Ordering::SeqCst));

    arch::interrupts::set_lapic_resched_handler(lapic_resched_ipi);
    arch::interrupts::set_lapic_tlb_handler(lapic_tlb_ipi);

    // Enable LAPIC timer on BSP (no preemption yet).
    arch::apic::init_timer(arch::interrupts::LAPIC_TIMER_VECTOR);
    // TLB shootdown ACK wait is only safe/meaningful after handlers are installed.
    TLB_WAIT_READY.store(1, Ordering::SeqCst);

    // Track current CR3 for BSP.
    let (frame, _) = x86_64::registers::control::Cr3::read();
    set_current_cr3_for(arch::current_cpu_id_raw(), frame.start_address().as_u64());
}

extern "C" fn ap_entry(info: *const SmpInfo) -> ! {
    let cpu_id = unsafe { (*info).extra_argument as usize };

    // Per-CPU GDT/TSS + IDT + GS/CpuData
    arch::gdt::init_cpu(cpu_id);
    arch::interrupts::load_idt();
    arch::init_ap(cpu_id);

    arch::apic::init_timer(arch::interrupts::LAPIC_TIMER_VECTOR);

    let (frame, _) = x86_64::registers::control::Cr3::read();
    set_current_cr3_for(cpu_id, frame.start_address().as_u64());

    AP_ONLINE.fetch_add(1, Ordering::SeqCst);

    // Enable interrupts on APs; still idle (no scheduling).
    x86_64::instructions::interrupts::enable();
    loop {
        core::hint::spin_loop();
        x86_64::instructions::hlt();
    }
}

fn lapic_timer_tick(tf: &mut TrapFrame) {
    task::check_interrupt_trapframe(tf);
    let cpu = arch::current_cpu_id_raw();
    if cpu < LAPIC_TICKS.len() {
        LAPIC_TICKS[cpu].fetch_add(1, Ordering::Relaxed);
    }
    if cpu == 0 {
        tlb_deferred_poll();
    }
    if task::preempt_enabled() {
        task::handle_timer(tf);
    }
}

fn lapic_resched_ipi(tf: &mut TrapFrame) {
    let cpu = arch::current_cpu_id_raw();
    if cpu < IPI_PENDING.len() {
        IPI_PENDING[cpu].fetch_and(!IPI_RESCHED_BIT, Ordering::SeqCst);
        let seq = RESCHED_SEQ.load(Ordering::SeqCst);
        RESCHED_ACK[cpu].store(seq, Ordering::SeqCst);
    }
    task::handle_resched_ipi(tf);
}

fn lapic_tlb_ipi(_tf: &mut TrapFrame) {
    x86_64::instructions::tlb::flush_all();
    let cpu = arch::current_cpu_id_raw();
    if cpu < IPI_PENDING.len() {
        IPI_PENDING[cpu].fetch_and(!IPI_TLB_BIT, Ordering::SeqCst);
        let seq = TLB_SEQ.load(Ordering::SeqCst);
        TLB_ACK[cpu].store(seq, Ordering::SeqCst);
    }
}

pub fn cpu_count() -> usize {
    CPU_COUNT.load(Ordering::SeqCst)
}

pub fn set_current_cr3(p4_phys: u64) {
    set_current_cr3_for(arch::current_cpu_id_raw(), p4_phys);
}

fn set_current_cr3_for(cpu: usize, p4_phys: u64) {
    if cpu < CURRENT_CR3.len() {
        CURRENT_CR3[cpu].store(p4_phys, Ordering::SeqCst);
    }
}

pub fn send_resched_ipi_all() {
    let count = cpu_count();
    if count <= 1 {
        return;
    }
    let cpu = arch::current_cpu_id_raw();
    if cpu < LAPIC_TICKS.len() {
        let now = LAPIC_TICKS[cpu].load(Ordering::Relaxed);
        let last = RESCHED_LAST_SEND.load(Ordering::Relaxed);
        if now != 0 && now.wrapping_sub(last) < RESCHED_THROTTLE_TICKS {
            RESCHED_SKIPPED_THROTTLE.fetch_add(1, Ordering::Relaxed);
            return;
        }
        RESCHED_LAST_SEND.store(now, Ordering::Relaxed);
    }
    RESCHED_SEQ.fetch_add(1, Ordering::SeqCst);
    let self_apic = arch::apic::lapic_id() as u64;
    for i in 0..count.min(MAX_CPUS) {
        let apic_id = APIC_IDS[i].load(Ordering::Relaxed);
        if apic_id == self_apic {
            continue;
        }
        if IPI_PENDING[i].fetch_or(IPI_RESCHED_BIT, Ordering::SeqCst) & IPI_RESCHED_BIT != 0 {
            RESCHED_SKIPPED_PENDING.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let icr_low = arch::interrupts::LAPIC_RESCHED_VECTOR as u32 | 0x0000_4000;
        arch::apic::send_ipi(apic_id as u8, icr_low);
        RESCHED_SENT.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn tlb_shootdown_request(p4_phys: u64) {
    let count = cpu_count();
    if count <= 1 {
        return;
    }
    let now = LAPIC_TICKS[0].load(Ordering::Relaxed);
    if TLB_DEFERRED_PENDING.load(Ordering::Relaxed) != 0 {
        let pending = TLB_DEFERRED_CR3.load(Ordering::Relaxed);
        let deadline = TLB_DEFERRED_DEADLINE.load(Ordering::Relaxed);
        if pending == p4_phys && now < deadline {
            return;
        }
        if pending != 0 {
            send_tlb_shootdown_target(pending);
        }
    }
    TLB_DEFERRED_CR3.store(p4_phys, Ordering::Relaxed);
    TLB_DEFERRED_DEADLINE.store(now.wrapping_add(TLB_DEFER_TICKS), Ordering::Relaxed);
    TLB_DEFERRED_PENDING.store(1, Ordering::Relaxed);
}

fn tlb_deferred_poll() {
    if TLB_DEFERRED_PENDING.load(Ordering::Relaxed) == 0 {
        return;
    }
    let now = LAPIC_TICKS[0].load(Ordering::Relaxed);
    let deadline = TLB_DEFERRED_DEADLINE.load(Ordering::Relaxed);
    if now < deadline {
        return;
    }
    let p4_phys = TLB_DEFERRED_CR3.load(Ordering::Relaxed);
    if p4_phys == 0 {
        TLB_DEFERRED_PENDING.store(0, Ordering::Relaxed);
        return;
    }
    send_tlb_shootdown_target(p4_phys);
    TLB_DEFERRED_PENDING.store(0, Ordering::Relaxed);
}

fn send_tlb_shootdown_target(p4_phys: u64) {
    let count = cpu_count();
    if count <= 1 {
        return;
    }
    let seq = TLB_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let self_apic = arch::apic::lapic_id() as u64;
    let mut mask: u64 = 0;
    for i in 0..count.min(MAX_CPUS) {
        let apic_id = APIC_IDS[i].load(Ordering::Relaxed);
        if apic_id == self_apic {
            continue;
        }
        if CURRENT_CR3[i].load(Ordering::SeqCst) != p4_phys {
            TLB_SKIPPED_TARGET.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        if IPI_PENDING[i].fetch_or(IPI_TLB_BIT, Ordering::SeqCst) & IPI_TLB_BIT != 0 {
            TLB_SKIPPED_PENDING.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let icr_low = arch::interrupts::LAPIC_TLB_VECTOR as u32 | 0x0000_4000;
        arch::apic::send_ipi(apic_id as u8, icr_low);
        if i < 64 {
            mask |= 1u64 << i;
        }
        TLB_SENT.fetch_add(1, Ordering::Relaxed);
    }
    if mask == 0 || TLB_WAIT_READY.load(Ordering::SeqCst) == 0 {
        return;
    }
    // Wait for ACKs from targeted CPUs (best-effort).
    let max = count.min(MAX_CPUS);
    let mut timeout = true;
    for _ in 0..1_000_000 {
        let mut done = true;
        for i in 0..max {
            if (mask & (1u64 << i)) == 0 {
                continue;
            }
            if (TLB_UNRESP_MASK.load(Ordering::Relaxed) & (1u64 << i)) != 0 {
                continue;
            }
            if TLB_ACK[i].load(Ordering::SeqCst) < seq {
                done = false;
                break;
            }
        }
        if done {
            timeout = false;
            break;
        }
        core::hint::spin_loop();
    }
    if timeout {
        TLB_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
        let mut bad_mask = 0u64;
        for i in 0..max {
            if (mask & (1u64 << i)) == 0 {
                continue;
            }
            if TLB_ACK[i].load(Ordering::SeqCst) < seq {
                bad_mask |= 1u64 << i;
            }
        }
        if bad_mask != 0 {
            let prev = TLB_UNRESP_MASK.fetch_or(bad_mask, Ordering::Relaxed);
            let newly = bad_mask & !prev;
            if newly != 0 {
                logger::warn!(
                    "TLB shootdown timeout: seq={} bad_mask={:#x}",
                    seq,
                    bad_mask
                );
            }
        }
    }
}

pub fn ipi_stat_string() -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "ipi resched sent={} pending_skip={} throttle_skip={}\n",
        RESCHED_SENT.load(Ordering::Relaxed),
        RESCHED_SKIPPED_PENDING.load(Ordering::Relaxed),
        RESCHED_SKIPPED_THROTTLE.load(Ordering::Relaxed)
    );
    let _ = write!(
        out,
        "ipi tlb sent={} pending_skip={} target_skip={}\n",
        TLB_SENT.load(Ordering::Relaxed),
        TLB_SKIPPED_PENDING.load(Ordering::Relaxed),
        TLB_SKIPPED_TARGET.load(Ordering::Relaxed)
    );
    let _ = write!(
        out,
        "ipi tlb timeouts={} unresp_mask={:#x}\n",
        TLB_TIMEOUTS.load(Ordering::Relaxed),
        TLB_UNRESP_MASK.load(Ordering::Relaxed)
    );
    out
}
