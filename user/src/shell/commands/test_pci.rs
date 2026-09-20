use super::Command;
use user_lib::{sys_pci_count, sys_pci_mode, sys_write};

const COMMAND_ABOUT: &str =
    "test_pci\n\nShow PCI mode and device count (self-test).\nUsage: test_pci";

pub const CMD: Command = Command {
    name: b"test_pci",
    usage: "test_pci",
    desc: "Show PCI mode/count for self-test",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(_args: &[&[u8]]) {
    let mode = sys_pci_mode();
    if mode < 0 {
        print_err("test_pci: failed to query PCI mode");
        return;
    }
    match mode as u8 {
        0 => print_line("PCI mode", "legacy-IO"),
        1 => print_line("PCI mode", "mcfg-ECAM"),
        other => {
            print_string("PCI mode: unknown (value ");
            print_decimal(other as usize);
            let _ = sys_write(3, b")\n".as_ptr(), 2);
        }
    }

    let count = sys_pci_count();
    if count < 0 {
        print_err("test_pci: failed to count devices");
        return;
    }
    print_string("PCI device count: ");
    print_decimal(count as usize);
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn print_line(label: &str, value: &str) {
    print_string(label);
    print_string(": ");
    print_string(value);
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn print_err(msg: &str) {
    print_string(msg);
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn print_string(s: &str) {
    let _ = sys_write(3, s.as_ptr(), s.len());
}

fn print_decimal(mut value: usize) {
    let mut buf = [0u8; 20];
    let mut idx = 0usize;
    if value == 0 {
        buf[idx] = b'0';
        idx += 1;
    } else {
        while value > 0 {
            buf[idx] = b'0' + (value % 10) as u8;
            idx += 1;
            value /= 10;
        }
        buf[..idx].reverse();
    }
    let _ = sys_write(3, buf.as_ptr(), idx);
}
