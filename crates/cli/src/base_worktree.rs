use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use fallow_engine::changed_files::clear_ambient_git_env;
pub use fallow_engine::repo_refs::materialize_base_dependency_context;
use rustc_hash::{FxHashMap, FxHashSet};
use xxhash_rust::xxh3::xxh3_64;

use crate::report::plural;

pub struct BaseWorktree {
    path: PathBuf,
    persistent: bool,
    _reusable_lock: Option<ReusableWorktreeLock>,
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
        if let Err(error) = unregister_worktree(repo_root, guard.path()) {
            tracing::debug!(
                path = %guard.path().display(),
                error = %error,
                "could not deregister non-reusable audit base worktree",
            );
            return None;
        }
        guard.defuse();
        drop(guard);
        let worktree = Self {
            path,
            persistent: false,
            _reusable_lock: None,
        };
        materialize_base_dependency_context(repo_root, worktree.path());
        Some(worktree)
    }

    pub fn reuse_or_create(repo_root: &Path, base_sha: &str) -> Option<Self> {
        let path = reusable_audit_worktree_path(repo_root);
        let reusable_lock =
            ReusableWorktreeLock::try_acquire(&path, "falling back to non-reusable worktree")?;

        if reusable_audit_worktree_is_ready(&path, base_sha)
            || try_migrate_registered_current_cache(repo_root, &path, base_sha)
        {
            let worktree = Self {
                path,
                persistent: true,
                _reusable_lock: Some(reusable_lock),
            };
            materialize_base_dependency_context(repo_root, worktree.path());
            record_last_used(worktree.path(), repo_root);
            return Some(worktree);
        }

        if let Err(error) = remove_file_if_exists(&reusable_worktree_sha_path(&path)) {
            tracing::debug!(
                path = %path.display(),
                error = %error,
                "could not clear reusable audit worktree readiness before rebuild",
            );
            return None;
        }
        if let Err(error) = remove_reusable_cache_entry_locked(repo_root, &path) {
            tracing::debug!(
                path = %path.display(),
                error = %error,
                "could not remove stale reusable audit worktree before rebuild",
            );
            return None;
        }
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
        // Deregister while keeping the directory, then atomically publish the
        // full base SHA through the `.sha` sidecar. Publication happens only
        // after successful materialization and deregistration under the lock,
        // so a torn snapshot is never advertised as ready to the next run.
        if let Err(error) = unregister_worktree_checked(repo_root, guard.path()) {
            tracing::debug!(
                path = %guard.path().display(),
                error = %error,
                "could not deregister reusable audit base worktree",
            );
            return None;
        }
        guard.defuse();
        drop(guard);
        let readiness_published = write_reusable_sha(&path, base_sha).is_ok();

        let worktree = Self {
            path,
            persistent: true,
            _reusable_lock: Some(reusable_lock),
        };
        materialize_base_dependency_context(repo_root, worktree.path());
        if readiness_published {
            record_last_used(worktree.path(), repo_root);
        }
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
    file: std::fs::File,
}

impl ReusableWorktreeLock {
    /// `context` names the caller's contention fallback in the debug
    /// diagnostic ("falls back to a non-reusable worktree" is only true for
    /// `reuse_or_create`; a GC sweep or prune skips the entry instead).
    pub fn try_acquire(reusable_path: &Path, context: &'static str) -> Option<Self> {
        let lock_path = reusable_worktree_lock_path(reusable_path);
        let file = open_or_create_owned_sidecar(&lock_path).ok()?;
        match file.try_lock() {
            Ok(()) => Some(Self { file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                tracing::debug!(
                    path = %lock_path.display(),
                    context,
                    "reusable audit worktree lock contended",
                );
                None
            }
            Err(std::fs::TryLockError::Error(err)) => {
                tracing::debug!(
                    path = %lock_path.display(),
                    error = %err,
                    context,
                    "could not acquire reusable audit worktree lock",
                );
                None
            }
        }
    }
}

impl Drop for ReusableWorktreeLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
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
fn write_reusable_sha(reusable_path: &Path, base_sha: &str) -> std::io::Result<()> {
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
        file.write_all(format!("{base_sha}\n").as_bytes())?;
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

/// Default GC threshold for persistent reusable base-snapshot caches.
const DEFAULT_AUDIT_CACHE_MAX_AGE_DAYS: u32 = 30;

const SECONDS_PER_DAY: u64 = 86_400;

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
    stamp_last_used(reusable_path, None);
}

/// Stamp `.last-used` like [`touch_last_used`] and additionally record
/// `owner_root` (the requested analysis root that owns this cache) as the
/// sidecar's content. The cross-repo GC pass reads it back to decide whether
/// an entry outside the current repo's scope is still owned by a live project
/// (issue #2169): caches whose recorded owner no longer exists on disk are
/// abandoned and become eligible for age-based reclaim from any repo's sweep.
pub fn record_last_used(reusable_path: &Path, owner_root: &Path) {
    stamp_last_used(reusable_path, Some(owner_root));
}

fn stamp_last_used(reusable_path: &Path, owner_root: Option<&Path>) {
    let last_used = reusable_worktree_last_used_path(reusable_path);
    let result = open_or_create_owned_sidecar(&last_used).and_then(|mut file| {
        if let Some(owner_root) = owner_root {
            file.set_len(0)?;
            file.write_all(format!("{}\n", owner_root.display()).as_bytes())?;
        }
        file.set_modified(SystemTime::now())
    });
    if let Err(err) = result {
        tracing::warn!(
            path = %last_used.display(),
            error = %err,
            "failed to touch reusable audit worktree sidecar; staleness signal may not update",
        );
    }
}

/// Read the owner root recorded in a cache entry's `.last-used` sidecar.
/// Returns `None` for missing, empty (pre-#2169), oversized, or non-file
/// sidecars. The path is only ever used for an existence probe, never as a
/// removal target, so untrusted content cannot redirect the sweep.
fn read_last_used_owner(reusable_path: &Path) -> Option<PathBuf> {
    const MAX_OWNER_SIDECAR_BYTES: u64 = 4096;

    let sidecar = reusable_worktree_last_used_path(reusable_path);
    let metadata = std::fs::symlink_metadata(&sidecar).ok()?;
    if !metadata_is_regular_file(&metadata) || metadata.len() > MAX_OWNER_SIDECAR_BYTES {
        return None;
    }
    let mut contents = String::new();
    std::fs::File::open(sidecar)
        .ok()?
        .take(MAX_OWNER_SIDECAR_BYTES)
        .read_to_string(&mut contents)
        .ok()?;
    let owner = contents.trim();
    if owner.is_empty() {
        return None;
    }
    Some(PathBuf::from(owner))
}

fn open_or_create_owned_sidecar(path: &Path) -> std::io::Result<std::fs::File> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if sidecar_metadata_is_trusted(&metadata) => {
            std::fs::OpenOptions::new().write(true).open(path)
        }
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "refusing to open an untrusted audit cache sidecar",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            options.open(path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn sidecar_metadata_is_trusted(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    metadata_is_regular_file(metadata) && metadata.uid() == rustix::process::geteuid().as_raw()
}

#[cfg(not(unix))]
fn sidecar_metadata_is_trusted(metadata: &std::fs::Metadata) -> bool {
    metadata_is_regular_file(metadata)
}

#[expect(
    clippy::filetype_is_file,
    reason = "security-sensitive sidecars and gitfiles must be regular files, not arbitrary non-directories"
)]
fn metadata_is_regular_file(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_file()
}

