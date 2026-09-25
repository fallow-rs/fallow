//! Confine the files that a run writes on request (baselines, snapshots and
//! report files) to the project.
//!
//! A path must resolve inside the project root, or inside the Git work tree
//! that contains the root when the working directory is inside that tree too,
//! or inside a shared directory: the CI workspace (`GITHUB_WORKSPACE`, GitLab
//! `CI_PROJECT_DIR`) and the temp directories (`RUNNER_TEMP`, the system temp
//! directory), each when it is set. An existing character device or named
//! pipe (`/dev/null`, `/dev/stdout`, process substitution) is also allowed,
//! because a write to it cannot put a file anywhere. On Windows the null
//! device (`NUL`) is allowed for the same reason.
//!
//! The command line layer checks each path before the analysis starts and
//! then records the scope with [`confine`]. The writers call [`create_file`]
//! or [`write_file`], which resolve and check the path again right before the
//! write and do not follow a symlink at the final component.
//!
//! This narrows the window for a path that another local user swaps for a
//! symlink, from the whole analysis to the moment of the write. It does not
//! close the window for an intermediate directory: a directory swapped
//! between the last parent check and the open call is still followed. On
//! Windows the final component is checked just before the open, so a small
//! window stays there too.
//!
//! Symlinks are resolved on both sides: on the part of the path that exists,
//! and on each allowed directory. So a link inside the root that points
//! outside it does not open a way out, and `/var` and `/private/var` on macOS
//! compare equal.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// The directories a run may write into.
#[derive(Debug, Clone)]
pub struct WriteScope {
    root: PathBuf,
    work_tree: Option<PathBuf>,
    shared_dirs: Vec<PathBuf>,
}

impl WriteScope {
    /// Build the scope for a project root.
    ///
    /// The Git work tree that contains the root counts only when `cwd` is
    /// inside it too. That is the monorepo case the allowance exists for. It
    /// keeps a Git repository at `$HOME` (a dotfiles setup) from allowing
    /// every home path to a run started outside that tree.
    ///
    /// # Errors
    ///
    /// Returns a message when the root cannot be resolved, so a caller fails
    /// closed.
    pub fn new(root: &Path, cwd: &Path, shared_dirs: Vec<PathBuf>) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|err| {
            format!(
                "the project root {} cannot be resolved ({err})",
                root.display()
            )
        })?;
        let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        let work_tree = root
            .ancestors()
            .find(|dir| dir.join(".git").exists())
            .filter(|tree| cwd.starts_with(tree))
            .map(Path::to_path_buf);
        Ok(Self {
            root,
            work_tree,
            shared_dirs,
        })
    }

    /// Whether a resolved path lies inside one of the allowed directories, or
    /// is an existing character device or named pipe.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        is_stream_target(path)
            || path.starts_with(&self.root)
            || self
                .work_tree
                .as_deref()
                .is_some_and(|tree| path.starts_with(tree))
            || self.shared_dirs.iter().any(|dir| path.starts_with(dir))
    }

    /// Return an error message when `path`, relative to `cwd`, resolves
    /// outside the allowed directories. `flag` names the option that asked
    /// for the write.
    #[must_use]
    pub fn check(&self, flag: &str, path: &Path, cwd: &Path) -> Option<String> {
        if is_null_device(path) {
            return None;
        }
        let resolved = resolve(&cwd.join(path));
        if self.contains(&resolved) {
            return None;
        }
        Some(format!(
            "{flag} {} resolves to {}, which is outside the project root {}. Choose a path inside the project root{}, or inside the CI workspace (GITHUB_WORKSPACE or CI_PROJECT_DIR) or the temp directory (RUNNER_TEMP or the system temp directory).",
            path.display(),
            resolved.display(),
            self.root.display(),
            self.work_tree
                .as_deref()
                .filter(|tree| *tree != self.root)
                .map(|tree| format!(" or its Git work tree {}", tree.display()))
                .unwrap_or_default(),
        ))
    }
}

/// The environment variables that name a shared directory a run may write
/// into: the GitHub Actions workspace and temp directory, and the GitLab
/// project directory.
const SHARED_DIR_VARIABLES: [&str; 3] = ["GITHUB_WORKSPACE", "CI_PROJECT_DIR", "RUNNER_TEMP"];

