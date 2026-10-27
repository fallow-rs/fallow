//! Cleanup of base snapshots that a stopped process left behind.
//!
//! A process that stops between snapshot creation and cleanup leaves a
//! snapshot directory, and during the short creation window also a Git
//! registration. The CLI audit and the programmatic base snapshot both run
//! this cleanup before they create a new snapshot (issue #3885).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::git_env::git_command as git_in;

/// A snapshot entry younger than this is never removed. Its owner can still
/// be creating it, also when the process id was reused.
pub const ORPHAN_MIN_AGE: Duration = Duration::from_secs(60);

/// Remove a registered Fallow base snapshot with `git worktree remove --force`.
pub fn remove_audit_worktree(repo_root: &Path, path: &Path) {
    let mut command = git_in(repo_root);
    command.args([
        "worktree",
        "remove",
        "--force",
        path.to_string_lossy().as_ref(),
    ]);
    match fallow_process::output(&mut command) {
        Ok(output) => {
            if !output.status.success() && path.exists() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(
                    path = %path.display(),
                    stderr = %stderr.trim(),
                    "git worktree remove failed; the directory remains and may leak",
                );
            }
        }
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "git worktree remove subprocess failed to spawn",
            );
        }
    }
}

/// Remove the snapshots of dead processes: their Git registrations in
/// `repo_root` and their directories in the temp dir. Entries younger than
/// [`ORPHAN_MIN_AGE`] stay.
pub fn sweep_orphan_audit_worktrees(repo_root: &Path) {
    sweep_orphan_audit_worktrees_in(repo_root, &std::env::temp_dir(), ORPHAN_MIN_AGE);
}

/// [`sweep_orphan_audit_worktrees`] with an explicit temp dir and age floor.
///
/// `temp_root` is the directory scanned for unregistered dead-pid worktree
/// directories. Production always passes `std::env::temp_dir()`; tests pass a
/// private root because a fabricated dead-pid fixture in the SHARED temp dir
/// is legitimate prey for any concurrent sweep (a parallel test or a spawned
/// fallow binary), which races the fixture's own assertions.
pub fn sweep_orphan_audit_worktrees_in(repo_root: &Path, temp_root: &Path, min_age: Duration) {
    // Legacy pass: deregister dead-pid non-reusable worktrees left REGISTERED
    // by pre-#1815 fallow (or by a crash in the transient registration window).
    // Git removes only the selected owned entry, including a missing checkout.
    // Git runs only when the admin directory names a dead owner.
    if has_dead_owner_registration(repo_root) {
        deregister_legacy_orphan_worktrees(repo_root, min_age);
    }

    // Primary pass: remove unregistered dead-pid worktree DIRECTORIES via a
    // temp-dir prefix scan. A dead PID means the owning process is gone
    // regardless of repo, so this scan is global (not repo-scoped).
    for path in scan_non_reusable_orphan_paths(temp_root, min_age) {
        let _ = fs::remove_dir_all(&path);
    }
}

/// Deregister dead-pid non-reusable worktrees left REGISTERED by pre-#1815
/// fallow or by a crash mid-registration.
fn deregister_legacy_orphan_worktrees(repo_root: &Path, min_age: Duration) {
    let Some(worktrees) = list_audit_worktrees(repo_root) else {
        return;
    };
    for path in worktrees {
        if !is_fallow_audit_worktree_path(&path)
            || is_reusable_audit_worktree_path(&path)
            || audit_worktree_process_is_alive(&path)
            || (path.exists() && !is_older_than(&path, min_age))
        {
            continue;
        }
        remove_audit_worktree(repo_root, &path);
        let _ = fs::remove_dir_all(&path);
    }
}

/// Return whether the Git admin directory of `repo_root` names a
/// non-reusable snapshot whose owner is dead. Only the file system is read,
/// so an audit with nothing to remove starts no `git` process. An unknown
/// repository layout returns `true`, and Git then decides.
fn has_dead_owner_registration(repo_root: &Path) -> bool {
    let Some(admin) = worktrees_admin_dir(repo_root) else {
        return true;
    };
    let Ok(entries) = fs::read_dir(admin) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .and_then(audit_worktree_pid)
            .is_some_and(|pid| !process_is_alive(pid))
    })
}

/// The `worktrees` admin directory of the repository that contains `dir`.
fn worktrees_admin_dir(dir: &Path) -> Option<PathBuf> {
    let dir = std::path::absolute(dir).ok()?;
    for candidate in dir.ancestors() {
        let dotgit = candidate.join(".git");
        let Ok(metadata) = fs::symlink_metadata(&dotgit) else {
            continue;
        };
        if metadata.is_dir() {
            return Some(dotgit.join("worktrees"));
        }
        let contents = fs::read_to_string(&dotgit).ok()?;
        let gitdir = candidate.join(crate::repo_refs::parse_worktree_gitdir(&contents)?);
        let common = fs::read_to_string(gitdir.join("commondir"))
            .map_or_else(|_| gitdir.clone(), |common| gitdir.join(common.trim()));
        return Some(common.join("worktrees"));
    }
    None
}

