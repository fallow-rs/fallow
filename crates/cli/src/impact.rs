//! Fallow Impact: local, opt-in value reporting.

use std::path::{Path, PathBuf};

use fallow_types::envelope::Meta;
use fallow_types::results::{ActiveSuppression, AnalysisResults};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use crate::audit::{AuditSummary, AuditVerdict};
use crate::report::ci::fingerprint::fingerprint_hash;
use crate::report::format_display_path;

const STORE_SCHEMA_VERSION: u32 = 5;

const MAX_RECORDS: usize = 200;

const MAX_CONTAINMENT: usize = 200;

const TREND_TOLERANCE: i64 = 0;

const STORE_FILE: &str = "impact.json";

/// Env var: when set to a positive integer N, a recorded run opportunistically
/// removes per-project store files whose file mtime is older than N days,
/// reclaiming stores left behind by deleted repos. Unset / `0` / unparseable
/// disables the sweep (default: keep every store forever).
const STORE_MAX_AGE_ENV: &str = "FALLOW_IMPACT_STORE_MAX_AGE_DAYS";

const MAX_RECENT_RESOLVED: usize = 50;

const ID_SEP: &str = "\u{1f}";

const CODE_DUPLICATION_KIND: &str = "code-duplication";

const BLANKET_SUPPRESSION: &str = "*";

/// Per-category issue counts captured at a recorded run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImpactCounts {
    pub total_issues: usize,
    pub dead_code: usize,
    pub complexity: usize,
    pub duplication: usize,
}

impl ImpactCounts {
    fn from_summary(summary: &AuditSummary) -> Self {
        Self {
            total_issues: summary.dead_code_issues
                + summary.complexity_findings
                + summary.duplication_clone_groups,
            dead_code: summary.dead_code_issues,
            complexity: summary.complexity_findings,
            duplication: summary.duplication_clone_groups,
        }
    }

