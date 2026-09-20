use alloc::boxed::Box;
use core::fmt;
use spin::Mutex;

use flanterm_rust::{flanterm_fb_init, flanterm_write, FlantermContext};

struct FlantermContextHandle(Box<FlantermContext>);

unsafe impl Send for FlantermContextHandle {}
unsafe impl Sync for FlantermContextHandle {}

static WRITER: Mutex<Option<FlantermContextHandle>> = Mutex::new(None);

pub fn init(
    framebuffer: *mut u32,
    width: usize,
    height: usize,
    pitch: usize,
    red_mask_size: u8,
    red_mask_shift: u8,
    green_mask_size: u8,
    green_mask_shift: u8,
    blue_mask_size: u8,
    blue_mask_shift: u8,
) {
    let ctx = unsafe {
        flanterm_fb_init(
            framebuffer,
            width,
            height,
            pitch,
            red_mask_size,
            red_mask_shift,
            green_mask_size,
            green_mask_shift,
            blue_mask_size,
            blue_mask_shift,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            0,
            0,
            0,
            1,
            1,
            0,
            0,
        )
    };

    if let Some(ctx) = ctx {
        *WRITER.lock() = Some(FlantermContextHandle(ctx));
    }
}

pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    let mut writer = TerminalWriter;
    let _ = writer.write_fmt(args);
}

pub fn command_once() {
    if let Some(ctx) = WRITER.lock().as_mut() {
        let msg = b"[flanterm] command invoked\r\n";
        flanterm_write(&mut ctx.0, msg);
    }
}

pub fn write_bytes(bytes: &[u8]) {
    if let Some(ctx) = WRITER.lock().as_mut() {
        let mut start = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                if i > start {
                    flanterm_write(&mut ctx.0, &bytes[start..i]);
                }
                flanterm_write(&mut ctx.0, b"\r");
                flanterm_write(&mut ctx.0, b"\n");
                start = i + 1;
            }
        }
        if start < bytes.len() {
            flanterm_write(&mut ctx.0, &bytes[start..]);
        }
    }
}

struct TerminalWriter;

impl fmt::Write for TerminalWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if let Some(ctx) = WRITER.lock().as_mut() {
            let mut start = 0;
            for (i, byte) in s.bytes().enumerate() {
                if byte == b'\n' {
                    // Write text before newline
                    if i > start {
                        flanterm_write(&mut ctx.0, s[start..i].as_bytes());
                    }
                    // Write CR
                    flanterm_write(&mut ctx.0, b"\r");
                    // Write LF
                    flanterm_write(&mut ctx.0, b"\n");
                    start = i + 1;
                }
            }
            // Write remaining text
            if start < s.len() {
                flanterm_write(&mut ctx.0, s[start..].as_bytes());
            }
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! term_print {
    ($($arg:tt)*) => ($crate::driver_hub::terminal::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! term_println {
    () => ($crate::term_print!("\n"));
    ($($arg:tt)*) => ($crate::term_print!("{}\n", format_args!($($arg)*)));
}
