use super::Command;
use user_lib::{sys_pci_info, sys_write, PciDevice};

const COMMAND_ABOUT: &str =
    "pci_info\n\nShow detailed PCI device fields.\nUsage: pci_info <bus> <device> <function>";

pub const CMD: Command = Command {
    name: b"pci_info",
    usage: "pci_info <bus> <device> <function>",
    desc: "Show detailed PCI device fields",
    about: COMMAND_ABOUT,
    run: run,
};

fn run(args: &[&[u8]]) {
    if args.len() != 4 {
        print_usage();
        return;
    }
    let Some(bus) = parse_u8(args[1]) else {
        print_err("pci_info: invalid bus");
        return;
    };
    let Some(device) = parse_u8(args[2]) else {
        print_err("pci_info: invalid device");
        return;
    };
    let Some(function) = parse_u8(args[3]) else {
        print_err("pci_info: invalid function");
        return;
    };

    let mut info = PciDevice::default();
    if sys_pci_info(bus, device, function, &mut info) != 0 {
        print_err("pci_info: device not found");
        return;
    }
    print_device_info(&info);
}

fn parse_u8(arg: &[u8]) -> Option<u8> {
    if arg.is_empty() {
        return None;
    }
    if arg.len() > 2 && (arg[0] == b'0' && (arg[1] == b'x' || arg[1] == b'X')) {
        parse_hex(&arg[2..])
    } else {
        parse_decimal(arg)
    }
}

fn parse_hex(bytes: &[u8]) -> Option<u8> {
    if bytes.is_empty() || bytes.len() > 2 {
        return None;
    }
    let mut value = 0u8;
    for &b in bytes {
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => 10 + (b - b'a'),
            b'A'..=b'F' => 10 + (b - b'A'),
            _ => return None,
        };
        value = value.wrapping_mul(16);
        value = value.wrapping_add(digit);
    }
    Some(value)
}

fn parse_decimal(bytes: &[u8]) -> Option<u8> {
    if bytes.is_empty() {
        return None;
    }
    let mut value = 0u8;
    for &b in bytes {
        if b < b'0' || b > b'9' {
            return None;
        }
        let digit = b - b'0';
        value = match value.checked_mul(10) {
            Some(v) => v,
            None => return None,
        };
        value = match value.checked_add(digit) {
            Some(v) => v,
            None => return None,
        };
    }
    Some(value)
}

fn print_device_info(info: &PciDevice) {
    print_label_value("Bus", info.bus);
    print_label_value("Device", info.device);
    print_label_value("Function", info.function);
    print_label_hex("Vendor ID", info.vendor_id);
    print_label_hex("Device ID", info.device_id);
    print_label_value_hex("Class code", info.class_code);
    print_label_value_hex("Subclass", info.subclass);
    print_label_value_hex("Prog IF", info.prog_if);
    print_label_value_hex("Header Type", info.header_type);
    print_label_hex("Subsystem Vendor", info.subsystem_vendor);
    print_label_hex("Subsystem Device", info.subsystem_device);
    print_label_value_hex("Interrupt Line", info.interrupt_line);
    print_label_value_hex("Interrupt Pin", info.interrupt_pin);
}

fn print_label_value(label: &str, value: u8) {
    let _ = sys_write(3, label.as_ptr(), label.len());
    let _ = sys_write(3, b": 0x".as_ptr(), 4);
    write_hex8(value);
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn print_label_hex(label: &str, value: u16) {
    let _ = sys_write(3, label.as_ptr(), label.len());
    let _ = sys_write(3, b": 0x".as_ptr(), 4);
    write_hex16(value);
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn print_label_value_hex(label: &str, value: u8) {
    let _ = sys_write(3, label.as_ptr(), label.len());
    let _ = sys_write(3, b": 0x".as_ptr(), 4);
    write_hex8(value);
    let _ = sys_write(3, b"\n".as_ptr(), 1);
}

fn print_usage() {
    let _ = sys_write(3, b"usage: pci_info <bus> <device> <function>\n".as_ptr(), 41);
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
