//! Refuse writes through a symlink that leaves a trusted directory.
//!
//! A cloned repository can commit a symlink where fallow writes a file, for
//! example `.fallow/.gitignore` or the `.fallow` directory itself. A write
//! through that link reaches a file outside the project. A link that resolves
//! inside the base directory is safe and stays allowed, unless it leads into
//! Git metadata or into an installed package (see
//! [`is_protected_link_target`](crate::path_guard::is_protected_link_target)).

use std::path::{Component, Path, PathBuf};

/// The name of the Git metadata entry of a work tree.
const GIT_ENTRY: &str = ".git";

/// The name of the directory that package managers install into. Its files
/// can be hard links into a store that other projects use too, so fallow
/// does not write through a link into it.
const PACKAGE_DIR: &str = "node_modules";

/// The prefix of the line in a `.git` file that names the Git directory.
const GITDIR_PREFIX: &str = "gitdir:";

/// The file in a linked Git directory that names the common Git directory.
const COMMONDIR_FILE: &str = "commondir";

/// Why a write under a base directory is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    /// The path is not below the base, or a `..` takes it out of the base.
    NotBelowBase,
    /// A symlink resolves outside the base, or does not resolve.
    LinkOutside,
    /// A symlink resolves into Git metadata.
    GitMetadata,
    /// A symlink resolves into a `node_modules` directory.
    Packages,
    /// The target is not a regular file or a directory.
    NotRegular,
    /// An entry on the path cannot be read.
    Unreadable,
}

impl Refusal {
    /// The reason, as a sentence about `entry`.
    fn describe(self, entry: &Path, base: &Path) -> String {
        let entry = entry.display();
        match self {
            Self::NotBelowBase => format!("{entry} is not below {}", base.display()),
            Self::LinkOutside => format!(
                "{entry} is a symlink that resolves outside {} or does not resolve",
                base.display()
            ),
            Self::GitMetadata => format!("{entry} is a symlink that resolves into Git metadata"),
            Self::Packages => {
                format!("{entry} is a symlink that resolves into a node_modules directory")
            }
            Self::NotRegular => format!("{entry} is not a regular file or directory"),
            Self::Unreadable => format!("{entry} cannot be read"),
        }
    }
}

/// The entry that a write is refused at, and why.
struct Refused {
    entry: PathBuf,
    reason: Refusal,
}

/// How a walk treats a `..` or `.` component.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParentSteps {
    /// Refuse the path.
    Refuse,
    /// Apply the step to the resolved path, as the file system does.
    Resolve,
}

/// The first entry below `base`, up to and including `path`, that is a
/// symlink whose resolved target is outside the canonical `base`, whose
/// target leaves `base` on the way (also when a later `..` comes back), or
/// that [`is_protected_link_target`] refuses. A dangling link also counts, because
/// its target can appear outside `base` later. A `base` that is itself a link
/// into a protected directory returns `base`. An existing `path` that is not
/// a regular file or a directory (a device, a named pipe or a socket) returns
/// `path`. A path outside `base`, one with a `..` or `.` component, a `base`
/// that does not resolve, or an entry whose metadata cannot be read returns
/// that path. A missing entry ends the walk, because nothing below it exists
/// yet.
#[must_use]
pub fn outside_link(base: &Path, path: &Path) -> Option<PathBuf> {
    find_refusal(base, path, ParentSteps::Refuse).map(|refused| refused.entry)
}

/// The walk of [`outside_link`]. With [`ParentSteps::Resolve`], a `..` steps
/// up from the resolved path, so a link before it is checked and the walk
/// goes on below a missing entry.
fn find_refusal(base: &Path, path: &Path, steps: ParentSteps) -> Option<Refused> {
    let refuse = |entry: &Path, reason| {
        Some(Refused {
            entry: entry.to_path_buf(),
            reason,
        })
    };
    let Ok(relative) = path.strip_prefix(base) else {
        return refuse(path, Refusal::NotBelowBase);
    };
    let Ok(canonical_base) = std::fs::canonicalize(base) else {
        return refuse(path, Refusal::Unreadable);
    };
    if let Some(reason) = base_protected_link(base, &canonical_base) {
        return refuse(base, reason);
    }
    let mut current = base.to_path_buf();
    let mut resolved = canonical_base.clone();
    for component in relative.components() {
        let name = match component {
            Component::Normal(name) => name,
            Component::CurDir if steps == ParentSteps::Resolve => continue,
            Component::ParentDir if steps == ParentSteps::Resolve => {
                current.push(component);
                resolved.pop();
                if !resolved.starts_with(&canonical_base) {
                    return refuse(&current, Refusal::NotBelowBase);
                }
                continue;
            }
            _ => return refuse(path, Refusal::NotBelowBase),
        };
        current.push(name);
        match step_into(&canonical_base, &resolved, name, steps) {
            Ok(Some(next)) => resolved = next,
            Ok(None) => return None,
            Err(reason) => return refuse(&current, reason),
        }
    }
    is_special_file(&resolved).then(|| Refused {
        entry: path.to_path_buf(),
        reason: Refusal::NotRegular,
    })
}

/// The resolved path after the entry `name` below the resolved `parent`.
/// `None` ends the walk at a missing entry with [`ParentSteps::Refuse`].
fn step_into(
    canonical_base: &Path,
    parent: &Path,
    name: &std::ffi::OsStr,
    steps: ParentSteps,
) -> Result<Option<PathBuf>, Refusal> {
    let candidate = parent.join(name);
    match candidate.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => {
            link_target_in(canonical_base, parent, &candidate).map(Some)
        }
        Ok(_) => Ok(Some(candidate)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok((steps == ParentSteps::Resolve).then_some(candidate))
        }
        Err(_) => Err(Refusal::Unreadable),
    }
}

/// The resolved target of the symlink `link` in the resolved directory
/// `parent`, when each step of the target stays inside the canonical base,
/// the target exists, and it is not protected. A target that leaves the base
/// and comes back through `..` is refused, so the answer does not depend on
/// entries outside the base.
fn link_target_in(canonical_base: &Path, parent: &Path, link: &Path) -> Result<PathBuf, Refusal> {
    let target = crate::link_walk::link_target_below(canonical_base, parent, link)
        .map_err(|_| Refusal::LinkOutside)?;
    match protected_kind(canonical_base, &target) {
        Some(reason) => Err(reason),
        None => Ok(target),
    }
}

/// Whether `path` exists and is not a regular file or a directory.
fn is_special_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| !meta.is_file() && !meta.is_dir())
}

/// Why `base` is refused when it is a symlink whose target is a protected
/// directory, seen from the directory that contains the link.
fn base_protected_link(base: &Path, canonical_base: &Path) -> Option<Refusal> {
    if !base
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return None;
    }
    let Some(parent) = base
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
    else {
        return Some(Refusal::Unreadable);
    };
    protected_kind(&parent, canonical_base)
}

