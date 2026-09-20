use super::Command;
use user_lib::sys_write;

const COMMAND_ABOUT: &str = "clear\n\nClear the screen.\nUsage: clear";

pub const CMD: Command = Command {
    name: b"clear",
    usage: "clear",
    desc: "Clear screen",
    about: COMMAND_ABOUT,
    run,
};

fn run(_args: &[&[u8]]) {
    // ANSI clear screen + cursor home.
    let _ = sys_write(3, b"\x1b[2J\x1b[H".as_ptr(), 7);
}