/// Return whether `path` was last modified at least `min_age` ago. An
/// unreadable time counts as recent, so the entry stays.
fn is_older_than(path: &Path, min_age: Duration) -> bool {
    fs::symlink_metadata(path)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::ZERO)
                >= min_age
        })
}

/// Enumerate unregistered non-reusable worktree DIRECTORY paths owned by a
/// dead PID and older than `min_age`. Reusable caches yield no parseable PID
/// and are skipped.
fn scan_non_reusable_orphan_paths(temp: &Path, min_age: Duration) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(temp) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(pid) = audit_worktree_pid(name) else {
            continue;
        };
        let path = temp.join(name);
        if !path.is_dir() || process_is_alive(pid) || !is_older_than(&path, min_age) {
            continue;
        }
        paths.push(path);
    }
    paths
}

/// List the registered Fallow base snapshots of `repo_root`.
pub fn list_audit_worktrees(repo_root: &Path) -> Option<Vec<PathBuf>> {
    let output = git_in(repo_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_worktree_list(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// Return the Fallow base snapshots in `git worktree list --porcelain` output.
#[must_use]
pub fn parse_worktree_list(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .filter(|path| is_fallow_audit_worktree_path(path))
        .collect()
}

/// Return whether `path` is a Fallow base snapshot in the temp dir.
#[must_use]
pub fn is_fallow_audit_worktree_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with("fallow-audit-base-") && path_is_inside_temp_dir(path)
}

/// Return whether `path` is a reusable base snapshot of the CLI audit cache.
#[must_use]
pub fn is_reusable_audit_worktree_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("fallow-audit-base-cache-"))
}

fn path_is_inside_temp_dir(path: &Path) -> bool {
    let temp = std::env::temp_dir();
    let simple_path = dunce::simplified(path);
    let simple_temp = dunce::simplified(&temp);
    if simple_path.starts_with(simple_temp) {
        return true;
    }
    let Ok(canonical_temp) = std::fs::canonicalize(&temp) else {
        return false;
    };
    let simple_canonical_temp = dunce::simplified(&canonical_temp);
    simple_path.starts_with(simple_canonical_temp)
        || std::fs::canonicalize(path).is_ok_and(|canonical_path| {
            dunce::simplified(&canonical_path).starts_with(simple_canonical_temp)
        })
}

fn audit_worktree_process_is_alive(path: &Path) -> bool {
    let Some(pid) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(audit_worktree_pid)
    else {
        return false;
    };
    process_is_alive(pid)
}

/// Return the owner process id in a non-reusable snapshot directory name.
#[must_use]
pub fn audit_worktree_pid(name: &str) -> Option<u32> {
    name.strip_prefix("fallow-audit-base-")?
        .split('-')
        .next()?
        .parse()
        .ok()
}

/// Return whether the snapshot owner `pid` can still be running.
///
/// The check is conservative: a process that cannot be inspected counts as
/// alive, so the cleanup never removes a snapshot that a live run of another
/// user can hold. `fallow_process::pid_is_alive` stays separate on purpose.
/// Its callers wait for a child to exit, so it reports an uninspectable
/// process as gone.
///
/// On Unix, `kill(pid, 0)` sends no signal. Success and `EPERM` (a process of
/// another user) mean alive. Only `ESRCH` means dead.
#[cfg(unix)]
#[must_use]
#[allow(
    unsafe_code,
    reason = "kill(2) with signal 0 is the liveness probe; it has no memory preconditions"
)]
pub fn process_is_alive(pid: u32) -> bool {
    // Zero and values above `pid_t::MAX` address process groups, not one process.
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return true;
    };
    if pid <= 0 {
        return true;
    }
    // SAFETY: `kill` takes plain integers. Signal 0 only checks the target.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Return whether the snapshot owner `pid` can still be running. An
/// uninspectable process counts as alive (see the Unix variant).
#[cfg(windows)]
#[must_use]
pub fn process_is_alive(pid: u32) -> bool {
    windows_process::is_alive(pid)
}

