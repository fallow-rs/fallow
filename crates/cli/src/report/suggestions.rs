//! Command-level `next_steps[]` builder.
//!
//! Computes a small list of read-only, runnable follow-up commands from a run's
//! findings, surfaced at the JSON root (and as a one-line human `Next:` hint).
//! The purpose is to point agents and humans sideways to fallow's adjacent
//! verification capabilities (trace, complexity breakdown, audit, workspace
//! scoping) that telemetry shows agents rarely discover, because they act on the
//! output in front of them rather than on reference docs.
//!
//! Two hard contracts, both enforced by the tests in this module and by the
//! `next_step` constructor's debug assertions:
//!
//! 1. **Read-only.** A step NEVER suggests `fallow fix` or any mutating command.
//! 2. **Runnable, placeholder-free.** Every `command` runs as-is; it never
//!    contains an angle-bracket placeholder. Finding-derived values come from a
//!    real, deterministically-selected finding; values that cannot be made
//!    concrete (a coverage path) are dropped from v1 rather than shipped as a
//!    placeholder, and an unresolvable git ref omits its step entirely.

use std::path::Path;
use std::process::Command;

use fallow_core::results::AnalysisResults;
use fallow_output::{
    HealthNextStepsInput, ImpactDigestCounts,
    build_health_next_steps as build_health_next_steps_contract, impact_digest_summary,
};
use fallow_types::output::NextStep;

use crate::health_types::HealthReport;
use crate::output_dupes::DupesReportPayload;

/// Maximum number of next-steps emitted per envelope. Keeps the array a glance,
/// not a wall; the priority order decides which survive the cap.
const MAX_NEXT_STEPS: usize = 3;

/// Mutating verbs a next-step must never suggest (the read-only contract).
const MUTATING_VERBS: [&str; 5] = ["fix", "init", "hooks", "migrate", "setup-hooks"];

/// `FALLOW_SUGGESTIONS=off` (or `0`/`false`/`no`/`disabled`) disables next-steps
/// entirely. Default on. This is the documented escape hatch for CI consumers
/// that snapshot-diff raw `--format json` output; it reaches the spawned-CLI and
/// MCP surfaces without a CLI flag.
#[must_use]
pub fn suggestions_enabled() -> bool {
    suggestions_enabled_from(std::env::var("FALLOW_SUGGESTIONS").ok().as_deref())
}

/// Pure parse of the `FALLOW_SUGGESTIONS` value (kept separate so it is testable
/// without mutating process env, which is `unsafe` under edition 2024).
#[must_use]
fn suggestions_enabled_from(value: Option<&str>) -> bool {
    match value {
        Some(raw) => !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no" | "disabled"
        ),
        None => true,
    }
}

/// Construct a next-step, asserting the two contracts in debug builds so a new
/// trigger that violates them trips the test suite rather than shipping.
fn next_step(id: &str, command: String, reason: &str) -> NextStep {
    debug_assert!(
        !command.contains('<') && !command.contains('>'),
        "next-step command must be runnable (no placeholder): {command}"
    );
    debug_assert!(
        !command
            .split_whitespace()
            .any(|token| MUTATING_VERBS.contains(&token)),
        "next-step command must be read-only (no mutating verb): {command}"
    );
    NextStep {
        id: id.to_string(),
        command,
        reason: reason.to_string(),
    }
}

/// Project-root-relative, forward-slash path for embedding in a command string,
/// matching the wire form of finding paths.
fn relative_command_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

// ---------------------------------------------------------------------------
// Individual triggers. Each returns `Some(step)` only when its evidence exists.
// ---------------------------------------------------------------------------

/// `trace-unused-export`: verify an export is truly unused before deleting.
/// Uses the lexicographically smallest `(path, name)` finding so the embedded
/// command is deterministic across runs (independent of internal vec order).
fn trace_unused_export(results: &AnalysisResults, root: &Path) -> Option<NextStep> {
    let target = results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                relative_command_path(&finding.export.path, root),
                finding.export.export_name.clone(),
            )
        })
        .min()?;
    Some(next_step(
        "trace-unused-export",
        format!("fallow dead-code --trace {}:{}", target.0, target.1),
        "verify an export is truly unused before deleting",
    ))
}

