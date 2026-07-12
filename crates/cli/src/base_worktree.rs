use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use fallow_engine::changed_files::clear_ambient_git_env;
use rustc_hash::FxHashSet;
use xxhash_rust::xxh3::xxh3_64;

use crate::report::plural;

pub struct BaseWorktree {
    path: PathBuf,
    persistent: bool,
}

impl BaseWorktree {
    pub fn create(repo_root: &Path, base_ref: &str, base_sha: Option<&str>) -> Option<Self> {
        sweep_orphan_audit_worktrees(repo_root);
        if let Some(base_sha) = base_sha
            && let Some(worktree) = Self::reuse_or_create(repo_root, base_sha)
        {
            return Some(worktree);
        }
        let path = non_reusable_worktree_path()?;
        let mut guard = WorktreeCleanupGuard::new(repo_root, &path);
        if let Err(error) = fallow_engine::repo_refs::create_detached_base_worktree(
            repo_root,
            guard.path(),
            base_ref,
        ) {
            tracing::debug!(
                base_ref,
                error = %error,
                "could not materialize non-reusable audit base worktree",
            );
            return None;
        }
        // Deregister immediately so a crash before Drop never leaves a git
        // worktree admin entry behind (issue #1815). No early return runs
        // between here and the struct binding, so defusing the guard next is
        // safe: the entry is already unregistered.
        unregister_worktree(guard.path());
        guard.defuse();
        drop(guard);
        let worktree = Self {
            path,
            persistent: false,
        };
        materialize_base_dependency_context(repo_root, worktree.path());
        Some(worktree)
    }

    pub fn reuse_or_create(repo_root: &Path, base_sha: &str) -> Option<Self> {
        let path = reusable_audit_worktree_path(repo_root, base_sha);
        let _lock = ReusableWorktreeLock::try_acquire(&path)?;

        if reusable_audit_worktree_is_ready(&path, base_sha)
            || try_migrate_legacy_reusable_cache(repo_root, &path, base_sha)
        {
            let worktree = Self {
                path,
                persistent: true,
            };
            materialize_base_dependency_context(repo_root, worktree.path());
            touch_last_used(worktree.path());
            return Some(worktree);
        }

        // Only deregister when a stale entry is actually still registered: an
        // unregistered cache dir (the common case now) would otherwise emit a
        // spurious `git worktree remove failed` warning on every rebuild.
        if audit_worktree_is_registered(repo_root, &path) {
            remove_audit_worktree(repo_root, &path);
        }
        let _ = std::fs::remove_dir_all(&path);
        let mut guard = WorktreeCleanupGuard::new(repo_root, &path);
        if let Err(error) = fallow_engine::repo_refs::create_detached_base_worktree(
            repo_root,
            guard.path(),
            base_sha,
        ) {
            tracing::debug!(
                base_sha,
                error = %error,
                "could not materialize reusable audit base worktree",
            );
            return None;
        }
        // Deregister while keeping the directory, then record the base SHA in
        // the `.sha` sidecar. The sidecar is written strictly AFTER a
        // successful materialization + deregistration (under the reuse lock),
        // so a torn snapshot is never advertised as ready to the next run.
        unregister_worktree(guard.path());
        guard.defuse();
        drop(guard);
        write_reusable_sha(&path, base_sha);

        let worktree = Self {
            path,
            persistent: true,
        };
        materialize_base_dependency_context(repo_root, worktree.path());
        touch_last_used(worktree.path());
        Some(worktree)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Build a unique temp path for a non-reusable base worktree.
///
/// The pid stays the FIRST `-`-separated segment so [`audit_worktree_pid`] and
/// the orphan sweep keep working. A process-global monotonic counter is the
/// final segment: the wall-clock nanos read is NOT monotonic and repeats across
/// threads, so two `audit` runs in one process (e.g. parallel unit tests, or a
/// future in-process batch) could otherwise mint the same path and race on
/// `git worktree add`, where the loser fails and the audit aborts with a generic
/// error. The counter makes every path distinct regardless of clock resolution.
fn non_reusable_worktree_path() -> Option<PathBuf> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(std::env::temp_dir().join(format!(
        "fallow-audit-base-{}-{nanos}-{seq}",
        std::process::id()
    )))
}

/// RAII cleanup guard for a freshly-created git worktree directory.
///
/// Armed before the `git worktree add` subprocess runs. If the holder returns
/// early (`?`) between subprocess success and the `BaseWorktree` struct binding,
/// `Drop` rolls back BOTH git's `.git/worktrees/<name>` registration AND the
/// on-disk directory. The owner calls `defuse()` once `BaseWorktree` is bound
/// and takes over cleanup via its own `Drop`.
///
/// With `panic = "abort"` on the release profile, this does not provide
/// panic-recovery cleanup (no unwind runs), but it is still load-bearing for
/// every early-return path between subprocess success and struct construction.
pub struct WorktreeCleanupGuard<'a> {
    repo_root: PathBuf,
    path: &'a Path,
    armed: bool,
}

