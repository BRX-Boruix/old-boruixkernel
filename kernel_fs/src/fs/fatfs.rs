use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use serial;

use crate::vfs::{
    vfs_child_append, vfs_child_find, vfs_free, vfs_open, vfs_read, vfs_write, VfsError, VfsNode,
    VfsNodeType, VfsOps, VfsResult,
};

const FAT_EOC: u32 = 0x0FFF_FFFF;

#[derive(Clone, Copy, Debug)]
struct FatBpb {
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    num_fats: u8,
    root_entry_count: u16,
    total_sectors: u32,
    fat_size_sectors: u32,
    root_cluster: u32,
}

struct FatFsState {
    dev: Arc<VfsNode>,
    bpb: FatBpb,
    fat_offset: u64,
    fat_size: u64,
    data_offset: u64,
    cluster_count: u32,
}

#[derive(Clone, Copy)]
struct FatNode {
    fs: *const FatFsState,
    first_cluster: u32,
    size: u32,
    is_dir: bool,
    entry_offset: Option<u64>,
}

fn read_u16_le(data: &[u8], off: usize) -> Option<u16> {
    if off + 2 > data.len() {
        return None;
    }
    Some(u16::from_le_bytes([data[off], data[off + 1]]))
}

fn read_u32_le(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 > data.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
    ]))
}

fn write_u16_le(buf: &mut [u8], off: usize, val: u16) {
    let bytes = val.to_le_bytes();
    buf[off] = bytes[0];
    buf[off + 1] = bytes[1];
}

fn write_u32_le(buf: &mut [u8], off: usize, val: u32) {
    let bytes = val.to_le_bytes();
    buf[off] = bytes[0];
    buf[off + 1] = bytes[1];
    buf[off + 2] = bytes[2];
    buf[off + 3] = bytes[3];
}

fn parse_bpb(sector: &[u8]) -> Option<FatBpb> {
    if sector.len() < 90 {
        return None;
    }
    let bytes_per_sector = read_u16_le(sector, 11)?;
    let sectors_per_cluster = sector.get(13).copied()?;
    let reserved_sectors = read_u16_le(sector, 14)?;
    let num_fats = sector.get(16).copied()?;
    let root_entry_count = read_u16_le(sector, 17)?;
    let total_sectors_16 = read_u16_le(sector, 19)? as u32;
    let fat_size_16 = read_u16_le(sector, 22)? as u32;
    let total_sectors_32 = read_u32_le(sector, 32)?;
    let fat_size_32 = read_u32_le(sector, 36)?;
    let root_cluster = read_u32_le(sector, 44)?;

    let total_sectors = if total_sectors_16 != 0 {
        total_sectors_16
    } else {
        total_sectors_32
    };
    let fat_size_sectors = if fat_size_16 != 0 {
        fat_size_16
    } else {
        fat_size_32
    };

    if bytes_per_sector == 0 || sectors_per_cluster == 0 || fat_size_sectors == 0 {
        return None;
    }

    Some(FatBpb {
        bytes_per_sector,
        sectors_per_cluster,
        reserved_sectors,
        num_fats,
        root_entry_count,
        total_sectors,
        fat_size_sectors,
        root_cluster,
    })
}

fn cluster_size_bytes(bpb: &FatBpb) -> u64 {
    bpb.bytes_per_sector as u64 * bpb.sectors_per_cluster as u64
}

fn name_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn name_from_entry(entry: &[u8]) -> Option<String> {
    if entry.len() < 11 {
        return None;
    }
    let name = &entry[0..8];
    let ext = &entry[8..11];
    let mut base = String::new();
    for &b in name {
        if b == b' ' {
            break;
        }
        base.push(b as char);
    }
    if base.is_empty() {
        return None;
    }
    let mut ext_s = String::new();
    for &b in ext {
        if b == b' ' {
            break;
        }
        ext_s.push(b as char);
    }
    if !ext_s.is_empty() {
        base.push('.');
        base.push_str(&ext_s);
    }
    Some(base)
}

