use super::Command;
use user_lib::sys_write;

const COMMAND_ABOUT: &str = "echo\n\nEcho text to the console.\nUsage: echo <text>";

pub const CMD: Command = Command {
    name: b"echo",
    usage: "echo <text>",
    desc: "Echo text",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        let _ = sys_write(3, "\n".as_ptr(), 1);
        return;
    }
    let text = args[1];
    let _ = sys_write(3, text.as_ptr(), text.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}