/// Whether a symlink whose canonical target is `resolved` leads into Git
/// metadata or into an installed package, so fallow must not write through
/// it. `within` is the canonical directory the link was found in.
///
/// The target is protected when the part of it below `within` (all of it when
/// it is not below `within`) has a `.git` component (in any case) or a
/// `node_modules` component, or when [`GitDirs::holds`] it for the work tree
/// that contains `within` and the work trees above it. A `.git` file can
/// name a Git directory with any name, so the second test covers a linked
/// worktree, a submodule, the superproject of a submodule and a separate Git
/// directory.
///
/// The rule is for links only. A direct path into `.git`, such as a hook that
/// `fallow hooks install` writes on purpose, does not go through it.
#[must_use]
pub fn is_protected_link_target(within: &Path, resolved: &Path) -> bool {
    protected_kind(within, resolved).is_some()
}

/// The refusal of [`is_protected_link_target`], or `None`.
fn protected_kind(within: &Path, resolved: &Path) -> Option<Refusal> {
    let below = resolved.strip_prefix(within).unwrap_or(resolved);
    let names = || {
        below.components().filter_map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
    };
    if names().any(|name| name.eq_ignore_ascii_case(PACKAGE_DIR)) {
        return Some(Refusal::Packages);
    }
    if names().any(|name| name.eq_ignore_ascii_case(GIT_ENTRY)) {
        return Some(Refusal::GitMetadata);
    }
    GitDirs::of(within)
        .holds(resolved)
        .then_some(Refusal::GitMetadata)
}

/// The canonical Git directories of the work tree that contains a directory
/// and of each work tree above it: for each, the Git directory and the common
/// Git directory. The directories can be outside the work trees, for a
/// separate Git directory, a worktree of a bare repository, or a submodule
/// whose superproject keeps its Git directory elsewhere.
#[derive(Clone, Debug, Default)]
pub struct GitDirs {
    /// The canonical top of the nearest work tree.
    work_tree: Option<PathBuf>,
    dirs: Vec<PathBuf>,
    /// A `.git` file or a `commondir` file exists but cannot be read or
    /// resolved, so a Git directory is not known.
    unknown: bool,
}

impl GitDirs {
    /// The Git directories of the work tree that contains `dir` and of each
    /// enclosing work tree, such as the superproject of a submodule. Empty
    /// when no ancestor of `dir` has a `.git` entry, or when each entry names
    /// no Git directory that exists.
    #[must_use]
    pub fn of(dir: &Path) -> Self {
        let Some(top) = work_tree_top(dir) else {
            return Self::default();
        };
        let mut git_dirs = Self {
            work_tree: std::fs::canonicalize(top).ok(),
            ..Self::default()
        };
        let mut next = Some(top);
        while let Some(tree) = next {
            git_dirs.add_entry(&tree.join(GIT_ENTRY));
            next = tree.parent().and_then(work_tree_top);
        }
        git_dirs
    }

    /// Add the Git directory and the common Git directory that the `.git`
    /// entry `entry` stands for.
    fn add_entry(&mut self, entry: &Path) {
        let git_dir = match read_git_entry(entry) {
            Ok(Some(git_dir)) => git_dir,
            Ok(None) => return,
            Err(Unreadable) => {
                self.unknown = true;
                return;
            }
        };
        let common = common_dir_of(&git_dir);
        self.add(git_dir);
        match common {
            Some(common) => self.add(common),
            None => self.unknown = true,
        }
    }

    fn add(&mut self, dir: PathBuf) {
        if !self.dirs.contains(&dir) {
            self.dirs.push(dir);
        }
    }

    /// Whether the canonical `path` is in one of the Git directories. A
    /// path in the work tree does not count when the work tree itself is in
    /// that Git directory, as for a worktree inside a bare repository. When
    /// a Git directory is not known, every path counts.
    #[must_use]
    pub fn holds(&self, path: &Path) -> bool {
        if self.unknown {
            return true;
        }
        let in_work_tree_of = |dir: &Path| {
            self.work_tree
                .as_deref()
                .is_some_and(|tree| tree.starts_with(dir) && path.starts_with(tree))
        };
        self.dirs
            .iter()
            .any(|dir| path.starts_with(dir) && !in_work_tree_of(dir))
    }
}

/// The nearest ancestor of `dir`, `dir` included, with a `.git` entry. The
/// entry counts when it exists as an entry, also as a dangling link.
#[must_use]
pub fn work_tree_top(dir: &Path) -> Option<&Path> {
    dir.ancestors()
        .find(|ancestor| ancestor.join(GIT_ENTRY).symlink_metadata().is_ok())
}

/// A `.git` entry or a `commondir` file that exists but cannot be read,
/// is not a regular file, or names no Git directory.
struct Unreadable;

/// The size limit for a `.git` file or a `commondir` file. Git writes one
/// short line, so a larger file is not Git metadata.
const GIT_FILE_MAX_BYTES: u64 = 64 * 1024;

/// The canonical Git directory that the `.git` entry `entry` stands for: the
/// directory itself, or the directory that a `.git` file names.
fn git_dir_of_entry(entry: &Path) -> Option<PathBuf> {
    read_git_entry(entry).ok().flatten()
}

/// [`git_dir_of_entry`] that tells a missing entry from one that cannot be
/// read. `Ok(None)` when the entry does not resolve, or when it names a Git
/// directory that does not exist.
fn read_git_entry(entry: &Path) -> Result<Option<PathBuf>, Unreadable> {
    let meta = match std::fs::metadata(entry) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Unreadable),
    };
    if meta.is_dir() {
        return std::fs::canonicalize(entry)
            .map(Some)
            .map_err(|_| Unreadable);
    }
    let text = crate::read_guard::read_to_string_bounded(entry, GIT_FILE_MAX_BYTES)
        .map_err(|_| Unreadable)?;
    let named = text
        .lines()
        .find_map(|line| line.strip_prefix(GITDIR_PREFIX))
        .map(str::trim)
        .filter(|named| !named.is_empty())
        .ok_or(Unreadable)?;
    let parent = entry.parent().ok_or(Unreadable)?;
    Ok(std::fs::canonicalize(parent.join(named)).ok())
}

/// The canonical common Git directory of the Git directory `git_dir`: the
/// directory that its `commondir` file names, or `git_dir` itself when it
/// has no `commondir` file. `None` when the file cannot be read or names a
/// directory that does not resolve.
fn common_dir_of(git_dir: &Path) -> Option<PathBuf> {
    match crate::read_guard::read_to_string_bounded(
        &git_dir.join(COMMONDIR_FILE),
        GIT_FILE_MAX_BYTES,
    ) {
        Ok(text) => std::fs::canonicalize(git_dir.join(text.trim())).ok(),
        Err(error) if error.is_not_found() => Some(git_dir.to_path_buf()),
        Err(_) => None,
    }
}

