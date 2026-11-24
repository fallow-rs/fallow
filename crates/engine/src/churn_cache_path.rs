//! Platform-native byte form of the paths in the churn cache.

use std::path::{Path, PathBuf};

#[cfg(unix)]
pub(super) fn path_to_cache_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "Windows rejects truncated UTF-16 bytes through this shared fallible contract"
)]
pub(super) fn path_from_cache_bytes(path: &[u8]) -> Option<PathBuf> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    Some(PathBuf::from(OsStr::from_bytes(path)))
}

#[cfg(windows)]
pub(super) fn path_to_cache_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
pub(super) fn path_from_cache_bytes(path: &[u8]) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let (chunks, remainder) = path.as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    let wide: Vec<u16> = chunks
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    Some(PathBuf::from(OsString::from_wide(&wide)))
}
