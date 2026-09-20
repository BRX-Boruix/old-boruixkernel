use super::Command;
use user_lib::{sys_migrate_one, sys_rmapcount, sys_write, print};

const COMMAND_ABOUT: &str =
    "mmmigrate\n\nMigrate user pages (experimental).\nUsage: mmmigrate [count]";

pub const CMD: Command = Command {
    name: b"mmmigrate",
    usage: "mmmigrate [count]",
    desc: "Migrate user pages (experimental)",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    let mut count: usize = 1;
    if args.len() >= 2 {
        if let Ok(s) = core::str::from_utf8(args[1]) {
            if let Ok(v) = s.trim().parse::<usize>() {
                if v > 0 {
                    count = v;
                }
            }
        }
    }

    let before = sys_rmapcount();
    let mut ok = 0usize;
    for i in 0..count {
        let rc = sys_migrate_one();
        if rc > 0 {
            ok += 1;
            let _ = sys_write(1, b"mmmigrate: ok\n".as_ptr(), 14);
        } else {
            let _ = sys_write(1, b"mmmigrate: none\n".as_ptr(), 16);
            break;
        }
        let _ = i;
    }

    let after = sys_rmapcount();
    print(format_args!("mmmigrate: rmap_before={} rmap_after={}\n", before, after));
    if count > 1 {
        print(format_args!("mmmigrate: migrated={}/{}\n", ok, count));
    }
}
