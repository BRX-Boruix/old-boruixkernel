use alloc::string::{String, ToString};

use crate::vfs::{vfs_mkdir, vfs_mkfile, vfs_open, vfs_symlink, vfs_write, VfsResult};

fn read_hex(bytes: &[u8]) -> usize {
    let mut val = 0usize;
    for &b in bytes {
        val <<= 4;
        val |= match b {
            b'0'..=b'9' => (b - b'0') as usize,
            b'a'..=b'f' => (b - b'a' + 10) as usize,
            b'A'..=b'F' => (b - b'A' + 10) as usize,
            _ => 0,
        };
    }
    val
}

fn align4(x: usize) -> usize {
    (x + 3) & !3
}

pub fn unpack_cpio_newc(data: &[u8]) -> VfsResult {
    let mut off = 0usize;
    loop {
        if off + 110 > data.len() {
            break;
        }
        let header_off = off;
        let magic = &data[header_off..header_off + 6];
        if magic != b"070701" && magic != b"070702" {
            break;
        }

        let namesize = read_hex(&data[header_off + 94..header_off + 102]);
        let filesize = read_hex(&data[header_off + 54..header_off + 62]);
        let mode = read_hex(&data[header_off + 14..header_off + 22]);

        off = header_off + 110;
        if off + namesize > data.len() {
            break;
        }
        let name = &data[off..off + namesize];
        let name_str = core::str::from_utf8(name)
            .unwrap_or("")
            .trim_end_matches('\0');
        off = align4(off + namesize);

        if name_str == "TRAILER!!!" {
            break;
        }

        if off + filesize > data.len() {
            break;
        }
        let file_data = &data[off..off + filesize];
        off = align4(off + filesize);

        let path = if name_str.starts_with('/') {
            name_str.to_string()
        } else {
            let mut s = String::from("/");
            s.push_str(name_str);
            s
        };

        if (mode & 0o170000) == 0o040000 {
            let _ = vfs_mkdir(&path);
        } else if (mode & 0o170000) == 0o120000 {
            let target = core::str::from_utf8(file_data).unwrap_or("");
            let _ = vfs_symlink(&path, target);
        } else {
            let _ = vfs_mkfile(&path);
            if let Ok(node) = vfs_open(&path) {
                let _ = vfs_write(&node, 0, file_data);
            }
        }
    }
    Ok(())
}
