//! Fallow Impact: a local, opt-in value report.
//!
//! Impact answers "what did fallow do for you?" rather than "what is wrong now?".
//! v1 is deliberately thin and honest. It renders three things:
//!
//! 1. Surfacing: how many issues fallow is currently showing you.
//! 2. Trend: whether the issue count is moving the right way between recorded runs.
//! 3. Containment: how many times a pre-commit gate run blocked then cleared.
//!
//! Everything lives locally in a single rolling file at `.fallow/impact.json`
//! (gitignored). Writes are best-effort and NEVER affect the exit code of any
//! command: a corrupt or unwritable store degrades to "no history", never an
//! error.
//!
//! v1.5 adds per-finding attribution on top: it credits genuinely RESOLVED
//! findings (code removed or refactored) and never counts a `fallow-ignore`
//! suppression as a win. It tells the two apart by capturing the present
//! suppression state each run and diffing a per-file frontier against the files
//! audit re-analyzed; a finding that merely moved (within a file, or to another
//! file even across separate commits) is not counted as resolved. Attribution is
//! a local-developer signal: it accrues where `.fallow/impact.json` persists
//! across runs, not in ephemeral CI runners.

use std::path::{Path, PathBuf};

use fallow_types::results::{ActiveSuppression, AnalysisResults};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use crate::audit::{AuditSummary, AuditVerdict};
use crate::report::ci::fingerprint::fingerprint_hash;
use crate::report::format_display_path;

/// On-disk schema version for the rolling impact store. Distinct from the JSON
/// report's wire version ([`ImpactReportSchemaVersion`]): the store's persisted
/// shape and the `--format json` report's shape evolve independently. v2 added
/// the per-finding attribution surface (file frontier, clone frontier,
/// resolved/suppressed counters, recent resolutions). A v1 store reads forward
/// cleanly (new fields default empty; attribution accrues from the next run). A
/// v2 store is also safe to READ on an older v1 binary (unknown keys ignored);
/// the only caveat is DOWNGRADE: an older v1 binary that records a run rewrites
/// the store with only the v1 fields, silently dropping the frontier and the
/// lifetime counters, after which the next v2 run re-seeds from empty.
/// Attribution restarts, it does not corrupt.
const STORE_SCHEMA_VERSION: u32 = 2;

/// Upper bound on retained per-run records. The store is a single compacted file,
/// so this only bounds memory/disk, not file count. Oldest records are dropped first.
const MAX_RECORDS: usize = 200;

/// Upper bound on retained containment events (oldest dropped first).
const MAX_CONTAINMENT: usize = 200;

/// Tolerance (in absolute issue count) at or below which a trend is "stable"
/// rather than improving/declining. Zero means any nonzero delta (even a single
/// finding) registers as a direction; raise it to suppress single-finding noise.
const TREND_TOLERANCE: i64 = 0;

/// File name of the rolling impact store inside `.fallow/`.
const STORE_FILE: &str = "impact.json";

/// Upper bound on retained recent-resolution events (oldest dropped first).
/// Bounds the one growing list the v1.5 surface adds; the lifetime totals are
/// scalar counters and the frontier maps are pruned to on-disk files each run.
const MAX_RECENT_RESOLVED: usize = 50;

/// Field separator for composing a stable, line-independent finding identity
/// out of `(kind, path, symbol)` parts before hashing. ASCII unit separator so
/// it cannot collide with any path, symbol, or kind character.
const ID_SEP: &str = "\u{1f}";

/// The kebab-case kind string for duplication findings, used both as the
/// clone-frontier finding kind and as the suppression kind that silences them.
const CODE_DUPLICATION_KIND: &str = "code-duplication";

/// Sentinel a blanket suppression (`// fallow-ignore-*` with no kind) is stored
/// under, since it covers every kind on its target.
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
}

/// One recorded audit run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactRecord {
    pub timestamp: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    /// "pass" | "warn" | "fail".
    pub verdict: String,
    /// Whether this run was the pre-commit gate (carried the gate marker).
    #[serde(default)]
    pub gate: bool,
    pub counts: ImpactCounts,
}