/// Which precedence level supplied the effective cache GC threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMaxAgeSource {
    Flag,
    Env,
    Config,
    Default,
}

impl CacheMaxAgeSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Env => "env",
            Self::Config => "config",
            Self::Default => "default",
        }
    }
}

/// Effective GC threshold plus the precedence level that supplied it.
/// `max_age` is `None` when the winning value is `0` (age-based GC disabled).
#[derive(Debug, Clone, Copy)]
pub struct ResolvedCacheMaxAge {
    pub max_age: Option<Duration>,
    pub days: u32,
    pub source: CacheMaxAgeSource,
}

/// Resolve the GC threshold for persistent reusable caches.
///
/// Precedence: `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS` env var > `audit.cacheMaxAgeDays`
/// config field > 30-day default. `0` from either source disables the sweep
/// entirely (returns `None`). Invalid env values (non-integer) warn and fall
/// back to config / default; audits do not fail on a typo in a runner env var.
pub fn resolve_cache_max_age_with_options(
    root: &Path,
    config_path: Option<&PathBuf>,
    allow_remote_extends: bool,
) -> Option<Duration> {
    resolve_cache_max_age_with_source(root, config_path, allow_remote_extends, None).max_age
}

/// [`resolve_cache_max_age_with_options`] with an optional per-invocation
/// override (`fallow audit-cache prune --max-age-days`) that wins over every
/// other level, and the winning source reported for display.
pub fn resolve_cache_max_age_with_source(
    root: &Path,
    config_path: Option<&PathBuf>,
    allow_remote_extends: bool,
    flag_days: Option<u32>,
) -> ResolvedCacheMaxAge {
    if let Some(days) = flag_days {
        return ResolvedCacheMaxAge {
            max_age: days_to_duration(days),
            days,
            source: CacheMaxAgeSource::Flag,
        };
    }
    if let Ok(raw) = std::env::var(AUDIT_CACHE_MAX_AGE_ENV) {
        if let Ok(days) = raw.trim().parse::<u32>() {
            return ResolvedCacheMaxAge {
                max_age: days_to_duration(days),
                days,
                source: CacheMaxAgeSource::Env,
            };
        }
        tracing::warn!(
            value = %raw,
            "FALLOW_AUDIT_CACHE_MAX_AGE_DAYS is not a valid u32; falling back to config/default",
        );
    }
    if let Some(days) = load_audit_config(root, config_path, allow_remote_extends)
        .and_then(|c| c.cache_max_age_days)
    {
        return ResolvedCacheMaxAge {
            max_age: days_to_duration(days),
            days,
            source: CacheMaxAgeSource::Config,
        };
    }
    ResolvedCacheMaxAge {
        max_age: days_to_duration(DEFAULT_AUDIT_CACHE_MAX_AGE_DAYS),
        days: DEFAULT_AUDIT_CACHE_MAX_AGE_DAYS,
        source: CacheMaxAgeSource::Default,
    }
}

pub fn days_to_duration(days: u32) -> Option<Duration> {
    if days == 0 {
        return None;
    }
    Some(Duration::from_secs(u64::from(days) * SECONDS_PER_DAY))
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
/// Entries under other repo hashes (other linked worktrees, deleted or moved
/// repos) are visited by a cross-repo pass; see
/// [`reclaim_foreign_cache_entry`] for its owner-liveness gate (issue #2169).
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
    sweep_old_reusable_caches_in(repo_root, max_age, quiet, &std::env::temp_dir());
}

/// Scan-root-injectable body of [`sweep_old_reusable_caches`]. Production
/// always passes `std::env::temp_dir()`; tests pass a private per-test
/// directory so one test's cross-repo pass never scans (and reclaims)
/// another test's fixtures or a developer's real ownerless caches in the
/// shared temp dir.
pub fn sweep_old_reusable_caches_in(
    repo_root: &Path,
    max_age: Option<Duration>,
    quiet: bool,
    scan_root: &Path,
) {
    let report = sweep_reusable_caches_with_report(
        repo_root,
        max_age,
        scan_root,
        SweepMode::Apply,
        SweepSizes::Skip,
    );
    log_sweep_entries(&report, SweepMode::Apply, max_age);
    let removed = report.removed;
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

/// How a cache sweep executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepMode {
    /// Normal GC: acquire locks, seed grace sidecars, remove entries.
    Apply,
    /// Preview: dispositions are computed from read-only probes only. No lock
    /// acquisition (`try_acquire` would create `.lock` sidecars), no grace
    /// seeding, no removal, no `git` worktree deregistration or prune.
    DryRun,
}

impl SweepMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::DryRun => "dry-run",
        }
    }
}

/// Whether the sweep measures per-entry directory sizes. Measuring walks the
/// full cache tree, so only the manual prune command asks for it; the
/// per-audit sweep stays IO-light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepSizes {
    Skip,
    Measure,
}

/// Which enumeration pass produced a sweep candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepPass {
    /// The requested root's own cache entry.
    Owned,
    /// Old SHA-suffixed entries under this repo's hash.
    Legacy,
    /// Entries under other repo hashes (cross-repo pass, issue #2169).
    Foreign,
}

impl SweepPass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owned => "owned",
            Self::Legacy => "legacy",
            Self::Foreign => "foreign",
        }
    }
}

/// Decision bucket of a [`SweepDisposition`], the closed `disposition` set of
/// the prune JSON envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepDecision {
    Removed,
    Kept,
    Skipped,
    Failed,
}

impl SweepDecision {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Removed => "removed",
            Self::Kept => "kept",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
}

/// Per-entry outcome of one sweep candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepDisposition {
    /// Sidecars of a cache directory an external reaper already deleted.
    ReclaimedOrphan,
    /// Entry aged past the effective threshold.
    ReclaimedAged,
    /// Foreign entry whose recorded owner root is gone, reclaimed while
    /// age-based GC is disabled (`cacheMaxAgeDays = 0`). Nothing will ever
    /// sweep it under its own hash again, so the age gate that `0` switches
    /// off would otherwise strand it on disk forever.
    ReclaimedOwnerMissing,
    /// A released SHA-keyed directory still REGISTERED as a git worktree by
    /// pre-#1815 fallow; the root-owned cache cannot reuse it, so it is
    /// deregistered AND removed.
    ReclaimedLegacyRegistered,
    /// The current cache path still REGISTERED as a git worktree by pre-#1815
    /// fallow (a mixed-version registration); only the git registration is
    /// reclaimed and the directory stays warm on disk, so its measured size
    /// never counts as reclaimed bytes.
    KeptLegacyDeregistered,
    /// Younger than the threshold.
    KeptFresh,
    /// Foreign entry whose recorded owner root still exists; that repo's own
    /// sweep governs it.
    KeptOwnerLive,
    /// Foreign entry whose owner probe failed with a non-NotFound error; kept
    /// so a transient probe failure can never reclaim a live repo's cache.
    KeptOwnerUnverifiable(std::io::ErrorKind),
    /// Age-based GC disabled (`cacheMaxAgeDays = 0`) and the entry is not an
    /// orphan.
    KeptAgeGcDisabled,
    /// First encounter of a pre-upgrade entry without a `.last-used` sidecar;
    /// a sidecar is seeded so it ages from real last-use.
    KeptGraceSeeded,
    /// Only a `.lock` sidecar survives (never deleted by design); there is no
    /// directory, `.sha`, or `.last-used` left to reclaim.
    KeptLockOnly,
    /// The orphan re-check under the lock found the directory rebuilt by a
    /// concurrent run.
    KeptRecreated,
    /// The Unix uid ownership check rejects removal (another user's entry on
    /// a shared runner).
    KeptNotOwned,
    /// Lock contention; a concurrent fallow holds this entry.
    SkippedLocked,
    /// A genuine IO error after the ownership check passed.
    RemoveFailed,
}

