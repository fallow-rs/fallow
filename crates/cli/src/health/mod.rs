//! `fallow health` complexity / health command.
//!
//! The command-neutral analysis pipeline (scoring, hotspots, targets, grouping,
//! coverage gaps, vital signs, report assembly) lives in
//! `fallow_engine` health root API. This module owns the CLI orchestration that the
//! engine intentionally does not: command option validation, workspace /
//! changed-file / shared-diff scope resolution, CODEOWNERS-backed
//! grouping-resolver construction, the runtime coverage sidecar seam,
//! telemetry recording, exit-code gating, and human / machine rendering.

pub mod coverage;

/// Ownership analysis helpers, re-exported from the engine.
pub use fallow_engine::health_ownership as ownership;
/// Health scoring helpers, re-exported from the engine for CLI consumers that
/// still address them through `crate::health::scoring`.
pub use fallow_engine::health_scoring as scoring;

use std::process::ExitCode;
use std::time::Instant;

use colored::Colorize;
use fallow_config::OutputFormat;
use fallow_engine::{
    HealthExecutionOptions, HealthGateOptions, HealthGroupResolver, HealthPipelineInputs,
    HealthScopeInputs, HealthSeams, HealthSharedParseData, HealthSort, RuntimeCoverageSeamInput,
    execute_health_inner, validate_health_churn_file,
};

use crate::check::{get_changed_files, resolve_workspace_scope};
use crate::error::emit_error;
use crate::report;
use crate::report::OwnershipResolver;

/// Sort criteria for complexity output.
#[derive(Clone, clap::ValueEnum)]
pub enum SortBy {
    Severity,
    Cyclomatic,
    Cognitive,
    Lines,
}

impl From<SortBy> for HealthSort {
    fn from(sort: SortBy) -> Self {
        match sort {
            SortBy::Severity => Self::Severity,
            SortBy::Cyclomatic => Self::Cyclomatic,
            SortBy::Cognitive => Self::Cognitive,
            SortBy::Lines => Self::Lines,
        }
    }
}

pub type HealthOptions<'a> = HealthExecutionOptions<'a>;

impl HealthGroupResolver for OwnershipResolver {
    fn mode_label(&self) -> &'static str {
        OwnershipResolver::mode_label(self)
    }

    fn resolve_with_rule(&self, rel_path: &std::path::Path) -> (String, Option<String>) {
        OwnershipResolver::resolve_with_rule(self, rel_path)
    }

    fn section_owners_of(&self, rel_path: &std::path::Path) -> Option<&[String]> {
        OwnershipResolver::section_owners_of(self, rel_path)
    }
}

/// Resolve the diff index for a health run: an explicit `--diff-file` index
/// wins, otherwise the process-shared diff cache when the caller opted in.
fn health_diff_index<'a>(opts: &HealthOptions<'a>) -> Option<&'a fallow_output::DiffIndex> {
    match opts.diff_index {
        Some(index) => Some(index),
        None if opts.use_shared_diff_index => crate::report::ci::diff_filter::shared_diff_index(),
        None => None,
    }
}

/// Build the CODEOWNERS / package-backed grouping resolver for `--group-by`.
fn build_health_group_resolver(
    opts: &HealthOptions<'_>,
    config: &fallow_config::ResolvedConfig,
) -> Result<Option<OwnershipResolver>, ExitCode> {
    crate::runtime_support::build_ownership_resolver_for_mode(
        opts.group_by,
        opts.root,
        config.codeowners.as_deref(),
        opts.output,
    )
}

/// Record health telemetry from the finished report. Mirrors the per-analysis
/// telemetry the other commands record; lives in the CLI because the telemetry
/// sinks are process-global CLI state.
fn record_health_telemetry(report: &fallow_output::HealthReport, coverage_gaps_has_findings: bool) {
    if coverage_gaps_has_findings && report.findings.is_empty() {
        crate::telemetry::note_findings_present(true);
    } else {
        crate::telemetry::note_result_count(report.findings.len());
    }
    crate::telemetry::note_analysis_scale(
        Some(report.summary.files_analyzed),
        Some(report.summary.functions_analyzed),
    );
}

