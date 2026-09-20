use super::Command;
use user_lib::{sys_mmstat, sys_mmstat_reset, sys_write};

const COMMAND_ABOUT: &str =
    "mmstat\n\nShow memory manager statistics (and reset counters).\nUsage: mmstat";

pub const CMD: Command = Command {
    name: b"mmstat",
    usage: "mmstat",
    desc: "Show memory manager stats (mmstat reset)",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() > 1 && args[1] == b"reset" {
        let _ = sys_mmstat_reset();
        return;
    }
    let mut buf = [0u8; 2048];
    let n = sys_mmstat(buf.as_mut_ptr(), buf.len());
    if n <= 0 {
        return;
    }
    let n = n as usize;
    let _ = sys_write(3, buf.as_ptr(), n);
}
