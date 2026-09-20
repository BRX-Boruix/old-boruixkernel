#![no_std]
#![no_main]

use user_lib::sys_write;

#[no_mangle]
fn main() -> i32 {
    println3("ipi: start (stub)");
    println3("ipi: no kernel support yet, skipping");
    println3("ipi: done");
    0
}

fn println3(s: &str) {
    let _ = sys_write(3, s.as_ptr(), s.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}