/// Build the engine seam callbacks: the runtime coverage sidecar adapter and
/// the graph-structure telemetry hook.
fn health_seams<'a>() -> HealthSeams<'a> {
    HealthSeams {
        runtime_coverage_analyzer: &runtime_coverage_seam,
        note_graph_structure: &|module_count, edge_count| {
            crate::telemetry::note_graph_structure_counts(module_count, edge_count);
        },
    }
}

/// Adapt the engine's runtime coverage seam input to the CLI coverage module,
/// which owns the closed-source sidecar (license verification, subprocess
/// spawning, signal handling).
#[expect(
    clippy::needless_pass_by_value,
    reason = "by-value input matches the engine RuntimeCoverageAnalyzer seam signature"
)]
fn runtime_coverage_seam(
    options: &fallow_engine::RuntimeCoverageOptions,
    input: RuntimeCoverageSeamInput<'_>,
) -> Result<fallow_output::RuntimeCoverageReport, ExitCode> {
    coverage::analyze(
        options,
        &coverage::RuntimeCoverageAnalysisInput {
            root: input.root,
            modules: input.modules,
            analysis_output: input.analysis_output,
            istanbul_coverage: input.istanbul_coverage,
            file_paths: input.file_paths,
            ignore_set: input.ignore_set,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
            top: input.top,
            codeowners_path: input.codeowners_path,
            quiet: input.quiet,
            output: input.output,
        },
    )
}

/// Resolve the command-neutral scope inputs the engine needs: changed files,
/// the diff index, workspace roots, and the grouping resolver.
fn build_health_scope_inputs<'a>(
    opts: &HealthOptions<'a>,
    config: &fallow_config::ResolvedConfig,
) -> Result<HealthScopeInputs<'a, OwnershipResolver>, ExitCode> {
    let changed_files = opts
        .changed_since
        .and_then(|git_ref| get_changed_files(opts.root, git_ref));
    let diff_index = health_diff_index(opts);
    let ws_roots = resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;
    let group_resolver = build_health_group_resolver(opts, config)?;
    Ok(HealthScopeInputs {
        changed_files,
        diff_index,
        ws_roots,
        group_resolver,
    })
}

/// Load config for a health run, validating coverage-root and churn-file inputs
/// up front (loud exit 2 on a malformed input).
fn load_health_config(
    opts: &HealthOptions<'_>,
) -> Result<(fallow_config::ResolvedConfig, f64), ExitCode> {
    fallow_engine::validate_coverage_root_absolute(opts.coverage_inputs.coverage_root)
        .map_err(|e| emit_error(&e, 2, opts.output))?;
    validate_health_churn_file(opts)?;
    let t = Instant::now();
    let config = crate::load_config_for_analysis(
        opts.root,
        opts.config_path,
        crate::ConfigLoadOptions {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production_override: opts
                .production_override
                .or_else(|| opts.production.then_some(true)),
            quiet: opts.quiet,
        },
        fallow_config::ProductionAnalysis::Health,
    )?;
    let config_ms = t.elapsed().as_secs_f64() * 1000.0;
    Ok((config, config_ms))
}

/// Run health analysis using pre-parsed modules from the dead-code pipeline.
///
/// Skips file discovery and parsing (saves ~1.9s on 21K-file projects).
pub fn execute_health_with_shared_parse(
    opts: &HealthOptions<'_>,
    shared: HealthSharedParseData,
) -> Result<HealthResult, ExitCode> {
    let (config, config_ms) = load_health_config(opts)?;
    let scope_inputs = build_health_scope_inputs(opts, &config)?;
    let workspace_diagnostics = fallow_config::workspace_diagnostics_for(&config.root);
    let seams = health_seams();
    let result = execute_health_inner(
        opts,
        HealthPipelineInputs {
            config,
            files: shared.files,
            modules: shared.modules,
            config_ms,
            discover_ms: 0.0,
            parse_ms: 0.0,
            parse_cpu_ms: 0.0,
            shared_parse: true,
            pre_computed_analysis: shared.analysis_output,
            workspace_diagnostics,
        },
        scope_inputs,
        &seams,
    )?;
    record_health_telemetry(&result.report, result.coverage_gaps_has_findings);
    Ok(result)
}