    pub(crate) fn from_combined(dead_code: usize, complexity: usize, duplication: usize) -> Self {
        Self {
            total_issues: dead_code + complexity + duplication,
            dead_code,
            complexity,
            duplication,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactRecord {
    pub timestamp: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    pub verdict: String,
    #[serde(default)]
    pub gate: bool,
    pub counts: ImpactCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingContainment {
    pub blocked_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    pub blocked_counts: ImpactCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContainmentEvent {
    pub blocked_at: String,
    pub cleared_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    pub blocked_counts: ImpactCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierFinding {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

impl FrontierFinding {
    fn move_key(&self) -> String {
        match &self.symbol {
            Some(symbol) => format!("{}{ID_SEP}{symbol}", self.kind),
            None => self.id.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileFrontier {
    #[serde(default)]
    pub findings: Vec<FrontierFinding>,
    #[serde(default)]
    pub suppressions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ResolutionEvent {
    pub kind: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImpactStore {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_recorded: Option<String>,
    #[serde(default)]
    pub records: Vec<ImpactRecord>,
    #[serde(default)]
    pub project_records: Vec<ImpactRecord>,
    #[serde(default)]
    pub containment: Vec<ContainmentEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_containment: Option<PendingContainment>,
    /// Per-finding attribution baseline, namespaced by worktree key (schema v4)
    /// so two worktrees of one repo (collapsed to a single store) do not prune
    /// each other's per-file frontier. Inner map is rel-path -> findings.
    #[serde(default)]
    pub frontier: FxHashMap<String, FxHashMap<String, FileFrontier>>,
    /// Clone-family attribution baseline, namespaced by worktree key (schema
    /// v4). Inner map is clone fingerprint -> instance paths.
    #[serde(default)]
    pub clone_frontier: FxHashMap<String, FxHashMap<String, Vec<String>>>,
    #[serde(default)]
    pub resolved_total: usize,
    #[serde(default)]
    pub suppressed_total: usize,
    #[serde(default)]
    pub recent_resolved: Vec<ResolutionEvent>,
    #[serde(default)]
    pub onboarding_declined: bool,
    /// Whether the user ever ran an explicit `impact enable` or `impact
    /// disable`. Distinguishes "deliberately declined" from "never asked" so
    /// the agent skill asks for the impact opt-in exactly once per project.
    #[serde(default)]
    pub explicit_decision: bool,
    /// Unix epoch seconds when the periodic impact digest was last surfaced
    /// (the `impact-report` next-step / human one-liner). Internal cadence
    /// state, never exposed on the report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_digest_epoch: Option<u64>,
    /// Repo display name (the git-toplevel BASENAME only, never a full path),
    /// captured at record time so the cross-repo `fallow impact --all` view can
    /// label rows legibly without reversing the opaque project-key hash. Absent
    /// on pre-v5 stores (rows fall back to the short key) and on stores written
    /// by older builds. v5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Deserialize-only view of a pre-relocation in-repo store (schema <= 3), whose
/// `frontier` / `clone_frontier` were FLAT (not worktree-namespaced). Used once
/// during migration to import a legacy `.fallow/impact.json` into the user
/// store. Every field carries `#[serde(default)]` so any of v1/v2/v3 reads.
#[derive(Debug, Default, Deserialize)]
struct LegacyFlatStore {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    first_recorded: Option<String>,
    #[serde(default)]
    records: Vec<ImpactRecord>,
    #[serde(default)]
    project_records: Vec<ImpactRecord>,
    #[serde(default)]
    containment: Vec<ContainmentEvent>,
    #[serde(default)]
    pending_containment: Option<PendingContainment>,
    #[serde(default)]
    frontier: FlatFrontier,
    #[serde(default)]
    clone_frontier: FlatCloneFrontier,
    #[serde(default)]
    resolved_total: usize,
    #[serde(default)]
    suppressed_total: usize,
    #[serde(default)]
    recent_resolved: Vec<ResolutionEvent>,
    #[serde(default)]
    onboarding_declined: bool,
    #[serde(default)]
    explicit_decision: bool,
    #[serde(default)]
    last_digest_epoch: Option<u64>,
}

impl LegacyFlatStore {
    /// Convert into the current (v4) store, wrapping the flat frontier under the
    /// importing worktree's key.
    fn into_store(self, worktree_key: &str) -> ImpactStore {
        let mut frontier: FxHashMap<String, FlatFrontier> = FxHashMap::default();
        if !self.frontier.is_empty() {
            frontier.insert(worktree_key.to_owned(), self.frontier);
        }
        let mut clone_frontier: FxHashMap<String, FlatCloneFrontier> = FxHashMap::default();
        if !self.clone_frontier.is_empty() {
            clone_frontier.insert(worktree_key.to_owned(), self.clone_frontier);
        }
        ImpactStore {
            schema_version: STORE_SCHEMA_VERSION,
            enabled: self.enabled,
            first_recorded: self.first_recorded,
            records: self.records,
            project_records: self.project_records,
            containment: self.containment,
            pending_containment: self.pending_containment,
            frontier,
            clone_frontier,
            resolved_total: self.resolved_total,
            suppressed_total: self.suppressed_total,
            recent_resolved: self.recent_resolved,
            onboarding_declined: self.onboarding_declined,
            explicit_decision: self.explicit_decision,
            last_digest_epoch: self.last_digest_epoch,
            // Legacy stores carry no label; the next recorded run backfills it.
            label: None,
        }
    }
}

/// Process-global memo of `(project_key, worktree_key)` per analyzed root, so
/// the git subprocesses that derive them run at most once per root per run
/// (`fallow audit` is the perf-priority path and `load` is called several
/// times per invocation).
/// `(project_key, worktree_key, display_name)` for a root.
type ProjectIdentity = (String, String, Option<String>);

static IDENTITY_CACHE: std::sync::OnceLock<std::sync::Mutex<FxHashMap<PathBuf, ProjectIdentity>>> =
    std::sync::OnceLock::new();

/// Hash a filesystem-path identity into a stable key. On case-insensitive
/// filesystems (macOS APFS default, Windows) two spellings of one directory map
/// to the same on-disk location, so fold case before hashing to keep the key
/// stable across spellings. Linux paths are case-sensitive and left as-is.
fn hash_path_identity(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let normalized = if cfg!(any(target_os = "macos", target_os = "windows")) {
        raw.to_lowercase()
    } else {
        raw.into_owned()
    };
    fingerprint_hash(&[normalized.as_str()])
}

/// Resolve `resolved` to an existing absolute path, falling back to the
/// canonicalized `root` and finally the raw `root`. Shared by the project key
/// (git common dir) and the worktree key (git toplevel) so both keep identical
/// non-git fallback behavior.
fn resolve_or_root(resolved: Option<PathBuf>, root: &Path) -> PathBuf {
    resolved
        .or_else(|| dunce::canonicalize(root).ok())
        .unwrap_or_else(|| root.to_path_buf())
}

/// The repo's display name for cross-repo rows: the folder that owns the shared
/// `.git` (stable across worktrees), else the directory's own basename. This is
/// a BASENAME only (e.g. `fallow`), never a full path, so persisting it does not
/// reintroduce the absolute path the store relocation deliberately dropped.
fn repo_basename(common_or_dir: &Path) -> Option<String> {
    let dir = if common_or_dir.file_name().is_some_and(|n| n == ".git") {
        common_or_dir.parent()?
    } else {
        common_or_dir
    };
    dir.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Resolve (and memoize) the `(project_key, worktree_key, display_name)` for
/// `root`. `project_key` collapses all worktrees of a repo onto one identity via
/// the git common dir (falling back to the canonicalized root for non-git);
/// `worktree_key` is the per-tree toplevel (namespaces the attribution frontier
/// so concurrent worktrees do not prune each other's baseline); `display_name`
/// is the repo's basename for legible cross-repo rows. All three derive from a
/// single common-dir + toplevel resolution so the git subprocesses run at most
/// once per root per run (`fallow audit` is the perf-priority path).
fn project_identity(root: &Path) -> ProjectIdentity {
    let cache = IDENTITY_CACHE.get_or_init(|| std::sync::Mutex::new(FxHashMap::default()));
    if let Ok(map) = cache.lock()
        && let Some(found) = map.get(root)
    {
        return found.clone();
    }
    let common = resolve_or_root(
        fallow_core::changed_files::resolve_git_common_dir(root).ok(),
        root,
    );
    let toplevel = resolve_or_root(
        fallow_core::changed_files::resolve_git_toplevel(root).ok(),
        root,
    );
    let identity = (
        hash_path_identity(&common),
        hash_path_identity(&toplevel),
        repo_basename(&common),
    );
    if let Ok(mut map) = cache.lock() {
        map.insert(root.to_path_buf(), identity.clone());
    }
    identity
}

#[cfg(test)]
thread_local! {
    /// Per-test override of the user config dir, so parallel tests get isolated
    /// stores (the real config dir is process-global and would collide). Set via
    /// [`with_test_config_dir`]; unset = fall back to the real config dir.
    static TEST_CONFIG_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };

    /// Per-test CI signal for the record gate. Defaults to `false` so the unit
    /// tests record into their isolated store EVEN when the suite itself runs on
    /// CI (GitHub Actions sets `CI` / `GITHUB_ACTIONS`, which `telemetry::is_ci`
    /// reads); without this, every record-dependent test fails on CI because the
    /// real `is_ci()` short-circuits `record_*` before any store write. A test
    /// can flip it true to exercise the CI no-op gate. Thread-local, so it is
    /// parallel-safe and needs no unsafe env mutation.
    static TEST_FORCE_CI: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Fallow's per-user config dir. Under test it resolves ONLY the per-test
/// override (or `None` when unset), so a test never reads or writes the real
/// developer config dir and parallel tests stay isolated.
fn impact_config_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        TEST_CONFIG_DIR.with(|c| c.borrow().clone())
    }
    #[cfg(not(test))]
    {
        crate::telemetry::config_dir()
    }
}

/// Whether this run should be treated as CI for the Impact record gate. In
/// production it is `telemetry::is_ci()`; under test it reads the per-test
/// `TEST_FORCE_CI` override (default `false`) so the suite records into its
/// isolated store regardless of the ambient CI env. The store path is ALWAYS
/// the per-test temp dir under `#[cfg(test)]` (see [`impact_config_dir`]), so
/// bypassing the CI gate in tests can never touch a real user store.
fn record_gate_is_ci() -> bool {
    #[cfg(test)]
    {
        TEST_FORCE_CI.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        crate::telemetry::is_ci()
    }
}

/// Path to the per-project store file in the user's private config dir, or
/// `None` when no config dir is resolvable (e.g. stripped CI env), in which
/// case Impact is inert (no persistence). NEVER writes into the analyzed repo.
fn store_path(root: &Path) -> Option<PathBuf> {
    let (project_key, _, _) = project_identity(root);
    Some(
        impact_config_dir()?
            .join("impact")
            .join(format!("{project_key}.json")),
    )
}

/// Path to a project's legacy in-repo store (`<root>/.fallow/impact.json`),
/// read ONCE for migration into the user store, never written.
fn legacy_store_path(root: &Path) -> PathBuf {
    root.join(".fallow").join(STORE_FILE)
}

/// Load the store. Missing or unreadable files fall back to defaults; unreadable
/// files are warned about rather than silently disabling tracking.
pub fn load(root: &Path) -> ImpactStore {
    let Some(path) = store_path(root) else {
        return ImpactStore::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => parse_store(&content, &path),
        // No user-store file yet: attempt a one-time import of a legacy in-repo
        // `.fallow/impact.json` (pre-relocation). Returns default if none.
        Err(_) => migrate_legacy_store(root),
    }
}

fn parse_store(content: &str, path: &Path) -> ImpactStore {
    match serde_json::from_str::<ImpactStore>(content) {
        Ok(store) => {
            if store.schema_version > STORE_SCHEMA_VERSION {
                tracing::warn!(
                    "fallow impact: store at {} has schema_version {} but this build understands up to {}; reading it as best-effort, fields this build does not know are dropped on the next write. Upgrade fallow to read it fully.",
                    path.display(),
                    store.schema_version,
                    STORE_SCHEMA_VERSION,
                );
            }
            store
        }
        Err(err) => {
            tracing::warn!(
                "fallow impact: ignoring unreadable store at {} ({err}); run `fallow impact enable` to reset it",
                path.display()
            );
            ImpactStore::default()
        }
    }
}

/// Persist the store best-effort using atomic replace. No-op when no config dir
/// is resolvable (e.g. stripped CI env). NEVER writes into the analyzed repo.
fn save(store: &ImpactStore, root: &Path) {
    let Some(path) = store_path(root) else {
        return;
    };
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(store) {
        let _ = fallow_config::atomic_write(&path, json.as_bytes());
    }
}

/// The advisory-lock sidecar path for a store file (`<store>.json.lock`).
fn lock_path_for(store: &Path) -> PathBuf {
    let mut raw = store.as_os_str().to_owned();
    raw.push(".lock");
    PathBuf::from(raw)
}

/// Advisory lock serialising the load -> mutate -> save critical section of an
/// Impact record across concurrent `fallow` processes.
///
/// Two worktrees of the same repo collapse to the SAME store key (and SAME
/// store path), so a pre-commit gate firing in both at once would otherwise
/// lost-update each other (A loads, B loads, A saves, B saves => A's record is
/// dropped). Records BLOCK on this lock so a contended run is serialised rather
/// than dropped. The `.lock` sidecar is intentionally never deleted (an
/// unlinked-but-locked inode plus a racer's `O_CREAT` would split the lock
/// across two inodes); the kernel releases the lock when the handle drops,
/// including at process exit, so an abandoned record never wedges the next run.
struct ImpactStoreLock {
    _file: std::fs::File,
}

impl ImpactStoreLock {
    /// Block until the per-project store lock for `root` is held. Best-effort:
    /// returns `None` (proceed unlocked) when no store path resolves or the lock
    /// file cannot be opened/locked, so a lock-layer failure never drops a
    /// record (it only loses the cross-worktree serialisation guarantee).
    fn acquire(root: &Path) -> Option<Self> {
        let lock_path = lock_path_for(&store_path(root)?);
        if let Some(parent) = lock_path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return None;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .ok()?;
        match file.lock() {
            Ok(()) => Some(Self { _file: file }),
            Err(err) => {
                tracing::debug!(error = %err, "could not acquire impact store lock");
                None
            }
        }
    }
}

/// Resolve the store-GC window from [`STORE_MAX_AGE_ENV`]. `None` (no sweep)
/// when unset, `0`, or unparseable.
fn resolve_store_max_age() -> Option<std::time::Duration> {
    let raw = std::env::var(STORE_MAX_AGE_ENV).ok()?;
    let days: u32 = raw.trim().parse().ok()?;
    crate::base_worktree::days_to_duration(days)
}

/// Remove per-project store files older than `max_age`, reclaiming stores left
/// behind by deleted repos. Age is the store FILE's mtime (any recorded run
/// rewrites the file via atomic replace, refreshing the mtime), so an
/// actively-tracked repo never ages out. Best-effort and opportunistic (called
/// after a successful record), gated entirely on [`STORE_MAX_AGE_ENV`]. Never
/// deletes `.lock` sidecars (the lock-lifecycle invariant) and never the global
/// `impact.json` toggle (it is a sibling FILE one level up, not inside the
/// `impact/` dir). Skips `keep_key`'s own store so the just-written file is
/// never reclaimed by a stale-mtime race in the same run.
fn sweep_old_stores(keep_key: &str, max_age: std::time::Duration) {
    let Some(dir) = store_dir() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        // Only `<key>.json` store files; `.lock` sidecars have a `lock`
        // extension and are skipped (never deleted).
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if path.file_stem().and_then(|s| s.to_str()) == Some(keep_key) {
            continue;
        }
        let aged_out = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|mtime| now.duration_since(mtime).ok())
            .is_some_and(|age| age >= max_age);
        if aged_out {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// One-time import of a pre-relocation in-repo `.fallow/impact.json` into the
/// user store. The legacy store had a FLAT frontier (schema <= 3); this reads
/// it via [`LegacyFlatStore`] and wraps the flat frontier under the current
/// worktree key. The legacy file is left byte-for-byte untouched (it is no
/// longer read once the user store exists; re-running finds the user store and
/// does not re-import). Monorepo note: N subdir stores share one repo key, so
/// whichever subdir runs first wins (pick-first); the others are not merged.
fn migrate_legacy_store(root: &Path) -> ImpactStore {
    let legacy_path = legacy_store_path(root);
    let Ok(content) = std::fs::read_to_string(&legacy_path) else {
        return ImpactStore::default();
    };
    let Ok(legacy) = serde_json::from_str::<LegacyFlatStore>(&content) else {
        return ImpactStore::default();
    };
    let (_, worktree, display) = project_identity(root);
    let mut store = legacy.into_store(&worktree);
    // Backfill the cross-repo display label on migration so the imported repo
    // is legible in `impact --all` without waiting for its next recorded run.
    store.label = display;
    save(&store, root);
    store
}

/// Enable Impact tracking for THIS project (an explicit per-repo decision that
/// overrides the user-global default). Writes nothing into the repo: the store
/// lives in the user config dir.
pub fn enable(root: &Path) -> bool {
    let mut store = load(root);
    let was_enabled = store.enabled;
    store.enabled = true;
    store.explicit_decision = true;
    if store.schema_version == 0 {
        store.schema_version = STORE_SCHEMA_VERSION;
    }
    save(&store, root);
    !was_enabled
}

/// Disable Impact tracking. Retains existing history. Returns whether it was
/// newly disabled (false if already off). Also records the explicit decision,
/// so declining the impact opt-in on a never-enabled project (`impact
/// disable`) persists "asked and said no" for the agent skill.
pub fn disable(root: &Path) -> bool {
    let mut store = load(root);
    let was_enabled = store.enabled;
    store.enabled = false;
    store.explicit_decision = true;
    if store.schema_version == 0 {
        store.schema_version = STORE_SCHEMA_VERSION;
    }
    save(&store, root);
    was_enabled
}

/// A due periodic value digest: the headline counters for "what has fallow
/// done for you here". Returned by [`take_due_digest`] at most once per
/// `DIGEST_INTERVAL_SECS` per project.
#[derive(Debug, Clone, Copy)]
pub struct ImpactDigest {
    pub containment_count: usize,
    pub resolved_total: usize,
}

/// Minimum seconds between periodic digest surfacings (one week).
const DIGEST_INTERVAL_SECS: u64 = 7 * 24 * 60 * 60;

/// Return the periodic value digest when it is due, stamping the store so the
/// next one is at least `DIGEST_INTERVAL_SECS` away. Due means: tracking is
/// enabled, there is non-zero value to report (anti-nag: a zero digest never
/// surfaces), and the previous digest is older than the interval (or never
/// happened). Best-effort like the rest of the store: a clean run that drops
/// the emitted step simply defers the digest to the next interval.
pub fn take_due_digest(root: &Path) -> Option<ImpactDigest> {
    let mut store = load(root);
    if !resolve_enabled(&store).0 {
        return None;
    }
    let containment_count = store.containment.len();
    if containment_count == 0 && store.resolved_total == 0 {
        return None;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    if let Some(last) = store.last_digest_epoch
        && now.saturating_sub(last) < DIGEST_INTERVAL_SECS
    {
        return None;
    }
    store.last_digest_epoch = Some(now);
    save(&store, root);
    Some(ImpactDigest {
        containment_count,
        resolved_total: store.resolved_total,
    })
}

/// Persist that the local user declined the agent onboarding prompt. Writes
/// only to the user store; nothing is written into the repo.
pub fn decline_onboarding(root: &Path) -> bool {
    let mut store = load(root);
    let was_declined = store.onboarding_declined;
    store.onboarding_declined = true;
    if store.schema_version == 0 {
        store.schema_version = STORE_SCHEMA_VERSION;
    }
    save(&store, root);
    !was_declined
}

/// Why Impact tracking is (or is not) active for a project. `Project` = an
/// explicit per-repo `enable`; `User` = the user-global default with no per-repo
/// decision; `Default` = off (no per-repo decision and no global default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum EnabledSource {
    Project,
    User,
    Default,
}

/// The user-global Impact default, stored at `<config-dir>/fallow/impact.json`
/// (sibling to `telemetry.json`). A single toggle: when on, new projects record
/// without a per-repo `enable`. A per-repo explicit decision always wins.
#[derive(Debug, Default, Serialize, Deserialize)]
struct GlobalImpactConfig {
    #[serde(default)]
    default_enabled: bool,
}

fn global_config_path() -> Option<PathBuf> {
    Some(impact_config_dir()?.join(STORE_FILE))
}

/// Whether the user-global default is on. False when unset or unreadable.
fn load_global_default() -> bool {
    let Some(path) = global_config_path() else {
        return false;
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<GlobalImpactConfig>(&c).ok())
        .is_some_and(|c| c.default_enabled)
}

/// Set the user-global default. Returns whether the value changed.
pub fn set_global_default(on: bool) -> bool {
    let was = load_global_default();
    if let Some(path) = global_config_path() {
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return false;
        }
        let config = GlobalImpactConfig {
            default_enabled: on,
        };
        if let Ok(json) = serde_json::to_string_pretty(&config) {
            let _ = fallow_config::atomic_write(&path, json.as_bytes());
        }
    }
    was != on
}

/// Resolve whether Impact is active for this project and WHY. Precedence:
/// per-repo decision (enable/disable) > user-global default > off.
///
/// `enabled == true` is itself an explicit project opt-in (somebody ran
/// `enable` here), so it wins even when `explicit_decision` is unset, which is
/// the case for stores written before the `explicit_decision` field existed. A
/// store that is off-but-explicitly-decided (`!enabled && explicit_decision`)
/// stays off as a Project decision (the user disabled it here). Only a truly
/// never-asked store (`!enabled && !explicit_decision`) consults the global
/// default.
fn resolve_enabled(store: &ImpactStore) -> (bool, EnabledSource) {
    if store.enabled {
        return (true, EnabledSource::Project);
    }
    if store.explicit_decision {
        return (false, EnabledSource::Project);
    }
    if load_global_default() {
        return (true, EnabledSource::User);
    }
    (false, EnabledSource::Default)
}

/// The resolved per-project store-file path for `root`, for `status` display
/// (so a wrong key is debuggable). `None` when no config dir is resolvable.
#[must_use]
pub fn resolved_store_path(root: &Path) -> Option<PathBuf> {
    store_path(root)
}

/// The resolved (worktree-collapsed) project key for `root`, for display.
#[must_use]
pub fn resolved_project_key(root: &Path) -> String {
    project_identity(root).0
}

/// The per-project store directory (`<config-dir>/fallow/impact/`), for the
/// `impact --all` human discoverability footer. `None` when no config dir.
#[must_use]
pub fn store_dir() -> Option<PathBuf> {
    impact_config_dir().map(|d| d.join("impact"))
}

/// Delete THIS project's store file. Returns whether a file was removed.
pub fn reset(root: &Path) -> bool {
    store_path(root).is_some_and(|p| std::fs::remove_file(&p).is_ok())
}

/// Delete the whole per-project impact dir (`<config-dir>/fallow/impact/`).
/// Does NOT touch the global default toggle (`impact.json`): a data wipe should
/// not silently re-disable an opt-in the user made. Returns whether the dir was
/// present and removed.
pub fn reset_all() -> bool {
    let Some(dir) = impact_config_dir().map(|d| d.join("impact")) else {
        return false;
    };
    dir.is_dir() && std::fs::remove_dir_all(&dir).is_ok()
}

/// Record an audit run into the rolling store.
pub struct AuditRunRecord<'a> {
    pub verdict: AuditVerdict,
    pub gate: bool,
    pub git_sha: Option<&'a str>,
    pub version: &'a str,
    pub timestamp: &'a str,
    pub attribution: Option<&'a AttributionInput<'a>>,
}

pub fn record_audit_run(root: &Path, summary: &AuditSummary, record: &AuditRunRecord<'_>) {
    let AuditRunRecord {
        verdict,
        gate,
        git_sha,
        version,
        timestamp,
        attribution,
    } = record;
    // Impact is a LOCAL-DEV signal. Never record in CI: a user-global default
    // baked into a devcontainer/dotfiles image would otherwise start writing
    // per-project files on every CI run (pre-relocation this was emergent from
    // a fresh CI checkout having no in-repo store file; now it is explicit).
    if record_gate_is_ci() {
        return;
    }
    // Serialise the load -> mutate -> save window so two worktrees of the same
    // repo (same store key) cannot lost-update each other's record.
    let _lock = ImpactStoreLock::acquire(root);
    let mut store = load(root);
    if !resolve_enabled(&store).0 {
        return;
    }
    store.schema_version = STORE_SCHEMA_VERSION;
    // Capture the repo basename for the cross-repo view (memoized; no extra git
    // probe). Refreshed each run so a renamed repo folder updates its label.
    store.label = project_identity(root).2;

    let counts = ImpactCounts::from_summary(summary);
    let verdict_str = verdict_label(*verdict);

    if store.first_recorded.is_none() {
        store.first_recorded = Some((*timestamp).to_owned());
    }

    apply_containment(&mut store, *verdict, *gate, *git_sha, timestamp, &counts);

    store.records.push(ImpactRecord {
        timestamp: (*timestamp).to_owned(),
        version: (*version).to_owned(),
        git_sha: git_sha.map(ToOwned::to_owned),
        verdict: verdict_str.to_owned(),
        gate: *gate,
        counts,
    });
    compact(&mut store);

    if let Some(attribution) = attribution {
        let (_, worktree, _) = project_identity(root);
        apply_attribution(&mut store, attribution, &worktree, *git_sha, timestamp);
    }

    save(&store, root);
    if let Some(max_age) = resolve_store_max_age() {
        sweep_old_stores(&project_identity(root).0, max_age);
    }
}

/// Record a whole-project combined run into the project track.
pub fn record_combined_run(
    root: &Path,
    counts: ImpactCounts,
    git_sha: Option<&str>,
    version: &str,
    timestamp: &str,
    attribution: Option<&AttributionInput<'_>>,
) {
    if record_gate_is_ci() {
        return;
    }
    let _lock = ImpactStoreLock::acquire(root);
    let mut store = load(root);
    if !resolve_enabled(&store).0 {
        return;
    }
    store.schema_version = STORE_SCHEMA_VERSION;
    store.label = project_identity(root).2;

    if store.first_recorded.is_none() {
        store.first_recorded = Some(timestamp.to_owned());
    }

    let verdict_str = if counts.total_issues == 0 {
        "pass"
    } else {
        "warn"
    };
    store.project_records.push(ImpactRecord {
        timestamp: timestamp.to_owned(),
        version: version.to_owned(),
        git_sha: git_sha.map(ToOwned::to_owned),
        verdict: verdict_str.to_owned(),
        gate: false,
        counts,
    });
    if store.project_records.len() > MAX_RECORDS {
        let overflow = store.project_records.len() - MAX_RECORDS;
        store.project_records.drain(0..overflow);
    }

    if let Some(attribution) = attribution {
        let (_, worktree, _) = project_identity(root);
        apply_attribution(&mut store, attribution, &worktree, git_sha, timestamp);
    }

    save(&store, root);
    if let Some(max_age) = resolve_store_max_age() {
        sweep_old_stores(&project_identity(root).0, max_age);
    }
}

/// Update pending/contained state from a gate run's verdict.
fn apply_containment(
    store: &mut ImpactStore,
    verdict: AuditVerdict,
    gate: bool,
    git_sha: Option<&str>,
    timestamp: &str,
    counts: &ImpactCounts,
) {
    if !gate {
        return;
    }
    if verdict == AuditVerdict::Fail {
        if store.pending_containment.is_none() {
            store.pending_containment = Some(PendingContainment {
                blocked_at: timestamp.to_owned(),
                git_sha: git_sha.map(ToOwned::to_owned),
                blocked_counts: counts.clone(),
            });
        }
    } else if let Some(pending) = store.pending_containment.take() {
        store.containment.push(ContainmentEvent {
            blocked_at: pending.blocked_at,
            cleared_at: timestamp.to_owned(),
            git_sha: pending.git_sha,
            blocked_counts: pending.blocked_counts,
        });
        if store.containment.len() > MAX_CONTAINMENT {
            let overflow = store.containment.len() - MAX_CONTAINMENT;
            store.containment.drain(0..overflow);
        }
    }
}

fn compact(store: &mut ImpactStore) {
    if store.records.len() > MAX_RECORDS {
        let overflow = store.records.len() - MAX_RECORDS;
        store.records.drain(0..overflow);
    }
}

#[derive(Debug, Clone)]
pub struct FindingInput {
    pub path: PathBuf,
    pub kind: &'static str,
    pub symbol: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CloneInput {
    pub fingerprint: String,
    pub instance_paths: Vec<PathBuf>,
}

pub enum Scope<'a> {
    ChangedFiles(&'a [PathBuf]),
    WholeProject,
}

pub struct AttributionInput<'a> {
    pub root: &'a Path,
    pub scope: Scope<'a>,
    pub findings: Vec<FindingInput>,
    pub clones: Vec<CloneInput>,
    pub suppressions: &'a [ActiveSuppression],
}

fn finding_id(kind: &str, rel_path: &str, symbol: Option<&str>) -> String {
    fingerprint_hash(&[kind, rel_path, symbol.unwrap_or("")])
}

fn covered_by(present: &FxHashSet<String>, kind: &str) -> bool {
    present.contains(BLANKET_SUPPRESSION) || present.contains(kind)
}

/// A single worktree's flat per-file attribution baseline (rel-path -> findings).
type FlatFrontier = FxHashMap<String, FileFrontier>;
/// A single worktree's flat clone baseline (fingerprint -> instance paths).
type FlatCloneFrontier = FxHashMap<String, Vec<String>>;

fn apply_attribution(
    store: &mut ImpactStore,
    input: &AttributionInput<'_>,
    worktree_key: &str,
    git_sha: Option<&str>,
    timestamp: &str,
) {
    let root = input.root;
    // Pull THIS worktree's baseline out of the (repo-collapsed) store into owned
    // flat locals. The helpers mutate these locals plus the shared totals on
    // `store`; because the locals are owned (not borrowed from `store`) there is
    // no aliasing with the `store.resolved_total` / `recent_resolved` writes.
    let mut frontier: FlatFrontier = store.frontier.remove(worktree_key).unwrap_or_default();
    let mut clone_frontier: FlatCloneFrontier = store
        .clone_frontier
        .remove(worktree_key)
        .unwrap_or_default();

    let changed: FxHashSet<String> = match input.scope {
        Scope::ChangedFiles(files) => files.iter().map(|p| format_display_path(p, root)).collect(),
        Scope::WholeProject => whole_project_scope(&frontier, &clone_frontier, input, root),
    };

    let mut current_findings: FxHashMap<String, Vec<FrontierFinding>> = FxHashMap::default();
    for f in &input.findings {
        let rel = format_display_path(&f.path, root);
        if !changed.contains(&rel) {
            continue;
        }
        let id = finding_id(f.kind, &rel, f.symbol.as_deref());
        current_findings
            .entry(rel)
            .or_default()
            .push(FrontierFinding {
                id,
                kind: f.kind.to_owned(),
                symbol: f.symbol.clone(),
            });
    }
    let mut current_supps: FxHashMap<String, FxHashSet<String>> = FxHashMap::default();
    for s in input.suppressions {
        let rel = format_display_path(&s.path, root);
        if !changed.contains(&rel) {
            continue;
        }
        let key = s
            .kind
            .clone()
            .unwrap_or_else(|| BLANKET_SUPPRESSION.to_owned());
        current_supps.entry(rel).or_default().insert(key);
    }

    let mut appeared_move_keys: FxHashSet<String> = FxHashSet::default();
    for (rel, findings) in &current_findings {
        let prior_ids: FxHashSet<&str> = frontier
            .get(rel)
            .map(|f| f.findings.iter().map(|x| x.id.as_str()).collect())
            .unwrap_or_default();
        for ff in findings {
            if !prior_ids.contains(ff.id.as_str()) {
                appeared_move_keys.insert(ff.move_key());
            }
        }
    }

    uncredit_cross_run_moves(store, &appeared_move_keys);

    let mut disappearance_input = FileDisappearancesInput {
        store,
        frontier: &frontier,
        changed: &changed,
        current_findings: &current_findings,
        current_supps: &current_supps,
        appeared_move_keys: &appeared_move_keys,
        git_sha,
        timestamp,
    };
    classify_file_disappearances(&mut disappearance_input);
    update_file_frontier(&mut frontier, &changed, current_findings, current_supps);
    classify_clone_disappearances(
        store,
        &frontier,
        &mut clone_frontier,
        input,
        &changed,
        git_sha,
        timestamp,
    );
    prune_frontier(&mut frontier, &mut clone_frontier, root);
    bound_recent_resolved(store);

    // Store this worktree's baseline back; drop the worktree key entirely when
    // empty so deleted/abandoned worktrees do not accumulate.
    if frontier.is_empty() {
        store.frontier.remove(worktree_key);
    } else {
        store.frontier.insert(worktree_key.to_owned(), frontier);
    }
    if clone_frontier.is_empty() {
        store.clone_frontier.remove(worktree_key);
    } else {
        store
            .clone_frontier
            .insert(worktree_key.to_owned(), clone_frontier);
    }
}

fn whole_project_scope(
    frontier: &FlatFrontier,
    clone_frontier: &FlatCloneFrontier,
    input: &AttributionInput<'_>,
    root: &Path,
) -> FxHashSet<String> {
    let mut set: FxHashSet<String> = frontier.keys().cloned().collect();
    for paths in clone_frontier.values() {
        for p in paths {
            set.insert(p.clone());
        }
    }
    for f in &input.findings {
        set.insert(format_display_path(&f.path, root));
    }
    for c in &input.clones {
        for p in &c.instance_paths {
            set.insert(format_display_path(p, root));
        }
    }
    set
}

struct FileDisappearancesInput<'a> {
    store: &'a mut ImpactStore,
    frontier: &'a FlatFrontier,
    changed: &'a FxHashSet<String>,
    current_findings: &'a FxHashMap<String, Vec<FrontierFinding>>,
    current_supps: &'a FxHashMap<String, FxHashSet<String>>,
    appeared_move_keys: &'a FxHashSet<String>,
    git_sha: Option<&'a str>,
    timestamp: &'a str,
}

fn classify_file_disappearances(input: &mut FileDisappearancesInput<'_>) {
    let store = &mut *input.store;
    let frontier = input.frontier;
    let changed = input.changed;
    let current_findings = input.current_findings;
    let current_supps = input.current_supps;
    let appeared_move_keys = input.appeared_move_keys;
    let git_sha = input.git_sha;
    let timestamp = input.timestamp;
    let empty_supps = FxHashSet::default();
    for rel in changed {
        let Some(prior) = frontier.get(rel) else {
            continue;
        };
        let now_ids: FxHashSet<&str> = current_findings
            .get(rel)
            .map(|fs| fs.iter().map(|f| f.id.as_str()).collect())
            .unwrap_or_default();
        let now_supps = current_supps.get(rel).unwrap_or(&empty_supps);
        let prior_supps: FxHashSet<&str> = prior.suppressions.iter().map(String::as_str).collect();
        let new_supp_kinds: FxHashSet<String> = now_supps
            .iter()
            .filter(|k| !prior_supps.contains(k.as_str()))
            .cloned()
            .collect();

        let mut resolved = Vec::new();
        let mut suppressed = 0usize;
        for pf in &prior.findings {
            if now_ids.contains(pf.id.as_str()) {
                continue; // still present
            }
            if appeared_move_keys.contains(&pf.move_key()) {
                continue; // moved to another file this run
            }
            if covered_by(&new_supp_kinds, &pf.kind) {
                suppressed += 1; // conservative: a fresh fallow-ignore, never a win
            } else {
                resolved.push(pf.clone());
            }
        }
        store.suppressed_total += suppressed;
        for pf in resolved {
            store.resolved_total += 1;
            store.recent_resolved.push(ResolutionEvent {
                kind: pf.kind,
                path: rel.clone(),
                symbol: pf.symbol,
                git_sha: git_sha.map(ToOwned::to_owned),
                timestamp: timestamp.to_owned(),
            });
        }
    }
}

fn update_file_frontier(
    frontier: &mut FlatFrontier,
    changed: &FxHashSet<String>,
    mut current_findings: FxHashMap<String, Vec<FrontierFinding>>,
    mut current_supps: FxHashMap<String, FxHashSet<String>>,
) {
    for rel in changed {
        let findings = current_findings.remove(rel).unwrap_or_default();
        let mut suppressions: Vec<String> = current_supps
            .remove(rel)
            .unwrap_or_default()
            .into_iter()
            .collect();
        suppressions.sort_unstable();
        if findings.is_empty() && suppressions.is_empty() {
            frontier.remove(rel);
        } else {
            frontier.insert(
                rel.clone(),
                FileFrontier {
                    findings,
                    suppressions,
                },
            );
        }
    }
}

fn classify_clone_disappearances(
    store: &mut ImpactStore,
    frontier: &FlatFrontier,
    clone_frontier: &mut FlatCloneFrontier,
    input: &AttributionInput<'_>,
    changed: &FxHashSet<String>,
    git_sha: Option<&str>,
    timestamp: &str,
) {
    let root = input.root;
    let mut current: FxHashMap<String, Vec<String>> = FxHashMap::default();
    for c in &input.clones {
        let mut paths: Vec<String> = c
            .instance_paths
            .iter()
            .map(|p| format_display_path(p, root))
            .collect();
        paths.sort_unstable();
        paths.dedup();
        if paths.iter().any(|p| changed.contains(p)) {
            current.insert(c.fingerprint.clone(), paths);
        }
    }

    let dup_suppressed = |paths: &[String]| -> bool {
        paths.iter().any(|p| {
            changed.contains(p)
                && frontier.get(p).is_some_and(|f| {
                    f.suppressions
                        .iter()
                        .any(|k| k == CODE_DUPLICATION_KIND || k == BLANKET_SUPPRESSION)
                })
        })
    };

    let still_duplicated: FxHashSet<&String> = current.values().flatten().collect();

    let disappeared: Vec<(String, Vec<String>)> = clone_frontier
        .iter()
        .filter(|(fp, paths)| {
            paths.iter().any(|p| changed.contains(p)) && !current.contains_key(*fp)
        })
        .map(|(fp, paths)| (fp.clone(), paths.clone()))
        .collect();

    for (fp, paths) in disappeared {
        clone_frontier.remove(&fp);
        if paths.iter().any(|p| still_duplicated.contains(p)) {
            continue;
        }
        if dup_suppressed(&paths) {
            store.suppressed_total += 1;
        } else {
            store.resolved_total += 1;
            let path = paths.first().cloned().unwrap_or_default();
            store.recent_resolved.push(ResolutionEvent {
                kind: CODE_DUPLICATION_KIND.to_owned(),
                path,
                symbol: None,
                git_sha: git_sha.map(ToOwned::to_owned),
                timestamp: timestamp.to_owned(),
            });
        }
    }

    for (fp, paths) in current {
        clone_frontier.insert(fp, paths);
    }
}

fn prune_frontier(
    frontier: &mut FlatFrontier,
    clone_frontier: &mut FlatCloneFrontier,
    root: &Path,
) {
    frontier.retain(|rel, _| root.join(rel).exists());
    clone_frontier.retain(|_, paths| paths.iter().any(|p| root.join(p).exists()));
}

fn bound_recent_resolved(store: &mut ImpactStore) {
    if store.recent_resolved.len() > MAX_RECENT_RESOLVED {
        let overflow = store.recent_resolved.len() - MAX_RECENT_RESOLVED;
        store.recent_resolved.drain(0..overflow);
    }
}

fn event_move_key(ev: &ResolutionEvent) -> Option<String> {
    ev.symbol
        .as_ref()
        .map(|symbol| format!("{}{ID_SEP}{symbol}", ev.kind))
}

fn uncredit_cross_run_moves(store: &mut ImpactStore, appeared_move_keys: &FxHashSet<String>) {
    if appeared_move_keys.is_empty() {
        return;
    }
    let mut uncredited = 0usize;
    store.recent_resolved.retain(|ev| match event_move_key(ev) {
        Some(mk) if appeared_move_keys.contains(&mk) => {
            uncredited += 1;
            false
        }
        _ => true,
    });
    store.resolved_total = store.resolved_total.saturating_sub(uncredited);
}

#[must_use]
pub fn collect_dead_code_findings(results: &AnalysisResults) -> Vec<FindingInput> {
    let mut out = Vec::new();
    let mut push = |path: &Path, kind: &'static str, symbol: Option<String>| {
        out.push(FindingInput {
            path: path.to_path_buf(),
            kind,
            symbol,
        });
    };
    collect_unused_symbol_findings(results, &mut push);
    collect_dependency_findings(results, &mut push);
    collect_catalog_findings(results, &mut push);
    out
}

fn collect_unused_symbol_findings(
    results: &AnalysisResults,
    push: &mut impl FnMut(&Path, &'static str, Option<String>),
) {
    for f in &results.unused_files {
        push(&f.file.path, "unused-file", None);
    }
    for f in &results.unused_exports {
        push(
            &f.export.path,
            "unused-export",
            Some(f.export.export_name.clone()),
        );
    }
    for f in &results.unused_types {
        push(
            &f.export.path,
            "unused-type",
            Some(f.export.export_name.clone()),
        );
    }
    for f in &results.private_type_leaks {
        push(
            &f.leak.path,
            "private-type-leak",
            Some(format!(
                "{}{ID_SEP}{}",
                f.leak.export_name, f.leak.type_name
            )),
        );
    }
    for f in &results.unused_enum_members {
        push(
            &f.member.path,
            "unused-enum-member",
            Some(format!(
                "{}{ID_SEP}{}",
                f.member.parent_name, f.member.member_name
            )),
        );
    }
    for f in &results.unused_class_members {
        push(
            &f.member.path,
            "unused-class-member",
            Some(format!(
                "{}{ID_SEP}{}",
                f.member.parent_name, f.member.member_name
            )),
        );
    }
    for f in &results.unused_store_members {
        push(
            &f.member.path,
            "unused-store-member",
            Some(format!(
                "{}{ID_SEP}{}",
                f.member.parent_name, f.member.member_name
            )),
        );
    }
    for f in &results.unresolved_imports {
        push(
            &f.import.path,
            "unresolved-import",
            Some(f.import.specifier.clone()),
        );
    }
    for f in &results.boundary_violations {
        let to_path = f.violation.to_path.to_string_lossy().replace('\\', "/");
        push(
            &f.violation.from_path,
            "boundary-violation",
            Some(format!("{to_path}{ID_SEP}{}", f.violation.import_specifier)),
        );
    }
}

fn collect_dependency_findings(
    results: &AnalysisResults,
    push: &mut impl FnMut(&Path, &'static str, Option<String>),
) {
    for f in &results.unused_dependencies {
        push(
            &f.dep.path,
            "unused-dependency",
            Some(f.dep.package_name.clone()),
        );
    }
    for f in &results.unused_dev_dependencies {
        push(
            &f.dep.path,
            "unused-dev-dependency",
            Some(f.dep.package_name.clone()),
        );
    }
    for f in &results.unused_optional_dependencies {
        push(
            &f.dep.path,
            "unused-optional-dependency",
            Some(f.dep.package_name.clone()),
        );
    }
    for f in &results.type_only_dependencies {
        push(
            &f.dep.path,
            "type-only-dependency",
            Some(f.dep.package_name.clone()),
        );
    }
    for f in &results.test_only_dependencies {
        push(
            &f.dep.path,
            "test-only-dependency",
            Some(f.dep.package_name.clone()),
        );
    }
}

fn collect_catalog_findings(
    results: &AnalysisResults,
    push: &mut impl FnMut(&Path, &'static str, Option<String>),
) {
    for f in &results.unused_catalog_entries {
        push(
            &f.entry.path,
            "unused-catalog-entry",
            Some(format!(
                "{}{ID_SEP}{}",
                f.entry.catalog_name, f.entry.entry_name
            )),
        );
    }
    for f in &results.empty_catalog_groups {
        push(
            &f.group.path,
            "empty-catalog-group",
            Some(f.group.catalog_name.clone()),
        );
    }
    for f in &results.unresolved_catalog_references {
        push(
            &f.reference.path,
            "unresolved-catalog-reference",
            Some(format!(
                "{}{ID_SEP}{}",
                f.reference.catalog_name, f.reference.entry_name
            )),
        );
    }
    for f in &results.unused_dependency_overrides {
        push(
            &f.entry.path,
            "unused-dependency-override",
            Some(f.entry.raw_key.clone()),
        );
    }
    for f in &results.misconfigured_dependency_overrides {
        push(
            &f.entry.path,
            "misconfigured-dependency-override",
            Some(f.entry.raw_key.clone()),
        );
    }
}

/// Collect line-independent complexity finding identities `(path, function name)`
/// from a health report. The function name is line-independent, so a function
/// moving within its file keeps the same identity.
#[must_use]
pub fn collect_complexity_findings(
    report: &crate::health_types::HealthReport,
) -> Vec<FindingInput> {
    report
        .findings
        .iter()
        .map(|f| FindingInput {
            path: f.path.clone(),
            kind: "complexity",
            symbol: Some(f.name.clone()),
        })
        .collect()
}

/// Collect clone-group identities `(fingerprint, instance paths)` from a
/// duplication report. The fingerprint is content-derived (`dup:<hash>`), so it
/// is stable across pure relocation.
#[must_use]
pub fn collect_clone_findings(
    report: &fallow_core::duplicates::DuplicationReport,
) -> Vec<CloneInput> {
    report
        .clone_groups
        .iter()
        .map(|g| CloneInput {
            fingerprint: fallow_core::duplicates::clone_fingerprint(&g.instances),
            instance_paths: g.instances.iter().map(|i| i.file.clone()).collect(),
        })
        .collect()
}

const fn verdict_label(verdict: AuditVerdict) -> &'static str {
    match verdict {
        AuditVerdict::Pass => "pass",
        AuditVerdict::Warn => "warn",
        AuditVerdict::Fail => "fail",
    }
}

/// Direction of a count trend between two recorded runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ImpactTrendDirection {
    /// Issue count went down (good).
    Improving,
    /// Issue count went up.
    Declining,
    /// Within tolerance.
    Stable,
}

/// A computed trend between the two most recent records.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TrendSummary {
    pub direction: ImpactTrendDirection,
    /// Signed delta in total issues (current minus previous).
    pub total_delta: i64,
    pub previous_total: usize,
    pub current_total: usize,
}

fn direction_for(delta: i64) -> ImpactTrendDirection {
    if delta < -TREND_TOLERANCE {
        ImpactTrendDirection::Improving
    } else if delta > TREND_TOLERANCE {
        ImpactTrendDirection::Declining
    } else {
        ImpactTrendDirection::Stable
    }
}

/// Wire-version discriminator for [`ImpactReport`]. Independent from the global
/// `SchemaVersion` (the impact report versions on its own cadence) and from the
/// on-disk `STORE_SCHEMA_VERSION` (the persisted store shape versions
/// separately). Serializes as a string `const` so JSON consumers can switch on
/// it, matching the other independently-versioned envelopes (e.g.
/// `CoverageAnalyzeSchemaVersion`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ImpactReportSchemaVersion {
    /// First release of the `fallow impact --format json` shape.
    #[serde(rename = "1")]
    V1,
}

/// The rendered impact report, derived purely from the store (no analysis run).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow impact --format json"))]
pub struct ImpactReport {
    /// Output-shape version for this report, so JSON consumers have a
    /// forward-compat signal independent of the on-disk store version. Always
    /// present; bumped only on a breaking change to this report's wire shape.
    pub schema_version: ImpactReportSchemaVersion,
    pub enabled: bool,
    /// WHY tracking is on or off: `project` (an explicit per-repo enable/disable
    /// decision), `user` (the user-global default with no per-repo decision), or
    /// `default` (off, no per-repo decision and no global default). Combine with
    /// `explicit_decision` to tell a never-asked off-state (`enabled:false`,
    /// `explicit_decision:false`, offer to enable) from a declined-here one
    /// (`enabled:false`, `explicit_decision:true`, do not nag).
    pub enabled_source: EnabledSource,
    pub record_count: usize,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_recorded: Option<String>,
    /// Git SHA of the most recent recorded run, so a consumer can tell which
    /// commit the `surfacing` counts belong to. This is an ABBREVIATED SHA
    /// (`git rev-parse --short`), so it is for display/correlation only and will
    /// not match a full 40-character SHA from `$GITHUB_SHA` or the git API
    /// without expansion. None when the latest run had no SHA (not a git repo)
    /// or there are no records yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_git_sha: Option<String>,
    /// Counts from the most recent recorded run. These are CHANGED-FILE scoped
    /// (each record comes from a `fallow audit` run, whose default `new-only`
    /// gate counts only findings in the changed files of that run), NOT a
    /// whole-project total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surfacing: Option<ImpactCounts>,
    /// Trend between the two most recent records. None until two records exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<TrendSummary>,
    /// Counts from the most recent whole-project `fallow` run. WHOLE-PROJECT
    /// scope (not changed-file), so this is the current issue total across the
    /// whole repo, context next to the actionable changed-file `surfacing`
    /// count. None until a full `fallow` run has been recorded. v1.6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_surfacing: Option<ImpactCounts>,
    /// Trend between the two most recent whole-project records. Comparable over
    /// time (same whole-project denominator every run), unlike the changed-file
    /// `trend`. None until two full `fallow` runs exist. v1.6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_trend: Option<TrendSummary>,
    pub containment_count: usize,
    /// Most recent containment events (newest last), capped for display.
    pub recent_containment: Vec<ContainmentEvent>,
    /// Lifetime count of findings fallow credits as genuinely resolved (code
    /// removed or refactored, never a `fallow-ignore`). v1.5.
    pub resolved_total: usize,
    /// Lifetime count of findings silenced by a newly-added `fallow-ignore`.
    /// Reported as honest context, never as a win. v1.5.
    pub suppressed_total: usize,
    /// Most recent resolution events (newest last), capped for display. v1.5.
    pub recent_resolved: Vec<ResolutionEvent>,
    /// Whether per-finding attribution has a baseline yet. False on a freshly
    /// upgraded v1 store (no frontier captured), which the renderer uses to show
    /// "resolution tracking starts from your next run" instead of a bare zero.
    pub attribution_active: bool,
    /// Whether the local agent onboarding prompt has been explicitly declined.
    /// Stored in the user config dir (per project) so agents avoid cross-session
    /// nags without writing into the repo.
    pub onboarding_declined: bool,
    /// Whether the user ever made an explicit enable/disable decision for
    /// Impact tracking. `enabled: false` with `explicit_decision: false` means
    /// "never asked"; with `true` it means "asked and declined". Agents use
    /// this to offer the impact opt-in exactly once per project.
    pub explicit_decision: bool,
}

/// Build a report from the store. Defensive: a single record (or none) yields
/// no trend rather than a spurious spike, and an empty store yields an empty
/// report flagged so the renderer can show the first-run message.
/// Trend between the two most recent records in a series. None until two records
/// exist; a missing prior record is "unknown" (no trend), never a spike.
fn trend_for(records: &[ImpactRecord]) -> Option<TrendSummary> {
    if records.len() < 2 {
        return None;
    }
    let current = &records[records.len() - 1];
    let previous = &records[records.len() - 2];
    let current_total = current.counts.total_issues;
    let previous_total = previous.counts.total_issues;
    let total_delta = current_total as i64 - previous_total as i64;
    Some(TrendSummary {
        direction: direction_for(total_delta),
        total_delta,
        previous_total,
        current_total,
    })
}

pub fn build_report(store: &ImpactStore) -> ImpactReport {
    let surfacing = store.records.last().map(|r| r.counts.clone());
    let trend = trend_for(&store.records);
    let project_surfacing = store.project_records.last().map(|r| r.counts.clone());
    let project_trend = trend_for(&store.project_records);

    let recent_containment = store
        .containment
        .iter()
        .rev()
        .take(5)
        .rev()
        .cloned()
        .collect();

    let latest_git_sha = store.records.last().and_then(|r| r.git_sha.clone());

    let recent_resolved = store
        .recent_resolved
        .iter()
        .rev()
        .take(5)
        .rev()
        .cloned()
        .collect();
    let attribution_active = !store.frontier.is_empty()
        || !store.clone_frontier.is_empty()
        || store.resolved_total > 0
        || store.suppressed_total > 0;

    let (enabled, enabled_source) = resolve_enabled(store);
    ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled,
        enabled_source,
        record_count: store.records.len(),
        meta: None,
        first_recorded: store.first_recorded.clone(),
        latest_git_sha,
        surfacing,
        trend,
        project_surfacing,
        project_trend,
        containment_count: store.containment.len(),
        recent_containment,
        resolved_total: store.resolved_total,
        suppressed_total: store.suppressed_total,
        recent_resolved,
        attribution_active,
        onboarding_declined: store.onboarding_declined,
        explicit_decision: store.explicit_decision,
    }
}

// ----- Cross-repo aggregate view (`fallow impact --all`) -------------------

/// Independent wire-version for the cross-repo report, on its own cadence (it
/// versions separately from the per-project `ImpactReportSchemaVersion` and the
/// on-disk `STORE_SCHEMA_VERSION`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CrossRepoImpactSchemaVersion {
    /// First release of the `fallow impact --all --format json` shape.
    #[serde(rename = "1")]
    V1,
}

/// Grand totals across every tracked project (including repos whose directory no
/// longer exists on disk: their past wins still count toward lifetime impact).
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CrossRepoTotals {
    pub resolved_total: usize,
    pub suppressed_total: usize,
    pub containment_count: usize,
    /// Sum of whole-project issue totals across projects that have a full-run
    /// baseline, as of EACH project's last full `fallow` run (not a simultaneous
    /// snapshot).
    pub project_wide_issues: usize,
    pub projects_with_baseline: usize,
}

/// One project's row in the cross-repo roll-up.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CrossRepoProjectEntry {
    /// Stable, non-reversible project key (the store filename stem); the
    /// cross-tool/cross-run JOIN key. NEVER a path.
    pub project_key: String,
    /// Repo basename for display (never a full path). Absent on pre-v5 stores
    /// (the row falls back to the short key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Timestamp of the project's most recent recorded run (changed-file or
    /// whole-project), for the LAST RUN column and the default `recent` sort.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_recorded: Option<String>,
    /// The full per-project report (identical shape to `fallow impact --format
    /// json`), reused verbatim so the per-project wire contract is the sub-shape.
    pub report: ImpactReport,
}

/// The cross-repo aggregate report (`fallow impact --all --format json`).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow impact --all --format json")
)]
pub struct CrossRepoImpactReport {
    pub schema_version: CrossRepoImpactSchemaVersion,
    /// Per-project stores successfully parsed (add `unreadable_count` for the
    /// total number of store files found in the user config dir).
    pub project_count: usize,
    /// Stores with recorded history (the rows in `projects`); excludes
    /// enabled-but-empty stores, which are still counted in `project_count`.
    pub tracked_count: usize,
    /// Stores that failed to parse and were skipped (corrupt or newer-schema).
    pub unreadable_count: usize,
    pub totals: CrossRepoTotals,
    pub projects: Vec<CrossRepoProjectEntry>,
}

/// Ranking for the cross-repo rows.
#[derive(Debug, Clone, Copy)]
pub enum CrossRepoSort {
    /// Most recently recorded first (the default: active repos float up).
    Recent,
    /// Most findings resolved first.
    Resolved,
    /// Most commits contained first.
    Contained,
    /// Alphabetical by label/key.
    Name,
}

/// The newest record timestamp across the changed-file and whole-project series.
fn latest_activity(store: &ImpactStore) -> Option<String> {
    let a = store.records.last().map(|r| r.timestamp.clone());
    let b = store.project_records.last().map(|r| r.timestamp.clone());
    match (a, b) {
        (Some(x), Some(y)) => Some(if x >= y { x } else { y }),
        (x, y) => x.or(y),
    }
}

/// Enumerate every per-project store in `<config-dir>/fallow/impact/`, returning
/// `(project_key, store)` pairs plus the count of files that failed to parse.
/// Read-only; never writes. The global `impact.json` toggle is a sibling FILE of
/// this dir (one level up), so it is naturally excluded. Corrupt/newer-schema
/// files are skipped and counted, never substituted with a default store.
#[must_use]
pub fn load_all() -> (Vec<(String, ImpactStore)>, usize) {
    let Some(dir) = impact_config_dir().map(|d| d.join("impact")) else {
        return (Vec::new(), 0);
    };
    let Ok(read) = std::fs::read_dir(&dir) else {
        return (Vec::new(), 0);
    };
    let mut stores = Vec::new();
    let mut unreadable = 0usize;
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        match std::fs::read_to_string(&path)
            .ok()
            .and_then(|c| serde_json::from_str::<ImpactStore>(&c).ok())
        {
            Some(store) => stores.push((key, store)),
            None => unreadable += 1,
        }
    }
    (stores, unreadable)
}

/// Build the cross-repo aggregate from enumerated stores. Excludes
/// enabled-but-empty projects from the rows (counted in `project_count`), sums
/// totals over every tracked project, and sorts the rows by `sort`.
#[must_use]
pub fn build_aggregate_report(
    stores: Vec<(String, ImpactStore)>,
    unreadable: usize,
    sort: CrossRepoSort,
) -> CrossRepoImpactReport {
    let project_count = stores.len();
    let mut totals = CrossRepoTotals::default();
    let mut projects = Vec::new();
    for (key, store) in stores {
        let report = build_report(&store);
        let has_history = report.record_count > 0
            || report.project_surfacing.is_some()
            || report.resolved_total > 0
            || report.containment_count > 0;
        if !has_history {
            continue;
        }
        totals.resolved_total += report.resolved_total;
        totals.suppressed_total += report.suppressed_total;
        totals.containment_count += report.containment_count;
        if let Some(ps) = &report.project_surfacing {
            totals.project_wide_issues += ps.total_issues;
            totals.projects_with_baseline += 1;
        }
        projects.push(CrossRepoProjectEntry {
            project_key: key,
            label: store.label.clone(),
            last_recorded: latest_activity(&store),
            report,
        });
    }
    sort_cross_repo(&mut projects, sort);
    CrossRepoImpactReport {
        schema_version: CrossRepoImpactSchemaVersion::V1,
        project_count,
        tracked_count: projects.len(),
        unreadable_count: unreadable,
        totals,
        projects,
    }
}

fn sort_cross_repo(projects: &mut [CrossRepoProjectEntry], sort: CrossRepoSort) {
    match sort {
        // Newest activity first; missing timestamps sort last. project_key
        // tiebreak keeps the order deterministic.
        CrossRepoSort::Recent => projects.sort_by(|a, b| {
            b.last_recorded
                .cmp(&a.last_recorded)
                .then_with(|| a.project_key.cmp(&b.project_key))
        }),
        CrossRepoSort::Resolved => projects.sort_by(|a, b| {
            b.report
                .resolved_total
                .cmp(&a.report.resolved_total)
                .then_with(|| a.project_key.cmp(&b.project_key))
        }),
        CrossRepoSort::Contained => projects.sort_by(|a, b| {
            b.report
                .containment_count
                .cmp(&a.report.containment_count)
                .then_with(|| a.project_key.cmp(&b.project_key))
        }),
        CrossRepoSort::Name => projects.sort_by(|a, b| {
            cross_repo_label(a)
                .cmp(&cross_repo_label(b))
                .then_with(|| a.project_key.cmp(&b.project_key))
        }),
    }
}

/// The display label for a row: the basename when present, else the short key.
/// Pure (no path access), so JSON/markdown using it can never leak a path.
fn cross_repo_label(entry: &CrossRepoProjectEntry) -> String {
    entry
        .label
        .clone()
        .unwrap_or_else(|| short_key(&entry.project_key))
}

/// First 12 hex of a project key, for opaque-but-stable row labels.
fn short_key(key: &str) -> String {
    key.chars().take(12).collect()
}

/// Build the cross-repo report by enumerating the config dir.
#[must_use]
pub fn aggregate(sort: CrossRepoSort) -> CrossRepoImpactReport {
    let (stores, unreadable) = load_all();
    build_aggregate_report(stores, unreadable, sort)
}

/// Render the whole-project view for the human report. Deliberately understated
/// (one count line, one trend line, one caveat) rather than a co-equal header:
/// the project track advances only on local full `fallow` runs, not CI, so it is
/// context for the changed-file story above, not the headline. Renders nothing
/// when no full `fallow` run has been recorded yet.
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
fn render_project_section(out: &mut String, report: &ImpactReport) {
    let Some(s) = &report.project_surfacing else {
        return;
    };
    out.push_str(&format!(
        "  WHOLE PROJECT (whole-repo context, not a to-do)\n    {} issue{} across the whole project at your last full `fallow` run\n",
        s.total_issues,
        plural(s.total_issues),
    ));
    if let Some(t) = &report.project_trend {
        let arrow = trend_arrow(t.direction);
        out.push_str(&format!(
            "    {} -> {} ({}) across your last two full runs (comparable over time)\n",
            t.previous_total, t.current_total, arrow,
        ));
    } else {
        out.push_str("    project trend starts after your next full `fallow` run\n");
    }
    out.push_str("      advances only on your local full `fallow` runs, not CI\n\n");
}

/// Render the report as human-readable text.
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
pub fn render_human(report: &ImpactReport) -> String {
    let mut out = String::new();
    out.push_str("FALLOW IMPACT\n\n");

    if !report.enabled {
        out.push_str(
            "Impact tracking is off. Enable it with `fallow impact enable`, then\n\
             let your pre-commit gate run a few times to build history.\n",
        );
        return out;
    }

    if report.enabled_source == EnabledSource::User {
        out.push_str(
            "Enabled by your user-global default (`fallow impact default on`). Run\n\
             `fallow impact disable` to opt this project out.\n\n",
        );
    }

    if report.record_count == 0 && report.project_surfacing.is_none() {
        out.push_str(
            "Tracking enabled. No history yet: check back after your next few\n\
             commits (Impact records each `fallow audit` / pre-commit gate run,\n\
             and each full `fallow` run for the whole-project view).\n",
        );
        return out;
    }

    if let Some(s) = &report.surfacing {
        out.push_str(&format!(
            "  LATEST RUN (changed files, act on these now)\n    {} issue{} flagged in your last `fallow audit` run\n",
            s.total_issues,
            plural(s.total_issues),
        ));
        out.push_str(&format!(
            "      dead code {}  ·  complexity {}  ·  duplication {}\n\n",
            s.dead_code, s.complexity, s.duplication,
        ));
    }

    if let Some(t) = &report.trend {
        let arrow = trend_arrow(t.direction);
        out.push_str(&format!(
            "  TREND\n    {} -> {} issues ({}) across your last two recorded runs\n      each run is changed-file scope, so consecutive runs may cover different changes\n\n",
            t.previous_total, t.current_total, arrow,
        ));
    }

    render_project_section(&mut out, report);

    out.push_str(&format!(
        "  CONTAINED AT COMMIT\n    {} time{} fallow blocked a commit until it was fixed\n",
        report.containment_count,
        plural(report.containment_count),
    ));

    if report.resolved_total > 0 {
        out.push_str(&format!(
            "\n  RESOLVED\n    {} finding{} you cleared since fallow started tracking\n",
            report.resolved_total,
            plural(report.resolved_total),
        ));
        for ev in &report.recent_resolved {
            match &ev.symbol {
                Some(symbol) => {
                    out.push_str(&format!("      {} {} in {}\n", ev.kind, symbol, ev.path));
                }
                None => out.push_str(&format!("      {} in {}\n", ev.kind, ev.path)),
            }
        }
    } else if report.attribution_active {
        out.push_str(
            "\n  RESOLVED\n    none yet; a finding is credited when fallow re-analyzes the\n      file it left (a fix that reverts a file to its base state\n      may not be individually credited)\n",
        );
    } else {
        out.push_str("\n  RESOLVED\n    resolution tracking starts from your next gate run\n");
    }

    if report.suppressed_total > 0 {
        out.push_str(&format!(
            "      {} finding{} you marked intentional (fallow-ignore), not counted as resolved\n",
            report.suppressed_total,
            plural(report.suppressed_total),
        ));
    }

    out.push('\n');
    let since = report
        .first_recorded
        .as_deref()
        .map_or("the first run", date_only);
    if report.record_count > 0 {
        out.push_str(&format!(
            "Based on {} recorded audit run{} since {}. Local-only; never uploaded.\n\
             Changed-file scope: each audit run only sees files differing from your base.\n",
            report.record_count,
            plural(report.record_count),
            since,
        ));
    } else {
        out.push_str(&format!(
            "Tracking since {since}. Local-only; never uploaded.\n",
        ));
    }
    out.push_str(
        "Resolution tracking is a local-developer signal: it accrues on your\n\
         machine across runs, not in CI (fallow never records there).\n",
    );
    out
}

/// Render the report as JSON.
pub fn render_json(report: &ImpactReport) -> String {
    let value = crate::output_envelope::serialize_root_output(
        crate::output_envelope::FallowOutput::Impact(report.clone()),
    )
    .unwrap_or_else(|_| serde_json::json!({"error":"failed to serialize impact report"}));
    serde_json::to_string_pretty(&value)
        .unwrap_or_else(|_| "{\"error\":\"failed to serialize impact report\"}".to_owned())
}

/// Render the whole-project view for the markdown report. One understated line
/// plus a trend line when available, matching the human renderer's framing.
/// Renders nothing when no full `fallow` run has been recorded yet.
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
fn render_project_markdown(out: &mut String, report: &ImpactReport) {
    let Some(s) = &report.project_surfacing else {
        return;
    };
    out.push_str(&format!(
        "- **Whole project (whole-repo context, last full `fallow` run):** {} issue{} (dead code {}, complexity {}, duplication {})\n",
        s.total_issues,
        plural(s.total_issues),
        s.dead_code,
        s.complexity,
        s.duplication,
    ));
    if let Some(t) = &report.project_trend {
        let arrow = trend_arrow(t.direction);
        out.push_str(&format!(
            "- **Project trend (whole project, last two full runs):** {} -> {} ({})\n",
            t.previous_total, t.current_total, arrow,
        ));
    }
}

/// Render the report as Markdown (paste-ready for a PR description or standup).
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
pub fn render_markdown(report: &ImpactReport) -> String {
    let mut out = String::new();
    out.push_str("## Fallow impact\n\n");

    if !report.enabled {
        out.push_str("Impact tracking is off. Run `fallow impact enable` to start.\n");
        return out;
    }
    if report.record_count == 0 && report.project_surfacing.is_none() {
        out.push_str("Tracking enabled. No history yet; check back after a few commits.\n");
        return out;
    }

    if let Some(s) = &report.surfacing {
        out.push_str(&format!(
            "- **Latest run (changed files):** {} issue{} (dead code {}, complexity {}, duplication {})\n",
            s.total_issues,
            plural(s.total_issues),
            s.dead_code,
            s.complexity,
            s.duplication,
        ));
    }
    if let Some(t) = &report.trend {
        out.push_str(&format!(
            "- **Trend (changed-file scope, last two runs):** {} -> {} ({})\n",
            t.previous_total,
            t.current_total,
            trend_arrow(t.direction),
        ));
    }
    render_project_markdown(&mut out, report);
    out.push_str(&format!(
        "- **Contained at commit:** {} time{}\n",
        report.containment_count,
        plural(report.containment_count),
    ));
    if report.resolved_total > 0 {
        out.push_str(&format!(
            "- **Resolved:** {} finding{} cleared since tracking started\n",
            report.resolved_total,
            plural(report.resolved_total),
        ));
    } else if report.attribution_active {
        out.push_str("- **Resolved:** none yet; tracking active\n");
    } else {
        out.push_str("- **Resolved:** resolution tracking starts from your next gate run\n");
    }
    if report.suppressed_total > 0 {
        out.push_str(&format!(
            "- **Marked intentional:** {} finding{} (`fallow-ignore`), not counted as resolved\n",
            report.suppressed_total,
            plural(report.suppressed_total),
        ));
    }
    let since = report
        .first_recorded
        .as_deref()
        .map_or("the first run", date_only);
    if report.record_count > 0 {
        out.push_str(&format!(
            "\n_Based on {} recorded audit run{} since {}. Local-only; resolution is a local-developer signal._\n",
            report.record_count,
            plural(report.record_count),
            since,
        ));
    } else {
        out.push_str(&format!(
            "\n_Tracking since {since}. Local-only; resolution is a local-developer signal._\n",
        ));
    }
    out
}

/// Render the cross-repo report as JSON via the typed `ImpactCrossRepo` envelope.
#[must_use]
pub fn render_cross_repo_json(report: &CrossRepoImpactReport) -> String {
    let value = crate::output_envelope::serialize_root_output(
        crate::output_envelope::FallowOutput::ImpactCrossRepo(report.clone()),
    )
    .unwrap_or_else(
        |_| serde_json::json!({"error":"failed to serialize cross-repo impact report"}),
    );
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| {
        "{\"error\":\"failed to serialize cross-repo impact report\"}".to_owned()
    })
}

/// A single row's display label (basename when present, else short key). Pure:
/// never touches the filesystem, so it can never leak a path.
fn row_label(entry: &CrossRepoProjectEntry) -> String {
    cross_repo_label(entry)
}

fn opt_count(c: Option<&ImpactCounts>) -> String {
    c.map_or_else(|| "-".to_owned(), |c| c.total_issues.to_string())
}

fn row_trend(report: &ImpactReport) -> &'static str {
    report
        .project_trend
        .as_ref()
        .or(report.trend.as_ref())
        .map_or("-", |t| trend_arrow(t.direction))
}

