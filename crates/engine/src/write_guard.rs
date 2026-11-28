//! Confine the files that a run writes on request (baselines, snapshots and
//! report files) to the project.
//!
//! A path must resolve inside the project root, or inside the Git work tree
//! that contains the root when the working directory is inside that tree too,
//! or inside a shared directory: the CI workspace (`GITHUB_WORKSPACE`, GitLab
//! `CI_PROJECT_DIR`) and the temp directories (`RUNNER_TEMP`, the system temp
//! directory, and `/tmp` on Unix), each when it is set. An existing character device or named
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

use fallow_types::display_text::display_path;

/// The directories a run may write into.
#[derive(Debug, Clone)]
pub struct WriteScope {
    root: PathBuf,
    work_tree: Option<PathBuf>,
    /// The Git work tree that contains the root, wherever the run starts.
    /// A symlink in it is a link that the repository controls.
    checkout: Option<PathBuf>,
    /// The Git directory and common Git directory of the root and of each
    /// superproject above it. No symlink may resolve into them, wherever
    /// they are.
    git_dirs: fallow_types::path_guard::GitDirs,
    shared_dirs: Vec<PathBuf>,
}

/// Why [`WriteScope`] refuses a path.
enum Refusal {
    /// The path resolves outside the allowed directories.
    Outside,
    /// A symlink in the project leads out of the project, or into a `.git`
    /// or `node_modules` directory. `link` is the symlink when it is in the
    /// checkout. A message never names the target, because a symlink
    /// outside the project can lead anywhere on the host.
    Link {
        link: Option<PathBuf>,
        kind: LinkTarget,
    },
    /// The path names a file that is not a regular file or a directory, and
    /// is not a character device or named pipe that the user named directly.
    Special,
}

/// Where a refused symlink in the project leads.
#[derive(Clone, Copy)]
enum LinkTarget {
    /// Out of the project root and its allowed Git work tree.
    Outside,
    /// Into a `node_modules` directory.
    Packages,
    /// Into a `.git` directory, or the Git directory that a `.git` file names.
    GitMetadata,
}