impl SweepDisposition {
    #[must_use]
    pub const fn decision(self) -> SweepDecision {
        match self {
            Self::ReclaimedOrphan
            | Self::ReclaimedAged
            | Self::ReclaimedOwnerMissing
            | Self::ReclaimedLegacyRegistered => SweepDecision::Removed,
            Self::KeptLegacyDeregistered
            | Self::KeptFresh
            | Self::KeptOwnerLive
            | Self::KeptOwnerUnverifiable(_)
            | Self::KeptAgeGcDisabled
            | Self::KeptGraceSeeded
            | Self::KeptLockOnly
            | Self::KeptRecreated
            | Self::KeptNotOwned => SweepDecision::Kept,
            Self::SkippedLocked => SweepDecision::Skipped,
            Self::RemoveFailed => SweepDecision::Failed,
        }
    }

    /// Closed `reason` vocabulary of the prune JSON envelope and the debug
    /// diagnostics.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::ReclaimedOrphan => "orphaned-sidecars",
            Self::ReclaimedAged => "aged-out",
            Self::ReclaimedOwnerMissing => "owner-missing",
            Self::ReclaimedLegacyRegistered => "legacy-registered",
            Self::KeptLegacyDeregistered => "legacy-deregistered",
            Self::KeptFresh => "fresh",
            Self::KeptOwnerLive => "owner-live",
            Self::KeptOwnerUnverifiable(_) => "owner-unverifiable",
            Self::KeptAgeGcDisabled => "age-gc-disabled",
            Self::KeptGraceSeeded => "grace-seeded",
            Self::KeptLockOnly => "lock-only",
            Self::KeptRecreated => "recreated",
            Self::KeptNotOwned => "not-owned",
            Self::SkippedLocked => "lock-contention",
            Self::RemoveFailed => "remove-failed",
        }
    }

    /// Whether this disposition increments the legacy `removed` counter used
    /// by the per-audit stderr summary. Legacy-registered reclaims never did.
    const fn counts_toward_summary(self) -> bool {
        matches!(
            self,
            Self::ReclaimedOrphan | Self::ReclaimedAged | Self::ReclaimedOwnerMissing
        )
    }
}

/// One considered candidate of a cache sweep.
#[derive(Debug, Clone)]
pub struct SweepEntry {
    pub path: PathBuf,
    pub pass: SweepPass,
    pub disposition: SweepDisposition,
    /// Whole days since the `.last-used` sidecar mtime, when readable.
    pub age_days: Option<u64>,
    /// Owner root recorded in the `.last-used` sidecar, when present.
    pub owner_root: Option<PathBuf>,
    /// Recursive directory size, measured only under [`SweepSizes::Measure`]
    /// and `None` when the directory is absent or unreadable.
    pub size_bytes: Option<u64>,
}

/// Full result of one sweep. `removed` keeps the pre-#2221 counter semantics
/// (orphan and aged reclaims from the owned/legacy and cross-repo passes) so
/// the audit stderr summary is unchanged.
#[derive(Debug, Default)]
pub struct SweepReport {
    pub entries: Vec<SweepEntry>,
    pub removed: u32,
}

impl SweepReport {
    fn push(&mut self, entry: SweepEntry) {
        if entry.disposition.counts_toward_summary() {
            self.removed += 1;
        }
        self.entries.push(entry);
    }
}

const GC_LOCK_CONTEXT: &str = "gc sweep skips the entry";

/// Emit one `tracing::debug!` line per considered entry. Shared by the
/// implicit per-audit sweep and `fallow audit-cache prune`, so both surfaces
/// produce identical diagnostics under an explicit `RUST_LOG` directive.
pub fn log_sweep_entries(report: &SweepReport, mode: SweepMode, max_age: Option<Duration>) {
    let threshold_days = max_age.map(|age| age.as_secs() / SECONDS_PER_DAY);
    for entry in &report.entries {
        let owner_probe_error = match entry.disposition {
            SweepDisposition::KeptOwnerUnverifiable(kind) => Some(kind),
            _ => None,
        };
        tracing::debug!(
            path = %entry.path.display(),
            pass = entry.pass.as_str(),
            mode = mode.as_str(),
            decision = entry.disposition.decision().as_str(),
            reason = entry.disposition.reason(),
            age_days = entry.age_days,
            threshold_days,
            owner_root = entry
                .owner_root
                .as_ref()
                .map(|owner| tracing::field::display(owner.display())),
            owner_probe_error = owner_probe_error.map(tracing::field::debug),
            "audit cache sweep considered entry",
        );
    }
}

/// Report-producing body shared by the per-audit GC sweep
/// ([`sweep_old_reusable_caches_in`], [`SweepMode::Apply`], sizes skipped)
/// and `fallow audit-cache prune` (both modes, sizes measured). Running the
/// same code path is what guarantees prune "runs the same reclaim logic"
/// (issue #2221) by construction.
pub fn sweep_reusable_caches_with_report(
    repo_root: &Path,
    max_age: Option<Duration>,
    scan_root: &Path,
    mode: SweepMode,
    sizes: SweepSizes,
) -> SweepReport {
    let now = SystemTime::now();
    let mut report = SweepReport::default();

    // Legacy pass: deregister reusable caches left REGISTERED by pre-#1815
    // fallow (the reporter's `git worktree list` backlog). This is what makes
    // those entries vanish on the first post-upgrade audit; the `--expire=now`
    // prune is retained ONLY here to also sweep any admin entry orphaned by a
    // crash in the transient registration window.
    legacy_registered_pass(repo_root, mode, sizes, now, &mut report);

    // Primary pass: visit this requested root's cache plus legacy SHA-suffixed
    // entries from the old git-top-level identity. `git worktree list` no
    // longer sees the deregistered caches.
    let owned_path = reusable_audit_worktree_path(repo_root);
    let mut paths = vec![owned_path.clone()];
    paths.extend(scan_legacy_reusable_cache_paths(repo_root, scan_root));
    paths.sort();
    paths.dedup();

    // Cross-repo pass (issue #2169): every other repo hash accumulates caches
    // the repo-scoped pass can never see (one per linked git worktree, plus
    // entries from deleted or moved repos that no surviving sweep covers).
    // Entries whose recorded owner root still exists are left to that repo's
    // own sweep, so its `cacheMaxAgeDays` setting, including `0`, governs;
    // the rest are abandoned and age out under this run's threshold.
    let scoped: FxHashSet<&PathBuf> = paths.iter().collect();
    let foreign: Vec<PathBuf> = scan_all_reusable_cache_paths(scan_root)
        .into_iter()
        .filter(|path| !scoped.contains(path))
        .collect();
    drop(scoped);

    // Sizes are measured BEFORE the decision pass so removed entries still
    // report what they occupied.
    let mut size_map = measure_entry_sizes(sizes, paths.iter().chain(foreign.iter()));

    for path in &paths {
        // The requested root's path is a synthetic candidate (always added);
        // when nothing exists for it on disk (no directory, sidecars, or
        // `.lock`) there is nothing to consider or report, and probing it
        // under `Apply` would mint a `.lock` sidecar for a cache that was
        // never built. Scan-derived candidates always have some presence.
        if !cache_entry_has_presence(path) {
            continue;
        }
        let pass = if *path == owned_path {
            SweepPass::Owned
        } else {
            SweepPass::Legacy
        };
        let (age_days, owner_root) = entry_probe_metadata(path, now);
        let disposition = match mode {
            SweepMode::Apply => reclaim_reusable_cache_entry(repo_root, path, max_age, now),
            SweepMode::DryRun => classify_entry_dry_run(path, max_age, now, OwnerGate::Off),
        };
        report.push(SweepEntry {
            path: path.clone(),
            pass,
            disposition,
            age_days,
            owner_root,
            size_bytes: size_map.remove(path).flatten(),
        });
    }
    for path in &foreign {
        let (age_days, owner_root) = entry_probe_metadata(path, now);
        let disposition = match mode {
            SweepMode::Apply => reclaim_foreign_cache_entry(repo_root, path, max_age, now),
            SweepMode::DryRun => classify_entry_dry_run(path, max_age, now, OwnerGate::On),
        };
        report.push(SweepEntry {
            path: path.clone(),
            pass: SweepPass::Foreign,
            disposition,
            age_days,
            owner_root,
            size_bytes: size_map.remove(path).flatten(),
        });
    }
    report
}

