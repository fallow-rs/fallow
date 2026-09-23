//! `fallow health` complexity / health command.
//!
//! The command-neutral analysis pipeline (scoring, hotspots, targets, grouping,
//! coverage gaps, vital signs, report assembly) lives in
//! `fallow_engine::health` API. This module owns the CLI orchestration that the
//! engine intentionally does not: command option validation, workspace /
//! changed-file / shared-diff scope resolution, CODEOWNERS-backed
//! grouping-resolver construction, the runtime coverage sidecar seam,
//! telemetry recording, exit-code gating, and human / machine rendering.

pub mod coverage;

/// Health scoring helpers, re-exported from the engine for CLI consumers that
/// still address them through `crate::health::scoring`.
pub use fallow_engine::health::scoring;

use std::process::ExitCode;
use std::time::Instant;

use colored::Colorize;
use fallow_config::OutputFormat;
use fallow_engine::health::{
    HealthError, HealthExecutionOptions, HealthGateOptions, HealthGroupResolver,
    HealthPipelineInputs, HealthScopeInputs, HealthSeams, HealthSharedParseData, HealthSort,
    RuntimeCoverageSeamInput, execute_health_inner, validate_health_churn_file,
};

use crate::check::resolve_workspace_scope;
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

/// CLI-only semantic overlay options for `health --type-coupling`.
pub struct TypeAwareHealthOptions<'a> {
    /// CLI override: `Some(true)` for `--type-aware`, `Some(false)` for
    /// `--no-type-aware`, `None` when neither flag was passed.
    pub enabled: Option<bool>,
    pub requested: bool,
    pub unfiltered: bool,
    pub projects: &'a [std::path::PathBuf],
    pub require: Option<fallow_config::TypeAwareRequire>,
}

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
    options: &fallow_engine::health::RuntimeCoverageOptions,
    input: RuntimeCoverageSeamInput<'_>,
) -> Result<fallow_output::RuntimeCoverageReport, u8> {
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
        .and_then(|git_ref| crate::requests::resolve_changed_since(opts.root, git_ref));
    let diff_index = health_diff_index(opts);
    let mut ws_roots = resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;
    if let Some(scope) = opts.scope.as_ref() {
        ws_roots.get_or_insert_with(Vec::new).push(scope.clone());
    }
    let group_resolver = build_health_group_resolver(opts, config)?;
    Ok(HealthScopeInputs {
        changed_files,
        diff_index,
        ws_roots,
        group_resolver,
    })
}

/// Translate an engine [`HealthError`] into a CLI exit code at the command
/// boundary. `Message` is rendered here (the engine no longer prints fatal
/// errors); `Printed` was already emitted by a lower layer (the runtime-coverage
/// seam), so its exit code is honored without a second error document.
fn health_err_to_exit(error: HealthError, output: OutputFormat) -> ExitCode {
    match error {
        HealthError::Message { message, exit_code } => emit_error(&message, exit_code, output),
        HealthError::Printed(code) => ExitCode::from(code),
    }
}

/// Load config for a health run, validating coverage-root and churn-file inputs
/// and the baseline destination up front (loud exit 2 on a malformed input).
pub fn load_health_config(
    opts: &HealthOptions<'_>,
) -> Result<(fallow_config::ResolvedConfig, f64), ExitCode> {
    if let Some(code) = crate::baseline_gate::refuse_save_before_analysis(
        opts.save_baseline,
        fallow_engine::baseline::BaselineKind::Health,
        opts.output,
    ) {
        return Err(code);
    }
    fallow_engine::health::validate_coverage_root_absolute(opts.coverage_inputs.coverage_root)
        .map_err(|e| emit_error(&e, 2, opts.output))?;
    validate_health_churn_file(opts).map_err(|e| health_err_to_exit(e, opts.output))?;
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
            allow_remote_extends: opts.allow_remote_extends,
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
    let workspaces = shared.workspaces;
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
            dead_code_results: shared.dead_code_results,
            styling_artifacts: None,
            pre_computed_duplication: None,
            workspaces,
            workspace_diagnostics,
        },
        scope_inputs,
        &seams,
    )
    .map_err(|e| health_err_to_exit(e, opts.output))?;
    record_health_telemetry(&result.report, result.coverage_gaps_has_findings);
    Ok(result)
}

pub fn execute_health(opts: &HealthOptions<'_>) -> Result<HealthResult, ExitCode> {
    let (config, config_ms) = load_health_config(opts)?;
    execute_health_with_config(opts, config, config_ms)
}

