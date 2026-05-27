//! Cross-platform path classification helpers.
//!
//! Rust's [`std::path::Path::is_absolute`] uses platform-specific semantics:
//! on Unix a path starting with `/` is absolute, while on Windows a path needs
//! a drive prefix (`C:\foo`) or a UNC root (`\\?\C:\foo`). A POSIX-style
//! absolute path like `/project/foo.ts` returns `false` from `is_absolute()`
//! on Windows, which breaks code that conditionally joins relative paths
//! against a root.
//!
//! Use these helpers whenever input paths may originate from user-supplied
//! data shared across CI runners, config files, source maps, or diff output.

use std::path::{Component, Path};

/// Returns `true` if `path` is anchored under either platform's path
/// conventions.
///
/// Recognises host-absolute paths, POSIX-style rooted paths (`/foo`), and
/// Windows drive-prefixed paths (`C:\foo`, `c:/foo`) regardless of the host OS.
pub fn is_absolute_path_any_platform(path: &Path) -> bool {
    if path.is_absolute() {
        return true;
    }
    if matches!(path.components().next(), Some(Component::RootDir)) {
        return true;
    }
    looks_like_windows_drive_absolute(path.as_os_str().as_encoded_bytes())
}

/// Returns `true` if `value` looks like a Windows-style absolute path
/// with a drive letter, colon, and path separator.
///
/// This string-shaped variant is useful before constructing a [`Path`].
pub fn looks_like_windows_absolute_path(value: &str) -> bool {
    looks_like_windows_drive_absolute(value.as_bytes())
}

fn looks_like_windows_drive_absolute(bytes: &[u8]) -> bool {
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn posix_style_root_is_absolute_on_any_platform() {
        assert!(is_absolute_path_any_platform(Path::new(
            "/project/src/a.ts"
        )));
        assert!(is_absolute_path_any_platform(Path::new("/foo")));
        assert!(is_absolute_path_any_platform(Path::new("/")));
    }

    #[test]
    fn windows_drive_letter_is_absolute_on_any_platform() {
        assert!(is_absolute_path_any_platform(Path::new(
            "C:\\project\\src\\a.ts"
        )));
        assert!(is_absolute_path_any_platform(Path::new(
            "C:/project/src/a.ts"
        )));
        assert!(is_absolute_path_any_platform(Path::new("d:/foo")));
    }

    #[test]
    fn relative_paths_return_false() {
        assert!(!is_absolute_path_any_platform(Path::new("src/a.ts")));
        assert!(!is_absolute_path_any_platform(Path::new("./src/a.ts")));
        assert!(!is_absolute_path_any_platform(Path::new("../parent/a.ts")));
        assert!(!is_absolute_path_any_platform(Path::new("a.ts")));
        assert!(!is_absolute_path_any_platform(Path::new("")));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn host_absolute_works_through_is_absolute() {
        let cwd = std::env::current_dir().expect("current_dir");
        assert!(is_absolute_path_any_platform(&cwd));
    }

    #[test]
    fn looks_like_windows_absolute_path_recognises_drive_shapes() {
        assert!(looks_like_windows_absolute_path("C:\\foo"));
        assert!(looks_like_windows_absolute_path("c:/foo"));
        assert!(looks_like_windows_absolute_path("Z:/very/deep/path.ts"));
        assert!(!looks_like_windows_absolute_path("/foo"));
        assert!(!looks_like_windows_absolute_path("src/foo"));
        assert!(!looks_like_windows_absolute_path("C:"));
        assert!(!looks_like_windows_absolute_path("CC:/foo"));
        assert!(!looks_like_windows_absolute_path(""));
    }

    #[test]
    fn drive_prefix_path_string_is_absolute_via_os_str_bytes() {
        let p = PathBuf::from("E:/source/map.js");
        assert!(is_absolute_path_any_platform(&p));
    }
}
