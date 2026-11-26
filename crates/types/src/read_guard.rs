//! Bounded reads of project files.
//!
//! fallow reads project files such as a config file, a `tsconfig.json` or a
//! `CODEOWNERS` file in full. The helpers here read such a file only when it
//! resolves inside the project root, is a regular file, and fits a size
//! limit. A link that resolves inside the root stays allowed. A device or a
//! very large file is refused before the read, so the read cannot block or
//! fill memory.
//!
//! Each check fails closed: when the helper cannot decide, it refuses.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The default size limit for a project file that fallow reads in full.
///
/// Config files, `tsconfig.json` files, rule packs and `CODEOWNERS` files of
/// large monorepos stay well below 1 MiB. 16 MiB leaves room for configs
/// that tools write, and still stops a read of a device or a multi-gigabyte
/// file.
pub const DEFAULT_MAX_READ_BYTES: u64 = 16 * 1024 * 1024;

/// The reason that a bounded read refused a file.
///
/// The `Display` text names the path and the reason. It never contains
/// content of the file.
#[derive(Debug)]
pub enum ReadRefusal {
    /// The path, after symlinks are resolved, is outside the root.
    OutsideRoot {
        /// The path as the caller gave it.
        path: PathBuf,
        /// The root that the path must stay in.
        root: PathBuf,
    },
    /// The path is not a regular file, for example a directory or a device.
    NotRegularFile {
        /// The path as the caller gave it.
        path: PathBuf,
    },
    /// The file is larger than the limit.
    TooLarge {
        /// The path as the caller gave it.
        path: PathBuf,
        /// The limit in bytes.
        max_bytes: u64,
    },
    /// The path or the root does not resolve, or the read failed.
    Io {
        /// The path as the caller gave it.
        path: PathBuf,
        /// The error of the file system call.
        source: std::io::Error,
    },
}

impl ReadRefusal {
    /// The path as the caller gave it.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::OutsideRoot { path, .. }
            | Self::NotRegularFile { path }
            | Self::TooLarge { path, .. }
            | Self::Io { path, .. } => path,
        }
    }

    /// True when the file does not exist. A link on the path that does not
    /// resolve gives [`ReadRefusal::OutsideRoot`], not this state, so a
    /// dangling link and a link to a file outside the root look the same.
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
    }
}

impl fmt::Display for ReadRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideRoot { path, root } => write!(
                f,
                "refusing to read {}: it resolves to a location outside {}",
                path.display(),
                root.display()
            ),
            Self::NotRegularFile { path } => write!(
                f,
                "refusing to read {}: it is not a regular file",
                path.display()
            ),
            Self::TooLarge { path, max_bytes } => write!(
                f,
                "refusing to read {}: it is larger than {max_bytes} bytes",
                path.display()
            ),
            Self::Io { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ReadRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// True when `path`, after symlinks are resolved, is `root` or a path below
/// the resolved `root`. A relative `path` is taken relative to `root`.
///
/// Returns `false` when `root` or `path` does not resolve, for example a
/// missing file, a dangling link or a path that is too long.
#[must_use]
pub fn is_inside_root(root: &Path, path: &Path) -> bool {
    let Ok(canonical_root) = std::fs::canonicalize(root) else {
        return false;
    };
    std::fs::canonicalize(root.join(path))
        .is_ok_and(|resolved| resolved.starts_with(&canonical_root))
}

/// Read `path` in full when it resolves inside `root`, is a regular file and
/// has at most `max_bytes` bytes. A relative `path` is taken relative to
/// `root`. Use [`DEFAULT_MAX_READ_BYTES`] when no other limit applies.
///
/// # Errors
///
/// Returns a [`ReadRefusal`] when a check fails or the read fails.
pub fn read_bytes_inside(root: &Path, path: &Path, max_bytes: u64) -> Result<Vec<u8>, ReadRefusal> {
    let joined = root.join(path);
    let io = |source: std::io::Error| ReadRefusal::Io {
        path: path.to_path_buf(),
        source,
    };
    let outside = || ReadRefusal::OutsideRoot {
        path: path.to_path_buf(),
        root: root.to_path_buf(),
    };
    let canonical_root = std::fs::canonicalize(root).map_err(io)?;
    let resolved = match std::fs::canonicalize(&joined) {
        Ok(resolved) => resolved,
        Err(_) if unresolved_is_outside(&canonical_root, &joined) => return Err(outside()),
        Err(source) => return Err(io(source)),
    };
    if !resolved.starts_with(&canonical_root) {
        return Err(outside());
    }
    read_resolved(path, &resolved, max_bytes)
}

/// True when `joined`, a path that does not resolve, names a place outside
/// `canonical_root` or ends in a dangling link. The longest leading part of
/// `joined` that resolves is resolved, and the rest is taken by text, because
/// no link exists below a missing entry. A dangling link at the first missing
/// entry counts as outside, so a caller cannot tell a dangling link from a
/// link to a file outside the root.
fn unresolved_is_outside(canonical_root: &Path, joined: &Path) -> bool {
    use std::path::Component;
    let components: Vec<Component<'_>> = joined.components().collect();
    let Some((resolved_len, mut current)) = (0..=components.len()).rev().find_map(|len| {
        let prefix: PathBuf = components[..len].iter().collect();
        std::fs::canonicalize(&prefix)
            .ok()
            .map(|resolved| (len, resolved))
    }) else {
        return true;
    };
    if !current.starts_with(canonical_root) {
        return true;
    }
    for (index, component) in components[resolved_len..].iter().enumerate() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
                if !current.starts_with(canonical_root) {
                    return true;
                }
            }
            Component::Normal(name) => {
                current.push(name);
                let dangling = index == 0
                    && current
                        .symlink_metadata()
                        .is_ok_and(|meta| meta.file_type().is_symlink());
                if dangling {
                    return true;
                }
            }
            Component::RootDir | Component::Prefix(_) => return true,
        }
    }
    !current.starts_with(canonical_root)
}