/// `setup`: first-contact pointer for unconfigured projects. The command is the
/// read-only capability manifest (`fallow schema`), whose `task_matrix` and
/// commands list name the guided-setup surface (`init --agents`, the hooks
/// installer); the mutating commands themselves are never embedded here (the
/// read-only contract), the agent offers them to the user instead. Callers gate
/// this via [`setup_pointer_applicable`] so CI runs, configured projects, and
/// projects that declined onboarding never see it.
fn setup_pointer(offer_setup: bool) -> Option<NextStep> {
    if !offer_setup {
        return None;
    }
    Some(next_step(
        "setup",
        "fallow schema".to_string(),
        "fallow has no config here; the manifest lists guided-setup commands (agent guide, commit gate) to offer the user",
    ))
}

/// Shared first-contact gate for the `setup` next-step and the human setup hint
/// on bare `fallow`: the project has no fallow config (searched up to the repo
/// root, same as config loading), the run is not in CI, and onboarding has not
/// been declined for this project (`fallow init --decline`).
#[must_use]
pub fn setup_pointer_applicable(root: &Path) -> bool {
    root.exists()
        && fallow_config::FallowConfig::find_config_path(root).is_none()
        && !crate::telemetry::is_ci()
        && !crate::impact::load(root).onboarding_declined
}

/// One-line human setup hint for bare `fallow` output: the prose counterpart of
/// the `setup` next-step (agents get the JSON form, humans get this line).
/// Worded as an offer, not a deficiency: zero-config is a supported happy path.
pub const SETUP_HINT: &str = "Setup: `fallow init --agents` writes an agent guide; `fallow hooks install --target agent` adds a commit gate (hide this hint: `fallow init --decline`).";

/// `impact-report`: the periodic local value digest. Emitted at most once per
/// week per project (the cadence stamp lives in the impact store, not the
/// agent, so it is consistent across agents and sessions), only when impact
/// tracking is enabled and has non-zero value to report, never in CI. Unlike
/// every other trigger this one may surface on a CLEAN run: a clean project
/// after a period of gate containment is exactly the moment the value report
/// is informative.
fn impact_digest_step(digest: Option<crate::impact::ImpactDigest>) -> Option<NextStep> {
    let digest = digest?;
    Some(next_step(
        "impact-report",
        "fallow impact".to_string(),
        &format!(
            "local value report: {}; share the non-zero numbers with the user",
            digest_summary(digest)
        ),
    ))
}

/// Real-counter summary fragment shared by the next-step reason and the human
/// one-liner (the placeholder-free contract: numbers come from the store).
fn impact_counts(digest: crate::impact::ImpactDigest) -> ImpactDigestCounts {
    ImpactDigestCounts {
        containment_count: digest.containment_count,
        resolved_total: digest.resolved_total,
    }
}

fn digest_summary(digest: crate::impact::ImpactDigest) -> String {
    impact_digest_summary(impact_counts(digest))
}

/// One-line human counterpart of the `impact-report` next-step, printed with
/// the run summary on bare `fallow`.
#[must_use]
pub fn impact_digest_line(digest: crate::impact::ImpactDigest) -> String {
    format!(
        "Impact: {} (details: `fallow impact`).",
        digest_summary(digest)
    )
}

/// Read-and-stamp the due periodic impact digest for the envelope being built.
/// Returns `None` in CI or when suggestions are disabled, WITHOUT consuming the
/// cadence stamp, so the digest is never burned by a surface that will not
/// show it.
#[must_use]
pub fn due_impact_digest(root: &Path) -> Option<crate::impact::ImpactDigest> {
    if !suggestions_enabled() || crate::telemetry::is_ci() {
        return None;
    }
    crate::impact::take_due_digest(root)
}

