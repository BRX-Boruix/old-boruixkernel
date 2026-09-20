use core::mem::size_of;
use core::ptr::read_unaligned;
use limine::RsdpRequest;

use crate::hal::arch;
use crate::memory as mem;
use logger::{info, warn};
use x86_64::instructions::port::Port;

static RSDP_REQUEST: RsdpRequest = RsdpRequest::new(0);

#[repr(C, packed)]
struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_addr: u32,
    length: u32,
    xsdt_addr: u64,
    extended_checksum: u8,
    _reserved: [u8; 3],
}

#[repr(C, packed)]
struct SdtHeader {
    signature: [u8; 4],
    length: u32,
    _revision: u8,
    _checksum: u8,
    _oem_id: [u8; 6],
    _oem_table_id: [u8; 8],
    _oem_revision: u32,
    _creator_id: u32,
    _creator_revision: u32,
}

#[repr(C, packed)]
struct Madt {
    header: SdtHeader,
    _lapic_addr: u32,
    _flags: u32,
}

#[repr(C, packed)]
struct MadtEntryHeader {
    entry_type: u8,
    length: u8,
}

#[repr(C, packed)]
struct IoApicEntry {
    entry_type: u8,
    length: u8,
    _ioapic_id: u8,
    _reserved: u8,
    ioapic_addr: u32,
    gsi_base: u32,
}

#[repr(C, packed)]
struct IsoEntry {
    entry_type: u8,
    length: u8,
    bus: u8,
    source: u8,
    gsi: u32,
    flags: u16,
}

struct IoApic {
    base: *mut u32,
    gsi_base: u32,
}

impl IoApic {
    unsafe fn write(&self, reg: u32, val: u32) {
        self.base.write_volatile(reg);
        self.base.add(4).write_volatile(val);
    }

    unsafe fn set_redir(&self, gsi: u32, vector: u8, flags: u16, dest_apic: u8) {
        let redir_index = gsi - self.gsi_base;
        let low = 0x10 + redir_index * 2;
        let high = low + 1;

        let mut lo = vector as u32;
        if (flags & 0x2) != 0 {
            lo |= 1 << 13; // active low
        }
        if (flags & 0x8) != 0 {
            lo |= 1 << 15; // level trigger
        }

        self.write(high, (dest_apic as u32) << 24);
        self.write(low, lo);
    }
}

fn phys_to_virt(phys: u64) -> *const u8 {
    let offset = *mem::addr_space::PHYS_OFFSET
        .get()
        .expect("PHYS_OFFSET missing");
    (phys + offset) as *const u8
}

fn find_madt(rsdp: &Rsdp) -> Option<&'static Madt> {
    let xsdt = rsdp.revision >= 2 && rsdp.xsdt_addr != 0;
    if xsdt {
        let xsdt_ptr = phys_to_virt(rsdp.xsdt_addr) as *const SdtHeader;
        let header = unsafe { &*xsdt_ptr };
        let entries = (header.length as usize - size_of::<SdtHeader>()) / 8;
        let base = unsafe { xsdt_ptr.add(1) as *const u64 };
        for i in 0..entries {
            let addr = unsafe { *base.add(i) };
            let h = unsafe { &*(phys_to_virt(addr) as *const SdtHeader) };
            if &h.signature == b"APIC" {
                return Some(unsafe { &*(phys_to_virt(addr) as *const Madt) });
            }
        }
    } else {
        let rsdt_ptr = phys_to_virt(rsdp.rsdt_addr as u64) as *const SdtHeader;
        let header = unsafe { &*rsdt_ptr };
        let entries = (header.length as usize - size_of::<SdtHeader>()) / 4;
        let base = unsafe { rsdt_ptr.add(1) as *const u32 };
        for i in 0..entries {
            let addr = unsafe { *base.add(i) } as u64;
            let h = unsafe { &*(phys_to_virt(addr) as *const SdtHeader) };
            if &h.signature == b"APIC" {
                return Some(unsafe { &*(phys_to_virt(addr) as *const Madt) });
            }
        }
    }
    None
}

pub fn init() -> bool {
    let Some(resp) = RSDP_REQUEST.get_response().get() else {
        warn!("IOAPIC: no RSDP response");
        return false;
    };
    let Some(rsdp_ptr) = resp.address.as_ptr() else {
        warn!("IOAPIC: RSDP address missing");
        return false;
    };

    let phys_offset = *mem::addr_space::PHYS_OFFSET
        .get()
        .expect("PHYS_OFFSET missing");
    let rsdp_addr = rsdp_ptr as u64;
    let rsdp_va = if rsdp_addr >= phys_offset {
        rsdp_addr
    } else {
        rsdp_addr + phys_offset
    };
    let rsdp = unsafe { &*(rsdp_va as *const Rsdp) };
    let Some(madt) = find_madt(rsdp) else {
        warn!("IOAPIC: MADT not found");
        return false;
    };

    let mut ioapic: Option<IoApic> = None;
    let mut irq1_gsi: u32 = 1;
    let mut irq1_flags: u16 = 0;

    let mut ptr = (madt as *const Madt as *const u8).wrapping_add(size_of::<Madt>());
    let end = (madt as *const Madt as *const u8).wrapping_add(madt.header.length as usize);

    while (ptr as usize) < (end as usize) {
        let header = unsafe { &*(ptr as *const MadtEntryHeader) };
        if header.length == 0 {
            break;
        }
        match header.entry_type {
            1 => {
                let e = unsafe { &*(ptr as *const IoApicEntry) };
                if ioapic.is_none() {
                    let addr = unsafe { read_unaligned(core::ptr::addr_of!(e.ioapic_addr)) } as u64;
                    let gsi_base = unsafe { read_unaligned(core::ptr::addr_of!(e.gsi_base)) };
                    ioapic = Some(IoApic {
                        base: phys_to_virt(addr) as *mut u32,
                        gsi_base,
                    });
                }
            }
            2 => {
                let e = unsafe { &*(ptr as *const IsoEntry) };
                if e.bus == 0 && e.source == 1 {
                    irq1_gsi = unsafe { read_unaligned(core::ptr::addr_of!(e.gsi)) };
                    irq1_flags = unsafe { read_unaligned(core::ptr::addr_of!(e.flags)) };
                }
            }
            _ => {}
        }
        ptr = ptr.wrapping_add(header.length as usize);
    }

    let Some(ioa) = ioapic else {
        warn!("IOAPIC: not found");
        return false;
    };

    let vector = arch::interrupts::InterruptIndex::Keyboard as u8;
    let bsp_apic = arch::apic::lapic_id() as u8;
    unsafe {
        ioa.set_redir(irq1_gsi, vector, irq1_flags, bsp_apic);
    }
    arch::interrupts::set_ioapic_enabled(true);
    // Avoid double-delivery from legacy PIC once IOAPIC routing is active.
    arch::interrupts::mask_all_pic();
    enable_imcr();
    info!(
        "IOAPIC: irq1 gsi={} flags={:#x} -> vector {} apic {}",
        irq1_gsi, irq1_flags, vector, bsp_apic
    );
    true
}

fn enable_imcr() {
    unsafe {
        let mut index = Port::<u8>::new(0x22);
        let mut data = Port::<u8>::new(0x23);
        index.write(0x70);
        let val = data.read();
        data.write(val | 0x01);
    }
}
