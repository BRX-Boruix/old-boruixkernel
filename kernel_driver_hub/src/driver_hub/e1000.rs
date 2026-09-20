use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use spin::Mutex;

use crate::driver_hub::pci;
use super::device::{Device, DeviceKind, DeviceOps, IoDevice};
use kernel_platform::memory as mem;

const RX_DESC_COUNT: usize = 32;
const TX_DESC_COUNT: usize = 32;
const TX_BUF_SIZE: usize = 2048;

const REG_CTRL: u32 = 0x0000;
const REG_STATUS: u32 = 0x0008;
const REG_EERD: u32 = 0x0014;
const REG_RCTL: u32 = 0x0100;
const REG_TCTL: u32 = 0x0400;
const REG_TIPG: u32 = 0x0410;
const REG_RDBAL: u32 = 0x2800;
const REG_RDBAH: u32 = 0x2804;
const REG_RDLEN: u32 = 0x2808;
const REG_RDH: u32 = 0x2810;
const REG_RDT: u32 = 0x2818;
const REG_TDBAL: u32 = 0x3800;
const REG_TDBAH: u32 = 0x3804;
const REG_TDLEN: u32 = 0x3808;
const REG_TDH: u32 = 0x3810;
const REG_TDT: u32 = 0x3818;
const REG_RAL: u32 = 0x5400;
const REG_RAH: u32 = 0x5404;

const RCTL_EN: u32 = 1 << 1;
const RCTL_SBP: u32 = 1 << 2;
const RCTL_UPE: u32 = 1 << 3;
const RCTL_MPE: u32 = 1 << 4;
const RCTL_LPE: u32 = 1 << 5;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;

const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;

const CMD_EOP: u8 = 1 << 0;
const CMD_IFCS: u8 = 1 << 1;
const CMD_RS: u8 = 1 << 3;
const TSTA_DD: u8 = 1 << 0;
const RSTA_DD: u8 = 1 << 0;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

pub struct E1000 {
    mmio_base: usize,
    mac: [u8; 6],
    rx_descs: &'static mut [RxDesc; RX_DESC_COUNT],
    tx_descs: &'static mut [TxDesc; TX_DESC_COUNT],
    rx_bufs: [*mut u8; RX_DESC_COUNT],
    tx_bufs: [*mut u8; TX_DESC_COUNT],
    rx_cur: usize,
    tx_cur: usize,
}

unsafe impl Send for E1000 {}
unsafe impl Sync for E1000 {}

static DEVICE: Mutex<Option<E1000>> = Mutex::new(None);
static READY: AtomicBool = AtomicBool::new(false);
static TX_OK: AtomicUsize = AtomicUsize::new(0);
static RX_OK: AtomicUsize = AtomicUsize::new(0);

struct NetDevice;

impl Device for NetDevice {
    fn name(&self) -> &'static str {
        "net0"
    }

    fn kind(&self) -> DeviceKind {
        DeviceKind::Char
    }
}

impl IoDevice for NetDevice {
    fn read(&self, out: &mut [u8]) -> usize {
        let ret = recv(out.as_mut_ptr(), out.len());
        if ret <= 0 { 0 } else { ret as usize }
    }

    fn write(&self, data: &[u8]) -> usize {
        let ret = send(data.as_ptr(), data.len());
        if ret <= 0 { 0 } else { ret as usize }
    }

    fn poll(&self) -> bool {
        rx_ready()
    }
}

impl DeviceOps for NetDevice {}

pub fn device_ops() -> &'static dyn DeviceOps {
    static DEV: NetDevice = NetDevice;
    &DEV
}