/// A pre-commit gate run that blocked (verdict fail) and is awaiting a clean run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingContainment {
    pub blocked_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    pub blocked_counts: ImpactCounts,
}

/// A blocked-then-cleared containment: fallow stopped a commit until it was fixed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContainmentEvent {
    pub blocked_at: String,
    pub cleared_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    pub blocked_counts: ImpactCounts,
}

/// One recorded finding's line-independent identity inside a [`FileFrontier`].
///
/// The `id` is a stable hash of `(kind, path, symbol)` so a finding that moves
/// up or down within its file keeps the same identity (line is excluded). The
/// `kind` and `symbol` are retained so the cross-file move-key can be recomputed
/// and a resolution event can name the finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierFinding {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

impl FrontierFinding {
    /// Path-independent key used to cancel within-run moves: a finding that
    /// disappears from one file and reappears (same kind + symbol) in another
    /// file analyzed the same run is a move, not a resolution. Symbol-less
    /// findings (e.g. `unused-file`) fall back to the full `id`, so they never
    /// spuriously cancel across files.
    fn move_key(&self) -> String {
        match &self.symbol {
            Some(symbol) => format!("{}{ID_SEP}{symbol}", self.kind),
            None => self.id.clone(),
        }
    }
}

/// The last-known per-finding state of one file: the findings it carried and the
/// suppression kinds present in it, captured the last time audit re-analyzed it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileFrontier {
    #[serde(default)]
    pub findings: Vec<FrontierFinding>,
    /// Suppression kinds present in the file (kebab-case, or `"*"` for a blanket
    /// marker). Used to detect a `fallow-ignore` that newly appeared covering a
    /// disappeared finding's kind.
    #[serde(default)]
    pub suppressions: Vec<String>,
}

/// A genuinely-resolved finding, recorded for the recent-resolutions display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ResolutionEvent {
    /// The resolved finding's kind, kebab-case (e.g. `"unused-export"`).
    pub kind: String,
    /// Workspace-relative, forward-slash path of the file the finding was in.
    pub path: String,
    /// The finding's symbol (export / member / dependency name), when it has
    /// one. `None` for file-level and content-hash-keyed findings (duplication).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Short git SHA of the run that recorded the resolution, when in a git repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    /// ISO-8601 timestamp of the recording run.
    pub timestamp: String,
}

/// The rolling impact store, persisted to `.fallow/impact.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImpactStore {
    #[serde(default)]
    pub schema_version: u32,
    /// Whether the user has opted in via `fallow impact enable`.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_recorded: Option<String>,
    #[serde(default)]
    pub records: Vec<ImpactRecord>,
    #[serde(default)]
    pub containment: Vec<ContainmentEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_containment: Option<PendingContainment>,
    /// Per-file last-known finding + suppression state (dead-code and complexity
    /// findings). Diffed each run for the files audit re-analyzed; entries for
    /// files no longer on disk are pruned. v1.5.
    #[serde(default)]
    pub frontier: FxHashMap<String, FileFrontier>,
    /// Clone-group state keyed by content fingerprint (`dup:<hash>`), value is
    /// the workspace-relative instance paths. Duplication is multi-file, so it
    /// needs a fingerprint-keyed frontier rather than the per-file one. v1.5.
    #[serde(default)]
    pub clone_frontier: FxHashMap<String, Vec<String>>,
    /// Lifetime count of findings fallow credits as genuinely resolved. v1.5.
    #[serde(default)]
    pub resolved_total: usize,
    /// Lifetime count of findings silenced by a newly-added `fallow-ignore`
    /// (never counted as resolved). v1.5.
    #[serde(default)]
    pub suppressed_total: usize,
    /// Most recent resolution events (newest last), bounded by the
    /// `MAX_RECENT_RESOLVED` cap. v1.5.
    #[serde(default)]
    pub recent_resolved: Vec<ResolutionEvent>,
}

