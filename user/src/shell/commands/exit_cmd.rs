use super::Command;
use user_lib::sys_exit;

const COMMAND_ABOUT: &str = "exit\n\nExit current process.\nUsage: exit";

pub const CMD: Command = Command {
    name: b"exit",
    usage: "exit",
    desc: "Exit current process",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(_args: &[&[u8]]) {
    sys_exit(0);
}
