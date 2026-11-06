//! Process-wide registry of paths that the signal cleanup deletes.
//!
//! A signal ends Fallow with `exit`, so no destructor runs. An owner that
//! deletes a temp directory in `Drop` creates the directory through this
//! registry, and the signal cleanup deletes it after it ends the registered
//! children. The children must end first: a live child can still write into
//! the directory.
//!
//! The signal cleanup closes the registry before it kills the children. The
//! process keeps running until the exit, and a worker can then try to start
//! a new child (for example a mutation companion that replaces a killed
//! one). A closed registry refuses to create new paths, so no path can
//! appear after the deletion and stay after the exit.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::Duration;

use rustc_hash::FxHashMap;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static PATHS: OnceLock<PathRegistry> = OnceLock::new();

type Entries = FxHashMap<u64, Vec<PathBuf>>;

#[derive(Default)]
struct PathRegistry {
    entries: Mutex<Entries>,
    /// True after a signal cleanup began. The flag stays set: the child
    /// registry also drains only once per process. It is outside the mutex,
    /// so that a drain can close the registry while a creation holds the
    /// lock. That creation still registers its paths, and the deletion
    /// waits for the lock.
    closed: AtomicBool,
}

impl PathRegistry {
    fn lock(&self) -> MutexGuard<'_, Entries> {
        self.entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

fn paths() -> &'static PathRegistry {
    PATHS.get_or_init(PathRegistry::default)
}

/// Calls `create`, which must create `paths_to_delete`, and registers the
/// paths for deletion on a signal. The signal cleanup waits until `create`
/// returns, so a signal deletes either all paths of `create` or none of
/// them. Keep `create` short and free of waits on other threads.
///
/// The signal cleanup deletes the paths in order. A directory goes with its
/// contents. The deletion stops at the first path that it cannot delete, so
/// a later path (for example a lock file that marks the directory) stays
/// when an earlier path stays.
///
/// Returns the value of `create` and the key for
/// [`deregister_cleanup_paths`]. After a signal cleanup began, the call
/// does not call `create` and returns an error of kind
/// [`io::ErrorKind::Interrupted`]. An error of `create` registers nothing,
/// so `create` must delete each path that it made before the error.
///
/// The call holds the registry lock during `create`. A panic hook that
/// runs in that time cannot take the lock and skips the deletion, so keep
/// `create` to a few file system calls.
pub fn create_cleanup_paths<T>(
    paths_to_delete: Vec<PathBuf>,
    create: impl FnOnce() -> io::Result<T>,
) -> io::Result<(T, u64)> {
    create_in(paths(), paths_to_delete, create)
}

#[expect(
    clippy::significant_drop_tightening,
    reason = "the lock covers `create`, so a signal deletes all of its paths or none"
)]
fn create_in<T>(
    registry: &PathRegistry,
    paths_to_delete: Vec<PathBuf>,
    create: impl FnOnce() -> io::Result<T>,
) -> io::Result<(T, u64)> {
    let mut entries = registry.lock();
    if registry.closed.load(Ordering::SeqCst) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "the signal cleanup has begun",
        ));
    }
    let value = create()?;
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    entries.insert(id, paths_to_delete);
    Ok((value, id))
}

/// Removes the entry for `id`. Idempotent. An owner calls it after it
/// deleted the paths itself, so that a signal during that delete still
/// deletes them.
pub fn deregister_cleanup_paths(id: u64) {
    paths().lock().remove(&id);
}

/// True while the entry for `id` is registered. An owner test uses it to
/// check that its paths stay registered until they are gone.
pub fn is_cleanup_registered(id: u64) -> bool {
    paths().lock().contains_key(&id)
}

/// Refuses each later [`create_cleanup_paths`]. The signal thread calls it
/// before it kills the children, so that no child that replaces a killed
/// one gets a new path.
pub fn close_registry() {
    paths().close();
}

/// Closes the registry and deletes every registered path. The signal
/// thread calls it after the children end or the drain budget runs out. A
/// path that a live child still uses can fail to delete, and then the later
/// paths of its entry stay.
///
/// In graceful mode (`fallow watch`) Fallow keeps running after the drain,
/// so an owner must register only a path that it can lose at a signal. The
/// registry stays closed, so each later [`create_cleanup_paths`] fails.
pub fn delete_registered() {
    delete_in(paths());
}

fn delete_in(registry: &PathRegistry) {
    registry.close();
    let entries = drain(&mut registry.lock());
    delete_entries(entries);
}