/// [`read_bytes_inside`] that also requires valid UTF-8.
///
/// # Errors
///
/// Returns a [`ReadRefusal`] when a check fails, the read fails, or the
/// content is not valid UTF-8.
pub fn read_to_string_inside(
    root: &Path,
    path: &Path,
    max_bytes: u64,
) -> Result<String, ReadRefusal> {
    let bytes = read_bytes_inside(root, path, max_bytes)?;
    into_string(path, bytes)
}

/// Read `path` in full when it is a regular file with at most `max_bytes`
/// bytes, with no root check. Use this only for a path that may leave the
/// project by design, for example an absolute `extends` value of the user.
///
/// # Errors
///
/// Returns a [`ReadRefusal`] when a check fails, the read fails, or the
/// content is not valid UTF-8.
pub fn read_to_string_bounded(path: &Path, max_bytes: u64) -> Result<String, ReadRefusal> {
    let bytes = read_resolved(path, path, max_bytes)?;
    into_string(path, bytes)
}

/// [`read_to_string_bounded`] for bytes: read `path` in full when it is a
/// regular file with at most `max_bytes` bytes, with no root check.
///
/// # Errors
///
/// Returns a [`ReadRefusal`] when a check fails or the read fails.
pub fn read_bytes_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, ReadRefusal> {
    read_resolved(path, path, max_bytes)
}

/// Whether `path` is a regular file with at most `max_bytes` bytes, after a
/// link resolves. A bounded read of such a file does not refuse it for its
/// type or size.
#[must_use]
pub fn is_bounded_regular_file(path: &Path, max_bytes: u64) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() <= max_bytes)
}