fn lfn_part_from_entry(entry: &[u8]) -> String {
    let mut out = String::new();
    let mut read_u16 = |off: usize| -> u16 {
        let lo = entry[off] as u16;
        let hi = entry[off + 1] as u16;
        (hi << 8) | lo
    };
    let mut push_u16 = |val: u16, out: &mut String| {
        if val == 0x0000 || val == 0xFFFF {
            return false;
        }
        if let Some(ch) = core::char::from_u32(val as u32) {
            out.push(ch);
        }
        true
    };

    let offs = [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
    for off in offs.iter() {
        let v = read_u16(*off);
        if !push_u16(v, &mut out) {
            break;
        }
    }
    out
}

fn is_valid_short_char(b: u8) -> bool {
    matches!(
        b,
        b'A'..=b'Z'
            | b'0'..=b'9'
            | b' '
            | b'$'
            | b'%'
            | b'\''
            | b'-'
            | b'_'
            | b'@'
            | b'~'
            | b'`'
            | b'!'
            | b'('
            | b')'
            | b'{'
            | b'}'
            | b'^'
            | b'#'
            | b'&'
    )
}

fn to_short_name(name: &str) -> Option<[u8; 11]> {
    if name == "." || name == ".." {
        return None;
    }
    let mut out = [b' '; 11];
    let mut parts = name.split('.');
    let base = parts.next().unwrap_or("");
    let ext = parts.next().unwrap_or("");
    if parts.next().is_some() {
        return None;
    }
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return None;
    }
    for (i, ch) in base.bytes().enumerate() {
        let b = ch.to_ascii_uppercase();
        if !is_valid_short_char(b) || b == b' ' {
            return None;
        }
        out[i] = b;
    }
    for (i, ch) in ext.bytes().enumerate() {
        let b = ch.to_ascii_uppercase();
        if !is_valid_short_char(b) || b == b' ' {
            return None;
        }
        out[8 + i] = b;
    }
    Some(out)
}

fn lfn_checksum(short: &[u8; 11]) -> u8 {
    let mut sum = 0u8;
    for b in short.iter() {
        sum = ((sum & 1) << 7) + (sum >> 1) + b;
    }
    sum
}

fn build_lfn_entries(name: &str, short: &[u8; 11]) -> Vec<[u8; 32]> {
    let mut utf16: Vec<u16> = Vec::new();
    for ch in name.chars() {
        let v = ch as u32;
        if v <= 0xFFFF {
            utf16.push(v as u16);
        } else {
            utf16.push(b'?' as u16);
        }
    }

    let mut chunks: Vec<&[u16]> = Vec::new();
    let mut i = 0usize;
    while i < utf16.len() {
        let end = core::cmp::min(i + 13, utf16.len());
        chunks.push(&utf16[i..end]);
        i = end;
    }
    if chunks.is_empty() {
        chunks.push(&[]);
    }

    let checksum = lfn_checksum(short);
    let mut entries = Vec::new();
    let total = chunks.len() as u8;
    for (idx, part) in chunks.iter().enumerate().rev() {
        let seq = (idx + 1) as u8;
        let mut entry = [0u8; 32];
        entry[0] = if seq == total { seq | 0x40 } else { seq };
        entry[11] = 0x0F;
        entry[12] = 0;
        entry[13] = checksum;
        entry[26] = 0;
        entry[27] = 0;

        let mut name_slots: [u16; 13] = [0xFFFF; 13];
        for (i, &ch) in part.iter().enumerate() {
            name_slots[i] = ch;
        }
        if part.len() < 13 {
            name_slots[part.len()] = 0x0000;
        }

        let mut write_u16 = |off: usize, val: u16| {
            entry[off] = (val & 0xFF) as u8;
            entry[off + 1] = (val >> 8) as u8;
        };

        for i in 0..5 {
            write_u16(1 + i * 2, name_slots[i]);
        }
        for i in 0..6 {
            write_u16(14 + i * 2, name_slots[5 + i]);
        }
        for i in 0..2 {
            write_u16(28 + i * 2, name_slots[11 + i]);
        }
        entries.push(entry);
    }
    entries
}

fn normalize_short_base(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        let b = ch as u8;
        if b.is_ascii_alphanumeric() {
            out.push((b as char).to_ascii_uppercase());
        }
    }
    if out.is_empty() {
        out.push('F');
        out.push('I');
        out.push('L');
        out.push('E');
    }
    out
}

fn to_short_name_auto(state: &FatFsState, dir_cluster: u32, name: &str) -> [u8; 11] {
    let mut parts = name.split('.');
    let base = parts.next().unwrap_or("");
    let ext = parts.next().unwrap_or("");
    let base_norm = normalize_short_base(base);
    let ext_norm = normalize_short_base(ext);

    let mut existing: Vec<[u8; 11]> = Vec::new();
    if let Ok(list) = list_dir_shortnames(state, dir_cluster) {
        existing = list;
    }

    for n in 1u8..=99u8 {
        let mut short = [b' '; 11];
        let suffix = if n == 1 { String::from("~1") } else { alloc::format!("~{}", n) };
        let base_len = core::cmp::min(8 - suffix.len(), base_norm.len());
        let mut idx = 0usize;
        for ch in base_norm.bytes().take(base_len) {
            short[idx] = ch;
            idx += 1;
        }
        for ch in suffix.bytes() {
            if idx < 8 {
                short[idx] = ch;
                idx += 1;
            }
        }
        for (i, ch) in ext_norm.bytes().take(3).enumerate() {
            short[8 + i] = ch;
        }
        if !existing.iter().any(|e| e == &short) {
            return short;
        }
    }

    let mut short = [b' '; 11];
    for (i, ch) in base_norm.bytes().take(8).enumerate() {
        short[i] = ch;
    }
    for (i, ch) in ext_norm.bytes().take(3).enumerate() {
        short[8 + i] = ch;
    }
    short
}

