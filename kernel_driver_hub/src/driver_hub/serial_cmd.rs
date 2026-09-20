use core::str;
use serial;

const CMD_BUF_SIZE: usize = 64;

pub fn poll() {
    static mut BUFFER: [u8; CMD_BUF_SIZE] = [0; CMD_BUF_SIZE];
    static mut LEN: usize = 0;

    while let Some(byte) = serial::read_byte_nonblock() {
        unsafe {
            match byte {
                b'\r' | b'\n' => {
                    if LEN > 0 {
                        match core::str::from_utf8(&BUFFER[..LEN]) {
                            Ok(cmd) => handle_command(cmd),
                            Err(_) => serial::println!("command parse error"),
                        }
                        LEN = 0;
                    }
                }
                0x08 | 0x7f => {
                    if LEN > 0 {
                        LEN -= 1;
                    }
                }
                b if LEN < CMD_BUF_SIZE - 1 => {
                    BUFFER[LEN] = b;
                    LEN += 1;
                }
                _ => {}
            }
        }
    }
}

fn handle_command(cmd: &str) {
    serial::println!("unknown command: {}", cmd);
}