pub fn execute_health_with_config(
    opts: &HealthOptions<'_>,
    config: fallow_config::ResolvedConfig,
    config_ms: f64,
) -> Result<HealthResult, ExitCode> {
    let seams = health_seams();
    let result = execute_health_with_config_and_seams(opts, config, config_ms, &seams)?;
    record_health_telemetry(&result.report, result.coverage_gaps_has_findings);
    Ok(result)
}

fn execute_health_with_config_and_seams(
    opts: &HealthOptions<'_>,
    config: fallow_config::ResolvedConfig,
    config_ms: f64,
    seams: &HealthSeams<'_>,
) -> Result<HealthResult, ExitCode> {
    let t = Instant::now();
    let session = fallow_engine::session::AnalysisSession::from_resolved_config(config)
        .map_err(|e| emit_error(&format!("analysis failed: {e}"), 2, opts.output))?;
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;
    let parts = session.parsed_parts_uncached(true);
    let pre_computed_analysis =
        fallow_engine::health::should_precompute_dead_code_analysis(opts, session.config())
            .then(|| session.analyze_dead_code_with_parsed_modules(&parts.modules))
            .transpose()
            .map_err(|e| emit_error(&format!("analysis failed: {e}"), 2, opts.output))?;
    let config = parts.config;
    let files = parts.files;
    let modules = parts.modules;
    let workspaces = parts.workspaces;
    let workspace_diagnostics = if pre_computed_analysis.is_some() {
        session.current_workspace_diagnostics()
    } else {
        parts.workspace_diagnostics
    };
    let parse_ms = parts.parse_ms;
    let parse_cpu_ms = parts.parse_cpu_ms;

    let scope_inputs = build_health_scope_inputs(opts, &config)?;
    execute_health_inner(
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
            dead_code_results: None,
            styling_artifacts: None,
            pre_computed_analysis,
            pre_computed_duplication: None,
            workspaces,
            workspace_diagnostics,
        },
        scope_inputs,
        seams,
    )
    .map_err(|e| health_err_to_exit(e, opts.output))
}

pub fn benchmark_execute_health_with_response(
    opts: &HealthOptions<'_>,
    response_bytes: &[u8],
    request_len: &std::cell::Cell<usize>,
) -> Result<HealthResult, ExitCode> {
    let analyzer = |options: &fallow_engine::health::RuntimeCoverageOptions,
                    input: RuntimeCoverageSeamInput<'_>| {
        coverage::analyze_with_transport(
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
            |request, _quiet, output| {
                let (response, len) =
                    coverage::in_process_response_transport(request, response_bytes, output)?;
                request_len.set(len);
                Ok(response)
            },
        )
    };
    let seams = HealthSeams {
        runtime_coverage_analyzer: &analyzer,
        note_graph_structure: &|_module_count, _edge_count| {},
    };
    let (config, config_ms) = load_health_config(opts)?;
    execute_health_with_config_and_seams(opts, config, config_ms, &seams)
}

