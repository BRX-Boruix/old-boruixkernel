#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

#[no_mangle]
fn main() -> i32 {
    println!("I am a rogue task (PID={}), and I will not yield!", user_lib::sys_getpid());
    let mut _i = 0;
    loop {
        _i += 1;
        // if i % 10000000 == 0 {
        //     println!("Rogue task running... i={}", i);
        // }
    }
}