/// Path to the rolling store for a project root.
fn store_path(root: &Path) -> PathBuf {
    root.join(".fallow").join(STORE_FILE)
}

/// Load the store. A missing file is the normal "not enabled yet" case and
/// returns a default silently. A present-but-unparsable file is surfaced with
/// a one-line warning (rather than silently disabling tracking) and then
/// degrades to a default; the corrupt file is left on disk untouched, and
/// because [`record_audit_run`] no-ops on a disabled store it is never
/// overwritten, so re-running `fallow impact enable` is a deliberate reset.
pub fn load(root: &Path) -> ImpactStore {
    let path = store_path(root);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return ImpactStore::default();
    };
    match serde_json::from_str::<ImpactStore>(&content) {
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

/// Persist the store, best-effort. Uses `atomic_write` (tempfile + rename) so a
/// crash or a concurrent writer can never leave a torn, half-written file that
/// the next `load` would treat as corrupt and silently disable. Errors are
/// swallowed: Impact must never affect the exit code or output of the command
/// that triggered the write. Concurrent writers still race (last-write-wins can
/// drop a record), but each write lands as whole, valid JSON.
fn save(store: &ImpactStore, root: &Path) {
    let path = store_path(root);
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(store) {
        let _ = fallow_config::atomic_write(&path, json.as_bytes());
    }
}

/// Enable Impact tracking. Returns whether it was newly enabled (false if already on).
///
/// Also ensures `.fallow/` is gitignored so the store is not accidentally
/// committed: the store is the feature's local-only promise, and `enable` is the
/// moment it is first created, so it is the right place to make
/// "gitignored, never uploaded" true even when the user never ran `fallow init`.
/// Best-effort: a gitignore write failure must never fail enabling.
pub fn enable(root: &Path) -> bool {
    let mut store = load(root);
    let was_enabled = store.enabled;
    store.enabled = true;
    if store.schema_version == 0 {
        store.schema_version = STORE_SCHEMA_VERSION;
    }
    save(&store, root);
    ensure_fallow_gitignored(root);
    !was_enabled
}

/// Best-effort: append `.fallow/` to the project's `.gitignore` if no line
/// already ignores it. Idempotent, and a no-op when `fallow init` (which writes
/// the same entry) already added it. Any IO error is swallowed: enabling Impact
/// must never fail on a gitignore write. `impact` lives in the library crate
/// while `setup_hooks::ensure_gitignore_entry` is binary-only, so this small
/// helper is intentionally self-contained rather than shared.
fn ensure_fallow_gitignored(root: &Path) {
    let path = root.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let already = existing
        .lines()
        .any(|line| matches!(line.trim(), ".fallow" | ".fallow/"));
    if already {
        return;
    }
    let mut contents = existing;
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(".fallow/\n");
    // atomic_write (tempfile + rename) so a crash mid-write cannot truncate the
    // project's .gitignore, matching save()'s store-write durability.
    let _ = fallow_config::atomic_write(&path, contents.as_bytes());
}

/// Disable Impact tracking. Retains existing history. Returns whether it was
/// newly disabled (false if already off).
pub fn disable(root: &Path) -> bool {
    let mut store = load(root);
    let was_enabled = store.enabled;
    store.enabled = false;
    save(&store, root);
    was_enabled
}

/// Record an audit run into the rolling store. No-op when tracking is disabled
/// or the store cannot be read. Best-effort throughout; never returns an error.
///
/// `gate` indicates the run carried the pre-commit gate marker. Containment
/// events are only derived from gate runs: a `fail` gate run sets a pending
/// containment; a later non-`fail` gate run clears it into a containment event.
///
/// `attribution`, when present, carries the per-finding state for this run and
/// drives v1.5 resolved/suppressed attribution against the per-file frontier.
/// Pass `None` to record only the v1 surfacing/trend/containment data.
#[expect(
    clippy::too_many_arguments,
    reason = "best-effort recorder threading the v1 record fields plus the v1.5 attribution input; a params struct would not improve the single call site"
)]
pub fn record_audit_run(
    root: &Path,
    summary: &AuditSummary,
    verdict: AuditVerdict,
    gate: bool,
    git_sha: Option<&str>,
    version: &str,
    timestamp: &str,
    attribution: Option<&AttributionInput<'_>>,
) {
    let mut store = load(root);
    if !store.enabled {
        return;
    }
    // Bump a forward-read v1 store to the current schema once we write it.
    store.schema_version = STORE_SCHEMA_VERSION;

    let counts = ImpactCounts::from_summary(summary);
    let verdict_str = verdict_label(verdict);

    if store.first_recorded.is_none() {
        store.first_recorded = Some(timestamp.to_owned());
    }

    apply_containment(&mut store, verdict, gate, git_sha, timestamp, &counts);

    store.records.push(ImpactRecord {
        timestamp: timestamp.to_owned(),
        version: version.to_owned(),
        git_sha: git_sha.map(ToOwned::to_owned),
        verdict: verdict_str.to_owned(),
        gate,
        counts,
    });
    compact(&mut store);

    if let Some(attribution) = attribution {
        apply_attribution(&mut store, attribution, git_sha, timestamp);
    }

    save(&store, root);
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
        // Blocked. Record (or keep) a pending containment with the blocking counts.
        if store.pending_containment.is_none() {
            store.pending_containment = Some(PendingContainment {
                blocked_at: timestamp.to_owned(),
                git_sha: git_sha.map(ToOwned::to_owned),
                blocked_counts: counts.clone(),
            });
        }
    } else if let Some(pending) = store.pending_containment.take() {
        // Cleared. A previously-blocked commit now passes the gate.
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

/// Drop oldest records beyond the retention bound.
fn compact(store: &mut ImpactStore) {
    if store.records.len() > MAX_RECORDS {
        let overflow = store.records.len() - MAX_RECORDS;
        store.records.drain(0..overflow);
    }
}

/// One finding's identity inputs for a run, in absolute-path form. Built by the
/// [`collect_dead_code_findings`] / [`collect_complexity_findings`] helpers from
/// the typed audit results, or by tests directly.
#[derive(Debug, Clone)]
pub struct FindingInput {
    pub path: PathBuf,
    pub kind: &'static str,
    pub symbol: Option<String>,
}

/// One clone group's identity for a run: its content fingerprint plus the
/// absolute paths of its instances. Built by [`collect_clone_findings`].
#[derive(Debug, Clone)]
pub struct CloneInput {
    pub fingerprint: String,
    pub instance_paths: Vec<PathBuf>,
}

/// Everything the per-finding attribution pass needs for one recorded run.
///
/// All paths are absolute (relativized internally against `root`). `findings`
/// holds dead-code and complexity findings; `clones` holds duplication groups;
/// `suppressions` is the present-suppression snapshot from
/// [`AnalysisResults::active_suppressions`].
pub struct AttributionInput<'a> {
    pub root: &'a Path,
    pub changed_files: &'a [PathBuf],
    pub findings: Vec<FindingInput>,
    pub clones: Vec<CloneInput>,
    pub suppressions: &'a [ActiveSuppression],
}