impl LinkTarget {
    /// The place, as the end of a sentence that starts with "resolves".
    const fn phrase(self) -> &'static str {
        match self {
            Self::Outside => "outside the project",
            Self::Packages => "into a node_modules directory",
            Self::GitMetadata => "into Git metadata",
        }
    }

    fn of(within: Option<&Path>, target: &Path) -> Option<Self> {
        let Some(within) = within else {
            return Some(Self::Outside);
        };
        if !fallow_types::path_guard::is_protected_link_target(within, target) {
            return None;
        }
        let below = target.strip_prefix(within).unwrap_or(target);
        let in_packages = below.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("node_modules"))
        });
        Some(if in_packages {
            Self::Packages
        } else {
            Self::GitMetadata
        })
    }
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
        let checkout = fallow_types::path_guard::work_tree_top(&root).map(Path::to_path_buf);
        let work_tree = checkout.clone().filter(|tree| cwd.starts_with(tree));
        let git_dirs = fallow_types::path_guard::GitDirs::of(&root);
        Ok(Self {
            root,
            work_tree,
            checkout,
            git_dirs,
            shared_dirs,
        })
    }

    /// Whether a write to the absolute path `path`, as written, stays in the
    /// scope. See [`Self::check`] for the rules.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        walk(path).is_ok_and(|walk| self.refusal(&walk).is_none())
    }

    /// Return an error message when a write to `path`, relative to `cwd`,
    /// leaves the scope. `flag` names the option that asked for the write.
    ///
    /// - The path must resolve inside one of the allowed directories, or be
    ///   an existing character device or named pipe.
    /// - Each symlink on the way that lies in the project root or its Git
    ///   work tree must resolve inside the root or the allowed work tree.
    /// - No symlink on the way, wherever it lies, may resolve into a `.git`
    ///   or `node_modules` directory
    ///   ([`fallow_types::path_guard::is_protected_link_target`]), or into
    ///   the Git directory or common Git directory of the root or of a
    ///   superproject above it, also when that directory is outside the work
    ///   tree. A shared
    ///   directory or a device is allowed only for a path that reaches it
    ///   without such a link.
    /// - An existing target that is not a regular file or a directory is
    ///   refused, except a character device or named pipe that is reached
    ///   without such a link.
    /// - A path component that cannot be read for a reason other than "not
    ///   found" is refused.
    #[must_use]
    pub fn check(&self, flag: &str, path: &Path, cwd: &Path) -> Option<String> {
        if is_null_device(path) {
            return None;
        }
        let walk = match walk(&cwd.join(path)) {
            Ok(walk) => walk,
            Err(err) => {
                return Some(format!(
                    "{flag} {} cannot be resolved ({err}). Choose a path that fallow can read.",
                    path.display()
                ));
            }
        };
        let refusal = self.refusal(&walk)?;
        Some(match refusal {
            Refusal::Outside => format!(
                "{flag} {} resolves outside the project root {}. Choose a path inside the project root{}, or inside the CI workspace (GITHUB_WORKSPACE or CI_PROJECT_DIR) or a temp directory (RUNNER_TEMP, the system temp directory, or /tmp on Unix).",
                display_path(path),
                self.root.display(),
                self.work_tree
                    .as_deref()
                    .filter(|tree| *tree != self.root)
                    .map(|tree| format!(" or its Git work tree {}", tree.display()))
                    .unwrap_or_default(),
            ),
            Refusal::Link { link, kind } => format!(
                "{flag} {} goes through {}, which resolves {}. No symlink on the path may resolve into a .git or node_modules directory, and a symlink in the project must resolve inside the project. Choose a path without the symlink.",
                display_path(path),
                link_phrase(link.as_deref()),
                kind.phrase(),
            ),
            Refusal::Special => format!(
                "{flag} {} does not lead to a regular file. Choose a path to a regular file.",
                display_path(path),
            ),
        })
    }

    fn refusal(&self, walk: &Walk) -> Option<Refusal> {
        let mut through_project_link = false;
        for link in &walk.links {
            let kind = if self.git_dirs.holds(&link.target) {
                Some(LinkTarget::GitMetadata)
            } else if self.in_checkout(&link.location) {
                through_project_link = true;
                let within = if link.target.starts_with(&self.root) {
                    Some(self.root.as_path())
                } else {
                    self.work_tree
                        .as_deref()
                        .filter(|tree| link.target.starts_with(tree))
                };
                LinkTarget::of(within, &link.target)
            } else {
                // A link elsewhere may lead anywhere outside the project, but
                // not into Git metadata or installed packages.
                LinkTarget::of(Some(self.protected_scope_for(link)), &link.target)
                    .filter(|kind| !matches!(kind, LinkTarget::Outside))
            };
            if let Some(kind) = kind {
                return Some(Refusal::Link {
                    link: self
                        .in_checkout(&link.location)
                        .then(|| link.location.clone()),
                    kind,
                });
            }
        }
        match std::fs::metadata(&walk.resolved) {
            Ok(meta) if !meta.is_file() && !meta.is_dir() => {
                return (through_project_link || !is_stream_target(&walk.resolved))
                    .then_some(Refusal::Special);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Some(Refusal::Special),
        }
        let path = walk.resolved.as_path();
        let inside = path.starts_with(&self.root)
            || self
                .work_tree
                .as_deref()
                .is_some_and(|tree| path.starts_with(tree))
            || self.shared_dirs.iter().any(|dir| path.starts_with(dir));
        (!inside).then_some(Refusal::Outside)
    }

    /// Why a write to the absolute path `path` leaves the scope, as the end
    /// of a sentence that starts with "resolves", or `None` when it stays in
    /// the scope.
    #[must_use]
    pub fn refusal_reason(&self, path: &Path) -> Option<&'static str> {
        let Ok(walk) = walk(path) else {
            return Some("to a path that cannot be read");
        };
        Some(match self.refusal(&walk)? {
            Refusal::Outside => "outside the project",
            Refusal::Link { kind, .. } => kind.phrase(),
            Refusal::Special => "to a file that is not a regular file",
        })
    }

    /// The directory that the protected names of the target of a link
    /// outside the checkout are read below: the root or the checkout when the
    /// target is inside one, else the folder of the link.
    fn protected_scope_for<'a>(&'a self, link: &'a Link) -> &'a Path {
        if link.target.starts_with(&self.root) {
            return &self.root;
        }
        if let Some(tree) = self
            .checkout
            .as_deref()
            .filter(|tree| link.target.starts_with(tree))
        {
            return tree;
        }
        link.location.parent().unwrap_or(&link.location)
    }

    fn in_checkout(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
            || self
                .checkout
                .as_deref()
                .is_some_and(|tree| path.starts_with(tree))
    }

    fn refusal_error(&self, requested: &Path, walk: &Walk) -> Option<io::Error> {
        let reason = match self.refusal(walk)? {
            Refusal::Outside => format!(
                "{} resolves outside the directories this run may write to",
                display_path(requested),
            ),
            Refusal::Link { link, kind } => format!(
                "{} goes through {}, which resolves {}, so fallow did not write it",
                display_path(requested),
                link_phrase(link.as_deref()),
                kind.phrase(),
            ),
            Refusal::Special => format!(
                "{} does not lead to a regular file, so fallow did not write it",
                display_path(requested),
            ),
        };
        Some(io::Error::new(io::ErrorKind::PermissionDenied, reason))
    }
}