impl<'a> WorktreeCleanupGuard<'a> {
    pub fn new(repo_root: &Path, path: &'a Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            path,
            armed: true,
        }
    }

    pub fn path(&self) -> &Path {
        self.path
    }

    /// Disarm in place. Idempotent; calling twice is harmless. Drop becomes a
    /// no-op after this returns.
    pub fn defuse(&mut self) {
        self.armed = false;
    }
}

impl Drop for WorktreeCleanupGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            remove_audit_worktree(&self.repo_root, self.path);
            let _ = std::fs::remove_dir_all(self.path);
        }
    }
}

/// Kernel-level advisory lock around the reusable-cache `reuse_or_create`
/// critical section, backed by `std::fs::File::try_lock` (stable since Rust
/// 1.89), which wraps `flock(2)` on Unix and `LockFileEx` on Windows.
/// Concurrent acquirers either fall through (`None`) or observe a
/// freshly-prepared cache after the holder releases.
pub struct ReusableWorktreeLock {
    _file: std::fs::File,
}

impl ReusableWorktreeLock {
    pub fn try_acquire(reusable_path: &Path) -> Option<Self> {
        let lock_path = reusable_worktree_lock_path(reusable_path);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .ok()?;
        match file.try_lock() {
            Ok(()) => Some(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                tracing::debug!(
                    path = %lock_path.display(),
                    "reusable audit worktree lock contended; falling back to non-reusable worktree",
                );
                None
            }
            Err(std::fs::TryLockError::Error(err)) => {
                tracing::debug!(
                    path = %lock_path.display(),
                    error = %err,
                    "could not acquire reusable audit worktree lock; falling back to non-reusable worktree",
                );
                None
            }
        }
    }
}

pub fn reusable_worktree_lock_path(reusable_path: &Path) -> PathBuf {
    sidecar_path(reusable_path, REUSABLE_LOCK_SUFFIX)
}

/// Build a sidecar path `<cache dir name><suffix>` next to (NOT inside) the
/// reusable cache directory, so the sidecar survives `git worktree`
/// operations and directory materialization on the cache dir itself.
fn sidecar_path(reusable_path: &Path, suffix: &str) -> PathBuf {
    let mut name = reusable_path
        .file_name()
        .map(std::ffi::OsString::from)
        .unwrap_or_default();
    name.push(suffix);
    reusable_path
        .parent()
        .map_or_else(|| PathBuf::from(&name), |parent| parent.join(&name))
}

/// Sidecar path recording the base SHA a reusable cache entry was
/// materialized at. Lives next to the cache directory like the `.last-used` /
/// `.lock` sidecars, so readiness can be verified without a `git` subprocess.
pub fn reusable_worktree_sha_path(reusable_path: &Path) -> PathBuf {
    sidecar_path(reusable_path, REUSABLE_SHA_SUFFIX)
}

/// Record the base SHA a reusable cache holds. Failure is non-fatal: this run
/// proceeds and the next run rebuilds (a missing `.sha` reads as not-ready).
fn write_reusable_sha(reusable_path: &Path, base_sha: &str) {
    let sha_path = reusable_worktree_sha_path(reusable_path);
    if let Err(err) = std::fs::write(&sha_path, format!("{base_sha}\n")) {
        tracing::debug!(
            path = %sha_path.display(),
            error = %err,
            "failed to write reusable audit worktree .sha sidecar; next run will rebuild",
        );
    }
}

/// Default GC threshold for persistent reusable base-snapshot caches.
const DEFAULT_AUDIT_CACHE_MAX_AGE_DAYS: u32 = 30;

/// Env var that overrides `audit.cacheMaxAgeDays` from the config.
const AUDIT_CACHE_MAX_AGE_ENV: &str = "FALLOW_AUDIT_CACHE_MAX_AGE_DAYS";

/// Sidecar filename suffix used to track last-use of a reusable worktree.
const REUSABLE_LAST_USED_SUFFIX: &str = ".last-used";

