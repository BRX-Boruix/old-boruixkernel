use super::Command;
use user_lib::{sys_ps, sys_write};

const COMMAND_ABOUT: &str = "jobs\n\nAlias of ps.\nUsage: jobs";

pub const CMD: Command = Command {
    name: b"jobs",
    usage: "jobs",
    desc: "Alias of ps",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(_args: &[&[u8]]) {
    let mut buf = [0u8; 256];
    let n = sys_ps(buf.as_mut_ptr(), buf.len());
    if n > 0 {
        let _ = sys_write(3, buf.as_ptr(), n as usize);
    }
}
