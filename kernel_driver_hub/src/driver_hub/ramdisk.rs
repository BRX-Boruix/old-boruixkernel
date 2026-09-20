use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::min;

use spin::Mutex;

use super::device::{Device, DeviceKind, DeviceOps, IoDevice};
use super::register_device_info;
use super::device::DeviceInfo;

pub struct RamDiskDevice {
    name: &'static str,
    data: Mutex<Vec<u8>>,
}

impl Device for RamDiskDevice {
    fn name(&self) -> &'static str {
        self.name
    }

    fn kind(&self) -> DeviceKind {
        DeviceKind::Block
    }
}

impl IoDevice for RamDiskDevice {
    fn read_at(&self, offset: u64, out: &mut [u8]) -> usize {
        let data = self.data.lock();
        let off = offset as usize;
        if off >= data.len() {
            return 0;
        }
        let n = min(out.len(), data.len() - off);
        out[..n].copy_from_slice(&data[off..off + n]);
        n
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> usize {
        let mut data = self.data.lock();
        let off = offset as usize;
        if off >= data.len() {
            return 0;
        }
        let n = min(buf.len(), data.len() - off);
        data[off..off + n].copy_from_slice(&buf[..n]);
        n
    }

    fn size(&self) -> Option<u64> {
        Some(self.data.lock().len() as u64)
    }
}

impl DeviceOps for RamDiskDevice {}

pub fn register_ramdisk(name: &str, image: &[u8]) -> &'static RamDiskDevice {
    let mut n = String::from(name);
    if n.is_empty() {
        n = String::from("ram0");
    }
    let dev = RamDiskDevice {
        name: Box::leak(n.into_boxed_str()),
        data: Mutex::new(image.to_vec()),
    };
    let dev_ref = Box::leak(Box::new(dev));
    register_device_info(
        DeviceInfo {
            name: dev_ref.name(),
            kind: DeviceKind::Block,
            bus: super::device::BusType::Platform,
            location: 0,
            vendor_id: 0,
            device_id: 0,
            class_code: 0,
            subclass: 0,
            prog_if: 0,
        },
        Some(dev_ref as &dyn DeviceOps),
        Some("ramdisk"),
    );
    dev_ref
}
