use super::Command;
use user_lib::{sys_testmm1, sys_write};

const COMMAND_ABOUT: &str = "testmm1\n\nRun memory manager test #1 in kernel.\nUsage: testmm1";

pub const CMD: Command = Command {
    name: b"testmm1",
    usage: "testmm1",
    desc: "Run mm test #1 in kernel",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(_args: &[&[u8]]) {
    let r = sys_testmm1();
    if r == 0 {
        println3("testmm1: OK (kernel reported PASS)");
    } else {
        println3("testmm1: FAIL (kernel reported FAIL)");
    }
}

fn println3(s: &str) {
    let _ = sys_write(3, s.as_ptr(), s.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}
