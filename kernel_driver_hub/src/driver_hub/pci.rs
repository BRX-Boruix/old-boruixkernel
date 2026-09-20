use alloc::vec::Vec;
use spin::{Lazy, Mutex};
use x86_64::instructions::{interrupts, port::Port};

use logger::info;

use super::device::{BusType, DeviceInfo, DeviceKind};
use super::register_device_info;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;
const ENABLE_BIT: u32 = 1 << 31;
const MAX_BUS: u8 = 255;

#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub header_type: u8,
    pub subsystem_vendor: u16,
    pub subsystem_device: u16,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PciDeviceInfo {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub header_type: u8,
    pub subsystem_vendor: u16,
    pub subsystem_device: u16,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum PciMode {
    Legacy = 0,
    Mcfg = 1,
}

static DEVICES: Lazy<Mutex<Vec<PciDevice>>> = Lazy::new(|| Mutex::new(Vec::new()));

impl PciDevice {
    fn to_info(&self) -> PciDeviceInfo {
        PciDeviceInfo {
            bus: self.bus,
            device: self.device,
            function: self.function,
            vendor_id: self.vendor_id,
            device_id: self.device_id,
            class_code: self.class_code,
            subclass: self.subclass,
            prog_if: self.prog_if,
            header_type: self.header_type,
            subsystem_vendor: self.subsystem_vendor,
            subsystem_device: self.subsystem_device,
            interrupt_line: self.interrupt_line,
            interrupt_pin: self.interrupt_pin,
        }
    }
}

pub fn init() {
    let mut scanned = Vec::new();
    for bus in 0..=MAX_BUS {
        scan_bus(bus, &mut scanned);
    }

    info!("PCI: found {} device(s)", scanned.len());
    for dev in &scanned {
        info!(
            "PCI device {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x} subclass={:02x} prog-if={:02x}",
            dev.bus,
            dev.device,
            dev.function,
            dev.vendor_id,
            dev.device_id,
            dev.class_code,
            dev.subclass,
            dev.prog_if,
        );
    }

    for dev in &scanned {
        register_device_info(
            DeviceInfo {
                name: "pci-device",
                kind: kind_for_class(dev.class_code),
                bus: BusType::Pci,
                location: pci_location(dev.bus, dev.device, dev.function),
                vendor_id: dev.vendor_id,
                device_id: dev.device_id,
                class_code: dev.class_code,
                subclass: dev.subclass,
                prog_if: dev.prog_if,
            },
            None,
            None,
        );
    }

    let mut guard = DEVICES.lock();
    guard.extend(scanned.into_iter());
}

pub fn device_count() -> usize {
    DEVICES.lock().len()
}

pub fn fill_device_list(dst: &mut [PciDeviceInfo]) -> usize {
    let guard = DEVICES.lock();
    let total = guard.len();
    let count = core::cmp::min(dst.len(), total);
    for (slot, dev) in dst.iter_mut().zip(guard.iter()) {
        *slot = dev.to_info();
    }
    count
}

pub fn device_by_address(bus: u8, device: u8, function: u8) -> Option<PciDeviceInfo> {
    let guard = DEVICES.lock();
    guard
        .iter()
        .find(|dev| dev.bus == bus && dev.device == device && dev.function == function)
        .map(|dev| dev.to_info())
}

pub fn mode() -> PciMode {
    PciMode::Legacy
}

fn scan_bus(bus: u8, output: &mut Vec<PciDevice>) {
    for device in 0..32 {
        scan_device(bus, device, output);
    }
}

fn scan_device(bus: u8, device: u8, output: &mut Vec<PciDevice>) {
    let vendor = read_config_u16(bus, device, 0, 0x0);
    if vendor == 0xFFFF {
        return;
    }
    let header_type = read_config_u8(bus, device, 0, 0x0E);
    let functions = if header_type & 0x80 != 0 { 8 } else { 1 };
    for function in 0..functions {
        if let Some(info) = scan_function(bus, device, function) {
            output.push(info);
        }
    }
}

fn scan_function(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
    let vendor = read_config_u16(bus, device, function, 0x0);
    if vendor == 0xFFFF {
        return None;
    }
    let device_id = read_config_u16(bus, device, function, 0x2);
    let prog_if = read_config_u8(bus, device, function, 0x9);
    let subclass = read_config_u8(bus, device, function, 0xA);
    let class_code = read_config_u8(bus, device, function, 0xB);
    let header_type = read_config_u8(bus, device, function, 0xE);
    let subsystem_vendor = read_config_u16(bus, device, function, 0x2C);
    let subsystem_device = read_config_u16(bus, device, function, 0x2E);
    let interrupt_line = read_config_u8(bus, device, function, 0x3C);
    let interrupt_pin = read_config_u8(bus, device, function, 0x3D);

    Some(PciDevice {
        bus,
        device,
        function,
        vendor_id: vendor,
        device_id,
        class_code,
        subclass,
        prog_if,
        header_type,
        subsystem_vendor,
        subsystem_device,
        interrupt_line,
        interrupt_pin,
    })
}

pub fn read_config_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = pci_address(bus, device, function, offset);
    interrupts::without_interrupts(|| unsafe {
        let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data_port = Port::<u32>::new(CONFIG_DATA);
        address_port.write(address);
        data_port.read()
    })
}

pub fn read_config_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = read_config_u32(bus, device, function, offset);
    let shift = (offset & 0x3) * 8;
    ((value >> shift) & 0xFFFF) as u16
}

pub fn read_config_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = read_config_u32(bus, device, function, offset);
    let shift = (offset & 0x3) * 8;
    ((value >> shift) & 0xFF) as u8
}

pub fn write_config_u32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address = pci_address(bus, device, function, offset);
    interrupts::without_interrupts(|| unsafe {
        let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data_port = Port::<u32>::new(CONFIG_DATA);
        address_port.write(address);
        data_port.write(value);
    })
}

pub fn write_config_u16(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    let mut reg = read_config_u32(bus, device, function, offset);
    let shift = (offset & 0x3) * 8;
    reg &= !(0xFFFFu32 << shift);
    reg |= (value as u32) << shift;
    write_config_u32(bus, device, function, offset, reg);
}

pub fn read_bar0(bus: u8, device: u8, function: u8) -> u32 {
    read_config_u32(bus, device, function, 0x10)
}

pub fn enable_bus_master(bus: u8, device: u8, function: u8) {
    let cmd = read_config_u16(bus, device, function, 0x04);
    let new_cmd = cmd | (1 << 2) | (1 << 1);
    if new_cmd != cmd {
        write_config_u16(bus, device, function, 0x04, new_cmd);
    }
}

fn pci_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    ENABLE_BIT
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

fn pci_location(bus: u8, device: u8, function: u8) -> u32 {
    ((bus as u32) << 16) | ((device as u32) << 8) | (function as u32)
}

fn kind_for_class(class_code: u8) -> DeviceKind {
    match class_code {
        0x01 => DeviceKind::Block,
        0x02 => DeviceKind::Net,
        0x03 => DeviceKind::Display,
        _ => DeviceKind::Misc,
    }
}