/// Sidecar filename suffix recording the base SHA a reusable cache holds.
const REUSABLE_SHA_SUFFIX: &str = ".sha";

/// Sidecar filename suffix of the reuse-lock file.
const REUSABLE_LOCK_SUFFIX: &str = ".lock";

/// Invalid gitdir pointer written into `<cache>/.git` after deregistering a
/// transient audit worktree (issue #1815).
///
/// The `.git` file is REPLACED, never deleted: both discovery walkers use the
/// `ignore` crate with `require_git` on, whose gitignore handling is gated on
/// `<root>/.git` existing. Deleting the gitfile would silently stop the base
/// pass from honoring `.gitignore`, inflating base findings and skewing
/// audit's introduced-vs-inherited split. The stub keeps gitignore parity
/// while pointing at a nonexistent gitdir, so any stray `git` command inside
/// the snapshot fails loudly instead of operating on the host repo.
const UNREGISTERED_GITDIR_STUB: &str = "gitdir: fallow-audit-unregistered\n";

/// Sidecar path for the "last used" timestamp of a reusable cache entry.
///
/// Lives next to the cache directory (NOT inside it) so the sidecar is
/// untouched by `git worktree add/remove` on the cache directory itself.
pub fn reusable_worktree_last_used_path(reusable_path: &Path) -> PathBuf {
    sidecar_path(reusable_path, REUSABLE_LAST_USED_SUFFIX)
}

/// Stamp the sidecar `.last-used` file's mtime to now.
///
/// Called on every cache-hit reuse (and from the pre-upgrade-grace branch
/// of the GC sweep) so the staleness signal stays current even when the
/// cache directory itself is not mutated. Failures are surfaced at
/// `warn!` so a persistent ENOSPC / read-only-tmp condition is visible at
/// default `RUST_LOG=warn`; the caller does not abort the audit.
pub fn touch_last_used(reusable_path: &Path) {
    let last_used = reusable_worktree_last_used_path(reusable_path);
    let result = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&last_used)
        .and_then(|file| file.set_modified(SystemTime::now()));
    if let Err(err) = result {
        tracing::warn!(
            path = %last_used.display(),
            error = %err,
            "failed to touch reusable audit worktree sidecar; staleness signal may not update",
        );
    }
}

/// Resolve the GC threshold for persistent reusable caches.
///
/// Precedence: `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS` env var > `audit.cacheMaxAgeDays`
/// config field > 30-day default. `0` from either source disables the sweep
/// entirely (returns `None`). Invalid env values (non-integer) silently fall
/// back to config / default; audits do not fail on a typo in a runner env var.
pub fn resolve_cache_max_age_with_options(
    root: &Path,
    config_path: Option<&PathBuf>,
    allow_remote_extends: bool,
) -> Option<Duration> {
    if let Ok(raw) = std::env::var(AUDIT_CACHE_MAX_AGE_ENV) {
        if let Ok(days) = raw.trim().parse::<u32>() {
            return days_to_duration(days);
        }
        tracing::debug!(
            value = %raw,
            "FALLOW_AUDIT_CACHE_MAX_AGE_DAYS is not a valid u32; falling back to config/default",
        );
    }
    if let Some(days) = load_audit_config(root, config_path, allow_remote_extends)
        .and_then(|c| c.cache_max_age_days)
    {
        return days_to_duration(days);
    }
    days_to_duration(DEFAULT_AUDIT_CACHE_MAX_AGE_DAYS)
}

pub fn days_to_duration(days: u32) -> Option<Duration> {
    if days == 0 {
        return None;
    }
    Some(Duration::from_secs(u64::from(days) * 86_400))
}

/// Load `AuditConfig` from `opts.config_path` (or auto-discover from
/// `opts.root`) for GC-threshold resolution only. Errors silently fall
/// back to `None`; the caller defaults to a 30-day window.
fn load_audit_config(
    root: &Path,
    config_path: Option<&PathBuf>,
    allow_remote_extends: bool,
) -> Option<fallow_config::AuditConfig> {
    let options = fallow_config::ConfigLoadOptions {
        allow_remote_extends,
    };
    if let Some(path) = config_path {
        return fallow_config::FallowConfig::load_with_options(path, options)
            .ok()
            .map(|config| config.audit);
    }
    fallow_config::FallowConfig::find_and_load_with_options(root, options)
        .ok()
        .flatten()
        .map(|(config, _path)| config.audit)
}

