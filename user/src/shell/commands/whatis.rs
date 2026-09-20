use super::{all_commands, Command};
use user_lib::sys_write;

const COMMAND_ABOUT: &str =
    "whatis\n\nShow detailed help text for a command.\nUsage: whatis <name>";

pub const CMD: Command = Command {
    name: b"whatis",
    usage: "whatis <name>",
    desc: "Show command help text",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        let msg = b"usage: whatis <name>\n";
        let _ = sys_write(3, msg.as_ptr(), msg.len());
        return;
    }
    let name = args[1];
    for cmd in all_commands() {
        if cmd.name == name {
            let _ = sys_write(3, cmd.about.as_ptr(), cmd.about.len());
            let _ = sys_write(3, b"\n".as_ptr(), 1);
            return;
        }
    }
    let msg = b"whatis: command not found\n";
    let _ = sys_write(3, msg.as_ptr(), msg.len());
}