pub fn execute_health(opts: &HealthOptions<'_>) -> Result<HealthResult, ExitCode> {
    let (config, config_ms) = load_health_config(opts)?;

    let t = Instant::now();
    let session = fallow_engine::AnalysisSession::from_resolved_config(config);
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;
    let session = session.into_parsed_parts(true);
    let config = session.config;
    let files = session.files;
    let modules = session.modules;
    let workspace_diagnostics = session.workspace_diagnostics;
    let parse_ms = session.parse_ms;
    let parse_cpu_ms = session.parse_cpu_ms;

    let scope_inputs = build_health_scope_inputs(opts, &config)?;
    let seams = health_seams();
    let result = execute_health_inner(
        opts,
        HealthPipelineInputs {
            config,
            files,
            modules,
            config_ms,
            discover_ms,
            parse_ms,
            parse_cpu_ms,
            shared_parse: false,
            pre_computed_analysis: None,
            workspace_diagnostics,
        },
        scope_inputs,
        &seams,
    )?;
    record_health_telemetry(&result.report, result.coverage_gaps_has_findings);
    Ok(result)
}

pub fn run_health(opts: &HealthOptions<'_>) -> ExitCode {
    let result = match execute_health(opts) {
        Ok(r) => r,
        Err(code) => return code,
    };
    if let Some(ref timings) = result.timings {
        report::print_health_performance(timings, opts.output);
    }
    print_health_result(
        &result,
        HealthPrintOptions {
            quiet: opts.quiet,
            explain: opts.explain,
            gates: opts.gates,
            summary: opts.summary,
            summary_heading: true,
            show_explain_tip: true,
            skip_score_and_trend: false,
            css_requested: opts.css,
        },
    )
}

/// Result of executing health analysis without printing.
pub type HealthResult = fallow_engine::HealthAnalysisResult<crate::report::OwnershipResolver>;

/// Print health results and return appropriate exit code.
///
/// When called from combined mode (`fallow --score` / `fallow --trend`),
/// `skip_score_and_trend` MUST be `true`: the orientation header already
/// renders both blocks and rendering them a second time here would duplicate
/// the lines. Standalone `fallow health` invocations pass `false`.
///
/// Exit-code gating (when `report_only` is `false`): the score gate
/// (`--min-score`), the findings gate (`--min-severity`, or any finding when
/// no gate flag is set), the runtime-coverage gate, and the coverage-gap gate
/// are OR-combined. `report_only` short-circuits all of them to
/// `ExitCode::SUCCESS` after rendering. Combined and audit callers pass
/// `report_only: false` (they own their own gate semantics).
///
/// Callers that pass `min_score: Some(_)` must ensure
/// `result.report.health_score` is `Some` (the CLI guarantees this because
/// `--min-score` implies `--score`). If the score is missing the score gate
/// cannot evaluate, so a direct API caller that requests a score gate without
/// computing the score would get a permissive `ExitCode::SUCCESS`.
#[derive(Clone, Copy)]
pub struct HealthPrintOptions {
    pub quiet: bool,
    pub explain: bool,
    pub gates: HealthGateOptions,
    pub summary: bool,
    pub summary_heading: bool,
    pub show_explain_tip: bool,
    pub skip_score_and_trend: bool,
    /// Whether `--css` was requested. Forwarded to the human renderer so an empty
    /// CSS result (no import-reachable stylesheet) is explained rather than
    /// silently omitted. Defaults `false` for callers that do not request CSS.
    pub css_requested: bool,
}

pub fn print_health_result(result: &HealthResult, options: HealthPrintOptions) -> ExitCode {
    let ctx = health_report_context(result, options);
    let report_code = report::print_health_report(
        &result.report,
        result.grouping.as_ref(),
        result.group_resolver.as_ref(),
        &ctx,
        result.config.output,
    );
    if report_code != ExitCode::SUCCESS {
        return report_code;
    }

    if options.gates.report_only {
        return ExitCode::SUCCESS;
    }

    if health_exit_gate_failed(result, options) {
        return ExitCode::from(1);
    }
    if result.should_fail_on_coverage_gaps && result.coverage_gaps_has_findings {
        return ExitCode::from(1);
    }
    maybe_print_score_gate_note(result, options);

    ExitCode::SUCCESS
}