/// Read-only per-entry metadata for reporting, gathered BEFORE the decision
/// pass may remove the sidecars it comes from.
fn entry_probe_metadata(path: &Path, now: SystemTime) -> (Option<u64>, Option<PathBuf>) {
    let age_days = last_used_mtime(path)
        .and_then(|mtime| now.duration_since(mtime).ok())
        .map(|age| age.as_secs() / SECONDS_PER_DAY);
    (age_days, read_last_used_owner(path))
}

/// Deregister reusable base-snapshot caches left REGISTERED by fallow versions
/// before #1815. A mixed-version registration at the current path stays warm
/// and reports [`SweepDisposition::KeptLegacyDeregistered`] (only the git
/// registration goes away, so nothing is reclaimed); released SHA-keyed paths
/// are removed because the root-owned cache cannot reuse them. Under
/// [`SweepMode::DryRun`] only the read-only halves run
/// (`list_audit_worktrees`, `audit_worktree_is_registered`): registered
/// entries are reported without locks, deregistration, removal, or the
/// trailing `git worktree prune`.
fn legacy_registered_pass(
    repo_root: &Path,
    mode: SweepMode,
    sizes: SweepSizes,
    now: SystemTime,
    report: &mut SweepReport,
) {
    let Some(worktrees) = list_audit_worktrees(repo_root) else {
        return;
    };
    let candidates: Vec<PathBuf> = worktrees
        .into_iter()
        .filter(|path| is_reusable_audit_worktree_path(path))
        .collect();
    let mut size_map = measure_entry_sizes(sizes, candidates.iter());
    let owned_path = reusable_audit_worktree_path(repo_root);
    let mut deregistered = false;
    for path in candidates {
        let pass = if paths_equal(&path, &owned_path) {
            SweepPass::Owned
        } else {
            SweepPass::Legacy
        };
        let (age_days, owner_root) = entry_probe_metadata(&path, now);
        let size_bytes = size_map.remove(&path).flatten();
        let is_current_path = pass == SweepPass::Owned;
        let disposition = match mode {
            SweepMode::DryRun => {
                if !audit_worktree_is_registered(repo_root, &path) {
                    continue;
                }
                if is_current_path {
                    SweepDisposition::KeptLegacyDeregistered
                } else {
                    SweepDisposition::ReclaimedLegacyRegistered
                }
            }
            SweepMode::Apply => {
                let Some(_lock) = ReusableWorktreeLock::try_acquire(
                    &path,
                    "legacy deregistration skips the entry",
                ) else {
                    report.push(SweepEntry {
                        path,
                        pass,
                        disposition: SweepDisposition::SkippedLocked,
                        age_days,
                        owner_root,
                        size_bytes,
                    });
                    continue;
                };
                if !audit_worktree_is_registered(repo_root, &path) {
                    continue;
                }
                let head = is_current_path
                    .then(|| legacy_reusable_sha(&path))
                    .flatten();
                if unregister_worktree_checked(repo_root, &path).is_err() {
                    report.push(SweepEntry {
                        path,
                        pass,
                        disposition: SweepDisposition::RemoveFailed,
                        age_days,
                        owner_root,
                        size_bytes,
                    });
                    continue;
                }
                let disposition = if is_current_path {
                    if let Some(head) = head {
                        let _ = write_reusable_sha(&path, &head);
                    }
                    SweepDisposition::KeptLegacyDeregistered
                } else if let Err(error) = remove_reusable_cache_entry_locked(repo_root, &path) {
                    tracing::warn!(
                        path = %path.display(),
                        error = %error,
                        "failed to remove released SHA-keyed audit cache",
                    );
                    SweepDisposition::RemoveFailed
                } else {
                    SweepDisposition::ReclaimedLegacyRegistered
                };
                deregistered = true;
                disposition
            }
        };
        report.push(SweepEntry {
            path,
            pass,
            disposition,
            age_days,
            owner_root,
            size_bytes,
        });
    }
    if deregistered {
        let mut command = Command::new("git");
        command
            .args(["worktree", "prune", "--expire=now"])
            .current_dir(repo_root);
        clear_ambient_git_env(&mut command);
        let _ = command.output();
    }
}

/// Seed the `.sha` sidecar for a still-registered legacy cache from its HEAD,
/// so after deregistration the readiness probe recognizes it as warm. Seeds
/// only when the snapshot was raw-materialized and no `.sha` exists yet.
fn legacy_reusable_sha(path: &Path) -> Option<String> {
    if reusable_worktree_sha_path(path).exists()
        || !fallow_engine::repo_refs::detached_base_worktree_is_raw_materialized(path)
    {
        return None;
    }
    git_rev_parse(path, "HEAD")
}

/// Enumerate reusable cache DIRECTORY paths for `prefix` by scanning the temp
/// dir. Sidecar entries (`.last-used` / `.sha` / `.lock`) are folded back to
/// their owning cache path and deduplicated, so a dir removed out from under
/// its sidecars is still visited for sidecar-orphan cleanup.
fn scan_legacy_reusable_cache_paths(repo_root: &Path, scan_root: &Path) -> Vec<PathBuf> {
    let Some(prefix) = legacy_reusable_cache_repo_prefix(repo_root) else {
        return Vec::new();
    };
    scan_cache_paths_with_hex_suffix(&prefix, scan_root)
}

fn scan_root_owned_cache_paths(repo_root: &Path) -> Vec<PathBuf> {
    let Some(prefix) = root_owned_cache_repo_prefix(repo_root) else {
        return Vec::new();
    };
    scan_cache_paths_with_hex_suffix(&prefix, &std::env::temp_dir())
}

