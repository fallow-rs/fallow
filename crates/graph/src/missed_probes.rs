//! The paths that import resolution checked and did not find.
//!
//! The persisted graph cache reuses import resolutions. A resolution can
//! depend on a path that did not exist: for `./util`, the resolver tries the
//! extensionless file `util` before `util.ts`. Discovery does not list an
//! extensionless file, a `.json` file or build output in an ignored
//! directory, so a new file at such a path does not change the discovered
//! file set. The graph cache stores the missed paths, and a warm run refuses
//! the cache when one of them exists (#4233).
//!
//! The resolver and the resolution fallbacks report each missed path to
//! `note`. A `MissRecorder` collects the reported paths below its project
//! root. The recorders are process-wide, because resolution runs on rayon
//! workers and the fallbacks have no handle to a session. Two analyses of the
//! same root at the same time each record the misses of both. That only adds
//! paths, so a warm run can refuse a cache that it could use, but it never
//! reuses a stale one.
//!
//! The existence checks follow symlinks, as the resolver does, so a dangling
//! symlink counts as a missing path. Paths outside the project root are not
//! recorded, which includes a workspace package outside the root.

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use dashmap::DashSet;
use rustc_hash::FxBuildHasher;

/// The number of open recorders. A zero count keeps [`note`] at one atomic
/// load.
static OPEN: AtomicUsize = AtomicUsize::new(0);
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
static RECORDERS: RwLock<Vec<Active>> = RwLock::new(Vec::new());

struct Active {
    id: u64,
    /// The project root, and its canonical spelling when that differs.
    scopes: Vec<PathBuf>,
    /// Root-relative missed paths.
    paths: DashSet<PathBuf, FxBuildHasher>,
}

/// Collects the missed paths below one project root until [`Self::finish`].
#[derive(Debug)]
pub(crate) struct MissRecorder {
    id: u64,
}

impl MissRecorder {
    /// Start to record the missed paths below `root`.
    pub(crate) fn start(root: &Path) -> Self {
        let mut scopes = vec![root.to_path_buf()];
        if let Ok(canonical) = dunce::canonicalize(root)
            && canonical != root
        {
            scopes.push(canonical);
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        RECORDERS
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(Active {
                id,
                scopes,
                paths: DashSet::with_hasher(FxBuildHasher),
            });
        OPEN.fetch_add(1, Ordering::SeqCst);
        Self { id }
    }

    /// Stop the recorder and return the missed paths.
    pub(crate) fn finish(self) -> MissedProbes {
        let Some(active) = remove(self.id) else {
            return MissedProbes::default();
        };
        let paths: Vec<String> = active
            .paths
            .into_iter()
            .map(|path| portable(&path))
            .collect();
        MissedProbes::from_paths(paths)
    }
}

/// Root-relative missed paths, grouped by their parent directory.
///
/// A resolver tries many extensions for one specifier in one directory, so
/// the grouping keeps the stored form small. It also lets the check skip
/// each name below a directory that does not exist.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MissedProbes {
    /// Sorted by directory, each with sorted file names.
    dirs: Vec<MissedDir>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct MissedDir {
    /// Root-relative with forward slashes, empty for the root itself.
    dir: String,
    names: Vec<String>,
}

impl MissedProbes {
    pub(crate) fn from_paths(paths: Vec<String>) -> Self {
        let mut by_dir: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for path in paths {
            let (dir, name) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
            by_dir
                .entry(dir.to_string())
                .or_default()
                .push(name.to_string());
        }
        let dirs = by_dir
            .into_iter()
            .map(|(dir, mut names)| {
                names.sort_unstable();
                names.dedup();
                MissedDir { dir, names }
            })
            .collect();
        Self { dirs }
    }

    /// The number of missed paths.
    #[must_use]
    pub fn len(&self) -> usize {
        self.dirs.iter().map(|entry| entry.names.len()).sum()
    }

    /// True when no path was missed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty()
    }

    /// True when a missed path exists below `root` now.
    #[must_use]
    pub fn any_exists(&self, root: &Path) -> bool {
        let check = |entry: &MissedDir| {
            let dir = root.join(&entry.dir);
            // A missing directory holds none of the names, and the creation
            // of the directory itself is a separate missed path when a
            // resolution step checked it.
            dir.metadata().is_ok()
                && entry
                    .names
                    .iter()
                    .any(|name| dir.join(name).metadata().is_ok())
        };
        if self.dirs.len() < PARALLEL_MIN {
            return self.dirs.iter().any(check);
        }
        use rayon::prelude::*;
        self.dirs.par_iter().any(check)
    }

    /// The missed paths, root-relative with forward slashes.
    pub fn paths(&self) -> impl Iterator<Item = String> + '_ {
        self.dirs.iter().flat_map(|entry| {
            entry.names.iter().map(move |name| {
                if entry.dir.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{name}", entry.dir)
                }
            })
        })
    }
}

/// A panic in resolution must not leave the recorder open, or each later
/// miss takes the lock.
impl Drop for MissRecorder {
    fn drop(&mut self) {
        let _ = remove(self.id);
    }
}

fn remove(id: u64) -> Option<Active> {
    let active = {
        let mut recorders = RECORDERS.write().unwrap_or_else(|e| e.into_inner());
        let index = recorders.iter().position(|active| active.id == id)?;
        recorders.swap_remove(index)
    };
    OPEN.fetch_sub(1, Ordering::SeqCst);
    Some(active)
}

