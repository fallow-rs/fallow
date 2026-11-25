//! Refuse writes through a symlink that leaves a trusted directory.
//!
//! A cloned repository can commit a symlink where fallow writes a file, for
//! example `.fallow/.gitignore` or the `.fallow` directory itself. A write
//! through that link reaches a file outside the project. A link that resolves
//! inside the base directory is safe and stays allowed, unless it leads into
//! Git metadata or into an installed package (see
//! [`is_protected_link_target`]).

use std::ffi::OsStr;
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

/// The first entry below `base`, up to and including `path`, that is a
/// symlink whose resolved target is outside the canonical `base`, or that
/// [`is_protected_link_target`] refuses. A dangling link also counts, because
/// its target can appear outside `base` later. A `base` that is itself a link
/// into a protected directory returns `base`. An existing `path` that is not
/// a regular file or a directory (a device, a named pipe or a socket) returns
/// `path`. A path outside `base`, one with a `..` or `.` component, a `base`
/// that does not resolve, or an entry whose metadata cannot be read returns
/// that path. A missing entry ends the walk, because nothing below it exists
/// yet.
#[must_use]
pub fn outside_link(base: &Path, path: &Path) -> Option<PathBuf> {
    let Ok(relative) = path.strip_prefix(base) else {
        return Some(path.to_path_buf());
    };
    let Ok(canonical_base) = std::fs::canonicalize(base) else {
        return Some(path.to_path_buf());
    };
    if base_is_protected_link(base, &canonical_base) {
        return Some(base.to_path_buf());
    }
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Some(path.to_path_buf());
        };
        current.push(name);
        match current.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                let allowed = std::fs::canonicalize(&current).is_ok_and(|resolved| {
                    resolved.starts_with(&canonical_base)
                        && !is_protected_link_target(&canonical_base, &resolved)
                });
                if !allowed {
                    return Some(current);
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => return Some(current),
        }
    }
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_file() && !meta.is_dir() => Some(path.to_path_buf()),
        _ => None,
    }
}

/// Whether `base` is a symlink whose target is a protected directory, seen
/// from the directory that contains the link.
fn base_is_protected_link(base: &Path, canonical_base: &Path) -> bool {
    if !base
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return false;
    }
    let Some(parent) = base
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
    else {
        return true;
    };
    is_protected_link_target(&parent, canonical_base)
}

/// Whether a symlink whose canonical target is `resolved` leads into Git
/// metadata or into an installed package, so fallow must not write through
/// it. `within` is the canonical directory the link was found in.
///
/// The target is protected when the part of it below `within` (all of it when
/// it is not below `within`) has a `.git` component (in any case) or a
/// `node_modules` component, or when it is inside the Git directory or the
/// common Git directory of the work tree that contains `within`. A `.git`
/// file can name a Git directory with any name, so the second test covers a
/// linked worktree and a submodule.
///
/// The rule is for links only. A direct path into `.git`, such as a hook that
/// `fallow hooks install` writes on purpose, does not go through it.
#[must_use]
pub fn is_protected_link_target(within: &Path, resolved: &Path) -> bool {
    let below = resolved.strip_prefix(within).unwrap_or(resolved);
    let protected_name = below.components().any(|component| match component {
        Component::Normal(name) => is_protected_name(name),
        _ => false,
    });
    protected_name
        || git_dirs(within)
            .iter()
            .any(|git_dir| resolved.starts_with(git_dir))
}

fn is_protected_name(name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        name.eq_ignore_ascii_case(GIT_ENTRY) || name.eq_ignore_ascii_case(PACKAGE_DIR)
    })
}

/// The canonical Git directory and common Git directory of the work tree
/// that contains `dir`. Empty when no ancestor of `dir` has a `.git` entry.
fn git_dirs(dir: &Path) -> Vec<PathBuf> {
    let Some(top) = work_tree_top(dir) else {
        return Vec::new();
    };
    let entry = top.join(GIT_ENTRY);
    let Some(git_dir) = git_dir_of_entry(&entry) else {
        return Vec::new();
    };
    let mut dirs = vec![git_dir.clone()];
    if let Some(common) = common_dir_of(&git_dir)
        && common != git_dir
    {
        dirs.push(common);
    }
    dirs
}

/// The nearest ancestor of `dir`, `dir` included, with a `.git` entry.
fn work_tree_top(dir: &Path) -> Option<&Path> {
    dir.ancestors()
        .find(|ancestor| ancestor.join(GIT_ENTRY).symlink_metadata().is_ok())
}

/// The canonical Git directory that the `.git` entry `entry` stands for: the
/// directory itself, or the directory that a `.git` file names.
fn git_dir_of_entry(entry: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(entry).ok()?;
    if meta.is_dir() {
        return std::fs::canonicalize(entry).ok();
    }
    let text = std::fs::read_to_string(entry).ok()?;
    let named = text
        .lines()
        .find_map(|line| line.strip_prefix(GITDIR_PREFIX))?
        .trim();
    let parent = entry.parent()?;
    std::fs::canonicalize(parent.join(named)).ok()
}

/// The canonical common Git directory of the Git directory `git_dir`: the
/// directory that its `commondir` file names, or `git_dir` itself.
fn common_dir_of(git_dir: &Path) -> Option<PathBuf> {
    match std::fs::read_to_string(git_dir.join(COMMONDIR_FILE)) {
        Ok(text) => std::fs::canonicalize(git_dir.join(text.trim())).ok(),
        Err(_) => Some(git_dir.to_path_buf()),
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
/// `cache.dir: "../shared"` counts as a directory outside the root.
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
/// still checked.
///
/// # Errors
///
/// Returns a `PermissionDenied` error that names the entry that
/// [`outside_link`] refuses.
pub fn ensure_no_outside_link(base: &Path, path: &Path) -> std::io::Result<()> {
    if base.symlink_metadata().is_err() {
        return Ok(());
    }
    match outside_link(base, path) {
        None => Ok(()),
        Some(link) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "refusing to write {}: {} is a symlink that resolves outside {} or into a .git or node_modules directory, or it is not a regular file or directory",
                path.display(),
                link.display(),
                base.display()
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
    /// symlink check.
    ///
    /// # Errors
    ///
    /// Returns the error of the check, of the directory creation, or of the
    /// write.
    pub fn write_file(&self, name: &str, data: &[u8]) -> std::io::Result<()> {
        let path = self.dir.join(name);
        self.check(&path)?;
        std::fs::create_dir_all(self.dir)?;
        crate::atomic_file::write_atomic(&path, data)
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
        assert_eq!(outside_link(&f.root, &link), Some(link));
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
}