fn health_report_context(
    result: &HealthResult,
    options: HealthPrintOptions,
) -> report::ReportContext<'_> {
    report::ReportContext {
        root: &result.config.root,
        rules: &result.config.rules,
        elapsed: result.elapsed,
        quiet: options.quiet,
        explain: options.explain,
        group_by: None,
        top: None,
        summary: options.summary,
        summary_heading: options.summary_heading,
        show_explain_tip: options.show_explain_tip,
        baseline_matched: None,
        config_fixable: false,
        skip_score_and_trend: options.skip_score_and_trend,
        css_requested: options.css_requested,
    }
}

fn health_exit_gate_failed(result: &HealthResult, options: HealthPrintOptions) -> bool {
    score_gate_failed(result, options)
        || findings_gate_failed(result, options)
        || has_failing_runtime_coverage(result)
}

fn score_gate_failed(result: &HealthResult, options: HealthPrintOptions) -> bool {
    let Some(threshold) = options.gates.min_score else {
        return false;
    };
    let Some(ref hs) = result.report.health_score else {
        return false;
    };
    if hs.score >= threshold {
        return false;
    }

    if !options.quiet {
        eprintln!(
            "Health score {:.1} ({}) is below minimum threshold {:.0}",
            hs.score, hs.grade, threshold
        );
    }
    true
}

fn findings_gate_failed(result: &HealthResult, options: HealthPrintOptions) -> bool {
    if let Some(min_sev) = options.gates.min_severity {
        result.report.findings.iter().any(|f| f.severity >= min_sev)
    } else if options.gates.min_score.is_none() {
        !result.report.findings.is_empty()
    } else {
        false
    }
}

fn has_failing_runtime_coverage(result: &HealthResult) -> bool {
    result
        .report
        .runtime_coverage
        .as_ref()
        .is_some_and(|report| report.findings.iter().any(is_failing_runtime_coverage))
}

fn is_failing_runtime_coverage(finding: &fallow_output::RuntimeCoverageFinding) -> bool {
    matches!(
        finding.verdict,
        fallow_output::RuntimeCoverageVerdict::SafeToDelete
            | fallow_output::RuntimeCoverageVerdict::ReviewRequired
            | fallow_output::RuntimeCoverageVerdict::LowTraffic
    )
}