/// Enumerate every reusable cache entry in the temp dir, regardless of repo
/// hash, for the cross-repo GC pass. Only names matching the two shapes
/// fallow has ever minted are accepted: `<repo16>-<sha16>` (legacy) and
/// `<repo16>-root-<root16>` (current). Sidecar entries fold back to their
/// owning cache path like the repo-scoped scan.
fn scan_all_reusable_cache_paths(scan_root: &Path) -> Vec<PathBuf> {
    const GLOBAL_CACHE_PREFIX: &str = "fallow-audit-base-cache-";

    let Ok(entries) = std::fs::read_dir(scan_root) else {
        return Vec::new();
    };
    let mut seen: FxHashSet<PathBuf> = FxHashSet::default();
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let cache_name = strip_cache_sidecar_suffix(name);
        let Some(hash_suffix) = cache_name.strip_prefix(GLOBAL_CACHE_PREFIX) else {
            continue;
        };
        if !cache_hash_suffix_is_valid(hash_suffix) {
            continue;
        }
        let path = scan_root.join(cache_name);
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    paths
}

fn cache_hash_suffix_is_valid(suffix: &str) -> bool {
    fn is_hex16(part: &str) -> bool {
        part.len() == 16 && part.bytes().all(|byte| byte.is_ascii_hexdigit())
    }
    if let Some((repo, root)) = suffix.split_once("-root-") {
        return is_hex16(repo) && is_hex16(root);
    }
    suffix
        .split_once('-')
        .is_some_and(|(repo, sha)| is_hex16(repo) && is_hex16(sha))
}

fn scan_cache_paths_with_hex_suffix(prefix: &str, scan_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(scan_root) else {
        return Vec::new();
    };
    let mut seen: FxHashSet<PathBuf> = FxHashSet::default();
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let cache_name = strip_cache_sidecar_suffix(name);
        let Some(hash_suffix) = cache_name.strip_prefix(prefix) else {
            continue;
        };
        if hash_suffix.len() != 16 || !hash_suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let path = scan_root.join(cache_name);
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

/// Reclaim a single reusable-cache entry, reporting the per-entry outcome.
fn reclaim_reusable_cache_entry(
    repo_root: &Path,
    path: &Path,
    max_age: Option<Duration>,
    now: SystemTime,
) -> SweepDisposition {
    // Sidecar orphan: an external temp-reaper (macOS `$TMPDIR` cleanup,
    // container restart, CI cache eviction) removed the cache directory but
    // its sidecars survive next to it. Reclaim the leftover `.last-used` /
    // `.sha` eagerly, independent of `max_age`, so orphans do not accumulate
    // even when age-based GC is disabled (`cacheMaxAgeDays = 0`). This also
    // fixes a leak that predates #1815: a manual `git worktree remove` left
    // sidecars the old git-scoped sweep could never see.
    //
    // A probe error here reads as "exists" and falls through to the age
    // branch; failing toward keeping the entry is a cheap self-rebuild at
    // worst, and the orphan branch re-checks under the lock anyway.
    if !path.exists() {
        return reclaim_orphan_cache_entry(repo_root, path);
    }
    let Some(max_age) = max_age else {
        return SweepDisposition::KeptAgeGcDisabled;
    };
    reclaim_aged_cache_entry(repo_root, path, max_age, now)
}

/// Reclaim the leftover sidecars of a cache directory that was deleted out
/// from under them. Lock-guarded with a re-check so a concurrent rebuild that
/// recreated the directory is preserved. The `.lock` sidecar is deliberately
/// never removed (an unlinked-but-still-flocked inode plus a racer's
/// `open(O_CREAT)` would split the lock across two inodes).
pub fn reclaim_orphan_cache_entry(repo_root: &Path, path: &Path) -> SweepDisposition {
    let Some(_lock) = ReusableWorktreeLock::try_acquire(path, GC_LOCK_CONTEXT) else {
        return SweepDisposition::SkippedLocked;
    };
    // Re-check under the lock: a concurrent `reuse_or_create` rebuild may
    // have recreated the directory between the existence check and the lock.
    if path.exists() {
        return SweepDisposition::KeptRecreated;
    }
    match remove_cache_entry_for_sweep(repo_root, path) {
        Ok(CacheRemovalOutcome::Removed) => SweepDisposition::ReclaimedOrphan,
        Ok(CacheRemovalOutcome::NothingLeft) => SweepDisposition::KeptLockOnly,
        Ok(CacheRemovalOutcome::NotOwned) => SweepDisposition::KeptNotOwned,
        Err(_) => SweepDisposition::RemoveFailed,
    }
}

/// Reclaim a cache entry whose `.last-used` sidecar is older than `max_age`.
/// Seeds a fresh owner-recording sidecar for pre-upgrade entries that lack
/// one (grace-kept so they age from real last-use on the next run).
/// Removal is directory-and-sidecars only: the entry is unregistered, so no
/// `git` subprocess is involved.
fn reclaim_aged_cache_entry(
    repo_root: &Path,
    path: &Path,
    max_age: Duration,
    now: SystemTime,
) -> SweepDisposition {
    let Some(mtime) = last_used_mtime(path) else {
        record_last_used(path, repo_root);
        return SweepDisposition::KeptGraceSeeded;
    };
    remove_entry_past_max_age(repo_root, path, max_age, now, mtime)
}

/// Reclaim a cache entry that belongs to a DIFFERENT repo hash than the
/// sweeping repo (issue #2169).
///
/// An entry whose recorded owner root still exists on disk is skipped
/// unconditionally: that repo's own sweep governs it, so its
/// `cacheMaxAgeDays` policy (including `0` = never) cannot be defeated from
/// the outside. Entries with a dead owner, or with no recorded owner (pre-
/// upgrade or externally created), are abandoned: nothing will ever sweep
/// them under their own hash again, so they age out under THIS run's
/// threshold, and a probed-dead owner is reclaimed outright when that
/// threshold is switched off (`cacheMaxAgeDays = 0`), which is the documented
/// contract of `0` and the only thing that keeps abandoned entries from
/// accumulating forever. The grace seed for a missing sidecar stays
/// mtime-only because the true owner is unknown here.
fn reclaim_foreign_cache_entry(
    repo_root: &Path,
    path: &Path,
    max_age: Option<Duration>,
    now: SystemTime,
) -> SweepDisposition {
    // Like the owned pass, a probe error on the entry itself fails toward a
    // cheap self-rebuild and the orphan branch re-checks under the lock.
    if !path.exists() {
        return reclaim_orphan_cache_entry(repo_root, path);
    }
    let mut owner_missing = false;
    if let Some(owner) = read_last_used_owner(path) {
        match probe_owner_liveness(&owner) {
            OwnerLiveness::Live => return SweepDisposition::KeptOwnerLive,
            OwnerLiveness::Unverifiable(kind) => {
                return SweepDisposition::KeptOwnerUnverifiable(kind);
            }
            OwnerLiveness::Dead => owner_missing = true,
        }
    }
    let Some(max_age) = max_age else {
        if owner_missing {
            return remove_cache_entry_under_lock(
                repo_root,
                path,
                SweepDisposition::ReclaimedOwnerMissing,
            );
        }
        return SweepDisposition::KeptAgeGcDisabled;
    };
    let Some(mtime) = last_used_mtime(path) else {
        touch_last_used(path);
        return SweepDisposition::KeptGraceSeeded;
    };
    remove_entry_past_max_age(repo_root, path, max_age, now, mtime)
}

enum OwnerLiveness {
    Live,
    Dead,
    Unverifiable(std::io::ErrorKind),
}

/// NotFound-only deadness probe for a recorded owner root (issue #2221).
///
/// `std::fs::metadata` follows symlinks, so a dangling-symlink owner root
/// resolves to NotFound (dead), matching the previous `Path::exists()`
/// behavior. Every other error (EACCES, EIO, ENOTDIR) classifies the owner as
/// unverifiable and keeps the entry: a transient probe failure must never let
/// this repo's sweep reclaim another live repo's warm cache or defeat its
/// `cacheMaxAgeDays: 0` policy. A path below an unmounted mountpoint still
/// reads as NotFound, so that case is NOT protected.
fn probe_owner_liveness(owner: &Path) -> OwnerLiveness {
    match std::fs::metadata(owner) {
        Ok(_) => OwnerLiveness::Live,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => OwnerLiveness::Dead,
        Err(error) => OwnerLiveness::Unverifiable(error.kind()),
    }
}

/// Whether the owner-liveness gate applies to a dry-run classification (it
/// only ever applies to the cross-repo pass).
#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnerGate {
    On,
    Off,
}

