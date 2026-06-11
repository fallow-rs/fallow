//! Fallow Impact: local, opt-in value reporting.

use std::path::{Path, PathBuf};

use fallow_types::envelope::Meta;
use fallow_types::results::{ActiveSuppression, AnalysisResults};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use crate::audit::{AuditSummary, AuditVerdict};
use crate::report::ci::fingerprint::fingerprint_hash;
use crate::report::format_display_path;

const STORE_SCHEMA_VERSION: u32 = 3;

const MAX_RECORDS: usize = 200;

const MAX_CONTAINMENT: usize = 200;

const TREND_TOLERANCE: i64 = 0;

const STORE_FILE: &str = "impact.json";

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
    #[serde(default)]
    pub frontier: FxHashMap<String, FileFrontier>,
    #[serde(default)]
    pub clone_frontier: FxHashMap<String, Vec<String>>,
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
}

fn store_path(root: &Path) -> PathBuf {
    root.join(".fallow").join(STORE_FILE)
}

/// Load the store. Missing or unreadable files fall back to defaults; unreadable
/// files are warned about rather than silently disabling tracking.
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

/// Persist the store best-effort using atomic replace.
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

/// Enable Impact tracking and ensure `.fallow/` is gitignored.
pub fn enable(root: &Path) -> bool {
    let mut store = load(root);
    let was_enabled = store.enabled;
    store.enabled = true;
    store.explicit_decision = true;
    if store.schema_version == 0 {
        store.schema_version = STORE_SCHEMA_VERSION;
    }
    save(&store, root);
    ensure_fallow_gitignored(root);
    !was_enabled
}

/// Append `.fallow/` to `.gitignore` if needed.
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
    let _ = fallow_config::atomic_write(&path, contents.as_bytes());
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
    if !store.enabled {
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

/// Persist that the local user declined the agent onboarding prompt.
pub fn decline_onboarding(root: &Path) -> bool {
    let mut store = load(root);
    let was_declined = store.onboarding_declined;
    store.onboarding_declined = true;
    if store.schema_version == 0 {
        store.schema_version = STORE_SCHEMA_VERSION;
    }
    save(&store, root);
    ensure_fallow_gitignored(root);
    !was_declined
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
    let mut store = load(root);
    if !store.enabled {
        return;
    }
    store.schema_version = STORE_SCHEMA_VERSION;

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
        apply_attribution(&mut store, attribution, *git_sha, timestamp);
    }

    save(&store, root);
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
    let mut store = load(root);
    if !store.enabled {
        return;
    }
    store.schema_version = STORE_SCHEMA_VERSION;

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

fn apply_attribution(
    store: &mut ImpactStore,
    input: &AttributionInput<'_>,
    git_sha: Option<&str>,
    timestamp: &str,
) {
    let root = input.root;
    let changed: FxHashSet<String> = match input.scope {
        Scope::ChangedFiles(files) => files.iter().map(|p| format_display_path(p, root)).collect(),
        Scope::WholeProject => whole_project_scope(store, input, root),
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

    uncredit_cross_run_moves(store, &appeared_move_keys);

    let mut disappearance_input = FileDisappearancesInput {
        store,
        changed: &changed,
        current_findings: &current_findings,
        current_supps: &current_supps,
        appeared_move_keys: &appeared_move_keys,
        git_sha,
        timestamp,
    };
    classify_file_disappearances(&mut disappearance_input);
    update_file_frontier(store, &changed, current_findings, current_supps);
    classify_clone_disappearances(store, input, &changed, git_sha, timestamp);
    prune_frontier(store, root);
    bound_recent_resolved(store);
}

fn whole_project_scope(
    store: &ImpactStore,
    input: &AttributionInput<'_>,
    root: &Path,
) -> FxHashSet<String> {
    let mut set: FxHashSet<String> = store.frontier.keys().cloned().collect();
    for paths in store.clone_frontier.values() {
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
    changed: &'a FxHashSet<String>,
    current_findings: &'a FxHashMap<String, Vec<FrontierFinding>>,
    current_supps: &'a FxHashMap<String, FxHashSet<String>>,
    appeared_move_keys: &'a FxHashSet<String>,
    git_sha: Option<&'a str>,
    timestamp: &'a str,
}

fn classify_file_disappearances(input: &mut FileDisappearancesInput<'_>) {
    let store = &mut *input.store;
    let changed = input.changed;
    let current_findings = input.current_findings;
    let current_supps = input.current_supps;
    let appeared_move_keys = input.appeared_move_keys;
    let git_sha = input.git_sha;
    let timestamp = input.timestamp;
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

fn classify_clone_disappearances(
    store: &mut ImpactStore,
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
                && store.frontier.get(p).is_some_and(|f| {
                    f.suppressions
                        .iter()
                        .any(|k| k == CODE_DUPLICATION_KIND || k == BLANKET_SUPPRESSION)
                })
        })
    };

    let still_duplicated: FxHashSet<&String> = current.values().flatten().collect();

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
        store.clone_frontier.insert(fp, paths);
    }
}

fn prune_frontier(store: &mut ImpactStore, root: &Path) {
    store.frontier.retain(|rel, _| root.join(rel).exists());
    store
        .clone_frontier
        .retain(|_, paths| paths.iter().any(|p| root.join(p).exists()));
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
    /// Stored under `.fallow/` so agents can avoid cross-session nags.
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

    ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: store.enabled,
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
        "Resolution tracking is a local-developer signal: it accrues where\n\
         .fallow/impact.json persists across runs, not in ephemeral CI runners.\n",
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        assert!(decline_onboarding(root));
        assert!(!decline_onboarding(root));

        let store = load(root);
        assert!(store.onboarding_declined);
        assert_eq!(store.schema_version, STORE_SCHEMA_VERSION);
        assert!(root.join(".gitignore").exists());
        let report = build_report(&store);
        assert!(report.onboarding_declined);
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".fallow")).unwrap();
        std::fs::write(store_path(root), b"{ not valid json ][").unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".fallow")).unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        assert_eq!(load(root).clone_frontier.len(), 1);

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
        let dir = tempfile::tempdir().unwrap();
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
        assert_eq!(load(root).frontier.len(), 1);

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
        let dir = tempfile::tempdir().unwrap();
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
        assert_eq!(load(root).clone_frontier.len(), 1);

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
        assert!(store.clone_frontier.contains_key("dup:bbb"));
        assert!(!store.clone_frontier.contains_key("dup:aaa"));
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
}