/// Compute a finding's stable, line-independent identity hash from its
/// workspace-relative path, kind, and optional symbol.
fn finding_id(kind: &str, rel_path: &str, symbol: Option<&str>) -> String {
    fingerprint_hash(&[kind, rel_path, symbol.unwrap_or("")])
}

/// Whether a finding of `kind` is covered by a set of suppression kinds present
/// in the file. A blanket marker (`"*"`) covers everything.
fn covered_by(present: &FxHashSet<String>, kind: &str) -> bool {
    present.contains(BLANKET_SUPPRESSION) || present.contains(kind)
}

/// Drive v1.5 attribution: diff this run's per-file findings against the stored
/// frontier for the files audit re-analyzed, classify each disappearance as
/// resolved / suppressed / moved, update the frontier, and prune dead entries.
fn apply_attribution(
    store: &mut ImpactStore,
    input: &AttributionInput<'_>,
    git_sha: Option<&str>,
    timestamp: &str,
) {
    let root = input.root;
    let changed: FxHashSet<String> = input
        .changed_files
        .iter()
        .map(|p| format_display_path(p, root))
        .collect();

    // Current findings and suppressions for the changed (re-analyzed) files only.
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

    // Move keys that newly appeared this run (present now, absent from that file's
    // prior frontier). A disappearance whose move key is in this set is a
    // cross-file move, not a resolution.
    let mut appeared_move_keys: FxHashSet<String> = FxHashSet::default();
    for (rel, findings) in &current_findings {
        let prior_ids: FxHashSet<&str> = store
            .frontier
            .get(rel)
            .map(|f| f.findings.iter().map(|x| x.id.as_str()).collect())
            .unwrap_or_default();
        for ff in findings {
            if !prior_ids.contains(ff.id.as_str()) {
                appeared_move_keys.insert(ff.move_key());
            }
        }
    }

    // Cross-RUN move correction: a finding credited resolved in a PRIOR run whose
    // move-key reappears as a new finding this run was a move across runs (e.g. a
    // dead export relocated between barrels in separate commits), not a
    // resolution. Un-credit it so a move never counts as a win. Safe direction:
    // the (kind, symbol) move-key is path-independent, so a rare unrelated finding
    // of the same kind+name under-counts here, never over-counts. Runs BEFORE this
    // run's own classification adds events, so it only touches prior resolutions
    // (within-run moves are handled by `appeared_move_keys`).
    uncredit_cross_run_moves(store, &appeared_move_keys);

    classify_file_disappearances(
        store,
        &changed,
        &current_findings,
        &current_supps,
        &appeared_move_keys,
        git_sha,
        timestamp,
    );
    update_file_frontier(store, &changed, current_findings, current_supps);
    classify_clone_disappearances(store, input, &changed, git_sha, timestamp);
    prune_frontier(store, root);
    bound_recent_resolved(store);
}

