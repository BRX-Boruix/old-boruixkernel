use alloc::string::String;
use alloc::vec::Vec;

pub fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return String::from("/");
    }
    let is_abs = path.as_bytes()[0] == b'/';
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if !parts.is_empty() {
                parts.pop();
            }
            continue;
        }
        parts.push(part);
    }
    let mut out = String::new();
    if is_abs {
        out.push('/');
    }
    out.push_str(&parts.join("/"));
    if out.is_empty() {
        out.push('/');
    }
    out
}

pub fn path_join(base: &str, rel: &str) -> String {
    if rel.starts_with('/') {
        return normalize_path(rel);
    }
    let mut tmp = String::from(base);
    if !tmp.ends_with('/') {
        tmp.push('/');
    }
    tmp.push_str(rel);
    normalize_path(&tmp)
}
