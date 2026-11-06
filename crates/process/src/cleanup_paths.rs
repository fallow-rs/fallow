//! Process-wide registry of paths that the signal cleanup deletes.
//!
//! A signal ends Fallow with `exit`, so no destructor runs. An owner that
//! deletes a temp directory in `Drop` registers the directory here, and the
//! signal cleanup deletes it after it ends the registered children. The
//! children must end first: a live child can still write into the directory.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, TryLockError};
use std::time::Duration;

use rustc_hash::FxHashMap;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static PATHS: OnceLock<Mutex<FxHashMap<u64, Vec<PathBuf>>>> = OnceLock::new();

fn paths() -> &'static Mutex<FxHashMap<u64, Vec<PathBuf>>> {
    PATHS.get_or_init(|| Mutex::new(FxHashMap::default()))
}

/// Registers `paths` for deletion on a signal, in order. A directory goes
/// with its contents. The deletion stops at the first path that it cannot
/// delete, so a later path (for example a lock file that marks the
/// directory) stays when an earlier path stays. Returns the key for
/// [`deregister_cleanup_paths`].
pub fn register_cleanup_paths(paths_to_delete: Vec<PathBuf>) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    paths()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(id, paths_to_delete);
    id
}

/// Removes the entry for `id`. Idempotent.
pub fn deregister_cleanup_paths(id: u64) {
    paths()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(&id);
}

/// Deletes every registered path. The signal thread calls it after the
/// children end or the drain budget runs out. A path that a live child still
/// uses can fail to delete, and then the later paths of its entry stay.
///
/// In graceful mode (`fallow watch`) Fallow keeps running after the drain,
/// so an owner must register only a path that it can lose at a signal.
pub fn delete_registered() {
    let entries = drain_entries(&mut paths().lock().unwrap_or_else(|error| error.into_inner()));
    delete_entries(entries);
}

/// Like [`delete_registered`], but it returns at once when another thread
/// holds the lock. A panic hook calls it: a blocking lock would deadlock when
/// the panicking thread itself holds the lock.
pub fn try_delete_registered() {
    let entries = match paths().try_lock() {
        Ok(mut guard) => drain_entries(&mut guard),
        Err(TryLockError::Poisoned(error)) => drain_entries(&mut error.into_inner()),
        Err(TryLockError::WouldBlock) => return,
    };
    delete_entries(entries);
}

fn drain_entries(registry: &mut FxHashMap<u64, Vec<PathBuf>>) -> Vec<Vec<PathBuf>> {
    registry.drain().map(|(_, entry)| entry).collect()
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
    use super::*;

    /// One test only: the deletion drains the process-wide registry, so a
    /// parallel test could delete the paths of another test.
    #[test]
    fn registered_paths_are_deleted_and_deregistered_paths_stay() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("dir");
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("nested/file"), "data").unwrap();
        let marker = parent.path().join("dir.lock");
        std::fs::write(&marker, "").unwrap();
        let kept = parent.path().join("kept");
        std::fs::create_dir(&kept).unwrap();
        register_cleanup_paths(vec![dir.clone(), marker.clone()]);
        let id = register_cleanup_paths(vec![kept.clone()]);
        deregister_cleanup_paths(id);
        delete_registered();
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
}