/// Classify each finding that left a changed file's frontier since the last run.
fn classify_file_disappearances(
    store: &mut ImpactStore,
    changed: &FxHashSet<String>,
    current_findings: &FxHashMap<String, Vec<FrontierFinding>>,
    current_supps: &FxHashMap<String, FxHashSet<String>>,
    appeared_move_keys: &FxHashSet<String>,
    git_sha: Option<&str>,
    timestamp: &str,
) {
    let empty_supps = FxHashSet::default();
    for rel in changed {
        let Some(prior) = store.frontier.get(rel) else {
            continue;
        };
        let now_ids: FxHashSet<&str> = current_findings
            .get(rel)
            .map(|fs| fs.iter().map(|f| f.id.as_str()).collect())
            .unwrap_or_default();
        let now_supps = current_supps.get(rel).unwrap_or(&empty_supps);
        let prior_supps: FxHashSet<&str> = prior.suppressions.iter().map(String::as_str).collect();
        // Suppression kinds that newly appeared in this file this run.
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

/// Overwrite the frontier entry for each changed file with its current state,
/// removing entries that now hold neither findings nor suppressions.
fn update_file_frontier(
    store: &mut ImpactStore,
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
            store.frontier.remove(rel);
        } else {
            store.frontier.insert(
                rel.clone(),
                FileFrontier {
                    findings,
                    suppressions,
                },
            );
        }
    }
}