/// The shared directories a run may write into: each directory in
/// `GITHUB_WORKSPACE`, `CI_PROJECT_DIR` and `RUNNER_TEMP` that is set and not
/// empty, and the system temp directory. The CI workspace keeps a job working that checks the
/// repository out into a subdirectory and writes its report beside it. A
/// directory that does not exist is skipped, because it cannot be resolved.
#[must_use]
pub fn shared_dirs() -> Vec<PathBuf> {
    SHARED_DIR_VARIABLES
        .iter()
        .filter_map(std::env::var_os)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .chain(std::iter::once(std::env::temp_dir()))
        .filter_map(|dir| dir.canonicalize().ok())
        .collect()
}

/// Whether `path` is an existing character device or named pipe. A write to
/// one cannot create or replace a file, so it needs no confinement.
#[cfg(unix)]
fn is_stream_target(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path).is_ok_and(|meta| {
        let file_type = meta.file_type();
        file_type.is_char_device() || file_type.is_fifo()
    })
}

#[cfg(not(unix))]
fn is_stream_target(_path: &Path) -> bool {
    false
}

/// The device path of the Windows null device.
#[cfg(windows)]
const WINDOWS_NULL_DEVICE: &str = r"\\.\NUL";

/// Whether `path`, as the command line gave it, names the null device on
/// this platform. Only Windows has a device name that the path check must
/// know: the resolved path of `NUL` is a normal path in the working
/// directory, where the device name no longer applies.
fn is_null_device(path: &Path) -> bool {
    cfg!(windows) && names_windows_null_device(path)
}

/// Whether `path` names the Windows null device: `NUL` alone, in any case,
/// with an optional colon, or the device path `\\.\NUL`.
///
/// A name with an extension (`NUL.txt`) and `NUL` inside a directory are not
/// included. Windows versions do not agree on them, so they stay normal
/// paths and get the normal check.
fn names_windows_null_device(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    let name = text
        .strip_prefix(r"\\.\")
        .or_else(|| text.strip_prefix("//./"))
        .unwrap_or_else(|| text.strip_suffix(':').unwrap_or(text));
    name.eq_ignore_ascii_case("NUL")
}

/// What kind of file a write targets. The kind selects the scope that the
/// write is checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteTarget {
    /// A path from the command line, or a default destination in the root.
    Path,
    /// The config file that fallow discovered and read for the project. The
    /// Git work tree of the root counts for it wherever the run starts.
    DiscoveredConfig,
}

/// The scopes recorded for this process.
#[derive(Debug)]
struct Confinement {
    cwd: PathBuf,
    paths: WriteScope,
    config: WriteScope,
}

static CONFINEMENT: OnceLock<Confinement> = OnceLock::new();

/// Record the scopes that later writes are checked against. The first call
/// wins. Without a call, writes are not confined, but they still do not
/// follow a symlink at the final component that appears after the path was
/// resolved.
pub fn confine(cwd: PathBuf, paths: WriteScope, config: WriteScope) {
    let _ = CONFINEMENT.set(Confinement { cwd, paths, config });
}

/// Why a confined write did not happen.
#[derive(Debug)]
pub enum WriteFailure {
    /// A missing parent directory could not be created.
    Directory(io::Error),
    /// The file could not be created or written, or the path resolved
    /// outside the recorded scope, or a path component changed after the
    /// check.
    File(io::Error),
}

impl WriteFailure {
    /// Whether the failure happened while the parent directories were
    /// created, so nothing was written.
    #[must_use]
    pub const fn is_directory(&self) -> bool {
        matches!(self, Self::Directory(_))
    }
}

impl std::fmt::Display for WriteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Directory(error) | Self::File(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for WriteFailure {}

impl From<WriteFailure> for io::Error {
    fn from(failure: WriteFailure) -> Self {
        match failure {
            WriteFailure::Directory(error) | WriteFailure::File(error) => error,
        }
    }
}

/// Create or truncate `path` for writing, after the checks this module
/// describes. Missing parent directories are created.
///
/// # Errors
///
/// Returns [`WriteFailure::Directory`] when a parent directory cannot be
/// created. Returns [`WriteFailure::File`] with `PermissionDenied` when the
/// path resolves outside the recorded scope, or when a path component changed
/// while the parent directories were created, and with the error of the open
/// call when the final component became a symlink after the path was resolved
/// (Unix) or the file cannot be created.
pub fn create_file(path: &Path, target: WriteTarget) -> Result<File, WriteFailure> {
    let confinement = CONFINEMENT.get();
    let absolute = match confinement {
        Some(confinement) => confinement.cwd.join(path),
        None => std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path)),
    };
    let scope = confinement.map(|confinement| match target {
        WriteTarget::Path => &confinement.paths,
        WriteTarget::DiscoveredConfig => &confinement.config,
    });
    create_checked(path, &absolute, scope)
}