pub fn attach(bus: u8, device: u8, function: u8) -> bool {
    let bar0 = pci::read_bar0(bus, device, function);
    if bar0 == 0 {
        logger::warn!("e1000: BAR0 missing");
        return false;
    }
    if bar0 & 0x1 != 0 {
        logger::warn!("e1000: BAR0 is IO space, unsupported");
        return false;
    }
    pci::enable_bus_master(bus, device, function);

    let phys_base = (bar0 & 0xffff_fff0) as u64;
    let phys_offset = match mem::addr_space::PHYS_OFFSET.get() {
        Some(v) => *v,
        None => {
            logger::warn!("e1000: PHYS_OFFSET missing");
            return false;
        }
    };
    let mmio_base = (phys_base + phys_offset) as usize;
    logger::info!("e1000: bar0={:#x} mmio={:#x}", bar0, mmio_base);

    let Some(rx_descs) = alloc_descs::<RxDesc, RX_DESC_COUNT>() else {
        logger::warn!("e1000: rx desc alloc failed");
        return false;
    };
    let Some(tx_descs) = alloc_descs::<TxDesc, TX_DESC_COUNT>() else {
        logger::warn!("e1000: tx desc alloc failed");
        return false;
    };

    let mut rx_bufs = [core::ptr::null_mut(); RX_DESC_COUNT];
    for i in 0..RX_DESC_COUNT {
        let Some((phys, virt)) = alloc_frame_zero() else {
            logger::warn!("e1000: rx buf alloc failed");
            return false;
        };
        rx_bufs[i] = virt;
        let desc = &mut rx_descs[i];
        desc.addr = phys.as_u64();
        desc.status = 0;
        desc.errors = 0;
        desc.length = 0;
        desc.checksum = 0;
        desc.special = 0;
    }

    let mut tx_bufs = [core::ptr::null_mut(); TX_DESC_COUNT];
    for (i, slot) in tx_bufs.iter_mut().enumerate() {
        let Some((phys, virt)) = alloc_frame_zero() else {
            logger::warn!("e1000: tx buf alloc failed");
            return false;
        };
        *slot = virt;
        let desc = &mut tx_descs[i];
        desc.addr = phys.as_u64();
        desc.status = TSTA_DD;
        desc.cmd = 0;
        desc.length = 0;
        desc.cso = 0;
        desc.css = 0;
        desc.special = 0;
    }

    let mut dev = E1000 {
        mmio_base,
        mac: [0; 6],
        rx_descs,
        tx_descs,
        rx_bufs,
        tx_bufs,
        rx_cur: 0,
        tx_cur: 0,
    };
    dev.mac = dev.read_mac();
    dev.init_hw();
    logger::info!(
        "e1000: mac {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        dev.mac[0],
        dev.mac[1],
        dev.mac[2],
        dev.mac[3],
        dev.mac[4],
        dev.mac[5]
    );

    *DEVICE.lock() = Some(dev);
    READY.store(true, Ordering::SeqCst);
    true
}

pub fn ready() -> bool {
    READY.load(Ordering::SeqCst)
}

pub fn rx_ready() -> bool {
    let mut guard = DEVICE.lock();
    let Some(dev) = guard.as_mut() else {
        return false;
    };
    let idx = dev.rx_cur;
    let desc = &mut dev.rx_descs[idx];
    (desc.status & RSTA_DD) != 0
}

pub fn mac_addr() -> Option<[u8; 6]> {
    let guard = DEVICE.lock();
    guard.as_ref().map(|d| d.mac)
}

pub fn status() -> Option<u32> {
    let guard = DEVICE.lock();
    guard.as_ref().map(|d| d.read_reg(REG_STATUS))
}

pub fn regs(out: &mut [u32; 8]) -> bool {
    let guard = DEVICE.lock();
    let Some(dev) = guard.as_ref() else {
        return false;
    };
    out[0] = dev.read_reg(REG_STATUS);
    out[1] = dev.read_reg(REG_CTRL);
    out[2] = dev.read_reg(REG_RCTL);
    out[3] = dev.read_reg(REG_TCTL);
    out[4] = dev.read_reg(REG_RDH);
    out[5] = dev.read_reg(REG_RDT);
    out[6] = dev.read_reg(REG_TDH);
    out[7] = dev.read_reg(REG_TDT);
    true
}

pub fn counters() -> (u64, u64) {
    (
        TX_OK.load(Ordering::Relaxed) as u64,
        RX_OK.load(Ordering::Relaxed) as u64,
    )
}

pub fn send(buf: *const u8, len: usize) -> isize {
    let mut guard = DEVICE.lock();
    let Some(dev) = guard.as_mut() else {
        return -1;
    };
    if len == 0 || len > TX_BUF_SIZE {
        return -1;
    }
    let idx = dev.tx_cur;
    let desc = &mut dev.tx_descs[idx];
    if (desc.status & TSTA_DD) == 0 {
        return 0;
    }
    let send_len = if len < 60 { 60 } else { len };
    if send_len > TX_BUF_SIZE {
        return -1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(buf, dev.tx_bufs[idx], len);
        if send_len > len {
            dev.tx_bufs[idx].add(len).write_bytes(0, send_len - len);
        }
    }
    desc.length = send_len as u16;
    desc.cmd = CMD_EOP | CMD_IFCS | CMD_RS;
    desc.status = 0;
    dev.write_reg(REG_TDT, ((idx + 1) % TX_DESC_COUNT) as u32);
    dev.tx_cur = (idx + 1) % TX_DESC_COUNT;
    TX_OK.fetch_add(1, Ordering::Relaxed);
    len as isize
}

