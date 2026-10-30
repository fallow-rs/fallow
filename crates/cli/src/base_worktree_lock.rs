//! The per-entry advisory lock of the reusable audit caches, and the unlink
//! of its `.lock` file when the cache cleanup reclaims an entry.

use std::path::{Path, PathBuf};

use super::{SweepDisposition, open_or_create_owned_sidecar, reusable_worktree_lock_path};

/// Kernel-level advisory lock around the reusable-cache `reuse_or_create`
/// critical section, backed by `std::fs::File::try_lock` (stable since Rust
/// 1.89), which wraps `flock(2)` on Unix and `LockFileEx` on Windows.
/// Concurrent acquirers either fall through (`None`) or observe a
/// freshly-prepared cache after the holder releases.
///
/// On Unix a sweep unlinks the `.lock` file of a reclaimed entry while it
/// holds the lock ([`Self::remove_lock_file`]). A process that opened the old
/// inode before the unlink can still win the flock on it later, so every
/// acquirer checks after the lock that its fd is still the inode at the
/// path, and opens the path again when it is not. Without that check, two
/// processes could hold a flock on two different inodes for one path.
pub struct ReusableWorktreeLock {
    pub(super) file: std::fs::File,
    path: PathBuf,
}

/// Open attempts of [`ReusableWorktreeLock::try_acquire`]. A retry is needed
/// only when a sweep unlinked the lock file between the open and the flock.
const LOCK_OPEN_ATTEMPTS: usize = 3;

impl ReusableWorktreeLock {
    /// `context` names the caller's contention fallback in the debug
    /// diagnostic ("falls back to a non-reusable worktree" is only true for
    /// `reuse_or_create`; a GC sweep or prune skips the entry instead).
    pub fn try_acquire(reusable_path: &Path, context: &'static str) -> Option<Self> {
        let lock_path = reusable_worktree_lock_path(reusable_path);
        for _ in 0..LOCK_OPEN_ATTEMPTS {
            let file = open_or_create_owned_sidecar(&lock_path).ok()?;
            match file.try_lock() {
                Ok(()) if lock_file_is_current(&file, &lock_path) => {
                    return Some(Self {
                        file,
                        path: lock_path,
                    });
                }
                Ok(()) => {
                    let _ = file.unlock();
                }
                Err(err) => {
                    log_lock_failure(&lock_path, &err, context);
                    return None;
                }
            }
        }
        tracing::debug!(
            path = %lock_path.display(),
            context,
            "reusable audit worktree lock file kept changing during acquisition",
        );
        None
    }

    /// Unlink the held lock file, so a reclaimed entry leaves nothing behind.
    /// Returns `Ok(false)` when the path no longer names the held inode. On
    /// Windows it always returns `Ok(false)` and the `.lock` file stays,
    /// because the inode check of [`Self::try_acquire`] is Unix only.
    pub fn remove_lock_file(&self) -> std::io::Result<bool> {
        if !cfg!(unix) || !lock_file_is_current(&self.file, &self.path) {
            return Ok(false);
        }
        std::fs::remove_file(&self.path).map(|()| true)
    }
}

#[cfg(unix)]
pub(super) fn lock_file_is_current(file: &std::fs::File, lock_path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    let (Ok(held), Ok(at_path)) = (file.metadata(), std::fs::symlink_metadata(lock_path)) else {
        return false;
    };
    held.dev() == at_path.dev() && held.ino() == at_path.ino()
}

#[cfg(not(unix))]
pub(super) const fn lock_file_is_current(_file: &std::fs::File, _lock_path: &Path) -> bool {
    true
}

fn log_lock_failure(lock_path: &Path, err: &std::fs::TryLockError, context: &'static str) {
    match err {
        std::fs::TryLockError::WouldBlock => {
            tracing::debug!(
                path = %lock_path.display(),
                context,
                "reusable audit worktree lock contended",
            );
        }
        std::fs::TryLockError::Error(err) => {
            tracing::debug!(
                path = %lock_path.display(),
                error = %err,
                context,
                "could not acquire reusable audit worktree lock",
            );
        }
    }
}

impl Drop for ReusableWorktreeLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Unlink the held lock file of an entry whose other parts are gone. A
/// failure is not an error of the sweep: the lock file stays and the next
/// sweep reports it as lock-only.
pub(super) fn remove_held_lock_file(lock: &ReusableWorktreeLock) -> bool {
    match lock.remove_lock_file() {
        Ok(removed) => removed,
        Err(err) => {
            tracing::debug!(
                error = %err,
                "failed to remove the lock sidecar of a reclaimed audit cache entry",
            );
            false
        }
    }
}

pub(super) fn reclaim_lock_only(lock: &ReusableWorktreeLock) -> SweepDisposition {
    if remove_held_lock_file(lock) {
        SweepDisposition::ReclaimedLockOnly
    } else {
        SweepDisposition::KeptLockOnly
    }
}
