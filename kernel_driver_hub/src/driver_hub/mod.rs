pub mod cmos;
pub mod device;
pub mod e1000;
pub mod keyboard;
pub mod pci;
pub mod pci_devices;
pub mod pseudo;
pub mod ata_pio;
pub mod ramdisk;
pub mod serial_cmd;
pub mod terminal;

use alloc::string::String;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

use device::{BusType, DeviceInfo, DeviceKind, DeviceOps};

pub const NAME_LEN: usize = 32;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DriverInfoRaw {
    pub name: [u8; NAME_LEN],
    pub stage: u8,
    pub has_probe: u8,
    pub has_attach: u8,
    pub _pad: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DeviceInfoRaw {
    pub name: [u8; NAME_LEN],
    pub driver_name: [u8; NAME_LEN],
    pub kind: u8,
    pub bus: u8,
    pub _pad0: [u8; 2],
    pub location: u32,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub _pad1: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DriverStage {
    Early = 0,
    Core = 1,
    Devices = 2,
    Late = 3,
}

pub trait Driver {
    fn name(&self) -> &'static str;
    fn stage(&self) -> DriverStage;
    fn init(&self, hub: &DriverHub);
    fn probe(&self, _hub: &DriverHub, _dev: &DeviceInfo) -> bool {
        false
    }
    fn attach(&self, _hub: &DriverHub, _dev: &DeviceInfo) {}
}

#[derive(Clone, Copy)]
struct DriverEntry {
    name: &'static str,
    stage: DriverStage,
    init: fn(&DriverHub),
    probe: Option<fn(&DriverHub, &DeviceInfo) -> bool>,
    attach: Option<fn(&DriverHub, &DeviceInfo)>,
}

impl Driver for DriverEntry {
    fn name(&self) -> &'static str {
        self.name
    }

    fn stage(&self) -> DriverStage {
        self.stage
    }

    fn init(&self, hub: &DriverHub) {
        (self.init)(hub);
    }

    fn probe(&self, hub: &DriverHub, dev: &DeviceInfo) -> bool {
        match self.probe {
            Some(f) => f(hub, dev),
            None => false,
        }
    }

    fn attach(&self, hub: &DriverHub, dev: &DeviceInfo) {
        if let Some(f) = self.attach {
            f(hub, dev);
        }
    }
}

fn noop(_hub: &DriverHub) {}

impl DriverEntry {
    const EMPTY: DriverEntry = DriverEntry {
        name: "",
        stage: DriverStage::Late,
        init: noop,
        probe: None,
        attach: None,
    };
}

#[derive(Clone, Copy)]
struct DeviceEntry {
    info: DeviceInfo,
    dev: Option<&'static dyn DeviceOps>,
    driver_name: Option<&'static str>,
}

pub struct DriverHub;

const MAX_DRIVERS: usize = 32;
static DRIVER_COUNT: AtomicUsize = AtomicUsize::new(0);
static DRIVERS: Mutex<[DriverEntry; MAX_DRIVERS]> = Mutex::new([DriverEntry::EMPTY; MAX_DRIVERS]);
static REGISTERED: AtomicBool = AtomicBool::new(false);

static DEVICE_COUNT: AtomicUsize = AtomicUsize::new(0);
static DEVICES: Mutex<[Option<DeviceEntry>; MAX_DRIVERS]> = Mutex::new([None; MAX_DRIVERS]);

pub fn register_driver(name: &'static str, stage: DriverStage, init: fn(&DriverHub)) {
    let idx = DRIVER_COUNT.fetch_add(1, Ordering::Relaxed);
    if idx >= MAX_DRIVERS {
        logger::warn!("DriverHub: driver list full, drop {}", name);
        return;
    }
    let mut list = DRIVERS.lock();
    list[idx] = DriverEntry {
        name,
        stage,
        init,
        probe: None,
        attach: None,
    };
}

pub fn register_driver_ops(
    name: &'static str,
    stage: DriverStage,
    init: fn(&DriverHub),
    probe: Option<fn(&DriverHub, &DeviceInfo) -> bool>,
    attach: Option<fn(&DriverHub, &DeviceInfo)>,
) {
    let idx = DRIVER_COUNT.fetch_add(1, Ordering::Relaxed);
    if idx >= MAX_DRIVERS {
        logger::warn!("DriverHub: driver list full, drop {}", name);
        return;
    }
    let mut list = DRIVERS.lock();
    list[idx] = DriverEntry {
        name,
        stage,
        init,
        probe,
        attach,
    };
}

pub fn register_device_info(
    info: DeviceInfo,
    dev: Option<&'static dyn DeviceOps>,
    driver_name: Option<&'static str>,
) {
    let idx = DEVICE_COUNT.fetch_add(1, Ordering::Relaxed);
    if idx >= MAX_DRIVERS {
        logger::warn!("DriverHub: device list full, drop {}", info.name);
        return;
    }
    let mut list = DEVICES.lock();
    list[idx] = Some(DeviceEntry {
        info,
        dev,
        driver_name,
    });
}

pub fn register_device(dev: &'static dyn DeviceOps) {
    register_device_info(
        DeviceInfo {
            name: dev.name(),
            kind: dev.kind(),
            bus: BusType::Unknown,
            location: 0,
            vendor_id: 0,
            device_id: 0,
            class_code: 0,
            subclass: 0,
            prog_if: 0,
        },
        Some(dev),
        None,
    );
}

pub fn device_count() -> usize {
    DEVICE_COUNT.load(Ordering::Relaxed)
}

pub fn device_at(index: usize) -> Option<&'static dyn DeviceOps> {
    if index >= DEVICE_COUNT.load(Ordering::Relaxed) {
        return None;
    }
    let list = DEVICES.lock();
    list.get(index).and_then(|e| e.and_then(|entry| entry.dev))
}