/// Write `contents` to `path` through [`create_file`].
///
/// # Errors
///
/// Returns the errors of [`create_file`], and a write or flush error as
/// [`WriteFailure::File`].
pub fn write_file(path: &Path, contents: &[u8], target: WriteTarget) -> Result<(), WriteFailure> {
    let mut file = create_file(path, target)?;
    file.write_all(contents).map_err(WriteFailure::File)?;
    file.flush().map_err(WriteFailure::File)
}

fn create_checked(
    requested: &Path,
    absolute: &Path,
    scope: Option<&WriteScope>,
) -> Result<File, WriteFailure> {
    if is_null_device(requested) {
        return open_null_device().map_err(WriteFailure::File);
    }
    let resolved = resolve(absolute);
    if is_stream_target(&resolved) {
        return open_stream(&resolved).map_err(WriteFailure::File);
    }
    if scope.is_some_and(|scope| !scope.contains(&resolved)) {
        return Err(WriteFailure::File(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} resolves to {}, which is outside the directories this run may write to",
                requested.display(),
                resolved.display()
            ),
        )));
    }
    if let Some(parent) = resolved.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(WriteFailure::Directory)?;
        // A missing component can become a symlink between the resolve and
        // the create. The parent must still resolve to the same directory.
        let real_parent = parent.canonicalize().map_err(WriteFailure::File)?;
        if real_parent != parent {
            return Err(WriteFailure::File(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{} changed while fallow prepared the write, so fallow did not write it",
                    requested.display()
                ),
            )));
        }
    }
    open_no_follow(&resolved).map_err(WriteFailure::File)
}

/// Open an existing character device or named pipe for writing. It is not
/// created and not truncated. On Unix the open does not follow a symlink at
/// the final component, and the opened handle must still be a character
/// device or named pipe, so a path swapped after the check is refused.
fn open_stream(path: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    ensure_stream_handle(&file, path)?;
    Ok(file)
}

/// Open the Windows null device for writing through its device path.
#[cfg(windows)]
fn open_null_device() -> io::Result<File> {
    std::fs::OpenOptions::new()
        .write(true)
        .open(WINDOWS_NULL_DEVICE)
}

/// Only Windows has a null device name that [`is_null_device`] accepts.
#[cfg(not(windows))]
fn open_null_device() -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "only Windows has a null device name",
    ))
}

/// Refuse a handle that is not a character device or named pipe.
#[cfg(unix)]
fn ensure_stream_handle(file: &File, path: &Path) -> io::Result<()> {
    use std::os::unix::fs::FileTypeExt;
    let file_type = file.metadata()?.file_type();
    if file_type.is_char_device() || file_type.is_fifo() {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "{} is no longer a device or a named pipe, so fallow did not write it",
            path.display()
        ),
    ))
}

#[cfg(not(unix))]
fn ensure_stream_handle(_file: &File, _path: &Path) -> io::Result<()> {
    Ok(())
}

/// Open `path` for writing without following a symlink at the final
/// component. On Unix the open call refuses the link itself. Elsewhere the
/// final component is checked right before the open.
fn open_no_follow(path: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    // This check and the open are two steps, so a link that appears between
    // them is still followed. Only the Unix open closes that window.
    #[cfg(not(unix))]
    if path
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} became a symlink after fallow checked it, so fallow did not write it",
                path.display()
            ),
        ));
    }
    options.open(path)
}

/// How many dangling symlinks [`resolve`] follows before it gives up.
const MAX_LINK_HOPS: usize = 40;

/// Resolve `path` the way a write would reach it: symlinks and `..` in the
/// part that exists are resolved by the file system, and the rest, which a
/// write would create, is normalised lexically. A dangling symlink is followed
/// to its target, because a write through it lands there.
#[must_use]
pub fn resolve(path: &Path) -> PathBuf {
    resolve_with_hops(path, 0)
}