/// Read-only disposition classifier for [`SweepMode::DryRun`]. Mirrors the
/// decision order of [`reclaim_reusable_cache_entry`] /
/// [`reclaim_foreign_cache_entry`] exactly, minus locks and mutation, so a
/// preview predicts what an apply run would do (modulo concurrent runs, the
/// same caveat `audit-cache remove --dry-run` accepts).
fn classify_entry_dry_run(
    path: &Path,
    max_age: Option<Duration>,
    now: SystemTime,
    owner_gate: OwnerGate,
) -> SweepDisposition {
    if !path.exists() {
        if !reusable_cache_entry_exists(path) {
            return SweepDisposition::KeptLockOnly;
        }
        return classify_would_remove(path, SweepDisposition::ReclaimedOrphan);
    }
    let mut owner_missing = false;
    if owner_gate == OwnerGate::On
        && let Some(owner) = read_last_used_owner(path)
    {
        match probe_owner_liveness(&owner) {
            OwnerLiveness::Live => return SweepDisposition::KeptOwnerLive,
            OwnerLiveness::Unverifiable(kind) => {
                return SweepDisposition::KeptOwnerUnverifiable(kind);
            }
            OwnerLiveness::Dead => owner_missing = true,
        }
    }
    let Some(max_age) = max_age else {
        if owner_missing {
            return classify_would_remove(path, SweepDisposition::ReclaimedOwnerMissing);
        }
        return SweepDisposition::KeptAgeGcDisabled;
    };
    let Some(mtime) = last_used_mtime(path) else {
        return SweepDisposition::KeptGraceSeeded;
    };
    let Ok(age) = now.duration_since(mtime) else {
        return SweepDisposition::KeptFresh;
    };
    if age < max_age {
        return SweepDisposition::KeptFresh;
    }
    classify_would_remove(path, SweepDisposition::ReclaimedAged)
}

/// Apply the same up-front ownership classification to a dry-run
/// "would remove" verdict that [`remove_cache_entry_for_sweep`] applies to a
/// real removal.
fn classify_would_remove(path: &Path, removed: SweepDisposition) -> SweepDisposition {
    match cache_entry_ownership(path) {
        Ok(CacheEntryOwnership::Owned) => removed,
        Ok(CacheEntryOwnership::Unowned(_)) => SweepDisposition::KeptNotOwned,
        Err(_) => SweepDisposition::RemoveFailed,
    }
}

fn last_used_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(reusable_worktree_last_used_path(path))
        .ok()
        .and_then(|metadata| metadata.modified().ok())
}

fn remove_entry_past_max_age(
    repo_root: &Path,
    path: &Path,
    max_age: Duration,
    now: SystemTime,
    mtime: SystemTime,
) -> SweepDisposition {
    let Ok(age) = now.duration_since(mtime) else {
        return SweepDisposition::KeptFresh;
    };
    if age < max_age {
        return SweepDisposition::KeptFresh;
    }
    remove_cache_entry_under_lock(repo_root, path, SweepDisposition::ReclaimedAged)
}

/// Lock, remove, and map the outcome, reporting `removed` on success.
fn remove_cache_entry_under_lock(
    repo_root: &Path,
    path: &Path,
    removed: SweepDisposition,
) -> SweepDisposition {
    let Some(_lock) = ReusableWorktreeLock::try_acquire(path, GC_LOCK_CONTEXT) else {
        return SweepDisposition::SkippedLocked;
    };
    match remove_cache_entry_for_sweep(repo_root, path) {
        Ok(CacheRemovalOutcome::Removed) => removed,
        Ok(CacheRemovalOutcome::NothingLeft) => SweepDisposition::KeptLockOnly,
        Ok(CacheRemovalOutcome::NotOwned) => SweepDisposition::KeptNotOwned,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "failed to remove stale reusable audit worktree entry; entry may leak",
            );
            SweepDisposition::RemoveFailed
        }
    }
}

enum CacheRemovalOutcome {
    Removed,
    /// Nothing but the (never-deleted) `.lock` sidecar was left to remove.
    NothingLeft,
    /// The Unix uid ownership check rejected the entry; nothing was touched.
    NotOwned,
}

/// Sweep-side removal wrapper: classifies unowned entries up front (a shared
/// runner's other-user entries are a kept outcome, not a failure) and reserves
/// `Err` for genuine IO errors after the ownership check passed.
fn remove_cache_entry_for_sweep(
    repo_root: &Path,
    path: &Path,
) -> std::io::Result<CacheRemovalOutcome> {
    if matches!(
        cache_entry_ownership(path)?,
        CacheEntryOwnership::Unowned(_)
    ) {
        return Ok(CacheRemovalOutcome::NotOwned);
    }
    remove_reusable_cache_entry_locked(repo_root, path).map(|removed| {
        if removed {
            CacheRemovalOutcome::Removed
        } else {
            CacheRemovalOutcome::NothingLeft
        }
    })
}

/// Recursive byte count of the regular files under `root`.
///
/// A deliberate plain `read_dir` walk: an `ignore`-crate walk would honor the
/// `.gitignore` checked out INSIDE the cache snapshot and skip `node_modules`,
/// the bulk of the mass being measured. Symlinks are never followed
/// (`DirEntry::metadata` does not traverse them), so pnpm stores and
/// cross-volume links cannot inflate or loop the walk; per-entry errors are
/// tolerated. Returns `None` when `root` itself is absent or unreadable.
fn directory_size_bytes(root: &Path) -> Option<u64> {
    let root_entries = std::fs::read_dir(root).ok()?;
    let mut total: u64 = 0;
    let mut stack = vec![root_entries];
    while let Some(entries) = stack.pop() {
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                if let Ok(child) = std::fs::read_dir(entry.path()) {
                    stack.push(child);
                }
            } else if metadata_is_regular_file(&metadata) {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Some(total)
}

/// Measure candidate entry sizes in parallel on the already-configured rayon
/// pool. Returns an empty map under [`SweepSizes::Skip`].
fn measure_entry_sizes<'a>(
    sizes: SweepSizes,
    paths: impl Iterator<Item = &'a PathBuf>,
) -> FxHashMap<PathBuf, Option<u64>> {
    use rayon::iter::{IntoParallelRefIterator as _, ParallelIterator as _};

    if sizes == SweepSizes::Skip {
        return FxHashMap::default();
    }
    let paths: Vec<&PathBuf> = paths.collect();
    paths
        .par_iter()
        .map(|path| ((*path).clone(), directory_size_bytes(path)))
        .collect()
}

