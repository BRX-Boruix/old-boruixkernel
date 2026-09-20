#![no_std]
#![feature(alloc_error_handler)]

use core::arch::asm;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};

mod alloc;
pub mod net;

pub fn sbrk_stats() -> (usize, usize) {
    alloc::sbrk_stats()
}

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "C" fn _start() -> ! {
    unsafe { alloc::init(); }
    extern "Rust" {
        fn main() -> i32;
    }
    let exit_code = unsafe { main() };
    sys_exit(exit_code);
}

#[inline(always)]
pub fn syscall(id: usize, arg1: usize, arg2: usize, arg3: usize) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "syscall",
            in("rax") id,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            out("rcx") _,
            out("r11") _,
            lateout("rax") ret,
        );
    }
    ret
}

static OTTO: AtomicBool = AtomicBool::new(false);
static STDOUT_REDIRECT: core::sync::atomic::AtomicIsize =
    core::sync::atomic::AtomicIsize::new(-1);

pub fn otto_set(enabled: bool) {
    OTTO.store(enabled, Ordering::Relaxed);
}

pub fn otto_toggle() -> bool {
    let cur = OTTO.load(Ordering::Relaxed);
    OTTO.store(!cur, Ordering::Relaxed);
    !cur
}

fn sys_write_raw(fd: usize, buffer: *const u8, len: usize) -> isize {
    syscall(1, fd, buffer as usize, len)
}

pub fn sys_write(fd: usize, buffer: *const u8, len: usize) -> isize {
    if fd == 3 {
        let rfd = STDOUT_REDIRECT.load(Ordering::Relaxed);
        if rfd >= 0 {
            return sys_write_raw(rfd as usize, buffer, len);
        }
        if OTTO.load(Ordering::Relaxed) {
            let _ = sys_write_raw(1, buffer, len);
        }
    }
    sys_write_raw(fd, buffer, len)
}

fn print3(s: &str) {
    let _ = sys_write(3, s.as_ptr(), s.len());
}

fn println3(s: &str) {
    let _ = sys_write(3, s.as_ptr(), s.len());
    let _ = sys_write(3, "\n".as_ptr(), 1);
}

fn print_cstr3(ptr: *const u8) {
    if ptr.is_null() {
        return;
    }
    let mut i = 0usize;
    unsafe {
        while i < 256 {
            let b = *ptr.add(i);
            if b == 0 {
                break;
            }
            let _ = sys_write(3, &b as *const u8, 1);
            i += 1;
        }
    }
}

fn print_num3(mut n: isize) {
    let mut buf = [0u8; 24];
    let mut i = 0usize;
    let neg = n < 0;
    if neg {
        n = -n;
    }
    let mut v = n as usize;
    if v == 0 {
        buf[i] = b'0';
        i += 1;
    } else {
        while v > 0 && i < buf.len() {
            buf[i] = b'0' + (v % 10) as u8;
            v /= 10;
            i += 1;
        }
    }
    if neg && i < buf.len() {
        buf[i] = b'-';
        i += 1;
    }
    // reverse
    let mut j = 0usize;
    while j < i / 2 {
        let a = j;
        let b = i - 1 - j;
        let tmp = buf[a];
        buf[a] = buf[b];
        buf[b] = tmp;
        j += 1;
    }
    let _ = sys_write(3, buf.as_ptr(), i);
}

pub fn sys_read(fd: usize, buffer: *mut u8, len: usize) -> isize {
    syscall(0, fd, buffer as usize, len)
}

pub fn stdout_redirect_set(fd: isize) {
    STDOUT_REDIRECT.store(fd, Ordering::Relaxed);
}

pub fn stdout_redirect_clear() {
    STDOUT_REDIRECT.store(-1, Ordering::Relaxed);
}

pub const O_CREAT: usize = 0x40;
pub const O_DIRECTORY: usize = 0x10000;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Stat {
    pub st_mode: u16,
    pub st_size: u64,
    pub st_type: u32,
}

pub fn sys_open(path: *const u8, flags: usize, mode: usize) -> isize {
    let ret = syscall(2, path as usize, flags, mode);
    if ret < 0 {
        print3("sys_open failed: ");
        print_cstr3(path);
        print3(" ret=");
        print_num3(ret);
        println3("");
    }
    ret
}

