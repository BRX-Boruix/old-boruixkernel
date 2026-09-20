use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Lazy;
use x86_64::instructions::port::Port;

use super::device::{Device, DeviceKind, IoDevice};
use kernel_task::waitqueue::WaitQueue;

const BUF_SIZE: usize = 256;

static mut BUF: [u8; BUF_SIZE] = [0; BUF_SIZE];
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);
static SHIFT: AtomicBool = AtomicBool::new(false);
static CAPS_LOCK: AtomicBool = AtomicBool::new(false);
static CTRL: AtomicBool = AtomicBool::new(false);
#[allow(dead_code)]
static DROPPED: AtomicUsize = AtomicUsize::new(0);
static IRQ_INPUT_ACTIVE: AtomicBool = AtomicBool::new(false);
static FALLBACK_ACTIVE: AtomicBool = AtomicBool::new(false);
static FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);
const NO_LAST_MAKE: usize = 0x1ff;
static LAST_MAKE: AtomicUsize = AtomicUsize::new(NO_LAST_MAKE);

pub struct KeyboardDevice {
    waitq: WaitQueue,
}

impl KeyboardDevice {
    pub fn new() -> Self {
        Self {
            waitq: WaitQueue::new(),
        }
    }

    pub fn read_nonblock(&self, out: &mut [u8]) -> usize {
        read_bytes(out)
    }
}

impl Device for KeyboardDevice {
    fn name(&self) -> &'static str {
        "ps2-keyboard"
    }

    fn kind(&self) -> DeviceKind {
        DeviceKind::Char
    }
}

static KEYBOARD_DEVICE: Lazy<KeyboardDevice> = Lazy::new(|| KeyboardDevice::new());

pub fn keyboard_device() -> &'static KeyboardDevice {
    &KEYBOARD_DEVICE
}

pub fn wake_readers() {
    KEYBOARD_DEVICE.waitq.wake_all();
}

pub fn init_controller() {
    unsafe {
        let mut status = Port::<u8>::new(0x64);
        let mut data = Port::<u8>::new(0x60);

        wait_input_clear(&mut status);
        status.write(0xAD); // disable keyboard

        flush_output(&mut status, &mut data);

        wait_input_clear(&mut status);
        status.write(0xAA);
        let _self_test = wait_output_full_timeout(&mut status, &mut data, 1_000_000);

        wait_input_clear(&mut status);
        status.write(0xAB);
        let _port_test = wait_output_full_timeout(&mut status, &mut data, 1_000_000);

        wait_input_clear(&mut status);
        status.write(0x20);
        let mut cfg = wait_output_full(&mut status, &mut data);
        cfg |= 0x01; // IRQ1
        cfg &= !0x10; // enable keyboard clock
        cfg |= 0x40; // force Set1 translation for current decoder table
        wait_input_clear(&mut status);
        status.write(0x60);
        wait_input_clear(&mut status);
        data.write(cfg);

        wait_input_clear(&mut status);
        status.write(0xAE); // enable keyboard

        wait_input_clear(&mut status);
        data.write(0xFF); // reset
        let _ack = wait_output_full_timeout(&mut status, &mut data, 1_000_000);
        let _bat = wait_output_full_timeout(&mut status, &mut data, 1_000_000);
        logger::println!("[KBD][INIT] reset ack={:?} bat={:?}", _ack, _bat);

        wait_input_clear(&mut status);
        data.write(0xF4); // enable scanning
        let scan_ack = wait_output_full_timeout(&mut status, &mut data, 1_000_000);
        logger::println!("[KBD][INIT] scan-enable ack={:?}", scan_ack);
        // Drain any remaining controller/device responses before handing
        // input bytes to the runtime ring buffer.
        flush_output(&mut status, &mut data);
    }
}

unsafe fn wait_input_clear(status: &mut x86_64::instructions::port::Port<u8>) {
    while status.read() & 0x02 != 0 {}
}

unsafe fn wait_output_full(
    status: &mut x86_64::instructions::port::Port<u8>,
    data: &mut x86_64::instructions::port::Port<u8>,
) -> u8 {
    while status.read() & 0x01 == 0 {}
    data.read()
}

unsafe fn wait_output_full_timeout(
    status: &mut x86_64::instructions::port::Port<u8>,
    data: &mut x86_64::instructions::port::Port<u8>,
    limit: usize,
) -> Option<u8> {
    let mut i = 0;
    while i < limit {
        if status.read() & 0x01 != 0 {
            return Some(data.read());
        }
        i += 1;
    }
    None
}

unsafe fn flush_output(
    status: &mut x86_64::instructions::port::Port<u8>,
    data: &mut x86_64::instructions::port::Port<u8>,
) {
    while status.read() & 0x01 != 0 {
        let _ = data.read();
    }
}

pub fn irq_scancode(scancode: u8) {
    if scancode != 0xFA {
        IRQ_INPUT_ACTIVE.store(true, Ordering::Relaxed);
    }
    FALLBACK_ACTIVE.store(false, Ordering::Relaxed);
    FALLBACK_LOGGED.store(false, Ordering::Relaxed);
    feed_scancode(scancode, true);
}