pub fn canonical_root_hash(root: &Path) -> u64 {
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    xxh3_64(&path_identity_bytes(&canonical_root))
}

#[cfg(unix)]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

pub fn reusable_audit_worktree_path(requested_root: &Path) -> PathBuf {
    let root_hash = canonical_root_hash(requested_root);
    let repo_hash = git_toplevel(requested_root)
        .as_deref()
        .map_or(root_hash, canonical_root_hash);
    std::env::temp_dir().join(format!(
        "fallow-audit-base-cache-{repo_hash:016x}-root-{root_hash:016x}"
    ))
}

fn root_owned_cache_repo_prefix(requested_root: &Path) -> Option<String> {
    let git_root = git_toplevel(requested_root)?;
    let repo_hash = canonical_root_hash(&git_root);
    Some(format!("fallow-audit-base-cache-{repo_hash:016x}-root-"))
}

fn legacy_reusable_cache_repo_prefix(requested_root: &Path) -> Option<String> {
    let git_root = git_toplevel(requested_root)?;
    let repo_hash = canonical_root_hash(&git_root);
    Some(format!("fallow-audit-base-cache-{repo_hash:016x}-"))
}

#[cfg(test)]
pub fn legacy_reusable_audit_worktree_path(
    requested_root: &Path,
    base_sha: &str,
) -> Option<PathBuf> {
    let sha_prefix = base_sha.get(..16).unwrap_or(base_sha);
    Some(std::env::temp_dir().join(format!(
        "{}{sha_prefix}",
        legacy_reusable_cache_repo_prefix(requested_root)?
    )))
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
    if !reusable_cache_directory_is_trusted(path) {
        return false;
    }
    let recorded = read_reusable_sha(path);
    if recorded.as_deref() != Some(base_sha) {
        return false;
    }
    repair_unregistered_git_stub(path)
}

fn read_reusable_sha(path: &Path) -> Option<String> {
    const MAX_SHA_SIDECAR_BYTES: u64 = 129;

    let sidecar = reusable_worktree_sha_path(path);
    let metadata = std::fs::symlink_metadata(&sidecar).ok()?;
    if !metadata_is_regular_file(&metadata) || metadata.len() > MAX_SHA_SIDECAR_BYTES {
        return None;
    }
    let mut contents = String::new();
    std::fs::File::open(sidecar)
        .ok()?
        .take(MAX_SHA_SIDECAR_BYTES)
        .read_to_string(&mut contents)
        .ok()?;
    Some(contents.trim().to_owned())
}

#[cfg(unix)]
fn reusable_cache_directory_is_trusted(path: &Path) -> bool {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    metadata.file_type().is_dir()
        && metadata.uid() == rustix::process::geteuid().as_raw()
        && metadata.permissions().mode().trailing_zeros() >= 6
}

#[cfg(not(unix))]
fn reusable_cache_directory_is_trusted(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

/// Recover a current-path cache that is still a registered Git worktree.
///
/// This can occur during mixed-version use or after interruption between
/// registration and deregistration. Keep the cache warm only when HEAD matches
/// the requested full SHA and raw materialization completed.
fn try_migrate_registered_current_cache(repo_root: &Path, path: &Path, base_sha: &str) -> bool {
    if !path.exists() || !audit_worktree_is_registered(repo_root, path) {
        return false;
    }
    let head_matches = git_rev_parse(path, "HEAD").is_some_and(|head| head == base_sha);
    if !head_matches || !fallow_engine::repo_refs::detached_base_worktree_is_raw_materialized(path)
    {
        return false;
    }
    if unregister_worktree_checked(repo_root, path).is_err() {
        return false;
    }
    write_reusable_sha(path, base_sha).is_ok()
}

/// Remove every reusable base-snapshot cache owned by `requested_root`.
///
/// The root-owned entry and every SHA-suffixed entry from the old
/// git-top-level identity are locked independently. Contended entries are
/// reported as skipped. Lock files are permanent lock identities and are never
/// removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditCacheRemovalReport {
    pub found: usize,
    pub removed: usize,
    pub skipped: usize,
    pub dry_run: bool,
}

pub fn remove_reusable_audit_caches(
    requested_root: &Path,
    dry_run: bool,
) -> std::io::Result<AuditCacheRemovalReport> {
    let mut paths = vec![reusable_audit_worktree_path(requested_root)];
    paths.extend(scan_legacy_reusable_cache_paths(
        requested_root,
        &std::env::temp_dir(),
    ));
    if git_toplevel(requested_root).is_some_and(|root| paths_equal(&root, requested_root)) {
        paths.extend(scan_root_owned_cache_paths(requested_root));
    }
    paths.sort();
    paths.dedup();

    let mut report = AuditCacheRemovalReport {
        found: 0,
        removed: 0,
        skipped: 0,
        dry_run,
    };
    for path in paths {
        if !reusable_cache_entry_exists(&path) {
            continue;
        }
        report.found += 1;
        if dry_run {
            // A preview must not touch the filesystem. Acquiring the lock would
            // create the `.lock` sidecar (create_new) for entries that lack one,
            // violating the documented --dry-run contract. Contention is a race
            // only the real removal reports.
            continue;
        }
        let Some(_lock) =
            ReusableWorktreeLock::try_acquire(&path, "cache removal reports the entry as skipped")
        else {
            report.skipped += 1;
            continue;
        };
        if remove_reusable_cache_entry_locked(requested_root, &path)? {
            report.removed += 1;
        }
    }
    Ok(report)
}

fn reusable_cache_entry_exists(path: &Path) -> bool {
    path_entry_exists(path)
        || path_entry_exists(&reusable_worktree_sha_path(path))
        || path_entry_exists(&reusable_worktree_last_used_path(path))
}

/// Like [`reusable_cache_entry_exists`] but also counts a surviving `.lock`
/// sidecar, which is deliberately never deleted and marks a real (if fully
/// reclaimed) cache identity worth reporting.
fn cache_entry_has_presence(path: &Path) -> bool {
    reusable_cache_entry_exists(path) || path_entry_exists(&reusable_worktree_lock_path(path))
}

fn path_entry_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Remove one reusable cache while its caller holds the entry's exclusive
/// lock. Absence is success. The lock sidecar is deliberately preserved.
fn remove_reusable_cache_entry_locked(repo_root: &Path, path: &Path) -> std::io::Result<bool> {
    let existed = reusable_cache_entry_exists(path);
    ensure_cache_entry_is_owned(path)?;
    if trusted_worktree_admin_dir(repo_root, path).is_some() {
        unregister_worktree_checked(repo_root, path)?;
    }
    remove_dir_if_exists(path)?;
    remove_file_if_exists(&reusable_worktree_sha_path(path))?;
    remove_file_if_exists(&reusable_worktree_last_used_path(path))?;
    Ok(existed)
}

