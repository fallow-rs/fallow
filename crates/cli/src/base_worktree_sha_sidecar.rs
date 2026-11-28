//! The `.sha` sidecar of a reusable audit snapshot: the identity of the
//! snapshot that readiness compares before a reuse.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{REUSABLE_SHA_SUFFIX, read_trusted_sidecar, sidecar_path};

/// Sidecar path recording the base SHA a reusable cache entry was
/// materialized at. Lives next to the cache directory like the `.last-used` /
/// `.lock` sidecars, so readiness needs no `git` subprocess in the snapshot.
pub fn reusable_worktree_sha_path(reusable_path: &Path) -> PathBuf {
    sidecar_path(reusable_path, REUSABLE_SHA_SUFFIX)
}

/// The identity of a reusable snapshot: the base SHA on the first line and
/// the materialization scope key on the second. The scope key covers the
/// sparse cone of the host checkout, so a cone change at the same SHA
/// rebuilds the snapshot instead of reusing an old working set.
pub(super) fn reusable_snapshot_identity(repo_root: &Path, base_sha: &str) -> String {
    format!(
        "{base_sha}\nscope {}",
        fallow_engine::repo_refs::materialization_scope_key(repo_root)
    )
}

/// The identity of a snapshot that an older fallow version built, whose
/// materialization scope is unknown. It never equals a
/// [`reusable_snapshot_identity`], so the next run rebuilds the snapshot.
pub(super) fn legacy_snapshot_identity(base_sha: &str) -> String {
    format!("{base_sha}\nscope legacy")
}

/// Record the identity (see [`reusable_snapshot_identity`]) a reusable cache
/// holds. Failure is non-fatal: this run proceeds and the next run rebuilds
/// (a missing `.sha` reads as not-ready).
pub(super) fn write_reusable_sha(reusable_path: &Path, identity: &str) -> std::io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let sha_path = reusable_worktree_sha_path(reusable_path);
    let sequence = SEQ.fetch_add(1, Ordering::Relaxed);
    let temp_path = sidecar_path(
        reusable_path,
        &format!(
            "{REUSABLE_SHA_SUFFIX}.tmp-{}-{sequence}",
            std::process::id()
        ),
    );
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path)?;
        file.write_all(format!("{identity}\n").as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp_path, &sha_path)
    })();
    if let Err(err) = &result {
        let _ = std::fs::remove_file(&temp_path);
        tracing::debug!(
            path = %sha_path.display(),
            error = %err,
            "failed to write reusable audit worktree .sha sidecar; next run will rebuild",
        );
    }
    result
}

pub(super) fn read_reusable_sha(path: &Path) -> Option<String> {
    // A SHA-256 hex SHA (64), the scope line (`scope ` plus 16 hex) and two
    // newlines take 88 bytes.
    const MAX_SHA_SIDECAR_BYTES: u64 = 129;

    read_trusted_sidecar(&reusable_worktree_sha_path(path), MAX_SHA_SIDECAR_BYTES)
        .map(|contents| contents.trim().to_owned())
}
