use super::Command;
use user_lib::{sys_pci_list, sys_write, PciDevice};

const MAX_PCI_DEVICES: usize = 128;

const COMMAND_ABOUT: &str = "lspci\n\nList discovered PCI devices.\nUsage: lspci";

pub const CMD: Command = Command {
    name: b"lspci",
    usage: "lspci",
    desc: "List discovered PCI devices",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(_args: &[&[u8]]) {
    let mut devices = [PciDevice::default(); MAX_PCI_DEVICES];
    let count = sys_pci_list(devices.as_mut_ptr(), MAX_PCI_DEVICES);
    if count < 0 {
        print_err("lspci: failed to fetch PCI devices");
        return;
    }
    if count == 0 {
        print_err("lspci: no PCI devices found");
        return;
    }
    for dev in &devices[..count as usize] {
        print_device_line(dev);
    }
}

fn print_device_line(dev: &PciDevice) {
    let _ = sys_write(3, b"pci ".as_ptr(), 4);
    write_hex8(dev.bus);
    let _ = sys_write(3, b":".as_ptr(), 1);
    write_hex8(dev.device);
    let _ = sys_write(3, b".".as_ptr(), 1);
    write_hex8(dev.function);
    let _ = sys_write(3, b" vendor=".as_ptr(), 8);
    write_hex16(dev.vendor_id);
    let _ = sys_write(3, b" device=".as_ptr(), 8);
    write_hex16(dev.device_id);
    let _ = sys_write(3, b" class=".as_ptr(), 8);
    write_hex8(dev.class_code);
    let _ = sys_write(3, b" subclass=".as_ptr(), 10);
    write_hex8(dev.subclass);
    let _ = sys_write(3, b" prog-if=".as_ptr(), 9);
    write_hex8(dev.prog_if);
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn print_err(msg: &str) {
    let _ = sys_write(3, msg.as_ptr(), msg.len());
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn write_hex8(value: u8) {
    let mut buf = [0u8; 2];
    buf[0] = nibble((value >> 4) & 0xF);
    buf[1] = nibble(value & 0xF);
    let _ = sys_write(3, buf.as_ptr(), 2);
}

fn write_hex16(value: u16) {
    write_hex8((value >> 8) as u8);
    write_hex8((value & 0xFF) as u8);
}

fn nibble(x: u8) -> u8 {
    match x {
        0..=9 => b'0' + x,
        10..=15 => b'a' + (x - 10),
        _ => b'?'
    }
}
