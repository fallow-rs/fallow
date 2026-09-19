//! Pure builders for JSON `next_steps[]` entries.
//!
//! Runtime probes stay with callers. This module owns the stable command,
//! ordering, capping, and read-only contracts once a caller has already decided
//! which signals apply.

use fallow_types::output::NextStep;
use fallow_types::results::AnalysisResults;
use std::path::Path;

use crate::{BaselineScopeReasons, HealthReport};

const MAX_NEXT_STEPS: usize = 3;
const MUTATING_VERBS: [&str; 5] = ["fix", "init", "hooks", "migrate", "setup-hooks"];

/// The loaded baseline a narrowed run could not judge, for the
/// `recheck-baseline` next step.
///
/// A run narrowed to part of the project compares a whole-project baseline
/// against a slice of it, so both the advisory and the gate stand down and the
/// baseline can rot unnoticed for as long as every run is narrowed. The step
/// names the one run that CAN judge it. It stays read-only, like every other
/// entry: it re-reads the baseline and reports, it never re-saves.
///
/// Offered only when dropping every channel in `scope_reasons` widens the run,
/// because the step's command carries `--baseline` and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaselineRecheckInput<'a> {
    /// The command whose baseline this is, as `fallow <command>` spells it.
    pub command: &'a str,
    /// The `--baseline` path as the caller wrote it. The envelope's
    /// root-prefix post-process renders it root-relative when it lives under
    /// the analyzed root and leaves it absolute otherwise, so the command stays
    /// runnable from the root either way.
    pub path: &'a str,
    /// Entries the loaded baseline carried. Zero means there is nothing to
    /// re-check.
    pub baseline_entries: usize,
    /// The channels that narrowed this run, named in the reason so the two
    /// cannot drift from the published `scope_reasons`. A channel the printed
    /// command cannot drop suppresses the step entirely.
    pub scope_reasons: BaselineScopeReasons,
}

/// Local impact digest counters used to render the `impact-report` next step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpactDigestCounts {
    /// Commits contained at the gate according to the local impact digest.
    pub containment_count: usize,
    /// Findings resolved according to the local impact digest.
    pub resolved_total: usize,
}

/// Runtime-independent inputs for standalone dead-code next steps.
#[derive(Debug, Clone, Copy)]
pub struct DeadCodeNextStepsInput<'a> {
    /// False when `FALLOW_SUGGESTIONS=off`; suppresses all steps.
    pub suggestions_enabled: bool,
    /// Analysis results the steps are derived from.
    pub results: &'a AnalysisResults,
    /// Analysis root, used to render trace targets relative.
    pub root: &'a Path,
    /// Offer the guided-setup pointer because no fallow config exists yet.
    pub offer_setup: bool,
    /// Local impact digest counters, when a digest is available.
    pub impact_digest: Option<ImpactDigestCounts>,
    /// Workspace name to scope suggested commands to, when analysing one.
    pub workspace_ref: Option<&'a str>,
    /// Offer `fallow audit` because the working tree has changed files.
    pub audit_changed: bool,
    /// The project has external plugins declared; when files are also unused,
    /// route the agent to `fallow plugin-check` to verify what they seed.
    pub has_external_plugins: bool,
    /// The loaded baseline this run was too narrow to judge, when there is one.
    pub baseline_recheck: Option<BaselineRecheckInput<'a>>,
}

/// Runtime-independent inputs for standalone duplication next steps.
#[derive(Debug, Clone, Copy)]
pub struct DupesNextStepsInput<'a> {
    /// False when `FALLOW_SUGGESTIONS=off`; suppresses all steps.
    pub suggestions_enabled: bool,
    /// Clone fingerprints from this run, used for the suppress example.
    pub clone_fingerprints: &'a [&'a str],
    /// Offer the guided-setup pointer because no fallow config exists yet.
    pub offer_setup: bool,
    /// Local impact digest counters, when a digest is available.
    pub impact_digest: Option<ImpactDigestCounts>,
    /// Offer `fallow audit` because the working tree has changed files.
    pub audit_changed: bool,
    /// The loaded baseline this run was too narrow to judge, when there is one.
    pub baseline_recheck: Option<BaselineRecheckInput<'a>>,
}

/// Deterministic unused-export trace target selected by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceUnusedExportInput {
    /// Root-relative path of the file declaring the export.
    pub path: String,
    /// Name of the unused export to trace.
    pub export_name: String,
}

