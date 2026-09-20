#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

#[no_mangle]
fn main() -> i32 {
    let _pid = user_lib::sys_getpid();
    for _i in 0..5 {
        // println!("Task B (PID={}): iteration {}", pid, i);
        user_lib::sys_yield();
    }
    println!("Task B finished!");
    0
}
