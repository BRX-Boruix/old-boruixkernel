#![no_std]

// Re-export println so macros can find it
pub use serial::print;
pub use serial::println;

/// 强制解锁底层的串口（不安全）
///
/// 用于 Panic 或 Double Fault 等极端情况。
pub unsafe fn force_unlock() {
    serial::force_unlock();
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => ({
        $crate::println!("\x1b[36m[INFO]\x1b[0m {}", format_args!($($arg)*));
    });
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => ({
        $crate::println!("\x1b[33m[WARN]\x1b[0m {}", format_args!($($arg)*));
    });
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => ({
        $crate::println!("\x1b[31m[ERROR]\x1b[0m {}", format_args!($($arg)*));
    });
}

#[macro_export]
macro_rules! tip {
    ($($arg:tt)*) => ({
        $crate::println!("\x1b[32m[TIP]\x1b[0m {}", format_args!($($arg)*));
    });
}