/// The symlink of a refusal for a message: its path when it is in the
/// checkout, else no path.
fn link_phrase(link: Option<&Path>) -> String {
    link.map_or_else(
        || "a symlink".to_owned(),
        |link| format!("the symlink {}", display_path(link)),
    )
}

/// The environment variables that name a shared directory a run may write
/// into: the GitHub Actions workspace and temp directory, and the GitLab
/// project directory.
const SHARED_DIR_VARIABLES: [&str; 3] = ["GITHUB_WORKSPACE", "CI_PROJECT_DIR", "RUNNER_TEMP"];

/// The shared directories a run may write into: each directory in
/// `GITHUB_WORKSPACE`, `CI_PROJECT_DIR` and `RUNNER_TEMP` that is set and not
/// empty, and the system temp directories (see [`system_temp_dirs`]). The CI
/// workspace keeps a job working that checks the repository out into a
/// subdirectory and writes its report beside it. A directory that does not
/// exist is skipped, because it cannot be resolved.
#[must_use]
pub fn shared_dirs() -> Vec<PathBuf> {
    shared_dirs_for(std::env::temp_dir())
}

fn shared_dirs_for(temp_dir: PathBuf) -> Vec<PathBuf> {
    SHARED_DIR_VARIABLES
        .iter()
        .filter_map(std::env::var_os)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .chain(temp_dirs_with(temp_dir))
        .filter_map(|dir| dir.canonicalize().ok())
        .collect()
}

/// The system temp directory on Unix. It is listed on its own because macOS
/// sets `TMPDIR` to a per-user folder, so `std::env::temp_dir()` is not
/// `/tmp` there.
const UNIX_SYSTEM_TEMP_DIR: &str = "/tmp";

/// The system temp directories: `std::env::temp_dir()`, and on Unix also
/// `/tmp`. The paths are not resolved. `/var/tmp` is not in the list: Linux
/// keeps it across reboots, and people keep long-lived clones there.
#[must_use]
pub fn system_temp_dirs() -> Vec<PathBuf> {
    temp_dirs_with(std::env::temp_dir())
}

