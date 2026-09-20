#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use user_lib::{sys_get_exec_arg, sys_write, sys_yield, sbrk_stats, sys_exit};

#[no_mangle]
fn main() -> i32 {
    let arg = sys_get_exec_arg();
    let mut mb = if arg > 0 { arg as usize } else { 32 };
    let mut chunk = 64 * 1024;
    if arg as usize > 0xFFFF {
        mb = ((arg as usize) >> 16) & 0xFFFF;
        let ck = (arg as usize) & 0xFFFF;
        if ck > 0 {
            chunk = ck * 1024;
        }
    }
    if mb == 0 {
        mb = 32;
    }
    let total = mb * 1024 * 1024;
    let n = total / chunk;

    print_str("memstress: start\n");
    print_num("memstress: total_mb=", mb as isize);
    print_num("memstress: chunk_kb=", (chunk / 1024) as isize);
    print_num("memstress: blocks=", n as isize);

    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut v = vec![0u8; chunk];
        v[0] = (i & 0xff) as u8;
        blocks.push(v);
        if i % 128 == 0 {
            sys_yield();
        }
    }

    let mut kept: Vec<Vec<u8>> = Vec::with_capacity((n + 1) / 2);
    for (i, b) in blocks.into_iter().enumerate() {
        if i % 2 == 0 {
            kept.push(b);
        }
    }
    print_str("memstress: fragmented\n");

    let small = 4 * 1024;
    let mut smalls: Vec<Vec<u8>> = Vec::new();
    let target_small = total / small / 2;
    for i in 0..target_small {
        smalls.push(vec![1u8; small]);
        if i % 256 == 0 {
            sys_yield();
        }
    }
    print_str("memstress: filled\n");

    drop(smalls);
    drop(kept);
    print_str("memstress: done\n");

    let (count, bytes) = sbrk_stats();
    print_num("memstress: sbrk_count=", count as isize);
    print_num("memstress: sbrk_bytes=", bytes as isize);

    sys_exit(0);
}

fn print_str(s: &str) {
    let _ = sys_write(3, s.as_ptr(), s.len());
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