/// Classify duplication clone groups that left the clone frontier for a changed
/// file. Clone fingerprints are content-derived, so a relocated identical clone
/// keeps its fingerprint and is never counted as resolved (move handled for
/// free). A clone is suppressed when a `code-duplication` suppression is present
/// in any of its instance files this run.
fn classify_clone_disappearances(
    store: &mut ImpactStore,
    input: &AttributionInput<'_>,
    changed: &FxHashSet<String>,
    git_sha: Option<&str>,
    timestamp: &str,
) {
    let root = input.root;
    // Current clone fingerprints touching a changed file, with relative paths.
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

    // A clone is suppressed when an instance file currently carries a
    // code-duplication (or blanket) suppression. Reads the just-updated frontier;
    // since a reported clone was not suppressed in the prior run, a suppression
    // present now is necessarily newly-appeared, so this is the conservative
    // (never-over-credit) read.
    let dup_suppressed = |paths: &[String]| -> bool {
        paths.iter().any(|p| {
            changed.contains(p)
                && store.frontier.get(p).is_some_and(|f| {
                    f.suppressions
                        .iter()
                        .any(|k| k == CODE_DUPLICATION_KIND || k == BLANKET_SUPPRESSION)
                })
        })
    };

    // Disappeared clones: in the stored frontier, intersecting a changed file,
    // not present in the current run.
    let disappeared: Vec<(String, Vec<String>)> = store
        .clone_frontier
        .iter()
        .filter(|(fp, paths)| {
            paths.iter().any(|p| changed.contains(p)) && !current.contains_key(*fp)
        })
        .map(|(fp, paths)| (fp.clone(), paths.clone()))
        .collect();

    for (fp, paths) in disappeared {
        store.clone_frontier.remove(&fp);
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

    // Record the current clones for the next run.
    for (fp, paths) in current {
        store.clone_frontier.insert(fp, paths);
    }
}

/// Drop frontier and clone-frontier entries whose files no longer exist on disk,
/// bounding both maps to the live working tree.
fn prune_frontier(store: &mut ImpactStore, root: &Path) {
    store.frontier.retain(|rel, _| root.join(rel).exists());
    store
        .clone_frontier
        .retain(|_, paths| paths.iter().any(|p| root.join(p).exists()));
}

/// Bound the recent-resolutions list, dropping the oldest entries.
fn bound_recent_resolved(store: &mut ImpactStore) {
    if store.recent_resolved.len() > MAX_RECENT_RESOLVED {
        let overflow = store.recent_resolved.len() - MAX_RECENT_RESOLVED;
        store.recent_resolved.drain(0..overflow);
    }
}

/// Path-independent move-key for a recorded resolution, for cross-run move
/// detection. Mirrors [`FrontierFinding::move_key`]'s symbol branch. `None` for
/// symbol-less resolutions (file-level, duplication), which are not move-tracked
/// across runs (file moves are delete+create; clone fingerprints are content-
/// derived and already move-stable within a run).
fn event_move_key(ev: &ResolutionEvent) -> Option<String> {
    ev.symbol
        .as_ref()
        .map(|symbol| format!("{}{ID_SEP}{symbol}", ev.kind))
}

/// Retroactively un-credit prior-run resolutions revealed as cross-run moves:
/// a `(kind, symbol)` that newly appeared this run and was already recorded as
/// resolved in an earlier run was relocated, not removed. Drops the stale event
/// and decrements the lifetime tally. Bounded by `recent_resolved`'s cap.
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

/// Collect line-independent dead-code finding identities from an analysis result.
///
/// Covers the single-file-anchored dead-code kinds. Multi-file kinds (circular
/// dependencies, re-export cycles, duplicate exports, unlisted dependencies) are
/// intentionally not attributed in v1.5: they surface and trend, but their
/// multi-file nature does not fit the per-file frontier (duplication has its own
/// fingerprint frontier). Boundary violations are anchored at the importing file.
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
    for f in &results.unresolved_imports {
        push(
            &f.import.path,
            "unresolved-import",
            Some(f.import.specifier.clone()),
        );
    }
    for f in &results.boundary_violations {
        // Forward-slash normalize the target path so the finding identity hashes
        // identically across platforms (the symbol feeds finding_id).
        let to_path = f.violation.to_path.to_string_lossy().replace('\\', "/");
        push(
            &f.violation.from_path,
            "boundary-violation",
            Some(format!("{to_path}{ID_SEP}{}", f.violation.import_specifier)),
        );
    }
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
    out
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
    pub record_count: usize,
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
}

