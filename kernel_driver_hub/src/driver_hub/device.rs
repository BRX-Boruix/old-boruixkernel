use super::keyboard::{keyboard_device, KeyboardDevice};

pub trait IoDevice {
    fn read(&self, _out: &mut [u8]) -> usize {
        0
    }
    fn write(&self, _data: &[u8]) -> usize {
        0
    }
    fn read_at(&self, _offset: u64, out: &mut [u8]) -> usize {
        self.read(out)
    }
    fn write_at(&self, _offset: u64, data: &[u8]) -> usize {
        self.write(data)
    }
    fn poll(&self) -> bool {
        false
    }
    fn ioctl(&self, _cmd: usize, _arg: usize) -> isize {
        -1
    }
    fn size(&self) -> Option<u64> {
        None
    }
}

pub trait CharDevice: IoDevice {}
impl<T: IoDevice + ?Sized> CharDevice for T {}

pub trait InputDevice: CharDevice {}
impl<T: CharDevice + ?Sized> InputDevice for T {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Char,
    Block,
    Net,
    Display,
    Misc,
}

pub trait Device {
    fn name(&self) -> &'static str;
    fn kind(&self) -> DeviceKind;
}

pub trait DeviceOps: Device + IoDevice + Sync {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusType {
    Platform,
    Pci,
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub struct DeviceInfo {
    pub name: &'static str,
    pub kind: DeviceKind,
    pub bus: BusType,
    pub location: u32,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

pub fn stdin_device() -> &'static dyn InputDevice {
    keyboard_device()
}

#[allow(dead_code)]
pub fn keyboard() -> &'static KeyboardDevice {
    keyboard_device()
}
