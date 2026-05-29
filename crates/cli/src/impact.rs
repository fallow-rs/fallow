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
//! error. Per-finding resolved/suppressed/moved attribution is intentionally NOT
//! part of v1: fallow cannot yet distinguish "code removed" from "a fallow-ignore
//! was added", and a value report that might count a suppression as a win is
//! worse than one that does not. That attribution lands in a later version once
//! active-suppression state is captured.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audit::{AuditSummary, AuditVerdict};

/// Schema version for the rolling impact store.
const IMPACT_SCHEMA_VERSION: u32 = 1;

/// Upper bound on retained per-run records. The store is a single compacted file,
/// so this only bounds memory/disk, not file count. Oldest records are dropped first.
const MAX_RECORDS: usize = 200;

/// Upper bound on retained containment events (oldest dropped first).
const MAX_CONTAINMENT: usize = 200;

/// Tolerance (in absolute issue count) below which a trend is "stable" rather
/// than improving/declining. A delta of a single finding is noise.
const TREND_TOLERANCE: i64 = 0;

/// File name of the rolling impact store inside `.fallow/`.
const STORE_FILE: &str = "impact.json";

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
    match serde_json::from_str(&content) {
        Ok(store) => store,
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
        store.schema_version = IMPACT_SCHEMA_VERSION;
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
    let _ = std::fs::write(&path, contents);
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
pub fn record_audit_run(
    root: &Path,
    summary: &AuditSummary,
    verdict: AuditVerdict,
    gate: bool,
    git_sha: Option<&str>,
    version: &str,
    timestamp: &str,
) {
    let mut store = load(root);
    if !store.enabled {
        return;
    }

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

/// The rendered impact report, derived purely from the store (no analysis run).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow impact --format json"))]
pub struct ImpactReport {
    pub enabled: bool,
    pub record_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_recorded: Option<String>,
    /// Git SHA of the most recent recorded run, so a consumer can tell which
    /// commit the `surfacing` counts belong to. None when the latest run had no
    /// SHA (not a git repo) or there are no records yet.
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

    ImpactReport {
        enabled: store.enabled,
        record_count: store.records.len(),
        first_recorded: store.first_recorded.clone(),
        latest_git_sha,
        surfacing,
        trend,
        containment_count: store.containment.len(),
        recent_containment,
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

    out.push('\n');
    out.push_str(&format!(
        "Based on {} recorded run{} since {}. Local-only; never uploaded.\n",
        report.record_count,
        plural(report.record_count),
        report.first_recorded.as_deref().unwrap_or("the first run"),
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
    out.push_str(&format!(
        "\n_Based on {} recorded run{}. Local-only._\n",
        report.record_count,
        plural(report.record_count),
    ));
    out
}

const fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
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

    #[test]
    fn disabled_store_does_not_record() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Not enabled: recording is a no-op.
        record_audit_run(
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
        record_audit_run(
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
        record_audit_run(
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
        record_audit_run(
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
        record_audit_run(
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
        record_audit_run(
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
}