/// Like [`delete_registered`], but it returns at once when another thread
/// holds the lock. A panic hook calls it: a blocking lock would deadlock when
/// the panicking thread itself holds the lock.
pub fn try_delete_registered() {
    let registry = paths();
    registry.close();
    let entries = match registry.entries.try_lock() {
        Ok(mut guard) => drain(&mut guard),
        Err(TryLockError::Poisoned(error)) => drain(&mut error.into_inner()),
        Err(TryLockError::WouldBlock) => return,
    };
    delete_entries(entries);
}

fn drain(entries: &mut Entries) -> Vec<Vec<PathBuf>> {
    entries.drain().map(|(_, entry)| entry).collect()
}

fn delete_entries(entries: Vec<Vec<PathBuf>>) {
    for entry in entries {
        let _ = entry.iter().all(|path| delete(path));
    }
}

/// How often the cleanup tries to delete one path. The owner can delete the
/// same tree at the same time, and a child that the drain budget did not end
/// can still write a file.
const DELETE_ATTEMPTS: u32 = 10;
/// The pause between two delete attempts.
const DELETE_PAUSE: Duration = Duration::from_millis(50);

/// Deletes `path`. True when it is gone.
fn delete(path: &Path) -> bool {
    for attempt in 0..DELETE_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(DELETE_PAUSE);
        }
        let _ = match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
            Ok(_) => std::fs::remove_file(path),
            Err(error) => Err(error),
        };
        // A parallel delete makes `remove_dir_all` fail with `NotFound` on an
        // inner entry while the path itself stays, so only a missing path
        // counts as gone.
        if std::fs::symlink_metadata(path)
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn make_dir(dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir.join("nested"))?;
        std::fs::write(dir.join("nested/file"), "data")
    }

    #[test]
    fn registered_paths_are_deleted_and_deregistered_paths_stay() {
        let registry = PathRegistry::default();
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("dir");
        let marker = parent.path().join("dir.lock");
        create_in(&registry, vec![dir.clone(), marker.clone()], || {
            make_dir(&dir)?;
            std::fs::write(&marker, "")
        })
        .unwrap();
        let kept = parent.path().join("kept");
        let ((), id) =
            create_in(&registry, vec![kept.clone()], || std::fs::create_dir(&kept)).unwrap();
        registry.lock().remove(&id);
        delete_in(&registry);
        assert!(!dir.exists());
        assert!(!marker.exists());
        assert!(kept.is_dir());
    }

    /// The owner and the signal cleanup can delete one tree at the same time.
    #[test]
    fn a_parallel_delete_of_the_same_tree_leaves_nothing() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("tree");
        for outer in 0..20 {
            let nested = dir.join(format!("d{outer}"));
            std::fs::create_dir_all(&nested).unwrap();
            for inner in 0..50 {
                std::fs::write(nested.join(format!("f{inner}")), "data").unwrap();
            }
        }
        let other = dir.clone();
        let racer = std::thread::spawn(move || delete(&other));
        let gone = delete(&dir);
        assert!(racer.join().unwrap());
        assert!(gone);
        assert!(!dir.exists());
    }

    /// The mutation scheduler starts a new companion when the signal
    /// cleanup kills the old one. Its temp directory must not appear after
    /// the cleanup, because the exit follows with no further cleanup.
    #[test]
    fn a_path_after_the_cleanup_is_not_created() {
        let registry = PathRegistry::default();
        let parent = tempfile::tempdir().unwrap();
        registry.close();
        let late = parent.path().join("late");
        let error = create_in(&registry, vec![late.clone()], || make_dir(&late)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(!late.exists());
    }

    /// A cleanup that starts while a path is created waits for the
    /// creation, and then deletes the new path.
    #[test]
    fn a_cleanup_during_a_creation_deletes_the_new_path() {
        let registry = PathRegistry::default();
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("dir");
        let (inside, started) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                create_in(&registry, vec![dir.clone()], || {
                    inside.send(()).unwrap();
                    std::thread::sleep(Duration::from_millis(100));
                    make_dir(&dir)
                })
                .unwrap();
            });
            started.recv().unwrap();
            delete_in(&registry);
        });
        assert!(!dir.exists());
    }

    #[test]
    fn a_failed_creation_registers_nothing() {
        let registry = PathRegistry::default();
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("dir");
        let failed = create_in(&registry, vec![dir], || -> io::Result<()> {
            Err(io::Error::other("no space"))
        });
        assert!(failed.is_err());
        assert!(registry.lock().is_empty());
    }
}