/// `trace-clone`: see sibling locations and an extract-function suggestion for a
/// duplicated block. Uses the smallest fingerprint for run-to-run determinism.
fn trace_clone(payload: &DupesReportPayload) -> Option<NextStep> {
    let fingerprint = payload
        .clone_groups
        .iter()
        .map(|group| group.fingerprint.as_str())
        .min()?;
    Some(next_step(
        "trace-clone",
        format!("fallow dupes --trace {fingerprint}"),
        "see sibling locations and an extract-function suggestion",
    ))
}

/// `complexity-breakdown`: see the per-decision-point contributions behind a
/// high-complexity finding.
fn complexity_breakdown(report: &HealthReport) -> Option<NextStep> {
    if report.findings.is_empty() {
        return None;
    }
    Some(next_step(
        "complexity-breakdown",
        "fallow health --complexity-breakdown".to_string(),
        "see per-decision-point contributions for a hotspot",
    ))
}

/// `scope-workspaces`: scope a monorepo run to the packages touched since the
/// default branch. Emitted only when the project is a monorepo AND a concrete
/// default ref resolves, so the embedded ref is real (never a placeholder).
fn scope_workspaces(root: &Path) -> Option<NextStep> {
    if fallow_config::discover_workspaces(root).is_empty() {
        return None;
    }
    let reference = resolve_default_workspace_ref(root)?;
    Some(next_step(
        "scope-workspaces",
        format!("fallow dead-code --changed-workspaces {reference}"),
        "scope a monorepo run to the packages your branch touched",
    ))
}

/// `audit-changed`: gate only the files the current branch changed. `fallow
/// audit` auto-detects its base, so no ref needs embedding.
fn audit_changed(root: &Path) -> Option<NextStep> {
    if !fallow_core::churn::is_git_repo(root) {
        return None;
    }
    Some(next_step(
        "audit-changed",
        "fallow audit".to_string(),
        "gate only the files your branch changed (auto-detects the base)",
    ))
}

// ---------------------------------------------------------------------------
// Git ref resolution (self-contained; keeps `scope-workspaces` placeholder-free)
// ---------------------------------------------------------------------------

/// Resolve a concrete, human-readable default ref for `--changed-workspaces`.
/// Tries `origin/HEAD` then verifies `origin/main` / `origin/master`. Returns
/// the first that resolves, or `None` (in which case `scope-workspaces` is
/// omitted rather than shipping an unrunnable `origin/main` guess).
fn resolve_default_workspace_ref(root: &Path) -> Option<String> {
    if let Some(reference) = run_git(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        let reference = reference.trim();
        if !reference.is_empty() {
            return Some(reference.to_string());
        }
    }
    ["origin/main", "origin/master"]
        .into_iter()
        .find(|candidate| git_ref_exists(root, candidate))
        .map(str::to_string)
}

