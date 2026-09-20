#![no_std]

use limine::File;

pub fn module_path_str(module: &File) -> Option<&'static str> {
    let ptr = module.path.as_ptr()?;
    let mut len = 0usize;
    unsafe {
        while *ptr.add(len) != 0 && len < 256 {
            len += 1;
        }
        if len == 256 && *ptr.add(len) != 0 {
            logger::warn!("module path truncated to 256 bytes; possible mismatch");
        }
        core::str::from_utf8(core::slice::from_raw_parts(ptr as *const u8, len)).ok()
    }
}

pub fn module_matches(module_path: &str, requested: &str) -> bool {
    fn normalize(p: &str) -> &str {
        p.strip_prefix('/').unwrap_or(p)
    }
    normalize(module_path) == normalize(requested)
}
