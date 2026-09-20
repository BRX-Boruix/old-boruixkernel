use alloc::boxed::Box;

use spin::Mutex;
use x86_64::instructions::port::Port;

use super::terminal;
use super::device::{BusType, Device, DeviceInfo, DeviceKind, DeviceOps, IoDevice};
use super::register_device_info;

const ATA_DATA: u16 = 0x1F0;
const ATA_ERROR: u16 = 0x1F1;
const ATA_FEATURES: u16 = 0x1F1;
const ATA_SECTOR_COUNT: u16 = 0x1F2;
const ATA_LBA_LOW: u16 = 0x1F3;
const ATA_LBA_MID: u16 = 0x1F4;
const ATA_LBA_HIGH: u16 = 0x1F5;
const ATA_DRIVE: u16 = 0x1F6;
const ATA_STATUS: u16 = 0x1F7;
const ATA_COMMAND: u16 = 0x1F7;

const ATA_SR_BSY: u8 = 0x80;
const ATA_SR_DRDY: u8 = 0x40;
const ATA_SR_DF: u8 = 0x20;
const ATA_SR_DRQ: u8 = 0x08;
const ATA_SR_ERR: u8 = 0x01;

const ATA_CMD_IDENTIFY: u8 = 0xEC;
const ATA_CMD_READ_SECTORS: u8 = 0x20;
const ATA_CMD_WRITE_SECTORS: u8 = 0x30;

fn io_delay() {
    unsafe {
        let mut port = Port::<u8>::new(0x80);
        port.write(0u8);
    }
}

fn status_read() -> u8 {
    unsafe { Port::<u8>::new(ATA_STATUS).read() }
}

fn wait_not_busy() -> bool {
    for _ in 0..10_000 {
        let s = status_read();
        if s == 0xFF {
            return false;
        }
        if (s & ATA_SR_BSY) == 0 {
            return true;
        }
    }
    false
}

fn wait_drq() -> bool {
    for _ in 0..10_000 {
        let s = status_read();
        if s == 0xFF {
            return false;
        }
        if (s & ATA_SR_BSY) == 0 && (s & ATA_SR_DRQ) != 0 {
            return true;
        }
        if (s & ATA_SR_ERR) != 0 || (s & ATA_SR_DF) != 0 {
            return false;
        }
    }
    false
}

fn select_drive_lba(lba: u64) {
    let drive = 0xE0u8 | (((lba >> 24) & 0x0F) as u8);
    unsafe { Port::<u8>::new(ATA_DRIVE).write(drive) };
    io_delay();
}

fn set_lba_regs(lba: u64, count: u8) {
    unsafe {
        Port::<u8>::new(ATA_FEATURES).write(0u8);
        Port::<u8>::new(ATA_SECTOR_COUNT).write(count);
        Port::<u8>::new(ATA_LBA_LOW).write((lba & 0xFF) as u8);
        Port::<u8>::new(ATA_LBA_MID).write(((lba >> 8) & 0xFF) as u8);
        Port::<u8>::new(ATA_LBA_HIGH).write(((lba >> 16) & 0xFF) as u8);
    }
}

fn identify() -> Option<(u16, u16, u16, u64)> {
    if !wait_not_busy() {
        return None;
    }
    select_drive_lba(0);
    unsafe {
        Port::<u8>::new(ATA_SECTOR_COUNT).write(0);
        Port::<u8>::new(ATA_LBA_LOW).write(0);
        Port::<u8>::new(ATA_LBA_MID).write(0);
        Port::<u8>::new(ATA_LBA_HIGH).write(0);
        Port::<u8>::new(ATA_COMMAND).write(ATA_CMD_IDENTIFY);
    }
    let status = status_read();
    if status == 0 {
        return None;
    }
    if !wait_drq() {
        return None;
    }
    let mut data = [0u16; 256];
    unsafe {
        let mut port = Port::<u16>::new(ATA_DATA);
        for word in data.iter_mut() {
            *word = port.read();
        }
    }
    let cyl = data[1];
    let head = data[3];
    let sect = data[6];
    let lba28 = ((data[60] as u32) | ((data[61] as u32) << 16)) as u64;
    let lba48 = ((data[100] as u64)
        | ((data[101] as u64) << 16)
        | ((data[102] as u64) << 32)
        | ((data[103] as u64) << 48));
    let sectors = if lba48 != 0 { lba48 } else { lba28 };
    Some((cyl, head, sect, sectors))
}

pub struct AtaPioDevice {
    name: &'static str,
    sectors: u64,
    lock: Mutex<()>,
}

impl Device for AtaPioDevice {
    fn name(&self) -> &'static str {
        self.name
    }

    fn kind(&self) -> DeviceKind {
        DeviceKind::Block
    }
}

