use super::Command;
use user_lib::{
    sys_driverhub, sys_driverhub_devices, sys_driverhub_drivers, sys_write, DeviceInfoRaw,
    DriverInfoRaw, DRIVERHUB_NAME_LEN,
};

const FLAG_DRIVERS: usize = 1;
const FLAG_DEVICES: usize = 2;

const COMMAND_ABOUT: &str =
    "driverhub\n\nShow DriverHub drivers/devices summary or raw tables.\nUsage: driverhub [drivers|devices|all|raw [drivers|devices]]";

pub const CMD: Command = Command {
    name: b"driverhub",
    usage: "driverhub [drivers|devices|all|raw [drivers|devices]]",
    desc: "Show DriverHub drivers/devices summary",
    about: COMMAND_ABOUT,
    run,
};

fn run(args: &[&[u8]]) {
    let mut flags = 0usize;
    if args.len() > 1 {
        if args[1] == b"drivers" {
            flags = FLAG_DRIVERS;
        } else if args[1] == b"devices" {
            flags = FLAG_DEVICES;
        } else if args[1] == b"all" {
            flags = FLAG_DRIVERS | FLAG_DEVICES;
        } else if args[1] == b"raw" {
            if args.len() > 2 {
                if args[2] == b"drivers" {
                    raw_dump(true, false);
                } else if args[2] == b"devices" {
                    raw_dump(false, true);
                } else {
                    let msg = b"usage: driverhub raw [drivers|devices]\n";
                    let _ = sys_write(3, msg.as_ptr(), msg.len());
                }
            } else {
                raw_dump(true, true);
            }
            return;
        } else {
            let msg = b"usage: driverhub [drivers|devices|all|raw [drivers|devices]]\n";
            let _ = sys_write(3, msg.as_ptr(), msg.len());
            return;
        }
    }

    let mut buf = [0u8; 2048];
    let n = sys_driverhub(buf.as_mut_ptr(), buf.len(), flags);
    if n <= 0 {
        return;
    }
    let _ = sys_write(3, buf.as_ptr(), n as usize);
}

fn raw_dump(show_drivers: bool, show_devices: bool) {
    let mut drivers = [DriverInfoRaw::default(); 32];
    let mut devices = [DeviceInfoRaw::default(); 32];
    let drv_count = if show_drivers {
        sys_driverhub_drivers(drivers.as_mut_ptr(), drivers.len())
    } else {
        0
    };
    let dev_count = if show_devices {
        sys_driverhub_devices(devices.as_mut_ptr(), devices.len())
    } else {
        0
    };
    if (show_drivers && drv_count < 0) || (show_devices && dev_count < 0) {
        return;
    }

    if show_drivers {
        let _ = sys_write(3, b"Drivers (raw):\n".as_ptr(), 15);
        for (idx, drv) in drivers.iter().take(drv_count as usize).enumerate() {
            write_index(idx);
            write_name(&drv.name);
            let _ = sys_write(3, b" stage=".as_ptr(), 7);
            write_u8(drv.stage);
            let _ = sys_write(3, b" probe=".as_ptr(), 7);
            write_u8(drv.has_probe);
            let _ = sys_write(3, b" attach=".as_ptr(), 8);
            write_u8(drv.has_attach);
            let _ = sys_write(3, b"\n".as_ptr(), 1);
        }
    }

    if show_devices {
        if show_drivers {
            let _ = sys_write(3, b"\n".as_ptr(), 1);
        }
        let _ = sys_write(3, b"Devices (raw):\n".as_ptr(), 17);
        for (idx, dev) in devices.iter().take(dev_count as usize).enumerate() {
            write_index(idx);
            write_name(&dev.name);
            let _ = sys_write(3, b" driver=".as_ptr(), 8);
            write_name(&dev.driver_name);
            let _ = sys_write(3, b" kind=".as_ptr(), 6);
            write_u8(dev.kind);
            let _ = sys_write(3, b" bus=".as_ptr(), 5);
            write_u8(dev.bus);
            let _ = sys_write(3, b" loc=".as_ptr(), 5);
            write_hex32(dev.location);
            let _ = sys_write(3, b" vendor=".as_ptr(), 8);
            write_hex16(dev.vendor_id);
            let _ = sys_write(3, b" device=".as_ptr(), 8);
            write_hex16(dev.device_id);
            let _ = sys_write(3, b" class=".as_ptr(), 7);
            write_hex8(dev.class_code);
            let _ = sys_write(3, b" subclass=".as_ptr(), 10);
            write_hex8(dev.subclass);
            let _ = sys_write(3, b" prog-if=".as_ptr(), 9);
            write_hex8(dev.prog_if);
            let _ = sys_write(3, b"\n".as_ptr(), 1);
        }
    }
}

fn write_name(name: &[u8; DRIVERHUB_NAME_LEN]) {
    let mut len = 0usize;
    while len < name.len() && name[len] != 0 {
        len += 1;
    }
    if len == 0 {
        let _ = sys_write(3, b"-".as_ptr(), 1);
        return;
    }
    let _ = sys_write(3, name.as_ptr(), len);
}

fn write_index(idx: usize) {
    let _ = sys_write(3, b"  [".as_ptr(), 3);
    write_u32(idx as u32);
    let _ = sys_write(3, b"] ".as_ptr(), 2);
}

fn write_u8(value: u8) {
    write_u32(value as u32);
}

fn write_u32(mut value: u32) {
    let mut buf = [0u8; 10];
    let mut i = 0usize;
    if value == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        while value > 0 {
            buf[i] = b'0' + (value % 10) as u8;
            i += 1;
            value /= 10;
        }
    }
    while i > 0 {
        i -= 1;
        let _ = sys_write(3, &buf[i] as *const u8, 1);
    }
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

fn write_hex32(value: u32) {
    write_hex16((value >> 16) as u16);
    write_hex16((value & 0xFFFF) as u16);
}

fn nibble(x: u8) -> u8 {
    match x {
        0..=9 => b'0' + x,
        10..=15 => b'a' + (x - 10),
        _ => b'?'
    }
}