fn maybe_print_score_gate_note(result: &HealthResult, options: HealthPrintOptions) {
    if options.gates.min_score.is_none()
        || options.gates.min_severity.is_some()
        || options.quiet
        || result.report.findings.is_empty()
        || !matches!(result.config.output, OutputFormat::Human)
    {
        return;
    }

    {
        eprintln!(
            "{}",
            "Findings above are informational: --min-score gates on the score, not on findings."
                .dimmed()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::{FallowConfig, OutputFormat};
    use fallow_output::{ComplexityViolation, ExceededThreshold, FindingSeverity};
    use std::path::PathBuf;
    use std::time::Duration;

    fn make_finding(name: &str, exceeded: ExceededThreshold) -> ComplexityViolation {
        ComplexityViolation {
            path: PathBuf::from("/project/src/a.ts"),
            name: name.to_string(),
            line: 1,
            col: 0,
            cyclomatic: match exceeded {
                ExceededThreshold::Cyclomatic
                | ExceededThreshold::Both
                | ExceededThreshold::CyclomaticCrap
                | ExceededThreshold::All => 25,
                _ => 8,
            },
            cognitive: match exceeded {
                ExceededThreshold::Cognitive
                | ExceededThreshold::Both
                | ExceededThreshold::CognitiveCrap
                | ExceededThreshold::All => 20,
                _ => 5,
            },
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded,
            severity: FindingSeverity::Moderate,
            crap: exceeded.includes_crap().then_some(30.0),
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
        }
    }

    fn test_resolved_config() -> fallow_config::ResolvedConfig {
        FallowConfig::default().resolve(
            PathBuf::from("/project"),
            OutputFormat::Json,
            1,
            true,
            true,
            None,
        )
    }

    fn fx_summary(
        tracked: usize,
        hit: usize,
        unhit: usize,
        untracked: usize,
    ) -> fallow_output::RuntimeCoverageSummary {
        #[expect(
            clippy::cast_precision_loss,
            reason = "test fixture totals are tiny, f64 precision is fine"
        )]
        let coverage_percent = if tracked == 0 {
            0.0
        } else {
            (hit as f64 / tracked as f64) * 100.0
        };
        fallow_output::RuntimeCoverageSummary {
            data_source: fallow_output::RuntimeCoverageDataSource::Local,
            last_received_at: None,
            functions_tracked: tracked,
            functions_hit: hit,
            functions_unhit: unhit,
            functions_untracked: untracked,
            coverage_percent,
            trace_count: 512,
            period_days: 7,
            deployments_seen: 2,
            capture_quality: None,
        }
    }

    fn fx_evidence(
        static_status: &str,
        test_coverage: &str,
        v8_tracking: &str,
    ) -> fallow_output::RuntimeCoverageEvidence {
        fallow_output::RuntimeCoverageEvidence {
            static_status: static_status.to_owned(),
            test_coverage: test_coverage.to_owned(),
            v8_tracking: v8_tracking.to_owned(),
            untracked_reason: None,
            observation_days: 7,
            deployments_observed: 2,
        }
    }

    fn fx_health_score(score: f64, grade: &'static str) -> fallow_output::HealthScore {
        fallow_output::HealthScore {
            formula_version: 2,
            score,
            grade,
            penalties: fallow_output::HealthScorePenalties {
                dead_files: None,
                dead_exports: None,
                complexity: 0.0,
                p90_complexity: 0.0,
                maintainability: None,
                hotspots: None,
                unused_deps: None,
                circular_deps: None,
                unit_size: None,
                coupling: None,
                duplication: None,
                prop_drilling: None,
            },
        }
    }

    fn fx_gate_result(
        findings: Vec<fallow_output::HealthFinding>,
        score: Option<fallow_output::HealthScore>,
    ) -> HealthResult {
        HealthResult {
            report: fallow_output::HealthReport {
                findings,
                health_score: score,
                ..fallow_output::HealthReport::default()
            },
            grouping: None,
            group_resolver: None,
            config: test_resolved_config(),
            workspace_diagnostics: Vec::new(),
            elapsed: Duration::default(),
            timings: None,
            coverage_gaps_has_findings: false,
            should_fail_on_coverage_gaps: false,
        }
    }

    fn moderate_finding() -> fallow_output::HealthFinding {
        make_finding("moderate", ExceededThreshold::Cyclomatic).into()
    }

    fn critical_finding() -> fallow_output::HealthFinding {
        let mut v = make_finding("critical", ExceededThreshold::All);
        v.severity = FindingSeverity::Critical;
        v.into()
    }

    /// Helper: run the gate with the given flags, quiet, no report-only.
    fn gate_exit(
        result: &HealthResult,
        min_score: Option<f64>,
        min_severity: Option<FindingSeverity>,
        report_only: bool,
    ) -> ExitCode {
        print_health_result(
            result,
            HealthPrintOptions {
                quiet: true,
                explain: false,
                gates: HealthGateOptions {
                    min_score,
                    min_severity,
                    report_only,
                },
                summary: false,
                summary_heading: true,
                show_explain_tip: true,
                skip_score_and_trend: false,
                css_requested: false,
            },
        )
    }

    #[test]
    fn plain_health_with_findings_fails() {
        let result = fx_gate_result(vec![moderate_finding()], Some(fx_health_score(87.5, "A")));
        assert_eq!(gate_exit(&result, None, None, false), ExitCode::from(1));
    }

    #[test]
    fn plain_health_with_no_findings_succeeds() {
        let result = fx_gate_result(vec![], Some(fx_health_score(100.0, "A")));
        assert_eq!(gate_exit(&result, None, None, false), ExitCode::SUCCESS);
    }

    #[test]
    fn min_score_zero_never_fails_even_with_findings() {
        let result = fx_gate_result(vec![moderate_finding()], Some(fx_health_score(50.0, "D")));
        assert_eq!(
            gate_exit(&result, Some(0.0), None, false),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn min_score_passing_demotes_findings_to_informational() {
        let result = fx_gate_result(vec![moderate_finding()], Some(fx_health_score(87.5, "A")));
        assert_eq!(
            gate_exit(&result, Some(80.0), None, false),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn min_score_below_threshold_fails() {
        let result = fx_gate_result(vec![moderate_finding()], Some(fx_health_score(50.0, "D")));
        assert_eq!(
            gate_exit(&result, Some(80.0), None, false),
            ExitCode::from(1)
        );
    }

    #[test]
    fn min_severity_gates_on_severity_independent_of_min_score() {
        let only_moderate =
            fx_gate_result(vec![moderate_finding()], Some(fx_health_score(87.5, "A")));
        assert_eq!(
            gate_exit(&only_moderate, None, Some(FindingSeverity::Critical), false),
            ExitCode::SUCCESS,
        );
        let with_critical = fx_gate_result(
            vec![moderate_finding(), critical_finding()],
            Some(fx_health_score(87.5, "A")),
        );
        assert_eq!(
            gate_exit(&with_critical, None, Some(FindingSeverity::Critical), false),
            ExitCode::from(1),
        );
    }

    #[test]
    fn min_score_and_min_severity_compose_as_or() {
        let pass = fx_gate_result(vec![moderate_finding()], Some(fx_health_score(87.5, "A")));
        assert_eq!(
            gate_exit(&pass, Some(80.0), Some(FindingSeverity::Critical), false),
            ExitCode::SUCCESS,
        );
        let low_score = fx_gate_result(vec![moderate_finding()], Some(fx_health_score(50.0, "D")));
        assert_eq!(
            gate_exit(
                &low_score,
                Some(80.0),
                Some(FindingSeverity::Critical),
                false
            ),
            ExitCode::from(1),
        );
        let critical = fx_gate_result(vec![critical_finding()], Some(fx_health_score(87.5, "A")));
        assert_eq!(
            gate_exit(
                &critical,
                Some(80.0),
                Some(FindingSeverity::Critical),
                false
            ),
            ExitCode::from(1),
        );
    }

    #[test]
    fn report_only_never_fails_on_findings_or_low_score() {
        let result = fx_gate_result(
            vec![moderate_finding(), critical_finding()],
            Some(fx_health_score(10.0, "F")),
        );
        assert_eq!(gate_exit(&result, None, None, true), ExitCode::SUCCESS);
    }

    #[test]
    fn runtime_coverage_gate_independent_of_min_score() {
        let result = fx_low_traffic_runtime_result();
        assert_eq!(
            gate_exit(&result, Some(0.0), None, false),
            ExitCode::from(1)
        );
        assert_eq!(gate_exit(&result, None, None, true), ExitCode::SUCCESS);
    }

    fn fx_low_traffic_runtime_result() -> HealthResult {
        HealthResult {
            report: fallow_output::HealthReport {
                runtime_coverage: Some(fallow_output::RuntimeCoverageReport {
                    schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
                    verdict: fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected,
                    signals: Vec::new(),
                    summary: fx_summary(1, 0, 1, 0),
                    findings: vec![fallow_output::RuntimeCoverageFinding {
                        id: "fallow:prod:lowtraffic".to_owned(),
                        stable_id: None,
                        path: PathBuf::from("/project/src/cold.ts"),
                        function: "coldPath".to_owned(),
                        line: 14,
                        verdict: fallow_output::RuntimeCoverageVerdict::LowTraffic,
                        invocations: Some(1),
                        confidence: fallow_output::RuntimeCoverageConfidence::Low,
                        evidence: fx_evidence("used", "not_covered", "tracked"),
                        actions: vec![],
                        source_hash: None,
                        discriminators: None,
                    }],
                    hot_paths: vec![],
                    blast_radius: vec![],
                    importance: vec![],
                    watermark: None,
                    warnings: vec![],
                    actionable: true,
                    actionability_reason: None,
                    actionability_verdict: None,
                    provenance: fallow_output::RuntimeCoverageProvenance::default(),
                }),
                ..fallow_output::HealthReport::default()
            },
            grouping: None,
            group_resolver: None,
            config: test_resolved_config(),
            workspace_diagnostics: Vec::new(),
            elapsed: Duration::default(),
            timings: None,
            coverage_gaps_has_findings: false,
            should_fail_on_coverage_gaps: false,
        }
    }

    #[test]
    fn print_health_result_fails_on_low_traffic_runtime_coverage() {
        let result = fx_low_traffic_runtime_result();

        assert_eq!(
            print_health_result(
                &result,
                HealthPrintOptions {
                    quiet: true,
                    explain: false,
                    gates: HealthGateOptions::default(),
                    summary: false,
                    summary_heading: true,
                    show_explain_tip: true,
                    skip_score_and_trend: false,
                    css_requested: false,
                },
            ),
            ExitCode::from(1),
        );
    }
}
