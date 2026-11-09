//! Refuse writes through a symlink that leaves a trusted directory.
//!
//! A cloned repository can commit a symlink where fallow writes a file, for
//! example `.fallow/.gitignore` or the `.fallow` directory itself. A write
//! through that link reaches a file outside the project. A link that resolves
//! inside the base directory is safe and stays allowed.

use std::path::{Component, Path, PathBuf};

/// The first entry below `base`, up to and including `path`, that is a
/// symlink whose resolved target is outside the canonical `base`. A dangling
/// link also counts, because its target can appear outside `base` later.
/// A path outside `base`, one with a `..` or `.` component, or a `base` that
/// does not resolve returns `path` itself. A missing entry ends the walk,
/// because nothing below it exists yet.
#[must_use]
pub fn outside_link(base: &Path, path: &Path) -> Option<PathBuf> {
    let Ok(relative) = path.strip_prefix(base) else {
        return Some(path.to_path_buf());
    };
    let Ok(canonical_base) = std::fs::canonicalize(base) else {
        return Some(path.to_path_buf());
    };
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Some(path.to_path_buf());
        };
        current.push(name);
        match current.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                let inside = std::fs::canonicalize(&current)
                    .is_ok_and(|resolved| resolved.starts_with(&canonical_base));
                if !inside {
                    return Some(current);
                }
            }
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

/// The directory that cache writes must stay in. A cache directory inside the
/// project root is checked from the root, so a committed `.fallow` link is
/// refused. A cache directory outside the root comes from the user, such as
/// `FALLOW_CACHE_DIR` on a CI runner, so only the entries below it are checked.
/// A `..` in the cache directory is resolved by text first, so
/// `cache.dir: "../shared"` counts as a directory outside the root.
#[must_use]
pub fn cache_write_base<'a>(root: &'a Path, cache_dir: &'a Path) -> &'a Path {
    if lexically_normalized(cache_dir).starts_with(root) {
        root
    } else {
        cache_dir
    }
}

/// `path` with each `..` applied to the component before it, and each `.`
/// dropped, without a file system read.
fn lexically_normalized(path: &Path) -> PathBuf {
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
/// Returns a `PermissionDenied` error that names the link.
pub fn ensure_no_outside_link(base: &Path, path: &Path) -> std::io::Result<()> {
    if base.symlink_metadata().is_err() {
        return Ok(());
    }
    match outside_link(base, path) {
        None => Ok(()),
        Some(link) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "refusing to write {}: {} is a symlink that resolves outside {}",
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
    write_base: &'a Path,
}

impl<'a> CacheTarget<'a> {
    /// The cache directory `dir` of the project at `root`.
    #[must_use]
    pub fn new(root: &'a Path, dir: &'a Path) -> Self {
        Self {
            dir,
            write_base: cache_write_base(root, dir),
        }
    }

    /// A cache directory that is its own base, such as a directory that the
    /// user chose.
    #[must_use]
    pub const fn inside(dir: &'a Path) -> Self {
        Self {
            dir,
            write_base: dir,
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
            write_base: self.write_base,
        }
    }

    /// Fail when a write to `path` would follow a symlink that leaves the
    /// base.
    ///
    /// # Errors
    ///
    /// Returns the error of [`ensure_no_outside_link`].
    pub fn check(&self, path: &Path) -> std::io::Result<()> {
        ensure_no_outside_link(self.write_base, path)
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
    fn a_user_cache_dir_outside_the_root_is_checked_from_itself() {
        let f = fixture();
        let target = CacheTarget::new(&f.root, &f.outside);
        target.write_file("cache.bin", b"data").expect("file");
        assert!(f.outside.join("cache.bin").exists());
    }
}
