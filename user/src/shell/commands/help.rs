use super::{Command, all_commands};
use user_lib::sys_write;

const COMMAND_ABOUT: &str = "help\n\nShow the command list.\nUsage: help";

pub const CMD: Command = Command {
    name: b"help",
    usage: "help",
    desc: "Show command list",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(_args: &[&[u8]]) {
    let list = all_commands();
    for cmd in list {
        let _ = sys_write(3, cmd.name.as_ptr(), cmd.name.len());
        let _ = sys_write(3, " ".as_ptr(), 1);
        let _ = sys_write(3, cmd.usage.as_ptr(), cmd.usage.len());
        let _ = sys_write(3, " - ".as_ptr(), 3);
        let _ = sys_write(3, cmd.desc.as_ptr(), cmd.desc.len());
        let _ = sys_write(3, "\n".as_ptr(), 1);
    }
}
