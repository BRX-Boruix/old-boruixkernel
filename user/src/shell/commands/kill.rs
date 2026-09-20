use super::Command;
use user_lib::{sys_kill, sys_write};

const COMMAND_ABOUT: &str = "kill\n\nTerminate a process by PID.\nUsage: kill <pid>";

pub const CMD: Command = Command {
    name: b"kill",
    usage: "kill <pid>",
    desc: "Kill a process",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        println3("usage: kill <pid>");
        return;
    }
    let pid = parse_int(args[1]);
    if pid <= 0 {
        println3("invalid pid");
        return;
    }
    let r = sys_kill(pid as usize, -1);
    if r == 0 {
        println3("killed");
    } else {
        println3("kill failed");
    }
}

fn parse_int(s: &[u8]) -> isize {
    let mut i = 0usize;
    let mut val: isize = 0;
    while i < s.len() {
        let b = s[i];
        if b < b'0' || b > b'9' {
            return -1;
        }
        val = val * 10 + (b - b'0') as isize;
        i += 1;
    }
    val
}

fn println3(s: &str) {
    let _ = sys_write(3, s.as_ptr(), s.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}