/// Without a liveness probe, every owner counts as alive.
#[cfg(not(any(unix, windows)))]
#[must_use]
pub fn process_is_alive(_pid: u32) -> bool {
    true
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Win32 process-query API (OpenProcess / WaitForSingleObject / CloseHandle / GetLastError) requires unsafe FFI"
)]
mod windows_process {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, GetLastError, HANDLE,
        WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
    };

    /// RAII wrapper that calls `CloseHandle` on drop, mirroring `std::mem::drop`
    /// semantics for kernel handles. Used so every exit path through
    /// `is_alive` releases the handle without manual cleanup.
    struct ProcessHandle(HANDLE);

    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a non-null handle obtained from a successful
            // `OpenProcess` call. We have unique ownership (the value is only
            // ever created inside `is_alive`), so this is the sole consumer.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    /// Cross-platform PID liveness check for Windows.
    ///
    /// Mirrors `kill -0 $pid` semantics: returns `true` when the process is
    /// running OR when we cannot prove it dead (e.g., `ERROR_ACCESS_DENIED` on
    /// processes owned by another session). Returns `false` only when the PID
    /// definitively does not exist (`ERROR_INVALID_PARAMETER`) or the wait
    /// reports the process has exited.
    pub fn is_alive(pid: u32) -> bool {
        // SAFETY: `OpenProcess` accepts any `u32` PID; it either returns a
        // non-null handle we own, or null on failure with `GetLastError`
        // describing why. No memory is borrowed across the FFI boundary.
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if raw.is_null() {
            // SAFETY: `GetLastError` reads thread-local storage set by the
            // failing `OpenProcess` call. It has no preconditions.
            let err = unsafe { GetLastError() };
            #[expect(
                clippy::match_same_arms,
                reason = "named arm documents the cross-session case"
            )]
            return match err {
                ERROR_INVALID_PARAMETER => false,
                ERROR_ACCESS_DENIED => true,
                _ => true,
            };
        }
        let handle = ProcessHandle(raw);
        // SAFETY: `handle.0` is non-null (checked above) and owned by the
        // `ProcessHandle` RAII wrapper.
        let wait_result = unsafe { WaitForSingleObject(handle.0, 0) };
        wait_result != WAIT_OBJECT_0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A process id that `kill(pid, 0)` reports as `ESRCH`.
    const DEAD_PID: u32 = 99_999_998;

    #[cfg(unix)]
    fn snapshot_dir(root: &Path, pid: u32, age: Duration) -> PathBuf {
        let path = root.join(format!("fallow-audit-base-{pid}-1-0"));
        fs::create_dir_all(&path).expect("create snapshot dir");
        fs::write(path.join("index.ts"), "").expect("write snapshot file");
        let modified = SystemTime::now() - age;
        fs::File::open(&path)
            .and_then(|dir| dir.set_modified(modified))
            .expect("set snapshot time");
        path
    }

    #[cfg(unix)]
    #[test]
    fn liveness_counts_only_esrch_as_dead() {
        assert!(process_is_alive(std::process::id()));
        assert!(!process_is_alive(DEAD_PID));
        // Process 1 belongs to root, so `kill` fails with EPERM for a user.
        assert!(process_is_alive(1), "an uninspectable process is alive");
        assert!(process_is_alive(0), "process group ids are never probed");
        assert!(process_is_alive(u32::MAX));
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_removes_only_old_snapshots_of_dead_owners() {
        let temp = tempfile::tempdir().expect("temp dir");
        let old = ORPHAN_MIN_AGE + Duration::from_secs(60);
        let live = snapshot_dir(temp.path(), std::process::id(), old);
        let dead = temp.path().join("dead");
        fs::create_dir_all(&dead).expect("create dead parent");
        let dead_old = snapshot_dir(&dead, DEAD_PID, old);
        let young = temp.path().join("young");
        fs::create_dir_all(&young).expect("create young parent");
        let dead_young = snapshot_dir(&young, DEAD_PID, Duration::ZERO);
        let not_a_repo = temp.path().join("not-a-repo");

        for root in [temp.path(), dead.as_path(), young.as_path()] {
            sweep_orphan_audit_worktrees_in(&not_a_repo, root, ORPHAN_MIN_AGE);
        }

        assert!(live.is_dir(), "the snapshot of a live owner stays");
        assert!(!dead_old.exists(), "an old snapshot of a dead owner goes");
        assert!(
            dead_young.is_dir(),
            "a snapshot younger than the floor stays"
        );
    }

    #[test]
    fn registration_check_reads_the_admin_dir_without_git() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        let admin = repo.join(".git").join("worktrees");
        fs::create_dir_all(&admin).expect("create admin dir");
        assert!(!has_dead_owner_registration(&repo));
        fs::create_dir_all(admin.join("fallow-audit-base-cache-0-root-0"))
            .expect("create reusable admin entry");
        fs::create_dir_all(admin.join(format!("fallow-audit-base-{}-1-0", std::process::id())))
            .expect("create live admin entry");
        assert!(!has_dead_owner_registration(&repo.join("nested")));
        fs::create_dir_all(admin.join(format!("fallow-audit-base-{DEAD_PID}-1-0")))
            .expect("create dead admin entry");
        assert!(has_dead_owner_registration(&repo.join("nested")));
    }
}