fn read_resolved(path: &Path, resolved: &Path, max_bytes: u64) -> Result<Vec<u8>, ReadRefusal> {
    let io = |source: std::io::Error| ReadRefusal::Io {
        path: path.to_path_buf(),
        source,
    };
    let not_regular = || ReadRefusal::NotRegularFile {
        path: path.to_path_buf(),
    };
    let too_large = || ReadRefusal::TooLarge {
        path: path.to_path_buf(),
        max_bytes,
    };
    // The check before the open skips a device or a FIFO without opening it.
    // The path can change between that check and the open, so the opened
    // file is checked again. The open does not wait for a FIFO writer.
    if !std::fs::metadata(resolved).map_err(io)?.is_file() {
        return Err(not_regular());
    }
    let file = open_without_blocking(resolved).map_err(io)?;
    let meta = file.metadata().map_err(io)?;
    if !meta.is_file() {
        return Err(not_regular());
    }
    if meta.len() > max_bytes {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    // The file can grow after the metadata call, so the read stops one byte
    // past the limit and refuses the file when it reaches that byte.
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > max_bytes {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Open `path` for reading. On Unix the open does not wait for the writer of
/// a FIFO, so a FIFO that replaces a checked file cannot hang the read.
fn open_without_blocking(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed());
    }
    options.open(path)
}

fn into_string(path: &Path, bytes: Vec<u8>) -> Result<String, ReadRefusal> {
    String::from_utf8(bytes).map_err(|_| ReadRefusal::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The checked open does not wait for a FIFO writer, and the check of
    /// the opened file refuses what is not a regular file.
    #[test]
    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn the_open_does_not_wait_for_a_fifo_and_checks_the_opened_file() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe.json");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        let file = open_without_blocking(&fifo).unwrap();
        assert!(!file.metadata().unwrap().is_file());
        assert!(matches!(
            read_to_string_bounded(&fifo, 16),
            Err(ReadRefusal::NotRegularFile { .. })
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn reads_a_regular_file_inside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.json"), "{}").unwrap();
        let text = read_to_string_inside(dir.path(), Path::new("a.json"), 16).unwrap();
        assert_eq!(text, "{}");
        let abs = dir.path().join("a.json");
        assert_eq!(read_bytes_inside(dir.path(), &abs, 16).unwrap(), b"{}");
        assert!(is_inside_root(dir.path(), &abs));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn bounded_bytes_read_takes_regular_files_within_the_limit_only() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.json");
        std::fs::write(&file, "{}").unwrap();
        assert_eq!(read_bytes_bounded(&file, 16).unwrap(), b"{}");
        assert!(is_bounded_regular_file(&file, 16));
        assert!(read_bytes_bounded(&file, 1).is_err());
        assert!(!is_bounded_regular_file(&file, 1));
        assert!(read_bytes_bounded(dir.path(), 16).is_err());
        assert!(!is_bounded_regular_file(dir.path(), 16));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn refuses_a_file_over_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.json"), "0123456789").unwrap();
        let err = read_to_string_inside(dir.path(), Path::new("a.json"), 4).unwrap_err();
        assert!(matches!(err, ReadRefusal::TooLarge { max_bytes: 4, .. }));
        assert!(!err.to_string().contains("0123"));
        let err = read_to_string_bounded(&dir.path().join("a.json"), 4).unwrap_err();
        assert!(matches!(err, ReadRefusal::TooLarge { .. }));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn refuses_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let err = read_to_string_inside(dir.path(), Path::new("sub"), 16).unwrap_err();
        assert!(matches!(err, ReadRefusal::NotRegularFile { .. }));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn refuses_a_missing_file_and_reports_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_to_string_inside(dir.path(), Path::new("none.json"), 16).unwrap_err();
        assert!(err.is_not_found());
        assert!(!is_inside_root(dir.path(), Path::new("none.json")));
        assert!(!is_inside_root(&dir.path().join("no-root"), Path::new("x")));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn refuses_a_dot_dot_path_that_leaves_the_root() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(outer.path().join("other.txt"), "content-7531").unwrap();
        let err = read_to_string_inside(&root, Path::new("../other.txt"), 16).unwrap_err();
        assert!(matches!(err, ReadRefusal::OutsideRoot { .. }));
        assert!(!err.to_string().contains("content-7531"));
        assert!(!is_inside_root(&root, Path::new("../other.txt")));
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn refuses_a_link_that_leaves_the_root() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(outer.path().join("other.txt"), "content-7531").unwrap();
        std::os::unix::fs::symlink(outer.path().join("other.txt"), root.join("a.json")).unwrap();
        let err = read_to_string_inside(&root, Path::new("a.json"), 16).unwrap_err();
        assert!(matches!(err, ReadRefusal::OutsideRoot { .. }));
        assert!(!is_inside_root(&root, Path::new("a.json")));
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn allows_a_link_that_stays_inside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("real.json"), "{}").unwrap();
        std::os::unix::fs::symlink("real.json", dir.path().join("a.json")).unwrap();
        let text = read_to_string_inside(dir.path(), Path::new("a.json"), 16).unwrap();
        assert_eq!(text, "{}");
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn refuses_a_dangling_link_as_outside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/nonexistent/fallow/target", dir.path().join("a.json"))
            .unwrap();
        let err = read_to_string_inside(dir.path(), Path::new("a.json"), 16).unwrap_err();
        assert!(matches!(err, ReadRefusal::OutsideRoot { .. }), "{err:?}");
        assert!(!err.is_not_found());
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::os::unix::fs::symlink("missing-dir", dir.path().join("sub/link")).unwrap();
        let err = read_to_string_inside(dir.path(), Path::new("sub/link/a.json"), 16).unwrap_err();
        assert!(matches!(err, ReadRefusal::OutsideRoot { .. }), "{err:?}");
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_missing_file_inside_the_root_stays_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        for path in ["sub/none.json", "sub/../none.json", "none/a.json"] {
            let err = read_to_string_inside(dir.path(), Path::new(path), 16).unwrap_err();
            assert!(err.is_not_found(), "{path}: {err:?}");
        }
        let err = read_to_string_inside(dir.path(), Path::new("../none.json"), 16).unwrap_err();
        assert!(matches!(err, ReadRefusal::OutsideRoot { .. }), "{err:?}");
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_missing_file_through_another_spelling_of_the_root_stays_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("real");
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&root, dir.path().join("alias")).unwrap();
        let path = dir.path().join("alias/none.json");
        let err = read_to_string_inside(&root, &path, 16).unwrap_err();
        assert!(err.is_not_found(), "{err:?}");
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "file system access is blocked by Miri isolation")]
    fn refuses_a_device_even_without_a_root_check() {
        let err = read_to_string_bounded(Path::new("/dev/zero"), 16).unwrap_err();
        assert!(matches!(err, ReadRefusal::NotRegularFile { .. }));
    }
}