pub fn device_info_at(index: usize) -> Option<DeviceInfo> {
    if index >= DEVICE_COUNT.load(Ordering::Relaxed) {
        return None;
    }
    let list = DEVICES.lock();
    list.get(index).and_then(|e| e.map(|entry| entry.info))
}

pub fn device_driver_at(index: usize) -> Option<&'static str> {
    if index >= DEVICE_COUNT.load(Ordering::Relaxed) {
        return None;
    }
    let list = DEVICES.lock();
    list.get(index)
        .and_then(|e| e.as_ref())
        .and_then(|entry| entry.driver_name)
}

pub fn driver_count() -> usize {
    DRIVER_COUNT.load(Ordering::Relaxed)
}

pub fn fill_driver_list(out: &mut [DriverInfoRaw]) -> usize {
    let count = DRIVER_COUNT.load(Ordering::Relaxed);
    let drivers = DRIVERS.lock();
    let total = core::cmp::min(out.len(), count);
    for (slot, drv) in out.iter_mut().zip(drivers.iter().take(total)) {
        *slot = DriverInfoRaw::default();
        copy_name(&mut slot.name, drv.name);
        slot.stage = drv.stage as u8;
        slot.has_probe = drv.probe.is_some() as u8;
        slot.has_attach = drv.attach.is_some() as u8;
    }
    total
}

pub fn fill_device_list(out: &mut [DeviceInfoRaw]) -> usize {
    let count = DEVICE_COUNT.load(Ordering::Relaxed);
    let devices = DEVICES.lock();
    let total = core::cmp::min(out.len(), count);
    for (slot, dev) in out.iter_mut().zip(devices.iter().take(total)) {
        *slot = DeviceInfoRaw::default();
        let Some(entry) = dev else { continue };
        copy_name(&mut slot.name, entry.info.name);
        if let Some(drv) = entry.driver_name {
            copy_name(&mut slot.driver_name, drv);
        }
        slot.kind = entry.info.kind as u8;
        slot.bus = entry.info.bus as u8;
        slot.location = entry.info.location;
        slot.vendor_id = entry.info.vendor_id;
        slot.device_id = entry.info.device_id;
        slot.class_code = entry.info.class_code;
        slot.subclass = entry.info.subclass;
        slot.prog_if = entry.info.prog_if;
    }
    total
}

fn ensure_registered() {
    if REGISTERED.swap(true, Ordering::Relaxed) {
        return;
    }
    // Early: serial output
    register_driver("serial", DriverStage::Early, |_: &DriverHub| serial::init());

    // Core: keyboard controller + IRQ handler hook
    register_driver("keyboard", DriverStage::Core, init_keyboard_driver);
    register_driver("pseudo", DriverStage::Core, init_pseudo_devices);

    // Devices: PCI enumeration (requires allocator)
    register_driver("pci", DriverStage::Devices, |_: &DriverHub| pci::init());

    // Devices: PCI class drivers (binding only, no behavior yet)
    register_driver_ops(
        "pci-block",
        DriverStage::Devices,
        noop,
        Some(probe_pci_block),
        Some(attach_pci_block),
    );
    register_driver_ops(
        "pci-net",
        DriverStage::Devices,
        noop,
        Some(probe_pci_net),
        Some(attach_pci_net),
    );
    register_driver_ops(
        "pci-display",
        DriverStage::Devices,
        noop,
        Some(probe_pci_display),
        Some(attach_pci_display),
    );
}