pub fn run_health(
    opts: &HealthOptions<'_>,
    json_style: crate::json_style::JsonStyle,
    type_aware: &TypeAwareHealthOptions<'_>,
) -> ExitCode {
    let mut completeness_failed = false;
    let (config, config_ms) = match load_health_config(opts) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let resolved_type_aware = match resolve_type_aware_health_options(type_aware, &config) {
        Ok(options) => options,
        Err(message) => return emit_error(&message, 2, opts.output),
    };
    let requested = type_aware.requested || (type_aware.unfiltered && resolved_type_aware.enabled);
    let mut degraded_meta = None;
    let semantic = if requested {
        let enabled = resolved_type_aware.enabled;
        if !enabled {
            return emit_error(
                "--type-coupling requires --type-aware or typeAware.enabled in config",
                2,
                opts.output,
            );
        }
        let projects = resolved_type_aware.projects;
        let require = resolved_type_aware.require;
        let outcome = match fallow_api::analyze_type_coupling(opts.root, &projects, &[]) {
            Ok(outcome) => Some(outcome),
            Err(error) => {
                match crate::type_aware_degrade::degrade_or_fail(
                    &crate::type_aware_degrade::DegradeContext {
                        root: opts.root,
                        error: &error.to_string(),
                        failure_label: "Type-aware coupling failed",
                        require,
                        quiet: opts.quiet,
                        output: opts.output,
                    },
                ) {
                    Ok(meta) => degraded_meta = Some(meta),
                    Err(code) => return code,
                }
                None
            }
        };
        completeness_failed = outcome.as_ref().is_some_and(|outcome| {
            require == fallow_config::TypeAwareRequire::Complete
                && outcome.report.status != fallow_types::semantic::SemanticCompleteness::Complete
        });
        outcome
    } else {
        None
    };
    let mut execution_opts = opts.clone();
    if let Some(identity) = semantic
        .as_ref()
        .and_then(|outcome| outcome.type_aware.meta.identity.clone())
    {
        execution_opts.analysis_identity = identity;
    }
    let mut result = match execute_health_with_config(&execution_opts, config, config_ms) {
        Ok(result) => result,
        Err(code) => return code,
    };
    let required_completeness = result.config.type_aware.require.into();
    result.type_aware_meta = semantic
        .map(|outcome| {
            let mut meta = outcome.type_aware.meta;
            meta.required_completeness = Some(required_completeness);
            meta
        })
        .or(degraded_meta);
    if let Some(ref timings) = result.timings {
        report::print_health_performance(timings, opts.output, json_style);
    }
    let baseline_saved_by = report_loaded_baseline(&result, opts.baseline);
    let code = print_health_result(
        &result,
        HealthPrintOptions {
            quiet: opts.quiet,
            explain: opts.explain,
            gates: opts.gates,
            baseline_path: opts.baseline,
            baseline_saved_by: baseline_saved_by.as_deref(),
            summary: opts.summary,
            summary_heading: true,
            show_explain_tip: true,
            type_aware_scope: None,
            skip_score_and_trend: false,
            css_requested: opts.css,
            json_style,
        },
    );
    if code == ExitCode::SUCCESS && completeness_failed {
        ExitCode::from(1)
    } else {
        code
    }
}

pub struct ResolvedTypeAwareHealthOptions {
    pub enabled: bool,
    pub projects: Vec<std::path::PathBuf>,
    pub require: fallow_config::TypeAwareRequire,
}

pub fn resolve_type_aware_health_options(
    options: &TypeAwareHealthOptions<'_>,
    config: &fallow_config::ResolvedConfig,
) -> Result<ResolvedTypeAwareHealthOptions, String> {
    let env_enabled = std::env::var("FALLOW_TYPE_AWARE")
        .ok()
        .map(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(
                "FALLOW_TYPE_AWARE must be one of true, false, 1, 0, yes, no, on, or off"
                    .to_string(),
            ),
        })
        .transpose()?;
    let enabled = options
        .enabled
        .or(env_enabled)
        .unwrap_or(config.type_aware.enabled);
    let projects = if !options.projects.is_empty() {
        options.projects.to_vec()
    } else if let Some(value) = std::env::var_os("FALLOW_TYPE_AWARE_PROJECTS") {
        std::env::split_paths(&value).collect()
    } else {
        config
            .type_aware
            .projects
            .iter()
            .map(std::path::PathBuf::from)
            .collect()
    };
    let require = if let Some(require) = options.require {
        require
    } else if let Ok(value) = std::env::var("FALLOW_TYPE_AWARE_REQUIRE") {
        match value.trim().to_ascii_lowercase().as_str() {
            "best-effort" => fallow_config::TypeAwareRequire::BestEffort,
            "complete" => fallow_config::TypeAwareRequire::Complete,
            _ => {
                return Err("FALLOW_TYPE_AWARE_REQUIRE must be best-effort or complete".to_string());
            }
        }
    } else {
        config.type_aware.require
    };
    Ok(ResolvedTypeAwareHealthOptions {
        enabled,
        projects,
        require,
    })
}

/// Result of executing health analysis without printing.
pub type HealthResult =
    fallow_engine::health::HealthAnalysisResult<crate::report::OwnershipResolver>;

