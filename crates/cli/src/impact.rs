//! Fallow Impact: local, opt-in value reporting.

use std::path::{Path, PathBuf};

use fallow_engine::changed_files::ChangedFilesError;
pub use fallow_output::{
    ContainmentEvent, CrossRepoImpactReport, CrossRepoImpactSchemaVersion, CrossRepoProjectEntry,
    CrossRepoTotals, EnabledSource, GateRunCounts, ImpactCounts, ImpactReport,
    ImpactReportSchemaVersion, ImpactTrendDirection, ResolutionEvent, TrendSummary,
};
use fallow_types::results::{ActiveSuppression, AnalysisResults};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use crate::audit::{AuditSummary, AuditVerdict};
use crate::report::format_display_path;

/// Version 7 replaced the per-record `gate: bool` with `gate_source`; older
/// stores migrate on read (see [`StoredImpactRecord`]).
const STORE_SCHEMA_VERSION: u32 = 7;

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

fn impact_counts_from_summary(summary: &AuditSummary) -> ImpactCounts {
    ImpactCounts {
        total_issues: summary.dead_code_issues
            + summary.complexity_findings
            + summary.duplication_clone_groups,
        dead_code: summary.dead_code_issues,
        complexity: summary.complexity_findings,
        duplication: summary.duplication_clone_groups,
    }
}

/// Which gate produced a recorded run. The store is local and never leaves the
/// machine, so this is run provenance ("where do my gate runs come from"), not
/// an adoption signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateSource {
    /// The agent gate hook (`--gate-marker agent`).
    Agent,
    /// The generated git pre-commit hook (`--gate-marker pre-commit`).
    PreCommit,
    /// A CI gate (`--gate-marker ci`).
    Ci,
    /// A gate run whose marker this build does not recognise, plus every gate
    /// run recorded before the store kept its source.
    Unknown,
}