/// Report that a resolution step found nothing at `path`.
///
/// A path below `node_modules` is not recorded: the graph cache key reads
/// the lockfiles and install markers that a package manager writes on each
/// install, and these cover the content of `node_modules`.
pub(crate) fn note(path: &Path) {
    if OPEN.load(Ordering::Relaxed) == 0 || !path.is_absolute() {
        return;
    }
    let recorders = RECORDERS.read().unwrap_or_else(|e| e.into_inner());
    for active in recorders.iter() {
        let Some(relative) = active
            .scopes
            .iter()
            .find_map(|scope| path.strip_prefix(scope).ok())
        else {
            continue;
        };
        if relative.as_os_str().is_empty() || is_below_node_modules(relative) {
            continue;
        }
        if !active.paths.contains(relative) {
            active.paths.insert(relative.to_path_buf());
        }
    }
}

/// Report `path` when `result` failed because the path does not exist.
pub(crate) fn note_if_missing<T>(path: &Path, result: &io::Result<T>) {
    if result
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    {
        note(path);
    }
}

/// The root-relative spelling of `path`, with forward slashes, when it is
/// below `root` and not below `node_modules`.
#[must_use]
pub(crate) fn project_relative(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() || is_below_node_modules(relative) {
        return None;
    }
    Some(portable(relative))
}

/// The stored spelling of a root-relative path. Only Windows separators
/// change: on Unix a backslash is a valid file name character.
fn portable(relative: &Path) -> String {
    let text = relative.to_string_lossy();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

fn is_below_node_modules(relative: &Path) -> bool {
    relative
        .components()
        .any(|component| component == Component::Normal("node_modules".as_ref()))
}

/// True when a path of `found` (root-relative) does not exist below `root`
/// now.
#[must_use]
pub fn any_missing(root: &Path, found: &[String]) -> bool {
    let check = |relative: &String| root.join(relative).metadata().is_err();
    if found.len() < PARALLEL_MIN {
        return found.iter().any(check);
    }
    use rayon::prelude::*;
    found.par_iter().any(check)
}

/// Below this number of entries, a check runs on the calling thread: the
/// rayon pool costs more than a few `stat` calls.
const PARALLEL_MIN: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_paths_by_directory() {
        let missed = MissedProbes::from_paths(vec![
            "a/b/c.ts".to_string(),
            "a/b-x".to_string(),
            "a/b".to_string(),
            "a/b/c.ts".to_string(),
            "a/b/a.ts".to_string(),
            "top".to_string(),
        ]);
        assert_eq!(missed.len(), 5);
        let paths: Vec<String> = missed.paths().collect();
        assert_eq!(paths, ["top", "a/b", "a/b-x", "a/b/a.ts", "a/b/c.ts"]);
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn records_missed_paths_below_the_root_only() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let recorder = MissRecorder::start(&root);
        note(&root.join("src/util"));
        note(&root.join("src/util"));
        note(&root.join("node_modules/pkg/index.js"));
        note(&temp.path().join("outside.ts"));
        note(Path::new("relative.ts"));
        let missed: Vec<String> = recorder.finish().paths().collect();
        assert_eq!(missed, vec!["src/util".to_string()]);
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_missed_path_that_appears_is_seen() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let missed = MissedProbes::from_paths(vec!["src/util".to_string(), "index".to_string()]);
        assert!(!missed.any_exists(root));
        std::fs::create_dir_all(root.join("src")).unwrap();
        assert!(!missed.any_exists(root));
        std::fs::write(root.join("src/util"), "").unwrap();
        assert!(missed.any_exists(root));
        std::fs::remove_file(root.join("src/util")).unwrap();
        std::fs::write(root.join("index"), "").unwrap();
        assert!(missed.any_exists(root));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn the_parallel_check_sees_one_changed_path() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let paths: Vec<String> = (0..PARALLEL_MIN * 2).map(|i| format!("d{i}/x")).collect();
        let missed = MissedProbes::from_paths(paths.clone());
        assert!(!missed.any_exists(root));
        std::fs::create_dir_all(root.join("d77")).unwrap();
        std::fs::write(root.join("d77/x"), "").unwrap();
        assert!(missed.any_exists(root));
        assert!(any_missing(root, &paths));
        let present: Vec<String> = vec!["d77/x".to_string(); PARALLEL_MIN * 2];
        assert!(!any_missing(root, &present));
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_dangling_symlink_is_a_missing_path() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::os::unix::fs::symlink(root.join("dist/util.js"), root.join("util")).unwrap();
        let missed = MissedProbes::from_paths(vec!["util".to_string()]);
        assert!(!missed.any_exists(root));
        assert!(any_missing(root, &["util".to_string()]));
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/util.js"), "").unwrap();
        assert!(missed.any_exists(root));
        assert!(!any_missing(root, &["util".to_string()]));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn only_a_not_found_error_is_a_miss() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let file = root.join("file.ts");
        std::fs::write(&file, "").unwrap();
        let recorder = MissRecorder::start(root);
        let below_file = file.join("child.ts");
        note_if_missing(&below_file, &std::fs::metadata(&below_file));
        let absent = root.join("absent.ts");
        note_if_missing(&absent, &std::fs::metadata(&absent));
        let missed: Vec<String> = recorder.finish().paths().collect();
        assert_eq!(missed, vec!["absent.ts".to_string()]);
    }
}
