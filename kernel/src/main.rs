#![no_std]
#![no_main]

extern crate alloc;

use core::panic::PanicInfo;
use limine::BaseRevision;
use logger::{error, info, println, tip, warn};
use kernel_fs::vfs::{vfs_child_append, vfs_init, vfs_mount, vfs_register_fs};
use kernel_driver_hub::driver_hub::{self, device::DeviceKind, ramdisk};
use kernel_fs::fs::{tmpfs::Tmpfs, devtmpfs::DevTmpFs, sysfs::SysFs, procfs::ProcFs, fatfs::FatFs};
use kernel_fs::initramfs::unpack_cpio_newc;
use alloc::sync::Arc;

#[no_mangle]
// Set the base revision to the latest supported version.
pub static BASE_REVISION: BaseRevision = BaseRevision::new(0);

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Initialize early drivers (e.g., serial) before any logging.
    kernel_driver_hub::driver_hub::init_early();
    init_arch();
    kernel_driver_hub::driver_hub::init_core();
    init_mm();
    kernel_driver_hub::driver_hub::init_devices();
    register_ramdisks();
    init_vfs();
    kernel_task::task::set_kill_hook(Some(
        kernel_driver_hub::driver_hub::keyboard::wake_readers,
    ));
    kernel_task::smp::init();
    kernel_platform::ioapic::init();
    kernel_platform::memory::set_tlb_shootdown_hook(kernel_task::smp::tlb_shootdown_request);
    init_graphics();
    kernel_driver_hub::term_println!(
        "Welcome to {} v{}",
        config::KERNEL_NAME,
        config::KERNEL_VERSION
    );
    kernel_driver_hub::term_println!("Terminal initialized successfully!");
    println!("This message should appear on both serial and terminal.");
    print_banner();
    start_userspace();
    idle_loop()
}

#[no_mangle]
pub extern "C" fn syscall_stack_switch(
    tf: *mut arch::syscall::TrapFrame,
) -> *mut arch::syscall::TrapFrame {
    kernel_syscall::syscall::syscall_stack_switch(tf)
}

