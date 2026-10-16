//! The security invariant for config globs, written without the validator so
//! the fuzz targets can check the validator against it.

/// Returns the reason when `pattern`, joined onto a project root, can name a
/// path outside that root. The check uses both POSIX and Windows rules, because
/// a config file written on one platform runs on the other.
pub fn escape_reason(pattern: &str) -> Option<&'static str> {
    if std::path::Path::new(pattern).has_root() {
        return Some("rooted path");
    }
    let posix = pattern.replace('\\', "/");
    if posix.starts_with('/') {
        return Some("rooted path after backslash normalization");
    }
    let bytes = posix.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Some("Windows drive prefix");
    }
    let mut depth: i64 = 0;
    for segment in posix.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return Some("parent segment above the root");
                }
            }
            _ => depth += 1,
        }
    }
    None
}