/// Reclaim persistent reusable base-snapshot worktree caches.
///
/// Two reclaim conditions, checked per entry:
/// - Prunable orphan: the cache directory no longer exists (an external
///   `$TMPDIR` reaper, a container restart, or a CI cache eviction deleted it
///   but left git's admin entry behind). Reclaimed eagerly, independent of
///   `max_age`, because the `.last-used` sidecar lives next to the deleted
///   directory and survives the reaper, so the age branch would re-touch a
///   fresh sidecar and never reclaim the dead entry. Passing `max_age = None`
///   (age-based GC disabled) still runs this reclaim.
/// - Aged-out: the sidecar `.last-used` file is older than `max_age` (only
///   when `max_age` is `Some`).
///
/// Concurrency: each candidate is gated by [`ReusableWorktreeLock`] before
/// removal, so an in-flight `fallow audit` mid-rebuild against the same
/// cache entry will not be disturbed (the sweep skips on contention). The
/// orphan branch re-checks existence under the lock so a rebuild that
/// recreated the directory between the check and the lock is preserved.
///
/// Pre-upgrade caches lacking a sidecar are NOT removed: instead the sweep
/// seeds a fresh sidecar so the next invocation can age them from real
/// last-use. Without this grace, the dir's own mtime (= creation date on
/// POSIX) would wipe every legitimately-warm pre-upgrade cache on the
/// first run after upgrade.
///
/// The `.lock` sidecar file is intentionally NOT deleted on removal: a
/// racing acquirer of an unlinked-but-still-flocked inode plus a sibling
/// `open(O_CREAT)` at the same path would produce two processes each
/// holding a kernel flock on different inodes. Lock files are tens of
/// bytes; leaking them is harmless.
pub fn sweep_old_reusable_caches(repo_root: &Path, max_age: Option<Duration>, quiet: bool) {
    // Legacy pass: deregister reusable caches left REGISTERED by pre-#1815
    // fallow (the reporter's `git worktree list` backlog). This is what makes
    // those entries vanish on the first post-upgrade audit; the `--expire=now`
    // prune is retained ONLY here to also sweep any admin entry orphaned by a
    // crash in the transient registration window.
    if deregister_legacy_reusable_caches(repo_root) {
        let mut command = Command::new("git");
        command
            .args(["worktree", "prune", "--expire=now"])
            .current_dir(repo_root);
        clear_ambient_git_env(&mut command);
        let _ = command.output();
    }

    // Primary pass: a repo-scoped temp-dir prefix scan (NOT `git worktree
    // list`, which no longer sees the deregistered caches). Age-out and
    // sidecar-orphan cleanup run on the unregistered entries.
    let prefix = reusable_cache_repo_prefix(repo_root);
    let now = SystemTime::now();
    let mut removed: u32 = 0;
    for path in scan_reusable_cache_paths(&prefix) {
        if reclaim_reusable_cache_entry(&path, max_age, now) {
            removed += 1;
        }
    }
    if removed == 0 {
        return;
    }
    tracing::info!(
        count = removed,
        "reclaimed stale audit base-snapshot caches",
    );
    if !quiet {
        let s = plural(removed as usize);
        let _ = writeln!(
            std::io::stderr(),
            "fallow: reclaimed {removed} stale base-snapshot cache{s}",
        );
    }
}

/// Deregister reusable base-snapshot caches left REGISTERED by fallow versions
/// before #1815. Seeds the `.sha` sidecar from the entry's HEAD first so a
/// still-warm legacy cache stays reusable after deregistration. Returns `true`
/// when at least one entry was deregistered.
fn deregister_legacy_reusable_caches(repo_root: &Path) -> bool {
    let Some(worktrees) = list_audit_worktrees(repo_root) else {
        return false;
    };
    let mut deregistered = false;
    for path in worktrees {
        if !is_reusable_audit_worktree_path(&path) {
            continue;
        }
        let Some(_lock) = ReusableWorktreeLock::try_acquire(&path) else {
            continue;
        };
        if !audit_worktree_is_registered(repo_root, &path) {
            continue;
        }
        seed_legacy_reusable_sha(&path);
        unregister_worktree(&path);
        deregistered = true;
    }
    deregistered
}