fn read_bytes(state: &FatFsState, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
    vfs_read(&state.dev, offset as usize, buf)
}

fn write_bytes(state: &FatFsState, offset: u64, buf: &[u8]) -> VfsResult<usize> {
    vfs_write(&state.dev, offset as usize, buf)
}

fn fat_entry_offset(state: &FatFsState, cluster: u32) -> Option<u64> {
    let off = state.fat_offset + cluster as u64 * 4;
    if off + 4 > state.fat_offset + state.fat_size {
        return None;
    }
    Some(off)
}

fn read_fat_entry(state: &FatFsState, cluster: u32) -> VfsResult<u32> {
    let off = fat_entry_offset(state, cluster).ok_or(VfsError::Invalid)?;
    let mut buf = [0u8; 4];
    read_bytes(state, off, &mut buf)?;
    Ok(u32::from_le_bytes(buf) & 0x0FFF_FFFF)
}

fn write_fat_entry(state: &FatFsState, cluster: u32, val: u32) -> VfsResult {
    let off = fat_entry_offset(state, cluster).ok_or(VfsError::Invalid)?;
    let bytes = (val & 0x0FFF_FFFF).to_le_bytes();
    for idx in 0..state.bpb.num_fats {
        let fat_off = off + idx as u64 * state.fat_size;
        let _ = write_bytes(state, fat_off, &bytes)?;
    }
    Ok(())
}

fn next_cluster(state: &FatFsState, cluster: u32) -> VfsResult<Option<u32>> {
    let val = read_fat_entry(state, cluster)?;
    if val >= 0x0FFF_FFF8 {
        Ok(None)
    } else {
        Ok(Some(val))
    }
}

fn cluster_offset(state: &FatFsState, cluster: u32) -> Option<u64> {
    if cluster < 2 {
        return None;
    }
    let rel = (cluster - 2) as u64 * cluster_size_bytes(&state.bpb);
    Some(state.data_offset + rel)
}

fn read_cluster(state: &FatFsState, cluster: u32, out: &mut [u8]) -> VfsResult<usize> {
    let off = cluster_offset(state, cluster).ok_or(VfsError::Invalid)?;
    let size = cluster_size_bytes(&state.bpb) as usize;
    let to_copy = core::cmp::min(size, out.len());
    let n = read_bytes(state, off, &mut out[..to_copy])?;
    if n != to_copy {
        return Err(VfsError::Io);
    }
    Ok(n)
}

fn write_cluster(state: &FatFsState, cluster: u32, data: &[u8]) -> VfsResult<usize> {
    let off = cluster_offset(state, cluster).ok_or(VfsError::Invalid)?;
    let size = cluster_size_bytes(&state.bpb) as usize;
    let to_copy = core::cmp::min(size, data.len());
    let n = write_bytes(state, off, &data[..to_copy])?;
    if n != to_copy {
        return Err(VfsError::Io);
    }
    Ok(n)
}

fn zero_cluster(state: &FatFsState, cluster: u32) -> VfsResult {
    let size = cluster_size_bytes(&state.bpb) as usize;
    let zeros = vec![0u8; size];
    let _ = write_cluster(state, cluster, &zeros)?;
    Ok(())
}

fn allocate_cluster(state: &FatFsState) -> VfsResult<u32> {
    let max = state.cluster_count + 2;
    let mut cluster = 2u32;
    while cluster < max {
        let val = read_fat_entry(state, cluster)?;
        if val == 0 {
            write_fat_entry(state, cluster, FAT_EOC)?;
            zero_cluster(state, cluster)?;
            print3("fatfs: allocate cluster");
            return Ok(cluster);
        }
        cluster += 1;
    }
    print3("fatfs: no free clusters");
    Err(VfsError::NoDev)
}

fn free_chain(state: &FatFsState, start: u32) -> VfsResult {
    let mut cluster = start;
    loop {
        let next = next_cluster(state, cluster)?;
        write_fat_entry(state, cluster, 0)?;
        if let Some(n) = next {
            cluster = n;
        } else {
            break;
        }
    }
    Ok(())
}

