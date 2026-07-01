//! Health result assembly helpers.

use std::time::Duration;

use fallow_config::ResolvedConfig;
use fallow_output::{HealthGrouping, HealthReport, HealthTimings};
use fallow_types::discover::DiscoveredFile;
use fallow_types::workspace::WorkspaceDiagnostic;

use crate::results::HealthAnalysisResult;

use super::HealthExecutionOptions;
use super::css_analytics::{HealthScanCtx, compute_css_analytics_report};
use super::pipeline::HealthScope;

pub(super) struct HealthOutputParts {
    pub(super) report: HealthReport,
    pub(super) grouping: Option<HealthGrouping>,
    pub(super) timings: Option<HealthTimings>,
    pub(super) coverage_gaps_has_findings: bool,
}

struct HealthReportSideEffectsInput<'a> {
    opts: &'a HealthExecutionOptions<'a>,
    report: &'a mut HealthReport,
    files: &'a [DiscoveredFile],
    /// The per-file extraction output (always present, graph-independent). Used by
    /// the `--css` path to derive the CSS-in-JS design-token blast-radius from
    /// imports + member accesses without a resolved graph (Phase 3d).
    modules: &'a [fallow_types::extract::ModuleInfo],
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
}

pub(super) struct HealthFinalizeInput<'a, R> {
    pub(super) opts: &'a HealthExecutionOptions<'a>,
    pub(super) config: ResolvedConfig,
    pub(super) files: &'a [DiscoveredFile],
    pub(super) modules: &'a [fallow_types::extract::ModuleInfo],
    pub(super) scope: HealthScope<'a, R>,
    pub(super) output: HealthOutputParts,
    pub(super) elapsed: Duration,
    pub(super) should_fail_on_coverage_gaps: bool,
    pub(super) workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

