#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

#[path = "../shell/shell.rs"]
mod shell;

use user_lib::{sys_fork, sys_exec, sys_waitpid, sys_yield, sys_exit};

#[no_mangle]
fn main() -> i32 {
    println!("[initproc] Started!");

    // Spawn loop_a
    let pid_a = sys_fork();
    if pid_a == 0 {
        println!("[initproc] Child A execing loop_a...");
        sys_exec("loop_a\0".as_ptr());
        println!("[initproc] Child A exec failed!");
        sys_exit(-1);
    } else {
        println!("[initproc] Spawned loop_a with PID {}", pid_a);
    }

    // Spawn reaper
    let pid_r = sys_fork();
    if pid_r == 0 {
        loop {
            let mut exit_code: i32 = 0;
            let pid = sys_waitpid(-1, &mut exit_code);
            if pid > 0 {
                println!("[reaper] Child PID {} exited with code {}", pid, exit_code);
            } else {
                sys_yield();
            }
        }
    }

    println!("[initproc] Shell starting in initproc.");

    // Spawn loop_b
    let pid_c = sys_fork();
    if pid_c == 0 {
        println!("[initproc] Child C execing loop_b...");
        sys_exec("loop_b\0".as_ptr());
        println!("[initproc] Child C exec failed!");
        sys_exit(-1);
    } else {
         println!("[initproc] Spawned loop_b with PID {}", pid_c);
    }

    shell::run_shell();
}