fn list_dir_entries(state: &FatFsState, dir_cluster: u32) -> VfsResult<Vec<(String, FatNode)>> {
    let mut out = Vec::new();
    let mut cluster = dir_cluster;
    let mut buf = vec![0u8; cluster_size_bytes(&state.bpb) as usize];
    print3("fatfs: list_dir_entries");
    let mut lfn_parts: Vec<(u8, String)> = Vec::new();
    let mut lfn_active = false;
    loop {
        read_cluster(state, cluster, &mut buf)?;
        let mut off = 0usize;
        while off + 32 <= buf.len() {
            let ent = &buf[off..off + 32];
            let first = ent[0];
            if first == 0x00 {
                return Ok(out);
            }
            if first == 0xE5 {
                lfn_parts.clear();
                lfn_active = false;
                off += 32;
                continue;
            }
            let attr = ent[11];
            if attr == 0x0F {
                let seq = ent[0] & 0x1F;
                if (ent[0] & 0x40) != 0 {
                    lfn_parts.clear();
                    lfn_active = true;
                }
                if lfn_active {
                    lfn_parts.push((seq, lfn_part_from_entry(ent)));
                }
                off += 32;
                continue;
            }
            let mut name = name_from_entry(ent).unwrap_or_else(|| String::from(""));
            if lfn_active && !lfn_parts.is_empty() {
                lfn_parts.sort_by(|a, b| b.0.cmp(&a.0));
                let mut long_name = String::new();
                for (_, part) in lfn_parts.iter() {
                    long_name.push_str(part);
                }
                if !long_name.is_empty() {
                    name = long_name;
                }
            }
            lfn_parts.clear();
            lfn_active = false;
            if !name.is_empty() {
                if name == "." || name == ".." {
                    off += 32;
                    continue;
                }
                let hi = read_u16_le(ent, 20).unwrap_or(0) as u32;
                let lo = read_u16_le(ent, 26).unwrap_or(0) as u32;
                let first_cluster = (hi << 16) | lo;
                let size = read_u32_le(ent, 28).unwrap_or(0);
                let is_dir = (attr & 0x10) != 0;
                let entry_offset = cluster_offset(state, cluster).unwrap_or(0) + off as u64;
                out.push((
                    name,
                    FatNode {
                        fs: state as *const FatFsState,
                        first_cluster,
                        size,
                        is_dir,
                        entry_offset: Some(entry_offset),
                    },
                ));
            }
            off += 32;
        }
        match next_cluster(state, cluster)? {
            Some(next) => cluster = next,
            None => break,
        }
    }
    Ok(out)
}

fn list_dir_shortnames(state: &FatFsState, dir_cluster: u32) -> VfsResult<Vec<[u8; 11]>> {
    let mut out = Vec::new();
    let mut cluster = dir_cluster;
    let mut buf = vec![0u8; cluster_size_bytes(&state.bpb) as usize];
    loop {
        read_cluster(state, cluster, &mut buf)?;
        let mut off = 0usize;
        while off + 32 <= buf.len() {
            let ent = &buf[off..off + 32];
            let first = ent[0];
            if first == 0x00 {
                return Ok(out);
            }
            if first == 0xE5 {
                off += 32;
                continue;
            }
            let attr = ent[11];
            if attr == 0x0F {
                off += 32;
                continue;
            }
            let mut short = [b' '; 11];
            short.copy_from_slice(&ent[0..11]);
            out.push(short);
            off += 32;
        }
        match next_cluster(state, cluster)? {
            Some(next) => cluster = next,
            None => break,
        }
    }
    Ok(out)
}

fn find_free_dir_entries(state: &FatFsState, dir_cluster: u32, count: usize) -> VfsResult<u64> {
    let mut cluster = dir_cluster;
    let mut buf = vec![0u8; cluster_size_bytes(&state.bpb) as usize];
    let mut run = 0usize;
    let mut run_start = 0usize;
    loop {
        read_cluster(state, cluster, &mut buf)?;
        let mut off = 0usize;
        while off + 32 <= buf.len() {
            let first = buf[off];
            if first == 0x00 || first == 0xE5 {
                if run == 0 {
                    run_start = off;
                }
                run += 1;
                if run >= count {
                    let entry_offset = cluster_offset(state, cluster).unwrap_or(0) + run_start as u64;
                    return Ok(entry_offset);
                }
            } else {
                run = 0;
            }
            off += 32;
        }
        match next_cluster(state, cluster)? {
            Some(next) => {
                cluster = next;
                run = 0;
            }
            None => break,
        }
    }

    let new_cluster = allocate_cluster(state)?;
    write_fat_entry(state, cluster, new_cluster)?;
    write_fat_entry(state, new_cluster, FAT_EOC)?;
    zero_cluster(state, new_cluster)?;
    Ok(cluster_offset(state, new_cluster).unwrap_or(0))
}

fn find_free_dir_entry(state: &FatFsState, dir_cluster: u32) -> VfsResult<u64> {
    let mut cluster = dir_cluster;
    let mut buf = vec![0u8; cluster_size_bytes(&state.bpb) as usize];
    print3("fatfs: find_free_dir_entry");
    loop {
        read_cluster(state, cluster, &mut buf)?;
        let mut off = 0usize;
        while off + 32 <= buf.len() {
            let first = buf[off];
            if first == 0x00 || first == 0xE5 {
                let entry_offset = cluster_offset(state, cluster).unwrap_or(0) + off as u64;
                return Ok(entry_offset);
            }
            off += 32;
        }
        match next_cluster(state, cluster)? {
            Some(next) => cluster = next,
            None => break,
        }
    }

    let new_cluster = allocate_cluster(state)?;
    write_fat_entry(state, cluster, new_cluster)?;
    write_fat_entry(state, new_cluster, FAT_EOC)?;
    zero_cluster(state, new_cluster)?;
    Ok(cluster_offset(state, new_cluster).unwrap_or(0))
}