/// The directory that cache writes must stay in.
///
/// - A cache directory inside the project root is checked from the root, so
///   a committed `.fallow` link is refused.
/// - A cache directory in the same Git repository as the root, but not below
///   the root, is checked from the top of its work tree. This covers a cache
///   above a subdirectory root, and the cache in the checkout of an audit
///   that runs in a linked worktree (`audit --staged`, the base snapshot).
/// - Any other cache directory comes from the user, such as
///   `FALLOW_CACHE_DIR` on a CI runner, so only the entries below it are
///   checked.
///
/// A `..` in the cache directory is resolved by text first, so
/// `FALLOW_CACHE_DIR=../shared` counts as a directory outside the root.
#[must_use]
pub fn cache_write_base(root: &Path, cache_dir: &Path) -> PathBuf {
    let normalized = lexically_normalized(cache_dir);
    if normalized.starts_with(root) {
        return root.to_path_buf();
    }
    same_repository_top(root, &normalized).unwrap_or_else(|| cache_dir.to_path_buf())
}

/// The top of a work tree of the repository that contains `root`, as an
/// ancestor of `cache_dir`, when `cache_dir` is below one.
///
/// A work tree counts when its `.git` entry (a directory or a file) has the
/// same common Git directory as the work tree of the root. This covers the
/// main checkout, a linked worktree, a submodule and a worktree of a bare
/// repository. An ancestor that is a symlink never counts.
///
/// When more than one ancestor counts, the lowest one is the top, so a link
/// in a worktree inside the main checkout is checked from that worktree and
/// cannot lead up into the main checkout. An ancestor that is below a
/// symlink, seen from the highest ancestor that counts, is skipped, so the
/// base is never reached through a link. The links below the top are checked
/// later, from the top.
fn same_repository_top(root: &Path, cache_dir: &Path) -> Option<PathBuf> {
    let root_common = common_dir_of_work_tree(work_tree_top(root)?)?;
    let tops: Vec<&Path> = cache_dir
        .ancestors()
        .skip(1)
        .filter(|ancestor| {
            ancestor
                .symlink_metadata()
                .is_ok_and(|meta| !meta.file_type().is_symlink())
                && ancestor.join(GIT_ENTRY).symlink_metadata().is_ok()
                && common_dir_of_work_tree(ancestor).as_ref() == Some(&root_common)
        })
        .collect();
    let highest = *tops.last()?;
    tops.into_iter()
        .find(|top| !has_link_between(highest, top))
        .map(Path::to_path_buf)
}

/// Whether an entry below `upper`, down to and including `lower`, is a
/// symlink or cannot be read. `lower` must be below `upper`.
fn has_link_between(upper: &Path, lower: &Path) -> bool {
    let Ok(relative) = lower.strip_prefix(upper) else {
        return true;
    };
    let mut current = upper.to_path_buf();
    relative.components().any(|component| {
        current.push(component);
        !current
            .symlink_metadata()
            .is_ok_and(|meta| !meta.file_type().is_symlink())
    })
}

/// The canonical common Git directory of the work tree at `top`.
fn common_dir_of_work_tree(top: &Path) -> Option<PathBuf> {
    git_dir_of_entry(&top.join(GIT_ENTRY)).and_then(|git_dir| common_dir_of(&git_dir))
}

/// `path` with each `..` applied to the component before it, and each `.`
/// dropped, without a file system read.
#[must_use]
pub fn lexically_normalized(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

/// Fail when a write to `path` would follow a symlink that leaves `base`.
///
/// Call this before each create, write, rename or delete under a cache
/// directory, with the base from [`cache_write_base`]. A base that does not
/// exist yet has no link below it, so the first save into a new cache
/// directory passes. A dangling link at the base itself exists, so it is
/// still checked. A `..` in `path` is applied to the resolved path, as the
/// file system does, so `FALLOW_CACHE_DIR=../shared` from a subdirectory
/// root works and a link before the `..` is still checked.
///
/// # Errors
///
/// Returns a `PermissionDenied` error that names the refused entry and the
/// reason.
pub fn ensure_no_outside_link(base: &Path, path: &Path) -> std::io::Result<()> {
    if base.symlink_metadata().is_err() {
        return Ok(());
    }
    match find_refusal(base, path, ParentSteps::Resolve) {
        None => Ok(()),
        Some(refused) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "refusing to write {}: {}",
                path.display(),
                refused.reason.describe(&refused.entry, base)
            ),
        )),
    }
}

/// A cache directory and the directory that its writes must stay in.
///
/// Every cache writer takes this type instead of a bare directory, so a
/// writer cannot skip the symlink check.
#[derive(Clone, Copy, Debug)]
pub struct CacheTarget<'a> {
    dir: &'a Path,
    /// The cache directory that the base is computed from. A subdirectory
    /// from [`Self::with_dir`] keeps the base of its parent.
    anchor: &'a Path,
    /// The project root, or `None` when the anchor is its own base.
    root: Option<&'a Path>,
}

impl<'a> CacheTarget<'a> {
    /// The cache directory `dir` of the project at `root`.
    #[must_use]
    pub const fn new(root: &'a Path, dir: &'a Path) -> Self {
        Self {
            dir,
            anchor: dir,
            root: Some(root),
        }
    }

    /// A cache directory that is its own base, such as a directory that the
    /// user chose.
    #[must_use]
    pub const fn inside(dir: &'a Path) -> Self {
        Self {
            dir,
            anchor: dir,
            root: None,
        }
    }