pub fn dropped_count() -> usize {
    DROPPED.load(Ordering::Relaxed)
}

pub fn irq_input_active() -> bool {
    IRQ_INPUT_ACTIVE.load(Ordering::Relaxed)
}

pub fn poll_hardware_nonblock(out: &mut [u8]) -> usize {
    // Fallback when IRQ delivery is broken: poll controller output buffer.
    unsafe {
        let mut status = Port::<u8>::new(0x64);
        let mut data = Port::<u8>::new(0x60);
        while status.read() & 0x01 != 0 {
            let sc = data.read();
            if !FALLBACK_ACTIVE.swap(true, Ordering::Relaxed) {
                if !FALLBACK_LOGGED.swap(true, Ordering::Relaxed) {
                    logger::warn!("[KBD] IRQ inactive, switching to polling fallback");
                }
            }
            feed_scancode(sc, false);
        }
    }
    read_bytes(out)
}

fn feed_scancode(scancode: u8, wake: bool) {
    // Ignore controller/device ACK response bytes.
    if scancode == 0xFA {
        return;
    }
    push_scancode(scancode);
    if wake {
        KEYBOARD_DEVICE.waitq.wake_one();
    }
}

fn push_scancode(scancode: u8) {
    let head = HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % BUF_SIZE;
    let tail = TAIL.load(Ordering::Acquire);
    if next == tail {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    unsafe {
        BUF[head] = scancode;
    }
    HEAD.store(next, Ordering::Release);
}

fn pop_scancode() -> Option<u8> {
    let tail = TAIL.load(Ordering::Relaxed);
    let head = HEAD.load(Ordering::Acquire);
    if tail == head {
        return None;
    }
    let b = unsafe { BUF[tail] };
    let next = (tail + 1) % BUF_SIZE;
    TAIL.store(next, Ordering::Release);
    Some(b)
}

fn read_bytes(out: &mut [u8]) -> usize {
    let mut count = 0;
    let mut saw_release_prefix = false;
    while count < out.len() {
        let sc = match pop_scancode() {
            Some(s) => s,
            None => break,
        };
        // Ignore extended-prefix bytes in the current minimal decoder.
        if sc == 0xE0 || sc == 0xE1 {
            continue;
        }
        // Handle explicit release (Set 1) and Set 2 `0xF0` prefix
        if saw_release_prefix {
            saw_release_prefix = false;
            handle_key_release(sc);
            continue;
        }
        if sc == 0xF0 {
            saw_release_prefix = true;
            continue;
        }

        if sc & 0x80 != 0 {
            handle_key_release(sc & 0x7F);
            continue;
        }
        // Debounce duplicated make codes that can appear when fallback polling
        // and IRQ delivery overlap. This avoids sticky-key failure modes.
        if LAST_MAKE.load(Ordering::Relaxed) == sc as usize {
            continue;
        }
        LAST_MAKE.store(sc as usize, Ordering::Relaxed);
        // Handle shift press
        if sc == 0x2A || sc == 0x36 {
            SHIFT.store(true, Ordering::Relaxed);
            continue;
        }
        if sc == 0x3A {
            CAPS_LOCK.store(!CAPS_LOCK.load(Ordering::Relaxed), Ordering::Relaxed);
            continue;
        }
        // Handle left control press.
        if sc == 0x1D {
            CTRL.store(true, Ordering::Relaxed);
            continue;
        }
        // Ctrl+R hotkey: recover decoder/buffer state without reboot.
        if sc == 0x13 && CTRL.load(Ordering::Relaxed) {
            reset_input_state();
            logger::println!("[KBD] Ctrl+R recovery");
            continue;
        }
        let shift_active = SHIFT.load(Ordering::Relaxed);
        let caps_active = CAPS_LOCK.load(Ordering::Relaxed);
        if let Some(ch) = scancode_to_ascii(sc, shift_active, caps_active) {
            out[count] = ch;
            count += 1;
        }
    }
    count
}

fn reset_input_state() {
    SHIFT.store(false, Ordering::Relaxed);
    CTRL.store(false, Ordering::Relaxed);
    LAST_MAKE.store(NO_LAST_MAKE, Ordering::Relaxed);
    let head = HEAD.load(Ordering::Acquire);
    TAIL.store(head, Ordering::Release);
}

fn handle_key_release(code: u8) {
    if LAST_MAKE.load(Ordering::Relaxed) == code as usize {
        LAST_MAKE.store(NO_LAST_MAKE, Ordering::Relaxed);
    }
    if code == 0x2A || code == 0x36 {
        SHIFT.store(false, Ordering::Relaxed);
    }
    if code == 0x1D {
        CTRL.store(false, Ordering::Relaxed);
    }
}

impl IoDevice for KeyboardDevice {
    fn read(&self, out: &mut [u8]) -> usize {
        loop {
            let n = read_bytes(out);
            if n > 0 {
                return n;
            }
            self.waitq.wait_while(|| {
                let tail = TAIL.load(Ordering::Relaxed);
                let head = HEAD.load(Ordering::Acquire);
                tail == head
            });
        }
    }

    fn poll(&self) -> bool {
        let tail = TAIL.load(Ordering::Relaxed);
        let head = HEAD.load(Ordering::Acquire);
        tail != head
    }
}

impl super::device::DeviceOps for KeyboardDevice {}

fn scancode_to_ascii(sc: u8, shift: bool, caps: bool) -> Option<u8> {
    if let Some(letter) = scancode_letter(sc, caps) {
        return Some(letter);
    }
    // Set 1, with shift
    let ch = match (sc, shift) {
        (0x02, false) => b'1',
        (0x03, false) => b'2',
        (0x04, false) => b'3',
        (0x05, false) => b'4',
        (0x06, false) => b'5',
        (0x07, false) => b'6',
        (0x08, false) => b'7',
        (0x09, false) => b'8',
        (0x0A, false) => b'9',
        (0x0B, false) => b'0',
        (0x02, true) => b'!',
        (0x03, true) => b'@',
        (0x04, true) => b'#',
        (0x05, true) => b'$',
        (0x06, true) => b'%',
        (0x07, true) => b'^',
        (0x08, true) => b'&',
        (0x09, true) => b'*',
        (0x0A, true) => b'(',
        (0x0B, true) => b')',
        (0x39, _) => b' ',
        (0x1C, _) => b'\n',
        (0x0E, _) => 0x08, // backspace
        (0x0C, false) => b'-',
        (0x0C, true) => b'_',
        (0x0D, false) => b'=',
        (0x0D, true) => b'+',
        (0x1A, false) => b'[',
        (0x1A, true) => b'{',
        (0x1B, false) => b']',
        (0x1B, true) => b'}',
        (0x27, false) => b';',
        (0x27, true) => b':',
        (0x28, false) => b'\'',
        (0x28, true) => b'\"',
        (0x29, false) => b'`',
        (0x29, true) => b'~',
        (0x2B, false) => b'\\',
        (0x2B, true) => b'|',
        (0x33, false) => b',',
        (0x33, true) => b'<',
        (0x34, false) => b'.',
        (0x34, true) => b'>',
        (0x35, false) => b'/',
        (0x35, true) => b'?',
        _ => return None,
    };
    Some(ch)
}

fn scancode_letter(sc: u8, uppercase: bool) -> Option<u8> {
    let letter = match sc {
        0x10 => {
            if uppercase {
                b'Q'
            } else {
                b'q'
            }
        }
        0x11 => {
            if uppercase {
                b'W'
            } else {
                b'w'
            }
        }
        0x12 => {
            if uppercase {
                b'E'
            } else {
                b'e'
            }
        }
        0x13 => {
            if uppercase {
                b'R'
            } else {
                b'r'
            }
        }
        0x14 => {
            if uppercase {
                b'T'
            } else {
                b't'
            }
        }
        0x15 => {
            if uppercase {
                b'Y'
            } else {
                b'y'
            }
        }
        0x16 => {
            if uppercase {
                b'U'
            } else {
                b'u'
            }
        }
        0x17 => {
            if uppercase {
                b'I'
            } else {
                b'i'
            }
        }
        0x18 => {
            if uppercase {
                b'O'
            } else {
                b'o'
            }
        }
        0x19 => {
            if uppercase {
                b'P'
            } else {
                b'p'
            }
        }
        0x1E => {
            if uppercase {
                b'A'
            } else {
                b'a'
            }
        }
        0x1F => {
            if uppercase {
                b'S'
            } else {
                b's'
            }
        }
        0x20 => {
            if uppercase {
                b'D'
            } else {
                b'd'
            }
        }
        0x21 => {
            if uppercase {
                b'F'
            } else {
                b'f'
            }
        }
        0x22 => {
            if uppercase {
                b'G'
            } else {
                b'g'
            }
        }
        0x23 => {
            if uppercase {
                b'H'
            } else {
                b'h'
            }
        }
        0x24 => {
            if uppercase {
                b'J'
            } else {
                b'j'
            }
        }
        0x25 => {
            if uppercase {
                b'K'
            } else {
                b'k'
            }
        }
        0x26 => {
            if uppercase {
                b'L'
            } else {
                b'l'
            }
        }
        0x2C => {
            if uppercase {
                b'Z'
            } else {
                b'z'
            }
        }
        0x2D => {
            if uppercase {
                b'X'
            } else {
                b'x'
            }
        }
        0x2E => {
            if uppercase {
                b'C'
            } else {
                b'c'
            }
        }
        0x2F => {
            if uppercase {
                b'V'
            } else {
                b'v'
            }
        }
        0x30 => {
            if uppercase {
                b'B'
            } else {
                b'b'
            }
        }
        0x31 => {
            if uppercase {
                b'N'
            } else {
                b'n'
            }
        }
        0x32 => {
            if uppercase {
                b'M'
            } else {
                b'm'
            }
        }
        _ => return None,
    };
    Some(letter)
}