pub fn init_stage(stage: DriverStage) {
    ensure_registered();
    let count = DRIVER_COUNT.load(Ordering::Relaxed);
    let list = DRIVERS.lock();
    let hub = DriverHub;
    for entry in list.iter().take(count) {
        if entry.stage == stage {
            entry.init(&hub);
            logger::info!("DriverHub: init {}", entry.name());
        }
    }
}

pub fn init_early() {
    init_stage(DriverStage::Early);
}

pub fn init_core() {
    init_stage(DriverStage::Core);
}

pub fn init_devices() {
    init_stage(DriverStage::Devices);
    ata_pio::init_primary();
    attach_all();
}

pub fn init_all() {
    init_early();
    init_core();
    init_devices();
    init_stage(DriverStage::Late);
}

pub fn attach_all() {
    ensure_registered();
    let dev_count = DEVICE_COUNT.load(Ordering::Relaxed);
    let drv_count = DRIVER_COUNT.load(Ordering::Relaxed);
    let hub = DriverHub;
    for idx in 0..dev_count {
        let info = {
            let devices = DEVICES.lock();
            match devices.get(idx).and_then(|e| e.as_ref()) {
                Some(entry) => entry.info,
                None => continue,
            }
        };

        let mut attached: Option<&'static str> = None;
        let drivers = DRIVERS.lock();
        for drv in drivers.iter().take(drv_count) {
            if drv.probe(&hub, &info) {
                if drv.attach.is_some() {
                    drv.attach(&hub, &info);
                    attached = Some(drv.name);
                }
                break;
            }
        }
        drop(drivers);

        if let Some(name) = attached {
            let mut devices = DEVICES.lock();
            if let Some(entry) = devices.get_mut(idx).and_then(|e| e.as_mut()) {
                entry.driver_name = Some(name);
            }
        }
    }
}

fn init_keyboard_driver(_hub: &DriverHub) {
    // Avoid racing with IRQ1 handler while controller init polls port 0x60.
    x86_64::instructions::interrupts::disable();
    keyboard::init_controller();
    kernel_platform::hal::arch::interrupts::set_keyboard_handler(keyboard::irq_scancode);
    x86_64::instructions::interrupts::enable();
    register_device_info(
        DeviceInfo {
            name: "ps2-keyboard",
            kind: DeviceKind::Char,
            bus: BusType::Platform,
            location: 0,
            vendor_id: 0,
            device_id: 0,
            class_code: 0,
            subclass: 0,
            prog_if: 0,
        },
        Some(keyboard::keyboard_device()),
        Some("keyboard"),
    );
}

fn init_pseudo_devices(_hub: &DriverHub) {
    pseudo::register_pseudo_devices();
}

fn probe_pci_block(_hub: &DriverHub, dev: &DeviceInfo) -> bool {
    dev.bus == BusType::Pci && dev.class_code == 0x01
}

fn probe_pci_net(_hub: &DriverHub, dev: &DeviceInfo) -> bool {
    dev.bus == BusType::Pci
        && dev.class_code == 0x02
        && dev.vendor_id == 0x8086
        && (dev.device_id == 0x100e || dev.device_id == 0x100f)
}

fn probe_pci_display(_hub: &DriverHub, dev: &DeviceInfo) -> bool {
    dev.bus == BusType::Pci && dev.class_code == 0x03
}

fn attach_pci_block(_hub: &DriverHub, dev: &DeviceInfo) {
    logger::info!(
        "DriverHub: attach pci-block {:02x}:{:02x}.{:02x} vendor={:04x} device={:04x}",
        ((dev.location >> 16) & 0xff) as u8,
        ((dev.location >> 8) & 0xff) as u8,
        (dev.location & 0xff) as u8,
        dev.vendor_id,
        dev.device_id
    );
    let _ = pci_devices::register_pci_block(*dev).map(|ops| {
        register_device_info(
            DeviceInfo { name: ops.name(), ..*dev },
            Some(ops),
            Some("pci-block"),
        );
    });
}