pub fn sys_close(fd: usize) -> isize {
    syscall(3, fd, 0, 0)
}

pub fn sys_stat(path: *const u8, st: *mut Stat) -> isize {
    let ret = syscall(4, path as usize, st as usize, 0);
    if ret < 0 {
        print3("sys_stat failed: ");
        print_cstr3(path);
        print3(" ret=");
        print_num3(ret);
        println3("");
    }
    ret
}

pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    syscall(5, fd, st as usize, 0)
}

pub fn sys_lseek(fd: usize, offset: usize) -> isize {
    syscall(8, fd, offset, 0)
}

pub fn sys_dup(fd: usize) -> isize {
    syscall(32, fd, 0, 0)
}

pub fn sys_dup2(oldfd: usize, newfd: usize) -> isize {
    syscall(33, oldfd, newfd, 0)
}

pub fn sys_fcntl(fd: usize, cmd: usize, arg: usize) -> isize {
    syscall(72, fd, cmd, arg)
}

pub fn sys_mount(dev: *const u8, dir: *const u8, fstype: *const u8) -> isize {
    syscall(165, dev as usize, dir as usize, fstype as usize)
}

pub fn sys_umount(dir: *const u8) -> isize {
    syscall(166, dir as usize, 0, 0)
}

pub fn sys_unlink(path: *const u8) -> isize {
    syscall(167, path as usize, 0, 0)
}

pub fn sys_rename(old: *const u8, new: *const u8) -> isize {
    syscall(168, old as usize, new as usize, 0)
}