fn resolve_with_hops(path: &Path, hops: usize) -> PathBuf {
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            return append_missing(real, &missing);
        }
        if hops < MAX_LINK_HOPS
            && let Ok(target) = std::fs::read_link(existing)
        {
            let base = existing.parent().unwrap_or_else(|| Path::new(""));
            let followed = resolve_with_hops(&base.join(target), hops + 1);
            return append_missing(followed, &missing);
        }
        let (Some(parent), Some(last)) = (existing.parent(), existing.components().next_back())
        else {
            return append_missing(PathBuf::new(), &missing);
        };
        missing.push(last);
        existing = parent;
    }
}

/// Append the components that do not exist yet, innermost last.
fn append_missing(mut resolved: PathBuf, missing: &[Component<'_>]) -> PathBuf {
    for component in missing.iter().rev() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{WriteScope, create_checked, names_windows_null_device, open_no_follow, resolve};

    /// The Windows null device is named by `NUL` alone, in any case, with an
    /// optional colon, or by its device path. A file name that only starts
    /// with `NUL`, or `NUL` in a directory, is a normal file name.
    #[test]
    fn the_windows_null_device_is_named_by_nul_alone() {
        for name in [
            "NUL", "nul", "Nul", "NUL:", r"\\.\NUL", r"\\.\nul", "//./NUL",
        ] {
            assert!(names_windows_null_device(Path::new(name)), "{name}");
        }
        for name in [
            "NUL.txt",
            "nul.sarif",
            "null",
            "NULL",
            "report",
            r"dir\NUL",
            "dir/nul",
            r".\NUL",
            r"C:\NUL",
            "",
        ] {
            assert!(!names_windows_null_device(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn resolve_normalises_the_missing_part() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().canonicalize().unwrap();
        assert_eq!(
            resolve(&base.join("a/b/../c.json")),
            base.join("a").join("c.json")
        );
        assert_eq!(
            resolve(&base.join("a/../../x.json")),
            base.parent().unwrap().join("x.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_resolves_to_its_target() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("project");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(base.join("elsewhere.json"), root.join("b.json")).unwrap();
        assert_eq!(resolve(&root.join("b.json")), base.join("elsewhere.json"));
    }

    #[test]
    fn scope_accepts_a_temp_dir_through_a_symlinked_spelling() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let temp = dir.path().join("temp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&temp).unwrap();
        let scope =
            WriteScope::new(&root, &root, vec![temp.canonicalize().unwrap()]).expect("scope");
        // `dir.path()` is not canonical on macOS (`/var` links to
        // `/private/var`), so this also checks that both sides are resolved.
        assert!(
            scope
                .check("--save-baseline", &temp.join("b.json"), &root)
                .is_none()
        );
        assert!(
            scope
                .check("--save-baseline", &dir.path().join("b.json"), &root)
                .is_some()
        );
    }

    #[test]
    fn a_root_that_cannot_be_resolved_fails_closed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("missing");
        assert!(WriteScope::new(&missing, dir.path(), Vec::new()).is_err());
    }

    #[test]
    fn the_work_tree_needs_the_working_directory_inside_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path().join("home");
        let root = home.join("project");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(home.join(".git")).unwrap();
        let target = home.join(".config").join("x.json");
        let from_outside = WriteScope::new(&root, &elsewhere, Vec::new()).expect("scope");
        assert!(
            from_outside
                .check("--save-baseline", &target, &elsewhere)
                .is_some()
        );
        let from_inside = WriteScope::new(&root, &home, Vec::new()).expect("scope");
        assert!(
            from_inside
                .check("--save-baseline", &target, &home)
                .is_none()
        );
    }

    #[test]
    fn scope_accepts_the_root_and_rejects_a_sibling() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        assert!(
            scope
                .check("--save-baseline", "nested/b.json".as_ref(), &root)
                .is_none()
        );
        let message = scope
            .check("--save-baseline", "../b.json".as_ref(), &root)
            .expect("outside");
        assert!(message.contains("outside the project root"), "{message}");
    }

    #[test]
    fn scope_accepts_the_git_work_tree_of_the_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = dir.path().join("repo");
        let root = repo.join("packages/app");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let scope = WriteScope::new(&root, &repo, Vec::new()).expect("scope");
        assert!(
            scope
                .check("--save-baseline", "baselines/b.json".as_ref(), &repo)
                .is_none()
        );
        let message = scope
            .check("--save-baseline", "../b.json".as_ref(), &repo)
            .expect("outside");
        assert!(message.contains("Git work tree"), "{message}");
    }

    #[test]
    fn a_checked_write_lands_inside_the_scope() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        let target = root.join("out/nested/report.json");
        assert!(scope.check("--output-file", &target, &root).is_none());
        let file = create_checked(&target, &target, Some(&scope)).expect("write inside");
        drop(file);
        assert!(target.is_file());
    }

    /// A character device is allowed and written through.
    #[cfg(unix)]
    #[test]
    fn a_character_device_is_allowed_outside_the_scope() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        let null = std::path::Path::new("/dev/null");
        assert!(scope.check("--output-file", null, &root).is_none());
        let file = create_checked(null, null, Some(&scope)).expect("write to /dev/null");
        drop(file);
    }

    /// A named pipe outside the scope is allowed, and the write reaches the
    /// reader.
    #[cfg(unix)]
    #[test]
    fn a_named_pipe_is_allowed_outside_the_scope() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let fifo = dir.path().join("report.fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("run mkfifo");
        assert!(made.success());
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        assert!(scope.check("--sarif-file", &fifo, &root).is_none());
        let reader_path = fifo.clone();
        let reader = std::thread::spawn(move || std::fs::read_to_string(reader_path));
        let mut file = create_checked(&fifo, &fifo, Some(&scope)).expect("write to fifo");
        file.write_all(b"sarif").unwrap();
        drop(file);
        assert_eq!(reader.join().unwrap().unwrap(), "sarif");
    }

    /// A stream target is refused when the path is a symlink at the open, or
    /// when the opened handle is a regular file.
    #[cfg(unix)]
    #[test]
    fn a_stream_target_swapped_after_the_check_is_refused() {
        use super::{ensure_stream_handle, open_stream};
        let dir = tempfile::tempdir().expect("temp dir");
        let link = dir.path().join("null-link");
        std::os::unix::fs::symlink("/dev/null", &link).unwrap();
        assert!(open_stream(&link).is_err(), "a symlink is not followed");

        let regular = dir.path().join("regular.json");
        std::fs::write(&regular, "keep").unwrap();
        assert!(open_stream(&regular).is_err(), "a regular file is refused");
        let handle = std::fs::File::open(&regular).unwrap();
        assert!(ensure_stream_handle(&handle, &regular).is_err());
        assert_eq!(std::fs::read_to_string(&regular).unwrap(), "keep");

        let null = std::fs::File::open("/dev/null").unwrap();
        assert!(ensure_stream_handle(&null, std::path::Path::new("/dev/null")).is_ok());
    }

    /// A directory that passed the check and then became a symlink to a
    /// place outside the project does not carry the write out.
    #[cfg(unix)]
    #[test]
    fn a_directory_swapped_for_a_symlink_after_the_check_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(root.join("out")).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        let target = root.join("out/report.json");
        assert!(scope.check("--output-file", &target, &root).is_none());

        std::fs::remove_dir(root.join("out")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join("out")).unwrap();

        let error = std::io::Error::from(
            create_checked(&target, &target, Some(&scope)).expect_err("refused"),
        );
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!elsewhere.join("report.json").exists());
    }

    /// A final component that became a symlink after the check is refused,
    /// also when the link points outside the project.
    #[cfg(unix)]
    #[test]
    fn a_file_swapped_for_a_symlink_after_the_check_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside.json");
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        let target = root.join("report.json");
        assert!(scope.check("--output-file", &target, &root).is_none());

        std::os::unix::fs::symlink(&outside, &target).unwrap();

        assert!(create_checked(&target, &target, Some(&scope)).is_err());
        assert!(!outside.exists());
    }

    /// The open itself refuses a symlink at the final component, which covers
    /// a link that appears after the path was resolved.
    #[cfg(unix)]
    #[test]
    fn the_open_does_not_follow_a_final_symlink() {
        let dir = tempfile::tempdir().expect("temp dir");
        let real = dir.path().join("real.json");
        std::fs::write(&real, "keep").unwrap();
        let link = dir.path().join("link.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert!(open_no_follow(&link).is_err());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "keep");
    }
}
