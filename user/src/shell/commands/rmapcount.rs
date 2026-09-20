use super::Command;
use user_lib::{sys_rmapcount, print};

const COMMAND_ABOUT: &str = "rmapcount\n\nShow current rmap entry count.\nUsage: rmapcount";

pub const CMD: Command = Command {
    name: b"rmapcount",
    usage: "rmapcount",
    desc: "Show current rmap entry count",
    about: COMMAND_ABOUT,
    run,
};

fn run(_args: &[&[u8]]) {
    let n = sys_rmapcount();
    print(format_args!("rmapcount: {}\n", n));
}
