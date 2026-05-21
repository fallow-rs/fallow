//! Cross-platform path classification helpers.
//!
//! Rust's [`std::path::Path::is_absolute`] uses platform-specific semantics: on
//! Unix a path starting with `/` is absolute, on Windows a path needs a drive
//! prefix (`C:\foo`) or a UNC root (`\\?\C:\foo`). A POSIX-style absolute path
//! like `/project/foo.ts` returns `false` from `is_absolute()` on Windows,
//! which breaks any code that conditionally joins relative paths against a
//! root: the POSIX-shaped input is treated as relative and joined under root,
//! producing wrong results.
//!
//! This module exposes path-classification helpers that recognise BOTH
//! platforms' conventions regardless of host. Use them whenever the input
//! path may originate from user-supplied data shared across CI runners (CLI
//! flags, config files, source maps, diff output) and the calling code needs
//! to recognise it as already-anchored before joining against a root.

use std::path::{Component, Path};

/// Returns `true` if `path` is anchored under either platform's path
/// conventions.
///
/// Recognises three shapes regardless of host:
///
/// - **Host absolute** (`Path::is_absolute()` returns `true`). On Unix this
///   means `/foo`; on Windows this means `C:\foo`, `\\?\C:\foo`, `\\server\share\foo`.
/// - **POSIX-style root** (`/foo`, `/project/src/a.ts`). Recognised via
///   [`Component::RootDir`] which appears as the first component on both
///   platforms regardless of whether `is_absolute()` returned `true`. This is
///   the case Rust's `is_absolute()` misses on Windows.
/// - **Windows-style drive prefix** (`C:\foo`, `c:/foo`). Recognised via a
///   byte-level scan of the path's `OsStr` encoding so it works on Unix hosts
///   too (a source-map file authored on Windows can contain `C:/foo` strings
///   that a Unix-hosted analysis still needs to classify correctly).
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
/// (drive letter + colon + separator).
///
/// String-shaped variant for callers that have the raw `&str` (typically from
/// parsing a source map, file URL, or config field) before constructing a
/// `PathBuf`. The two helpers exist side-by-side so neither caller has to
/// pay an unnecessary `PathBuf::from` round-trip.
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

    #[test]
    fn host_absolute_works_through_is_absolute() {
        // `current_dir` always returns a host-absolute path; the helper must
        // agree with `Path::is_absolute()` on those inputs.
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
        // Round-trip a Windows drive-prefixed string through PathBuf and
        // confirm the helper still recognises it on the test-host platform.
        let p = PathBuf::from("E:/source/map.js");
        assert!(is_absolute_path_any_platform(&p));
    }
}