/// Print health results and return appropriate exit code.
///
/// When called from combined mode (`fallow --score` / `fallow --trend`),
/// `skip_score_and_trend` MUST be `true`: the orientation header already
/// renders both blocks and rendering them a second time here would duplicate
/// the lines. Standalone `fallow health` invocations pass `false`.
///
/// Exit-code gating (when `report_only` is `false`): the score gate
/// (`--min-score`), the findings gate (`--min-severity`, or any finding when
/// no gate flag is set), the runtime-coverage gate, the opt-in stale-baseline
/// gate (`--fail-on-stale-baseline`) and the coverage-gap gate are
/// OR-combined. `report_only` short-circuits all of them to
/// `ExitCode::SUCCESS` after rendering. Combined and audit callers pass
/// `report_only: false` (they own their own gate semantics).
///
/// Callers that pass `min_score: Some(_)` must ensure
/// `result.report.health_score` is `Some` (the CLI guarantees this because
/// `--min-score` implies `--score`). If the score is missing the score gate
/// cannot evaluate, so a direct API caller that requests a score gate without
/// computing the score would get a permissive `ExitCode::SUCCESS`.
#[derive(Clone, Copy)]
pub struct HealthPrintOptions<'a> {
    pub quiet: bool,
    pub explain: bool,
    pub gates: HealthGateOptions,
    /// Loaded `--baseline` path, so the stale-baseline gate can name the file
    /// to re-save. `None` when no baseline was loaded, which makes the gate
    /// inert.
    pub baseline_path: Option<&'a std::path::Path>,
    /// The command that saved the loaded baseline, when it names one other than
    /// `health`. The load note resolves it once and passes it here, so the gate
    /// line names the same writer and reads no file. `None` on the routes that
    /// print no note. Those routes arm no gate either.
    pub baseline_saved_by: Option<&'a str>,
    pub summary: bool,
    pub summary_heading: bool,
    pub show_explain_tip: bool,
    pub type_aware_scope: Option<&'static str>,
    pub skip_score_and_trend: bool,
    /// Whether `--css` was requested. Forwarded to the human renderer so an empty
    /// CSS result (no import-reachable stylesheet) is explained rather than
    /// silently omitted. Defaults `false` for callers that do not request CSS.
    pub css_requested: bool,
    pub json_style: crate::json_style::JsonStyle,
}

pub fn print_health_result(result: &HealthResult, options: HealthPrintOptions<'_>) -> ExitCode {
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
        note_stale_baseline_gate_stood_down(result, options);
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

fn health_report_context<'a>(
    result: &'a HealthResult,
    options: HealthPrintOptions<'a>,
) -> report::ReportContext<'a> {
    report::ReportContext {
        root: &result.config.root,
        rules: &result.config.rules,
        workspace_diagnostics: &result.workspace_diagnostics,
        elapsed: result.elapsed,
        quiet: options.quiet,
        explain: options.explain,
        type_aware: result.type_aware_meta.as_ref(),
        type_aware_scope: options.type_aware_scope,
        group_by: None,
        top: None,
        summary: options.summary,
        summary_heading: options.summary_heading,
        show_explain_tip: options.show_explain_tip,
        baseline_matched: None,
        baseline_staleness: None,
        gate_outcomes: health_gate_outcomes(result, options),
        config_fixable: false,
        skip_score_and_trend: options.skip_score_and_trend,
        css_requested: options.css_requested,
        json_style: options.json_style,
        include_fragments: true,
    }
}

