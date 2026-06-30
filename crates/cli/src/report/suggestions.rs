//! Runtime fact adapters for `next_steps[]` builders.
//!
//! The stable command strings, ordering, caps, and read-only contract live in
//! `fallow-output`. This module keeps the CLI-specific probes: environment
//! toggles, project setup state, git refs, and changed-branch applicability.

use std::path::Path;
use std::process::Command;

use fallow_api::DupesReportPayload;
use fallow_output::{
    CombinedNextStepsInput, DeadCodeNextStepsInput, DupesNextStepsInput, HealthNextStepsInput,
    ImpactDigestCounts, build_combined_next_steps as build_combined_next_steps_contract,
    build_dead_code_next_steps as build_dead_code_next_steps_contract,
    build_dupes_next_steps as build_dupes_next_steps_contract, impact_digest_summary,
    trace_unused_export_input,
};
use fallow_types::output::NextStep;
use fallow_types::results::AnalysisResults;

use fallow_output::HealthReport;

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

/// Real-counter summary fragment shared by the next-step reason and the human
/// one-liner. The output crate owns the `impact-report` command contract.
pub fn impact_counts(digest: crate::impact::ImpactDigest) -> ImpactDigestCounts {
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

fn default_workspace_ref_for_next_step(root: &Path) -> Option<String> {
    if fallow_config::discover_workspaces(root).is_empty() {
        return None;
    }
    resolve_default_workspace_ref(root)
}

/// `audit-changed`: gate only the files the current branch changed. `fallow
/// audit` auto-detects its base, so no ref needs embedding.
pub fn audit_changed_applicable(root: &Path) -> bool {
    fallow_engine::is_git_repo(root)
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
    let workspace_ref = default_workspace_ref_for_next_step(root);
    build_dead_code_next_steps_contract(DeadCodeNextStepsInput {
        suggestions_enabled: suggestions_enabled(),
        results,
        root,
        offer_setup,
        impact_digest: digest.map(impact_counts),
        workspace_ref: workspace_ref.as_deref(),
        audit_changed: audit_changed_applicable(root),
    })
}

/// Next-steps for standalone `fallow health`. See [`build_dead_code_next_steps`]
/// for the `offer_setup` parameter contract.
#[must_use]
pub fn health_next_steps_input(
    report: &HealthReport,
    root: &Path,
    offer_setup: bool,
    digest: Option<crate::impact::ImpactDigest>,
) -> HealthNextStepsInput {
    fallow_output::build_health_next_steps_input(
        report,
        suggestions_enabled(),
        offer_setup,
        digest.map(impact_counts),
        audit_changed_applicable(root),
    )
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
    let clone_fingerprints = payload
        .clone_groups
        .iter()
        .map(|group| group.fingerprint.as_str())
        .collect::<Vec<_>>();
    build_dupes_next_steps_contract(DupesNextStepsInput {
        suggestions_enabled: suggestions_enabled(),
        clone_fingerprints: &clone_fingerprints,
        offer_setup,
        impact_digest: digest.map(impact_counts),
        audit_changed: audit_changed_applicable(root),
    })
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
    let workspace_ref = default_workspace_ref_for_next_step(root);
    let clone_fingerprints = dupes
        .map(|payload| {
            payload
                .clone_groups
                .iter()
                .map(|group| group.fingerprint.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    build_combined_next_steps_contract(&CombinedNextStepsInput {
        suggestions_enabled: suggestions_enabled(),
        has_dead_code_findings: results.is_some_and(|r| r.total_issues() > 0),
        trace_unused_export: results.and_then(|r| trace_unused_export_input(r, root)),
        workspace_ref: workspace_ref.as_deref(),
        clone_fingerprints: &clone_fingerprints,
        has_complexity_findings: health.is_some_and(|h| !h.findings.is_empty()),
        offer_setup,
        impact_digest: digest.map(impact_counts),
        audit_changed: audit_changed_applicable(root),
    })
}

/// Next-steps for `fallow audit`. No `audit-changed` (audit IS the changed
/// scope) and no `scope-workspaces` (audit already gates the change). The
/// `check` tuple carries the changed-file analysis results plus the project root
/// so the trace anchor is made root-relative the same way every other surface
/// does it (in-memory finding paths are absolute; the wire form is relative).
#[must_use]
#[cfg(test)]
pub fn build_audit_next_steps(
    check: Option<(&AnalysisResults, &Path)>,
    complexity: Option<&HealthReport>,
) -> Vec<NextStep> {
    fallow_output::build_audit_next_steps(&fallow_output::build_audit_next_steps_input(
        check,
        complexity,
        suggestions_enabled(),
    ))
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

    use fallow_output::build_health_next_steps as build_health_next_steps_contract;
    use fallow_output::{
        ComplexityViolation, ExceededThreshold, FindingSeverity, HealthFinding, HealthReport,
    };
    use fallow_types::duplicates::{
        CloneGroup, CloneInstance, DuplicationReport, DuplicationStats,
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

    fn clone_instance(path: &str, fragment: &str) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(path),
            start_line: 1,
            end_line: 8,
            start_col: 0,
            end_col: 0,
            fragment: fragment.to_string(),
        }
    }

    fn dupes_payload() -> DupesReportPayload {
        let group = CloneGroup {
            instances: vec![
                clone_instance("/project/src/a.ts", "export const shared = 1;"),
                clone_instance("/project/src/b.ts", "export const shared = 1;"),
            ],
            token_count: 20,
            line_count: 8,
        };
        DupesReportPayload::from_report(&DuplicationReport {
            clone_groups: vec![group],
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats: DuplicationStats::default(),
        })
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
        const MUTATING_VERBS: [&str; 5] = ["fix", "init", "hooks", "migrate", "setup-hooks"];

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
    fn audit_next_steps_emit_runnable_relative_trace_command() {
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![unused_export("/project/src/util.ts", "foo")]);
        let steps = build_audit_next_steps(Some((&results, &root)), None);

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "trace-unused-export");
        assert_eq!(steps[0].command, "fallow dead-code --trace src/util.ts:foo");
        assert_valid(&steps[0]);
    }

    #[test]
    fn audit_next_steps_select_deterministic_trace_target() {
        let root = PathBuf::from("/project");
        let forward = results_with_exports(vec![
            unused_export("/project/src/b.ts", "beta"),
            unused_export("/project/src/a.ts", "alpha"),
        ]);
        let reverse = results_with_exports(vec![
            unused_export("/project/src/a.ts", "alpha"),
            unused_export("/project/src/b.ts", "beta"),
        ]);
        let a = build_audit_next_steps(Some((&forward, &root)), None);
        let b = build_audit_next_steps(Some((&reverse, &root)), None);
        assert_eq!(a[0].command, b[0].command);
        assert_eq!(a[0].command, "fallow dead-code --trace src/a.ts:alpha");
    }

    #[test]
    fn clean_run_emits_no_next_steps() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        assert!(build_dead_code_next_steps(&results, &root, true, None).is_empty());
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
        let steps = build_health_next_steps_contract(health_next_steps_input(
            &report,
            Path::new("/project"),
            false,
            None,
        ));
        let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();

        assert_eq!(ids, ["complexity-breakdown"]);
        assert_valid(&steps[0]);
    }

    #[test]
    fn health_next_steps_input_feeds_output_contract_builder() {
        let report = health_report_with_finding();
        let input =
            health_next_steps_input(&report, Path::new("/project"), true, Some(digest(2, 1)));

        assert!(input.suggestions_enabled);
        assert!(input.has_findings);
        assert!(input.offer_setup);
        assert_eq!(
            input.impact_digest,
            Some(ImpactDigestCounts {
                containment_count: 2,
                resolved_total: 1,
            })
        );

        let steps = build_health_next_steps_contract(input);
        let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["setup", "impact-report", "complexity-breakdown"]);
    }

    #[test]
    fn dupes_next_steps_route_payload_fingerprints_to_output_contract() {
        let payload = dupes_payload();

        let steps = build_dupes_next_steps(&payload, Path::new("/project"), false, None);

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "trace-clone");
        assert!(steps[0].command.starts_with("fallow dupes --trace dup:"));
        assert_valid(&steps[0]);
    }

    #[test]
    fn combined_next_steps_route_payload_facts_to_output_contract() {
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![
            unused_export("/project/src/b.ts", "beta"),
            unused_export("/project/src/a.ts", "alpha"),
        ]);
        let payload = dupes_payload();
        let report = health_report_with_finding();

        let steps = build_combined_next_steps(
            Some(&results),
            Some(&payload),
            Some(&report),
            &root,
            true,
            Some(digest(2, 1)),
        );
        let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();

        assert_eq!(ids, ["setup", "impact-report", "trace-unused-export"]);
        assert_eq!(steps[2].command, "fallow dead-code --trace src/a.ts:alpha");
        for step in &steps {
            assert_valid(step);
        }
    }

    #[test]
    fn audit_next_steps_route_payload_facts_to_output_contract() {
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![
            unused_export("/project/src/b.ts", "beta"),
            unused_export("/project/src/a.ts", "alpha"),
        ]);
        let report = health_report_with_finding();

        let steps = build_audit_next_steps(Some((&results, &root)), Some(&report));
        let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();

        assert_eq!(ids, ["trace-unused-export", "complexity-breakdown"]);
        assert_eq!(steps[0].command, "fallow dead-code --trace src/a.ts:alpha");
        for step in &steps {
            assert_valid(step);
        }
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
        // Exercise CLI adapters and assert the output-owned command contracts.
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![unused_export("/project/src/a.ts", "alpha")]);
        let payload = dupes_payload();
        let report = health_report_with_finding();
        let mut all = Vec::new();
        all.extend(build_audit_next_steps(Some((&results, &root)), None));
        all.extend(build_dead_code_next_steps(
            &results,
            &root,
            true,
            Some(digest(2, 1)),
        ));
        all.extend(build_dupes_next_steps(&payload, &root, false, None));
        all.extend(build_health_next_steps_contract(health_next_steps_input(
            &report, &root, false, None,
        )));
        all.extend(build_combined_next_steps(
            Some(&results),
            Some(&payload),
            Some(&report),
            &root,
            true,
            Some(digest(2, 1)),
        ));
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
        assert!(steps.len() <= 3);
    }
}