fn write_dir_entry(state: &FatFsState, offset: u64, entry: &[u8; 32]) -> VfsResult {
    let _ = write_bytes(state, offset, entry)?;
    Ok(())
}

fn read_dir_entry(state: &FatFsState, offset: u64) -> VfsResult<[u8; 32]> {
    let mut buf = [0u8; 32];
    read_bytes(state, offset, &mut buf)?;
    Ok(buf)
}

fn build_dir_entry(name: &[u8; 11], attr: u8, first_cluster: u32, size: u32) -> [u8; 32] {
    let mut entry = [0u8; 32];
    entry[0..11].copy_from_slice(name);
    entry[11] = attr;
    write_u16_le(&mut entry, 20, (first_cluster >> 16) as u16);
    write_u16_le(&mut entry, 26, (first_cluster & 0xFFFF) as u16);
    write_u32_le(&mut entry, 28, size);
    entry
}

fn delete_lfn_chain(state: &FatFsState, short_offset: u64) -> VfsResult {
    if short_offset < 32 {
        return Ok(());
    }
    let short_entry = read_dir_entry(state, short_offset)?;
    let mut short = [0u8; 11];
    short.copy_from_slice(&short_entry[0..11]);
    let chk = lfn_checksum(&short);

    let mut off = short_offset - 32;
    loop {
        let mut ent = read_dir_entry(state, off)?;
        if ent[11] != 0x0F || ent[13] != chk {
            break;
        }
        ent[0] = 0xE5;
        write_dir_entry(state, off, &ent)?;
        if off < 32 {
            break;
        }
        off -= 32;
    }
    Ok(())
}

fn create_node(parent: &Arc<VfsNode>, name: &str, node_info: FatNode) -> Arc<VfsNode> {
    if let Some(existing) = vfs_child_find(parent, name) {
        return existing;
    }
    let node = vfs_child_append(parent, name);
    set_handle(&node, node_info);
    let mut meta = node.meta.lock();
    meta.node_type = if node_info.is_dir {
        VfsNodeType::Dir
    } else {
        VfsNodeType::Stream
    };
    meta.size = node_info.size as u64;
    drop(meta);
    node
}

fn set_handle(node: &Arc<VfsNode>, handle: FatNode) {
    let ptr = Box::into_raw(Box::new(handle));
    node.meta.lock().handle = Some(ptr as usize);
}

fn get_handle(node: &Arc<VfsNode>) -> VfsResult<&'static mut FatNode> {
    let ptr = node.meta.lock().handle.ok_or(VfsError::Invalid)? as *mut FatNode;
    if ptr.is_null() {
        return Err(VfsError::Invalid);
    }
    Ok(unsafe { &mut *ptr })
}

fn drop_handle(node: &Arc<VfsNode>) {
    let ptr = node.meta.lock().handle.take().unwrap_or(0) as *mut FatNode;
    if !ptr.is_null() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    }
}

fn populate_dir(state: &FatFsState, dir: &Arc<VfsNode>, dir_cluster: u32, depth: usize) -> VfsResult {
    if depth > 32 {
        return Ok(());
    }
    let entries = list_dir_entries(state, dir_cluster)?;
    for (name, child) in entries {
        let node = create_node(dir, &name, child);
        if child.is_dir {
            populate_dir(state, &node, child.first_cluster, depth + 1)?;
        }
    }
    Ok(())
}

fn update_entry_cluster_and_size(state: &FatFsState, handle: &FatNode) -> VfsResult {
    let Some(offset) = handle.entry_offset else { return Ok(()) };
    let mut entry = read_dir_entry(state, offset)?;
    write_u16_le(&mut entry, 20, (handle.first_cluster >> 16) as u16);
    write_u16_le(&mut entry, 26, (handle.first_cluster & 0xFFFF) as u16);
    write_u32_le(&mut entry, 28, handle.size);
    write_dir_entry(state, offset, &entry)
}

fn ensure_cluster_for_index(state: &FatFsState, mut first: u32, idx: usize) -> VfsResult<(u32, u32)> {
    let mut cluster = first;
    if cluster == 0 {
        cluster = allocate_cluster(state)?;
        first = cluster;
    }
    let mut i = 0usize;
    while i < idx {
        match next_cluster(state, cluster)? {
            Some(next) => cluster = next,
            None => {
                let new_cluster = allocate_cluster(state)?;
                write_fat_entry(state, cluster, new_cluster)?;
                write_fat_entry(state, new_cluster, FAT_EOC)?;
                cluster = new_cluster;
            }
        }
        i += 1;
    }
    Ok((first, cluster))
}

