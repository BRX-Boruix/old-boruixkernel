extern crate alloc;

use alloc::vec::Vec;
use user_lib::sys_plog;

use super::Command;

const COMMAND_ABOUT: &str =
    "plog\n\nLog a message via kernel logger.\nUsage: plog [-a|--all] <type> \"content\"";

pub const CMD: Command = Command {
    name: b"plog",
    usage: "plog [-a|--all] <type> \"content\"",
    desc: "log message via kernel logger",
    about: COMMAND_ABOUT,
    run: run,
};

const PLOG_ALL_FLAG: usize = 1 << 16;

fn run(args: &[&[u8]]) {
    if args.len() < 2 {
        print_help();
        return;
    }

    let mut all = false;
    let mut ty_opt: Option<&[u8]> = None;
    let mut start_quote: Option<usize> = None;
    let mut end_quote: Option<usize> = None;

    for (i, a) in args.iter().enumerate().skip(1) {
        if eq(a, b"-a") || eq(a, b"--all") {
            all = true;
            continue;
        }
        if a.starts_with(b"\"") && start_quote.is_none() {
            start_quote = Some(i);
        }
        if a.ends_with(b"\"") {
            end_quote = Some(i);
        }
        if ty_opt.is_none() && start_quote.is_none() {
            ty_opt = Some(*a);
        }
    }

    let Some(ty) = ty_opt else {
        print_help();
        return;
    };
    if eq(ty, b"get-help") {
        print_types();
        return;
    }

    let mut ty_id = match ty {
        t if eq(t, b"info") => 0,
        t if eq(t, b"warn") => 1,
        t if eq(t, b"error") => 2,
        t if eq(t, b"tip") => 3,
        t if eq(t, b"print") => 4,
        _ => {
            println3("plog: unknown type");
            print_types();
            return;
        }
    };

    if all {
        ty_id |= PLOG_ALL_FLAG;
    }

    let Some(sq) = start_quote else {
        println3("plog: content must be quoted");
        print_help();
        return;
    };
    let Some(eq) = end_quote else {
        println3("plog: missing closing quote");
        print_help();
        return;
    };
    if eq < sq {
        println3("plog: invalid quoted content");
        print_help();
        return;
    }

    let mut buf: Vec<u8> = Vec::new();
    for (i, part) in args[sq..=eq].iter().enumerate() {
        if i > 0 {
            buf.push(b' ');
        }
        buf.extend_from_slice(part);
    }
    if buf.len() >= 2 && buf[0] == b'\"' && buf[buf.len() - 1] == b'\"' {
        buf = buf[1..buf.len() - 1].to_vec();
    } else {
        println3("plog: content must be quoted");
        print_help();
        return;
    }

    let _ = sys_plog(ty_id, buf.as_ptr(), buf.len());
}

fn print_help() {
    println3("usage: plog [-a|--all] <type> \"content\"");
}

fn print_types() {
    println3("types: info warn error tip print get-help");
}

fn eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] { return false; }
        i += 1;
    }
    true
}

fn println3(s: &str) {
    let _ = user_lib::sys_write(3, s.as_ptr(), s.len());
    let _ = user_lib::sys_write(3, "\n".as_ptr(), 1);
}