impl GateSource {
    /// Classify the `--gate-marker` string the installers pass. An unrecognised
    /// marker is still a gate run, recorded as [`GateSource::Unknown`].
    #[must_use]
    pub fn from_marker(marker: &str) -> Self {
        match marker {
            "agent" => Self::Agent,
            "pre-commit" => Self::PreCommit,
            "ci" => Self::Ci,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "StoredImpactRecord")]
pub struct ImpactRecord {
    pub timestamp: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    pub verdict: String,
    /// The gate this run came from, `None` for a run no gate produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_source: Option<GateSource>,
    pub counts: ImpactCounts,
}

/// On-disk shape of [`ImpactRecord`], carrying both the schema 6 `gate: bool`
/// and the schema 7 `gate_source`. Every read goes through it, so a store an
/// older fallow wrote keeps its gate runs instead of silently losing them.
#[derive(Deserialize)]
struct StoredImpactRecord {
    timestamp: String,
    version: String,
    #[serde(default)]
    git_sha: Option<String>,
    verdict: String,
    #[serde(default)]
    gate: bool,
    #[serde(default)]
    gate_source: Option<GateSource>,
    counts: ImpactCounts,
}

impl From<StoredImpactRecord> for ImpactRecord {
    fn from(stored: StoredImpactRecord) -> Self {
        // Migration arm, store schema 6 -> 7: a legacy `gate: true` was a gate
        // run whose source was never stored, so it becomes `Unknown`; `false`
        // was not a gate run at all, so it becomes `None`.
        let gate_source = stored
            .gate_source
            .or_else(|| stored.gate.then_some(GateSource::Unknown));
        Self {
            timestamp: stored.timestamp,
            version: stored.version,
            git_sha: stored.git_sha,
            verdict: stored.verdict,
            gate_source,
            counts: stored.counts,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingContainment {
    pub blocked_at: String,
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
    /// Compatibility identity shared by every comparable record in this store.
    /// Legacy stores deserialize as syntactic analysis.
    #[serde(default)]
    pub analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity,
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
            analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity::default(),
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
    fallow_output::codeclimate_fingerprint_hash(&[normalized.as_str()])
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

/// The git common dir and toplevel for `root`, from the result of the combined
/// probe.
///
/// One git call answers both in a work tree. Outside a repository, or without
/// git, each single probe fails in the same way, so both paths fall back to
/// the root and no more git processes start. After another git error, for
/// example in a bare repository, each single probe decides on its own.
fn identity_paths_from(
    root: &Path,
    combined: Result<(PathBuf, PathBuf), ChangedFilesError>,
    common_dir: impl FnOnce() -> Option<PathBuf>,
    toplevel: impl FnOnce() -> Option<PathBuf>,
) -> (PathBuf, PathBuf) {
    match combined {
        Ok(paths) => paths,
        Err(ChangedFilesError::NotARepository | ChangedFilesError::GitMissing(_)) => {
            let fallback = resolve_or_root(None, root);
            (fallback.clone(), fallback)
        }
        Err(_) => (
            resolve_or_root(common_dir(), root),
            resolve_or_root(toplevel(), root),
        ),
    }
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
    let (common, toplevel) = identity_paths_from(
        root,
        fallow_engine::changed_files::resolve_git_common_dir_and_toplevel(root),
        || fallow_engine::changed_files::resolve_git_common_dir(root).ok(),
        || fallow_engine::changed_files::resolve_git_toplevel(root).ok(),
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
    /// CI (GitHub Actions sets `CI` / `GITHUB_ACTIONS`, which `ci_env::is_ci`
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
/// production it is `fallow_engine::ci_env::is_ci()`; under test it reads the per-test
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
        fallow_engine::ci_env::is_ci()
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

/// Stable failure states for status-bar callers. The statusline loader never
/// logs the store path or substitutes corrupt/newer data with an empty report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatuslineLoadError {
    /// The store is unreadable or corrupt.
    DataUnavailable,
    /// The store was written by a newer Fallow whose schema this build cannot read.
    NewerSchema,
}

/// Load Impact state without writes, warnings, or path-bearing diagnostics.
///
/// Unlike [`load`], this does not persist a legacy in-repo store when the user
/// store is missing. A corrupt, unreadable, or newer-schema store is reported
/// as unavailable so the compact renderer cannot mislabel lost data as "off".
pub fn load_statusline(root: &Path) -> Result<ImpactStore, StatuslineLoadError> {
    let Some(path) = store_path(root) else {
        return Ok(ImpactStore::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => parse_statusline_store(&content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            load_legacy_statusline_store(root)
        }
        Err(_) => Err(StatuslineLoadError::DataUnavailable),
    }
}

fn parse_statusline_store(content: &str) -> Result<ImpactStore, StatuslineLoadError> {
    let store = serde_json::from_str::<ImpactStore>(content)
        .map_err(|_| StatuslineLoadError::DataUnavailable)?;
    if store.schema_version > STORE_SCHEMA_VERSION {
        return Err(StatuslineLoadError::NewerSchema);
    }
    Ok(store)
}

fn load_legacy_statusline_store(root: &Path) -> Result<ImpactStore, StatuslineLoadError> {
    let legacy_path = legacy_store_path(root);
    let content = match std::fs::read_to_string(legacy_path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ImpactStore::default());
        }
        Err(_) => return Err(StatuslineLoadError::DataUnavailable),
    };
    let legacy = serde_json::from_str::<LegacyFlatStore>(&content)
        .map_err(|_| StatuslineLoadError::DataUnavailable)?;
    let (_, worktree, display) = project_identity(root);
    let mut store = legacy.into_store(&worktree);
    store.label = display;
    Ok(store)
}

fn parse_store(content: &str, path: &Path) -> ImpactStore {
    match serde_json::from_str::<ImpactStore>(content) {
        Ok(store) => {
            if store.schema_version > STORE_SCHEMA_VERSION {
                tracing::warn!(
                    "fallow impact: store at {} has schema_version {} but this build understands up to {}; reading it as best-effort. This build does not write to it, so the store stays unchanged. Upgrade fallow to read it fully.",
                    path.display(),
                    store.schema_version,
                    STORE_SCHEMA_VERSION,
                );
            }
            store
        }
        Err(err) => {
            if let Some(newer) = newer_schema_in(content, path) {
                tracing::warn!("{}", newer.message());
            } else {
                tracing::warn!(
                    "fallow impact: ignoring unreadable store at {} ({err}); run `fallow impact enable` to reset it",
                    path.display()
                );
            }
            ImpactStore::default()
        }
    }
}

/// An Impact store written by a newer Fallow. This build refuses to write it,
/// because a typed round trip through the older [`ImpactStore`] drops every
/// field and every record shape that this build does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewerStoreSchema {
    path: PathBuf,
    schema_version: u32,
    writer_version: Option<String>,
}

impl NewerStoreSchema {
    /// The user-facing explanation, with the next step.
    #[must_use]
    pub fn message(&self) -> String {
        let writer = self
            .writer_version
            .as_deref()
            .map_or_else(String::new, |version| {
                format!(" (newest record by fallow {version})")
            });
        format!(
            "fallow impact: the store at {} has schema_version {}{writer}, but this build supports up to schema_version {}. The store is unchanged. Upgrade fallow, or use the newer fallow binary, to change or record Impact data for this project.",
            self.path.display(),
            self.schema_version,
            STORE_SCHEMA_VERSION,
        )
    }
}

/// Read only the schema marker and the newest writer version of a raw store.
/// This works when the typed parse fails, because a newer schema can change
/// the shape of a known field. Returns `None` for a supported or corrupt store.
fn newer_schema_in(content: &str, path: &Path) -> Option<NewerStoreSchema> {
    let raw: serde_json::Value = serde_json::from_str(content).ok()?;
    let schema_version = raw.get("schema_version")?.as_u64()?;
    if schema_version <= u64::from(STORE_SCHEMA_VERSION) {
        return None;
    }
    Some(NewerStoreSchema {
        path: path.to_path_buf(),
        schema_version: u32::try_from(schema_version).unwrap_or(u32::MAX),
        writer_version: newest_writer_version(&raw),
    })
}

/// The fallow version on the newest record of either track, when present.
fn newest_writer_version(raw: &serde_json::Value) -> Option<String> {
    ["records", "project_records"]
        .iter()
        .filter_map(|track| raw.get(*track)?.as_array()?.last())
        .filter_map(|record| {
            let timestamp = record.get("timestamp")?.as_str()?;
            let version = record.get("version")?.as_str()?;
            Some((timestamp, version))
        })
        .max_by(|a, b| a.0.cmp(b.0))
        .map(|(_, version)| version.to_owned())
}

/// Load the store for a load, mutate, and save cycle. Refuses a store that a
/// newer Fallow wrote, so the caller leaves the file unchanged. Every write
/// path must use this instead of [`load`].
fn load_for_write(root: &Path) -> Result<ImpactStore, NewerStoreSchema> {
    let Some(path) = store_path(root) else {
        return Ok(ImpactStore::default());
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(migrate_legacy_store(root));
    };
    if let Some(newer) = newer_schema_in(&content, &path) {
        return Err(newer);
    }
    Ok(parse_store(&content, &path))
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
///
/// Cross-project GC race: this sweep does NOT take the per-store
/// [`ImpactStoreLock`] of the OTHER projects it inspects, so in principle it
/// could delete a store another process is mid-writing. This is a bounded,
/// best-effort limitation rather than a corruption bug. A store becomes a
/// deletion candidate only when its file mtime is already older than
/// `max_age`, and any record refreshes the mtime via an atomic replace, so a
/// repo with even occasional activity never ages out. The one genuinely lossy
/// window is sub-millisecond: a concurrent writer that completes its atomic
/// replace AFTER this fn's `metadata().modified()` read but BEFORE the
/// `remove_file` would have its just-written (fresh) record deleted. That
/// window is vanishingly small, the deletion is opt-in (gated on
/// [`STORE_MAX_AGE_ENV`]), and the store is reconstructed on the project's
/// next recorded run, so the worst case is the loss of a single just-written
/// record for an otherwise-dormant project, never partial/corrupt state.
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
///
/// # Errors
///
/// Returns [`NewerStoreSchema`] and leaves the store unchanged when a newer
/// Fallow wrote it.
pub fn enable(root: &Path) -> Result<bool, NewerStoreSchema> {
    let mut store = load_for_write(root)?;
    let was_enabled = store.enabled;
    store.enabled = true;
    store.explicit_decision = true;
    if store.schema_version == 0 {
        store.schema_version = STORE_SCHEMA_VERSION;
    }
    save(&store, root);
    Ok(!was_enabled)
}

/// Disable Impact tracking. Retains existing history. Returns whether it was
/// newly disabled (false if already off). Also records the explicit decision,
/// so declining the impact opt-in on a never-enabled project (`impact
/// disable`) persists "asked and said no" for the agent skill.
///
/// # Errors
///
/// Returns [`NewerStoreSchema`] and leaves the store unchanged when a newer
/// Fallow wrote it. This keeps the retained-history promise for that store.
pub fn disable(root: &Path) -> Result<bool, NewerStoreSchema> {
    let mut store = load_for_write(root)?;
    let was_enabled = store.enabled;
    store.enabled = false;
    store.explicit_decision = true;
    if store.schema_version == 0 {
        store.schema_version = STORE_SCHEMA_VERSION;
    }
    save(&store, root);
    Ok(was_enabled)
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
    let mut store = load_for_write(root).ok()?;
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
///
/// # Errors
///
/// Returns [`NewerStoreSchema`] and leaves the store unchanged when a newer
/// Fallow wrote it.
pub fn decline_onboarding(root: &Path) -> Result<bool, NewerStoreSchema> {
    let mut store = load_for_write(root)?;
    let was_declined = store.onboarding_declined;
    store.onboarding_declined = true;
    if store.schema_version == 0 {
        store.schema_version = STORE_SCHEMA_VERSION;
    }
    save(&store, root);
    Ok(!was_declined)
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
    pub gate_source: Option<GateSource>,
    pub git_sha: Option<&'a str>,
    pub version: &'a str,
    pub timestamp: &'a str,
    pub attribution: Option<&'a AttributionInput<'a>>,
    pub analysis_identity: &'a fallow_types::semantic::SemanticAnalysisIdentity,
}

pub fn record_audit_run(
    root: &Path,
    summary: &AuditSummary,
    record: &AuditRunRecord<'_>,
) -> Result<(), String> {
    let AuditRunRecord {
        verdict,
        gate_source,
        git_sha,
        version,
        timestamp,
        attribution,
        analysis_identity,
    } = record;
    // Impact is a LOCAL-DEV signal. Never record in CI: a user-global default
    // baked into a devcontainer/dotfiles image would otherwise start writing
    // per-project files on every CI run (pre-relocation this was emergent from
    // a fresh CI checkout having no in-repo store file; now it is explicit).
    if record_gate_is_ci() {
        return Ok(());
    }
    // Serialise the load -> mutate -> save window so two worktrees of the same
    // repo (same store key) cannot lost-update each other's record.
    let _lock = ImpactStoreLock::acquire(root);
    let mut store = load_for_write(root).map_err(|newer| {
        let message = newer.message();
        tracing::warn!("{message}");
        message
    })?;
    if !resolve_enabled(&store).0 {
        return Ok(());
    }
    validate_store_identity(&store, analysis_identity)?;
    store.schema_version = STORE_SCHEMA_VERSION;
    store.analysis_identity = (*analysis_identity).clone();
    // Capture the repo basename for the cross-repo view (memoized; no extra git
    // probe). Refreshed each run so a renamed repo folder updates its label.
    store.label = project_identity(root).2;

    let counts = impact_counts_from_summary(summary);
    let verdict_str = verdict_label(*verdict);

    if store.first_recorded.is_none() {
        store.first_recorded = Some((*timestamp).to_owned());
    }

    apply_containment(
        &mut store,
        *verdict,
        gate_source.is_some(),
        *git_sha,
        timestamp,
        &counts,
    );

    store.records.push(ImpactRecord {
        timestamp: (*timestamp).to_owned(),
        version: (*version).to_owned(),
        git_sha: git_sha.map(ToOwned::to_owned),
        verdict: verdict_str.to_owned(),
        gate_source: *gate_source,
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
    Ok(())
}

/// Record a whole-project combined run into the project track.
#[expect(
    clippy::too_many_arguments,
    reason = "historical impact writes keep provenance and analysis identity explicit"
)]
pub fn record_combined_run(
    root: &Path,
    counts: ImpactCounts,
    git_sha: Option<&str>,
    version: &str,
    timestamp: &str,
    attribution: Option<&AttributionInput<'_>>,
    analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity,
) -> Result<(), String> {
    if record_gate_is_ci() {
        return Ok(());
    }
    let _lock = ImpactStoreLock::acquire(root);
    let mut store = load_for_write(root).map_err(|newer| {
        let message = newer.message();
        tracing::warn!("{message}");
        message
    })?;
    if !resolve_enabled(&store).0 {
        return Ok(());
    }
    validate_store_identity(&store, analysis_identity)?;
    store.schema_version = STORE_SCHEMA_VERSION;
    store.analysis_identity = analysis_identity.clone();
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
        gate_source: None,
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
    Ok(())
}

fn validate_store_identity(
    store: &ImpactStore,
    current: &fallow_types::semantic::SemanticAnalysisIdentity,
) -> Result<(), String> {
    let has_history = !store.records.is_empty()
        || !store.project_records.is_empty()
        || !store.frontier.is_empty()
        || !store.clone_frontier.is_empty();
    if !has_history {
        return Ok(());
    }
    let incompatible = store.analysis_identity.incompatible_fields(current);
    if incompatible.is_empty() {
        return Ok(());
    }
    Err(format!(
        "impact history has an incompatible analysis identity ({}); run `fallow impact reset` before switching semantic analysis modes or capability sets",
        incompatible.join(", ")
    ))
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
    fallow_output::codeclimate_fingerprint_hash(&[kind, rel_path, symbol.unwrap_or("")])
}

fn covered_by(present: &FxHashSet<String>, kind: &str) -> bool {
    present.contains(BLANKET_SUPPRESSION) || present.contains(kind)
}

/// A single worktree's flat per-file attribution baseline (rel-path -> findings).
type FlatFrontier = FxHashMap<String, FileFrontier>;
/// A single worktree's flat clone baseline (fingerprint -> instance paths).
type FlatCloneFrontier = FxHashMap<String, Vec<String>>;
/// This run's per-file findings and present-suppression kinds, scoped to changed files.
type CurrentState = (
    FxHashMap<String, Vec<FrontierFinding>>,
    FxHashMap<String, FxHashSet<String>>,
);

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

    let (current_findings, current_supps) = collect_current_state(input, &changed, root);

    let appeared_move_keys = compute_appeared_move_keys(&frontier, &current_findings);

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
    classify_clone_disappearances(&mut CloneDisappearancesInput {
        store,
        frontier: &frontier,
        clone_frontier: &mut clone_frontier,
        input,
        changed: &changed,
        git_sha,
        timestamp,
    });
    prune_frontier(&mut frontier, &mut clone_frontier, root);
    bound_recent_resolved(store);

    store_worktree_baseline(store, worktree_key, frontier, clone_frontier);
}

/// Collect the move-keys of findings that newly appeared this run (no prior ID in
/// the same file), so cross-file moves can be cancelled against disappearances.
fn compute_appeared_move_keys(
    frontier: &FlatFrontier,
    current_findings: &FxHashMap<String, Vec<FrontierFinding>>,
) -> FxHashSet<String> {
    let mut appeared_move_keys: FxHashSet<String> = FxHashSet::default();
    for (rel, findings) in current_findings {
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
    appeared_move_keys
}

/// Build this run's per-file findings + present-suppression maps, scoped to
/// changed files. Findings carry a line-independent ID; suppressions collapse to
/// kind keys (blanket when no kind is given).
fn collect_current_state(
    input: &AttributionInput<'_>,
    changed: &FxHashSet<String>,
    root: &Path,
) -> CurrentState {
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
    (current_findings, current_supps)
}

/// Store this worktree's baseline back; drop the worktree key entirely when empty
/// so deleted/abandoned worktrees do not accumulate.
fn store_worktree_baseline(
    store: &mut ImpactStore,
    worktree_key: &str,
    frontier: FlatFrontier,
    clone_frontier: FlatCloneFrontier,
) {
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

/// Inputs to the clone-disappearance classifier, bundled so it takes a single
/// parameter struct instead of seven (mirrors the sibling
/// `FileDisappearancesInput` used by `classify_file_disappearances`).
struct CloneDisappearancesInput<'a> {
    store: &'a mut ImpactStore,
    frontier: &'a FlatFrontier,
    clone_frontier: &'a mut FlatCloneFrontier,
    input: &'a AttributionInput<'a>,
    changed: &'a FxHashSet<String>,
    git_sha: Option<&'a str>,
    timestamp: &'a str,
}

fn classify_clone_disappearances(args: &mut CloneDisappearancesInput<'_>) {
    let store = &mut *args.store;
    let frontier = args.frontier;
    let clone_frontier = &mut *args.clone_frontier;
    let input = args.input;
    let changed = args.changed;
    let git_sha = args.git_sha;
    let timestamp = args.timestamp;
    let current = collect_changed_clone_groups(input, changed);

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
        credit_clone_disappearance(store, frontier, changed, &paths, git_sha, timestamp);
    }

    for (fp, paths) in current {
        clone_frontier.insert(fp, paths);
    }
}

/// Build this run's changed-touching clone groups (fingerprint -> sorted/deduped
/// display paths), keeping only groups with at least one changed instance path.
fn collect_changed_clone_groups(
    input: &AttributionInput<'_>,
    changed: &FxHashSet<String>,
) -> FxHashMap<String, Vec<String>> {
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
    current
}

/// True when a disappeared clone group's changed paths carry a fresh duplication
/// or blanket suppression in the frontier (conservative: counts as suppressed).
fn clone_dup_suppressed(
    frontier: &FlatFrontier,
    changed: &FxHashSet<String>,
    paths: &[String],
) -> bool {
    paths.iter().any(|p| {
        changed.contains(p)
            && frontier.get(p).is_some_and(|f| {
                f.suppressions
                    .iter()
                    .any(|k| k == CODE_DUPLICATION_KIND || k == BLANKET_SUPPRESSION)
            })
    })
}

/// Credit a fully-disappeared clone group as resolved or suppressed on `store`.
fn credit_clone_disappearance(
    store: &mut ImpactStore,
    frontier: &FlatFrontier,
    changed: &FxHashSet<String>,
    paths: &[String],
    git_sha: Option<&str>,
    timestamp: &str,
) {
    if clone_dup_suppressed(frontier, changed, paths) {
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
    collect_file_and_export_findings(results, push);
    collect_member_findings(results, push);
    collect_component_findings(results, push);
    collect_import_boundary_findings(results, push);
}

/// Push unused-file, export, type, and private-type-leak findings.
fn collect_file_and_export_findings(
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
    for f in &results.deprecated_exports_in_use {
        push(
            &f.export.path,
            "deprecated-export-in-use",
            Some(f.export.export_name.clone()),
        );
    }
}

/// Push unused enum/class/store member and unprovided-inject findings.
fn collect_member_findings(
    results: &AnalysisResults,
    push: &mut impl FnMut(&Path, &'static str, Option<String>),
) {
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
    for f in &results.unused_namespace_members {
        push(
            &f.member.path,
            "unused-namespace-member",
            Some(format!(
                "{}{ID_SEP}{}",
                f.member.parent_name, f.member.member_name
            )),
        );
    }
    for f in &results.unprovided_injects {
        push(
            &f.inject.path,
            "unprovided-inject",
            Some(f.inject.key_name.clone()),
        );
    }
}

/// Push unrendered-component and component prop/emit/input/output findings.
fn collect_component_findings(
    results: &AnalysisResults,
    push: &mut impl FnMut(&Path, &'static str, Option<String>),
) {
    for f in &results.unrendered_components {
        push(
            &f.component.path,
            "unrendered-component",
            Some(f.component.component_name.clone()),
        );
    }
    for finding in &results.absent_component_props {
        push(
            &finding.prop.path,
            "absent-component-prop",
            Some(format!(
                "{}.{}",
                finding.prop.component_name, finding.prop.prop_name
            )),
        );
    }
    for f in &results.unused_component_props {
        push(
            &f.prop.path,
            "unused-component-prop",
            Some(f.prop.prop_name.clone()),
        );
    }
    for f in &results.unused_component_emits {
        push(
            &f.emit.path,
            "unused-component-emit",
            Some(f.emit.emit_name.clone()),
        );
    }
    for f in &results.unused_component_inputs {
        push(
            &f.input.path,
            "unused-component-input",
            Some(f.input.input_name.clone()),
        );
    }
    for f in &results.unused_component_outputs {
        push(
            &f.output.path,
            "unused-component-output",
            Some(f.output.output_name.clone()),
        );
    }
}

/// Push unresolved-import and boundary-violation findings.
fn collect_import_boundary_findings(
    results: &AnalysisResults,
    push: &mut impl FnMut(&Path, &'static str, Option<String>),
) {
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
    for f in &results.dev_dependencies_in_production {
        push(
            &f.dep.path,
            "dev-dependency-in-production",
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
pub fn collect_complexity_findings(report: &fallow_output::HealthReport) -> Vec<FindingInput> {
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
    report: &fallow_types::duplicates::DuplicationReport,
) -> Vec<CloneInput> {
    report
        .clone_groups
        .iter()
        .map(|g| CloneInput {
            fingerprint: fallow_engine::duplicates::clone_fingerprint(&g.instances),
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

fn direction_for(delta: i64) -> ImpactTrendDirection {
    if delta < -TREND_TOLERANCE {
        ImpactTrendDirection::Improving
    } else if delta > TREND_TOLERANCE {
        ImpactTrendDirection::Declining
    } else {
        ImpactTrendDirection::Stable
    }
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
    let total_delta = crate::count_delta::signed_delta(current_total, previous_total);
    Some(TrendSummary {
        direction: direction_for(total_delta),
        total_delta,
        previous_total,
        current_total,
    })
}

/// Tally recorded gate runs by source. `None` when the store holds no gate run
/// at all, so the report field stays absent on the unchanged path.
fn gate_run_counts(records: &[ImpactRecord]) -> Option<GateRunCounts> {
    let mut counts = GateRunCounts::default();
    for source in records.iter().filter_map(|record| record.gate_source) {
        match source {
            GateSource::Agent => counts.agent += 1,
            GateSource::PreCommit => counts.pre_commit += 1,
            GateSource::Ci => counts.ci += 1,
            GateSource::Unknown => counts.unknown += 1,
        }
    }
    (!counts.is_empty()).then_some(counts)
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
        schema_version: ImpactReportSchemaVersion::V2,
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
        gate_runs: gate_run_counts(&store.records),
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
        schema_version: CrossRepoImpactSchemaVersion::V2,
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

    render_human_changed_section(&mut out, report);

    render_project_section(&mut out, report);

    out.push_str(&format!(
        "  CONTAINED AT COMMIT\n    {} time{} fallow blocked a commit until it was fixed\n",
        report.containment_count,
        plural(report.containment_count),
    ));

    render_human_resolved_section(&mut out, report);

    render_human_footer(&mut out, report);
    out
}

/// Render one stable, path-free line for editor and shell status bars.
#[must_use]
pub fn render_statusline(report: &ImpactReport) -> String {
    const PREFIX: &str = "fallow impact  ";

    if !report.enabled {
        return format!("{PREFIX}off");
    }

    let cleared = statusline_cleared_segment(report);
    if let Some(surfacing) = &report.project_surfacing {
        let mut segments = vec![format!(
            "{} issue{} in last full scan",
            surfacing.total_issues,
            plural(surfacing.total_issues)
        )];
        segments.push(match &report.project_trend {
            Some(trend) => statusline_trend_segment(trend),
            None => "compare after next full scan".to_owned(),
        });
        if let Some(cleared) = cleared {
            segments.push(cleared);
        }
        return format!("{PREFIX}{}", segments.join(" · "));
    }

    if let Some(surfacing) = &report.surfacing {
        let mut segments = vec![format!(
            "{} issue{} in last changed-file scan",
            surfacing.total_issues,
            plural(surfacing.total_issues)
        )];
        if let Some(cleared) = cleared {
            segments.push(cleared);
        }
        return format!("{PREFIX}{}", segments.join(" · "));
    }

    format!("{PREFIX}tracking · awaiting first scan")
}

/// Render the exact stable state used when Impact history cannot be read.
#[must_use]
pub fn render_statusline_unavailable() -> String {
    "fallow impact  data unavailable".to_owned()
}

/// Render the exact stable state used when the store comes from a newer Fallow.
/// Naming the cause lets a status-bar wrapper retry with a newer binary and
/// tells the user which fix applies.
#[must_use]
pub fn render_statusline_newer_schema() -> String {
    "fallow impact  data from newer fallow · upgrade this fallow".to_owned()
}

fn statusline_trend_segment(trend: &TrendSummary) -> String {
    let magnitude = trend.total_delta.unsigned_abs();
    match trend.direction {
        ImpactTrendDirection::Improving => format!("{magnitude} fewer than prior"),
        ImpactTrendDirection::Declining => format!("{magnitude} more than prior"),
        ImpactTrendDirection::Stable => "unchanged from prior".to_owned(),
    }
}

fn statusline_cleared_segment(report: &ImpactReport) -> Option<String> {
    if !report.attribution_active {
        return None;
    }
    Some(format!(
        "{} cleared while tracking",
        compact_count(report.resolved_total)
    ))
}

fn compact_count(count: usize) -> String {
    const THOUSAND: usize = 1_000;
    const MILLION: usize = 1_000_000;
    const BILLION: usize = 1_000_000_000;

    let (unit, suffix) = if count >= BILLION {
        (BILLION, "b")
    } else if count >= MILLION {
        (MILLION, "m")
    } else if count >= THOUSAND {
        (THOUSAND, "k")
    } else {
        return count.to_string();
    };
    let tenths = count.saturating_mul(10).saturating_add(unit / 2) / unit;
    if tenths.is_multiple_of(10) {
        format!("{}{suffix}", tenths / 10)
    } else {
        format!("{}.{:01}{suffix}", tenths / 10, tenths % 10)
    }
}

/// Render the changed-file LATEST RUN and TREND sections of the human report.
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
fn render_human_changed_section(out: &mut String, report: &ImpactReport) {
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
}

/// Render the RESOLVED and marked-intentional sections of the human report.
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
fn render_human_resolved_section(out: &mut String, report: &ImpactReport) {
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
}

/// Join the non-zero gate sources into one `agent 12, pre-commit 3` phrase.
fn gate_run_summary(counts: &GateRunCounts) -> String {
    [
        ("agent", counts.agent),
        ("pre-commit", counts.pre_commit),
        ("ci", counts.ci),
        ("unknown", counts.unknown),
    ]
    .into_iter()
    .filter(|(_, runs)| *runs > 0)
    .map(|(label, runs)| format!("{label} {runs}"))
    .collect::<Vec<_>>()
    .join(", ")
}

/// Render the trailing provenance/footer lines of the human report.
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
fn render_human_footer(out: &mut String, report: &ImpactReport) {
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
    if let Some(gate_runs) = &report.gate_runs {
        out.push_str(&format!(
            "Where your gate runs come from: {}. Local provenance, not adoption.\n",
            gate_run_summary(gate_runs),
        ));
    }
    out.push_str(
        "Resolution tracking is a local-developer signal: it accrues on your\n\
         machine across runs, not in CI (fallow never records there).\n",
    );
}

/// Render the report as JSON.
#[cfg(test)]
pub fn render_json(report: &ImpactReport) -> String {
    render_json_with_style(report, crate::json_style::JsonStyle::Compact)
}

pub fn render_json_with_style(
    report: &ImpactReport,
    json_style: crate::json_style::JsonStyle,
) -> String {
    let value = fallow_output::serialize_impact_json_output(
        report.clone(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )
    .unwrap_or_else(|_| serde_json::json!({"error":"failed to serialize impact report"}));
    json_style
        .serialize(&value)
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
    render_markdown_resolved_section(&mut out, report);
    render_markdown_footer(&mut out, report);
    out
}

/// Render the Resolved and marked-intentional bullets of the markdown report.
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
fn render_markdown_resolved_section(out: &mut String, report: &ImpactReport) {
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
}

/// Render the trailing provenance line of the markdown report.
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
fn render_markdown_footer(out: &mut String, report: &ImpactReport) {
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
}

/// Render the cross-repo report as JSON via the typed `ImpactCrossRepo` envelope.
#[must_use]
#[cfg(test)]
pub fn render_cross_repo_json(report: &CrossRepoImpactReport) -> String {
    render_cross_repo_json_with_style(report, crate::json_style::JsonStyle::Compact)
}

pub fn render_cross_repo_json_with_style(
    report: &CrossRepoImpactReport,
    json_style: crate::json_style::JsonStyle,
) -> String {
    let value = fallow_output::serialize_cross_repo_impact_json_output(
        report.clone(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )
    .unwrap_or_else(
        |_| serde_json::json!({"error":"failed to serialize cross-repo impact report"}),
    );
    json_style.serialize(&value).unwrap_or_else(|_| {
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

    render_cross_repo_table(&mut out, report, limit);
    render_cross_repo_skipped(&mut out, report);
    render_cross_repo_totals(&mut out, report);
    out.push_str("\nLocal-only; never uploaded; accrues on this machine, not CI.\n");
    out
}

/// Render the per-project table (header, rows capped at `limit`, overflow line).
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
fn render_cross_repo_table(out: &mut String, report: &CrossRepoImpactReport, limit: Option<usize>) {
    if report.projects.is_empty() {
        return;
    }
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

/// Render the no-history and skipped-unreadable summary lines.
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
fn render_cross_repo_skipped(out: &mut String, report: &CrossRepoImpactReport) {
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
}

/// Render the GRAND TOTALS block (resolved/contained/intentional + baseline line).
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
fn render_cross_repo_totals(out: &mut String, report: &CrossRepoImpactReport) {
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
        "{} project{} tracked, {} with history.\n\n",
        report.project_count,
        plural(report.project_count),
        report.tracked_count,
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
        "\n**Grand totals:** {} resolved, {} contained, {} marked intentional across {} tracked project{}",
        t.resolved_total,
        t.containment_count,
        t.suppressed_total,
        report.tracked_count,
        plural(report.tracked_count),
    ));
    if t.projects_with_baseline > 0 {
        out.push_str(&format!(
            "; {} issue{} project-wide across {} project{} with a full-run baseline (as of each project's last full run)",
            t.project_wide_issues,
            plural(t.project_wide_issues),
            t.projects_with_baseline,
            plural(t.projects_with_baseline),
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
#[path = "impact_tests.rs"]
mod tests;