fn write_dot_entries(state: &FatFsState, dir_cluster: u32, parent_cluster: u32) -> VfsResult {
    let mut dot = [b' '; 11];
    dot[0] = b'.';
    let mut dotdot = [b' '; 11];
    dotdot[0] = b'.';
    dotdot[1] = b'.';
    let e1 = build_dir_entry(&dot, 0x10, dir_cluster, 0);
    let e2 = build_dir_entry(&dotdot, 0x10, parent_cluster, 0);
    let base = cluster_offset(state, dir_cluster).ok_or(VfsError::Invalid)?;
    write_dir_entry(state, base, &e1)?;
    write_dir_entry(state, base + 32, &e2)?;
    Ok(())
}

fn create_entry(
    parent: &Arc<VfsNode>,
    name: &str,
    is_dir: bool,
    node: &Arc<VfsNode>,
) -> VfsResult {
    print3("fatfs: create_entry");
    let parent_handle = get_handle(parent)?;
    if !parent_handle.is_dir {
        return Err(VfsError::NotDir);
    }
    let state = unsafe { &*parent_handle.fs };
    if is_dir {
        print3("fatfs: mkdir");
    } else {
        print3("fatfs: mkfile");
    }
    let short_opt = to_short_name(name);
    let needs_lfn = short_opt.is_none();
    let short = if let Some(s) = short_opt {
        s
    } else {
        to_short_name_auto(state, parent_handle.first_cluster, name)
    };

    print3("fatfs: create_entry list_dir");
    let existing = list_dir_entries(state, parent_handle.first_cluster)?;
    print3("fatfs: create_entry list_dir ok");
    for (entry_name, _) in existing {
        if name_eq(&entry_name, name) {
            return Err(VfsError::Exists);
        }
    }

    print3("fatfs: create_entry find_free");
    let lfn_entries = if needs_lfn { build_lfn_entries(name, &short) } else { Vec::new() };
    let total_entries = 1 + lfn_entries.len();
    let entry_offset = find_free_dir_entries(state, parent_handle.first_cluster, total_entries)?;
    print3("fatfs: create_entry find_free ok");

    let mut first_cluster = 0u32;
    if is_dir {
        first_cluster = allocate_cluster(state)?;
        write_dot_entries(state, first_cluster, parent_handle.first_cluster)?;
    }

    let attr = if is_dir { 0x10 } else { 0x20 };
    let entry = build_dir_entry(&short, attr, first_cluster, 0);
    if needs_lfn {
        for (i, lfn) in lfn_entries.iter().enumerate() {
            let off = entry_offset + (i as u64) * 32;
            write_dir_entry(state, off, lfn)?;
        }
        let short_off = entry_offset + (lfn_entries.len() as u64) * 32;
        write_dir_entry(state, short_off, &entry)?;
        set_handle(
            node,
            FatNode {
                fs: state as *const FatFsState,
                first_cluster,
                size: 0,
                is_dir,
                entry_offset: Some(short_off),
            },
        );
    } else {
        write_dir_entry(state, entry_offset, &entry)?;
        set_handle(
            node,
            FatNode {
                fs: state as *const FatFsState,
                first_cluster,
                size: 0,
                is_dir,
                entry_offset: Some(entry_offset),
            },
        );
    }

    let mut meta = node.meta.lock();
    meta.node_type = if is_dir { VfsNodeType::Dir } else { VfsNodeType::Stream };
    meta.size = 0;
    Ok(())
}

pub struct FatFs;

impl FatFs {
    pub fn new() -> Self {
        Self
    }

    fn build_state(src: &str) -> VfsResult<FatFsState> {
        let dev = vfs_open(src).map_err(|_| VfsError::NotFound)?;
        let mut sector = [0u8; 512];
        vfs_read(&dev, 0, &mut sector).map_err(|_| VfsError::Io)?;
        let bpb = parse_bpb(&sector).ok_or(VfsError::Invalid)?;
        if bpb.root_entry_count != 0 {
            return Err(VfsError::NotSupported);
        }

        let bytes_per_sector = bpb.bytes_per_sector as u64;
        let fat_offset = bpb.reserved_sectors as u64 * bytes_per_sector;
        let fat_size = bpb.fat_size_sectors as u64 * bytes_per_sector;
        let root_dir_sectors = ((bpb.root_entry_count as u32 * 32)
            + (bpb.bytes_per_sector as u32 - 1))
            / bpb.bytes_per_sector as u32;
        let data_offset = (bpb.reserved_sectors as u64
            + bpb.num_fats as u64 * bpb.fat_size_sectors as u64
            + root_dir_sectors as u64)
            * bytes_per_sector;

        let data_sectors = bpb.total_sectors
            - (bpb.reserved_sectors as u32
                + bpb.num_fats as u32 * bpb.fat_size_sectors
                + root_dir_sectors);
        let cluster_count = data_sectors / bpb.sectors_per_cluster as u32;

        Ok(FatFsState {
            dev,
            bpb,
            fat_offset,
            fat_size,
            data_offset,
            cluster_count,
        })
    }
}