    /// The cache directory.
    #[must_use]
    pub const fn dir(&self) -> &'a Path {
        self.dir
    }

    /// The same base with `dir` as the cache directory, for a subdirectory.
    #[must_use]
    pub const fn with_dir(&self, dir: &'a Path) -> Self {
        Self {
            dir,
            anchor: self.anchor,
            root: self.root,
        }
    }

    /// The directory that the writes must stay in, from
    /// [`cache_write_base`].
    #[must_use]
    pub fn write_base(&self) -> PathBuf {
        self.root.map_or_else(
            || self.anchor.to_path_buf(),
            |root| cache_write_base(root, self.anchor),
        )
    }

    /// Fail when a write to `path` would follow a symlink that leaves the
    /// base.
    ///
    /// # Errors
    ///
    /// Returns the error of [`ensure_no_outside_link`].
    pub fn check(&self, path: &Path) -> std::io::Result<()> {
        ensure_no_outside_link(&self.write_base(), path)
    }

    /// Create the cache directory and replace the file `name` in it with
    /// `data` through [`crate::atomic_file::write_atomic`], after the
    /// symlink check. Use it only for a cache that fallow can build again.
    ///
    /// # Errors
    ///
    /// Returns the error of the check, of the directory creation, or of the
    /// write.
    pub fn write_file(&self, name: &str, data: &[u8]) -> std::io::Result<()> {
        let path = self.prepare(name)?;
        crate::atomic_file::write_atomic(&path, data)
    }

    /// [`Self::write_file`] through [`crate::atomic_file::write_atomic_synced`],
    /// for data that fallow cannot build again.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Self::write_file`].
    pub fn write_file_synced(&self, name: &str, data: &[u8]) -> std::io::Result<()> {
        let path = self.prepare(name)?;
        crate::atomic_file::write_atomic_synced(&path, data)
    }

    fn prepare(&self, name: &str) -> std::io::Result<PathBuf> {
        let path = self.dir.join(name);
        self.check(&path)?;
        std::fs::create_dir_all(self.dir)?;
        Ok(path)
    }

    /// Read the file `name` of the cache directory in full, through
    /// [`crate::read_guard::read_bytes_inside`] with the base of
    /// [`Self::write_base`] as the root.
    ///
    /// A cache directory is a path in the checkout, so a clone can hold a
    /// cache file. The read refuses a file that resolves outside the base,
    /// a file that is not a regular file, and a file of more than
    /// `max_bytes` bytes, before it reads a byte.
    ///
    /// # Errors
    ///
    /// Returns a [`crate::read_guard::ReadRefusal`] when a check fails or
    /// the read fails. A missing file gives a refusal whose
    /// [`crate::read_guard::ReadRefusal::is_not_found`] is true.
    pub fn read_file(
        &self,
        name: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, crate::read_guard::ReadRefusal> {
        crate::read_guard::read_bytes_inside(&self.write_base(), &self.dir.join(name), max_bytes)
    }

    /// Write `.gitignore` with `*`, so the cache directory is never
    /// committed. An equal file stays as it is.
    ///
    /// # Errors
    ///
    /// Returns the error of [`Self::write_file`].
    pub fn write_gitignore(&self) -> std::io::Result<()> {
        let path = self.dir.join(".gitignore");
        self.check(&path)?;
        if std::fs::read(&path).is_ok_and(|content| content == b"*\n") {
            return Ok(());
        }
        self.write_file(".gitignore", b"*\n")
    }
}