impl IoDevice for AtaPioDevice {
    fn read_at(&self, offset: u64, out: &mut [u8]) -> usize {
        let _guard = self.lock.lock();
        let mut lba = offset / 512;
        let mut sector_off = (offset % 512) as usize;
        if lba >= self.sectors {
            return 0;
        }
        let mut done = 0usize;
        let mut buf = [0u8; 512];
        let mut remaining = out.len();
        while remaining > 0 && lba < self.sectors {
            if !ata_read_sector(lba, &mut buf) {
                break;
            }
            let take = core::cmp::min(remaining, 512 - sector_off);
            out[done..done + take].copy_from_slice(&buf[sector_off..sector_off + take]);
            done += take;
            remaining -= take;
            lba += 1;
            sector_off = 0;
        }
        done
    }

    fn write_at(&self, offset: u64, data: &[u8]) -> usize {
        let _guard = self.lock.lock();
        let mut lba = offset / 512;
        let mut sector_off = (offset % 512) as usize;
        if lba >= self.sectors {
            return 0;
        }
        let mut done = 0usize;
        let mut remaining = data.len();
        let mut buf = [0u8; 512];
        while remaining > 0 && lba < self.sectors {
            let take = core::cmp::min(remaining, 512 - sector_off);
            if sector_off != 0 || take < 512 {
                if !ata_read_sector(lba, &mut buf) {
                    break;
                }
                buf[sector_off..sector_off + take]
                    .copy_from_slice(&data[done..done + take]);
                if !ata_write_sector(lba, &buf) {
                    break;
                }
            } else {
                buf.copy_from_slice(&data[done..done + 512]);
                if !ata_write_sector(lba, &buf) {
                    break;
                }
            }
            done += take;
            remaining -= take;
            lba += 1;
            sector_off = 0;
        }
        done
    }

    fn size(&self) -> Option<u64> {
        Some(self.sectors * 512)
    }
}

impl DeviceOps for AtaPioDevice {}

fn ata_read_sector(lba: u64, out: &mut [u8; 512]) -> bool {
    for _ in 0..3 {
        if !wait_not_busy() {
            print3("ata_pio: read wait_not_busy timeout");
            continue;
        }
        select_drive_lba(lba);
        set_lba_regs(lba, 1);
        unsafe { Port::<u8>::new(ATA_COMMAND).write(ATA_CMD_READ_SECTORS) };
        let st = status_read();
        if st == 0xFF {
            print3("ata_pio: read status 0xFF");
            return false;
        }
        if (st & ATA_SR_ERR) != 0 || (st & ATA_SR_DF) != 0 {
            print3("ata_pio: read status error");
            continue;
        }
        if !wait_drq() {
            print3("ata_pio: read wait_drq timeout");
            continue;
        }
        unsafe {
            let mut port = Port::<u16>::new(ATA_DATA);
            for i in 0..256 {
                let word = port.read();
                out[i * 2] = (word & 0xFF) as u8;
                out[i * 2 + 1] = (word >> 8) as u8;
            }
        }
        return true;
    }
    false
}

fn ata_write_sector(lba: u64, data: &[u8; 512]) -> bool {
    for _ in 0..3 {
        if !wait_not_busy() {
            print3("ata_pio: write wait_not_busy timeout");
            continue;
        }
        select_drive_lba(lba);
        set_lba_regs(lba, 1);
        unsafe { Port::<u8>::new(ATA_COMMAND).write(ATA_CMD_WRITE_SECTORS) };
        let st = status_read();
        if st == 0xFF {
            print3("ata_pio: write status 0xFF");
            return false;
        }
        if (st & ATA_SR_ERR) != 0 || (st & ATA_SR_DF) != 0 {
            print3("ata_pio: write status error");
            continue;
        }
        if !wait_drq() {
            print3("ata_pio: write wait_drq timeout");
            continue;
        }
        unsafe {
            let mut port = Port::<u16>::new(ATA_DATA);
            for i in 0..256 {
                let word = (data[i * 2] as u16) | ((data[i * 2 + 1] as u16) << 8);
                port.write(word);
            }
        }
        if wait_not_busy() {
            return true;
        }
    }
    false
}

pub fn init_primary() {
    let info = identify();
    let Some((_cyl, _head, _sect, sectors)) = info else {
        return;
    };
    let dev = AtaPioDevice {
        name: "ata0",
        sectors,
        lock: Mutex::new(()),
    };
    let dev_ref = Box::leak(Box::new(dev));
    register_device_info(
        DeviceInfo {
            name: dev_ref.name(),
            kind: DeviceKind::Block,
            bus: BusType::Platform,
            location: 0,
            vendor_id: 0,
            device_id: 0,
            class_code: 0x01,
            subclass: 0x01,
            prog_if: 0x80,
        },
        Some(dev_ref as &dyn DeviceOps),
        Some("ata-pio"),
    );
}

fn print3(msg: &str) {
    terminal::write_bytes(msg.as_bytes());
    terminal::write_bytes(b"\n");
}