/// Seed the `.sha` sidecar for a still-registered legacy cache from its HEAD,
/// so after deregistration the readiness probe recognizes it as warm. Seeds
/// only when the snapshot was raw-materialized and no `.sha` exists yet.
fn seed_legacy_reusable_sha(path: &Path) {
    if reusable_worktree_sha_path(path).exists()
        || !fallow_engine::repo_refs::detached_base_worktree_is_raw_materialized(path)
    {
        return;
    }
    if let Some(head) = git_rev_parse(path, "HEAD") {
        write_reusable_sha(path, &head);
    }
}

/// Enumerate reusable cache DIRECTORY paths for `prefix` by scanning the temp
/// dir. Sidecar entries (`.last-used` / `.sha` / `.lock`) are folded back to
/// their owning cache path and deduplicated, so a dir removed out from under
/// its sidecars is still visited for sidecar-orphan cleanup.
fn scan_reusable_cache_paths(prefix: &str) -> Vec<PathBuf> {
    let temp = std::env::temp_dir();
    let Ok(entries) = std::fs::read_dir(&temp) else {
        return Vec::new();
    };
    let mut seen: FxHashSet<PathBuf> = FxHashSet::default();
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(prefix) {
            continue;
        }
        let path = temp.join(strip_cache_sidecar_suffix(name));
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    paths
}

/// Strip a known reusable-cache sidecar suffix so a sidecar entry maps to its
/// owning cache directory name. Cache dir names end in a hex SHA prefix and
/// never contain these suffixes, so the mapping is unambiguous.
fn strip_cache_sidecar_suffix(name: &str) -> &str {
    for suffix in [
        REUSABLE_LAST_USED_SUFFIX,
        REUSABLE_SHA_SUFFIX,
        REUSABLE_LOCK_SUFFIX,
    ] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            return stripped;
        }
    }
    name
}

/// Reclaim a single reusable-cache entry. Returns `true` when the entry was
/// removed (either as a sidecar orphan or as aged-out past `max_age`).
fn reclaim_reusable_cache_entry(path: &Path, max_age: Option<Duration>, now: SystemTime) -> bool {
    // Sidecar orphan: an external temp-reaper (macOS `$TMPDIR` cleanup,
    // container restart, CI cache eviction) removed the cache directory but
    // its sidecars survive next to it. Reclaim the leftover `.last-used` /
    // `.sha` eagerly, independent of `max_age`, so orphans do not accumulate
    // even when age-based GC is disabled (`cacheMaxAgeDays = 0`). This also
    // fixes a leak that predates #1815: a manual `git worktree remove` left
    // sidecars the old git-scoped sweep could never see.
    if !path.exists() {
        return reclaim_orphan_cache_entry(path);
    }
    let Some(max_age) = max_age else {
        return false;
    };
    reclaim_aged_cache_entry(path, max_age, now)
}

/// Reclaim the leftover sidecars of a cache directory that was deleted out
/// from under them. Lock-guarded with a re-check so a concurrent rebuild that
/// recreated the directory is preserved. The `.lock` sidecar is deliberately
/// never removed (an unlinked-but-flocked inode plus a racer's `open(O_CREAT)`
/// would split the lock across two inodes).
fn reclaim_orphan_cache_entry(path: &Path) -> bool {
    let Some(_lock) = ReusableWorktreeLock::try_acquire(path) else {
        return false;
    };
    // Re-check under the lock: a concurrent `reuse_or_create` rebuild may
    // have recreated the directory between the existence check and the lock.
    if path.exists() {
        return false;
    }
    let last_used = reusable_worktree_last_used_path(path);
    let sha = reusable_worktree_sha_path(path);
    if !last_used.exists() && !sha.exists() {
        return false;
    }
    let _ = std::fs::remove_file(&last_used);
    let _ = std::fs::remove_file(&sha);
    true
}

/// Reclaim a cache entry whose `.last-used` sidecar is older than `max_age`.
/// Seeds a fresh sidecar for pre-upgrade entries that lack one (returns
/// `false` so they age from real last-use on the next run). Removal is
/// directory-and-sidecars only: the entry is unregistered, so no `git`
/// subprocess is involved.
fn reclaim_aged_cache_entry(path: &Path, max_age: Duration, now: SystemTime) -> bool {
    let sidecar = reusable_worktree_last_used_path(path);
    let sidecar_mtime = std::fs::metadata(&sidecar)
        .ok()
        .and_then(|m| m.modified().ok());
    let Some(mtime) = sidecar_mtime else {
        touch_last_used(path);
        return false;
    };
    let Ok(age) = now.duration_since(mtime) else {
        return false;
    };
    if age < max_age {
        return false;
    }
    let Some(_lock) = ReusableWorktreeLock::try_acquire(path) else {
        return false;
    };
    let dir_removed = match std::fs::remove_dir_all(path) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "failed to remove stale reusable audit worktree directory; entry may leak",
            );
            false
        }
    };
    let _ = std::fs::remove_file(&sidecar);
    let _ = std::fs::remove_file(reusable_worktree_sha_path(path));
    dir_removed
}