/// Build a report from the store. Defensive: a single record (or none) yields
/// no trend rather than a spurious spike, and an empty store yields an empty
/// report flagged so the renderer can show the first-run message.
pub fn build_report(store: &ImpactStore) -> ImpactReport {
    let surfacing = store.records.last().map(|r| r.counts.clone());

    // Trend only when we have at least two records to compare. Treat a missing
    // prior record as "unknown" (no trend), never as a spike.
    let trend = if store.records.len() >= 2 {
        let current = &store.records[store.records.len() - 1];
        let previous = &store.records[store.records.len() - 2];
        let current_total = current.counts.total_issues;
        let previous_total = previous.counts.total_issues;
        let total_delta = current_total as i64 - previous_total as i64;
        Some(TrendSummary {
            direction: direction_for(total_delta),
            total_delta,
            previous_total,
            current_total,
        })
    } else {
        None
    };

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
    // Attribution has a baseline once any file frontier, clone frontier, or
    // lifetime counter exists.
    let attribution_active = !store.frontier.is_empty()
        || !store.clone_frontier.is_empty()
        || store.resolved_total > 0
        || store.suppressed_total > 0;

    ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: store.enabled,
        record_count: store.records.len(),
        first_recorded: store.first_recorded.clone(),
        latest_git_sha,
        surfacing,
        trend,
        containment_count: store.containment.len(),
        recent_containment,
        resolved_total: store.resolved_total,
        suppressed_total: store.suppressed_total,
        recent_resolved,
        attribution_active,
    }
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

    if report.record_count == 0 {
        out.push_str(
            "Tracking enabled. No history yet: check back after your next few\n\
             commits (Impact records each `fallow audit` / pre-commit gate run).\n",
        );
        return out;
    }

    if let Some(s) = &report.surfacing {
        out.push_str(&format!(
            "  LATEST RUN (changed files)\n    {} issue{} flagged in your last `fallow audit` run\n",
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

    out.push_str(&format!(
        "  CONTAINED AT COMMIT\n    {} time{} fallow blocked a commit until it was fixed\n",
        report.containment_count,
        plural(report.containment_count),
    ));

    // RESOLVED always renders a header so the suppression line below always has
    // a section to belong to (the three states are exhaustive).
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
            "\n  RESOLVED\n    none yet; a finding is credited when fallow re-analyzes the\n      changed file it left (a fix that reverts a file to its base state\n      may not be individually credited)\n",
        );
    } else {
        out.push_str("\n  RESOLVED\n    resolution tracking starts from your next gate run\n");
    }

    // Suppression is honest context, indented under RESOLVED, never a scoreboard.
    if report.suppressed_total > 0 {
        out.push_str(&format!(
            "      {} finding{} you marked intentional (fallow-ignore), not counted as resolved\n",
            report.suppressed_total,
            plural(report.suppressed_total),
        ));
    }

    out.push('\n');
    out.push_str(&format!(
        "Based on {} recorded run{} since {}. Local-only; never uploaded.\n\
         Changed-file scope: each run only sees files differing from your audit base.\n\
         Resolution tracking is a local-developer signal: it accrues where\n\
         .fallow/impact.json persists across runs, not in ephemeral CI runners.\n",
        report.record_count,
        plural(report.record_count),
        report
            .first_recorded
            .as_deref()
            .map_or("the first run", date_only),
    ));
    out
}