fn git_ref_exists(root: &Path, reference: &str) -> bool {
    Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

// ---------------------------------------------------------------------------
// Public per-command builders. Each no-ops when suggestions are disabled or the
// run is clean (no findings), so a clean run never emits `next_steps`, with one
// documented exception: a due `impact-report` digest may ride a clean run.
// ---------------------------------------------------------------------------

/// Next-steps for standalone `fallow dead-code`. `offer_setup` is the caller's
/// [`setup_pointer_applicable`] result (threaded as a parameter so the builders
/// stay free of env/filesystem probes and deterministic under test).
#[must_use]
pub fn build_dead_code_next_steps(
    results: &AnalysisResults,
    root: &Path,
    offer_setup: bool,
    digest: Option<crate::impact::ImpactDigest>,
) -> Vec<NextStep> {
    if !suggestions_enabled() {
        return Vec::new();
    }
    if results.total_issues() == 0 {
        return impact_digest_step(digest).into_iter().collect();
    }
    let mut steps: Vec<NextStep> = [
        setup_pointer(offer_setup),
        impact_digest_step(digest),
        trace_unused_export(results, root),
        scope_workspaces(root),
        audit_changed(root),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Next-steps for standalone `fallow health`. See [`build_dead_code_next_steps`]
/// for the `offer_setup` parameter contract.
#[must_use]
pub fn build_health_next_steps(
    report: &HealthReport,
    root: &Path,
    offer_setup: bool,
    digest: Option<crate::impact::ImpactDigest>,
) -> Vec<NextStep> {
    build_health_next_steps_contract(HealthNextStepsInput {
        suggestions_enabled: suggestions_enabled(),
        has_findings: !report.findings.is_empty(),
        offer_setup,
        impact_digest: digest.map(impact_counts),
        audit_changed: audit_changed(root).is_some(),
    })
}

/// Next-steps for standalone `fallow dupes`. See [`build_dead_code_next_steps`]
/// for the `offer_setup` parameter contract.
#[must_use]
pub fn build_dupes_next_steps(
    payload: &DupesReportPayload,
    root: &Path,
    offer_setup: bool,
    digest: Option<crate::impact::ImpactDigest>,
) -> Vec<NextStep> {
    if !suggestions_enabled() {
        return Vec::new();
    }
    if payload.clone_groups.is_empty() {
        return impact_digest_step(digest).into_iter().collect();
    }
    let mut steps: Vec<NextStep> = [
        setup_pointer(offer_setup),
        impact_digest_step(digest),
        trace_clone(payload),
        audit_changed(root),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Aggregated next-steps for bare `fallow` (combined). Candidates are pushed in
/// priority order, then capped. `trace-unused-export` leads because it is the
/// highest-value verification path; `scope-workspaces` is boosted above the
/// trace-clone / complexity tier so big-repo runs that trigger everything still
/// surface the rare monorepo-scoping capability instead of always dropping it
/// under the cap. `audit-changed` is last (broadly applicable, least specific).
#[must_use]
pub fn build_combined_next_steps(
    results: Option<&AnalysisResults>,
    dupes: Option<&DupesReportPayload>,
    health: Option<&HealthReport>,
    root: &Path,
    offer_setup: bool,
    digest: Option<crate::impact::ImpactDigest>,
) -> Vec<NextStep> {
    if !suggestions_enabled() {
        return Vec::new();
    }
    let has_findings = results.is_some_and(|r| r.total_issues() > 0)
        || dupes.is_some_and(|d| !d.clone_groups.is_empty())
        || health.is_some_and(|h| !h.findings.is_empty());
    if !has_findings {
        return impact_digest_step(digest).into_iter().collect();
    }
    let mut steps: Vec<NextStep> = [
        setup_pointer(offer_setup),
        impact_digest_step(digest),
        results.and_then(|r| trace_unused_export(r, root)),
        scope_workspaces(root),
        dupes.and_then(trace_clone),
        health.and_then(complexity_breakdown),
        audit_changed(root),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Next-steps for `fallow audit`. No `audit-changed` (audit IS the changed
/// scope) and no `scope-workspaces` (audit already gates the change). The
/// `check` tuple carries the changed-file analysis results plus the project root
/// so the trace anchor is made root-relative the same way every other surface
/// does it (in-memory finding paths are absolute; the wire form is relative).
#[must_use]
pub fn build_audit_next_steps(
    check: Option<(&AnalysisResults, &Path)>,
    complexity: Option<&HealthReport>,
) -> Vec<NextStep> {
    if !suggestions_enabled() {
        return Vec::new();
    }
    let mut steps: Vec<NextStep> = [
        check.and_then(|(results, root)| trace_unused_export(results, root)),
        complexity.and_then(complexity_breakdown),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// The single highest-priority next-step for the human `Next:` line, computed
/// from the same candidates and ordering as the combined JSON array so a human
/// and an agent on the same run never see a contradictory top step. The `setup`
/// pointer is deliberately excluded here (`offer_setup: false`): humans get the
/// dedicated prose [`SETUP_HINT`] line instead, so the `Next:` slot always
/// shows an analysis follow-up.
#[must_use]
pub fn top_combined_next_step(
    results: Option<&AnalysisResults>,
    dupes: Option<&DupesReportPayload>,
    health: Option<&HealthReport>,
    root: &Path,
) -> Option<NextStep> {
    build_combined_next_steps(results, dupes, health, root, false, None)
        .into_iter()
        .next()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::health_types::{
        ComplexityViolation, ExceededThreshold, FindingSeverity, HealthFinding, HealthReport,
    };
    use fallow_types::output_dead_code::UnusedExportFinding;
    use fallow_types::results::{AnalysisResults, UnusedExport};

    use super::*;

    fn unused_export(path: &str, name: &str) -> UnusedExportFinding {
        UnusedExportFinding::with_actions(UnusedExport {
            path: PathBuf::from(path),
            export_name: name.to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        })
    }

    fn results_with_exports(exports: Vec<UnusedExportFinding>) -> AnalysisResults {
        AnalysisResults {
            unused_exports: exports,
            ..AnalysisResults::default()
        }
    }

    fn health_report_with_finding() -> HealthReport {
        HealthReport {
            findings: vec![HealthFinding::from(ComplexityViolation {
                path: PathBuf::from("/project/src/hot.ts"),
                name: "hot".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 21,
                cognitive: 16,
                line_count: 42,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: ExceededThreshold::Both,
                severity: FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
                coverage_source: None,
                inherited_from: None,
                component_rollup: None,
                contributions: Vec::new(),
                effective_thresholds: None,
                threshold_source: None,
            })],
            ..HealthReport::default()
        }
    }

    fn assert_valid(step: &NextStep) {
        assert!(
            !step.command.contains('<') && !step.command.contains('>'),
            "command must be placeholder-free: {}",
            step.command
        );
        assert!(
            !step
                .command
                .split_whitespace()
                .any(|token| MUTATING_VERBS.contains(&token)),
            "command must be read-only: {}",
            step.command
        );
    }

    #[test]
    fn trace_unused_export_emits_runnable_relative_command() {
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![unused_export("/project/src/util.ts", "foo")]);
        let step = trace_unused_export(&results, &root).expect("step");
        assert_eq!(step.id, "trace-unused-export");
        assert_eq!(step.command, "fallow dead-code --trace src/util.ts:foo");
        assert_valid(&step);
    }

    #[test]
    fn trace_unused_export_is_deterministic_regardless_of_vec_order() {
        let root = PathBuf::from("/project");
        let forward = results_with_exports(vec![
            unused_export("/project/src/b.ts", "beta"),
            unused_export("/project/src/a.ts", "alpha"),
        ]);
        let reverse = results_with_exports(vec![
            unused_export("/project/src/a.ts", "alpha"),
            unused_export("/project/src/b.ts", "beta"),
        ]);
        let a = trace_unused_export(&forward, &root).expect("step");
        let b = trace_unused_export(&reverse, &root).expect("step");
        assert_eq!(a.command, b.command);
        assert_eq!(a.command, "fallow dead-code --trace src/a.ts:alpha");
    }

    #[test]
    fn clean_run_emits_no_next_steps() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        assert!(build_dead_code_next_steps(&results, &root, true, None).is_empty());
    }

    #[test]
    fn setup_pointer_emits_only_when_applicable() {
        assert!(setup_pointer(false).is_none());
        let step = setup_pointer(true).expect("step");
        assert_eq!(step.id, "setup");
        assert_eq!(step.command, "fallow schema");
        assert_valid(&step);
    }

    #[test]
    fn setup_pointer_gate_ignores_nonexistent_roots() {
        assert!(!setup_pointer_applicable(Path::new(
            "/fallow-test-project-does-not-exist"
        )));
    }

    #[test]
    fn setup_pointer_leads_when_offered() {
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![unused_export("/project/src/a.ts", "alpha")]);
        let steps = build_dead_code_next_steps(&results, &root, true, None);
        assert_eq!(steps.first().map(|s| s.id.as_str()), Some("setup"));
        let steps = build_dead_code_next_steps(&results, &root, false, None);
        assert!(steps.iter().all(|s| s.id != "setup"));
    }

    #[test]
    fn human_top_step_never_surfaces_setup() {
        let results = results_with_exports(vec![unused_export("/project/src/a.ts", "alpha")]);
        let top = top_combined_next_step(Some(&results), None, None, Path::new("/project"));
        if let Some(step) = top {
            assert_ne!(step.id, "setup");
        }
    }

    fn digest(containment: usize, resolved: usize) -> crate::impact::ImpactDigest {
        crate::impact::ImpactDigest {
            containment_count: containment,
            resolved_total: resolved,
        }
    }

    #[test]
    fn impact_digest_step_carries_real_counters() {
        assert!(impact_digest_step(None).is_none());
        let step = impact_digest_step(Some(digest(4, 12))).expect("step");
        assert_eq!(step.id, "impact-report");
        assert_eq!(step.command, "fallow impact");
        assert!(step.reason.contains("4 commits contained at the gate"));
        assert!(step.reason.contains("12 findings resolved"));
        assert_valid(&step);
        let singular = impact_digest_step(Some(digest(1, 0))).expect("step");
        assert!(singular.reason.contains("1 commit contained at the gate"));
        assert!(!singular.reason.contains("resolved"));
    }

    #[test]
    fn due_digest_rides_a_clean_run() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let steps = build_dead_code_next_steps(&results, &root, true, Some(digest(2, 0)));
        assert_eq!(steps.len(), 1, "clean run carries ONLY the digest");
        assert_eq!(steps[0].id, "impact-report");
    }

    #[test]
    fn digest_follows_setup_on_dirty_runs() {
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![unused_export("/project/src/a.ts", "alpha")]);
        let steps = build_dead_code_next_steps(&results, &root, true, Some(digest(2, 3)));
        let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids[0], "setup");
        assert_eq!(ids[1], "impact-report");
    }

    #[test]
    fn health_steps_keep_complexity_breakdown_from_output_contract() {
        let report = health_report_with_finding();
        let steps = build_health_next_steps(&report, Path::new("/project"), false, None);
        let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();

        assert_eq!(ids, ["complexity-breakdown"]);
        assert_valid(&steps[0]);
    }

    #[test]
    fn impact_digest_line_renders_counters() {
        let line = impact_digest_line(digest(2, 1));
        assert_eq!(
            line,
            "Impact: 2 commits contained at the gate, 1 finding resolved (details: `fallow impact`)."
        );
    }

    #[test]
    fn suggestions_enabled_parses_off_values() {
        for off in ["off", "0", "false", "no", "disabled", "OFF", " Off "] {
            assert!(!suggestions_enabled_from(Some(off)), "{off} should disable");
        }
        for on in ["on", "1", "true", "", "yes"] {
            assert!(suggestions_enabled_from(Some(on)), "{on} should enable");
        }
        assert!(suggestions_enabled_from(None), "default is enabled");
    }

    #[test]
    fn every_emitted_command_is_runnable_and_read_only() {
        // Exercise every data-driven trigger and assert both contracts.
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![unused_export("/project/src/a.ts", "alpha")]);
        let mut all = Vec::new();
        all.extend(trace_unused_export(&results, &root));
        // Static-command triggers (no findings needed to inspect the string).
        all.push(next_step("audit-changed", "fallow audit".to_string(), "x"));
        all.push(next_step(
            "scope-workspaces",
            "fallow dead-code --changed-workspaces origin/main".to_string(),
            "x",
        ));
        all.push(next_step(
            "complexity-breakdown",
            "fallow health --complexity-breakdown".to_string(),
            "x",
        ));
        all.push(next_step(
            "trace-clone",
            "fallow dupes --trace dup:abcd1234".to_string(),
            "x",
        ));
        all.extend(setup_pointer(true));
        assert!(!all.is_empty());
        for step in &all {
            assert_valid(step);
        }
    }

    #[test]
    fn dead_code_steps_capped_at_three() {
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![unused_export("/project/src/a.ts", "alpha")]);
        // Even if git/workspaces/setup add candidates, the cap holds.
        let steps = build_dead_code_next_steps(&results, &root, true, None);
        assert!(steps.len() <= MAX_NEXT_STEPS);
    }
}