/// Runtime-independent inputs for bare `fallow` combined next steps.
#[derive(Debug, Clone)]
pub struct CombinedNextStepsInput<'a> {
    /// False when `FALLOW_SUGGESTIONS=off`; suppresses all steps.
    pub suggestions_enabled: bool,
    /// The dead-code section reported findings.
    pub has_dead_code_findings: bool,
    /// Unused-export trace target, when the caller selected one.
    pub trace_unused_export: Option<TraceUnusedExportInput>,
    /// Workspace name to scope suggested commands to, when analysing one.
    pub workspace_ref: Option<&'a str>,
    /// Clone fingerprints from this run, used for the suppress example.
    pub clone_fingerprints: &'a [&'a str],
    /// The health section reported complexity findings.
    pub has_complexity_findings: bool,
    /// Offer the guided-setup pointer because no fallow config exists yet.
    pub offer_setup: bool,
    /// Local impact digest counters, when a digest is available.
    pub impact_digest: Option<ImpactDigestCounts>,
    /// Offer `fallow audit` because the working tree has changed files.
    pub audit_changed: bool,
    /// External plugins are declared for this project.
    pub has_external_plugins: bool,
    /// The run reported unused files (the signal a plugin may be misconfigured).
    pub has_unused_files: bool,
}

/// Runtime-independent inputs for audit next steps.
#[derive(Debug, Clone)]
pub struct AuditNextStepsInput {
    /// False when `FALLOW_SUGGESTIONS=off`; suppresses all steps.
    pub suggestions_enabled: bool,
    /// Unused-export trace target, when the caller selected one.
    pub trace_unused_export: Option<TraceUnusedExportInput>,
    /// The audit reported complexity findings.
    pub has_complexity_findings: bool,
}

/// Runtime-independent inputs for standalone health next steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthNextStepsInput<'a> {
    /// False when `FALLOW_SUGGESTIONS=off`; suppresses all steps.
    pub suggestions_enabled: bool,
    /// The health report contains findings.
    pub has_findings: bool,
    /// Offer the guided-setup pointer because no fallow config exists yet.
    pub offer_setup: bool,
    /// Local impact digest counters, when a digest is available.
    pub impact_digest: Option<ImpactDigestCounts>,
    /// Offer `fallow audit` because the working tree has changed files.
    pub audit_changed: bool,
    /// The loaded baseline this run was too narrow to judge, when there is one.
    pub baseline_recheck: Option<BaselineRecheckInput<'a>>,
}

/// Build standalone health next-step inputs from a typed health report plus
/// caller-supplied runtime probes.
#[must_use]
pub fn build_health_next_steps_input<'a>(
    report: &HealthReport,
    suggestions_enabled: bool,
    offer_setup: bool,
    impact_digest: Option<ImpactDigestCounts>,
    audit_changed: bool,
    baseline_recheck: Option<BaselineRecheckInput<'a>>,
) -> HealthNextStepsInput<'a> {
    HealthNextStepsInput {
        suggestions_enabled,
        has_findings: !report.findings.is_empty(),
        offer_setup,
        impact_digest,
        audit_changed,
        baseline_recheck,
    }
}