pub fn recv(buf: *mut u8, len: usize) -> isize {
    let mut guard = DEVICE.lock();
    let Some(dev) = guard.as_mut() else {
        return -1;
    };
    let idx = dev.rx_cur;
    let desc = &mut dev.rx_descs[idx];
    if (desc.status & RSTA_DD) == 0 {
        return 0;
    }
    let pkt_len = desc.length as usize;
    let copy_len = core::cmp::min(pkt_len, len);
    unsafe {
        core::ptr::copy_nonoverlapping(dev.rx_bufs[idx], buf, copy_len);
    }
    desc.status = 0;
    dev.write_reg(REG_RDT, idx as u32);
    dev.rx_cur = (idx + 1) % RX_DESC_COUNT;
    RX_OK.fetch_add(1, Ordering::Relaxed);
    copy_len as isize
}

impl E1000 {
    fn init_hw(&mut self) {
        let ctrl = self.read_reg(REG_CTRL);
        self.write_reg(REG_CTRL, ctrl | (1 << 26));
        while (self.read_reg(REG_CTRL) & (1 << 26)) != 0 {}
        self.write_reg(REG_CTRL, (1 << 6) | (1 << 5));
        let _ = self.read_reg(REG_STATUS);
        self.write_reg(
            REG_RAL,
            u32::from_le_bytes([self.mac[0], self.mac[1], self.mac[2], self.mac[3]]),
        );
        self.write_reg(
            REG_RAH,
            u32::from_le_bytes([self.mac[4], self.mac[5], 0, 0]) | (1 << 31),
        );

        let rx_descs_phys = self.rx_descs.as_ptr() as u64 - self.phys_offset();
        self.write_reg(REG_RDBAL, rx_descs_phys as u32);
        self.write_reg(REG_RDBAH, (rx_descs_phys >> 32) as u32);
        self.write_reg(
            REG_RDLEN,
            (RX_DESC_COUNT * core::mem::size_of::<RxDesc>()) as u32,
        );
        self.write_reg(REG_RDH, 0);
        self.write_reg(REG_RDT, (RX_DESC_COUNT - 1) as u32);
        self.write_reg(
            REG_RCTL,
            RCTL_EN | RCTL_SBP | RCTL_UPE | RCTL_MPE | RCTL_LPE | RCTL_BAM | RCTL_SECRC,
        );

        let tx_descs_phys = self.tx_descs.as_ptr() as u64 - self.phys_offset();
        self.write_reg(REG_TDBAL, tx_descs_phys as u32);
        self.write_reg(REG_TDBAH, (tx_descs_phys >> 32) as u32);
        self.write_reg(
            REG_TDLEN,
            (TX_DESC_COUNT * core::mem::size_of::<TxDesc>()) as u32,
        );
        self.write_reg(REG_TDH, 0);
        self.write_reg(REG_TDT, 0);
        self.write_reg(REG_TCTL, TCTL_EN | TCTL_PSP | (0x10 << 4) | (0x40 << 12));
        self.write_reg(REG_TIPG, 0x0060_200A);
    }

    fn read_mac(&mut self) -> [u8; 6] {
        let mut mac = [0u8; 6];
        for i in 0..3usize {
            let data = self.read_eeprom(i as u8);
            mac[i * 2] = (data & 0xff) as u8;
            mac[i * 2 + 1] = (data >> 8) as u8;
        }
        mac
    }

    fn read_eeprom(&mut self, index: u8) -> u16 {
        let value = ((index as u32) << 8) | 1;
        self.write_reg(REG_EERD, value);
        loop {
            let data = self.read_reg(REG_EERD);
            if (data & (1 << 4)) != 0 {
                return (data >> 16) as u16;
            }
        }
    }

    fn read_reg(&self, offset: u32) -> u32 {
        unsafe { core::ptr::read_volatile((self.mmio_base + offset as usize) as *const u32) }
    }

    fn write_reg(&self, offset: u32, value: u32) {
        unsafe { core::ptr::write_volatile((self.mmio_base + offset as usize) as *mut u32, value) }
    }

    fn phys_offset(&self) -> u64 {
        *mem::addr_space::PHYS_OFFSET
            .get()
            .expect("PHYS_OFFSET missing")
    }
}

fn alloc_descs<T: Copy, const N: usize>() -> Option<&'static mut [T; N]> {
    let (phys, virt) = alloc_frame_zero()?;
    let _ = phys;
    let ptr = virt as *mut T;
    unsafe { Some(&mut *(ptr as *mut [T; N])) }
}

fn alloc_frame_zero() -> Option<(mem::addr_space::PhysAddr, *mut u8)> {
    let frame = mem::pmm::frame_allocator::allocate_frame()?;
    let phys = mem::addr_space::PhysAddr::new(frame.start_address().as_u64());
    let phys_offset = *mem::addr_space::PHYS_OFFSET.get()?;
    let virt = (phys.as_u64() + phys_offset) as *mut u8;
    unsafe {
        virt.write_bytes(0, 4096);
    }
    Some((phys, virt))
}