/// Render the cross-repo roll-up as human-readable text. `limit` caps the
/// printed rows (grand totals always reflect every tracked project). Path-free:
/// the CLI adds the single store-dir discoverability line, gated on `!quiet`.
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
#[must_use]
pub fn render_cross_repo_human(report: &CrossRepoImpactReport, limit: Option<usize>) -> String {
    let mut out = String::new();
    out.push_str("FALLOW IMPACT (ALL PROJECTS)\n\n");

    if report.project_count == 0 {
        if report.unreadable_count > 0 {
            out.push_str(&format!(
                "No readable projects: skipped {} unreadable store{} (corrupt, or written by \
                 a newer fallow). Upgrade fallow to read them.\n",
                report.unreadable_count,
                plural(report.unreadable_count),
            ));
        } else {
            out.push_str(
                "No projects tracked yet. Enable in a repo with `fallow impact enable`, or for \
                 every project with `fallow impact default on`.\n",
            );
        }
        return out;
    }

    out.push_str(&format!(
        "{} project{} tracked, {} with history\n\n",
        report.project_count,
        plural(report.project_count),
        report.tracked_count,
    ));

    if !report.projects.is_empty() {
        out.push_str(&format!(
            "{:<24}{:>8}{:>10}{:>11}{:>10}{:>7}  {}\n",
            "PROJECT", "LATEST", "REPO-WIDE", "CONTAINED", "RESOLVED", "TREND", "LAST RUN",
        ));
        let rows = limit.map_or(report.projects.len(), |n| n.min(report.projects.len()));
        for entry in report.projects.iter().take(rows) {
            let mut label = row_label(entry);
            if label.chars().count() > 22 {
                label = format!("{}...", label.chars().take(19).collect::<String>());
            }
            let last = entry
                .last_recorded
                .as_deref()
                .map_or("-", date_only)
                .to_owned();
            out.push_str(&format!(
                "{:<24}{:>8}{:>10}{:>11}{:>10}{:>7}  {}\n",
                label,
                opt_count(entry.report.surfacing.as_ref()),
                opt_count(entry.report.project_surfacing.as_ref()),
                entry.report.containment_count,
                entry.report.resolved_total,
                row_trend(&entry.report),
                last,
            ));
        }
        if let Some(n) = limit
            && report.projects.len() > n
        {
            out.push_str(&format!(
                "  ... and {} more (raise --limit to show)\n",
                report.projects.len() - n,
            ));
        }
    }

    let no_history = report.project_count.saturating_sub(report.tracked_count);
    if no_history > 0 {
        out.push_str(&format!(
            "\n{no_history} tracked project{} with no history yet\n",
            plural(no_history),
        ));
    }
    if report.unreadable_count > 0 {
        out.push_str(&format!(
            "skipped {} unreadable store{}\n",
            report.unreadable_count,
            plural(report.unreadable_count),
        ));
    }

    let t = &report.totals;
    out.push_str("\nGRAND TOTALS\n");
    out.push_str(&format!(
        "  Across {} tracked project{}: {} finding{} resolved, {} commit{} contained, {} marked intentional\n",
        report.tracked_count,
        plural(report.tracked_count),
        t.resolved_total,
        plural(t.resolved_total),
        t.containment_count,
        plural(t.containment_count),
        t.suppressed_total,
    ));
    if t.projects_with_baseline > 0 {
        out.push_str(&format!(
            "  {} issue{} project-wide across {} project{} with a full-run baseline (as of each project's last full run)\n",
            t.project_wide_issues,
            plural(t.project_wide_issues),
            t.projects_with_baseline,
            plural(t.projects_with_baseline),
        ));
    }
    out.push_str("\nLocal-only; never uploaded; accrues on this machine, not CI.\n");
    out
}

