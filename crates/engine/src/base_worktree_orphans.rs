//! Cleanup of base snapshots that a stopped process left behind.
//!
//! A process that stops between snapshot creation and cleanup leaves a
//! snapshot directory, and during the short creation window also a Git
//! registration. The CLI audit and the programmatic base snapshot both run
//! this cleanup before they create a new snapshot (issue #3885).

use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;

use crate::git_env::git_command as git_in;

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
/// `repo_root` and their directories in the temp dir.
pub fn sweep_orphan_audit_worktrees(repo_root: &Path) {
    sweep_orphan_audit_worktrees_in(repo_root, &std::env::temp_dir());
}

/// [`sweep_orphan_audit_worktrees`] with an explicit temp dir.
///
/// `temp_root` is the directory scanned for unregistered dead-pid worktree
/// directories. Production always passes `std::env::temp_dir()`; tests pass a
/// private root because a fabricated dead-pid fixture in the SHARED temp dir
/// is legitimate prey for any concurrent sweep (a parallel test or a spawned
/// fallow binary), which races the fixture's own assertions.
pub fn sweep_orphan_audit_worktrees_in(repo_root: &Path, temp_root: &Path) {
    // Legacy pass: deregister dead-pid non-reusable worktrees left REGISTERED
    // by pre-#1815 fallow (or by a crash in the transient registration window).
    // Git removes only the selected owned entry, including a missing checkout.
    deregister_legacy_orphan_worktrees(repo_root);

    // Primary pass: remove unregistered dead-pid worktree DIRECTORIES via a
    // temp-dir prefix scan. A dead PID means the owning process is gone
    // regardless of repo, so this scan is global (not repo-scoped).
    for path in scan_non_reusable_orphan_paths(temp_root) {
        let _ = std::fs::remove_dir_all(&path);
    }
}

/// Deregister dead-pid non-reusable worktrees left REGISTERED by pre-#1815
/// fallow or by a crash mid-registration.
fn deregister_legacy_orphan_worktrees(repo_root: &Path) {
    let Some(worktrees) = list_audit_worktrees(repo_root) else {
        return;
    };
    for path in worktrees {
        if !is_fallow_audit_worktree_path(&path)
            || is_reusable_audit_worktree_path(&path)
            || audit_worktree_process_is_alive(&path)
        {
            continue;
        }
        remove_audit_worktree(repo_root, &path);
        let _ = std::fs::remove_dir_all(&path);
    }
}

/// Enumerate unregistered non-reusable worktree DIRECTORY paths owned by a
/// dead PID. Reusable caches yield no parseable PID and are skipped.
fn scan_non_reusable_orphan_paths(temp: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(temp) else {
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
        if process_is_alive(pid) || !entry.path().is_dir() {
            continue;
        }
        paths.push(temp.join(name));
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
#[cfg(unix)]
#[must_use]
#[expect(
    clippy::disallowed_methods,
    reason = "liveness probe of a snapshot owner, not a repository command"
)]
pub fn process_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|output| output.status.success())
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
