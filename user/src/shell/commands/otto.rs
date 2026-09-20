use super::Command;
use user_lib::{otto_toggle, sys_write};

const COMMAND_ABOUT: &str = "otto\n\nToggle fd=3 output mirroring to serial.\nUsage: otto";

pub const CMD: Command = Command {
    name: b"otto",
    usage: "otto",
    desc: "Mirror fd=3 output to serial",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(_args: &[&[u8]]) {
    let enabled = otto_toggle();
    if enabled {
        let _ = sys_write(3, "otto: serial mirror ON\n".as_ptr(), 23);
    } else {
        let _ = sys_write(3, "otto: serial mirror OFF\n".as_ptr(), 24);
    }
}