struct HealthResultInput<R> {
    config: ResolvedConfig,
    report: HealthReport,
    grouping: Option<HealthGrouping>,
    group_resolver: Option<R>,
    elapsed: Duration,
    timings: Option<HealthTimings>,
    coverage_gaps_has_findings: bool,
    should_fail_on_coverage_gaps: bool,
    workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

pub(super) fn finalize_health_result<R>(
    input: HealthFinalizeInput<'_, R>,
) -> HealthAnalysisResult<R> {
    let HealthFinalizeInput {
        opts,
        config,
        files,
        modules,
        scope,
        output,
        elapsed,
        should_fail_on_coverage_gaps,
        workspace_diagnostics,
    } = input;
    let HealthOutputParts {
        mut report,
        grouping,
        timings,
        coverage_gaps_has_findings,
    } = output;

    finalize_health_report_side_effects(&mut HealthReportSideEffectsInput {
        opts,
        report: &mut report,
        files,
        modules,
        config: &config,
        ignore_set: &scope.ignore_set,
        changed_files: scope.changed_files.as_ref(),
        ws_roots: scope.ws_roots.as_deref(),
    });

    build_health_result(HealthResultInput {
        config,
        report,
        grouping,
        group_resolver: scope.group_resolver,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
        workspace_diagnostics,
    })
}

fn finalize_health_report_side_effects(input: &mut HealthReportSideEffectsInput<'_>) {
    if input.opts.css {
        let computation = compute_css_analytics_report(
            input.files,
            input.modules,
            HealthScanCtx {
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: input.changed_files,
                ws_roots: input.ws_roots,
            },
        );
        input.report.styling_health = computation.as_ref().map(|computation| {
            super::styling_score::compute_styling_health_with_inputs(
                &computation.report,
                &computation.scoring_inputs,
            )
        });
        input.report.css_analytics = computation.map(|computation| computation.report);
        // Graduation (chunk 2): map the descriptive css candidates into first-class
        // styling findings, honoring inline suppression at production time. Styling
        // stays in its own domain (HealthReport), not the dead-code AnalysisResults.
        input.report.styling_findings = build_styling_findings(
            input.report.css_analytics.as_ref(),
            input.modules,
            input.files,
            input.config,
        );
    }
}

/// Graduate the descriptive css candidates into first-class [`StylingFinding`]s.
/// Honors inline suppression (`// fallow-ignore-next-line css-token-drift` /
/// `-file`) at production time, matched by the candidate's relative path against
/// the module's parsed suppressions. First family: `css-token-drift` (Tailwind
/// arbitrary values = hardcoded-instead-of-token). Default severity `warn` is
/// applied downstream; the finding is verdict-neutral by default.
fn build_styling_findings(
    css: Option<&fallow_output::CssAnalyticsReport>,
    modules: &[fallow_types::extract::ModuleInfo],
    files: &[DiscoveredFile],
    config: &ResolvedConfig,
) -> Vec<fallow_output::StylingFinding> {
    use fallow_config::Severity;
    use fallow_types::suppress::{IssueKind, Suppression, is_file_suppressed, is_suppressed};

    let Some(css) = css else {
        return Vec::new();
    };

    // Shared: relative-path -> the file's parsed suppressions, so every family
    // honors inline suppression at production time.
    let path_by_id: rustc_hash::FxHashMap<_, _> =
        files.iter().map(|f| (f.id, f.path.as_path())).collect();
    let mut supp_by_rel: rustc_hash::FxHashMap<String, &[Suppression]> =
        rustc_hash::FxHashMap::default();
    for module in modules {
        if module.suppressions.is_empty() {
            continue;
        }
        if let Some(abs) = path_by_id.get(&module.file_id)
            && let Some(rel) = super::runtime_filter::relative_to_root(abs, &config.root)
        {
            supp_by_rel.insert(rel, module.suppressions.as_slice());
        }
    }
    let suppressed = |path: &str, line: u32, kind: IssueKind| -> bool {
        supp_by_rel.get(path).is_some_and(|supps| {
            is_file_suppressed(supps, kind) || is_suppressed(supps, line, kind)
        })
    };

    let mut findings = Vec::new();

    // Family css-token-drift: Tailwind arbitrary values (hardcoded-instead-of-token).
    if config.rules.css_token_drift != Severity::Off {
        for candidate in &css.tailwind_arbitrary_values {
            if suppressed(&candidate.path, candidate.line, IssueKind::CssTokenDrift) {
                continue;
            }
            findings.push(fallow_output::StylingFinding {
                code: "css-token-drift".to_string(),
                sub_kind: "tailwind-arbitrary-value".to_string(),
                path: candidate.path.clone(),
                line: candidate.line,
                value: candidate.value.clone(),
                actions: candidate.actions.clone(),
            });
        }
    }

    // Family css-duplicate-block: copy-pasted declaration blocks (anchored at the
    // first occurrence).
    if config.rules.css_duplicate_block != Severity::Off {
        for block in &css.duplicate_declaration_blocks {
            let Some(first) = block.occurrences.first() else {
                continue;
            };
            if suppressed(&first.path, first.line, IssueKind::CssDuplicateBlock) {
                continue;
            }
            findings.push(fallow_output::StylingFinding {
                code: "css-duplicate-block".to_string(),
                sub_kind: "duplicate-declaration-block".to_string(),
                path: first.path.clone(),
                line: first.line,
                value: format!(
                    "{}-declaration block repeated {} times",
                    block.declaration_count, block.occurrence_count
                ),
                actions: block.actions.clone(),
            });
        }
    }

    findings
}

fn build_health_result<R>(input: HealthResultInput<R>) -> HealthAnalysisResult<R> {
    let HealthResultInput {
        config,
        report,
        grouping,
        group_resolver,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
        workspace_diagnostics,
    } = input;

    HealthAnalysisResult {
        report,
        grouping,
        group_resolver,
        config,
        workspace_diagnostics,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
    }
}