fn temp_dirs_with(temp_dir: PathBuf) -> Vec<PathBuf> {
    let mut dirs = vec![temp_dir];
    if cfg!(unix) {
        dirs.push(PathBuf::from(UNIX_SYSTEM_TEMP_DIR));
    }
    dirs
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
const fn is_stream_target(_path: &Path) -> bool {
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
    let walk = walk(absolute).map_err(WriteFailure::File)?;
    if let Some(error) = scope.and_then(|scope| scope.refusal_error(requested, &walk)) {
        return Err(WriteFailure::File(error));
    }
    let resolved = walk.resolved;
    if is_stream_target(&resolved) {
        return open_stream(&resolved).map_err(WriteFailure::File);
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
    #[cfg(unix)]
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

/// How many symlinks [`resolve`] follows before it gives up.
const MAX_LINK_HOPS: usize = 40;

/// Resolve `path` the way a write would reach it: each component that exists
/// is resolved by the file system, one at a time, so a symlink after a `..`
/// is resolved too, and the rest, which a write would create, is kept as
/// written. A dangling symlink is followed to its target, because a write
/// through it lands there. When a component cannot be read, the path is
/// normalised from its longest existing part.
#[must_use]
pub fn resolve(path: &Path) -> PathBuf {
    walk(path).map_or_else(|_| canonical_form(path, 0), |walk| walk.resolved)
}

/// A symlink that a walk followed.
struct Link {
    /// Where the link is: its resolved parent directory and its name.
    location: PathBuf,
    /// Where the link leads, resolved.
    target: PathBuf,
}

/// Where a write to a path lands, and the symlinks on the way.
struct Walk {
    resolved: PathBuf,
    links: Vec<Link>,
}

/// Resolve `path` one component at a time, and record each symlink on the
/// way. A relative path is relative to the working directory.
///
/// # Errors
///
/// Returns the error of a component that cannot be read for a reason other
/// than "not found", an error after [`MAX_LINK_HOPS`] symlinks, and the error
/// of the working directory for a relative path.
fn walk(path: &Path) -> io::Result<Walk> {
    let absolute;
    let path = if path.is_absolute() {
        path
    } else {
        absolute = std::env::current_dir()?.join(path);
        &absolute
    };
    let mut links = Vec::new();
    let mut hops = 0;
    let walked = walk_components(path, &mut links, &mut hops)?;
    Ok(Walk {
        resolved: canonical_form(&walked, 0),
        links,
    })
}

fn walk_components(path: &Path, links: &mut Vec<Link>, hops: &mut usize) -> io::Result<PathBuf> {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        let name = match component {
            Component::Prefix(_) | Component::RootDir => {
                resolved.push(component.as_os_str());
                continue;
            }
            Component::CurDir => continue,
            Component::ParentDir => {
                resolved.pop();
                continue;
            }
            Component::Normal(name) => name,
        };
        let candidate = resolved.join(name);
        match candidate.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                *hops += 1;
                if *hops > MAX_LINK_HOPS {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{} goes through too many symlinks", path.display()),
                    ));
                }
                let target = std::fs::read_link(&candidate)?;
                let followed = walk_components(&resolved.join(target), links, hops)?;
                links.push(Link {
                    location: canonical_form(&resolved, 0).join(name),
                    target: canonical_form(&followed, 0),
                });
                resolved = followed;
            }
            Ok(_) => resolved = candidate,
            Err(error) if error.kind() == io::ErrorKind::NotFound => resolved = candidate,
            Err(error) => return Err(error),
        }
    }
    Ok(resolved)
}

