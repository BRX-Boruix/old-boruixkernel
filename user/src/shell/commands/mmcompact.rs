use super::Command;
use user_lib::{sys_mmcompact, sys_write};

const COMMAND_ABOUT: &str = "mmcompact\n\nTrigger PMM compaction.\nUsage: mmcompact";

pub const CMD: Command = Command {
    name: b"mmcompact",
    usage: "mmcompact",
    desc: "Trigger PMM compaction",
    about: COMMAND_ABOUT,
    run,
};

fn run(_args: &[&[u8]]) {
    let rc = sys_mmcompact();
    if rc == 0 {
        let _ = sys_write(1, b"mmcompact: ok\n".as_ptr(), 14);
    } else {
        let _ = sys_write(1, b"mmcompact: error\n".as_ptr(), 17);
    }
}
