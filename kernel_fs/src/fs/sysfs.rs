use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Write;

use kernel_driver_hub::driver_hub;
use kernel_driver_hub::driver_hub::device::{BusType, DeviceInfo, DeviceKind};

use crate::vfs::{
    vfs_child_append, vfs_child_find, VfsError, VfsNode, VfsNodeType, VfsOps, VfsResult,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SysKind {
    Dir,
    File,
    Symlink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SysDirKind {
    Root,
    Class,
    ClassChar,
    ClassBlock,
    ClassNet,
    ClassDisplay,
    ClassMisc,
    Devices,
    Bus,
    BusPci,
    BusPlatform,
    BusUnknown,
    BusPciDevices,
    BusPlatformDevices,
    BusUnknownDevices,
    Dev,
    DevChar,
    DevBlock,
    Kernel,
}

struct SysHandle {
    kind: SysKind,
    data: Vec<u8>,
    dir_kind: Option<SysDirKind>,
}

impl SysHandle {
    fn dir(kind: SysDirKind) -> Self {
        Self {
            kind: SysKind::Dir,
            data: Vec::new(),
            dir_kind: Some(kind),
        }
    }

    fn file() -> Self {
        Self {
            kind: SysKind::File,
            data: Vec::new(),
            dir_kind: None,
        }
    }
}

fn set_handle(node: &Arc<VfsNode>, handle: SysHandle) {
    let ptr = Box::into_raw(Box::new(handle));
    node.meta.lock().handle = Some(ptr as usize);
}

fn get_handle(node: &Arc<VfsNode>) -> VfsResult<&'static mut SysHandle> {
    let ptr = node.meta.lock().handle.ok_or(VfsError::Invalid)? as *mut SysHandle;
    if ptr.is_null() {
        return Err(VfsError::Invalid);
    }
    Ok(unsafe { &mut *ptr })
}

fn drop_handle(node: &Arc<VfsNode>) {
    let ptr = node.meta.lock().handle.take().unwrap_or(0) as *mut SysHandle;
    if !ptr.is_null() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    }
}

fn set_dir(node: &Arc<VfsNode>, kind: SysDirKind) {
    set_handle(node, SysHandle::dir(kind));
    node.meta.lock().node_type = VfsNodeType::Dir;
}

fn set_file(node: &Arc<VfsNode>, data: Vec<u8>) {
    set_handle(
        node,
        SysHandle {
            kind: SysKind::File,
            data,
            dir_kind: None,
        },
    );
    node.meta.lock().node_type = VfsNodeType::Stream;
}

fn set_symlink(node: &Arc<VfsNode>, target: &str) {
    set_handle(
        node,
        SysHandle {
            kind: SysKind::Symlink,
            data: target.as_bytes().to_vec(),
            dir_kind: None,
        },
    );
    let mut meta = node.meta.lock();
    meta.node_type = VfsNodeType::Symlink;
    meta.linkto_path = Some(String::from(target));
}

fn prebuild(root: &Arc<VfsNode>) {
    let class = vfs_child_append(root, "class");
    let devices = vfs_child_append(root, "devices");
    let bus = vfs_child_append(root, "bus");
    let dev = vfs_child_append(root, "dev");
    let kernel = vfs_child_append(root, "kernel");

    set_dir(&class, SysDirKind::Class);
    set_dir(&devices, SysDirKind::Devices);
    set_dir(&bus, SysDirKind::Bus);
    set_dir(&dev, SysDirKind::Dev);
    set_dir(&kernel, SysDirKind::Kernel);

    let class_char = vfs_child_append(&class, "char");
    let class_block = vfs_child_append(&class, "block");
    let class_net = vfs_child_append(&class, "net");
    let class_display = vfs_child_append(&class, "display");
    let class_misc = vfs_child_append(&class, "misc");
    set_dir(&class_char, SysDirKind::ClassChar);
    set_dir(&class_block, SysDirKind::ClassBlock);
    set_dir(&class_net, SysDirKind::ClassNet);
    set_dir(&class_display, SysDirKind::ClassDisplay);
    set_dir(&class_misc, SysDirKind::ClassMisc);

    let dev_char = vfs_child_append(&dev, "char");
    let dev_block = vfs_child_append(&dev, "block");
    set_dir(&dev_char, SysDirKind::DevChar);
    set_dir(&dev_block, SysDirKind::DevBlock);

    let bus_pci = vfs_child_append(&bus, "pci");
    let bus_platform = vfs_child_append(&bus, "platform");
    let bus_unknown = vfs_child_append(&bus, "unknown");
    set_dir(&bus_pci, SysDirKind::BusPci);
    set_dir(&bus_platform, SysDirKind::BusPlatform);
    set_dir(&bus_unknown, SysDirKind::BusUnknown);

    let bus_pci_dev = vfs_child_append(&bus_pci, "devices");
    let bus_platform_dev = vfs_child_append(&bus_platform, "devices");
    let bus_unknown_dev = vfs_child_append(&bus_unknown, "devices");
    set_dir(&bus_pci_dev, SysDirKind::BusPciDevices);
    set_dir(&bus_platform_dev, SysDirKind::BusPlatformDevices);
    set_dir(&bus_unknown_dev, SysDirKind::BusUnknownDevices);
}