/// `path` with its longest existing part canonicalized and the rest appended
/// as written. For a path from [`walk_components`] this only gives the
/// spelling that `canonicalize` uses on this platform, so it compares equal
/// with the canonical allowed directories.
fn canonical_form(path: &Path, hops: usize) -> PathBuf {
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
            let followed = canonical_form(&base.join(target), hops + 1);
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

    use super::{WriteScope, create_checked, names_windows_null_device, resolve};

    /// macOS sets `TMPDIR` to a per-user folder, but a user who passes
    /// `/tmp/x.json` means the system temp directory. Each Unix allows it,
    /// whatever `TMPDIR` names.
    #[cfg(unix)]
    #[test]
    fn scope_accepts_the_unix_system_temp_dir_when_tmpdir_names_another_folder() {
        use super::shared_dirs_for;

        let project = tempfile::tempdir().expect("project dir");
        let other_temp = tempfile::tempdir().expect("temp dir");
        let scope = WriteScope::new(
            project.path(),
            project.path(),
            shared_dirs_for(other_temp.path().to_path_buf()),
        )
        .expect("scope");
        assert_eq!(
            scope.check("--save-baseline", Path::new("/tmp/x.json"), project.path()),
            None
        );
    }

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

    /// A `..` after a folder that does not exist yet returns to a folder that
    /// exists, so a link after it is resolved too. Nothing is created
    /// outside the project.
    #[cfg(unix)]
    #[test]
    fn a_link_after_a_parent_step_is_resolved() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join("away")).unwrap();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        let requested = Path::new("build/../away/x/r.json");
        assert!(scope.check("--output-file", requested, &root).is_some());
        let absolute = root.join(requested);
        assert!(create_checked(requested, &absolute, Some(&scope)).is_err());
        assert!(!elsewhere.join("x").exists());
        assert!(!root.join("build").exists());
    }

    /// A link in the project that leads into a shared directory, such as the
    /// temp directory or the CI workspace, does not carry a write there.
    #[cfg(unix)]
    #[test]
    fn a_project_link_into_a_shared_dir_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let shared = dir.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("run.sh"), "keep").unwrap();
        std::os::unix::fs::symlink(shared.join("run.sh"), root.join("report.sarif")).unwrap();
        let scope =
            WriteScope::new(&root, &root, vec![shared.canonicalize().unwrap()]).expect("scope");
        let target = root.join("report.sarif");
        assert!(scope.check("--sarif-file", &target, &root).is_some());
        assert!(create_checked(&target, &target, Some(&scope)).is_err());
        assert_eq!(
            std::fs::read_to_string(shared.join("run.sh")).unwrap(),
            "keep"
        );
        assert!(
            scope
                .check("--sarif-file", &shared.join("report.sarif"), &root)
                .is_none()
        );
    }

    /// A refusal names the given path and the link in the project, never
    /// the place outside the project that a link leads to.
    #[cfg(unix)]
    #[test]
    fn a_refusal_does_not_print_where_an_outside_link_leads() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let elsewhere = dir.path().join("target-name-5520");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(elsewhere.join("sub")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join("out")).unwrap();
        std::os::unix::fs::symlink(elsewhere.join("sub"), dir.path().join("via")).unwrap();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        let messages = [
            scope.check("--sarif-file", &root.join("out/report.sarif"), &root),
            scope.check("--sarif-file", &dir.path().join("via/r.sarif"), &root),
            create_checked(
                &root.join("out/report.sarif"),
                &root.join("out/report.sarif"),
                Some(&scope),
            )
            .err()
            .map(|failure| failure.to_string()),
        ];
        for message in messages {
            let message = message.expect("refused");
            assert!(!message.contains("target-name-5520"), "{message}");
        }
    }

    /// A link in the project to a named pipe is refused, while a pipe that
    /// the user names directly stays allowed.
    #[cfg(unix)]
    #[test]
    fn a_project_link_to_a_named_pipe_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let fifo = dir.path().join("report.fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("run mkfifo");
        assert!(made.success());
        std::os::unix::fs::symlink(&fifo, root.join("report.json")).unwrap();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        assert!(
            scope
                .check("--output-file", &root.join("report.json"), &root)
                .is_some()
        );
        assert!(scope.check("--output-file", &fifo, &root).is_none());
    }

    /// A link that stays in the project but leads into `.git` or
    /// `node_modules` is refused. A link to another project folder passes.
    #[cfg(unix)]
    #[test]
    fn a_project_link_into_git_or_node_modules_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(root.join("reports")).unwrap();
        std::os::unix::fs::symlink(root.join(".git"), root.join("meta")).unwrap();
        std::os::unix::fs::symlink(root.join("node_modules/pkg"), root.join("pkg")).unwrap();
        std::os::unix::fs::symlink(root.join("reports"), root.join("out")).unwrap();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        for refused in ["meta/config", "pkg/index.js"] {
            assert!(
                scope
                    .check("--output-file", Path::new(refused), &root)
                    .is_some(),
                "{refused}"
            );
            let absolute = root.join(refused);
            assert!(create_checked(Path::new(refused), &absolute, Some(&scope)).is_err());
        }
        assert!(!root.join(".git/config").exists());
        assert!(
            scope
                .check("--output-file", Path::new("out/r.json"), &root)
                .is_none()
        );
    }

    /// A link outside the project that leads into the `.git` directory of
    /// the project is refused too, and the hook keeps its content.
    #[cfg(unix)]
    #[test]
    fn an_outside_link_into_git_metadata_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let shared = dir.path().join("shared");
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        let hook = root.join(".git/hooks/pre-commit");
        std::fs::write(&hook, "keep").unwrap();
        let link = shared.join("r.json");
        std::os::unix::fs::symlink(&hook, &link).unwrap();
        let scope =
            WriteScope::new(&root, &root, vec![shared.canonicalize().unwrap()]).expect("scope");
        assert!(scope.check("--output-file", &link, &root).is_some());
        assert!(create_checked(&link, &link, Some(&scope)).is_err());
        assert_eq!(std::fs::read_to_string(&hook).unwrap(), "keep");
    }

    /// A project whose Git directory is outside the work tree
    /// (`git init --separate-git-dir`) keeps its hooks there. A link outside
    /// the project into that Git directory is refused too.
    #[cfg(unix)]
    #[test]
    fn an_outside_link_into_a_separate_git_dir_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let shared = dir.path().join("shared");
        let git_dir = shared.join("gd");
        std::fs::create_dir_all(git_dir.join("hooks")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .unwrap();
        let hook = git_dir.join("hooks/pre-commit");
        std::fs::write(&hook, "keep").unwrap();
        let link = shared.join("r.json");
        std::os::unix::fs::symlink(&hook, &link).unwrap();
        let scope =
            WriteScope::new(&root, &root, vec![shared.canonicalize().unwrap()]).expect("scope");
        assert!(scope.check("--output-file", &link, &root).is_some());
        assert!(!scope.contains(&link));
        assert!(create_checked(&link, &link, Some(&scope)).is_err());
        assert_eq!(std::fs::read_to_string(&hook).unwrap(), "keep");
    }

    /// The same for a worktree of a bare repository: the link leads into the
    /// common Git directory, which holds the hooks.
    #[cfg(unix)]
    #[test]
    fn an_outside_link_into_the_common_git_dir_of_a_worktree_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let shared = dir.path().join("shared");
        let bare = shared.join("repo.git");
        let linked = bare.join("worktrees/project");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::create_dir_all(bare.join("hooks")).unwrap();
        std::fs::write(linked.join("commondir"), "../..\n").unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".git"), format!("gitdir: {}\n", linked.display())).unwrap();
        let hook = bare.join("hooks/pre-commit");
        std::fs::write(&hook, "keep").unwrap();
        let link = shared.join("r.json");
        std::os::unix::fs::symlink(&hook, &link).unwrap();
        let scope =
            WriteScope::new(&root, &root, vec![shared.canonicalize().unwrap()]).expect("scope");
        assert!(scope.check("--output-file", &link, &root).is_some());
        assert_eq!(std::fs::read_to_string(&hook).unwrap(), "keep");
    }

    /// A worktree of a bare repository lies inside the common Git directory.
    /// A link in it to another file of the same worktree is allowed, and a
    /// link into the Git directory is still refused.
    #[cfg(unix)]
    #[test]
    fn a_link_inside_a_worktree_of_a_bare_repository_is_allowed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bare = dir.path().join("proj.git");
        let linked = bare.join("worktrees/main");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::create_dir_all(bare.join("hooks")).unwrap();
        std::fs::write(linked.join("commondir"), "../..\n").unwrap();
        let root = bare.join("main");
        std::fs::create_dir_all(root.join("reports")).unwrap();
        std::fs::write(root.join(".git"), format!("gitdir: {}\n", linked.display())).unwrap();
        std::os::unix::fs::symlink(root.join("reports"), root.join("out")).unwrap();
        std::os::unix::fs::symlink(bare.join("hooks"), root.join("hooks")).unwrap();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        assert!(scope.contains(&root.join("out/r.json")));
        assert!(scope.contains(&root.join("reports/r.json")));
        assert!(!scope.contains(&root.join("hooks/pre-commit")));
    }

    /// A `.git` entry counts for the checkout when it exists as an entry,
    /// also as a dangling link, the same rule that the cache guard uses.
    #[cfg(unix)]
    #[test]
    fn a_dangling_git_link_marks_the_checkout() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("pkg")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("missing"), repo.join(".git")).unwrap();
        let scope = WriteScope::new(&repo.join("pkg"), &repo, Vec::new()).expect("scope");
        assert!(scope.contains(&repo.join("r.json")));
    }

    /// A socket is not a regular file, so a write to it is refused.
    #[cfg(unix)]
    #[test]
    fn a_socket_target_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let socket = root.join("s.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind");
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        assert!(scope.check("--output-file", &socket, &root).is_some());
    }

    /// A folder that cannot be read is refused instead of treated as
    /// missing.
    #[cfg(unix)]
    #[test]
    fn a_component_that_cannot_be_read_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let closed = root.join("closed");
        std::fs::create_dir_all(&closed).unwrap();
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = closed.join("probe").symlink_metadata();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        let result = scope.check("--output-file", Path::new("closed/sub/r.json"), &root);
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
        // A superuser reads the folder anyway, so the case does not apply.
        if readable.is_err_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied) {
            assert!(result.is_some());
        }
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

        assert!(super::open_no_follow(&link).is_err());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "keep");
    }

    /// Run a fixture `git` command in `dir` and require that it succeeds.
    #[cfg(unix)]
    fn git(dir: &Path, args: &[&str]) {
        let output = crate::git_fixture::fixture_git_command(dir)
            .args([
                "-c",
                "protocol.file.allow=always",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A superproject with its Git directory in `shared`
    /// (`git init --separate-git-dir`) and a submodule at `vendor/lib`.
    /// Returns the submodule root and the Git directory of the superproject.
    #[cfg(unix)]
    fn submodule_with_separate_superproject_git_dir(
        dir: &Path,
        shared: &Path,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let lib = dir.join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        git(&lib, &["init", "-q"]);
        std::fs::write(lib.join("index.ts"), "export const a = 1;\n").unwrap();
        git(&lib, &["add", "."]);
        git(&lib, &["commit", "-q", "-m", "lib"]);
        let superproject = dir.join("super");
        let git_dir = shared.join("super.git");
        std::fs::create_dir_all(&superproject).unwrap();
        git(
            &superproject,
            &[
                "init",
                "-q",
                "--separate-git-dir",
                git_dir.to_str().unwrap(),
            ],
        );
        git(
            &superproject,
            &[
                "submodule",
                "add",
                "-q",
                lib.to_str().unwrap(),
                "vendor/lib",
            ],
        );
        (superproject.join("vendor/lib"), git_dir)
    }

    /// The analysis root is a submodule, and the superproject keeps its Git
    /// directory in a shared directory. A link in the shared directory into
    /// the hooks of the superproject is refused.
    #[cfg(unix)]
    #[test]
    fn an_outside_link_into_the_git_dir_of_the_superproject_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let shared = dir.path().join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        let (root, git_dir) = submodule_with_separate_superproject_git_dir(dir.path(), &shared);
        let hook = git_dir.join("hooks/pre-commit");
        std::fs::write(&hook, "keep").unwrap();
        let link = shared.join("r.json");
        std::os::unix::fs::symlink(&hook, &link).unwrap();
        let config_link = shared.join("c.json");
        std::os::unix::fs::symlink(git_dir.join("config"), &config_link).unwrap();
        let module_link = shared.join("m.json");
        std::os::unix::fs::symlink(git_dir.join("modules/vendor/lib/config"), &module_link)
            .unwrap();
        let scope =
            WriteScope::new(&root, &root, vec![shared.canonicalize().unwrap()]).expect("scope");
        assert!(scope.check("--output-file", &link, &root).is_some());
        assert!(scope.check("--output-file", &config_link, &root).is_some());
        assert!(scope.check("--output-file", &module_link, &root).is_some());
        assert!(create_checked(&link, &link, Some(&scope)).is_err());
        assert_eq!(std::fs::read_to_string(&hook).unwrap(), "keep");
    }

    /// In a submodule, a link to an ordinary file of the submodule and a
    /// plain path in the shared directory stay allowed.
    #[cfg(unix)]
    #[test]
    fn a_link_inside_a_submodule_is_allowed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let shared = dir.path().join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        let (root, _) = submodule_with_separate_superproject_git_dir(dir.path(), &shared);
        std::fs::create_dir_all(root.join("reports")).unwrap();
        std::os::unix::fs::symlink("reports", root.join("out")).unwrap();
        let scope =
            WriteScope::new(&root, &root, vec![shared.canonicalize().unwrap()]).expect("scope");
        assert_eq!(
            scope.check("--output-file", Path::new("out/r.json"), &root),
            None
        );
        assert!(scope.contains(&shared.join("r.json")));
    }
}