impl VfsOps for FatFs {
    fn mount(&self, src: Option<&str>, node: &Arc<VfsNode>) -> VfsResult {
        let Some(src) = src else { return Err(VfsError::Invalid) };
        let state = Self::build_state(src)?;
        let state = Box::leak(Box::new(state));

        set_handle(
            node,
            FatNode {
                fs: state as *const FatFsState,
                first_cluster: state.bpb.root_cluster,
                size: 0,
                is_dir: true,
                entry_offset: None,
            },
        );
        node.meta.lock().node_type = VfsNodeType::Dir;
        populate_dir(state, node, state.bpb.root_cluster, 0)?;
        Ok(())
    }

    fn unmount(&self, node: &Arc<VfsNode>) -> VfsResult {
        fn walk(n: &Arc<VfsNode>) {
            let children = n.meta.lock().children.clone();
            for c in children {
                walk(&c);
            }
            drop_handle(n);
        }
        walk(node);
        Ok(())
    }

    fn open(&self, parent: Option<&Arc<VfsNode>>, name: &str, node: &Arc<VfsNode>) -> VfsResult {
        let Some(parent) = parent else { return Err(VfsError::NotFound) };
        let parent_handle = get_handle(parent)?;
        if !parent_handle.is_dir {
            return Err(VfsError::NotDir);
        }
        let state = unsafe { &*parent_handle.fs };
        let entries = list_dir_entries(state, parent_handle.first_cluster)?;
        for (entry_name, entry_node) in entries {
            if name_eq(&entry_name, name) {
                set_handle(node, entry_node);
                let mut meta = node.meta.lock();
                meta.node_type = if entry_node.is_dir {
                    VfsNodeType::Dir
                } else {
                    VfsNodeType::Stream
                };
                meta.size = entry_node.size as u64;
                if entry_node.is_dir {
                    drop(meta);
                    populate_dir(state, node, entry_node.first_cluster, 0)?;
                }
                return Ok(());
            }
        }
        Err(VfsError::NotFound)
    }

    fn mkdir(&self, parent: &Arc<VfsNode>, name: &str, node: &Arc<VfsNode>) -> VfsResult {
        create_entry(parent, name, true, node)
    }

    fn mkfile(&self, parent: &Arc<VfsNode>, name: &str, node: &Arc<VfsNode>) -> VfsResult {
        print3("fatfs: mkfile enter");
        create_entry(parent, name, false, node)
    }