#[cfg(all(test, unix, not(miri)))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        outside: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("project");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&outside).expect("outside");
        Fixture {
            _dir: dir,
            root,
            outside,
        }
    }

    #[test]
    fn a_plain_path_passes() {
        let f = fixture();
        let path = f.root.join(".fallow/cache/x/cache.bin");
        assert_eq!(outside_link(&f.root, &path), None);
        assert!(ensure_no_outside_link(&f.root, &path).is_ok());
    }

    #[test]
    fn a_linked_cache_dir_is_refused() {
        let f = fixture();
        symlink(&f.outside, f.root.join(".fallow")).expect("link");
        let path = f.root.join(".fallow/.gitignore");
        assert_eq!(outside_link(&f.root, &path), Some(f.root.join(".fallow")));
        let error = ensure_no_outside_link(&f.root, &path).expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_dangling_file_link_is_refused() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join(".fallow")).expect("cache");
        let link = f.root.join(".fallow/.gitignore");
        symlink(f.outside.join("missing"), &link).expect("link");
        assert_eq!(outside_link(&f.root, &link), Some(link.clone()));
        let error = ensure_no_outside_link(&f.root, &link).expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_link_inside_the_base_passes() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join("build/cache")).expect("target");
        symlink(f.root.join("build/cache"), f.root.join(".fallow")).expect("link");
        assert_eq!(
            outside_link(&f.root, &f.root.join(".fallow/cache.bin")),
            None
        );
    }

    #[test]
    fn a_path_outside_the_base_is_refused() {
        let f = fixture();
        let path = f.outside.join("cache.bin");
        assert_eq!(outside_link(&f.root, &path), Some(path));
    }

    #[test]
    fn the_base_follows_where_the_cache_dir_is() {
        let root = Path::new("/p");
        assert_eq!(cache_write_base(root, Path::new("/p/.fallow")), root);
        assert_eq!(
            cache_write_base(root, Path::new("/ci/cache")),
            Path::new("/ci/cache")
        );
    }

    #[test]
    fn a_target_writes_the_file_and_the_gitignore() {
        let f = fixture();
        let dir = f.root.join(".fallow/cache");
        let target = CacheTarget::new(&f.root, &dir);
        target.write_gitignore().expect("gitignore");
        target.write_file("cache.bin", b"data").expect("file");
        assert_eq!(std::fs::read(dir.join(".gitignore")).expect("read"), b"*\n");
        assert_eq!(std::fs::read(dir.join("cache.bin")).expect("read"), b"data");
    }

    #[test]
    fn a_synced_write_replaces_the_file() {
        let f = fixture();
        let dir = f.root.join(".fallow/cache");
        let target = CacheTarget::new(&f.root, &dir);
        target
            .write_file_synced("state.json", b"old")
            .expect("first");
        target
            .write_file_synced("state.json", b"new")
            .expect("second");
        assert_eq!(std::fs::read(dir.join("state.json")).expect("read"), b"new");
    }

    #[test]
    fn a_synced_write_refuses_a_linked_cache_dir() {
        let f = fixture();
        symlink(&f.outside, f.root.join(".fallow")).expect("link");
        let dir = f.root.join(".fallow");
        let target = CacheTarget::new(&f.root, &dir);
        assert!(target.write_file_synced("state.json", b"data").is_err());
        assert_eq!(std::fs::read_dir(&f.outside).expect("outside").count(), 0);
    }

    #[test]
    fn a_target_refuses_a_linked_cache_dir_and_writes_nothing() {
        let f = fixture();
        symlink(&f.outside, f.root.join(".fallow")).expect("link");
        let dir = f.root.join(".fallow");
        let target = CacheTarget::new(&f.root, &dir);
        assert!(target.write_gitignore().is_err());
        assert!(target.write_file("cache.bin", b"data").is_err());
        assert_eq!(std::fs::read_dir(&f.outside).expect("outside").count(), 0);
    }

    #[test]
    fn a_target_reads_a_file_of_its_cache_dir() {
        let f = fixture();
        let dir = f.root.join(".fallow/cache");
        let target = CacheTarget::new(&f.root, &dir);
        target.write_file("cache.bin", b"data").expect("file");
        assert_eq!(target.read_file("cache.bin", 4).expect("read"), b"data");
        let missing = target.read_file("other.bin", 4).expect_err("missing");
        assert!(missing.is_not_found());
    }

    #[test]
    fn a_target_refuses_a_cache_file_link_out_of_the_base() {
        let f = fixture();
        let dir = f.root.join(".fallow");
        std::fs::create_dir_all(&dir).expect("cache");
        std::fs::write(f.outside.join("cache.bin"), b"data").expect("outside file");
        symlink(f.outside.join("cache.bin"), dir.join("cache.bin")).expect("link");
        let refusal = CacheTarget::new(&f.root, &dir)
            .read_file("cache.bin", 1024)
            .expect_err("refused");
        assert!(matches!(
            refusal,
            crate::read_guard::ReadRefusal::OutsideRoot { .. }
        ));
    }

    #[test]
    fn a_target_refuses_a_cache_file_that_is_not_a_regular_file() {
        let f = fixture();
        let dir = f.root.join(".fallow");
        std::fs::create_dir_all(&dir).expect("cache");
        let made = std::process::Command::new("mkfifo")
            .arg(dir.join("cache.bin"))
            .status()
            .expect("run mkfifo");
        assert!(made.success());
        let refusal = CacheTarget::new(&f.root, &dir)
            .read_file("cache.bin", 1024)
            .expect_err("refused");
        assert!(matches!(
            refusal,
            crate::read_guard::ReadRefusal::NotRegularFile { .. }
        ));
    }

    #[test]
    fn a_target_refuses_a_cache_file_over_the_limit_before_the_read() {
        let f = fixture();
        let dir = f.root.join(".fallow");
        let target = CacheTarget::new(&f.root, &dir);
        target.write_file("cache.bin", b"12345").expect("file");
        let refusal = target.read_file("cache.bin", 4).expect_err("refused");
        assert!(matches!(
            refusal,
            crate::read_guard::ReadRefusal::TooLarge { max_bytes: 4, .. }
        ));
    }

    #[test]
    fn a_target_refuses_a_dangling_gitignore_link() {
        let f = fixture();
        let dir = f.root.join(".fallow");
        std::fs::create_dir_all(&dir).expect("cache");
        symlink(f.outside.join("victim"), dir.join(".gitignore")).expect("link");
        assert!(CacheTarget::new(&f.root, &dir).write_gitignore().is_err());
        assert!(!f.outside.join("victim").exists());
    }

    #[test]
    fn a_relative_cache_dir_above_the_root_is_its_own_base() {
        let root = Path::new("/p/project");
        let shared = Path::new("/p/project/../shared");
        assert_eq!(cache_write_base(root, shared), shared);
        let inside = Path::new("/p/project/sub/../.fallow");
        assert_eq!(cache_write_base(root, inside), root);
    }

    #[test]
    fn a_relative_cache_dir_above_the_root_gets_its_cache() {
        let f = fixture();
        let dir = f.root.join("../shared");
        let target = CacheTarget::new(&f.root, &dir);
        target.write_file("cache.bin", b"data").expect("file");
        assert!(f.outside.join("../shared/cache.bin").exists());
    }

    #[test]
    fn a_missing_base_passes_the_check() {
        let f = fixture();
        let base = f.outside.join("new");
        assert!(ensure_no_outside_link(&base, &base.join("cache.bin")).is_ok());
    }

    #[test]
    fn a_target_writes_into_a_new_cache_dir() {
        let f = fixture();
        let dir = f.outside.join("new/cache");
        CacheTarget::inside(&dir)
            .write_gitignore()
            .expect("gitignore");
        assert!(dir.join(".gitignore").exists());
    }

    #[test]
    fn a_dir_link_into_the_git_dir_below_the_base_is_refused() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join(".git")).expect("git dir");
        symlink(f.root.join(".git"), f.root.join("conf")).expect("link");
        let path = f.root.join("conf/config");
        assert_eq!(outside_link(&f.root, &path), Some(f.root.join("conf")));
        let error = ensure_no_outside_link(&f.root, &path).expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("Git metadata"), "{error}");
    }

    #[test]
    fn a_file_link_into_a_nested_git_dir_is_refused() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join("sub/.GIT")).expect("git dir");
        std::fs::write(f.root.join("sub/.GIT/HEAD"), "ref").expect("head");
        symlink(f.root.join("sub/.GIT/HEAD"), f.root.join("head")).expect("link");
        let path = f.root.join("head");
        assert_eq!(outside_link(&f.root, &path), Some(path));
    }

    #[test]
    fn a_link_into_the_dir_that_a_git_file_names_is_refused() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join("meta")).expect("meta");
        std::fs::write(f.root.join(".git"), "gitdir: meta\n").expect("git file");
        symlink(f.root.join("meta"), f.root.join("m")).expect("link");
        let path = f.root.join("m/config");
        assert_eq!(outside_link(&f.root, &path), Some(f.root.join("m")));
    }

    #[test]
    fn a_base_that_links_into_the_git_dir_is_refused() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join(".git/hooks")).expect("git dir");
        let base = f.root.join("alias");
        symlink(f.root.join(".git/hooks"), &base).expect("link");
        let path = base.join("pre-commit");
        assert_eq!(outside_link(&base, &path), Some(base));
    }

    #[test]
    fn a_plain_path_into_the_git_dir_passes() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join(".git/hooks")).expect("git dir");
        let path = f.root.join(".git/hooks/pre-commit");
        assert_eq!(outside_link(&f.root, &path), None);
    }

    #[test]
    fn a_link_into_node_modules_is_refused() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join("node_modules/left-pad")).expect("package");
        std::fs::write(f.root.join("node_modules/left-pad/index.js"), "x").expect("file");
        let path = f.root.join(".gitignore");
        symlink(f.root.join("node_modules/left-pad/index.js"), &path).expect("link");
        assert_eq!(outside_link(&f.root, &path), Some(path));
    }

    #[test]
    fn a_named_pipe_target_is_refused() {
        let f = fixture();
        let fifo = f.root.join("report");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("run mkfifo");
        assert!(made.success());
        assert_eq!(outside_link(&f.root, &fifo), Some(fifo));
    }

    /// A staged audit runs in a linked worktree outside the checkout and
    /// keeps its cache in the checkout. A link at the cache directory of the
    /// checkout must not take the writes out of the checkout.
    #[test]
    fn a_cache_dir_in_the_checkout_of_a_linked_worktree_is_checked_from_the_checkout() {
        let f = fixture();
        let host = f.root.join("host");
        std::fs::create_dir_all(host.join(".git/worktrees/staged")).expect("host git");
        std::fs::write(host.join(".git/worktrees/staged/commondir"), "../..\n").expect("common");
        std::fs::create_dir_all(host.join("packages/app")).expect("host app");
        let staged = f.root.join("staged");
        std::fs::create_dir_all(staged.join("packages/app")).expect("staged app");
        std::fs::write(
            staged.join(".git"),
            format!("gitdir: {}\n", host.join(".git/worktrees/staged").display()),
        )
        .expect("git file");
        symlink(&f.outside, host.join("packages/app/buildcache")).expect("link");
        let root = staged.join("packages/app");
        let dir = host.join("packages/app/buildcache");
        let target = CacheTarget::new(&root, &dir);
        assert!(target.check(&dir.join("cache.bin")).is_err());
        assert!(target.write_file("cache.bin", b"data").is_err());
        assert_eq!(std::fs::read_dir(&f.outside).expect("outside").count(), 0);
    }

    /// A folder in the checkout that links to another Git checkout does not
    /// make that checkout the base of the cache.
    #[test]
    fn a_cache_dir_through_a_link_to_another_checkout_is_checked_from_the_checkout() {
        let f = fixture();
        let host = f.root.join("host");
        std::fs::create_dir_all(host.join(".git/worktrees/staged")).expect("host git");
        std::fs::write(host.join(".git/worktrees/staged/commondir"), "../..\n").expect("common");
        std::fs::create_dir_all(host.join("packages/app")).expect("host app");
        let other = f.outside.join("other");
        std::fs::create_dir_all(other.join(".git")).expect("other git");
        symlink(&other, host.join("packages/app/link")).expect("link");
        let staged = f.root.join("staged");
        std::fs::create_dir_all(staged.join("packages/app")).expect("staged app");
        std::fs::write(
            staged.join(".git"),
            format!("gitdir: {}\n", host.join(".git/worktrees/staged").display()),
        )
        .expect("git file");
        let root = staged.join("packages/app");
        let dir = host.join("packages/app/link/cache");
        let target = CacheTarget::new(&root, &dir);
        assert!(target.write_file("cache.bin", b"data").is_err());
        assert!(!other.join("cache").exists());
    }

    /// A staged audit of a linked worktree keeps its cache in that worktree,
    /// not below the main checkout. A link in it is still checked.
    #[test]
    fn a_cache_dir_in_a_linked_worktree_host_is_checked_from_the_worktree() {
        let f = fixture();
        let main = f.root.join("repo");
        std::fs::create_dir_all(main.join(".git/worktrees/feature")).expect("main git");
        std::fs::create_dir_all(main.join(".git/worktrees/staged")).expect("staged git");
        for name in ["feature", "staged"] {
            std::fs::write(
                main.join(format!(".git/worktrees/{name}/commondir")),
                "../..\n",
            )
            .expect("common");
        }
        let host = f.root.join("wt/feature");
        std::fs::create_dir_all(host.join("packages/app")).expect("host app");
        std::fs::write(
            host.join(".git"),
            format!(
                "gitdir: {}\n",
                main.join(".git/worktrees/feature").display()
            ),
        )
        .expect("host git file");
        let staged = f.root.join("staged");
        std::fs::create_dir_all(staged.join("packages/app")).expect("staged app");
        std::fs::write(
            staged.join(".git"),
            format!("gitdir: {}\n", main.join(".git/worktrees/staged").display()),
        )
        .expect("staged git file");
        symlink(&f.outside, host.join("packages/app/build")).expect("link");
        let root = staged.join("packages/app");
        let dir = host.join("packages/app/build/cache");
        assert!(
            CacheTarget::new(&root, &dir)
                .write_file("cache.bin", b"data")
                .is_err()
        );
        assert_eq!(std::fs::read_dir(&f.outside).expect("outside").count(), 0);
    }

    /// The same for a submodule, whose Git directory is below the Git
    /// directory of the superproject.
    #[test]
    fn a_cache_dir_in_a_submodule_host_is_checked_from_the_submodule() {
        let f = fixture();
        let modules = f.root.join("super/.git/modules/sub");
        std::fs::create_dir_all(modules.join("worktrees/staged")).expect("module git");
        std::fs::write(modules.join("worktrees/staged/commondir"), "../..\n").expect("common");
        let host = f.root.join("super/sub");
        std::fs::create_dir_all(host.join("packages/app")).expect("host app");
        std::fs::write(
            host.join(".git"),
            format!("gitdir: {}\n", modules.display()),
        )
        .expect("host git file");
        let staged = f.root.join("staged");
        std::fs::create_dir_all(staged.join("packages/app")).expect("staged app");
        std::fs::write(
            staged.join(".git"),
            format!("gitdir: {}\n", modules.join("worktrees/staged").display()),
        )
        .expect("staged git file");
        symlink(&f.outside, host.join("packages/app/build")).expect("link");
        let root = staged.join("packages/app");
        let dir = host.join("packages/app/build/cache");
        assert!(
            CacheTarget::new(&root, &dir)
                .write_file("cache.bin", b"data")
                .is_err()
        );
        assert_eq!(std::fs::read_dir(&f.outside).expect("outside").count(), 0);
    }

    /// Write a `.git` file at `top` that names the linked Git directory
    /// `name` below the common Git directory `common`, with its `commondir`.
    fn linked_work_tree(common: &Path, top: &Path, name: &str) {
        let git_dir = common.join("worktrees").join(name);
        std::fs::create_dir_all(&git_dir).expect("linked git dir");
        std::fs::write(git_dir.join("commondir"), "../..\n").expect("common");
        std::fs::create_dir_all(top).expect("work tree");
        std::fs::write(top.join(".git"), format!("gitdir: {}\n", git_dir.display()))
            .expect("git file");
    }

    /// A linked worktree can live inside the main checkout. A link in it
    /// that leads up into the main checkout must not take the cache writes
    /// of a staged audit there.
    #[test]
    fn a_link_from_a_worktree_inside_the_main_checkout_up_into_it_is_refused() {
        let f = fixture();
        let repo = f.root.join("repo");
        std::fs::create_dir_all(repo.join(".git")).expect("main git");
        let host = repo.join(".worktrees/feature");
        linked_work_tree(&repo.join(".git"), &host, "feature");
        let staged = f.root.join("staged");
        linked_work_tree(&repo.join(".git"), &staged, "staged");
        symlink("../..", host.join("build")).expect("link");
        let dir = host.join("build/cache");
        for root in [&staged, &host] {
            let target = CacheTarget::new(root, &dir);
            assert!(target.write_gitignore().is_err());
            assert!(target.write_file("cache.bin", b"data").is_err());
        }
        assert!(!repo.join("cache").exists());
    }

    /// The same for two worktrees of a bare repository, one inside the
    /// other. The bare repository has no work tree of its own.
    #[test]
    fn a_link_from_a_nested_worktree_of_a_bare_repository_is_refused() {
        let f = fixture();
        let bare = f.root.join("repo.git");
        let outer = f.root.join("trees/main");
        linked_work_tree(&bare, &outer, "main");
        let host = outer.join(".worktrees/feature");
        linked_work_tree(&bare, &host, "feature");
        let staged = f.root.join("staged");
        linked_work_tree(&bare, &staged, "staged");
        symlink("../..", host.join("build")).expect("link");
        let dir = host.join("build/cache");
        assert!(
            CacheTarget::new(&staged, &dir)
                .write_file("cache.bin", b"data")
                .is_err()
        );
        assert!(!outer.join("cache").exists());
    }

    /// A work tree that is reached through a link is never the base, so the
    /// link is checked from the work tree above it.
    #[test]
    fn a_work_tree_behind_a_link_is_not_the_base() {
        let f = fixture();
        let repo = f.root.join("repo");
        std::fs::create_dir_all(repo.join(".git")).expect("main git");
        let host = f.root.join("host");
        linked_work_tree(&repo.join(".git"), &host, "host");
        let other = f.outside.join("other");
        linked_work_tree(&repo.join(".git"), &other, "other");
        linked_work_tree(&repo.join(".git"), &other.join("nested"), "nested");
        let staged = f.root.join("staged");
        linked_work_tree(&repo.join(".git"), &staged, "staged");
        symlink(&other, host.join("shared")).expect("link");
        let dir = host.join("shared/nested/cache");
        assert!(
            CacheTarget::new(&staged, &dir)
                .write_file("cache.bin", b"data")
                .is_err()
        );
        assert!(!other.join("nested/cache").exists());
    }

    /// A worktree of a bare repository lies inside the common Git directory.
    /// Its files are not Git metadata, but the Git directory still is.
    #[test]
    fn a_worktree_inside_a_bare_repository_is_not_git_metadata() {
        let f = fixture();
        let bare = f.root.join("proj.git");
        let main = bare.join("main");
        linked_work_tree(&bare, &main, "main");
        std::fs::create_dir_all(main.join("src")).expect("src");
        std::fs::create_dir_all(bare.join("hooks")).expect("hooks");
        let canonical_main = std::fs::canonicalize(&main).expect("main");
        let canonical_bare = std::fs::canonicalize(&bare).expect("bare");
        assert!(!is_protected_link_target(
            &canonical_main,
            &canonical_main.join("src/a.ts")
        ));
        assert!(is_protected_link_target(
            &canonical_main,
            &canonical_bare.join("hooks/pre-commit")
        ));
        assert!(is_protected_link_target(
            &canonical_main,
            &canonical_bare.join("worktrees/main/HEAD")
        ));
        std::fs::create_dir_all(main.join("build/cache")).expect("cache");
        symlink("build/cache", main.join(".fallow")).expect("link");
        let dir = main.join(".fallow");
        CacheTarget::new(&main, &dir)
            .write_file("cache.bin", b"data")
            .expect("file");
        symlink(bare.join("hooks"), main.join("hooks")).expect("hooks link");
        let error =
            ensure_no_outside_link(&main, &main.join("hooks/pre-commit")).expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    /// A `commondir` file that cannot be read leaves the common Git
    /// directory unknown, so every link target counts as protected.
    #[test]
    fn an_unreadable_commondir_protects_every_link_target() {
        let f = fixture();
        let repo = f.root.join("repo");
        std::fs::create_dir_all(repo.join(".git/worktrees/main/commondir")).expect("git dir");
        let main = f.root.join("main");
        std::fs::create_dir_all(&main).expect("main");
        std::fs::write(
            main.join(".git"),
            format!("gitdir: {}\n", repo.join(".git/worktrees/main").display()),
        )
        .expect("git file");
        let canonical = std::fs::canonicalize(&main).expect("main");
        assert!(is_protected_link_target(
            &canonical,
            &canonical.join("src/a.ts")
        ));
    }

    /// `FALLOW_CACHE_DIR=../shared` from a subdirectory root names a folder
    /// in the same work tree. The `..` is resolved on disk, so the save
    /// writes, and a link before the `..` is still checked.
    #[test]
    fn a_user_cache_dir_with_a_parent_step_in_the_work_tree_writes() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join(".git")).expect("git dir");
        let root = f.root.join("pkg");
        std::fs::create_dir_all(&root).expect("pkg");
        let dir = root.join("../shared");
        assert_eq!(cache_write_base(&root, &dir), f.root);
        CacheTarget::new(&root, &dir)
            .write_file("cache.bin", b"data")
            .expect("file");
        assert!(f.root.join("shared/cache.bin").exists());

        std::fs::create_dir_all(f.outside.join("sub")).expect("sub");
        symlink(f.outside.join("sub"), root.join("link")).expect("link");
        let through = root.join("link/../elsewhere");
        let error = CacheTarget::new(&root, &through)
            .write_file("cache.bin", b"data")
            .expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("outside"), "{error}");
        assert!(!f.outside.join("elsewhere").exists());
    }

    /// The refusal names the real reason.
    #[test]
    fn the_refusal_names_the_reason() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join("node_modules/pkg")).expect("packages");
        symlink(f.root.join("node_modules/pkg"), f.root.join(".fallow")).expect("link");
        let error = ensure_no_outside_link(&f.root, &f.root.join(".fallow/cache.bin"))
            .expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("node_modules"), "{error}");
        assert!(!error.to_string().contains("outside"), "{error}");
    }

    #[test]
    fn a_cache_dir_above_a_subdirectory_root_is_checked_from_the_work_tree() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join(".git")).expect("git dir");
        std::fs::create_dir_all(f.root.join("packages/app")).expect("app");
        symlink(&f.outside, f.root.join(".cache")).expect("link");
        let root = f.root.join("packages/app");
        let dir = f.root.join(".cache");
        assert!(
            CacheTarget::new(&root, &dir)
                .check(&dir.join("cache.bin"))
                .is_err()
        );
    }

    #[test]
    fn a_cache_dir_in_another_checkout_is_its_own_base() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join(".git")).expect("git dir");
        let other = f.outside.join("home");
        std::fs::create_dir_all(other.join(".git")).expect("other git");
        let elsewhere = f.outside.join("disk");
        std::fs::create_dir_all(&elsewhere).expect("disk");
        symlink(&elsewhere, other.join(".cache")).expect("link");
        let dir = other.join(".cache/fallow");
        let target = CacheTarget::new(&f.root, &dir);
        target.write_file("cache.bin", b"data").expect("file");
        assert!(elsewhere.join("fallow/cache.bin").exists());
    }

    #[test]
    fn a_user_cache_dir_outside_the_root_is_checked_from_itself() {
        let f = fixture();
        let target = CacheTarget::new(&f.root, &f.outside);
        target.write_file("cache.bin", b"data").expect("file");
        assert!(f.outside.join("cache.bin").exists());
    }

    /// A submodule below a superproject whose Git directory is outside its
    /// work tree (`git init --separate-git-dir`). The submodule keeps its
    /// own Git directory below `modules/` of that directory. Returns the
    /// canonical submodule root and the canonical Git directory of the
    /// superproject.
    fn submodule_of_separate_git_dir(f: &Fixture) -> (PathBuf, PathBuf) {
        let git_dir = f.outside.join("super.git");
        let module = git_dir.join("modules/lib");
        std::fs::create_dir_all(git_dir.join("hooks")).expect("super hooks");
        std::fs::create_dir_all(module.join("hooks")).expect("module hooks");
        let superproject = f.root.join("super");
        let submodule = superproject.join("vendor/lib");
        std::fs::create_dir_all(submodule.join("src")).expect("submodule");
        std::fs::write(
            superproject.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .expect("super git file");
        std::fs::write(
            submodule.join(".git"),
            format!("gitdir: {}\n", module.display()),
        )
        .expect("submodule git file");
        (
            std::fs::canonicalize(&submodule).expect("submodule"),
            std::fs::canonicalize(&git_dir).expect("super git dir"),
        )
    }

    /// The Git directory of the superproject holds its hooks and config.
    /// A link target there counts as Git metadata also from the submodule.
    #[test]
    fn a_link_into_the_git_dir_of_the_superproject_is_refused() {
        let f = fixture();
        let (submodule, git_dir) = submodule_of_separate_git_dir(&f);
        assert!(is_protected_link_target(
            &submodule,
            &git_dir.join("hooks/pre-commit")
        ));
        assert!(is_protected_link_target(
            &submodule.join("src"),
            &git_dir.join("config")
        ));
    }

    /// The Git directory of the submodule itself stays protected, and a link
    /// to an ordinary file of the submodule stays allowed.
    #[test]
    fn a_submodule_root_protects_its_own_git_dir_only() {
        let f = fixture();
        let (submodule, git_dir) = submodule_of_separate_git_dir(&f);
        assert!(is_protected_link_target(
            &submodule,
            &git_dir.join("modules/lib/hooks/pre-commit")
        ));
        assert!(!is_protected_link_target(
            &submodule,
            &submodule.join("src/a.ts")
        ));
        let other = std::fs::canonicalize(&f.outside).expect("outside");
        assert!(!is_protected_link_target(
            &submodule,
            &other.join("report.json")
        ));
    }

    /// The common Git directory of a worktree of the superproject is
    /// protected for a submodule in that worktree.
    #[test]
    fn a_link_into_the_common_dir_of_the_superproject_is_refused() {
        let f = fixture();
        let common = f.outside.join("super.git");
        std::fs::create_dir_all(common.join("hooks")).expect("hooks");
        let superproject = f.root.join("wt");
        linked_work_tree(&common, &superproject, "wt");
        let submodule = superproject.join("lib");
        let module = common.join("worktrees/wt/modules/lib");
        std::fs::create_dir_all(&module).expect("module");
        std::fs::create_dir_all(&submodule).expect("submodule");
        std::fs::write(
            submodule.join(".git"),
            format!("gitdir: {}\n", module.display()),
        )
        .expect("git file");
        let submodule = std::fs::canonicalize(&submodule).expect("submodule");
        let common = std::fs::canonicalize(&common).expect("common");
        assert!(is_protected_link_target(
            &submodule,
            &common.join("hooks/pre-commit")
        ));
    }

    /// A `.git` file that names no Git directory leaves the Git directory
    /// unknown, so every link target counts as protected. The same holds
    /// for such a file in a superproject.
    #[test]
    fn a_git_file_without_a_gitdir_line_protects_every_link_target() {
        let f = fixture();
        let superproject = f.root.join("super");
        let submodule = superproject.join("lib");
        std::fs::create_dir_all(&submodule).expect("submodule");
        std::fs::create_dir_all(submodule.join(".git")).expect("submodule git dir");
        std::fs::write(superproject.join(".git"), "not a git file\n").expect("git file");
        let submodule = std::fs::canonicalize(&submodule).expect("submodule");
        assert!(is_protected_link_target(
            &submodule,
            &submodule.join("src/a.ts")
        ));
    }

    /// A link target that leaves the base and comes back through `..` is
    /// refused at the step that leaves. The answer does not depend on whether
    /// the directory outside the base exists.
    #[test]
    fn a_link_that_leaves_the_base_and_comes_back_is_refused() {
        for middle_exists in [true, false] {
            let f = fixture();
            std::fs::create_dir_all(f.root.join(".fallow")).expect("cache");
            std::fs::write(f.root.join("ignore-all"), "*\n").expect("file");
            if middle_exists {
                std::fs::create_dir_all(f.outside.join("probe")).expect("probe");
            }
            let relative = f.root.join(".fallow/.gitignore");
            symlink("../../outside/probe/../../project/ignore-all", &relative).expect("link");
            assert_eq!(
                outside_link(&f.root, &relative),
                Some(relative.clone()),
                "relative target, middle exists: {middle_exists}"
            );
            assert!(ensure_no_outside_link(&f.root, &relative).is_err());

            let absolute = f.root.join(".fallow/absolute");
            let target = f.outside.join("probe/../../project/ignore-all");
            symlink(&target, &absolute).expect("link");
            assert_eq!(
                outside_link(&f.root, &absolute),
                Some(absolute.clone()),
                "absolute target, middle exists: {middle_exists}"
            );
        }
    }

    /// A link to a second link is refused when the second link leaves the
    /// base and comes back.
    #[test]
    fn a_chained_link_that_leaves_the_base_and_comes_back_is_refused() {
        let f = fixture();
        std::fs::create_dir_all(f.outside.join("probe")).expect("probe");
        std::fs::write(f.root.join("ignore-all"), "*\n").expect("file");
        symlink(
            "../outside/probe/../../project/ignore-all",
            f.root.join("hop"),
        )
        .expect("hop");
        symlink("hop", f.root.join(".fallow")).expect("link");
        let path = f.root.join(".fallow");
        assert_eq!(outside_link(&f.root, &path), Some(path));
    }

    /// A link with an absolute target that names the base through another
    /// spelling, such as `/var` for `/private/var` on macOS, stays allowed.
    #[test]
    fn an_absolute_link_inside_the_base_passes() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join("build/cache")).expect("target");
        symlink(f.root.join("build/./cache"), f.root.join(".fallow")).expect("link");
        assert_eq!(
            outside_link(&f.root, &f.root.join(".fallow/cache.bin")),
            None
        );
        let canonical = std::fs::canonicalize(&f.root).expect("root");
        symlink(canonical.join("build"), f.root.join("out")).expect("link");
        assert_eq!(outside_link(&f.root, &f.root.join("out/cache")), None);
    }

    /// A link inside the base that steps up and down inside the base stays
    /// allowed, as the links of a pnpm `node_modules` do.
    #[test]
    fn a_relative_link_with_parent_steps_inside_the_base_passes() {
        let f = fixture();
        std::fs::create_dir_all(f.root.join("packages/app/src")).expect("app");
        std::fs::create_dir_all(f.root.join("build")).expect("build");
        symlink("../../../build", f.root.join("packages/app/src/out")).expect("link");
        assert_eq!(
            outside_link(&f.root, &f.root.join("packages/app/src/out/x.bin")),
            None
        );
    }

    /// A `.git` file that is not text leaves the Git directory unknown too.
    #[test]
    fn an_unreadable_git_file_protects_every_link_target() {
        let f = fixture();
        let root = f.root.join("repo");
        std::fs::create_dir_all(&root).expect("repo");
        std::fs::write(root.join(".git"), [0xff, 0xfe, b'\n']).expect("git file");
        let root = std::fs::canonicalize(&root).expect("repo");
        assert!(is_protected_link_target(&root, &root.join("src/a.ts")));
    }

    /// A `.git` entry that is a named pipe is not read, because the read
    /// would block. The Git directory stays unknown.
    #[test]
    fn a_named_pipe_git_entry_protects_every_link_target() {
        let f = fixture();
        let root = f.root.join("repo");
        std::fs::create_dir_all(&root).expect("repo");
        let made = std::process::Command::new("mkfifo")
            .arg(root.join(".git"))
            .status()
            .expect("run mkfifo");
        assert!(made.success());
        let root = std::fs::canonicalize(&root).expect("repo");
        assert!(is_protected_link_target(&root, &root.join("src/a.ts")));
    }
}