/// Temp-dir basename prefix shared by every reusable cache entry of `repo_root`.
///
/// Scoping GC by this per-repo prefix keeps one repo's sweep from reclaiming
/// another repo's caches (which would let a sibling repo defeat a
/// `cacheMaxAgeDays = 0` opt-out). The hash matches
/// [`reusable_audit_worktree_path`] exactly.
fn reusable_cache_repo_prefix(repo_root: &Path) -> String {
    let repo_root = git_toplevel(repo_root).unwrap_or_else(|| repo_root.to_path_buf());
    let repo_root = dunce::canonicalize(&repo_root).unwrap_or(repo_root);
    let repo_hash = xxh3_64(repo_root.to_string_lossy().as_bytes());
    format!("fallow-audit-base-cache-{repo_hash:016x}-")
}

pub fn reusable_audit_worktree_path(repo_root: &Path, base_sha: &str) -> PathBuf {
    let sha_prefix = base_sha.get(..16).unwrap_or(base_sha);
    std::env::temp_dir().join(format!(
        "{}{sha_prefix}",
        reusable_cache_repo_prefix(repo_root)
    ))
}

/// Readiness for a reusable cache HIT: the directory exists and its `.sha`
/// sidecar records exactly `base_sha`.
///
/// Fidelity is equivalent to the old in-worktree `git rev-parse HEAD` probe:
/// that probe read the host admin dir's HEAD, never the snapshot's on-disk
/// content, so neither approach detects content damage. The `.sha` is only
/// ever written after a successful materialization + deregistration, so a
/// torn snapshot never presents a matching sidecar. On a hit the `.git` stub
/// is repaired idempotently so gitignore parity holds even if the stub was
/// removed out-of-band.
fn reusable_audit_worktree_is_ready(path: &Path, base_sha: &str) -> bool {
    if !path.exists() {
        return false;
    }
    let recorded = std::fs::read_to_string(reusable_worktree_sha_path(path))
        .ok()
        .map(|contents| contents.trim().to_owned());
    if recorded.as_deref() != Some(base_sha) {
        return false;
    }
    repair_unregistered_git_stub(path);
    true
}

/// Migrate a pre-#1815 reusable cache that is still a REGISTERED git worktree.
///
/// Older fallow versions left the base-snapshot worktree registered. On the
/// first run after upgrade, keep such a cache warm instead of rebuilding: if
/// the registered worktree's HEAD (read via the one remaining in-worktree
/// `git rev-parse`) matches `base_sha` and it was raw-materialized, seed the
/// `.sha` sidecar, deregister it in place, and treat it as ready.
fn try_migrate_legacy_reusable_cache(repo_root: &Path, path: &Path, base_sha: &str) -> bool {
    if !path.exists() || !audit_worktree_is_registered(repo_root, path) {
        return false;
    }
    let head_matches = git_rev_parse(path, "HEAD").is_some_and(|head| head == base_sha);
    if !head_matches || !fallow_engine::repo_refs::detached_base_worktree_is_raw_materialized(path)
    {
        return false;
    }
    write_reusable_sha(path, base_sha);
    unregister_worktree(path);
    true
}

/// Deregister a freshly-added audit worktree from git while KEEPING its
/// directory on disk (issue #1815).
///
/// Targets the single admin dir this worktree owns via its `.git` gitfile
/// pointer, rather than a global `git worktree prune`, so a user's unrelated
/// prunable worktrees are never collaterally deregistered and git's
/// name-collision admin suffixing (`<name>1`) is handled for free. The `.git`
/// gitfile is REPLACED with an invalid stub (see [`UNREGISTERED_GITDIR_STUB`]),
/// never deleted.
pub fn unregister_worktree(path: &Path) {
    let gitfile = path.join(".git");
    if let Ok(contents) = std::fs::read_to_string(&gitfile)
        && let Some(admin_dir) = parse_worktree_gitdir(&contents)
        && is_fallow_admin_dir(&admin_dir)
    {
        let _ = std::fs::remove_dir_all(&admin_dir);
    }
    let _ = std::fs::write(&gitfile, UNREGISTERED_GITDIR_STUB);
}