fn device_info_text(info: DeviceInfo, driver: Option<&'static str>) -> Vec<u8> {
    let mut out = String::new();
    let bus = match info.bus {
        BusType::Platform => "platform",
        BusType::Pci => "pci",
        BusType::Unknown => "unknown",
    };
    let kind = match info.kind {
        DeviceKind::Char => "char",
        DeviceKind::Block => "block",
        DeviceKind::Net => "net",
        DeviceKind::Display => "display",
        DeviceKind::Misc => "misc",
    };
    let _ = writeln!(out, "name={}", info.name);
    let _ = writeln!(out, "kind={}", kind);
    let _ = writeln!(out, "bus={}", bus);
    let _ = writeln!(
        out,
        "vendor={:04x} device={:04x} class={:02x} subclass={:02x} prog-if={:02x}",
        info.vendor_id, info.device_id, info.class_code, info.subclass, info.prog_if
    );
    if let Some(drv) = driver {
        let _ = writeln!(out, "driver={}", drv);
    }
    out.into_bytes()
}

fn ensure_device_file(parent: &Arc<VfsNode>, info: DeviceInfo, driver: Option<&'static str>) {
    if vfs_child_find(parent, info.name).is_some() {
        return;
    }
    let node = vfs_child_append(parent, info.name);
    set_file(&node, device_info_text(info, driver));
}

fn ensure_symlink(parent: &Arc<VfsNode>, name: &str, target: &str) {
    if vfs_child_find(parent, name).is_some() {
        return;
    }
    let node = vfs_child_append(parent, name);
    set_symlink(&node, target);
}

fn class_dir_for_kind(kind: DeviceKind) -> Option<SysDirKind> {
    match kind {
        DeviceKind::Char => Some(SysDirKind::ClassChar),
        DeviceKind::Block => Some(SysDirKind::ClassBlock),
        DeviceKind::Net => Some(SysDirKind::ClassNet),
        DeviceKind::Display => Some(SysDirKind::ClassDisplay),
        DeviceKind::Misc => Some(SysDirKind::ClassMisc),
    }
}

fn populate_sysfs(root: &Arc<VfsNode>) {
    let Some(class) = vfs_child_find(root, "class") else { return };
    let Some(devices) = vfs_child_find(root, "devices") else { return };
    let Some(bus) = vfs_child_find(root, "bus") else { return };
    let Some(dev) = vfs_child_find(root, "dev") else { return };
    let Some(dev_char) = vfs_child_find(&dev, "char") else { return };
    let Some(dev_block) = vfs_child_find(&dev, "block") else { return };

    let bus_pci_dev = vfs_child_find(&bus, "pci")
        .and_then(|b| vfs_child_find(&b, "devices"));
    let bus_platform_dev = vfs_child_find(&bus, "platform")
        .and_then(|b| vfs_child_find(&b, "devices"));
    let bus_unknown_dev = vfs_child_find(&bus, "unknown")
        .and_then(|b| vfs_child_find(&b, "devices"));

    let count = driver_hub::device_count();
    for idx in 0..count {
        let info = match driver_hub::device_info_at(idx) {
            Some(i) => i,
            None => continue,
        };
        let driver = driver_hub::device_driver_at(idx);

        ensure_device_file(&devices, info, driver);

        match info.kind {
            DeviceKind::Char => {
                ensure_symlink(&dev_char, info.name, &format!("/dev/{}", info.name));
            }
            DeviceKind::Block => {
                ensure_symlink(&dev_block, info.name, &format!("/dev/{}", info.name));
            }
            _ => {}
        }

        if let Some(dir_kind) = class_dir_for_kind(info.kind) {
            if let Some(class_dir) = match dir_kind {
                SysDirKind::ClassChar => vfs_child_find(&class, "char"),
                SysDirKind::ClassBlock => vfs_child_find(&class, "block"),
                SysDirKind::ClassNet => vfs_child_find(&class, "net"),
                SysDirKind::ClassDisplay => vfs_child_find(&class, "display"),
                SysDirKind::ClassMisc => vfs_child_find(&class, "misc"),
                _ => None,
            } {
                ensure_symlink(&class_dir, info.name, &format!("/sys/devices/{}", info.name));
            }
        }

        match info.bus {
            BusType::Pci => {
                if let Some(parent) = &bus_pci_dev {
                    ensure_symlink(parent, info.name, &format!("/sys/devices/{}", info.name));
                }
            }
            BusType::Platform => {
                if let Some(parent) = &bus_platform_dev {
                    ensure_symlink(parent, info.name, &format!("/sys/devices/{}", info.name));
                }
            }
            BusType::Unknown => {
                if let Some(parent) = &bus_unknown_dev {
                    ensure_symlink(parent, info.name, &format!("/sys/devices/{}", info.name));
                }
            }
        }
    }
}