#[no_mangle]
pub extern "C" fn syscall_dispatch(tf: &mut arch::syscall::TrapFrame) {
    kernel_syscall::syscall::syscall_dispatch(tf)
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Force unlock serial port to ensure panic message is printed
    // This prevents deadlock if panic occurs while serial lock is held
    unsafe {
        logger::force_unlock();
    }

    println!("");
    error!("KERNEL PANIC");
    println!("{}", info);
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

fn print_banner() {
    println!("=========================");
    println!("{}", config::KERNEL_NAME);
    println!("Version: {}", config::KERNEL_VERSION);
    println!("=========================");
}

fn init_arch() {
    tip!("Initializing architecture structures...");
    kernel_platform::hal::arch::init();
    info!("GDT and IDT initialized.");
}

fn init_mm() {
    tip!("Initializing physical memory management...");
    kernel_platform::memory::init();
    info!("PMM and Heap initialized.");
}

fn init_graphics() {
    tip!("Initializing graphics subsystem...");
    if graphics::init() {
        info!("Graphics INIT.");
        {
            let fb_info = graphics::FRAMEBUFFER.lock();
            if let Some(fb) = fb_info.as_ref() {
                info!(
                    "Framebuffer Info: {}x{} bpp={} pitch={} addr={:#x}",
                    fb.width(),
                    fb.height(),
                    fb.bpp(),
                    fb.pitch(),
                    fb.addr()
                );
                kernel_driver_hub::driver_hub::terminal::init(
                    fb.addr() as *mut u32,
                    fb.width(),
                    fb.height(),
                    fb.pitch(),
                    fb.red_mask_size,
                    fb.red_mask_shift,
                    fb.green_mask_size,
                    fb.green_mask_shift,
                    fb.blue_mask_size,
                    fb.blue_mask_shift,
                );
                kernel_driver_hub::driver_hub::terminal::command_once();
                serial::println!("flanterm command executed once.");
                serial::set_extra_writer(kernel_driver_hub::driver_hub::terminal::_print);
                serial::set_extra_writer_bytes(kernel_driver_hub::driver_hub::terminal::write_bytes);
            }
        }
        info!("Graphics initialized.");
    } else {
        error!("Graphics INIT FAILED: No framebuffer found.");
    }
}

fn start_userspace() {
    println!("------START USERSPACE------");
    let modules = kernel_platform::memory::get_modules();
    if modules.is_empty() {
        warn!("No user modules found!");
        return;
    }

    // Stop mirroring serial logs to terminal so user shell stays clean.
    serial::clear_extra_writer();
    serial::clear_extra_writer_bytes();

    kernel_task::task::init();
    for module_ptr in modules {
        let module = unsafe { &*module_ptr.as_ptr() };
        let module_base = match module.base.as_ptr() {
            Some(p) => p,
            None => {
                warn!("Module base is null, skipping.");
                continue;
            }
        };

        let module_path = match kernel_modules::module_path_str(module) {
            Some(p) => p,
            None => "",
        };

        info!(
            "Found module: {} at {:p}, size: {}",
            module_path, module_base, module.length
        );

        if kernel_modules::module_matches(module_path, "initramfs") {
            let ramfs =
                unsafe { core::slice::from_raw_parts(module_base, module.length as usize) };
            if let Err(e) = unpack_cpio_newc(ramfs) {
                warn!("initramfs unpack failed: {:?}", e);
            }
            continue;
        }
        if kernel_modules::module_matches(module_path, "initproc") {
            let elf_data =
                unsafe { core::slice::from_raw_parts(module_base, module.length as usize) };
            info!("Creating initproc task...");
            match kernel_task::task::add_task(elf_data) {
                Ok(pid) => info!("Created initproc PID: {}", pid),
                Err(e) => error!("Failed to create initproc ({}): {}", e.kind(), e),
            }
        }
    }
    kernel_task::task::enable_preempt();
    kernel_task::task::run_tasks();
}

fn register_ramdisks() {
    let modules = kernel_platform::memory::get_modules();
    if modules.is_empty() {
        return;
    }
    let mut ramdisk_idx = 0usize;
    for module_ptr in modules {
        let module = unsafe { &*module_ptr.as_ptr() };
        let module_base = match module.base.as_ptr() {
            Some(p) => p,
            None => continue,
        };
        let module_path = match kernel_modules::module_path_str(module) {
            Some(p) => p,
            None => "",
        };
        if kernel_modules::module_matches(module_path, "fatimg")
            || kernel_modules::module_matches(module_path, "ramdisk")
        {
            let image = unsafe { core::slice::from_raw_parts(module_base, module.length as usize) };
            let name = alloc::format!("ram{}", ramdisk_idx);
            ramdisk_idx += 1;
            ramdisk::register_ramdisk(&name, image);
        }
    }
}

fn init_vfs() {
    let root = vfs_init();
    let _ = vfs_register_fs("tmpfs", Arc::new(Tmpfs::new()), 0, 0);
    let _ = vfs_register_fs("devtmpfs", Arc::new(DevTmpFs::new()), 0, 0);
    let _ = vfs_register_fs("sysfs", Arc::new(SysFs::new()), 0, 0);
    let _ = vfs_register_fs("procfs", Arc::new(ProcFs::new()), 0, 0);
    let _ = vfs_register_fs("fatfs", Arc::new(FatFs::new()), 0, 0);

    let _ = vfs_mount(None, "tmpfs", &root);

    let dev = vfs_child_append(&root, "dev");
    dev.meta.lock().node_type = kernel_fs::vfs::VfsNodeType::Dir;
    let _ = vfs_mount(None, "devtmpfs", &dev);

    let sys = vfs_child_append(&root, "sys");
    sys.meta.lock().node_type = kernel_fs::vfs::VfsNodeType::Dir;
    let _ = vfs_mount(None, "sysfs", &sys);

    let proc = vfs_child_append(&root, "proc");
    proc.meta.lock().node_type = kernel_fs::vfs::VfsNodeType::Dir;
    let _ = vfs_mount(None, "procfs", &proc);

    let volumes = vfs_child_append(&root, "volumes");
    volumes.meta.lock().node_type = kernel_fs::vfs::VfsNodeType::Dir;
    let count = driver_hub::device_count();
    let mut vol_idx = 0usize;
    for idx in 0..count {
        let Some(info) = driver_hub::device_info_at(idx) else { continue };
        if info.kind != DeviceKind::Block {
            continue;
        }
        let name = alloc::format!("vol{}", vol_idx);
        vol_idx += 1;
        let mountpoint = vfs_child_append(&volumes, &name);
        let src = alloc::format!("/dev/{}", info.name);
        let _ = vfs_mount(Some(&src), "fatfs", &mountpoint);
    }
}

fn idle_loop() -> ! {
    println!("======STARTED======");
    loop {
        kernel_driver_hub::driver_hub::serial_cmd::poll();
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