/// Render the report as JSON.
pub fn render_json(report: &ImpactReport) -> String {
    serde_json::to_string_pretty(report)
        .unwrap_or_else(|_| "{\"error\":\"failed to serialize impact report\"}".to_owned())
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
    if report.record_count == 0 {
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
    out.push_str(&format!(
        "- **Contained at commit:** {} time{}\n",
        report.containment_count,
        plural(report.containment_count),
    ));
    // Always emit a Resolved bullet (three exhaustive states) so the Marked-
    // intentional bullet never appears without it.
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
    out.push_str(&format!(
        "\n_Based on {} recorded run{} since {}. Local-only; resolution is a local-developer signal._\n",
        report.record_count,
        plural(report.record_count),
        report
            .first_recorded
            .as_deref()
            .map_or("the first run", date_only),
    ));
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
            root, summary, verdict, gate, git_sha, version, timestamp, None,
        );
    }

    // ---- v1.5 per-finding attribution helpers ----

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
            changed_files: &changed_files,
            findings,
            clones,
            suppressions: supps,
        };
        record_audit_run(
            root,
            &summary(0, 0, 0),
            AuditVerdict::Pass,
            true,
            Some("sha"),
            "2.0.0",
            ts,
            Some(&input),
        );
    }

    #[test]
    fn disabled_store_does_not_record() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Not enabled: recording is a no-op.
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
    fn enable_then_record_accrues_history() {
        let dir = tempfile::tempdir().unwrap();
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
    fn enable_gitignores_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        enable(root);
        let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(
            gitignore.lines().any(|l| l.trim() == ".fallow/"),
            "enable must gitignore .fallow/, got: {gitignore:?}"
        );
        // Idempotent: a second enable does not duplicate the entry, and an
        // existing entry (e.g. from `fallow init`) is left alone.
        enable(root);
        let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert_eq!(
            gitignore.lines().filter(|l| l.trim() == ".fallow/").count(),
            1,
            "re-enabling must not duplicate the .fallow/ entry"
        );
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
        // A single record must NOT produce a trend (which would read as a spike
        // from zero on the first run after enabling).
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
        // Never a fabricated zero presented as a value claim.
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        enable(root);
        // Gate run fails: blocked.
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

        // Gate run passes: cleared -> one containment event.
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        enable(root);
        // Fail but NOT a gate run: no pending containment.
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".fallow")).unwrap();
        std::fs::write(store_path(root), b"{ not valid json ][").unwrap();
        // Must not panic; degrades to a default (disabled) store.
        let store = load(root);
        assert!(!store.enabled);
        assert!(store.records.is_empty());
        // Recording against a corrupt store is a no-op (disabled), never an error.
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
        // Oldest dropped: the surviving first record is t50.
        assert_eq!(store.records[0].timestamp, "t50");
    }

    #[test]
    fn report_always_carries_schema_version() {
        // Disabled / empty store still emits the schema version so a machine
        // consumer has a forward-compat signal regardless of state.
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
        // No `T` separator: returned unchanged.
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".fallow")).unwrap();
        // A store written by a hypothetical future fallow (schema_version 2)
        // must still load (best-effort) rather than be discarded as corrupt.
        let future = format!(
            "{{\"schema_version\":{},\"enabled\":true,\"records\":[],\"containment\":[]}}",
            STORE_SCHEMA_VERSION + 1
        );
        std::fs::write(store_path(root), future).unwrap();
        let store = load(root);
        assert_eq!(store.schema_version, STORE_SCHEMA_VERSION + 1);
        assert!(
            store.enabled,
            "future-version store must not degrade to default"
        );
    }

    // ---- v1.5 per-finding attribution ----

    #[test]
    fn removed_finding_is_credited_as_resolved() {
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".fallow")).unwrap();
        let v1 = r#"{"schema_version":1,"enabled":true,"first_recorded":"t0","records":[{"timestamp":"t0","version":"2.0.0","verdict":"warn","gate":false,"counts":{"total_issues":1,"dead_code":1,"complexity":0,"duplication":0}}],"containment":[]}"#;
        std::fs::write(store_path(root), v1).unwrap();
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
        assert!(store.frontier.contains_key("src/a.ts"));
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
        let dir = tempfile::tempdir().unwrap();
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
        assert!(load(root).frontier.contains_key("src/a.ts"));
        std::fs::remove_file(&a).unwrap();
        let b = touch(root, "src/b.ts");
        run(root, &[&b], vec![], vec![], &[], "t1");
        assert!(!load(root).frontier.contains_key("src/a.ts"));
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
            record_count: 2,
            first_recorded: Some("2026-05-29T10:00:00Z".into()),
            latest_git_sha: None,
            surfacing: Some(ImpactCounts::default()),
            trend: None,
            containment_count: 0,
            recent_containment: vec![],
            resolved_total: 0,
            suppressed_total: 2,
            recent_resolved: vec![],
            attribution_active: true,
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
}
