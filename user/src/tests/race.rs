#![no_std]
#![no_main]

use user_lib::{sys_write, sys_yield};

#[no_mangle]
fn main() -> i32 {
    println3("race: start");
    const TOTAL: usize = 200_000;
    const STEP: usize = 10_000;
    let mut i = 0usize;
    while i < TOTAL {
        if i % STEP == 0 {
            let n = i / STEP;
            print_num("race: progress ", n as isize);
        }
        let _ = sys_yield();
        i += 1;
    }
    println3("race: done");
    0
}

fn println3(s: &str) {
    let _ = sys_write(3, s.as_ptr(), s.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}

fn print_num(prefix: &str, n: isize) {
    let _ = sys_write(3, prefix.as_ptr(), prefix.len());
    let mut buf = [0u8; 24];
    let mut i = 0usize;
    let mut x = if n < 0 { -n } else { n } as usize;
    if n < 0 {
        buf[i] = b'-';
        i += 1;
    }
    if x == 0 {
        buf[i] = b'0';
        i += 1;
    } else {
        let mut tmp = [0u8; 20];
        let mut t = 0usize;
        while x > 0 {
            tmp[t] = b'0' + (x % 10) as u8;
            t += 1;
            x /= 10;
        }
        while t > 0 {
            t -= 1;
            buf[i] = tmp[t];
            i += 1;
        }
    }
    let _ = sys_write(3, buf.as_ptr(), i);
    let _ = sys_write(3, "\n".as_ptr(), 1);
}