fn find_device_by_name(name: &str) -> Option<(DeviceInfo, Option<&'static str>)> {
    let count = driver_hub::device_count();
    for idx in 0..count {
        let info = match driver_hub::device_info_at(idx) {
            Some(i) => i,
            None => continue,
        };
        if info.name == name {
            return Some((info, driver_hub::device_driver_at(idx)));
        }
    }
    None
}

pub struct SysFs;

impl SysFs {
    pub fn new() -> Self {
        Self
    }
}

impl VfsOps for SysFs {
    fn mount(&self, _src: Option<&str>, node: &Arc<VfsNode>) -> VfsResult {
        set_dir(node, SysDirKind::Root);
        prebuild(node);
        populate_sysfs(node);
        Ok(())
    }

    fn unmount(&self, node: &Arc<VfsNode>) -> VfsResult {
        drop_handle(node);
        Ok(())
    }

    fn mkdir(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_dir(node, SysDirKind::Root);
        Ok(())
    }

    fn mkfile(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_file(node, Vec::new());
        Ok(())
    }

    fn symlink(&self, _parent: &Arc<VfsNode>, _name: &str, node: &Arc<VfsNode>) -> VfsResult {
        set_symlink(node, "");
        Ok(())
    }

    fn read(&self, node: &Arc<VfsNode>, offset: usize, buf: &mut [u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        if handle.kind == SysKind::Dir {
            return Err(VfsError::Invalid);
        }
        if offset >= handle.data.len() {
            return Ok(0);
        }
        let end = core::cmp::min(handle.data.len(), offset + buf.len());
        let slice = &handle.data[offset..end];
        buf[..slice.len()].copy_from_slice(slice);
        Ok(slice.len())
    }

    fn write(&self, node: &Arc<VfsNode>, offset: usize, buf: &[u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        if handle.kind == SysKind::Dir {
            return Err(VfsError::Invalid);
        }
        let end = offset + buf.len();
        if end > handle.data.len() {
            handle.data.resize(end, 0);
        }
        handle.data[offset..end].copy_from_slice(buf);
        node.meta.lock().size = handle.data.len() as u64;
        Ok(buf.len())
    }

    fn readlink(&self, node: &Arc<VfsNode>, buf: &mut [u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        let n = core::cmp::min(handle.data.len(), buf.len());
        buf[..n].copy_from_slice(&handle.data[..n]);
        Ok(n)
    }

    fn open(&self, parent: Option<&Arc<VfsNode>>, name: &str, node: &Arc<VfsNode>) -> VfsResult {
        let Some(parent) = parent else { return Err(VfsError::NotFound) };
        let handle = get_handle(parent)?;
        let Some(dir_kind) = handle.dir_kind else { return Err(VfsError::NotFound) };

        let Some((info, driver)) = find_device_by_name(name) else {
            return Err(VfsError::NotFound);
        };

        match dir_kind {
            SysDirKind::Devices => {
                set_file(node, device_info_text(info, driver));
                Ok(())
            }
            SysDirKind::DevChar if info.kind == DeviceKind::Char => {
                set_symlink(node, &format!("/dev/{}", info.name));
                Ok(())
            }
            SysDirKind::DevBlock if info.kind == DeviceKind::Block => {
                set_symlink(node, &format!("/dev/{}", info.name));
                Ok(())
            }
            SysDirKind::ClassChar | SysDirKind::ClassBlock | SysDirKind::ClassNet | SysDirKind::ClassDisplay | SysDirKind::ClassMisc => {
                if class_dir_for_kind(info.kind) == Some(dir_kind) {
                    set_symlink(node, &format!("/sys/devices/{}", info.name));
                    Ok(())
                } else {
                    Err(VfsError::NotFound)
                }
            }
            SysDirKind::BusPciDevices if info.bus == BusType::Pci => {
                set_symlink(node, &format!("/sys/devices/{}", info.name));
                Ok(())
            }
            SysDirKind::BusPlatformDevices if info.bus == BusType::Platform => {
                set_symlink(node, &format!("/sys/devices/{}", info.name));
                Ok(())
            }
            SysDirKind::BusUnknownDevices if info.bus == BusType::Unknown => {
                set_symlink(node, &format!("/sys/devices/{}", info.name));
                Ok(())
            }
            _ => Err(VfsError::NotFound),
        }
    }

    fn delete(&self, _parent: &Arc<VfsNode>, node: &Arc<VfsNode>) -> VfsResult {
        drop_handle(node);
        Ok(())
    }

    fn rename(&self, node: &Arc<VfsNode>, new_name: &str) -> VfsResult {
        node.meta.lock().name = String::from(new_name);
        Ok(())
    }

    fn free(&self, node: &Arc<VfsNode>) -> VfsResult {
        drop_handle(node);
        Ok(())
    }
}