/// The gates a health run armed, for the envelope's `gate_outcomes`.
///
/// Every entry reads the same predicate the exit path reads, so the published
/// verdict and the process status cannot disagree. `--report-only` returns
/// `ExitCode::SUCCESS` before any gate is consulted, so it clamps `enforced` to
/// false on every entry while leaving each verdict in place; that is the case a
/// boolean-only shape could not express, and the stale-baseline entry is
/// clamped with the rest rather than reporting the flag it was armed with.
///
/// A gate armed by an explicit flag or by config always produces an entry.
/// `health-findings` fails a plain `fallow health` run on any finding. It is
/// the command's default exit rule, so it is always in the object, also when
/// no flag armed a gate. A JSON reader then sees a failing run without the
/// exit code.
fn health_gate_outcomes(
    result: &HealthResult,
    options: HealthPrintOptions<'_>,
) -> Option<fallow_output::GateOutcomes> {
    use fallow_output::{GateName, GateOutcome, GateStatus};

    let enforced = !options.gates.report_only;
    let mut gates = fallow_output::GateOutcomes::new();

    if let Some(threshold) = options.gates.min_score {
        // `--min-score` implies `--score`, so a missing score means the caller
        // is a programmatic one that requested the gate without computing what
        // it compares. Report the stand-down rather than nothing, or "armed"
        // and "not armed" read identically.
        gates.insert(
            GateName::HealthMinScore,
            result.report.health_score.as_ref().map_or_else(
                || GateOutcome::new(GateStatus::Skipped, false),
                |score| {
                    GateOutcome::measured(
                        crate::gates::status_of(score.score < threshold),
                        enforced,
                        score.score,
                        threshold,
                    )
                },
            ),
        );
    }

    if let Some(min_sev) = options.gates.min_severity {
        let reached = result
            .report
            .findings
            .iter()
            .filter(|f| f.severity >= min_sev)
            .count();
        gates.insert(
            GateName::HealthMinSeverity,
            GateOutcome::counted(
                crate::gates::status_of(reached > 0),
                enforced,
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a finding count never approaches the f64 integer limit"
                )]
                {
                    reached as f64
                },
                severity_floor_label(min_sev),
            ),
        );
    }

    if result.should_fail_on_coverage_gaps {
        gates.insert(
            GateName::HealthCoverageGaps,
            GateOutcome::new(
                crate::gates::status_of(result.coverage_gaps_has_findings),
                enforced,
            ),
        );
    }

    // Armed by `--runtime-coverage`, so it belongs with the flag-armed gates
    // rather than behind the default-rule guard below: without this a run whose
    // only gate is runtime coverage exits 1 and publishes nothing.
    if result.report.runtime_coverage.is_some() {
        gates.insert(
            GateName::HealthRuntimeCoverage,
            GateOutcome::new(
                crate::gates::status_of(has_failing_runtime_coverage(result)),
                enforced,
            ),
        );
    }

    gates.insert_if(
        GateName::StaleBaseline,
        crate::gates::stale_baseline_outcome(
            result.report.summary.baseline_staleness.as_ref(),
            options.gates.fail_on_stale_baseline && enforced,
        ),
    );

    // The default findings rule. With `--min-severity` the findings gate IS
    // the severity gate, already recorded above under its own name.
    if options.gates.min_severity.is_none() {
        gates.insert(
            GateName::HealthFindings,
            if options.gates.min_score.is_some() {
                // `--min-score` alone turns the findings branch off, which is
                // what "complexity findings become informational" means.
                GateOutcome::new(GateStatus::Skipped, false)
            } else {
                GateOutcome::new(
                    crate::gates::status_of(!result.report.findings.is_empty()),
                    enforced,
                )
            },
        );
    }

    gates.into_option()
}

/// The wire spelling of a severity floor, for `threshold_label`.
const fn severity_floor_label(severity: fallow_output::FindingSeverity) -> &'static str {
    match severity {
        fallow_output::FindingSeverity::Moderate => "moderate",
        fallow_output::FindingSeverity::High => "high",
        fallow_output::FindingSeverity::Critical => "critical",
    }
}

/// The OR of every health exit gate, with each one evaluated before the verdict
/// is combined so that none of them can swallow another's stderr line.
///
/// The baseline gate is why this is not a short-circuiting chain: the score and
/// findings gates have their condition printed in the report, a stale baseline
/// has it nowhere, so a run that already fails the findings gate would exit 1
/// with nothing about the baseline the user explicitly gated on.
fn health_exit_gate_failed(result: &HealthResult, options: HealthPrintOptions<'_>) -> bool {
    let score = score_gate_failed(result, options);
    let findings = findings_gate_failed(result, options);
    let runtime_coverage = has_failing_runtime_coverage(result);
    let stale_baseline = stale_baseline_gate_failed(result, options);
    score || findings || runtime_coverage || stale_baseline
}

/// Say what this run made of the loaded baseline, and record it for the
/// `recheck-baseline` next step.
///
/// Both happen here rather than at the engine's load site, which cannot reach
/// CLI runtime state or print a CLI note, and only for the standalone command:
/// `audit` and the combined run build their next steps from their own builders
/// and would otherwise offer a `health` path on an envelope that is not
/// health's.
/// Returns the command that saved the loaded file, when it names one other than
/// `health`. The gate line then names the same writer as this note, and reads no
/// file.
fn report_loaded_baseline(
    result: &HealthResult,
    baseline_path: Option<&std::path::Path>,
) -> Option<String> {
    let path = baseline_path?;
    let staleness = result.report.summary.baseline_staleness.as_ref()?;
    let saved_by = note_unrecognised_health_baseline(result, baseline_path, "--baseline");
    crate::output_runtime::set_loaded_baseline(crate::output_runtime::LoadedBaselineRecheck {
        command: "health",
        path: path.display().to_string(),
        baseline_entries: staleness.baseline_entries,
        scope_reasons: staleness.scope_reasons,
    });
    saved_by
}

