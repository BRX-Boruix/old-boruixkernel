use super::Command;
use user_lib::{sys_ipi_stat, sys_write};

const COMMAND_ABOUT: &str = "ipi_stat\n\nShow IPI statistics.\nUsage: ipi_stat";

pub const CMD: Command = Command {
    name: b"ipi_stat",
    usage: "ipi_stat",
    desc: "Show IPI statistics",
    about: COMMAND_ABOUT,
    run,
};

fn run(_args: &[&[u8]]) {
    let mut buf = [0u8; 256];
    let n = sys_ipi_stat(buf.as_mut_ptr(), buf.len());
    if n <= 0 {
        let _ = sys_write(1, b"ipi_stat: error\n".as_ptr(), 15);
        return;
    }
    let _ = sys_write(1, buf.as_ptr(), n as usize);
}