pub fn sys_pipe(fds: *mut i32) -> isize {
    syscall(22, fds as usize, 0, 0)
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DirEntry {
    pub name_len: u16,
    pub name: [u8; 256],
}

pub fn sys_getdents(path: *const u8, buf: *mut DirEntry, max: usize) -> isize {
    let ret = syscall(6, path as usize, buf as usize, max);
    if ret < 0 {
        print3("sys_getdents failed: ");
        print_cstr3(path);
        print3(" ret=");
        print_num3(ret);
        println3("");
    }
    ret
}

pub fn sys_exit(code: i32) -> ! {
    syscall(60, code as usize, 0, 0);
    loop { unsafe { asm!("hlt") } }
}

pub fn sys_yield() -> isize {
    syscall(24, 0, 0, 0)
}

pub fn sys_getpid() -> isize {
    syscall(39, 0, 0, 0)
}

pub fn sys_fork() -> isize {
    unsafe {
        let ret: isize;
        asm!(
            "syscall",
            in("rax") 57,
            out("rcx") _,
            out("r11") _,
            lateout("rax") ret,
        );
        ret
    }
}

pub fn sys_exec(path: *const u8) -> isize {
    unsafe {
        let ret: isize;
        asm!(
            "syscall",
            in("rax") 59,
            in("rdi") path,
            out("rcx") _,
            out("r11") _,
            lateout("rax") ret,
        );
        ret
    }
}

pub fn sys_waitpid(pid: isize, exit_code: *mut i32) -> isize {
    unsafe {
        let ret: isize;
        asm!(
            "syscall",
            in("rax") 61,
            in("rdi") pid,
            in("rsi") exit_code,
            out("rcx") _,
            out("r11") _,
            lateout("rax") ret,
        );
        ret
    }
}

pub fn sys_ps(buf: *mut u8, len: usize) -> isize {
    syscall(200, buf as usize, len, 0)
}

pub fn sys_kill(pid: usize, code: i32) -> isize {
    syscall(201, pid, code as usize, 0)
}

pub fn sys_testmm1() -> isize {
    syscall(210, 0, 0, 0)
}

pub fn sys_plog(ty: usize, msg: *const u8, len: usize) -> isize {
    syscall(211, ty, msg as usize, len)
}

pub fn sys_mmstat(buf: *mut u8, len: usize) -> isize {
    syscall(212, buf as usize, len, 0)
}

pub fn sys_mmstat_reset() -> isize {
    syscall(212, 0, 0, 1)
}

pub fn sys_get_exec_arg() -> isize {
    syscall(213, 0, 0, 0)
}

pub fn sys_cpu_count() -> isize {
    syscall(214, 0, 0, 0)
}

pub fn sys_ipi_stat(buf: *mut u8, len: usize) -> isize {
    syscall(215, buf as usize, len, 0)
}

pub fn sys_mmcompact() -> isize {
    syscall(216, 0, 0, 0)
}

pub fn sys_sbrk(increment: isize) -> isize {
    syscall(217, increment as usize, 0, 0)
}

pub fn sys_testpmm2m(count: usize) -> isize {
    syscall(218, count, 0, 0)
}

pub fn sys_migrate_one() -> isize {
    syscall(219, 0, 0, 0)
}

pub fn sys_rmapcount() -> isize {
    syscall(220, 0, 0, 0)
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub header_type: u8,
    pub subsystem_vendor: u16,
    pub subsystem_device: u16,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
}

pub fn sys_pci_list(buf: *mut PciDevice, len: usize) -> isize {
    syscall(223, buf as usize, len, 0)
}

pub fn sys_pci_info(bus: u8, device: u8, function: u8, out: *mut PciDevice) -> isize {
    unsafe {
        let ret: isize;
        asm!(
            "syscall",
            in("rax") 224,
            in("rdi") bus as usize,
            in("rsi") device as usize,
            in("rdx") function as usize,
            in("r10") out as usize,
            out("rcx") _,
            out("r11") _,
            lateout("rax") ret,
        );
        ret
    }
}

pub fn sys_pci_mode() -> isize {
    syscall(225, 0, 0, 0)
}

pub fn sys_pci_count() -> isize {
    syscall(226, 0, 0, 0)
}

pub fn sys_driverhub(buf: *mut u8, len: usize, flags: usize) -> isize {
    syscall(227, buf as usize, len, flags)
}

pub const DRIVERHUB_NAME_LEN: usize = 32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DriverInfoRaw {
    pub name: [u8; DRIVERHUB_NAME_LEN],
    pub stage: u8,
    pub has_probe: u8,
    pub has_attach: u8,
    pub _pad: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DeviceInfoRaw {
    pub name: [u8; DRIVERHUB_NAME_LEN],
    pub driver_name: [u8; DRIVERHUB_NAME_LEN],
    pub kind: u8,
    pub bus: u8,
    pub _pad0: [u8; 2],
    pub location: u32,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub _pad1: u8,
}

pub fn sys_driverhub_drivers(buf: *mut DriverInfoRaw, len: usize) -> isize {
    syscall(228, buf as usize, len, 0)
}

pub fn sys_driverhub_devices(buf: *mut DeviceInfoRaw, len: usize) -> isize {
    syscall(229, buf as usize, len, 0)
}

pub fn sys_net_send(buf: *const u8, len: usize) -> isize {
    syscall(230, buf as usize, len, 0)
}

pub fn sys_net_recv(buf: *mut u8, len: usize) -> isize {
    syscall(231, buf as usize, len, 0)
}

pub fn sys_net_mac(buf: *mut u8, len: usize) -> isize {
    syscall(232, buf as usize, len, 0)
}

pub fn sys_net_status() -> isize {
    syscall(233, 0, 0, 0)
}

pub fn sys_net_regs(buf: *mut u32, len: usize) -> isize {
    syscall(234, buf as usize, len, 0)
}

pub fn sys_net_counters(buf: *mut u64, len: usize) -> isize {
    syscall(235, buf as usize, len, 0)
}

pub fn sys_time_human(buf: *mut u8, len: usize) -> isize {
    syscall(221, buf as usize, len, 0)
}

pub fn sys_time_seconds() -> isize {
    syscall(222, 0, 0, 0)
}

struct Console;

impl core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        sys_write(1, s.as_ptr(), s.len());
        Ok(())
    }
}

pub fn print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    Console.write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::print(format_args!($($arg)*));
    }
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ({
        $crate::print(format_args!($($arg)*));
        $crate::print!("\n");
    })
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let _ = sys_write(2, "User Panic!\n".as_ptr(), 12);
    sys_exit(-1);
}