/// Say that the loaded baseline is not a health baseline.
///
/// Split from [`report_loaded_baseline`] because `fallow audit` needs the note
/// and must not get the `recheck-baseline` record beside it: the audit envelope
/// is not health's, so a `fallow health` next step on it would point at the
/// wrong report. `dupes` and `dead-code` reach their notes through their own
/// load sites, which audit shares.
///
/// `flag` is the argument that carried the path: `--health-baseline` on an audit
/// and `--baseline` on the standalone command.
///
/// Returns the command that saved the file, when it names one other than
/// `health`. The engine classifies this one command's baseline, and the bytes are
/// gone before the CLI prints under `--quiet`. So this function reads the file
/// once and passes the answer to the gate.
pub fn note_unrecognised_health_baseline(
    result: &HealthResult,
    baseline_path: Option<&std::path::Path>,
    flag: &str,
) -> Option<String> {
    let staleness = result.report.summary.baseline_staleness.as_ref()?;
    let path = baseline_path?;
    if !staleness.unrecognised_format {
        return None;
    }
    let saved_by = saved_by_another_command(path);
    crate::baseline_gate::note_unrecognised_baseline(
        Some(path),
        true,
        saved_by.as_deref(),
        fallow_engine::baseline::BaselineKind::Health,
        flag,
    );
    saved_by
}

/// The `kind` a baseline file names, when it names a command other than
/// `health`. `None` for a file that names none, which is every baseline saved
/// before the member existed, and for a file that can no longer be read.
fn saved_by_another_command(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    match fallow_engine::baseline::classify_baseline_file(
        &content,
        fallow_engine::baseline::BaselineKind::Health,
    ) {
        fallow_engine::baseline::BaselineFileKind::Foreign(found) => Some(found),
        _ => None,
    }
}

/// Say that `--report-only` suppressed the gate, so a job that passes both
/// flags learns its baseline was never judged instead of going green forever.
///
/// `--report-only` is an explicit request never to fail the run, so the gate
/// obeys it rather than overriding it; it just does not obey it in silence.
fn note_stale_baseline_gate_stood_down(result: &HealthResult, options: HealthPrintOptions<'_>) {
    let Some(staleness) = result.report.summary.baseline_staleness.as_ref() else {
        return;
    };
    // A baseline with no entries gives the gate nothing to judge, so no gate
    // stands down. A file this command cannot read as its own carries the same
    // zero, and there the gate rule holds. `--report-only` then suppresses a real
    // verdict, and it must say so.
    if staleness.baseline_entries == 0 && !staleness.unrecognised_format {
        return;
    }
    crate::baseline_gate::note_stood_down(
        options.baseline_path,
        options.gates.fail_on_stale_baseline,
        "--report-only never fails a run",
    );
}

/// The opt-in `--fail-on-stale-baseline` gate. Reads the staleness the engine
/// already put in the report, so no extra plumbing crosses the engine boundary.
fn stale_baseline_gate_failed(result: &HealthResult, options: HealthPrintOptions<'_>) -> bool {
    let Some(staleness) = result.report.summary.baseline_staleness.as_ref() else {
        return false;
    };
    crate::baseline_gate::gate_failed_from_envelope(
        staleness,
        options.baseline_path,
        options.gates.fail_on_stale_baseline,
        options.baseline_saved_by,
        fallow_engine::baseline::BaselineKind::Health,
    )
}

fn score_gate_failed(result: &HealthResult, options: HealthPrintOptions<'_>) -> bool {
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

fn findings_gate_failed(result: &HealthResult, options: HealthPrintOptions<'_>) -> bool {
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

fn maybe_print_score_gate_note(result: &HealthResult, options: HealthPrintOptions<'_>) {
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
            test_only_reference: None,
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
            branching_by_file: fallow_engine::health::BranchingByFile::default(),
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
            type_aware_meta: None,
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
                    fail_on_stale_baseline: false,
                },
                baseline_path: None,
                baseline_saved_by: None,
                summary: false,
                summary_heading: true,
                show_explain_tip: true,
                type_aware_scope: None,
                skip_score_and_trend: false,
                css_requested: false,
                json_style: crate::json_style::JsonStyle::Compact,
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
            branching_by_file: fallow_engine::health::BranchingByFile::default(),
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
            type_aware_meta: None,
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
                    baseline_path: None,
                    baseline_saved_by: None,
                    summary: false,
                    summary_heading: true,
                    show_explain_tip: true,
                    type_aware_scope: None,
                    skip_score_and_trend: false,
                    css_requested: false,
                    json_style: crate::json_style::JsonStyle::Compact,
                },
            ),
            ExitCode::from(1),
        );
    }
}
