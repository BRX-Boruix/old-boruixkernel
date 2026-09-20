use super::device::{Device, DeviceKind, DeviceOps, IoDevice};
use super::keyboard::keyboard_device;
use super::terminal;

pub struct NullDevice;

impl Device for NullDevice {
    fn name(&self) -> &'static str {
        "null"
    }

    fn kind(&self) -> DeviceKind {
        DeviceKind::Char
    }
}

impl IoDevice for NullDevice {
    fn read(&self, _out: &mut [u8]) -> usize {
        0
    }

    fn write(&self, data: &[u8]) -> usize {
        data.len()
    }
}

impl DeviceOps for NullDevice {}

pub struct ZeroDevice;

impl Device for ZeroDevice {
    fn name(&self) -> &'static str {
        "zero"
    }

    fn kind(&self) -> DeviceKind {
        DeviceKind::Char
    }
}

impl IoDevice for ZeroDevice {
    fn read(&self, out: &mut [u8]) -> usize {
        for b in out.iter_mut() {
            *b = 0;
        }
        out.len()
    }

    fn write(&self, data: &[u8]) -> usize {
        data.len()
    }
}

impl DeviceOps for ZeroDevice {}

pub struct TtyDevice;

impl Device for TtyDevice {
    fn name(&self) -> &'static str {
        "tty"
    }

    fn kind(&self) -> DeviceKind {
        DeviceKind::Char
    }
}

impl IoDevice for TtyDevice {
    fn read(&self, out: &mut [u8]) -> usize {
        keyboard_device().read(out)
    }

    fn write(&self, data: &[u8]) -> usize {
        terminal::write_bytes(data);
        data.len()
    }

    fn poll(&self) -> bool {
        keyboard_device().poll()
    }
}

impl DeviceOps for TtyDevice {}

pub fn register_pseudo_devices() {
    static NULL: NullDevice = NullDevice;
    static ZERO: ZeroDevice = ZeroDevice;
    static TTY: TtyDevice = TtyDevice;

    super::register_device(&NULL);
    super::register_device(&ZERO);
    super::register_device(&TTY);
}