    fn read(&self, node: &Arc<VfsNode>, offset: usize, buf: &mut [u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        if handle.is_dir {
            return Err(VfsError::Invalid);
        }
        if handle.first_cluster == 0 || handle.size == 0 {
            return Ok(0);
        }
        let state = unsafe { &*handle.fs };
        let size = handle.size as usize;
        if offset >= size {
            return Ok(0);
        }
        let mut remaining = core::cmp::min(buf.len(), size - offset);
        let mut written = 0usize;
        let cluster_size = cluster_size_bytes(&state.bpb) as usize;

        let mut cluster = handle.first_cluster;
        let mut skip = offset;
        while skip >= cluster_size {
            if let Some(next) = next_cluster(state, cluster)? {
                cluster = next;
                skip -= cluster_size;
            } else {
                return Ok(0);
            }
        }

        let mut cluster_buf = vec![0u8; cluster_size];
        loop {
            read_cluster(state, cluster, &mut cluster_buf)?;
            let start = skip;
            let available = cluster_buf.len().saturating_sub(start);
            let take = core::cmp::min(available, remaining);
            if take > 0 {
                buf[written..written + take]
                    .copy_from_slice(&cluster_buf[start..start + take]);
                written += take;
                remaining -= take;
            }
            if remaining == 0 {
                break;
            }
            skip = 0;
            match next_cluster(state, cluster)? {
                Some(next) => cluster = next,
                None => break,
            }
        }
        Ok(written)
    }

    fn write(&self, node: &Arc<VfsNode>, offset: usize, buf: &[u8]) -> VfsResult<usize> {
        let handle = get_handle(node)?;
        if handle.is_dir {
            return Err(VfsError::Invalid);
        }
        let state = unsafe { &*handle.fs };
        print3("fatfs: write");
        if buf.is_empty() {
            return Ok(0);
        }

        let cluster_size = cluster_size_bytes(&state.bpb) as usize;
        let start_idx = offset / cluster_size;
        let end_offset = offset + buf.len();
        let end_idx = (end_offset + cluster_size - 1) / cluster_size;

        let mut first = handle.first_cluster;
        for idx in start_idx..end_idx {
            let (new_first, cur) = ensure_cluster_for_index(state, first, idx)?;
            first = new_first;
            let cluster = cur;

            let cluster_off = idx * cluster_size;
            let start = if offset > cluster_off {
                offset - cluster_off
            } else {
                0
            };
            let end = core::cmp::min(cluster_size, end_offset.saturating_sub(cluster_off));
            let slice_start = idx * cluster_size + start - offset;
            let slice_end = slice_start + (end - start);

            let mut cluster_buf = vec![0u8; cluster_size];
            read_cluster(state, cluster, &mut cluster_buf)?;
            cluster_buf[start..end].copy_from_slice(&buf[slice_start..slice_end]);
            write_cluster(state, cluster, &cluster_buf)?;
        }

        handle.first_cluster = first;
        let new_size = core::cmp::max(handle.size as usize, end_offset) as u32;
        handle.size = new_size;
        update_entry_cluster_and_size(state, handle)?;
        let mut meta = node.meta.lock();
        meta.size = new_size as u64;
        Ok(buf.len())
    }

    fn stat(&self, node: &Arc<VfsNode>) -> VfsResult {
        let handle = get_handle(node)?;
        let mut meta = node.meta.lock();
        meta.node_type = if handle.is_dir {
            VfsNodeType::Dir
        } else {
            VfsNodeType::Stream
        };
        meta.size = handle.size as u64;
        Ok(())
    }

    fn delete(&self, parent: &Arc<VfsNode>, node: &Arc<VfsNode>) -> VfsResult {
        let handle = get_handle(node)?;
        if handle.is_dir {
            let state = unsafe { &*handle.fs };
            let entries = list_dir_entries(state, handle.first_cluster)?;
            if !entries.is_empty() {
                return Err(VfsError::NotEmpty);
            }
        }
        let state = unsafe { &*handle.fs };
        if let Some(offset) = handle.entry_offset {
            let mut entry = read_dir_entry(state, offset)?;
            entry[0] = 0xE5;
            write_dir_entry(state, offset, &entry)?;
        }
        if handle.first_cluster != 0 {
            free_chain(state, handle.first_cluster)?;
        }
        drop_handle(node);
        parent.meta.lock().children.retain(|c| !Arc::ptr_eq(c, node));
        Ok(())
    }

    fn rename(&self, node: &Arc<VfsNode>, new_name: &str) -> VfsResult {
        let handle = get_handle(node)?;
        let state = unsafe { &*handle.fs };
        let Some(offset) = handle.entry_offset else { return Err(VfsError::Invalid) };
        let parent = node
            .meta
            .lock()
            .parent
            .as_ref()
            .and_then(|p| p.upgrade())
            .ok_or(VfsError::Invalid)?;
        let parent_handle = get_handle(&parent)?;
        if !parent_handle.is_dir {
            return Err(VfsError::NotDir);
        }

        let short_opt = to_short_name(new_name);
        let needs_lfn = short_opt.is_none();
        let short = if let Some(s) = short_opt {
            s
        } else {
            to_short_name_auto(state, parent_handle.first_cluster, new_name)
        };

        let existing = list_dir_entries(state, parent_handle.first_cluster)?;
        for (entry_name, _) in existing {
            if name_eq(&entry_name, new_name) {
                return Err(VfsError::Exists);
            }
        }

        let lfn_entries = if needs_lfn { build_lfn_entries(new_name, &short) } else { Vec::new() };
        let total_entries = 1 + lfn_entries.len();
        let entry_offset = find_free_dir_entries(state, parent_handle.first_cluster, total_entries)?;

        let attr = if handle.is_dir { 0x10 } else { 0x20 };
        let entry = build_dir_entry(&short, attr, handle.first_cluster, handle.size);
        if needs_lfn {
            for (i, lfn) in lfn_entries.iter().enumerate() {
                let off = entry_offset + (i as u64) * 32;
                write_dir_entry(state, off, lfn)?;
            }
            let short_off = entry_offset + (lfn_entries.len() as u64) * 32;
            write_dir_entry(state, short_off, &entry)?;
            handle.entry_offset = Some(short_off);
        } else {
            write_dir_entry(state, entry_offset, &entry)?;
            handle.entry_offset = Some(entry_offset);
        }

        // delete old entry + its LFN chain
        delete_lfn_chain(state, offset)?;
        let mut old = read_dir_entry(state, offset)?;
        old[0] = 0xE5;
        write_dir_entry(state, offset, &old)?;

        node.meta.lock().name = String::from(new_name);
        Ok(())
    }

    fn free(&self, node: &Arc<VfsNode>) -> VfsResult {
        vfs_free(node);
        Ok(())
    }
}

fn print3(msg: &str) {
    serial::write_bytes(msg.as_bytes());
    serial::write_bytes(b"\n");
}
