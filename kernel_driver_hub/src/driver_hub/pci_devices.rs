extern crate alloc;

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::boxed::Box;
use alloc::string::String;
use super::device::{Device, DeviceInfo, DeviceKind, DeviceOps, IoDevice};
use spin::Mutex;

const MAX_PCI_NODES: usize = 32;

struct BufWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> BufWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }
}

impl<'a> Write for BufWriter<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buf.len().saturating_sub(self.pos);
        let n = core::cmp::min(remaining, bytes.len());
        if n > 0 {
            self.buf[self.pos..self.pos + n].copy_from_slice(&bytes[..n]);
            self.pos += n;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct PciClassDevice {
    name: &'static str,
    info: DeviceInfo,
    kind: DeviceKind,
}

impl Device for PciClassDevice {
    fn name(&self) -> &'static str {
        self.name
    }

    fn kind(&self) -> DeviceKind {
        self.kind
    }
}

impl IoDevice for PciClassDevice {
    fn read(&self, out: &mut [u8]) -> usize {
        let mut w = BufWriter::new(out);
        let bus = ((self.info.location >> 16) & 0xff) as u8;
        let dev = ((self.info.location >> 8) & 0xff) as u8;
        let func = (self.info.location & 0xff) as u8;
        let _ = writeln!(
            w,
            "pci device {}",
            self.name
        );
        let _ = writeln!(
            w,
            "  loc={:02x}:{:02x}.{:02x}",
            bus, dev, func
        );
        let _ = writeln!(
            w,
            "  vendor={:04x} device={:04x}",
            self.info.vendor_id, self.info.device_id
        );
        let _ = writeln!(
            w,
            "  class={:02x} subclass={:02x} prog-if={:02x}",
            self.info.class_code, self.info.subclass, self.info.prog_if
        );
        w.pos
    }

    fn write(&self, _data: &[u8]) -> usize {
        0
    }

    fn poll(&self) -> bool {
        false
    }

    fn ioctl(&self, _cmd: usize, _arg: usize) -> isize {
        -1
    }
}

impl DeviceOps for PciClassDevice {}

static DEVICE_COUNT: AtomicUsize = AtomicUsize::new(0);
static PCI_DEVICES: Mutex<[Option<PciClassDevice>; MAX_PCI_NODES]> =
    Mutex::new([None; MAX_PCI_NODES]);

fn build_name(prefix: &str, idx: usize) -> &'static str {
    let mut s = String::new();
    if idx == 0 {
        let _ = write!(s, "{}0", prefix);
    } else {
        let _ = write!(s, "{}{}", prefix, idx);
    }
    Box::leak(s.into_boxed_str())
}

fn alloc_device(name: &'static str, info: DeviceInfo, kind: DeviceKind) -> Option<&'static PciClassDevice> {
    let idx = DEVICE_COUNT.fetch_add(1, Ordering::Relaxed);
    if idx >= MAX_PCI_NODES {
        return None;
    }
    let dev = PciClassDevice {
        name,
        info,
        kind,
    };
    let mut list = PCI_DEVICES.lock();
    list[idx] = Some(dev);
    let dev_ref = list[idx].as_ref().unwrap() as *const PciClassDevice;
    Some(unsafe { &*dev_ref })
}

static BLOCK_COUNT: AtomicUsize = AtomicUsize::new(0);
static NET_COUNT: AtomicUsize = AtomicUsize::new(0);
static DISPLAY_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn register_pci_block(info: DeviceInfo) -> Option<&'static dyn DeviceOps> {
    let idx = BLOCK_COUNT.fetch_add(1, Ordering::Relaxed);
    let name = build_name("pci-block", idx);
    alloc_device(name, info, DeviceKind::Block).map(|d| d as &dyn DeviceOps)
}

pub fn register_pci_net(info: DeviceInfo) -> Option<&'static dyn DeviceOps> {
    let idx = NET_COUNT.fetch_add(1, Ordering::Relaxed);
    let name = build_name("pci-net", idx);
    alloc_device(name, info, DeviceKind::Net).map(|d| d as &dyn DeviceOps)
}

pub fn register_pci_display(info: DeviceInfo) -> Option<&'static dyn DeviceOps> {
    let idx = DISPLAY_COUNT.fetch_add(1, Ordering::Relaxed);
    let name = build_name("pci-display", idx);
    alloc_device(name, info, DeviceKind::Display).map(|d| d as &dyn DeviceOps)
}