/// Render the cross-repo roll-up as Markdown (paste-ready, path-free).
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
#[must_use]
pub fn render_cross_repo_markdown(report: &CrossRepoImpactReport) -> String {
    let mut out = String::new();
    out.push_str("## Fallow impact (all projects)\n\n");
    if report.project_count == 0 {
        if report.unreadable_count > 0 {
            out.push_str(&format!(
                "No readable projects: skipped {} unreadable store{}.\n",
                report.unreadable_count,
                plural(report.unreadable_count),
            ));
        } else {
            out.push_str("No projects tracked yet.\n");
        }
        return out;
    }
    out.push_str(&format!(
        "{} projects tracked, {} with history.\n\n",
        report.project_count, report.tracked_count,
    ));
    if !report.projects.is_empty() {
        out.push_str("| Project | Latest | Repo-wide | Contained | Resolved | Last run |\n");
        out.push_str("|:--------|-------:|----------:|----------:|---------:|:---------|\n");
        for entry in &report.projects {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                row_label(entry),
                opt_count(entry.report.surfacing.as_ref()),
                opt_count(entry.report.project_surfacing.as_ref()),
                entry.report.containment_count,
                entry.report.resolved_total,
                entry.last_recorded.as_deref().map_or("-", date_only),
            ));
        }
    }
    let t = &report.totals;
    out.push_str(&format!(
        "\n**Grand totals:** {} resolved, {} contained, {} marked intentional across {} tracked projects",
        t.resolved_total, t.containment_count, t.suppressed_total, report.tracked_count,
    ));
    if t.projects_with_baseline > 0 {
        out.push_str(&format!(
            "; {} issues project-wide across {} with a full-run baseline (as of each project's last full run)",
            t.project_wide_issues, t.projects_with_baseline,
        ));
    }
    out.push_str(".\n\n_Local-only; never uploaded; accrues on this machine, not CI._\n");
    out
}

const fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Trim a stored ISO-8601 timestamp (`2026-05-29T18:15:23Z`) to its date part
/// (`2026-05-29`) for human/markdown footers. The wall-clock time and `Z` add
/// noise without meaning when a reader just wants "tracking since when". JSON
/// keeps the full `first_recorded` timestamp. Returns the input unchanged if it
/// has no `T` separator.
fn date_only(ts: &str) -> &str {
    ts.split_once('T').map_or(ts, |(date, _)| date)
}

/// Single human-facing trend vocabulary, shared by the text and markdown
/// renderers so the same concept does not read three different ways. The JSON
/// wire keeps the `improving`/`declining`/`stable` enum form for machines.
const fn trend_arrow(direction: ImpactTrendDirection) -> &'static str {
    match direction {
        ImpactTrendDirection::Improving => "down",
        ImpactTrendDirection::Declining => "up",
        ImpactTrendDirection::Stable => "flat",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-test isolation: a fresh user-config dir (so the store never touches
    /// the real dir and parallel tests do not collide) plus a fresh project
    /// root. Bind BOTH returned `TempDir`s for the test's lifetime. The store
    /// for a non-git tempdir root keys on the canonical root, so each test's
    /// root is its own store.
    fn test_env() -> (tempfile::TempDir, tempfile::TempDir) {
        let config = tempfile::tempdir().unwrap();
        TEST_CONFIG_DIR.with(|c| *c.borrow_mut() = Some(config.path().to_path_buf()));
        let root = tempfile::tempdir().unwrap();
        (config, root)
    }

    /// All frontier rel-paths across every worktree sub-map (tests use one
    /// root => one worktree key), for the v4 nested-frontier shape.
    fn frontier_paths(store: &ImpactStore) -> FxHashSet<String> {
        store
            .frontier
            .values()
            .flat_map(|m| m.keys().cloned())
            .collect()
    }

    /// All clone fingerprints across every worktree sub-map.
    fn clone_fingerprints(store: &ImpactStore) -> FxHashSet<String> {
        store
            .clone_frontier
            .values()
            .flat_map(|m| m.keys().cloned())
            .collect()
    }

    /// Seed raw bytes at the resolved (user-dir) store path, creating parent
    /// dirs, to exercise the load/parse path against hand-authored JSON.
    fn seed_store_raw(root: &Path, bytes: &[u8]) {
        let path = store_path(root).expect("test config dir set");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }

    fn summary(dead: usize, complexity: usize, dupes: usize) -> AuditSummary {
        AuditSummary {
            dead_code_issues: dead,
            dead_code_has_errors: dead > 0,
            complexity_findings: complexity,
            max_cyclomatic: None,
            duplication_clone_groups: dupes,
        }
    }

    /// Record a run with no per-finding attribution (v1 surfacing/trend/containment only).
    fn record_v1(
        root: &Path,
        summary: &AuditSummary,
        verdict: AuditVerdict,
        gate: bool,
        git_sha: Option<&str>,
        version: &str,
        timestamp: &str,
    ) {
        record_audit_run(
            root,
            summary,
            &AuditRunRecord {
                verdict,
                gate,
                git_sha,
                version,
                timestamp,
                attribution: None,
            },
        );
    }

    /// Create a real file under `root` (attribution prunes frontier entries for
    /// files that no longer exist, so test files must exist on disk).
    fn touch(root: &Path, rel: &str) -> PathBuf {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, b"x").unwrap();
        p
    }

    fn fi(path: &Path, kind: &'static str, symbol: &str) -> FindingInput {
        FindingInput {
            path: path.to_path_buf(),
            kind,
            symbol: Some(symbol.to_owned()),
        }
    }

    fn supp(path: &Path, kind: &str) -> ActiveSuppression {
        ActiveSuppression {
            path: path.to_path_buf(),
            kind: Some(kind.to_owned()),
            is_file_level: false,
        }
    }

    /// Record one attribution run against the store.
    fn run(
        root: &Path,
        changed: &[&Path],
        findings: Vec<FindingInput>,
        clones: Vec<CloneInput>,
        supps: &[ActiveSuppression],
        ts: &str,
    ) {
        let changed_files: Vec<PathBuf> = changed.iter().map(|p| p.to_path_buf()).collect();
        let input = AttributionInput {
            root,
            scope: Scope::ChangedFiles(&changed_files),
            findings,
            clones,
            suppressions: supps,
        };
        record_audit_run(
            root,
            &summary(0, 0, 0),
            &AuditRunRecord {
                verdict: AuditVerdict::Pass,
                gate: true,
                git_sha: Some("sha"),
                version: "2.0.0",
                timestamp: ts,
                attribution: Some(&input),
            },
        );
    }

    #[test]
    fn disabled_store_does_not_record() {
        let (_config, dir) = test_env();
        let root = dir.path();
        record_v1(
            root,
            &summary(3, 1, 0),
            AuditVerdict::Fail,
            true,
            Some("abc1234"),
            "2.0.0",
            "2026-05-29T10:00:00Z",
        );
        let store = load(root);
        assert!(store.records.is_empty());
        assert!(!store.enabled);
    }

    #[test]
    fn enable_and_disable_record_the_explicit_decision() {
        let (_config, dir) = test_env();
        let root = dir.path();
        assert!(!load(root).explicit_decision, "fresh store: never asked");

        // Declining on a never-enabled project is an explicit decision too.
        disable(root);
        let store = load(root);
        assert!(!store.enabled);
        assert!(store.explicit_decision);
        assert!(build_report(&store).explicit_decision);
    }

    #[test]
    fn due_digest_stamps_and_respects_interval_and_gates() {
        let (_config, dir) = test_env();
        let root = dir.path();

        // Disabled, or enabled with zero value: never due.
        assert!(take_due_digest(root).is_none());
        enable(root);
        assert!(take_due_digest(root).is_none(), "zero counters never nag");

        let mut store = load(root);
        store.resolved_total = 3;
        store.containment.push(ContainmentEvent {
            blocked_at: "2026-06-11T00:00:00Z".to_string(),
            cleared_at: "2026-06-11T00:05:00Z".to_string(),
            git_sha: None,
            blocked_counts: ImpactCounts::default(),
        });
        save(&store, root);

        let digest = take_due_digest(root).expect("first digest is due");
        assert_eq!(digest.containment_count, 1);
        assert_eq!(digest.resolved_total, 3);
        assert!(
            take_due_digest(root).is_none(),
            "stamped: not due again within the interval"
        );

        // An expired stamp makes it due again.
        let mut store = load(root);
        store.last_digest_epoch = Some(0);
        save(&store, root);
        assert!(take_due_digest(root).is_some());
    }

    #[test]
    fn decline_onboarding_persists_in_existing_store() {
        let (_config, dir) = test_env();
        let root = dir.path();

        assert!(decline_onboarding(root));
        assert!(!decline_onboarding(root));

        let store = load(root);
        assert!(store.onboarding_declined);
        assert_eq!(store.schema_version, STORE_SCHEMA_VERSION);
        // Decline persists in the user store and writes nothing into the repo.
        assert!(!root.join(".gitignore").exists());
        let report = build_report(&store);
        assert!(report.onboarding_declined);
    }

    #[test]
    fn enable_then_record_accrues_history() {
        let (_config, dir) = test_env();
        let root = dir.path();
        assert!(enable(root));
        assert!(!enable(root)); // second enable is a no-op-ish (already on)
        record_v1(
            root,
            &summary(2, 1, 0),
            AuditVerdict::Warn,
            false,
            None,
            "2.0.0",
            "2026-05-29T10:00:00Z",
        );
        let store = load(root);
        assert_eq!(store.records.len(), 1);
        assert_eq!(store.records[0].counts.total_issues, 3);
        assert_eq!(
            store.first_recorded.as_deref(),
            Some("2026-05-29T10:00:00Z")
        );
    }

    #[test]
    fn record_is_a_noop_in_ci() {
        // Impact is local-dev-only: it must never record on CI. The suite itself
        // runs on CI (where `CI` / `GITHUB_ACTIONS` are set), so the gate uses a
        // per-test override instead of the ambient env; here we force it true to
        // prove the production no-op. Without the gate, an enabled project would
        // record on every CI run.
        let (_config, dir) = test_env();
        let root = dir.path();
        assert!(enable(root));
        TEST_FORCE_CI.with(|c| c.set(true));
        record_v1(
            root,
            &summary(2, 1, 0),
            AuditVerdict::Warn,
            false,
            None,
            "2.0.0",
            "2026-05-29T10:00:00Z",
        );
        TEST_FORCE_CI.with(|c| c.set(false));
        let store = load(root);
        assert_eq!(store.records.len(), 0, "impact must not record while in CI");
    }

    #[test]
    fn enable_writes_nothing_into_the_repo() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        // The user-store relocation means enable never touches the repo: no
        // .gitignore mutation and no in-repo .fallow/ dir.
        assert!(
            !root.join(".gitignore").exists(),
            "enable must not create or modify the repo's .gitignore"
        );
        assert!(
            !root.join(".fallow").exists(),
            "enable must not create an in-repo .fallow/ dir"
        );
        // The decision IS persisted, in the user store.
        let store = load(root);
        assert!(store.enabled);
        assert!(store.explicit_decision);
        assert!(resolve_enabled(&store).0);
    }

    #[test]
    fn single_record_yields_no_trend_no_spike() {
        let mut store = ImpactStore {
            enabled: true,
            ..Default::default()
        };
        store.records.push(ImpactRecord {
            timestamp: "t0".into(),
            version: "2.0.0".into(),
            git_sha: None,
            verdict: "warn".into(),
            gate: false,
            counts: ImpactCounts {
                total_issues: 5,
                dead_code: 5,
                complexity: 0,
                duplication: 0,
            },
        });
        let report = build_report(&store);
        assert!(report.trend.is_none());
        assert_eq!(report.surfacing.unwrap().total_issues, 5);
    }

    #[test]
    fn empty_store_report_is_first_run() {
        let store = ImpactStore::default();
        let report = build_report(&store);
        assert_eq!(report.record_count, 0);
        assert!(report.trend.is_none());
        assert!(report.surfacing.is_none());
        let human = render_human(&report);
        assert!(human.contains("off")); // default store is disabled
    }

    #[test]
    fn enabled_empty_store_shows_check_back() {
        let store = ImpactStore {
            enabled: true,
            ..Default::default()
        };
        let report = build_report(&store);
        let human = render_human(&report);
        assert!(human.contains("No history yet"));
        assert!(!human.contains("0 issues"));
    }

    #[test]
    fn trend_improving_when_issues_drop() {
        let mut store = ImpactStore {
            enabled: true,
            ..Default::default()
        };
        for total in [8usize, 3usize] {
            store.records.push(ImpactRecord {
                timestamp: format!("t{total}"),
                version: "2.0.0".into(),
                git_sha: None,
                verdict: "warn".into(),
                gate: false,
                counts: ImpactCounts {
                    total_issues: total,
                    dead_code: total,
                    complexity: 0,
                    duplication: 0,
                },
            });
        }
        let report = build_report(&store);
        let trend = report.trend.unwrap();
        assert_eq!(trend.direction, ImpactTrendDirection::Improving);
        assert_eq!(trend.total_delta, -5);
    }

    #[test]
    fn containment_blocked_then_cleared_records_one_event() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        record_v1(
            root,
            &summary(2, 0, 0),
            AuditVerdict::Fail,
            true,
            Some("sha1"),
            "2.0.0",
            "t0",
        );
        let store = load(root);
        assert!(store.pending_containment.is_some());
        assert!(store.containment.is_empty());

        record_v1(
            root,
            &summary(0, 0, 0),
            AuditVerdict::Pass,
            true,
            Some("sha2"),
            "2.0.0",
            "t1",
        );
        let store = load(root);
        assert!(store.pending_containment.is_none());
        assert_eq!(store.containment.len(), 1);
        assert_eq!(store.containment[0].blocked_at, "t0");
        assert_eq!(store.containment[0].cleared_at, "t1");
    }

    #[test]
    fn non_gate_run_never_creates_containment() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        record_v1(
            root,
            &summary(2, 0, 0),
            AuditVerdict::Fail,
            false,
            None,
            "2.0.0",
            "t0",
        );
        let store = load(root);
        assert!(store.pending_containment.is_none());
        assert!(store.containment.is_empty());
    }

    #[test]
    fn corrupt_store_loads_as_default_no_panic() {
        let (_config, dir) = test_env();
        let root = dir.path();
        seed_store_raw(root, b"{ not valid json ][");
        let store = load(root);
        assert!(!store.enabled);
        assert!(store.records.is_empty());
        record_v1(
            root,
            &summary(1, 0, 0),
            AuditVerdict::Fail,
            true,
            None,
            "2.0.0",
            "t0",
        );
    }

    #[test]
    fn records_are_bounded() {
        let mut store = ImpactStore {
            enabled: true,
            ..Default::default()
        };
        for i in 0..(MAX_RECORDS + 50) {
            store.records.push(ImpactRecord {
                timestamp: format!("t{i}"),
                version: "2.0.0".into(),
                git_sha: None,
                verdict: "pass".into(),
                gate: false,
                counts: ImpactCounts::default(),
            });
        }
        compact(&mut store);
        assert_eq!(store.records.len(), MAX_RECORDS);
        assert_eq!(store.records[0].timestamp, "t50");
    }

    #[test]
    fn report_always_carries_schema_version() {
        let empty = build_report(&ImpactStore::default());
        assert_eq!(empty.schema_version, ImpactReportSchemaVersion::V1);
        let json = render_json(&empty);
        assert!(
            json.contains("\"schema_version\": \"1\""),
            "schema_version must be present (as the \"1\" const) even when disabled: {json}"
        );

        let mut store = ImpactStore {
            enabled: true,
            ..Default::default()
        };
        store.records.push(ImpactRecord {
            timestamp: "2026-05-29T10:00:00Z".into(),
            version: "2.0.0".into(),
            git_sha: None,
            verdict: "pass".into(),
            gate: false,
            counts: ImpactCounts::default(),
        });
        assert_eq!(
            build_report(&store).schema_version,
            ImpactReportSchemaVersion::V1
        );
    }

    #[test]
    fn date_only_trims_iso_timestamp() {
        assert_eq!(date_only("2026-05-29T18:15:23Z"), "2026-05-29");
        assert_eq!(date_only("2026-05-29"), "2026-05-29");
        assert_eq!(date_only("the first run"), "the first run");
    }

    #[test]
    fn human_footer_shows_date_only() {
        let mut store = ImpactStore {
            enabled: true,
            ..Default::default()
        };
        store.first_recorded = Some("2026-05-29T18:15:23Z".into());
        store.records.push(ImpactRecord {
            timestamp: "2026-05-29T18:15:23Z".into(),
            version: "2.0.0".into(),
            git_sha: None,
            verdict: "pass".into(),
            gate: false,
            counts: ImpactCounts::default(),
        });
        let report = build_report(&store);
        let human = render_human(&report);
        assert!(
            human.contains("since 2026-05-29.") && !human.contains("18:15:23"),
            "human footer must show date-only: {human}"
        );
        let md = render_markdown(&report);
        assert!(
            md.contains("since 2026-05-29.") && !md.contains("18:15:23"),
            "markdown footer must show date-only: {md}"
        );
    }

    #[test]
    fn future_schema_version_store_loads_without_panic_or_loss() {
        let (_config, dir) = test_env();
        let root = dir.path();
        let future = format!(
            "{{\"schema_version\":{},\"enabled\":true,\"records\":[],\"containment\":[]}}",
            STORE_SCHEMA_VERSION + 1
        );
        seed_store_raw(root, future.as_bytes());
        let store = load(root);
        assert_eq!(store.schema_version, STORE_SCHEMA_VERSION + 1);
        assert!(
            store.enabled,
            "future-version store must not degrade to default"
        );
    }

    #[test]
    fn removed_finding_is_credited_as_resolved() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        run(
            root,
            &[&a],
            vec![fi(&a, "unused-export", "foo")],
            vec![],
            &[],
            "t0",
        );
        assert_eq!(
            load(root).resolved_total,
            0,
            "first run only establishes a baseline"
        );
        run(root, &[&a], vec![], vec![], &[], "t1");
        let store = load(root);
        assert_eq!(store.resolved_total, 1);
        assert_eq!(store.suppressed_total, 0);
        assert_eq!(store.recent_resolved.len(), 1);
        assert_eq!(store.recent_resolved[0].kind, "unused-export");
        assert_eq!(store.recent_resolved[0].symbol.as_deref(), Some("foo"));
        assert_eq!(store.recent_resolved[0].path, "src/a.ts");
    }

    #[test]
    fn suppressed_finding_is_not_a_win() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        run(
            root,
            &[&a],
            vec![fi(&a, "unused-export", "foo")],
            vec![],
            &[],
            "t0",
        );
        run(
            root,
            &[&a],
            vec![],
            vec![],
            &[supp(&a, "unused-export")],
            "t1",
        );
        let store = load(root);
        assert_eq!(
            store.resolved_total, 0,
            "a suppression must never count as a win"
        );
        assert_eq!(store.suppressed_total, 1);
    }

    #[test]
    fn fix_and_suppress_same_kind_credits_zero_resolved() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        run(
            root,
            &[&a],
            vec![
                fi(&a, "unused-export", "foo"),
                fi(&a, "unused-export", "bar"),
            ],
            vec![],
            &[],
            "t0",
        );
        run(
            root,
            &[&a],
            vec![],
            vec![],
            &[supp(&a, "unused-export")],
            "t1",
        );
        let store = load(root);
        assert_eq!(store.resolved_total, 0);
        assert_eq!(store.suppressed_total, 2);
    }

    #[test]
    fn within_file_move_is_not_resolved() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        run(
            root,
            &[&a],
            vec![fi(&a, "unused-export", "foo")],
            vec![],
            &[],
            "t0",
        );
        run(
            root,
            &[&a],
            vec![fi(&a, "unused-export", "foo")],
            vec![],
            &[],
            "t1",
        );
        let store = load(root);
        assert_eq!(store.resolved_total, 0);
        assert_eq!(store.suppressed_total, 0);
    }

    #[test]
    fn cross_file_move_in_same_run_is_not_resolved() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        let b = touch(root, "src/b.ts");
        run(
            root,
            &[&a],
            vec![fi(&a, "unused-export", "foo")],
            vec![],
            &[],
            "t0",
        );
        run(
            root,
            &[&a, &b],
            vec![fi(&b, "unused-export", "foo")],
            vec![],
            &[],
            "t1",
        );
        assert_eq!(
            load(root).resolved_total,
            0,
            "a cross-file move is not a resolution"
        );
    }

    #[test]
    fn cross_run_move_uncredits_the_prior_resolution() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        let b = touch(root, "src/b.ts");
        run(
            root,
            &[&a],
            vec![fi(&a, "unused-export", "foo")],
            vec![],
            &[],
            "t0",
        );
        run(root, &[&a], vec![], vec![], &[], "t1");
        assert_eq!(
            load(root).resolved_total,
            1,
            "source disappearance credited in run A"
        );
        run(
            root,
            &[&b],
            vec![fi(&b, "unused-export", "foo")],
            vec![],
            &[],
            "t2",
        );
        let store = load(root);
        assert_eq!(
            store.resolved_total, 0,
            "cross-run move must un-credit the phantom win"
        );
        assert!(
            store.recent_resolved.is_empty(),
            "the stale resolution event is dropped"
        );
    }

    #[test]
    fn resolved_complexity_finding_and_suppressed_complexity() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        run(
            root,
            &[&a],
            vec![fi(&a, "complexity", "bigFn")],
            vec![],
            &[],
            "t0",
        );
        run(root, &[&a], vec![], vec![], &[supp(&a, "complexity")], "t1");
        let store = load(root);
        assert_eq!(store.resolved_total, 0);
        assert_eq!(store.suppressed_total, 1);

        let b = touch(root, "src/b.ts");
        run(
            root,
            &[&b],
            vec![fi(&b, "complexity", "huge")],
            vec![],
            &[],
            "t2",
        );
        run(root, &[&b], vec![], vec![], &[], "t3");
        assert_eq!(load(root).resolved_total, 1);
    }

    #[test]
    fn resolved_duplication_clone_group() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        let b = touch(root, "src/b.ts");
        let clone = CloneInput {
            fingerprint: "dup:abc12345".to_owned(),
            instance_paths: vec![a.clone(), b],
        };
        run(root, &[&a], vec![], vec![clone], &[], "t0");
        run(root, &[&a], vec![], vec![], &[], "t1");
        let store = load(root);
        assert_eq!(store.resolved_total, 1);
        assert_eq!(store.recent_resolved[0].kind, "code-duplication");
    }

    #[test]
    fn blanket_suppression_covers_any_kind() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        run(
            root,
            &[&a],
            vec![fi(&a, "unused-export", "foo")],
            vec![],
            &[],
            "t0",
        );
        let blanket = ActiveSuppression {
            path: a.clone(),
            kind: None,
            is_file_level: true,
        };
        run(root, &[&a], vec![], vec![], &[blanket], "t1");
        let store = load(root);
        assert_eq!(store.resolved_total, 0);
        assert_eq!(store.suppressed_total, 1);
    }

    #[test]
    fn v1_store_loads_and_upgrades_to_v2() {
        let (_config, dir) = test_env();
        let root = dir.path();
        let v1 = r#"{"schema_version":1,"enabled":true,"first_recorded":"t0","records":[{"timestamp":"t0","version":"2.0.0","verdict":"warn","gate":false,"counts":{"total_issues":1,"dead_code":1,"complexity":0,"duplication":0}}],"containment":[]}"#;
        seed_store_raw(root, v1.as_bytes());
        let store = load(root);
        assert_eq!(store.schema_version, 1);
        assert!(store.frontier.is_empty());
        assert_eq!(store.resolved_total, 0);
        let a = touch(root, "src/a.ts");
        run(
            root,
            &[&a],
            vec![fi(&a, "unused-export", "foo")],
            vec![],
            &[],
            "t1",
        );
        let store = load(root);
        assert_eq!(store.schema_version, STORE_SCHEMA_VERSION);
        assert!(frontier_paths(&store).contains("src/a.ts"));
    }

    #[test]
    fn recent_resolved_is_bounded() {
        let mut store = ImpactStore {
            enabled: true,
            ..Default::default()
        };
        for i in 0..(MAX_RECENT_RESOLVED + 25) {
            store.recent_resolved.push(ResolutionEvent {
                kind: "unused-export".into(),
                path: format!("src/f{i}.ts"),
                symbol: Some(format!("s{i}")),
                git_sha: None,
                timestamp: format!("t{i}"),
            });
        }
        bound_recent_resolved(&mut store);
        assert_eq!(store.recent_resolved.len(), MAX_RECENT_RESOLVED);
        assert_eq!(store.recent_resolved[0].path, "src/f25.ts");
    }

    #[test]
    fn frontier_prunes_deleted_files() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        run(
            root,
            &[&a],
            vec![fi(&a, "unused-export", "foo")],
            vec![],
            &[],
            "t0",
        );
        assert!(frontier_paths(&load(root)).contains("src/a.ts"));
        std::fs::remove_file(&a).unwrap();
        let b = touch(root, "src/b.ts");
        run(root, &[&b], vec![], vec![], &[], "t1");
        assert!(!frontier_paths(&load(root)).contains("src/a.ts"));
    }

    #[test]
    fn honest_empty_state_before_attribution_baseline() {
        let store = ImpactStore {
            enabled: true,
            records: vec![ImpactRecord {
                timestamp: "t0".into(),
                version: "2.0.0".into(),
                git_sha: None,
                verdict: "warn".into(),
                gate: false,
                counts: ImpactCounts::default(),
            }],
            ..Default::default()
        };
        let report = build_report(&store);
        assert!(!report.attribution_active);
        let human = render_human(&report);
        assert!(human.contains("resolution tracking starts from your next gate run"));
        assert!(!human.contains("0 finding"));
    }

    #[test]
    fn suppression_only_state_renders_under_a_resolved_header() {
        let report = ImpactReport {
            schema_version: ImpactReportSchemaVersion::V1,
            enabled: true,
            enabled_source: EnabledSource::Project,
            record_count: 2,
            meta: None,
            first_recorded: Some("2026-05-29T10:00:00Z".into()),
            latest_git_sha: None,
            surfacing: Some(ImpactCounts::default()),
            trend: None,
            project_surfacing: None,
            project_trend: None,
            containment_count: 0,
            recent_containment: vec![],
            resolved_total: 0,
            suppressed_total: 2,
            recent_resolved: vec![],
            attribution_active: true,
            onboarding_declined: false,
            explicit_decision: false,
        };
        let human = render_human(&report);
        let resolved_idx = human.find("  RESOLVED").expect("RESOLVED header present");
        let supp_idx = human
            .find("2 findings you marked intentional")
            .expect("suppression line present");
        assert!(
            resolved_idx < supp_idx,
            "suppression must render under RESOLVED"
        );
        assert!(human.contains("none yet"));

        let md = render_markdown(&report);
        assert!(
            md.contains("- **Resolved:**"),
            "markdown always has a Resolved bullet"
        );
        assert!(md.contains("- **Marked intentional:** 2 finding"));
    }

    /// Build a `CloneInput` over real absolute paths (built from `root`).
    fn clone_at(fingerprint: &str, paths: &[&Path]) -> CloneInput {
        CloneInput {
            fingerprint: fingerprint.to_owned(),
            instance_paths: paths.iter().map(|p| p.to_path_buf()).collect(),
        }
    }

    /// Record a WHOLE-PROJECT run via the real combined-track recorder
    /// (`record_combined_run` with `Scope::WholeProject`), exercising the same
    /// path `combined.rs` uses on a full `fallow` run.
    fn run_wp(
        root: &Path,
        findings: Vec<FindingInput>,
        clones: Vec<CloneInput>,
        supps: &[ActiveSuppression],
        ts: &str,
    ) {
        let input = AttributionInput {
            root,
            scope: Scope::WholeProject,
            findings,
            clones,
            suppressions: supps,
        };
        record_combined_run(
            root,
            ImpactCounts::default(),
            Some("sha"),
            "2.0.0",
            ts,
            Some(&input),
        );
    }

    #[test]
    fn whole_project_run_does_not_double_credit_after_audit() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        let b = touch(root, "src/b.ts");
        run(
            root,
            &[&a, &b],
            vec![],
            vec![clone_at("dup:abc", &[&a, &b])],
            &[],
            "t1",
        );
        assert_eq!(clone_fingerprints(&load(root)).len(), 1);

        run(root, &[&a, &b], vec![], vec![], &[], "t2");
        assert_eq!(load(root).resolved_total, 1);
        assert!(load(root).clone_frontier.is_empty());

        run_wp(root, vec![], vec![], &[], "t3");
        assert_eq!(
            load(root).resolved_total,
            1,
            "whole-project run re-credited a resolution"
        );
    }

    #[test]
    fn whole_project_run_credits_suppressed_not_resolved() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let util = touch(root, "src/util.ts");
        run(
            root,
            &[&util],
            vec![fi(&util, "unused-export", "dead")],
            vec![],
            &[],
            "t1",
        );
        assert_eq!(frontier_paths(&load(root)).len(), 1);

        run_wp(root, vec![], vec![], &[supp(&util, "unused-export")], "t2");
        let store = load(root);
        assert_eq!(
            store.suppressed_total, 1,
            "suppressed finding not counted suppressed"
        );
        assert_eq!(
            store.resolved_total, 0,
            "suppressed finding wrongly counted resolved"
        );
    }

    #[test]
    fn clone_reshape_three_to_two_not_credited_as_resolved() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        let a = touch(root, "src/a.ts");
        let b = touch(root, "src/b.ts");
        let c = touch(root, "src/c.ts");
        run(
            root,
            &[&a, &b, &c],
            vec![],
            vec![clone_at("dup:aaa", &[&a, &b, &c])],
            &[],
            "t1",
        );
        assert_eq!(clone_fingerprints(&load(root)).len(), 1);

        run_wp(
            root,
            vec![],
            vec![clone_at("dup:bbb", &[&a, &b])],
            &[],
            "t2",
        );
        let store = load(root);
        assert_eq!(
            store.resolved_total, 0,
            "clone reshape miscredited as resolved"
        );
        assert!(clone_fingerprints(&store).contains("dup:bbb"));
        assert!(!clone_fingerprints(&store).contains("dup:aaa"));
    }

    fn rcounts(total: usize, dead: usize, complexity: usize, dup: usize) -> ImpactCounts {
        ImpactCounts {
            total_issues: total,
            dead_code: dead,
            complexity,
            duplication: dup,
        }
    }

    fn rtrend(prev: usize, cur: usize) -> TrendSummary {
        TrendSummary {
            direction: direction_for(cur as i64 - prev as i64),
            total_delta: cur as i64 - prev as i64,
            previous_total: prev,
            current_total: cur,
        }
    }

    /// Build a report literal for render-state tests.
    fn rreport(
        record_count: usize,
        first_recorded: Option<&str>,
        surfacing: Option<ImpactCounts>,
        trend: Option<TrendSummary>,
        project_surfacing: Option<ImpactCounts>,
        project_trend: Option<TrendSummary>,
        attribution_active: bool,
    ) -> ImpactReport {
        ImpactReport {
            schema_version: ImpactReportSchemaVersion::V1,
            enabled: true,
            enabled_source: EnabledSource::Project,
            record_count,
            meta: None,
            first_recorded: first_recorded.map(ToOwned::to_owned),
            latest_git_sha: None,
            surfacing,
            trend,
            project_surfacing,
            project_trend,
            containment_count: 0,
            recent_containment: vec![],
            resolved_total: 0,
            suppressed_total: 0,
            recent_resolved: vec![],
            attribution_active,
            onboarding_declined: false,
            explicit_decision: false,
        }
    }

    #[test]
    fn render_human_project_only_store_shows_whole_project_not_empty_state() {
        let r = rreport(
            0,
            Some("2026-05-30T10:00:00Z"),
            None,
            None,
            Some(rcounts(1, 1, 0, 0)),
            None,
            true,
        );
        let human = render_human(&r);
        assert!(
            human.contains("WHOLE PROJECT (whole-repo context, not a to-do)"),
            "project-only must render the labeled section"
        );
        assert!(human.contains("1 issue across the whole project"));
        assert!(
            human.contains("project trend starts after your next full `fallow` run"),
            "single project record => no trend line, shows the next-run hint"
        );
        assert!(human.contains("Tracking since 2026-05-30"));
        assert!(
            !human.contains("No history yet"),
            "must not show the empty-state copy"
        );
        assert!(
            !human.contains("LATEST RUN"),
            "no changed-file track recorded"
        );
        assert!(
            !human.contains("recorded audit run"),
            "no audit runs => no changed-file footer"
        );
    }

    #[test]
    fn render_human_both_tracks_label_actionable_vs_context() {
        let r = rreport(
            3,
            Some("2026-05-29T10:00:00Z"),
            Some(rcounts(4, 4, 0, 0)),
            Some(rtrend(6, 4)),
            Some(rcounts(40, 30, 5, 5)),
            Some(rtrend(45, 40)),
            true,
        );
        let human = render_human(&r);
        let latest = human
            .find("LATEST RUN (changed files, act on these now)")
            .expect("LATEST RUN labeled actionable");
        let whole = human
            .find("WHOLE PROJECT (whole-repo context, not a to-do)")
            .expect("WHOLE PROJECT labeled context");
        assert!(
            latest < whole,
            "changed-file section renders before whole-project"
        );
        assert!(human.contains("45 -> 40 (down) across your last two full runs"));
        assert!(human.contains("advances only on your local full `fallow` runs, not CI"));
    }

    #[test]
    fn render_markdown_project_only_store_shows_whole_project_not_empty_state() {
        let r = rreport(
            0,
            Some("2026-05-30T10:00:00Z"),
            None,
            None,
            Some(rcounts(1, 1, 0, 0)),
            None,
            true,
        );
        let md = render_markdown(&r);
        assert!(
            md.contains(
                "- **Whole project (whole-repo context, last full `fallow` run):** 1 issue"
            ),
            "project-only md must render the labeled whole-project line"
        );
        assert!(
            !md.contains("No history yet"),
            "project-only md must not show empty state"
        );
        assert!(md.contains("Tracking since 2026-05-30"));
    }

    #[test]
    fn resolve_enabled_precedence_table() {
        let (_config, _dir) = test_env();
        // enabled-true is an explicit project opt-in regardless of the flag.
        let on = ImpactStore {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(resolve_enabled(&on), (true, EnabledSource::Project));

        // explicitly disabled here stays off as a Project decision.
        let off_explicit = ImpactStore {
            enabled: false,
            explicit_decision: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_enabled(&off_explicit),
            (false, EnabledSource::Project)
        );

        // never-asked + no global default => off (Default).
        let never = ImpactStore::default();
        assert_eq!(resolve_enabled(&never), (false, EnabledSource::Default));

        // never-asked + global default on => on (User).
        assert!(set_global_default(true));
        assert_eq!(resolve_enabled(&never), (true, EnabledSource::User));
        // a per-repo disable still wins over the global default.
        assert_eq!(
            resolve_enabled(&off_explicit),
            (false, EnabledSource::Project)
        );
    }

    #[test]
    fn human_report_explains_user_global_default() {
        let (_config, _dir) = test_env();
        set_global_default(true);
        // A never-asked store resolved on a project: enabled via the global default.
        let report = build_report(&ImpactStore::default());
        assert_eq!(report.enabled_source, EnabledSource::User);
        let human = render_human(&report);
        assert!(
            human.contains("Enabled by your user-global default"),
            "human report must explain a global-default enable: {human}"
        );
        // A project-enabled report does NOT show the global-default note.
        let project = build_report(&ImpactStore {
            enabled: true,
            explicit_decision: true,
            ..Default::default()
        });
        assert_eq!(project.enabled_source, EnabledSource::Project);
        assert!(!render_human(&project).contains("user-global default"));
    }

    #[test]
    fn global_default_round_trips() {
        let (_config, _dir) = test_env();
        assert!(!load_global_default());
        assert!(set_global_default(true));
        assert!(load_global_default());
        assert!(!set_global_default(true)); // unchanged
        assert!(set_global_default(false));
        assert!(!load_global_default());
    }

    #[test]
    fn global_default_records_without_per_repo_enable() {
        let (_config, dir) = test_env();
        let root = dir.path();
        set_global_default(true);
        // No `enable(root)` call: the global default alone should activate.
        record_v1(
            root,
            &summary(2, 0, 0),
            AuditVerdict::Warn,
            false,
            None,
            "2.0.0",
            "t0",
        );
        let report = build_report(&load(root));
        assert!(report.enabled);
        assert_eq!(report.enabled_source, EnabledSource::User);
        assert_eq!(report.record_count, 1);
    }

    #[test]
    fn legacy_in_repo_store_is_migrated_on_first_load() {
        let (_config, dir) = test_env();
        let root = dir.path();
        // Seed a pre-relocation v3 store with a FLAT frontier in the repo.
        let legacy = r#"{"schema_version":3,"enabled":true,"explicit_decision":true,
            "records":[{"timestamp":"t0","version":"2.0.0","verdict":"warn","gate":false,
            "counts":{"total_issues":1,"dead_code":1,"complexity":0,"duplication":0}}],
            "resolved_total":2,
            "frontier":{"src/a.ts":{"findings":[{"id":"x","kind":"unused-export","symbol":"foo"}],"suppressions":[]}},
            "containment":[]}"#;
        std::fs::create_dir_all(root.join(".fallow")).unwrap();
        std::fs::write(legacy_store_path(root), legacy).unwrap();

        let store = load(root);
        assert!(store.enabled);
        assert_eq!(store.schema_version, STORE_SCHEMA_VERSION);
        assert_eq!(store.records.len(), 1);
        assert_eq!(store.resolved_total, 2);
        // The flat frontier was wrapped under the worktree key (nested v4 shape).
        assert!(frontier_paths(&store).contains("src/a.ts"));
        // The user store now exists, so a second load does NOT re-import (it
        // reads the user store directly).
        assert!(store_path(root).is_some_and(|p| p.exists()));
        let again = load(root);
        assert_eq!(again.records.len(), 1);
    }

    #[test]
    fn reset_removes_only_this_project() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        record_v1(
            root,
            &summary(1, 0, 0),
            AuditVerdict::Warn,
            false,
            None,
            "2.0.0",
            "t0",
        );
        assert_eq!(load(root).records.len(), 1);
        assert!(reset(root));
        assert!(load(root).records.is_empty());
        assert!(!reset(root)); // already gone
    }

    #[test]
    fn reset_all_clears_dir_but_keeps_global_default() {
        let (_config, dir) = test_env();
        let root = dir.path();
        set_global_default(true);
        enable(root);
        assert!(load(root).enabled);
        assert!(reset_all());
        // The global default toggle survives a data wipe.
        assert!(load_global_default());
    }

    // ----- cross-repo aggregate (`impact --all`) tests --------------------

    /// Set an isolated config dir (no project root needed) and return its guard.
    fn aggregate_env() -> tempfile::TempDir {
        let config = tempfile::tempdir().unwrap();
        TEST_CONFIG_DIR.with(|c| *c.borrow_mut() = Some(config.path().to_path_buf()));
        config
    }

    /// Write a store file directly under `<config>/impact/<key>.json`.
    fn seed_store(key: &str, store: &ImpactStore) {
        let dir = impact_config_dir().unwrap().join("impact");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{key}.json")),
            serde_json::to_string_pretty(store).unwrap(),
        )
        .unwrap();
    }

    fn store_with(
        label: &str,
        resolved: usize,
        contained: usize,
        latest_ts: &str,
        latest_issues: usize,
    ) -> ImpactStore {
        let mut s = ImpactStore {
            enabled: true,
            explicit_decision: true,
            resolved_total: resolved,
            label: Some(label.to_owned()),
            ..Default::default()
        };
        s.records.push(ImpactRecord {
            timestamp: latest_ts.to_owned(),
            version: "2.0.0".to_owned(),
            git_sha: None,
            verdict: "warn".to_owned(),
            gate: false,
            counts: ImpactCounts::from_combined(latest_issues, 0, 0),
        });
        for _ in 0..contained {
            s.containment.push(ContainmentEvent {
                blocked_at: "t0".to_owned(),
                cleared_at: "t1".to_owned(),
                git_sha: None,
                blocked_counts: ImpactCounts::default(),
            });
        }
        s
    }

    #[test]
    fn repo_basename_returns_last_component_only() {
        assert_eq!(
            repo_basename(Path::new("/a/b/myrepo/.git")).as_deref(),
            Some("myrepo")
        );
        assert_eq!(
            repo_basename(Path::new("/a/b/proj")).as_deref(),
            Some("proj")
        );
        // Never a separator in the result.
        let name = repo_basename(Path::new("/x/y/z/.git")).unwrap();
        assert!(!name.contains('/') && !name.contains('\\'));
    }

    #[test]
    fn aggregate_rolls_up_totals_and_excludes_empty() {
        let _cfg = aggregate_env();
        seed_store(
            "aaa",
            &store_with("alpha", 10, 2, "2026-06-10T00:00:00Z", 3),
        );
        seed_store("bbb", &store_with("beta", 5, 1, "2026-06-11T00:00:00Z", 0));
        // enabled-but-empty: no records, no resolved, no containment.
        seed_store(
            "ccc",
            &ImpactStore {
                enabled: true,
                explicit_decision: true,
                label: Some("gamma".into()),
                ..Default::default()
            },
        );
        let report = aggregate(CrossRepoSort::Recent);
        assert_eq!(report.project_count, 3, "all three stores enumerated");
        assert_eq!(report.tracked_count, 2, "empty store excluded from rows");
        assert_eq!(report.totals.resolved_total, 15);
        assert_eq!(report.totals.containment_count, 3);
        assert_eq!(report.unreadable_count, 0);
    }

    #[test]
    fn aggregate_sort_recent_orders_by_last_activity() {
        let _cfg = aggregate_env();
        seed_store("old", &store_with("older", 1, 0, "2026-06-01T00:00:00Z", 1));
        seed_store("new", &store_with("newer", 1, 0, "2026-06-12T00:00:00Z", 1));
        let report = aggregate(CrossRepoSort::Recent);
        assert_eq!(report.projects[0].label.as_deref(), Some("newer"));
        assert_eq!(report.projects[1].label.as_deref(), Some("older"));
    }

    #[test]
    fn cross_repo_json_carries_kind_and_leaks_no_path() {
        let _cfg = aggregate_env();
        seed_store("aaa", &store_with("alpha", 4, 1, "2026-06-10T00:00:00Z", 2));
        let report = aggregate(CrossRepoSort::Recent);
        let json = render_cross_repo_json(&report);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["kind"], "impact-cross-repo");
        // No label (or any string) may contain a path separator.
        for entry in value["projects"].as_array().unwrap() {
            if let Some(label) = entry["label"].as_str() {
                assert!(
                    !label.contains('/') && !label.contains('\\'),
                    "label must be a basename, got {label}"
                );
            }
        }
        assert!(
            !json.contains('/') || !json.contains("Users"),
            "json must not leak an absolute home path"
        );
    }

    #[test]
    fn cross_repo_corrupt_file_is_skipped_and_counted() {
        let _cfg = aggregate_env();
        seed_store("good", &store_with("good", 3, 0, "2026-06-10T00:00:00Z", 1));
        let dir = impact_config_dir().unwrap().join("impact");
        std::fs::write(dir.join("bad.json"), b"{ not valid json ][").unwrap();
        let report = aggregate(CrossRepoSort::Recent);
        assert_eq!(report.tracked_count, 1, "good store still aggregated");
        assert_eq!(
            report.unreadable_count, 1,
            "corrupt file counted, not crashed"
        );
    }

    #[test]
    fn cross_repo_empty_dir_is_first_run() {
        let _cfg = aggregate_env();
        let report = aggregate(CrossRepoSort::Recent);
        assert_eq!(report.project_count, 0);
        let human = render_cross_repo_human(&report, None);
        assert!(human.contains("No projects tracked yet"));
    }

    #[test]
    fn cross_repo_all_corrupt_reports_unreadable_not_first_run() {
        let _cfg = aggregate_env();
        let dir = impact_config_dir().unwrap().join("impact");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.json"), b"{ broken ][").unwrap();
        let report = aggregate(CrossRepoSort::Recent);
        assert_eq!(report.project_count, 0);
        assert_eq!(report.unreadable_count, 1);
        let human = render_cross_repo_human(&report, None);
        assert!(
            human.contains("unreadable store") && !human.contains("No projects tracked yet"),
            "all-corrupt must report unreadable, not a misleading first-run hint: {human}"
        );
    }

    #[test]
    fn record_audit_run_captures_basename_label() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        record_v1(
            root,
            &summary(1, 0, 0),
            AuditVerdict::Warn,
            false,
            None,
            "2.0.0",
            "t0",
        );
        let label = load(root).label.expect("label captured on record");
        assert!(
            !label.contains('/') && !label.contains('\\'),
            "label must be a basename, got {label}"
        );
    }

    // ----- store advisory lock + age-based GC --------------------------------

    #[test]
    fn lock_path_appends_lock_suffix() {
        assert_eq!(
            lock_path_for(Path::new("/c/fallow/impact/abc.json")),
            PathBuf::from("/c/fallow/impact/abc.json.lock")
        );
    }

    #[test]
    fn store_lock_acquire_drop_then_record_roundtrips() {
        let (_config, dir) = test_env();
        let root = dir.path();
        enable(root);
        // Acquiring + dropping the lock around a record must not deadlock and
        // the record must persist.
        {
            let _lock = ImpactStoreLock::acquire(root).expect("lock acquires");
        }
        record_v1(
            root,
            &summary(1, 0, 0),
            AuditVerdict::Warn,
            false,
            None,
            "2.0.0",
            "t0",
        );
        assert_eq!(load(root).records.len(), 1, "record persisted under lock");
        // The lock sidecar lives next to the store and is never the store itself.
        let store = store_path(root).unwrap();
        assert!(lock_path_for(&store).exists(), "lock sidecar created");
        assert!(store.exists(), "store file is distinct from its lock");
    }

    #[test]
    fn sweep_keeps_fresh_and_self_deletes_aged_out() {
        let _cfg = aggregate_env();
        seed_store("keepme", &store_with("keep", 1, 0, "t0", 1));
        seed_store("oldone", &store_with("old", 1, 0, "t0", 1));
        // A `.lock` sidecar must survive the sweep (lock-lifecycle invariant).
        let lock = impact_config_dir()
            .unwrap()
            .join("impact")
            .join("oldone.json.lock");
        std::fs::write(&lock, b"").unwrap();

        // max_age = 0 ages out every non-kept store regardless of mtime.
        sweep_old_stores("keepme", std::time::Duration::ZERO);

        let dir = impact_config_dir().unwrap().join("impact");
        assert!(dir.join("keepme.json").exists(), "kept store survives");
        assert!(
            !dir.join("oldone.json").exists(),
            "aged-out store reclaimed"
        );
        assert!(lock.exists(), "lock sidecar never deleted by the sweep");
    }

    #[test]
    fn sweep_keeps_everything_under_a_large_window() {
        let _cfg = aggregate_env();
        seed_store("a", &store_with("a", 1, 0, "t0", 1));
        seed_store("b", &store_with("b", 1, 0, "t0", 1));
        // 10-year window: freshly-written stores are never aged out.
        sweep_old_stores("a", std::time::Duration::from_hours(10 * 365 * 24));
        let dir = impact_config_dir().unwrap().join("impact");
        assert!(dir.join("a.json").exists());
        assert!(dir.join("b.json").exists());
    }
}