/// Render the human-readable impact counter summary shared by JSON and human
/// output surfaces.
#[must_use]
pub fn impact_digest_summary(digest: ImpactDigestCounts) -> String {
    let mut parts = Vec::new();
    if digest.containment_count > 0 {
        parts.push(format!(
            "{} commit{} contained at the gate",
            digest.containment_count,
            if digest.containment_count == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    if digest.resolved_total > 0 {
        parts.push(format!(
            "{} finding{} resolved",
            digest.resolved_total,
            if digest.resolved_total == 1 { "" } else { "s" }
        ));
    }
    parts.join(", ")
}

/// Next-steps for standalone `fallow health`.
#[must_use]
pub fn build_health_next_steps(input: HealthNextStepsInput<'_>) -> Vec<NextStep> {
    if !input.suggestions_enabled {
        return Vec::new();
    }
    let recheck = recheck_baseline(input.baseline_recheck);
    if !input.has_findings {
        return recheck
            .into_iter()
            .chain(impact_digest_step(input.impact_digest))
            .collect();
    }

    let mut steps: Vec<NextStep> = [
        recheck,
        setup_pointer(input.offer_setup),
        impact_digest_step(input.impact_digest),
        complexity_breakdown(input.has_findings),
        audit_changed(input.audit_changed),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Next-steps for standalone `fallow dead-code`.
#[must_use]
pub fn build_dead_code_next_steps(input: DeadCodeNextStepsInput<'_>) -> Vec<NextStep> {
    if !input.suggestions_enabled {
        return Vec::new();
    }
    let recheck = recheck_baseline(input.baseline_recheck);
    if input.results.total_issues() == 0 {
        return recheck
            .into_iter()
            .chain(impact_digest_step(input.impact_digest))
            .collect();
    }

    let mut steps: Vec<NextStep> = [
        recheck,
        verify_plugins(input.has_external_plugins && !input.results.unused_files.is_empty()),
        setup_pointer(input.offer_setup),
        impact_digest_step(input.impact_digest),
        trace_unused_export(input.results, input.root),
        scope_workspaces(input.workspace_ref),
        audit_changed(input.audit_changed),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Next-steps for standalone `fallow dupes`.
#[must_use]
pub fn build_dupes_next_steps(input: DupesNextStepsInput<'_>) -> Vec<NextStep> {
    if !input.suggestions_enabled {
        return Vec::new();
    }
    let recheck = recheck_baseline(input.baseline_recheck);
    if input.clone_fingerprints.is_empty() {
        return recheck
            .into_iter()
            .chain(impact_digest_step(input.impact_digest))
            .collect();
    }

    let mut steps: Vec<NextStep> = [
        recheck,
        setup_pointer(input.offer_setup),
        impact_digest_step(input.impact_digest),
        trace_clone(input.clone_fingerprints),
        audit_changed(input.audit_changed),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Aggregated next-steps for bare `fallow` combined output.
#[must_use]
pub fn build_combined_next_steps(input: &CombinedNextStepsInput<'_>) -> Vec<NextStep> {
    if !input.suggestions_enabled {
        return Vec::new();
    }
    let has_findings = input.has_dead_code_findings
        || !input.clone_fingerprints.is_empty()
        || input.has_complexity_findings;
    if !has_findings {
        return impact_digest_step(input.impact_digest)
            .into_iter()
            .collect();
    }

    let mut steps: Vec<NextStep> = [
        verify_plugins(input.has_external_plugins && input.has_unused_files),
        setup_pointer(input.offer_setup),
        impact_digest_step(input.impact_digest),
        trace_unused_export_from_input(input.trace_unused_export.as_ref()),
        scope_workspaces(input.workspace_ref),
        trace_clone(input.clone_fingerprints),
        complexity_breakdown(input.has_complexity_findings),
        audit_changed(input.audit_changed),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Next-steps for `fallow audit`.
#[must_use]
pub fn build_audit_next_steps(input: &AuditNextStepsInput) -> Vec<NextStep> {
    if !input.suggestions_enabled {
        return Vec::new();
    }

    let mut steps: Vec<NextStep> = [
        trace_unused_export_from_input(input.trace_unused_export.as_ref()),
        complexity_breakdown(input.has_complexity_findings),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Build audit next-step inputs from typed analysis payloads plus the
/// caller-supplied runtime suggestions gate.
#[must_use]
pub fn build_audit_next_steps_input(
    check: Option<(&AnalysisResults, &Path)>,
    complexity: Option<&HealthReport>,
    suggestions_enabled: bool,
) -> AuditNextStepsInput {
    AuditNextStepsInput {
        suggestions_enabled,
        trace_unused_export: check
            .and_then(|(results, root)| trace_unused_export_input(results, root)),
        has_complexity_findings: complexity.is_some_and(|report| !report.findings.is_empty()),
    }
}

fn relative_command_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Select the deterministic unused-export target used by read-only trace
/// next-step commands.
#[must_use]
pub fn trace_unused_export_input(
    results: &AnalysisResults,
    root: &Path,
) -> Option<TraceUnusedExportInput> {
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
    Some(TraceUnusedExportInput {
        path: target.0,
        export_name: target.1,
    })
}

fn trace_unused_export(results: &AnalysisResults, root: &Path) -> Option<NextStep> {
    trace_unused_export_from_input(trace_unused_export_input(results, root).as_ref())
}

fn trace_unused_export_from_input(target: Option<&TraceUnusedExportInput>) -> Option<NextStep> {
    let target = target?;
    Some(next_step(
        "trace-unused-export",
        format!(
            "fallow dead-code --trace {}:{}",
            target.path, target.export_name
        ),
        "verify an export is truly unused before deleting",
    ))
}

fn verify_plugins(applicable: bool) -> Option<NextStep> {
    applicable.then(|| {
        next_step(
            "verify-plugins",
            "fallow plugin-check --format json".to_string(),
            "external plugins are active and files are unused; verify what they seed",
        )
    })
}

fn trace_clone(fingerprints: &[&str]) -> Option<NextStep> {
    let fingerprint = fingerprints.iter().copied().min()?;
    Some(next_step(
        "trace-clone",
        format!("fallow dupes --trace {fingerprint}"),
        "see sibling locations and an extract-function suggestion",
    ))
}

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

/// The read-only re-read of a baseline this run was too narrow to judge.
///
/// Silent on an unscoped run, because there the advisory and the gate already
/// spoke, and silent on an empty baseline, because there is nothing to
/// re-check.
///
/// Silent as well when any channel that narrowed the run survives the printed
/// command. The command is rebuilt from scratch and carries nothing but
/// `--baseline`, so it drops only narrowing that came from a flag. Production
/// mode and workspace scoping also resolve from the project config and the
/// environment, where the same command would come back just as narrow and
/// re-emit this step without end. An entry that cannot judge the baseline is
/// worse than no entry: `next_steps` is a contract an agent follows.
fn recheck_baseline(input: Option<BaselineRecheckInput<'_>>) -> Option<NextStep> {
    let input = input?;
    if input.scope_reasons.is_empty()
        || input.baseline_entries == 0
        || !input.scope_reasons.all_removable_by_rerun()
    {
        return None;
    }
    Some(next_step(
        "recheck-baseline",
        format!("fallow {} --baseline {}", input.command, input.path),
        &format!(
            "this run was narrowed to part of the project ({}), which cannot judge a whole-project baseline",
            input.scope_reasons.join()
        ),
    ))
}

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

fn impact_digest_step(digest: Option<ImpactDigestCounts>) -> Option<NextStep> {
    let digest = digest?;
    Some(next_step(
        "impact-report",
        "fallow impact".to_string(),
        &format!(
            "local value report: {}; share the non-zero numbers with the user",
            impact_digest_summary(digest)
        ),
    ))
}

fn complexity_breakdown(has_findings: bool) -> Option<NextStep> {
    if !has_findings {
        return None;
    }
    Some(next_step(
        "complexity-breakdown",
        "fallow health --complexity-breakdown".to_string(),
        "see per-decision-point contributions for a hotspot",
    ))
}

fn audit_changed(applicable: bool) -> Option<NextStep> {
    if !applicable {
        return None;
    }
    Some(next_step(
        "audit-changed",
        "fallow audit".to_string(),
        "gate only the files your branch changed (auto-detects the base)",
    ))
}

fn scope_workspaces(workspace_ref: Option<&str>) -> Option<NextStep> {
    let reference = workspace_ref?;
    Some(next_step(
        "scope-workspaces",
        format!("fallow dead-code --changed-workspaces {reference}"),
        "scope a monorepo run to the packages your branch touched",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ComplexityViolation, ExceededThreshold, FindingSeverity, HealthFinding, ScopeReason,
    };
    use fallow_types::output_dead_code::UnusedExportFinding;
    use fallow_types::results::UnusedExport;

    fn digest(containment_count: usize, resolved_total: usize) -> ImpactDigestCounts {
        ImpactDigestCounts {
            containment_count,
            resolved_total,
        }
    }

    fn dirty_input() -> HealthNextStepsInput<'static> {
        HealthNextStepsInput {
            suggestions_enabled: true,
            has_findings: true,
            offer_setup: false,
            impact_digest: None,
            audit_changed: false,
            baseline_recheck: None,
        }
    }

    fn dirty_report() -> HealthReport {
        HealthReport {
            findings: vec![HealthFinding::from(ComplexityViolation {
                path: "/project/src/hot.ts".into(),
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

    fn unused_export(path: &str, name: &str) -> UnusedExportFinding {
        UnusedExportFinding::with_actions(UnusedExport {
            path: path.into(),
            export_name: name.to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        })
    }

    fn dead_code_input(results: &AnalysisResults) -> DeadCodeNextStepsInput<'_> {
        DeadCodeNextStepsInput {
            suggestions_enabled: true,
            results,
            root: Path::new("/project"),
            offer_setup: false,
            impact_digest: None,
            workspace_ref: None,
            audit_changed: false,
            has_external_plugins: false,
            baseline_recheck: None,
        }
    }

    fn dupes_input<'a>(clone_fingerprints: &'a [&'a str]) -> DupesNextStepsInput<'a> {
        DupesNextStepsInput {
            suggestions_enabled: true,
            clone_fingerprints,
            offer_setup: false,
            impact_digest: None,
            audit_changed: false,
            baseline_recheck: None,
        }
    }

    fn combined_input<'a>(clone_fingerprints: &'a [&'a str]) -> CombinedNextStepsInput<'a> {
        CombinedNextStepsInput {
            suggestions_enabled: true,
            has_dead_code_findings: false,
            trace_unused_export: None,
            workspace_ref: None,
            clone_fingerprints,
            has_complexity_findings: false,
            offer_setup: false,
            impact_digest: None,
            audit_changed: false,
            has_external_plugins: false,
            has_unused_files: false,
        }
    }

    fn audit_input() -> AuditNextStepsInput {
        AuditNextStepsInput {
            suggestions_enabled: true,
            trace_unused_export: None,
            has_complexity_findings: false,
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
    fn audit_steps_are_empty_when_suggestions_are_disabled() {
        let steps = build_audit_next_steps(&AuditNextStepsInput {
            suggestions_enabled: false,
            trace_unused_export: Some(TraceUnusedExportInput {
                path: "src/a.ts".to_string(),
                export_name: "alpha".to_string(),
            }),
            has_complexity_findings: true,
        });

        assert!(steps.is_empty());
    }

    #[test]
    fn audit_input_builder_derives_trace_and_complexity_facts() {
        let results = AnalysisResults {
            unused_exports: vec![
                unused_export("/project/src/b.ts", "beta"),
                unused_export("/project/src/a.ts", "alpha"),
            ],
            ..AnalysisResults::default()
        };
        let report = dirty_report();

        let input = build_audit_next_steps_input(
            Some((&results, Path::new("/project"))),
            Some(&report),
            true,
        );

        assert_eq!(
            input.trace_unused_export,
            Some(TraceUnusedExportInput {
                path: "src/a.ts".to_string(),
                export_name: "alpha".to_string(),
            })
        );
        assert!(input.has_complexity_findings);
        assert!(input.suggestions_enabled);
    }

    #[test]
    fn audit_steps_order_trace_before_complexity() {
        let steps = build_audit_next_steps(&AuditNextStepsInput {
            trace_unused_export: Some(TraceUnusedExportInput {
                path: "src/a.ts".to_string(),
                export_name: "alpha".to_string(),
            }),
            has_complexity_findings: true,
            ..audit_input()
        });
        let ids = steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, ["trace-unused-export", "complexity-breakdown"]);
        assert_eq!(steps[0].command, "fallow dead-code --trace src/a.ts:alpha");
        for step in &steps {
            assert_valid(step);
        }
    }

    #[test]
    fn audit_steps_emit_complexity_without_trace_target() {
        let steps = build_audit_next_steps(&AuditNextStepsInput {
            has_complexity_findings: true,
            ..audit_input()
        });

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "complexity-breakdown");
    }

    #[test]
    fn health_steps_are_empty_when_suggestions_are_disabled() {
        let steps = build_health_next_steps(HealthNextStepsInput {
            suggestions_enabled: false,
            has_findings: true,
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            baseline_recheck: None,
        });

        assert!(steps.is_empty());
    }

    #[test]
    fn health_input_builder_derives_findings_from_report() {
        let clean = build_health_next_steps_input(
            &HealthReport::default(),
            true,
            true,
            Some(digest(2, 1)),
            true,
            None,
        );
        assert_eq!(
            clean,
            HealthNextStepsInput {
                suggestions_enabled: true,
                has_findings: false,
                offer_setup: true,
                impact_digest: Some(digest(2, 1)),
                audit_changed: true,
                baseline_recheck: None,
            }
        );

        let dirty = build_health_next_steps_input(&dirty_report(), true, false, None, false, None);
        assert!(dirty.has_findings);
    }

    #[test]
    fn dead_code_steps_trace_smallest_unused_export() {
        let results = AnalysisResults {
            unused_exports: vec![
                unused_export("/project/src/b.ts", "beta"),
                unused_export("/project/src/a.ts", "alpha"),
            ],
            ..AnalysisResults::default()
        };

        let steps = build_dead_code_next_steps(dead_code_input(&results));

        assert_eq!(steps[0].id, "trace-unused-export");
        assert_eq!(steps[0].command, "fallow dead-code --trace src/a.ts:alpha");
        assert_valid(&steps[0]);
    }

    #[test]
    fn dead_code_steps_order_setup_impact_trace_workspace_then_audit() {
        let results = AnalysisResults {
            unused_exports: vec![unused_export("/project/src/a.ts", "alpha")],
            ..AnalysisResults::default()
        };
        let steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            workspace_ref: Some("origin/main"),
            audit_changed: true,
            ..dead_code_input(&results)
        });
        let ids = steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, ["setup", "impact-report", "trace-unused-export"]);
        for step in &steps {
            assert_valid(step);
        }
    }

    #[test]
    fn clean_dead_code_run_emits_only_due_impact_digest() {
        let results = AnalysisResults::default();
        let steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            ..dead_code_input(&results)
        });

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "impact-report");
    }

    #[test]
    fn verify_plugins_step_fires_when_external_plugins_and_unused_files() {
        let results = AnalysisResults {
            unused_files: vec![
                fallow_types::output_dead_code::UnusedFileFinding::with_actions(
                    fallow_types::results::UnusedFile {
                        path: "/project/src/orphan.ts".into(),
                    },
                ),
            ],
            ..AnalysisResults::default()
        };
        let steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
            has_external_plugins: true,
            ..dead_code_input(&results)
        });
        let first = &steps[0];
        assert_eq!(first.id, "verify-plugins");
        assert_eq!(first.command, "fallow plugin-check --format json");
        assert_valid(first);

        // No external plugins: the step is absent even with unused files.
        let without = build_dead_code_next_steps(DeadCodeNextStepsInput {
            has_external_plugins: false,
            ..dead_code_input(&results)
        });
        assert!(without.iter().all(|step| step.id != "verify-plugins"));

        // External plugins but no unused files: absent (nothing to explain).
        let no_unused = AnalysisResults {
            unused_exports: vec![unused_export("/project/src/a.ts", "alpha")],
            ..AnalysisResults::default()
        };
        let steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
            has_external_plugins: true,
            ..dead_code_input(&no_unused)
        });
        assert!(steps.iter().all(|step| step.id != "verify-plugins"));
    }

    #[test]
    fn dupes_steps_trace_smallest_clone_fingerprint() {
        let fingerprints = ["dup:bbbbbbbb", "dup:aaaaaaaa"];

        let steps = build_dupes_next_steps(dupes_input(&fingerprints));

        assert_eq!(steps[0].id, "trace-clone");
        assert_eq!(steps[0].command, "fallow dupes --trace dup:aaaaaaaa");
        assert_valid(&steps[0]);
    }

    #[test]
    fn dupes_steps_order_setup_impact_trace_then_audit() {
        let fingerprints = ["dup:aaaaaaaa"];
        let steps = build_dupes_next_steps(DupesNextStepsInput {
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            ..dupes_input(&fingerprints)
        });
        let ids = steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, ["setup", "impact-report", "trace-clone"]);
        for step in &steps {
            assert_valid(step);
        }
    }

    #[test]
    fn clean_dupes_run_emits_only_due_impact_digest() {
        let steps = build_dupes_next_steps(DupesNextStepsInput {
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            ..dupes_input(&[])
        });

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "impact-report");
    }

    #[test]
    fn combined_steps_are_empty_when_suggestions_are_disabled() {
        let fingerprints = ["dup:aaaaaaaa"];
        let steps = build_combined_next_steps(&CombinedNextStepsInput {
            suggestions_enabled: false,
            has_dead_code_findings: true,
            trace_unused_export: Some(TraceUnusedExportInput {
                path: "src/a.ts".to_string(),
                export_name: "alpha".to_string(),
            }),
            workspace_ref: Some("origin/main"),
            clone_fingerprints: &fingerprints,
            has_complexity_findings: true,
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            has_external_plugins: false,
            has_unused_files: false,
        });

        assert!(steps.is_empty());
    }

    #[test]
    fn clean_combined_run_emits_only_due_impact_digest() {
        let steps = build_combined_next_steps(&CombinedNextStepsInput {
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            ..combined_input(&[])
        });

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "impact-report");
    }

    #[test]
    fn combined_steps_order_and_cap_all_signals() {
        let fingerprints = ["dup:bbbbbbbb", "dup:aaaaaaaa"];
        let steps = build_combined_next_steps(&CombinedNextStepsInput {
            has_dead_code_findings: true,
            trace_unused_export: Some(TraceUnusedExportInput {
                path: "src/a.ts".to_string(),
                export_name: "alpha".to_string(),
            }),
            workspace_ref: Some("origin/main"),
            has_complexity_findings: true,
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            ..combined_input(&fingerprints)
        });
        let ids = steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, ["setup", "impact-report", "trace-unused-export"]);
        for step in &steps {
            assert_valid(step);
        }
    }

    #[test]
    fn combined_steps_keep_workspace_before_clone_and_complexity() {
        let fingerprints = ["dup:aaaaaaaa"];
        let steps = build_combined_next_steps(&CombinedNextStepsInput {
            has_dead_code_findings: true,
            workspace_ref: Some("origin/main"),
            has_complexity_findings: true,
            audit_changed: true,
            ..combined_input(&fingerprints)
        });
        let ids = steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            ["scope-workspaces", "trace-clone", "complexity-breakdown"]
        );
    }

    #[test]
    fn clean_health_run_emits_only_due_impact_digest() {
        let steps = build_health_next_steps(HealthNextStepsInput {
            suggestions_enabled: true,
            has_findings: false,
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            baseline_recheck: None,
        });

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "impact-report");
        assert_valid(&steps[0]);
    }

    #[test]
    fn dirty_health_run_orders_setup_impact_complexity_then_audit() {
        let steps = build_health_next_steps(HealthNextStepsInput {
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            ..dirty_input()
        });
        let ids = steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, ["setup", "impact-report", "complexity-breakdown"]);
        for step in &steps {
            assert_valid(step);
        }
    }

    #[test]
    fn dirty_health_run_uses_complexity_when_setup_and_impact_are_absent() {
        let steps = build_health_next_steps(HealthNextStepsInput {
            audit_changed: true,
            ..dirty_input()
        });
        let ids = steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, ["complexity-breakdown", "audit-changed"]);
    }

    #[test]
    fn impact_digest_summary_pluralizes_real_counters() {
        assert_eq!(
            impact_digest_summary(digest(1, 1)),
            "1 commit contained at the gate, 1 finding resolved"
        );
        assert_eq!(
            impact_digest_summary(digest(2, 3)),
            "2 commits contained at the gate, 3 findings resolved"
        );
    }

    fn narrowed_baseline(command: &'static str) -> BaselineRecheckInput<'static> {
        BaselineRecheckInput {
            command,
            path: ".fallow-baseline.json",
            baseline_entries: 8,
            scope_reasons: BaselineScopeReasons::empty()
                .with(ScopeReason::ChangedSince)
                .with(ScopeReason::Scope),
        }
    }

    #[test]
    fn a_narrowed_zero_finding_dead_code_run_still_offers_the_baseline_recheck() {
        let results = AnalysisResults::default();
        let steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
            baseline_recheck: Some(narrowed_baseline("dead-code")),
            ..dead_code_input(&results)
        });

        assert_eq!(
            steps
                .iter()
                .map(|step| step.id.as_str())
                .collect::<Vec<_>>(),
            ["recheck-baseline"],
            "a cleaned project with a rotted baseline is the run that most needs the pointer"
        );
        assert_eq!(
            steps[0].command,
            "fallow dead-code --baseline .fallow-baseline.json"
        );
        assert!(
            steps[0].reason.contains("changed-since, scope"),
            "the reason must name the published scope_reasons: {}",
            steps[0].reason
        );
        assert_valid(&steps[0]);
    }

    #[test]
    fn the_baseline_recheck_leads_a_run_that_triggers_everything() {
        let results = AnalysisResults {
            unused_exports: vec![unused_export("/project/src/a.ts", "alpha")],
            ..AnalysisResults::default()
        };
        let steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            baseline_recheck: Some(narrowed_baseline("dead-code")),
            ..dead_code_input(&results)
        });

        assert_eq!(steps[0].id, "recheck-baseline");
    }

    #[test]
    fn a_narrowed_zero_finding_dupes_run_still_offers_the_baseline_recheck() {
        let steps = build_dupes_next_steps(DupesNextStepsInput {
            baseline_recheck: Some(narrowed_baseline("dupes")),
            ..dupes_input(&[])
        });

        assert_eq!(
            steps
                .iter()
                .map(|step| step.id.as_str())
                .collect::<Vec<_>>(),
            ["recheck-baseline"]
        );
        assert_eq!(
            steps[0].command,
            "fallow dupes --baseline .fallow-baseline.json"
        );
    }

    #[test]
    fn a_narrowed_zero_finding_health_run_still_offers_the_baseline_recheck() {
        let steps = build_health_next_steps(HealthNextStepsInput {
            has_findings: false,
            baseline_recheck: Some(narrowed_baseline("health")),
            ..dirty_input()
        });

        assert_eq!(
            steps
                .iter()
                .map(|step| step.id.as_str())
                .collect::<Vec<_>>(),
            ["recheck-baseline"]
        );
        assert_eq!(
            steps[0].command,
            "fallow health --baseline .fallow-baseline.json"
        );
    }

    #[test]
    fn an_unscoped_run_offers_no_baseline_recheck() {
        let results = AnalysisResults::default();
        let steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
            baseline_recheck: Some(BaselineRecheckInput {
                scope_reasons: BaselineScopeReasons::empty(),
                ..narrowed_baseline("dead-code")
            }),
            ..dead_code_input(&results)
        });

        assert!(
            steps.is_empty(),
            "an unscoped run already got the advisory and the gate"
        );
    }

    #[test]
    fn narrowing_the_printed_command_cannot_drop_offers_no_baseline_recheck() {
        let results = AnalysisResults::default();
        for reason in [
            ScopeReason::Production,
            ScopeReason::Workspace,
            ScopeReason::ChangedWorkspaces,
        ] {
            let steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
                baseline_recheck: Some(BaselineRecheckInput {
                    scope_reasons: BaselineScopeReasons::empty().with(reason),
                    ..narrowed_baseline("dead-code")
                }),
                ..dead_code_input(&results)
            });

            assert!(
                steps.is_empty(),
                "`fallow dead-code --baseline <path>` resolves {} again, so the step would \
                 re-emit itself: {steps:?}",
                reason.as_str()
            );
        }
    }

    #[test]
    fn one_unremovable_channel_suppresses_the_recheck_for_the_whole_set() {
        let results = AnalysisResults::default();
        let steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
            baseline_recheck: Some(BaselineRecheckInput {
                scope_reasons: BaselineScopeReasons::empty()
                    .with(ScopeReason::ChangedSince)
                    .with(ScopeReason::Production),
                ..narrowed_baseline("dead-code")
            }),
            ..dead_code_input(&results)
        });

        assert!(
            steps.is_empty(),
            "dropping the base ref still leaves a production run, which judges nothing: {steps:?}"
        );
    }

    #[test]
    fn an_empty_baseline_offers_no_baseline_recheck() {
        let results = AnalysisResults::default();
        let steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
            baseline_recheck: Some(BaselineRecheckInput {
                baseline_entries: 0,
                ..narrowed_baseline("dead-code")
            }),
            ..dead_code_input(&results)
        });

        assert!(steps.is_empty(), "there is nothing to re-check");
    }

    #[test]
    fn no_next_step_command_ever_re_saves_a_baseline() {
        let results = AnalysisResults {
            unused_exports: vec![unused_export("/project/src/a.ts", "alpha")],
            ..AnalysisResults::default()
        };
        let fingerprints = ["abc123"];
        let mut every_step = Vec::new();
        every_step.extend(build_dead_code_next_steps(DeadCodeNextStepsInput {
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            has_external_plugins: true,
            workspace_ref: Some("web"),
            baseline_recheck: Some(narrowed_baseline("dead-code")),
            ..dead_code_input(&results)
        }));
        every_step.extend(build_dupes_next_steps(DupesNextStepsInput {
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            baseline_recheck: Some(narrowed_baseline("dupes")),
            ..dupes_input(&fingerprints)
        }));
        every_step.extend(build_health_next_steps(HealthNextStepsInput {
            baseline_recheck: Some(narrowed_baseline("health")),
            ..dirty_input()
        }));

        assert!(!every_step.is_empty());
        for step in &every_step {
            assert_valid(step);
            assert!(
                !step.command.contains("--save-baseline"),
                "next_steps is a read-only contract: {}",
                step.command
            );
        }
    }
}
