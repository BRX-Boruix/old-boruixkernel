#![no_std]

use core::fmt;
use spin::Mutex;
use x86_64::instructions::port::Port;

pub struct SerialPort {
    data: Port<u8>,
    int_en: Port<u8>,
    fifo_ctrl: Port<u8>,
    line_ctrl: Port<u8>,
    modem_ctrl: Port<u8>,
    line_sts: Port<u8>,
}

impl SerialPort {
    /// Creates a new serial port interface on the given base port.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the given base address is valid and not used elsewhere.
    pub const unsafe fn new(base: u16) -> Self {
        Self {
            data: Port::new(base),
            int_en: Port::new(base + 1),
            fifo_ctrl: Port::new(base + 2),
            line_ctrl: Port::new(base + 3),
            modem_ctrl: Port::new(base + 4),
            line_sts: Port::new(base + 5),
        }
    }

    /// Initializes the serial port.
    pub fn init(&mut self) {
        unsafe {
            self.int_en.write(0x00); // Disable all interrupts
            self.line_ctrl.write(0x80); // Enable DLAB (set baud rate divisor)
            self.data.write(0x03); // Set divisor to 3 (lo byte) 38400 baud
            self.int_en.write(0x00); //                  (hi byte)
            self.line_ctrl.write(0x03); // 8 bits, no parity, one stop bit
            self.fifo_ctrl.write(0xC7); // Enable FIFO, clear them, with 14-byte threshold
            self.modem_ctrl.write(0x0B); // IRQs enabled, RTS/DSR set
        }
    }

    /// Sends a byte to the serial port.
    pub fn send(&mut self, data: u8) {
        unsafe {
            // Wait for Transmit Holding Register to be empty (Bit 5 of LSR)
            while self.line_sts.read() & 0x20 == 0 {}
            self.data.write(data);
        }
    }

    /// Receives a byte from the serial port if available.
    pub fn recv_nonblock(&mut self) -> Option<u8> {
        unsafe {
            if self.line_sts.read() & 0x01 != 0 {
                Some(self.data.read())
            } else {
                None
            }
        }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.send(byte);
        }
        Ok(())
    }
}

// COM1 base address
pub const COM1_BASE: u16 = 0x3F8;

pub static SERIAL1: Mutex<SerialPort> = Mutex::new(unsafe { SerialPort::new(COM1_BASE) });

pub static EXTRA_WRITER: Mutex<Option<fn(fmt::Arguments)>> = Mutex::new(None);
pub static EXTRA_WRITER_BYTES: Mutex<Option<fn(&[u8])>> = Mutex::new(None);

pub fn set_extra_writer(writer: fn(fmt::Arguments)) {
    *EXTRA_WRITER.lock() = Some(writer);
}

pub fn set_extra_writer_bytes(writer: fn(&[u8])) {
    *EXTRA_WRITER_BYTES.lock() = Some(writer);
}

pub fn clear_extra_writer() {
    *EXTRA_WRITER.lock() = None;
}

pub fn clear_extra_writer_bytes() {
    *EXTRA_WRITER_BYTES.lock() = None;
}

/// 强制解锁串口（不安全）
///
/// 用于 Panic 或 Double Fault 等极端情况，防止死锁。
/// 调用此函数后，如果串口正被其他线程占用，可能会导致输出混乱，但能保证信息被打印。
pub unsafe fn force_unlock() {
    SERIAL1.force_unlock();
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    // 禁用中断以防止死锁
    interrupts::without_interrupts(|| {
        {
            let mut serial = SERIAL1.lock();
            let _ = serial.write_fmt(args);
        }

        if let Some(writer) = *EXTRA_WRITER.lock() {
            writer(args);
        }
    });
}

pub fn write_bytes(bytes: &[u8]) {
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        {
            let mut serial = SERIAL1.lock();
            for &b in bytes {
                serial.send(b);
            }
        }
        if let Some(writer) = *EXTRA_WRITER_BYTES.lock() {
            writer(bytes);
        }
    });
}

pub fn read_byte_nonblock() -> Option<u8> {
    SERIAL1.lock().recv_nonblock()
}

/// Initializes the serial port.
pub fn init() {
    SERIAL1.lock().init();
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