fn attach_pci_net(_hub: &DriverHub, dev: &DeviceInfo) {
    let bus = ((dev.location >> 16) & 0xff) as u8;
    let device = ((dev.location >> 8) & 0xff) as u8;
    let function = (dev.location & 0xff) as u8;
    if crate::driver_hub::e1000::attach(bus, device, function) {
        logger::info!(
            "DriverHub: attach e1000 {:02x}:{:02x}.{:02x} vendor={:04x} device={:04x}",
            bus,
            device,
            function,
            dev.vendor_id,
            dev.device_id
        );
        register_device_info(
            DeviceInfo {
                name: "net0",
                kind: DeviceKind::Char,
                bus: dev.bus,
                location: dev.location,
                vendor_id: dev.vendor_id,
                device_id: dev.device_id,
                class_code: dev.class_code,
                subclass: dev.subclass,
                prog_if: dev.prog_if,
            },
            Some(crate::driver_hub::e1000::device_ops()),
            Some("e1000"),
        );
    } else {
        let _ = pci_devices::register_pci_net(*dev).map(|ops| {
            register_device_info(
                DeviceInfo { name: ops.name(), ..*dev },
                Some(ops),
                Some("pci-net"),
            );
        });
        logger::warn!(
            "DriverHub: e1000 attach failed {:02x}:{:02x}.{:02x}",
            bus,
            device,
            function
        );
    }
}

fn attach_pci_display(_hub: &DriverHub, dev: &DeviceInfo) {
    logger::info!(
        "DriverHub: attach pci-display {:02x}:{:02x}.{:02x} vendor={:04x} device={:04x}",
        ((dev.location >> 16) & 0xff) as u8,
        ((dev.location >> 8) & 0xff) as u8,
        (dev.location & 0xff) as u8,
        dev.vendor_id,
        dev.device_id
    );
    let _ = pci_devices::register_pci_display(*dev).map(|ops| {
        register_device_info(
            DeviceInfo { name: ops.name(), ..*dev },
            Some(ops),
            Some("pci-display"),
        );
    });
}

const FLAG_DRIVERS: u32 = 1;
const FLAG_DEVICES: u32 = 2;

pub fn summary_string(flags: u32) -> String {
    let show_drivers = flags == 0 || (flags & FLAG_DRIVERS) != 0;
    let show_devices = flags == 0 || (flags & FLAG_DEVICES) != 0;
    let mut out = String::new();

    if show_drivers {
        let count = DRIVER_COUNT.load(Ordering::Relaxed);
        let list = DRIVERS.lock();
        let _ = writeln!(out, "Drivers ({}):", count);
        for (idx, drv) in list.iter().take(count).enumerate() {
            let stage = match drv.stage {
                DriverStage::Early => "Early",
                DriverStage::Core => "Core",
                DriverStage::Devices => "Devices",
                DriverStage::Late => "Late",
            };
            let _ = writeln!(
                out,
                "  [{}] {} stage={} probe={} attach={}",
                idx,
                drv.name,
                stage,
                if drv.probe.is_some() { "yes" } else { "no" },
                if drv.attach.is_some() { "yes" } else { "no" }
            );
        }
        let _ = writeln!(out, "");
    }

    if show_devices {
        let count = DEVICE_COUNT.load(Ordering::Relaxed);
        let list = DEVICES.lock();
        let _ = writeln!(out, "Devices ({}):", count);
        for (idx, dev) in list.iter().take(count).enumerate() {
            let Some(entry) = dev else { continue };
            let kind = match entry.info.kind {
                DeviceKind::Char => "Char",
                DeviceKind::Block => "Block",
                DeviceKind::Net => "Net",
                DeviceKind::Display => "Display",
                DeviceKind::Misc => "Misc",
            };
            let bus = match entry.info.bus {
                BusType::Platform => "Platform",
                BusType::Pci => "PCI",
                BusType::Unknown => "Unknown",
            };
            let driver = entry.driver_name.unwrap_or("-");
            if entry.info.bus == BusType::Pci {
                let bus_id = ((entry.info.location >> 16) & 0xff) as u8;
                let dev_id = ((entry.info.location >> 8) & 0xff) as u8;
                let func_id = (entry.info.location & 0xff) as u8;
                let _ = writeln!(
                    out,
                    "  [{}] {} kind={} bus={} loc={:02x}:{:02x}.{:02x} vendor={:04x} device={:04x} class={:02x} subclass={:02x} prog-if={:02x} driver={}",
                    idx,
                    entry.info.name,
                    kind,
                    bus,
                    bus_id,
                    dev_id,
                    func_id,
                    entry.info.vendor_id,
                    entry.info.device_id,
                    entry.info.class_code,
                    entry.info.subclass,
                    entry.info.prog_if,
                    driver
                );
            } else {
                let _ = writeln!(
                    out,
                    "  [{}] {} kind={} bus={} driver={}",
                    idx, entry.info.name, kind, bus, driver
                );
            }
        }
    }

    out
}

fn copy_name(dst: &mut [u8; NAME_LEN], name: &str) {
    let bytes = name.as_bytes();
    let n = core::cmp::min(bytes.len(), NAME_LEN);
    dst[..n].copy_from_slice(&bytes[..n]);
}