enum CacheEntryOwnership {
    Owned,
    #[cfg_attr(
        not(unix),
        expect(dead_code, reason = "only the Unix uid probe constructs this variant")
    )]
    Unowned(PathBuf),
}

#[cfg(unix)]
fn cache_entry_ownership(path: &Path) -> std::io::Result<CacheEntryOwnership> {
    use std::os::unix::fs::MetadataExt as _;

    let effective_uid = rustix::process::geteuid().as_raw();
    for entry in [
        path.to_path_buf(),
        reusable_worktree_sha_path(path),
        reusable_worktree_last_used_path(path),
    ] {
        let metadata = match std::fs::symlink_metadata(&entry) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if metadata.uid() != effective_uid {
            return Ok(CacheEntryOwnership::Unowned(entry));
        }
    }
    Ok(CacheEntryOwnership::Owned)
}

#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "shared cross-platform signature; the Unix ownership check is fallible, non-Unix has no POSIX owner to verify"
)]
fn cache_entry_ownership(_path: &Path) -> std::io::Result<CacheEntryOwnership> {
    Ok(CacheEntryOwnership::Owned)
}

fn ensure_cache_entry_is_owned(path: &Path) -> std::io::Result<()> {
    match cache_entry_ownership(path)? {
        CacheEntryOwnership::Owned => Ok(()),
        CacheEntryOwnership::Unowned(entry) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "refusing to remove unowned audit cache entry `{}`",
                entry.display()
            ),
        )),
    }
}

fn remove_dir_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn remove_file_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
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
pub fn unregister_worktree(repo_root: &Path, path: &Path) -> std::io::Result<()> {
    unregister_worktree_checked(repo_root, path)
}

fn unregister_worktree_checked(repo_root: &Path, path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let gitfile = path.join(".git");
    let metadata = std::fs::symlink_metadata(&gitfile)?;
    if !metadata_is_regular_file(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "refusing to deregister through a non-file audit worktree .git entry",
        ));
    }
    let contents = std::fs::read_to_string(&gitfile)?;
    if contents == UNREGISTERED_GITDIR_STUB {
        return Ok(());
    }
    let Some(admin_dir) = trusted_worktree_admin_dir(repo_root, path) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "refusing to deregister an unverified audit worktree admin entry",
        ));
    };
    remove_dir_if_exists(&admin_dir)?;
    write_git_stub_safely(&gitfile)
}

/// Ensure a cache hit's `.git` stub stays valid. Recreates the stub if the
/// gitfile is gone (so gitignore parity holds), and deregisters in place if a
/// live pointer to a fallow admin dir is found (a mixed-version re-registration
/// or a torn transient window).
fn repair_unregistered_git_stub(path: &Path) -> bool {
    let gitfile = path.join(".git");
    let Ok(metadata) = std::fs::symlink_metadata(&gitfile) else {
        return write_git_stub_safely(&gitfile).is_ok();
    };
    if !metadata_is_regular_file(&metadata) {
        return false;
    }
    std::fs::read_to_string(&gitfile).is_ok_and(|contents| contents == UNREGISTERED_GITDIR_STUB)
}

fn write_git_stub_safely(gitfile: &Path) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).truncate(true);
    match std::fs::symlink_metadata(gitfile) {
        Ok(metadata) if metadata_is_regular_file(&metadata) => {}
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "refusing to replace non-file audit worktree .git entry",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            options.create_new(true);
        }
        Err(error) => return Err(error),
    }
    let mut file = options.open(gitfile)?;
    file.write_all(UNREGISTERED_GITDIR_STUB.as_bytes())?;
    file.sync_all()
}

fn trusted_worktree_admin_dir(repo_root: &Path, path: &Path) -> Option<PathBuf> {
    let gitfile = path.join(".git");
    let metadata = std::fs::symlink_metadata(&gitfile).ok()?;
    if !metadata_is_regular_file(&metadata) {
        return None;
    }
    let contents = std::fs::read_to_string(&gitfile).ok()?;
    let admin_dir = parse_worktree_gitdir(&contents)?;
    if !is_fallow_admin_dir(&admin_dir) {
        return None;
    }
    let common_dir = fallow_engine::changed_files::resolve_git_common_dir(repo_root).ok()?;
    let worktrees_dir = dunce::canonicalize(common_dir.join("worktrees")).ok()?;
    let admin_parent = dunce::canonicalize(admin_dir.parent()?).ok()?;
    if admin_parent != worktrees_dir {
        return None;
    }
    let backlink = std::fs::read_to_string(admin_dir.join("gitdir")).ok()?;
    let expected_gitfile = dunce::canonicalize(&gitfile).ok()?;
    let actual_gitfile = dunce::canonicalize(Path::new(backlink.trim())).ok()?;
    (actual_gitfile == expected_gitfile).then_some(admin_dir)
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

    #[test]
    fn directory_size_bytes_sums_regular_files_recursively() {
        let temp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = temp.path().join("cache");
        std::fs::create_dir_all(root.join("node_modules/dep")).expect("tree should be created");
        std::fs::write(root.join("a.txt"), vec![0u8; 10]).expect("file should be written");
        std::fs::write(root.join("node_modules/dep/b.js"), vec![0u8; 32])
            .expect("nested file should be written");
        // A gitignore inside the snapshot must NOT hide the mass being
        // measured (the walk is deliberately ignore-crate-free).
        std::fs::write(root.join(".gitignore"), "node_modules\n")
            .expect("gitignore should be written");

        let size = directory_size_bytes(&root).expect("size walk should succeed");
        assert_eq!(size, 10 + 32 + "node_modules\n".len() as u64);
        assert_eq!(
            directory_size_bytes(&temp.path().join("missing")),
            None,
            "an absent directory reports no size",
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_size_bytes_never_follows_symlinks() {
        let temp = tempfile::TempDir::new().expect("temp dir should be created");
        let root = temp.path().join("cache");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).expect("root should be created");
        std::fs::create_dir_all(&outside).expect("outside dir should be created");
        std::fs::write(outside.join("big.bin"), vec![0u8; 4096])
            .expect("outside file should be written");
        std::os::unix::fs::symlink(&outside, root.join("link-dir"))
            .expect("dir symlink should be created");
        std::os::unix::fs::symlink(outside.join("big.bin"), root.join("link-file"))
            .expect("file symlink should be created");

        assert_eq!(
            directory_size_bytes(&root),
            Some(0),
            "symlinked directories and files must not be traversed or counted",
        );
    }

    #[cfg(unix)]
    #[test]
    fn cache_sidecar_open_does_not_follow_symlinks() {
        let temp = tempfile::TempDir::new().expect("temp dir should be created");
        let victim = temp.path().join("victim");
        let sidecar = temp.path().join("cache.lock");
        std::fs::write(&victim, "unchanged\n").expect("victim should be written");
        std::os::unix::fs::symlink(&victim, &sidecar).expect("sidecar symlink should be created");

        assert!(open_or_create_owned_sidecar(&sidecar).is_err());
        assert_eq!(
            std::fs::read_to_string(victim).expect("victim should remain readable"),
            "unchanged\n",
        );
    }
}