/// Ensure a cache hit's `.git` stub stays valid. Recreates the stub if the
/// gitfile is gone (so gitignore parity holds), and deregisters in place if a
/// live pointer to a fallow admin dir is found (a mixed-version re-registration
/// or a torn transient window).
fn repair_unregistered_git_stub(path: &Path) {
    let gitfile = path.join(".git");
    match std::fs::read_to_string(&gitfile) {
        Ok(contents) => {
            if parse_worktree_gitdir(&contents).is_some_and(|admin| is_fallow_admin_dir(&admin)) {
                unregister_worktree(path);
            }
        }
        Err(_) => {
            let _ = std::fs::write(&gitfile, UNREGISTERED_GITDIR_STUB);
        }
    }
}

/// Parse the `gitdir: <path>` pointer from a linked-worktree `.git` gitfile.
fn parse_worktree_gitdir(contents: &str) -> Option<PathBuf> {
    contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))
        .map(|rest| PathBuf::from(rest.trim()))
}

/// True when an admin dir basename is one fallow itself created, used as a
/// safety belt before removing it.
fn is_fallow_admin_dir(admin_dir: &Path) -> bool {
    admin_dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("fallow-audit-base-"))
}

pub fn git_rev_parse(root: &Path, rev: &str) -> Option<String> {
    let mut command = Command::new("git");
    command.args(["rev-parse", rev]).current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn git_toplevel(root: &Path) -> Option<PathBuf> {
    let mut command = Command::new("git");
    command
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    Some(dunce::canonicalize(&path).unwrap_or(path))
}

fn audit_worktree_is_registered(repo_root: &Path, path: &Path) -> bool {
    let Some(worktrees) = list_audit_worktrees(repo_root) else {
        return false;
    };
    worktrees.iter().any(|worktree| paths_equal(worktree, path))
}

pub fn paths_equal(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (dunce::canonicalize(left), dunce::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// Directories the audit base worktree shares with the host checkout.
///
/// `node_modules` is the original case: bare `git worktree add` lacks the
/// installed dependencies. `.nuxt` / `.astro` extend the same idea to
/// meta-framework `prepare` / `sync` outputs that the project gitignores;
/// without them the base pass cannot resolve tsconfig `references` chains
/// pointing into the generated tsconfigs and falls back to resolver-less
/// resolution. The trade-off matches `node_modules`: the symlinked dir is
/// HEAD-shaped, not base-shaped, but the alias resolution accuracy recovered
/// far outweighs the residual drift.
///
/// The meta-framework entries must stay aligned with the set recognized by
/// `missing_meta_framework_prerequisites` in `fallow_core`'s plugin registry.
/// Adding a framework's prepare-dir warning there without extending this list
/// silently reintroduces the broken-tsconfig-chain bug on the base pass for
/// that framework.
const MATERIALIZED_CONTEXT_DIRS: &[&str] = &["node_modules", ".nuxt", ".astro"];

pub fn materialize_base_dependency_context(repo_root: &Path, worktree_path: &Path) {
    for &name in MATERIALIZED_CONTEXT_DIRS {
        let source = repo_root.join(name);
        if !source.is_dir() {
            continue;
        }

        let destination = worktree_path.join(name);
        if destination.is_dir() {
            continue;
        }
        if let Ok(metadata) = std::fs::symlink_metadata(&destination) {
            if !metadata.file_type().is_symlink() {
                continue;
            }
            let _ = std::fs::remove_file(&destination);
        }

        let _ = symlink_dependency_dir(&source, &destination);
    }
}

#[cfg(unix)]
fn symlink_dependency_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
fn symlink_dependency_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(source, destination)
}

pub fn remove_audit_worktree(repo_root: &Path, path: &Path) {
    let mut command = Command::new("git");
    command
        .args([
            "worktree",
            "remove",
            "--force",
            path.to_string_lossy().as_ref(),
        ])
        .current_dir(repo_root);
    clear_ambient_git_env(&mut command);
    match crate::signal::scoped_child::output(&mut command) {
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

pub fn sweep_orphan_audit_worktrees(repo_root: &Path) {
    sweep_orphan_audit_worktrees_in(repo_root, &std::env::temp_dir());
}

/// `temp_root` is the directory scanned for unregistered dead-pid worktree
/// directories. Production always passes `std::env::temp_dir()`; tests pass a
/// private root because a fabricated dead-pid fixture in the SHARED temp dir
/// is legitimate prey for any concurrent sweep (a parallel test or a spawned
/// fallow binary), which races the fixture's own assertions.
pub fn sweep_orphan_audit_worktrees_in(repo_root: &Path, temp_root: &Path) {
    // Legacy pass: deregister dead-pid non-reusable worktrees left REGISTERED
    // by pre-#1815 fallow (or by a crash in the transient registration
    // window). The `--expire=now` prune, retained only here, also sweeps any
    // now-dangling admin entry.
    if deregister_legacy_orphan_worktrees(repo_root) {
        let mut command = Command::new("git");
        command
            .args(["worktree", "prune", "--expire=now"])
            .current_dir(repo_root);
        clear_ambient_git_env(&mut command);
        let _ = command.output();
    }

    // Primary pass: remove unregistered dead-pid worktree DIRECTORIES via a
    // temp-dir prefix scan. A dead PID means the owning process is gone
    // regardless of repo, so this scan is global (not repo-scoped).
    for path in scan_non_reusable_orphan_paths(temp_root) {
        let _ = std::fs::remove_dir_all(&path);
    }
}

/// Deregister dead-pid non-reusable worktrees left REGISTERED by pre-#1815
/// fallow or by a crash mid-registration. Returns `true` when any were removed.
fn deregister_legacy_orphan_worktrees(repo_root: &Path) -> bool {
    let Some(worktrees) = list_audit_worktrees(repo_root) else {
        return false;
    };
    let mut removed_any = false;
    for path in worktrees {
        if !is_fallow_audit_worktree_path(&path)
            || is_reusable_audit_worktree_path(&path)
            || audit_worktree_process_is_alive(&path)
        {
            continue;
        }
        remove_audit_worktree(repo_root, &path);
        let _ = std::fs::remove_dir_all(&path);
        removed_any = true;
    }
    removed_any
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

pub fn list_audit_worktrees(repo_root: &Path) -> Option<Vec<PathBuf>> {
    let mut command = Command::new("git");
    command
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_worktree_list(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub fn parse_worktree_list(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .filter(|path| is_fallow_audit_worktree_path(path))
        .collect()
}

pub fn is_fallow_audit_worktree_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with("fallow-audit-base-") && path_is_inside_temp_dir(path)
}

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

pub fn audit_worktree_pid(name: &str) -> Option<u32> {
    name.strip_prefix("fallow-audit-base-")?
        .split('-')
        .next()?
        .parse()
        .ok()
}

#[cfg(unix)]
pub fn process_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(windows)]
pub fn process_is_alive(pid: u32) -> bool {
    windows_process::is_alive(pid)
}

#[cfg(not(any(unix, windows)))]
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

impl Drop for BaseWorktree {
    fn drop(&mut self) {
        if self.persistent {
            return;
        }
        // The non-reusable worktree was deregistered right after creation, so
        // cleanup is a plain directory removal: no `git` subprocess runs, and
        // a SIGKILL before this Drop can never leave an admin entry behind.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Many threads minting a non-reusable worktree path at the same instant
    /// must each get a distinct path. Before the monotonic counter, concurrent
    /// callers read the same non-monotonic `nanos` and collided, so two parallel
    /// `audit` runs in one process raced on `git worktree add` and one aborted
    /// with a generic error (a flaky exit 2 under parallel unit tests).
    #[test]
    fn non_reusable_worktree_paths_are_unique_under_concurrency() {
        const N: usize = 64;
        let barrier = std::sync::Barrier::new(N);
        let paths = std::sync::Mutex::new(Vec::with_capacity(N));
        std::thread::scope(|s| {
            for _ in 0..N {
                let barrier = &barrier;
                let paths = &paths;
                s.spawn(move || {
                    barrier.wait();
                    let path = non_reusable_worktree_path().expect("path should build");
                    paths.lock().unwrap().push(path);
                });
            }
        });
        let mut paths = paths.into_inner().unwrap();
        assert_eq!(paths.len(), N);
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), N, "non-reusable worktree paths collided");
    }

    /// The pid stays the first segment so orphan-sweep parsing keeps working.
    #[test]
    fn non_reusable_worktree_path_pid_is_parseable() {
        let path = non_reusable_worktree_path().expect("path should build");
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(is_fallow_audit_worktree_path(&path));
        assert!(!is_reusable_audit_worktree_path(&path));
        assert_eq!(audit_worktree_pid(name), Some(std::process::id()));
    }
}
