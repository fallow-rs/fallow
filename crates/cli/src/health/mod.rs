mod assembly;
pub mod coverage;
mod coverage_gaps;
mod coverage_intelligence;
mod grouping;
mod hotspots;
pub mod ownership;
mod runtime_filter;
pub mod scoring;
mod targets;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use colored::Colorize;
use fallow_config::{OutputFormat, ResolvedConfig};

use crate::baseline::{HealthBaselineData, filter_new_health_findings, filter_new_health_targets};
use crate::check::{get_changed_files, resolve_workspace_scope};
use crate::error::emit_error;
pub use crate::health_types::*;
use crate::report;
use crate::vital_signs;

use assembly::assemble_health_report;
use hotspots::compute_hotspots;
use runtime_filter::{
    RuntimeCoverageFilterContext, apply_runtime_coverage_filters, relative_to_root,
};
use scoring::compute_file_scores;

/// Pre-parsed data from the dead-code pipeline, shared with health to avoid re-analysis.
pub struct SharedParseData {
    pub files: Vec<fallow_types::discover::DiscoveredFile>,
    pub modules: Vec<fallow_types::extract::ModuleInfo>,
    /// Full analysis output (graph + results) for file scoring.
    pub analysis_output: Option<fallow_core::AnalysisOutput>,
}
use targets::{TargetAuxData, compute_refactoring_targets};

pub struct RuntimeCoverageOptions {
    pub path: std::path::PathBuf,
    pub min_invocations_hot: u64,
    /// Minimum total trace volume before high-confidence `safe_to_delete` /
    /// `review_required` verdicts may be emitted. Below this the sidecar caps
    /// confidence at `medium`. `None` lets the sidecar use its spec-default
    /// (5000).
    pub min_observation_volume: Option<u32>,
    /// Fraction of total trace count below which an invoked function is
    /// classified as `low_traffic` rather than `active`. `None` lets the
    /// sidecar use its spec-default (0.001 = 0.1%).
    pub low_traffic_threshold: Option<f64>,
    pub license_jwt: String,
    pub watermark: Option<crate::health_types::RuntimeCoverageWatermark>,
}

/// Sort criteria for complexity output.
#[derive(Clone, clap::ValueEnum)]
pub enum SortBy {
    Severity,
    Cyclomatic,
    Cognitive,
    Lines,
}

pub struct HealthOptions<'a> {
    pub root: &'a std::path::Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub max_cyclomatic: Option<u16>,
    pub max_cognitive: Option<u16>,
    /// Maximum CRAP score threshold (overrides config, default 30.0). Functions
    /// meeting or exceeding this score are reported as complexity findings.
    pub max_crap: Option<f64>,
    pub top: Option<usize>,
    pub sort: SortBy,
    pub production: bool,
    pub production_override: Option<bool>,
    pub changed_since: Option<&'a str>,
    pub diff_index: Option<&'a crate::report::ci::diff_filter::DiffIndex>,
    pub use_shared_diff_index: bool,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub baseline: Option<&'a std::path::Path>,
    pub save_baseline: Option<&'a std::path::Path>,
    pub complexity: bool,
    /// Include the per-decision-point complexity breakdown (`contributions[]`)
    /// on each complexity finding in JSON output. Drives the VS Code inline
    /// editor breakdown; off by default to keep CI/default output lean.
    pub complexity_breakdown: bool,
    pub file_scores: bool,
    /// Explicitly include coverage gaps in the rendered report.
    pub coverage_gaps: bool,
    /// Allow config severity to enable coverage gap reporting when the caller
    /// did not explicitly select health sections.
    pub config_activates_coverage_gaps: bool,
    pub hotspots: bool,
    pub ownership: bool,
    pub ownership_emails: Option<fallow_config::EmailMode>,
    pub targets: bool,
    /// Run the full health pipeline even if some sections are hidden, so score
    /// and snapshot outputs stay accurate.
    pub force_full: bool,
    /// Render only the score/trend-oriented output, hiding supporting sections
    /// that were computed solely for score accuracy.
    pub score_only_output: bool,
    /// Enforce the configured coverage-gap severity as a failing quality gate.
    pub enforce_coverage_gap_gate: bool,
    pub effort: Option<EffortEstimate>,
    pub score: bool,
    pub min_score: Option<f64>,
    pub since: Option<&'a str>,
    pub min_commits: Option<u32>,
    pub explain: bool,
    /// When true, emit a condensed summary instead of full item-level output.
    #[allow(
        dead_code,
        reason = "wired from CLI but consumed by combined mode, not standalone health"
    )]
    pub summary: bool,
    pub save_snapshot: Option<std::path::PathBuf>,
    pub trend: bool,
    pub group_by: Option<crate::GroupBy>,
    /// Path to Istanbul-format coverage data (coverage-final.json) for accurate CRAP scores.
    pub coverage: Option<&'a std::path::Path>,
    /// Rebase file paths in coverage data by stripping this prefix and prepending project root.
    pub coverage_root: Option<&'a std::path::Path>,
    /// Show detailed pipeline timing breakdown.
    pub performance: bool,
    /// Only exit with error for findings at or above this severity level.
    pub min_severity: Option<FindingSeverity>,
    /// Render the score and findings but never fail CI on any quality gate.
    /// Mutually exclusive with `min_score` / `min_severity` (validated at the
    /// CLI dispatch layer).
    pub report_only: bool,
    /// Paid runtime coverage sidecar input.
    pub runtime_coverage: Option<RuntimeCoverageOptions>,
    /// Import churn from a `fallow-churn/v1` JSON file (`--churn-file`) instead
    /// of `git log`, for hotspots / ownership on non-git VCS. Resolved relative
    /// to `root`. When set, the git churn path is bypassed entirely.
    pub churn_file: Option<&'a std::path::Path>,
}

struct HealthPipelineTimings {
    config: f64,
    discover: f64,
    parse: f64,
    /// Summed parse CPU time across rayon workers; `0.0` when parse was reused.
    parse_cpu: f64,
    /// True when discover + parse were reused from the upstream check pass.
    shared_parse: bool,
}

struct HealthPipelineInput {
    config: ResolvedConfig,
    files: Vec<fallow_types::discover::DiscoveredFile>,
    modules: Vec<fallow_types::extract::ModuleInfo>,
    timings: HealthPipelineTimings,
    pre_computed_analysis: Option<fallow_core::AnalysisOutput>,
}

/// Validate an explicit `--churn-file` up front so a malformed import is a loud
/// hard error (exit 2) rather than a silent hotspot skip. Runs before the
/// pipeline, and only when churn would actually be consumed (`--hotspots` /
/// `--targets`; `--ownership` is subsumed because the dispatch layer sets
/// `hotspots = hotspots || ownership` before building `HealthOptions`), so an
/// inert `--churn-file` on a non-churn run is not penalized. The gate condition
/// mirrors `hotspots::fetch_churn_data`'s `needs_churn` exactly, keeping the
/// validate-iff-consume invariant. Failing here (instead of inside the parallel
/// hotspot pass) keeps combined `--format json` to a single error document. The
/// file is re-read in `fetch_churn_data`; the duplicate read is negligible for
/// realistic churn files and bounded by `MAX_CHURN_EVENTS`.
fn validate_churn_file(opts: &HealthOptions<'_>) -> Result<(), ExitCode> {
    if let Some(churn_file) = opts.churn_file
        && (opts.hotspots || opts.targets)
    {
        let resolved = scoring::resolve_relative_to_root(churn_file, Some(opts.root));
        fallow_core::churn::analyze_churn_from_file(&resolved, opts.root)
            .map_err(|e| emit_error(&e, 2, opts.output))?;
    }
    Ok(())
}

/// Run health analysis using pre-parsed modules from the dead-code pipeline.
///
/// Skips file discovery and parsing (saves ~1.9s on 21K-file projects).
pub fn execute_health_with_shared_parse(
    opts: &HealthOptions<'_>,
    shared: SharedParseData,
) -> Result<HealthResult, ExitCode> {
    scoring::validate_coverage_root_absolute(opts.coverage_root)
        .map_err(|e| emit_error(&e, 2, opts.output))?;
    validate_churn_file(opts)?;
    let t = Instant::now();
    let config = crate::load_config_for_analysis(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production_override
            .or_else(|| opts.production.then_some(true)),
        opts.quiet,
        fallow_config::ProductionAnalysis::Health,
    )?;
    let config_ms = t.elapsed().as_secs_f64() * 1000.0;
    execute_health_inner(
        opts,
        HealthPipelineInput {
            config,
            files: shared.files,
            modules: shared.modules,
            timings: HealthPipelineTimings {
                config: config_ms,
                discover: 0.0,
                parse: 0.0,
                parse_cpu: 0.0,
                shared_parse: true,
            },
            pre_computed_analysis: shared.analysis_output,
        },
    )
}

pub fn execute_health(opts: &HealthOptions<'_>) -> Result<HealthResult, ExitCode> {
    scoring::validate_coverage_root_absolute(opts.coverage_root)
        .map_err(|e| emit_error(&e, 2, opts.output))?;
    validate_churn_file(opts)?;
    let t = Instant::now();
    let config = crate::load_config_for_analysis(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production_override
            .or_else(|| opts.production.then_some(true)),
        opts.quiet,
        fallow_config::ProductionAnalysis::Health,
    )?;
    let config_ms = t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    let files = fallow_core::discover::discover_files_with_plugin_scopes(&config);
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;

    let cache = if config.no_cache {
        None
    } else {
        fallow_core::cache::CacheStore::load(
            &config.cache_dir,
            config.cache_config_hash,
            fallow_core::resolve_cache_max_size_bytes(&config),
        )
    };
    let t = Instant::now();
    let parse_result = fallow_core::extract::parse_all_files(&files, cache.as_ref(), true);
    let parse_ms = t.elapsed().as_secs_f64() * 1000.0;
    let parse_cpu_ms = parse_result.parse_cpu_ms;

    execute_health_inner(
        opts,
        HealthPipelineInput {
            config,
            files,
            modules: parse_result.modules,
            timings: HealthPipelineTimings {
                config: config_ms,
                discover: discover_ms,
                parse: parse_ms,
                parse_cpu: parse_cpu_ms,
                shared_parse: false,
            },
            pre_computed_analysis: None,
        },
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "health pipeline orchestration with many optional features"
)]
fn execute_health_inner(
    opts: &HealthOptions<'_>,
    input: HealthPipelineInput,
) -> Result<HealthResult, ExitCode> {
    let start = Instant::now();
    let HealthPipelineInput {
        config,
        files,
        modules,
        timings:
            HealthPipelineTimings {
                config: config_ms,
                discover: discover_ms,
                parse: parse_ms,
                parse_cpu: parse_cpu_ms,
                shared_parse,
            },
        pre_computed_analysis,
    } = input;

    let max_cyclomatic = opts.max_cyclomatic.unwrap_or(config.health.max_cyclomatic);
    let max_cognitive = opts.max_cognitive.unwrap_or(config.health.max_cognitive);
    let max_crap = opts.max_crap.unwrap_or(config.health.max_crap);
    let enforce_crap = max_crap > 0.0;

    let ignore_set = build_ignore_set(&config.health.ignore);
    let changed_files = opts
        .changed_since
        .and_then(|git_ref| get_changed_files(opts.root, git_ref));
    let diff_index = match opts.diff_index {
        Some(index) => Some(index),
        None if opts.use_shared_diff_index => crate::report::ci::diff_filter::shared_diff_index(),
        None => None,
    };
    let ws_roots = resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;

    let group_resolver = if opts.group_by.is_some() {
        crate::build_ownership_resolver(
            opts.group_by,
            opts.root,
            config.codeowners.as_deref(),
            opts.output,
        )?
    } else {
        None
    };

    let file_paths: rustc_hash::FxHashMap<_, _> = files.iter().map(|f| (f.id, &f.path)).collect();

    let t = Instant::now();
    let (findings, files_analyzed, total_functions) = collect_findings(
        &modules,
        &file_paths,
        &config.root,
        &ignore_set,
        changed_files.as_ref(),
        ws_roots.as_deref(),
        max_cyclomatic,
        max_cognitive,
        opts.complexity_breakdown,
    );
    let mut findings = findings;
    let complexity_ms = t.elapsed().as_secs_f64() * 1000.0;

    let config_coverage_enabled = config.rules.coverage_gaps != fallow_config::Severity::Off;
    let report_coverage_gaps =
        opts.coverage_gaps || (opts.config_activates_coverage_gaps && config_coverage_enabled);
    let enforce_coverage_gaps = opts.enforce_coverage_gap_gate
        && config.rules.coverage_gaps == fallow_config::Severity::Error;

    let istanbul_coverage = load_health_coverage(opts, &config)?;

    let needs_file_scores = opts.file_scores
        || report_coverage_gaps
        || enforce_coverage_gaps
        || opts.hotspots
        || opts.targets
        || opts.force_full
        || enforce_crap;
    let needs_analysis_output = needs_file_scores || opts.runtime_coverage.is_some();
    let mut shared_analysis_output = prepare_shared_analysis_output(
        opts,
        &config,
        &modules,
        pre_computed_analysis,
        needs_analysis_output,
    )?;
    if let Some(graph) = shared_analysis_output
        .as_ref()
        .and_then(|output| output.graph.as_ref())
    {
        crate::telemetry::note_graph_structure(graph);
    }

    let mut runtime_coverage = analyze_runtime_coverage(
        opts,
        &config,
        &modules,
        shared_analysis_output.as_ref(),
        istanbul_coverage.as_ref(),
        &file_paths,
        &ignore_set,
        changed_files.as_ref(),
        ws_roots.as_deref(),
    )?;

    let precomputed_for_scores = if needs_file_scores {
        shared_analysis_output.take()
    } else {
        None
    };

    let (file_score_result, file_scores_ms, churn_fetch) = compute_file_scores_and_churn(
        opts,
        &config,
        &modules,
        &file_paths,
        changed_files.as_ref(),
        ws_roots.as_deref(),
        &ignore_set,
        istanbul_coverage.as_ref(),
        needs_file_scores,
        precomputed_for_scores,
    )?;
    let (git_churn_ms, git_churn_cache_hit) = churn_fetch
        .as_ref()
        .map_or((0.0, false), |cf| (cf.git_log_ms, cf.cache_hit));
    let (score_output, files_scored, average_maintainability) = file_score_result;

    print_slow_churn_note(opts, churn_fetch.as_ref());

    let file_scores_slice = score_output
        .as_ref()
        .map_or(&[] as &[_], |o| o.scores.as_slice());

    if enforce_crap && let Some(ref score_out) = score_output {
        merge_crap_findings(
            &mut findings,
            &CrapFindingMergeInput {
                modules: &modules,
                file_paths: &file_paths,
                config_root: &config.root,
                ignore_set: &ignore_set,
                changed_files: changed_files.as_ref(),
                ws_roots: ws_roots.as_deref(),
                per_function_crap: &score_out.per_function_crap,
                template_inherit_provenance: &score_out.template_inherit_provenance,
                max_crap,
                max_cyclomatic,
                max_cognitive,
                complexity_breakdown: opts.complexity_breakdown,
            },
        );
    }
    let template_owner_lookup = score_output
        .as_ref()
        .map(|o| &o.template_inherit_provenance);
    append_component_rollup_findings(
        &mut findings,
        template_owner_lookup,
        max_cyclomatic,
        max_cognitive,
    );

    if let Some(diff_index) = diff_index {
        filter_complexity_findings_by_diff(&mut findings, diff_index, &config.root);
    }

    sort_findings(&mut findings, &opts.sort);
    let total_above_threshold = findings.len();
    let (sev_critical, sev_high, sev_moderate) = count_finding_severities(&findings);

    let loaded_baseline = apply_health_baseline_and_top(opts, &config, &mut findings)?;

    let candidate_paths = collect_candidate_paths(
        &files,
        &config,
        changed_files.as_ref(),
        ws_roots.as_deref(),
        &ignore_set,
    );

    let (dupes_report, duplication_ms) =
        compute_health_duplication_report(opts, &config, &files, &candidate_paths);

    let (hotspots, hotspot_summary, hotspots_ms) = compute_filtered_hotspots(
        opts,
        &config,
        file_scores_slice,
        &ignore_set,
        ws_roots.as_deref(),
        churn_fetch,
        diff_index,
    );

    let (targets, target_thresholds, targets_ms) = compute_filtered_targets(
        opts,
        score_output.as_ref(),
        file_scores_slice,
        &hotspots,
        loaded_baseline.as_ref(),
        &config,
        diff_index,
        dupes_report.as_ref(),
    );

    filter_runtime_coverage_report(
        opts,
        &config,
        runtime_coverage.as_mut(),
        loaded_baseline.as_ref(),
        changed_files.as_ref(),
        diff_index,
    );

    save_health_baseline_if_requested(
        opts,
        &config,
        &findings,
        runtime_coverage.as_ref(),
        &targets,
    )?;

    let project_subset = if candidate_paths.len() == files.len() {
        SubsetFilter::Full
    } else {
        SubsetFilter::Paths(&candidate_paths)
    };
    let total_files_scoped = candidate_paths.len();
    let vital_signs_input = VitalSignsAndCountsInput {
        score_output: score_output.as_ref(),
        modules: &modules,
        file_paths: &file_paths,
        needs_file_scores,
        file_scores_slice,
        needs_hotspots: opts.hotspots || opts.targets,
        hotspots: &hotspots,
        total_files: total_files_scoped,
        subset: &project_subset,
    };
    let (mut vital_signs, mut counts) = compute_vital_signs_and_counts(&vital_signs_input);

    let health_score = compute_health_score_metrics(
        opts,
        dupes_report.as_ref(),
        &mut vital_signs,
        &mut counts,
        total_files_scoped,
    );

    let large_functions = collect_filtered_large_functions(
        &vital_signs,
        &modules,
        &file_paths,
        &config,
        &ignore_set,
        changed_files.as_ref(),
        ws_roots.as_deref(),
        diff_index,
    );

    let active_coverage_model = if istanbul_coverage.is_some() {
        Some(crate::health_types::CoverageModel::Istanbul)
    } else {
        Some(crate::health_types::CoverageModel::StaticEstimated)
    };

    if let Some(ref snapshot_path) = opts.save_snapshot {
        save_snapshot(SnapshotInput {
            opts,
            snapshot_path,
            vital_signs: &vital_signs,
            counts: &counts,
            hotspot_summary: hotspot_summary.as_ref(),
            health_score: health_score.as_ref(),
            coverage_model: active_coverage_model,
        })?;
    }

    let health_trend = compute_health_trend(opts, &vital_signs, &counts, health_score.as_ref());

    let coverage_gaps_has_findings = score_output
        .as_ref()
        .is_some_and(|output| !output.coverage.report.is_empty());

    let baseline_active = opts.baseline.is_some() || opts.save_baseline.is_some();
    let action_opts = if baseline_active {
        crate::health_types::HealthActionOptions {
            omit_suppress_line: true,
            omit_reason: Some("baseline-active"),
        }
    } else if !config.health.suggest_inline_suppression {
        crate::health_types::HealthActionOptions {
            omit_suppress_line: true,
            omit_reason: Some("config-disabled"),
        }
    } else {
        crate::health_types::HealthActionOptions::default()
    };
    let action_ctx = crate::health_types::HealthActionContext {
        opts: action_opts,
        max_cyclomatic_threshold: max_cyclomatic,
        max_cognitive_threshold: max_cognitive,
        max_crap_threshold: max_crap,
        crap_refactor_band: config.health.crap_refactor_band,
    };

    let grouping = if let Some(ref resolver) = group_resolver {
        Some(grouping::build_health_grouping(
            resolver,
            &config.root,
            &candidate_paths,
            &grouping::HealthGroupingInput {
                files: &files,
                modules: &modules,
                file_paths: &file_paths,
                score_output: score_output.as_ref(),
                file_scores: file_scores_slice,
                findings: &findings,
                hotspots: &hotspots,
                large_functions: &large_functions,
                targets: &targets,
                score_requested: opts.score,
                duplicates_config: opts.score.then_some(&config.duplicates),
                needs_file_scores,
                needs_hotspots: opts.hotspots || opts.targets,
                show_vital_signs: !opts.score_only_output,
                action_ctx: &action_ctx,
            },
        ))
    } else {
        None
    };

    let report = assemble_health_report(
        opts,
        &action_ctx,
        HealthReportAssembly {
            report_coverage_gaps,
            findings,
            files_analyzed,
            total_functions,
            total_above_threshold,
            max_cyclomatic,
            max_cognitive,
            max_crap,
            files_scored,
            average_maintainability,
            vital_signs,
            health_score,
            score_output,
            hotspots,
            hotspot_summary,
            targets,
            target_thresholds,
            health_trend,
            has_istanbul_coverage: istanbul_coverage.is_some(),
            runtime_coverage,
            large_functions,
            sev_critical,
            sev_high,
            sev_moderate,
        },
    );

    let timings = if opts.performance {
        let inner_ms = start.elapsed().as_secs_f64() * 1000.0;
        let total_ms = config_ms + discover_ms + parse_ms + inner_ms;
        Some(HealthTimings {
            config_ms,
            discover_ms,
            parse_ms,
            parse_cpu_ms,
            complexity_ms,
            file_scores_ms,
            git_churn_ms,
            git_churn_cache_hit,
            hotspots_ms,
            duplication_ms,
            targets_ms,
            total_ms,
            shared_parse,
        })
    } else {
        None
    };

    // Report result volume to telemetry from the real result, independent of
    // the exit-code gate. Coverage gaps are a section-level signal today, so
    // they mark an unknown non-empty bucket until that report carries a stable
    // finding count.
    if coverage_gaps_has_findings && report.findings.is_empty() {
        crate::telemetry::note_findings_present(true);
    } else {
        crate::telemetry::note_result_count(report.findings.len());
    }
    crate::telemetry::note_analysis_scale(
        Some(report.summary.files_analyzed),
        Some(report.summary.functions_analyzed),
    );

    Ok(HealthResult {
        report,
        grouping,
        group_resolver,
        config,
        elapsed: start.elapsed(),
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps: enforce_coverage_gaps,
    })
}

fn load_health_coverage(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
) -> Result<Option<scoring::IstanbulCoverage>, ExitCode> {
    if let Some(coverage_path) = opts.coverage {
        return scoring::load_istanbul_coverage(
            coverage_path,
            opts.coverage_root,
            Some(&config.root),
        )
        .map(Some)
        .map_err(|e| {
            emit_error(&format!("coverage: {e}"), 2, opts.output);
            ExitCode::from(2)
        });
    }

    let Some(auto_path) = scoring::auto_detect_coverage(&config.root) else {
        return Ok(None);
    };
    if std::env::var("CI").is_ok_and(|v| !v.is_empty()) {
        eprintln!(
            "note: using auto-detected coverage at {}; pass --coverage explicitly for deterministic CI scores",
            auto_path.display()
        );
    }
    Ok(scoring::load_istanbul_coverage(&auto_path, opts.coverage_root, Some(&config.root)).ok())
}

#[expect(
    deprecated,
    reason = "ADR-008 deprecates fallow_core::analyze_with_parse_result externally; health still uses the workspace path dependency"
)]
fn prepare_shared_analysis_output(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    modules: &[fallow_core::extract::ModuleInfo],
    pre_computed: Option<fallow_core::AnalysisOutput>,
    needed: bool,
) -> Result<Option<fallow_core::AnalysisOutput>, ExitCode> {
    if !needed {
        return Ok(None);
    }
    if let Some(pre) = pre_computed {
        return Ok(Some(pre));
    }
    fallow_core::analyze_with_parse_result(config, modules)
        .map(Some)
        .map_err(|e| emit_error(&format!("analysis failed: {e}"), 2, opts.output))
}

#[expect(
    clippy::too_many_arguments,
    reason = "runtime coverage analysis needs the same filtered health context as scoring"
)]
fn analyze_runtime_coverage(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    modules: &[fallow_core::extract::ModuleInfo],
    shared_analysis_output: Option<&fallow_core::AnalysisOutput>,
    istanbul_coverage: Option<&scoring::IstanbulCoverage>,
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
    ignore_set: &globset::GlobSet,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
) -> Result<Option<crate::health_types::RuntimeCoverageReport>, ExitCode> {
    let Some(ref production_options) = opts.runtime_coverage else {
        return Ok(None);
    };
    let Some(analysis_output) = shared_analysis_output else {
        return Err(emit_error(
            "runtime coverage requires analysis output",
            2,
            opts.output,
        ));
    };
    coverage::analyze(
        production_options,
        &coverage::RuntimeCoverageAnalysisInput {
            root: &config.root,
            modules,
            analysis_output,
            istanbul_coverage,
            file_paths,
            ignore_set,
            changed_files,
            ws_roots,
            top: opts.top,
            codeowners_path: config.codeowners.as_deref(),
            quiet: opts.quiet,
            output: opts.output,
        },
    )
    .map(Some)
}

type FileScoresAndChurn = (FileScoreResult, f64, Option<hotspots::ChurnFetchResult>);

#[expect(
    clippy::too_many_arguments,
    reason = "file-score filtering needs the active health scope and optional precomputed analysis"
)]
fn compute_file_scores_and_churn(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    ignore_set: &globset::GlobSet,
    istanbul_coverage: Option<&scoring::IstanbulCoverage>,
    needs_file_scores: bool,
    precomputed_for_scores: Option<fallow_core::AnalysisOutput>,
) -> Result<FileScoresAndChurn, ExitCode> {
    let needs_churn = opts.hotspots || opts.targets;
    if needs_file_scores && needs_churn {
        return std::thread::scope(|s| {
            let churn_handle = s.spawn(|| hotspots::fetch_churn_data(opts, &config.cache_dir));
            let t = Instant::now();
            let score_result = compute_filtered_file_scores(FileScoreInput {
                config,
                modules,
                file_paths,
                changed_files,
                ws_roots,
                ignore_set,
                output: opts.output,
                istanbul_coverage,
                pre_computed: precomputed_for_scores,
            })?;
            let fs_ms = t.elapsed().as_secs_f64() * 1000.0;
            let churn = churn_handle
                .join()
                .map_err(|_| emit_error("churn thread panicked", 2, opts.output))?;
            Ok((score_result, fs_ms, churn))
        });
    }

    let t = Instant::now();
    let score_result = if needs_file_scores {
        compute_filtered_file_scores(FileScoreInput {
            config,
            modules,
            file_paths,
            changed_files,
            ws_roots,
            ignore_set,
            output: opts.output,
            istanbul_coverage,
            pre_computed: precomputed_for_scores,
        })?
    } else {
        (None, None, None)
    };
    let fs_ms = t.elapsed().as_secs_f64() * 1000.0;
    let churn = if needs_churn {
        hotspots::fetch_churn_data(opts, &config.cache_dir)
    } else {
        None
    };
    Ok((score_result, fs_ms, churn))
}

fn print_slow_churn_note(
    opts: &HealthOptions<'_>,
    churn_fetch: Option<&hotspots::ChurnFetchResult>,
) {
    if let Some(cf) = churn_fetch
        && !cf.cache_hit
        && !opts.no_cache
        && !opts.quiet
        && cf.git_log_ms > 500.0
    {
        eprintln!(
            "{}",
            format!(
                "  note: git churn analysis took {:.1}s (cached for next run at same HEAD)",
                cf.git_log_ms / 1000.0
            )
            .dimmed()
        );
    }
}

fn count_finding_severities(findings: &[ComplexityViolation]) -> (usize, usize, usize) {
    let (mut critical, mut high, mut moderate) = (0usize, 0usize, 0usize);
    for finding in findings {
        match finding.severity {
            FindingSeverity::Critical => critical += 1,
            FindingSeverity::High => high += 1,
            FindingSeverity::Moderate => moderate += 1,
        }
    }
    (critical, high, moderate)
}

fn apply_health_baseline_and_top(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &mut Vec<ComplexityViolation>,
) -> Result<Option<HealthBaselineData>, ExitCode> {
    let loaded_baseline = if let Some(load_path) = opts.baseline {
        Some(load_health_baseline(
            load_path,
            findings,
            &config.root,
            opts.quiet,
            opts.output,
        )?)
    } else {
        None
    };
    if let Some(top) = opts.top {
        findings.truncate(top);
    }
    Ok(loaded_baseline)
}

fn compute_filtered_hotspots(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    file_scores_slice: &[FileHealthScore],
    ignore_set: &globset::GlobSet,
    ws_roots: Option<&[std::path::PathBuf]>,
    churn_fetch: Option<hotspots::ChurnFetchResult>,
    diff_index: Option<&crate::report::ci::diff_filter::DiffIndex>,
) -> (Vec<HotspotEntry>, Option<HotspotSummary>, f64) {
    let t = Instant::now();
    let (mut hotspots, hotspot_summary) = if let Some(churn_data) = churn_fetch {
        compute_hotspots(
            opts,
            config,
            file_scores_slice,
            ignore_set,
            ws_roots,
            churn_data,
        )
    } else {
        (Vec::new(), None)
    };
    if let Some(diff_index) = diff_index {
        filter_hotspots_by_diff(&mut hotspots, diff_index, &config.root);
    }
    (
        hotspots,
        hotspot_summary,
        t.elapsed().as_secs_f64() * 1000.0,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "target filtering coordinates independent health scope inputs"
)]
fn compute_filtered_targets(
    opts: &HealthOptions<'_>,
    score_output: Option<&scoring::FileScoreOutput>,
    file_scores_slice: &[FileHealthScore],
    hotspots: &[HotspotEntry],
    loaded_baseline: Option<&HealthBaselineData>,
    config: &ResolvedConfig,
    diff_index: Option<&crate::report::ci::diff_filter::DiffIndex>,
    dupes_report: Option<&fallow_core::duplicates::DuplicationReport>,
) -> (Vec<RefactoringTarget>, Option<TargetThresholds>, f64) {
    let t = Instant::now();
    let (mut targets, target_thresholds) = compute_targets(
        opts,
        score_output,
        file_scores_slice,
        hotspots,
        loaded_baseline,
        &config.root,
        dupes_report,
    );
    if let Some(diff_index) = diff_index {
        filter_refactoring_targets_by_diff(&mut targets, diff_index, &config.root);
    }
    (
        targets,
        target_thresholds,
        t.elapsed().as_secs_f64() * 1000.0,
    )
}

fn filter_runtime_coverage_report(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    report: Option<&mut crate::health_types::RuntimeCoverageReport>,
    loaded_baseline: Option<&HealthBaselineData>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    diff_index: Option<&crate::report::ci::diff_filter::DiffIndex>,
) {
    if let Some(report) = report {
        let ctx = RuntimeCoverageFilterContext::new(&config.root)
            .with_baseline(loaded_baseline)
            .with_top(opts.top)
            .with_changed_files(changed_files)
            .with_diff_index(diff_index);
        apply_runtime_coverage_filters(report, &ctx);
    }
}

fn save_health_baseline_if_requested(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &[ComplexityViolation],
    runtime_coverage: Option<&crate::health_types::RuntimeCoverageReport>,
    targets: &[RefactoringTarget],
) -> Result<(), ExitCode> {
    if let Some(save_path) = opts.save_baseline {
        save_health_baseline(
            save_path,
            findings,
            runtime_coverage.map_or(&[], |report| report.findings.as_slice()),
            targets,
            &config.root,
            opts.quiet,
            opts.output,
        )?;
    }
    Ok(())
}

fn compute_health_duplication_report(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    files: &[fallow_types::discover::DiscoveredFile],
    candidate_paths: &rustc_hash::FxHashSet<std::path::PathBuf>,
) -> (Option<fallow_core::duplicates::DuplicationReport>, f64) {
    let t = Instant::now();
    let dupes_report = if opts.score || opts.targets {
        let scoped_files = filter_files_to_paths(files, candidate_paths);
        Some(if opts.no_cache {
            fallow_core::duplicates::find_duplicates(
                &config.root,
                &scoped_files,
                &config.duplicates,
            )
        } else {
            fallow_core::duplicates::find_duplicates_cached(
                &config.root,
                &scoped_files,
                &config.duplicates,
                &config.cache_dir,
            )
        })
    } else {
        None
    };
    (dupes_report, t.elapsed().as_secs_f64() * 1000.0)
}

fn compute_health_score_metrics(
    opts: &HealthOptions<'_>,
    dupes_report: Option<&fallow_core::duplicates::DuplicationReport>,
    vital_signs: &mut crate::health_types::VitalSigns,
    counts: &mut crate::health_types::VitalSignsCounts,
    total_files_scoped: usize,
) -> Option<HealthScore> {
    if opts.score
        && let Some(report) = dupes_report
    {
        apply_duplication_metrics(vital_signs, counts, report);
    }
    opts.score
        .then(|| vital_signs::compute_health_score(vital_signs, total_files_scoped))
}

#[expect(
    clippy::too_many_arguments,
    reason = "large-function filtering needs vital signs plus the active health scope"
)]
fn collect_filtered_large_functions(
    vital_signs: &crate::health_types::VitalSigns,
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    diff_index: Option<&crate::report::ci::diff_filter::DiffIndex>,
) -> Vec<crate::health_types::LargeFunctionEntry> {
    let input = LargeFunctionInput {
        vital_signs,
        modules,
        file_paths,
        config_root: &config.root,
        ignore_set,
        changed_files,
        ws_roots,
    };
    let mut large_functions = collect_large_functions(&input);
    if let Some(diff_index) = diff_index {
        filter_large_functions_by_diff(&mut large_functions, diff_index, &config.root);
    }
    large_functions
}

/// Drop complexity findings whose function body span does NOT overlap any
/// added line in the supplied diff. The function spans
/// `[line..=line + line_count - 1]`: a hotspot that starts before the
/// diff but extends into a touched line counts as overlap. `line_count`
/// of zero collapses to `[line..=line]` so older fixture rows without
/// extents do not silently match every diff.
///
/// Paths that cannot be expressed relative to `root` (different drive,
/// path-traversal escape) are RETAINED rather than silently dropped:
/// surfacing an unfilterable path is better than hiding a finding.
fn filter_complexity_findings_by_diff(
    findings: &mut Vec<ComplexityViolation>,
    diff_index: &crate::report::ci::diff_filter::DiffIndex,
    root: &std::path::Path,
) {
    findings.retain(|f| {
        let Some(rel) = relative_to_root(&f.path, root) else {
            return true;
        };
        let start = u64::from(f.line);
        let end = if f.line_count == 0 {
            start
        } else {
            start + u64::from(f.line_count) - 1
        };
        diff_index.range_overlaps_added(&rel, start, end)
    });
}

/// Drop hotspot entries whose file is not touched by the supplied diff.
/// Hotspots are per-file aggregates without a per-line position
/// (`HotspotEntry` has no `line` field), so file-level matching is the
/// only signal the diff carries. Paths outside `root` are RETAINED for
/// the same reason as [`filter_complexity_findings_by_diff`].
fn filter_hotspots_by_diff(
    hotspots: &mut Vec<crate::health_types::HotspotEntry>,
    diff_index: &crate::report::ci::diff_filter::DiffIndex,
    root: &std::path::Path,
) {
    hotspots.retain(|h| match relative_to_root(&h.path, root) {
        Some(rel) => diff_index.touches_file(&rel),
        None => true,
    });
}

/// Drop refactoring targets whose file is not touched by the diff.
/// `RefactoringTarget` is per-file (no line range on the target itself);
/// the line-anchored evidence under `target.evidence.complex_functions`
/// is left intact for downstream renderers because dropping individual
/// evidence rows could turn a multi-function recommendation into a
/// confusing zero-evidence entry.
fn filter_refactoring_targets_by_diff(
    targets: &mut Vec<crate::health_types::RefactoringTarget>,
    diff_index: &crate::report::ci::diff_filter::DiffIndex,
    root: &std::path::Path,
) {
    targets.retain(|t| match relative_to_root(&t.path, root) {
        Some(rel) => diff_index.touches_file(&rel),
        None => true,
    });
}

/// Drop large-function entries whose body span does NOT overlap any added
/// line in the supplied diff. Same range semantics as
/// [`filter_complexity_findings_by_diff`].
fn filter_large_functions_by_diff(
    entries: &mut Vec<crate::health_types::LargeFunctionEntry>,
    diff_index: &crate::report::ci::diff_filter::DiffIndex,
    root: &std::path::Path,
) {
    entries.retain(|e| {
        let Some(rel) = relative_to_root(&e.path, root) else {
            return true;
        };
        let start = u64::from(e.line);
        let end = if e.line_count == 0 {
            start
        } else {
            start + u64::from(e.line_count) - 1
        };
        diff_index.range_overlaps_added(&rel, start, end)
    });
}

fn collect_candidate_paths(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    ignore_set: &globset::GlobSet,
) -> rustc_hash::FxHashSet<std::path::PathBuf> {
    files
        .iter()
        .filter(|file| {
            path_in_health_scope(&file.path, config, changed_files, ws_roots, ignore_set)
        })
        .map(|file| file.path.clone())
        .collect()
}

fn filter_files_to_paths(
    files: &[fallow_types::discover::DiscoveredFile],
    candidate_paths: &rustc_hash::FxHashSet<std::path::PathBuf>,
) -> Vec<fallow_types::discover::DiscoveredFile> {
    files
        .iter()
        .filter(|file| candidate_paths.contains(&file.path))
        .cloned()
        .collect()
}

fn apply_duplication_metrics(
    vital_signs: &mut crate::health_types::VitalSigns,
    counts: &mut crate::health_types::VitalSignsCounts,
    dupes_report: &fallow_core::duplicates::DuplicationReport,
) {
    let pct = dupes_report.stats.duplication_percentage;
    vital_signs.duplication_pct = Some((pct * 10.0).round() / 10.0);
    counts.duplicated_lines = Some(dupes_report.stats.duplicated_lines);
    if let Some(ref mut vc) = vital_signs.counts {
        vc.duplicated_lines = Some(dupes_report.stats.duplicated_lines);
    }
}

/// Sort findings by the specified criteria.
fn sort_findings(findings: &mut [ComplexityViolation], sort: &SortBy) {
    match sort {
        SortBy::Severity => findings.sort_by_key(|f| {
            std::cmp::Reverse((
                exceeded_priority(f.exceeded),
                severity_priority(f.severity),
                f.crap.is_some(),
                f.cyclomatic,
                f.cognitive,
                f.line_count,
            ))
        }),
        SortBy::Cyclomatic => findings.sort_by_key(|f| std::cmp::Reverse(f.cyclomatic)),
        SortBy::Cognitive => findings.sort_by_key(|f| std::cmp::Reverse(f.cognitive)),
        SortBy::Lines => findings.sort_by_key(|f| std::cmp::Reverse(f.line_count)),
    }
}

const fn exceeded_priority(exceeded: ExceededThreshold) -> u8 {
    match exceeded {
        ExceededThreshold::All => 5,
        ExceededThreshold::CyclomaticCrap | ExceededThreshold::CognitiveCrap => 4,
        ExceededThreshold::Crap => 3,
        ExceededThreshold::Both => 2,
        ExceededThreshold::Cyclomatic | ExceededThreshold::Cognitive => 1,
    }
}

const fn severity_priority(severity: FindingSeverity) -> u8 {
    match severity {
        FindingSeverity::Critical => 3,
        FindingSeverity::High => 2,
        FindingSeverity::Moderate => 1,
    }
}

/// `(score_output, files_scored, average_maintainability)`.
type FileScoreResult = (Option<scoring::FileScoreOutput>, Option<usize>, Option<f64>);

/// Compute file scores, applying workspace and ignore filters.
struct FileScoreInput<'a> {
    config: &'a ResolvedConfig,
    modules: &'a [fallow_core::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<fallow_core::discover::FileId, &'a std::path::PathBuf>,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    ignore_set: &'a globset::GlobSet,
    output: OutputFormat,
    istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    pre_computed: Option<fallow_core::AnalysisOutput>,
}

fn compute_filtered_file_scores(input: FileScoreInput<'_>) -> Result<FileScoreResult, ExitCode> {
    #[expect(
        deprecated,
        reason = "ADR-008 deprecates fallow_core::analyze_with_parse_result externally; health still uses the workspace path dependency"
    )]
    let analysis_output = if let Some(pre) = input.pre_computed {
        pre
    } else {
        fallow_core::analyze_with_parse_result(input.config, input.modules)
            .map_err(|e| emit_error(&format!("analysis failed: {e}"), 2, input.output))?
    };
    match compute_file_scores(
        input.modules,
        input.file_paths,
        input.changed_files,
        analysis_output,
        input.istanbul_coverage,
        &input.config.root,
    ) {
        Ok(mut output) => {
            if let Some(ws) = input.ws_roots {
                output
                    .scores
                    .retain(|s| ws.iter().any(|r| s.path.starts_with(r)));
            }
            if !input.ignore_set.is_empty() {
                output.scores.retain(|s| {
                    let relative = s.path.strip_prefix(&input.config.root).unwrap_or(&s.path);
                    !input.ignore_set.is_match(relative)
                });
            }
            filter_coverage_gaps(
                &mut output.coverage.report,
                &mut output.coverage.runtime_paths,
                input.config,
                input.changed_files,
                input.ws_roots,
                input.ignore_set,
            );
            let total_scored = output.scores.len();
            let avg = if total_scored > 0 {
                let sum: f64 = output.scores.iter().map(|s| s.maintainability_index).sum();
                Some((sum / total_scored as f64 * 10.0).round() / 10.0)
            } else {
                None
            };
            Ok((Some(output), Some(total_scored), avg))
        }
        Err(e) => {
            eprintln!("Warning: failed to compute file scores: {e}");
            Ok((None, Some(0), None))
        }
    }
}

/// Compute refactoring targets when requested, applying baseline and top filters.
fn compute_targets(
    opts: &HealthOptions<'_>,
    score_output: Option<&scoring::FileScoreOutput>,
    file_scores_slice: &[FileHealthScore],
    hotspots: &[HotspotEntry],
    loaded_baseline: Option<&HealthBaselineData>,
    config_root: &std::path::Path,
    dupes_report: Option<&fallow_core::duplicates::DuplicationReport>,
) -> (Vec<RefactoringTarget>, Option<TargetThresholds>) {
    if !opts.targets {
        return (Vec::new(), None);
    }
    let Some(output) = score_output else {
        return (Vec::new(), None);
    };
    let clone_siblings = dupes_report.map_or_else(rustc_hash::FxHashMap::default, |report| {
        targets::build_clone_sibling_evidence(report)
    });
    let target_aux = TargetAuxData::from_output(output, &clone_siblings);
    let (mut tgts, thresholds) =
        compute_refactoring_targets(file_scores_slice, &target_aux, hotspots);
    if let Some(baseline) = loaded_baseline {
        tgts = filter_new_health_targets(tgts, baseline, config_root);
    }
    if let Some(ref effort) = opts.effort {
        tgts.retain(|t| t.effort == *effort);
    }
    if let Some(top) = opts.top {
        tgts.truncate(top);
    }
    (tgts, Some(thresholds))
}

fn path_in_health_scope(
    path: &std::path::Path,
    config: &ResolvedConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    ignore_set: &globset::GlobSet,
) -> bool {
    if let Some(changed) = changed_files
        && !changed.contains(path)
    {
        return false;
    }
    if let Some(ws) = ws_roots
        && !ws.iter().any(|r| path.starts_with(r))
    {
        return false;
    }
    if !ignore_set.is_empty() {
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            return false;
        }
    }
    true
}

fn filter_coverage_gaps(
    coverage_gaps: &mut CoverageGaps,
    runtime_paths: &mut Vec<std::path::PathBuf>,
    config: &ResolvedConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    ignore_set: &globset::GlobSet,
) {
    runtime_paths
        .retain(|path| path_in_health_scope(path, config, changed_files, ws_roots, ignore_set));
    coverage_gaps.files.retain(|item| {
        path_in_health_scope(&item.file.path, config, changed_files, ws_roots, ignore_set)
    });
    coverage_gaps.exports.retain(|item| {
        path_in_health_scope(
            &item.export.path,
            config,
            changed_files,
            ws_roots,
            ignore_set,
        )
    });

    runtime_paths.sort();
    runtime_paths.dedup();

    let runtime_files = runtime_paths.len();
    let untested_files = coverage_gaps.files.len();
    let covered_files = runtime_files.saturating_sub(untested_files);
    coverage_gaps.summary = scoring::build_coverage_summary(
        runtime_files,
        covered_files,
        untested_files,
        coverage_gaps.exports.len(),
    );
}

/// Subset selector used when scoping `vital_signs`, `health_score`, and
/// `analysis_counts` to a workspace package or a `--group-by` bucket.
///
/// `Full` skips filtering entirely (project-wide). `Paths` matches files whose
/// absolute path is in the given set (exact match), which is what scoped
/// project runs and `--group-by` use to keep every score input on the same
/// filtered file set.
pub enum SubsetFilter<'a> {
    Full,
    Paths(&'a rustc_hash::FxHashSet<std::path::PathBuf>),
}

impl SubsetFilter<'_> {
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }
    pub fn matches(&self, path: &std::path::Path) -> bool {
        match self {
            Self::Full => true,
            Self::Paths(set) => set.contains(path),
        }
    }
}

/// Build vital signs and counts for the slice of files selected by `subset`.
///
/// When `subset` is anything other than `SubsetFilter::Full`, per-module
/// aggregates (cyclomatic distribution, total LOC, unit profiles) are
/// restricted to modules in the subset, the analysis counts (`dead_files`,
/// `dead_exports`, `unused_deps`, `circular_deps`, `total_exports`) are
/// recomputed from the snapshot for the same subset, and `total_files` should
/// already reflect the subset-scoped count.
struct VitalSignsAndCountsInput<'a> {
    score_output: Option<&'a scoring::FileScoreOutput>,
    modules: &'a [fallow_core::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<fallow_core::discover::FileId, &'a std::path::PathBuf>,
    needs_file_scores: bool,
    file_scores_slice: &'a [FileHealthScore],
    needs_hotspots: bool,
    hotspots: &'a [HotspotEntry],
    total_files: usize,
    subset: &'a SubsetFilter<'a>,
}

fn compute_vital_signs_and_counts(
    input: &VitalSignsAndCountsInput<'_>,
) -> (
    crate::health_types::VitalSigns,
    crate::health_types::VitalSignsCounts,
) {
    let analysis_counts = input.score_output.map(|o| {
        o.analysis_snapshot
            .counts_for(input.subset, &o.analysis_counts)
    });
    let module_filter_set: Option<rustc_hash::FxHashSet<fallow_core::discover::FileId>> =
        if input.subset.is_full() {
            None
        } else {
            Some(
                input
                    .modules
                    .iter()
                    .filter_map(|m| {
                        let path = input.file_paths.get(&m.file_id)?;
                        if input.subset.matches(path) {
                            Some(m.file_id)
                        } else {
                            None
                        }
                    })
                    .collect(),
            )
        };
    let vs_input = vital_signs::VitalSignsInput {
        modules: input.modules,
        module_filter: module_filter_set.as_ref(),
        file_scores: if input.needs_file_scores {
            Some(input.file_scores_slice)
        } else {
            None
        },
        hotspots: if input.needs_hotspots {
            Some(input.hotspots)
        } else {
            None
        },
        total_files: input.total_files,
        analysis_counts,
    };
    let signs = vital_signs::compute_vital_signs(&vs_input);
    let counts = vital_signs::build_counts(&vs_input);
    (signs, counts)
}

/// Save a vital signs snapshot to disk if requested.
struct SnapshotInput<'a> {
    opts: &'a HealthOptions<'a>,
    snapshot_path: &'a std::path::Path,
    vital_signs: &'a crate::health_types::VitalSigns,
    counts: &'a crate::health_types::VitalSignsCounts,
    hotspot_summary: Option<&'a crate::health_types::HotspotSummary>,
    health_score: Option<&'a crate::health_types::HealthScore>,
    coverage_model: Option<crate::health_types::CoverageModel>,
}

fn save_snapshot(input: SnapshotInput<'_>) -> Result<(), ExitCode> {
    let shallow = input.hotspot_summary.is_some_and(|s| s.shallow_clone);
    let snapshot = vital_signs::build_snapshot(
        input.vital_signs.clone(),
        input.counts.clone(),
        input.opts.root,
        shallow,
        input.health_score,
        input.coverage_model,
    );
    let explicit = if input.snapshot_path.as_os_str().is_empty() {
        None
    } else {
        Some(input.snapshot_path)
    };
    match vital_signs::save_snapshot(&snapshot, input.opts.root, explicit) {
        Ok(saved_path) => {
            if !input.opts.quiet {
                eprintln!("Saved vital signs snapshot to {}", saved_path.display());
            }
            Ok(())
        }
        Err(e) => Err(emit_error(&e, 2, input.opts.output)),
    }
}

/// Compute health trend from historical snapshots if requested.
fn compute_health_trend(
    opts: &HealthOptions<'_>,
    vital_signs: &crate::health_types::VitalSigns,
    counts: &crate::health_types::VitalSignsCounts,
    health_score: Option<&crate::health_types::HealthScore>,
) -> Option<crate::health_types::HealthTrend> {
    if !opts.trend {
        return None;
    }
    if opts.changed_since.is_some() && !opts.quiet {
        eprintln!(
            "warning: --trend comparison may be inaccurate with --changed-since; \
             snapshots are typically from full-project runs"
        );
    }
    let snapshots = vital_signs::load_snapshots(opts.root);
    if snapshots.is_empty() && !opts.quiet {
        eprintln!(
            "No snapshots found. Run `fallow health --save-snapshot` to save a \
             baseline, then use --trend on subsequent runs to track progress."
        );
    }
    vital_signs::compute_trend(
        vital_signs,
        counts,
        health_score.map(|s| s.score),
        &snapshots,
    )
}

struct HealthReportAssembly {
    report_coverage_gaps: bool,
    findings: Vec<ComplexityViolation>,
    files_analyzed: usize,
    total_functions: usize,
    total_above_threshold: usize,
    max_cyclomatic: u16,
    max_cognitive: u16,
    max_crap: f64,
    files_scored: Option<usize>,
    average_maintainability: Option<f64>,
    vital_signs: crate::health_types::VitalSigns,
    health_score: Option<crate::health_types::HealthScore>,
    score_output: Option<scoring::FileScoreOutput>,
    hotspots: Vec<HotspotEntry>,
    hotspot_summary: Option<crate::health_types::HotspotSummary>,
    targets: Vec<RefactoringTarget>,
    target_thresholds: Option<TargetThresholds>,
    health_trend: Option<crate::health_types::HealthTrend>,
    has_istanbul_coverage: bool,
    runtime_coverage: Option<crate::health_types::RuntimeCoverageReport>,
    large_functions: Vec<LargeFunctionEntry>,
    sev_critical: usize,
    sev_high: usize,
    sev_moderate: usize,
}

/// Collect functions exceeding 60 LOC when the unit size risk profile warrants it.
///
/// Only populated when `very_high_risk >= 3%` in the unit size profile (same threshold
/// that triggers showing the risk profile line). Sorted by line count descending.
struct LargeFunctionInput<'a> {
    vital_signs: &'a crate::health_types::VitalSigns,
    modules: &'a [fallow_core::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<fallow_core::discover::FileId, &'a std::path::PathBuf>,
    config_root: &'a std::path::Path,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
}

fn collect_large_functions(input: &LargeFunctionInput<'_>) -> Vec<LargeFunctionEntry> {
    let dominated = input
        .vital_signs
        .unit_size_profile
        .as_ref()
        .is_some_and(|p| p.very_high_risk >= 3.0);
    if !dominated {
        return Vec::new();
    }

    let mut entries = Vec::new();
    for module in input.modules {
        let Some(&path) = input.file_paths.get(&module.file_id) else {
            continue;
        };
        let relative = path.strip_prefix(input.config_root).unwrap_or(path);
        if input.ignore_set.is_match(relative) {
            continue;
        }
        if let Some(changed) = input.changed_files
            && !changed.contains(path.as_path())
        {
            continue;
        }
        if let Some(ws) = input.ws_roots
            && !ws.iter().any(|r| path.starts_with(r))
        {
            continue;
        }
        for func in &module.complexity {
            if func.line_count > 60 {
                entries.push(LargeFunctionEntry {
                    path: path.clone(),
                    name: func.name.clone(),
                    line: func.line,
                    line_count: func.line_count,
                });
            }
        }
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.line_count));
    entries
}

/// Build a glob set from health ignore patterns.
///
/// User patterns were validated at config load time
/// (see `FallowConfig::validate_user_globs`).
#[expect(
    clippy::expect_used,
    reason = "health ignore globs are validated before health analysis"
)]
fn build_ignore_set(patterns: &[String]) -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            globset::Glob::new(pattern)
                .expect("health.ignore pattern was validated at config load time"),
        );
    }
    builder
        .build()
        .unwrap_or_else(|_| globset::GlobSet::empty())
}

/// Collect health findings from parsed modules, applying ignore, changed-since,
/// and workspace filters. The returned `files_analyzed` / `total_functions`
/// counters reflect only modules that pass every filter so the rendered
/// summary matches the produced findings.
#[expect(
    clippy::too_many_arguments,
    reason = "filter pipeline mirrors compute_filtered_file_scores"
)]
fn collect_findings(
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
    config_root: &std::path::Path,
    ignore_set: &globset::GlobSet,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    max_cyclomatic: u16,
    max_cognitive: u16,
    complexity_breakdown: bool,
) -> (Vec<ComplexityViolation>, usize, usize) {
    let mut files_analyzed = 0usize;
    let mut total_functions = 0usize;
    let mut findings: Vec<ComplexityViolation> = Vec::new();

    for module in modules {
        let Some(&path) = file_paths.get(&module.file_id) else {
            continue;
        };

        let relative = path.strip_prefix(config_root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }

        if let Some(changed) = changed_files
            && !changed.contains(path)
        {
            continue;
        }

        if let Some(ws) = ws_roots
            && !ws.iter().any(|r| path.starts_with(r))
        {
            continue;
        }

        files_analyzed += 1;
        for fc in &module.complexity {
            total_functions += 1;
            if fallow_core::suppress::is_suppressed(
                &module.suppressions,
                fc.line,
                fallow_core::suppress::IssueKind::Complexity,
            ) {
                continue;
            }
            let exceeds_cyclomatic = fc.cyclomatic > max_cyclomatic;
            let exceeds_cognitive = fc.cognitive > max_cognitive;
            if exceeds_cyclomatic || exceeds_cognitive {
                findings.push(ComplexityViolation {
                    path: path.clone(),
                    name: fc.name.clone(),
                    line: fc.line,
                    col: fc.col,
                    cyclomatic: fc.cyclomatic,
                    cognitive: fc.cognitive,
                    line_count: fc.line_count,
                    param_count: fc.param_count,
                    exceeded: ExceededThreshold::from_bools(
                        exceeds_cyclomatic,
                        exceeds_cognitive,
                        false,
                    ),
                    severity: compute_finding_severity(
                        fc.cognitive,
                        fc.cyclomatic,
                        None,
                        DEFAULT_COGNITIVE_HIGH,
                        DEFAULT_COGNITIVE_CRITICAL,
                        DEFAULT_CYCLOMATIC_HIGH,
                        DEFAULT_CYCLOMATIC_CRITICAL,
                    ),
                    crap: None,
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: contributions_for(complexity_breakdown, fc),
                });
            }
        }
    }

    (findings, files_analyzed, total_functions)
}

/// Clone the per-decision-point breakdown onto a finding only when the caller
/// opted in via `health --complexity-breakdown`; otherwise leave it empty so it
/// is omitted from JSON.
fn contributions_for(
    complexity_breakdown: bool,
    fc: &fallow_types::extract::FunctionComplexity,
) -> Vec<fallow_types::extract::ComplexityContribution> {
    if complexity_breakdown {
        fc.contributions.clone()
    } else {
        Vec::new()
    }
}

/// Merge per-function CRAP data into an existing complexity findings vector.
///
/// Functions that only exceed `--max-crap` (without exceeding cyclomatic or
/// cognitive) become new findings. Functions that already produced a finding
/// for cyclomatic/cognitive get their `crap` and `coverage_pct` fields
/// populated, and the `exceeded` discriminant plus `severity` are recomputed
/// to reflect CRAP's contribution.
struct CrapFindingMergeInput<'a> {
    modules: &'a [fallow_core::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<fallow_core::discover::FileId, &'a std::path::PathBuf>,
    config_root: &'a std::path::Path,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    per_function_crap: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<scoring::PerFunctionCrap>>,
    template_inherit_provenance: &'a rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>,
    max_crap: f64,
    max_cyclomatic: u16,
    max_cognitive: u16,
    complexity_breakdown: bool,
}

fn merge_crap_findings(findings: &mut Vec<ComplexityViolation>, input: &CrapFindingMergeInput<'_>) {
    let finding_index: rustc_hash::FxHashMap<(std::path::PathBuf, u32, u32), usize> = findings
        .iter()
        .enumerate()
        .map(|(idx, f)| ((f.path.clone(), f.line, f.col), idx))
        .collect();
    let mut complexity_by_pos: rustc_hash::FxHashMap<
        &std::path::Path,
        rustc_hash::FxHashMap<(u32, u32), &fallow_types::extract::FunctionComplexity>,
    > = rustc_hash::FxHashMap::default();
    for module in input.modules {
        let Some(&path) = input.file_paths.get(&module.file_id) else {
            continue;
        };
        let entry = complexity_by_pos.entry(path.as_path()).or_default();
        for fc in &module.complexity {
            entry.insert((fc.line, fc.col), fc);
        }
    }
    let suppressions_by_path: rustc_hash::FxHashMap<&std::path::Path, _> = input
        .modules
        .iter()
        .filter_map(|m| {
            input
                .file_paths
                .get(&m.file_id)
                .map(|p| (p.as_path(), &m.suppressions))
        })
        .collect();

    let mut new_findings: Vec<ComplexityViolation> = Vec::new();
    for (path, per_fn) in input.per_function_crap {
        let relative = path.strip_prefix(input.config_root).unwrap_or(path);
        if input.ignore_set.is_match(relative) {
            continue;
        }
        if let Some(changed) = input.changed_files
            && !changed.contains(path)
        {
            continue;
        }
        if let Some(ws) = input.ws_roots
            && !ws.iter().any(|r| path.starts_with(r))
        {
            continue;
        }

        for pf in per_fn {
            if pf.crap < input.max_crap {
                continue;
            }
            if let Some(sups) = suppressions_by_path.get(path.as_path())
                && fallow_core::suppress::is_suppressed(
                    sups,
                    pf.line,
                    fallow_core::suppress::IssueKind::Complexity,
                )
            {
                continue;
            }

            if let Some(&idx) = finding_index.get(&(path.clone(), pf.line, pf.col)) {
                let finding = &mut findings[idx];
                finding.crap = Some(pf.crap);
                finding.coverage_pct = pf.coverage_pct;
                finding.coverage_tier = Some(pf.coverage_tier);
                finding.coverage_source = Some(pf.coverage_source);
                finding.inherited_from = inherited_from_for(
                    pf.coverage_source,
                    path.as_path(),
                    input.template_inherit_provenance,
                );
                let exceeds_cyclomatic = finding.exceeded.includes_cyclomatic();
                let exceeds_cognitive = finding.exceeded.includes_cognitive();
                finding.exceeded =
                    ExceededThreshold::from_bools(exceeds_cyclomatic, exceeds_cognitive, true);
                finding.severity = compute_finding_severity(
                    finding.cognitive,
                    finding.cyclomatic,
                    Some(pf.crap),
                    DEFAULT_COGNITIVE_HIGH,
                    DEFAULT_COGNITIVE_CRITICAL,
                    DEFAULT_CYCLOMATIC_HIGH,
                    DEFAULT_CYCLOMATIC_CRITICAL,
                );
            } else {
                let Some(fc) = complexity_by_pos
                    .get(path.as_path())
                    .and_then(|m| m.get(&(pf.line, pf.col)).copied())
                else {
                    continue;
                };
                let exceeds_cyclomatic = fc.cyclomatic > input.max_cyclomatic;
                let exceeds_cognitive = fc.cognitive > input.max_cognitive;
                new_findings.push(ComplexityViolation {
                    path: path.clone(),
                    name: fc.name.clone(),
                    line: fc.line,
                    col: fc.col,
                    cyclomatic: fc.cyclomatic,
                    cognitive: fc.cognitive,
                    line_count: fc.line_count,
                    param_count: fc.param_count,
                    exceeded: ExceededThreshold::from_bools(
                        exceeds_cyclomatic,
                        exceeds_cognitive,
                        true,
                    ),
                    severity: compute_finding_severity(
                        fc.cognitive,
                        fc.cyclomatic,
                        Some(pf.crap),
                        DEFAULT_COGNITIVE_HIGH,
                        DEFAULT_COGNITIVE_CRITICAL,
                        DEFAULT_CYCLOMATIC_HIGH,
                        DEFAULT_CYCLOMATIC_CRITICAL,
                    ),
                    crap: Some(pf.crap),
                    coverage_pct: pf.coverage_pct,
                    coverage_tier: Some(pf.coverage_tier),
                    coverage_source: Some(pf.coverage_source),
                    inherited_from: inherited_from_for(
                        pf.coverage_source,
                        path.as_path(),
                        input.template_inherit_provenance,
                    ),
                    component_rollup: None,
                    contributions: contributions_for(input.complexity_breakdown, fc),
                });
            }
        }
    }
    findings.extend(new_findings);
}

/// Synthesise per-Angular-component rollup findings.
///
/// For each Angular component that has both at least one class-function
/// finding above threshold AND a synthetic `<template>` finding, emit a new
/// `<component>` `ComplexityViolation` whose `cyclomatic` / `cognitive` totals are
/// `max(class) + template`. The rollup is anchored at the worst class
/// function's `(path, line, col)` so an existing
/// `// fallow-ignore-next-line complexity` placed above that function (or
/// the `@Component` decorator on inline-template components) continues to
/// hide both the per-function finding AND the rollup. Per-function and
/// per-`<template>` findings are NOT removed; the rollup is strictly
/// additive, with [`ComponentRollup`] carrying the breakdown.
///
/// Component-owner resolution has two branches:
/// - `<template>` finding on an `.html` file: look up the owner `.ts` in
///   the inverse-`templateUrl` provenance map populated by
///   `scoring::build_template_inherit_contexts` (the same walker that drives
///   `coverage_source: "estimated_component_inherited"` for CRAP).
/// - `<template>` finding on a `.ts` / `.tsx` / `.mts` / `.cts` file
///   (inline `@Component({ template: \`...\` })` literals): the owner IS
///   the file (`template_complexity.rs` remaps inline-template line/col
///   onto the decorator on the same `.ts`).
///
/// "Class function" is approximated as any finding whose name contains a
/// `.` (the `ClassName.methodName` shape `complexity.rs` emits for class
/// methods). Free functions and anonymous arrows do not participate in
/// rollups; only methods owned by a class do.
///
/// A `.ts` file carrying TWO synthetic `<template>` findings is treated
/// defensively: rollups are skipped (a `.ts` with multiple `@Component`
/// decorators would need AST-level class attribution to map each template
/// to its owning class, which is out of scope for the first cut). Fallow
/// emits a single rollup per owner per pass.
///
/// `[`ComponentRollup`]`: crate::health_types::ComponentRollup
fn append_component_rollup_findings(
    findings: &mut Vec<crate::health_types::ComplexityViolation>,
    template_owner_lookup: Option<&rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>>,
    max_cyclomatic: u16,
    max_cognitive: u16,
) {
    use crate::health_types::{ComplexityViolation, ComponentRollup, ExceededThreshold};

    let mut by_owner: rustc_hash::FxHashMap<std::path::PathBuf, (Vec<usize>, Vec<usize>)> =
        rustc_hash::FxHashMap::default();
    for (idx, f) in findings.iter().enumerate() {
        if f.name == "<template>" {
            let ext = f
                .path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase);
            let owner = match ext.as_deref() {
                Some("html") => template_owner_lookup.and_then(|m| m.get(&f.path)).cloned(),
                Some("ts" | "tsx" | "mts" | "cts") => Some(f.path.clone()),
                _ => None,
            };
            if let Some(owner) = owner {
                by_owner.entry(owner).or_default().1.push(idx);
            }
        } else if f.name != "<component>" {
            let is_ts = f
                .path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "ts" | "tsx" | "mts" | "cts"
                    )
                });
            if is_ts {
                by_owner.entry(f.path.clone()).or_default().0.push(idx);
            }
        }
    }

    let mut to_push: Vec<ComplexityViolation> = Vec::new();
    for (owner, (class_idxs, template_idxs)) in by_owner {
        if class_idxs.is_empty() || template_idxs.is_empty() {
            continue;
        }
        if template_idxs.len() > 1 {
            continue;
        }
        let template = &findings[template_idxs[0]];
        let Some(worst_idx) = class_idxs
            .iter()
            .copied()
            .max_by_key(|&i| findings[i].cyclomatic)
        else {
            continue;
        };
        let worst = &findings[worst_idx];
        let component = owner.file_stem().map_or_else(
            || "<unknown-component>".to_string(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        let worst_method = worst.name.clone();
        let rollup_cyc = worst.cyclomatic.saturating_add(template.cyclomatic);
        let rollup_cog = worst.cognitive.saturating_add(template.cognitive);
        let exceeds_cyclomatic = rollup_cyc > max_cyclomatic;
        let exceeds_cognitive = rollup_cog > max_cognitive;
        if !exceeds_cyclomatic && !exceeds_cognitive {
            continue;
        }
        let severity = compute_finding_severity(
            rollup_cog,
            rollup_cyc,
            None,
            DEFAULT_COGNITIVE_HIGH,
            DEFAULT_COGNITIVE_CRITICAL,
            DEFAULT_CYCLOMATIC_HIGH,
            DEFAULT_CYCLOMATIC_CRITICAL,
        );
        to_push.push(ComplexityViolation {
            path: owner,
            name: "<component>".to_string(),
            line: worst.line,
            col: worst.col,
            cyclomatic: rollup_cyc,
            cognitive: rollup_cog,
            line_count: worst.line_count.saturating_add(template.line_count),
            param_count: 0,
            exceeded: ExceededThreshold::from_bools(exceeds_cyclomatic, exceeds_cognitive, false),
            severity,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: Some(ComponentRollup {
                component,
                class_worst_function: worst_method,
                class_cyclomatic: worst.cyclomatic,
                class_cognitive: worst.cognitive,
                template_path: template.path.clone(),
                template_cyclomatic: template.cyclomatic,
                template_cognitive: template.cognitive,
            }),
            contributions: Vec::new(),
        });
    }
    findings.extend(to_push);
}

/// Resolve the `inherited_from` provenance path for a CRAP finding.
///
/// Returns `Some(owner_path)` only for the
/// `CoverageSource::EstimatedComponentInherited` variant, so the field stays
/// absent on every Istanbul / regular-estimated row. Pairs with the
/// `coverage_source` discriminator: any finding carrying
/// `estimated_component_inherited` also carries `inherited_from`, and vice
/// versa.
fn inherited_from_for(
    source: crate::health_types::CoverageSource,
    template_path: &std::path::Path,
    template_inherit_provenance: &rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    if matches!(
        source,
        crate::health_types::CoverageSource::EstimatedComponentInherited
    ) {
        template_inherit_provenance.get(template_path).cloned()
    } else {
        None
    }
}

/// Save health baseline to disk.
fn save_health_baseline(
    save_path: &std::path::Path,
    findings: &[ComplexityViolation],
    runtime_coverage_findings: &[crate::health_types::RuntimeCoverageFinding],
    targets: &[RefactoringTarget],
    config_root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
) -> Result<(), ExitCode> {
    let baseline = HealthBaselineData::from_findings(
        findings,
        runtime_coverage_findings,
        targets,
        config_root,
    );
    match serde_json::to_string_pretty(&baseline) {
        Ok(json) => {
            if let Some(parent) = save_path.parent()
                && !parent.as_os_str().is_empty()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Err(emit_error(
                    &format!("failed to create health baseline directory: {e}"),
                    2,
                    output,
                ));
            }
            if let Err(e) = std::fs::write(save_path, json) {
                return Err(emit_error(
                    &format!("failed to save health baseline: {e}"),
                    2,
                    output,
                ));
            }
            if !quiet {
                eprintln!("Saved health baseline to {}", save_path.display());
            }
            Ok(())
        }
        Err(e) => Err(emit_error(
            &format!("failed to serialize health baseline: {e}"),
            2,
            output,
        )),
    }
}

/// Load and apply a health baseline, filtering findings to show only new ones.
fn load_health_baseline(
    baseline_path: &std::path::Path,
    findings: &mut Vec<ComplexityViolation>,
    root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
) -> Result<HealthBaselineData, ExitCode> {
    let json = std::fs::read_to_string(baseline_path)
        .map_err(|e| emit_error(&format!("failed to read health baseline: {e}"), 2, output))?;
    let baseline: HealthBaselineData = serde_json::from_str(&json)
        .map_err(|e| emit_error(&format!("failed to parse health baseline: {e}"), 2, output))?;
    let baseline_entries = baseline.finding_entry_count();
    let before = findings.len();
    let overlap_entries = baseline.overlap_entry_count(findings, root);
    *findings = filter_new_health_findings(std::mem::take(findings), &baseline, root);
    if !quiet {
        eprintln!(
            "Comparing against health baseline: {}",
            baseline_path.display()
        );
    }
    if baseline_entries > 0 && before > 0 && overlap_entries == 0 && !quiet {
        eprintln!(
            "Warning: health baseline has {baseline_entries} entries but matched \
             0 current findings. Your paths may have changed, or the baseline \
             was saved on a different machine. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        );
    }
    Ok(baseline)
}

/// Run health analysis, print results, and return exit code.
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
            min_score: opts.min_score,
            min_severity: opts.min_severity,
            report_only: opts.report_only,
            summary: opts.summary,
            summary_heading: true,
            show_explain_tip: true,
            skip_score_and_trend: false,
        },
    )
}

/// Result of executing health analysis without printing.
pub struct HealthResult {
    pub report: HealthReport,
    /// Per-group health output when `--group-by` is active.
    ///
    /// `None` for the default run; `Some` for any
    /// `--group-by package|owner|directory|section` invocation. The top-level
    /// `report` reflects the active run scope (for example, after
    /// `--workspace`); consumers that want per-group metrics read from
    /// `grouping.groups`.
    pub grouping: Option<crate::health_types::HealthGrouping>,
    /// Resolver retained alongside `grouping` so per-result formats
    /// (SARIF, CodeClimate) can tag findings with the active group key
    /// without re-discovering CODEOWNERS or workspace info at print time.
    /// Always `None` when `grouping` is `None`.
    pub group_resolver: Option<crate::report::OwnershipResolver>,
    pub config: ResolvedConfig,
    pub elapsed: Duration,
    pub timings: Option<HealthTimings>,
    pub coverage_gaps_has_findings: bool,
    pub should_fail_on_coverage_gaps: bool,
}

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
    pub min_score: Option<f64>,
    pub min_severity: Option<FindingSeverity>,
    pub report_only: bool,
    pub summary: bool,
    pub summary_heading: bool,
    pub show_explain_tip: bool,
    pub skip_score_and_trend: bool,
}

pub fn print_health_result(result: &HealthResult, options: HealthPrintOptions) -> ExitCode {
    let ctx = report::ReportContext {
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
    };
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

    if options.report_only {
        return ExitCode::SUCCESS;
    }

    let mut score_gate_failed = false;
    if let Some(threshold) = options.min_score
        && let Some(ref hs) = result.report.health_score
        && hs.score < threshold
    {
        score_gate_failed = true;
        if !options.quiet {
            eprintln!(
                "Health score {:.1} ({}) is below minimum threshold {:.0}",
                hs.score, hs.grade, threshold
            );
        }
    }

    let findings_gate_failed = if let Some(min_sev) = options.min_severity {
        result.report.findings.iter().any(|f| f.severity >= min_sev)
    } else if options.min_score.is_none() {
        !result.report.findings.is_empty()
    } else {
        false
    };
    let has_failing_runtime_coverage =
        result
            .report
            .runtime_coverage
            .as_ref()
            .is_some_and(|report| {
                report.findings.iter().any(|finding| {
                    matches!(
                        finding.verdict,
                        crate::health_types::RuntimeCoverageVerdict::SafeToDelete
                            | crate::health_types::RuntimeCoverageVerdict::ReviewRequired
                            | crate::health_types::RuntimeCoverageVerdict::LowTraffic
                    )
                })
            });
    if score_gate_failed || findings_gate_failed || has_failing_runtime_coverage {
        return ExitCode::from(1);
    }

    if result.should_fail_on_coverage_gaps && result.coverage_gaps_has_findings {
        return ExitCode::from(1);
    }

    if options.min_score.is_some()
        && options.min_severity.is_none()
        && !options.quiet
        && !result.report.findings.is_empty()
        && matches!(result.config.output, OutputFormat::Human)
    {
        eprintln!(
            "{}",
            "Findings above are informational: --min-score gates on the score, not on findings."
                .dimmed()
        );
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::{FallowConfig, OutputFormat};
    use fallow_core::extract::ModuleInfo;
    use fallow_types::discover::FileId;
    use fallow_types::extract::FunctionComplexity;
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::path::{Path, PathBuf};

    /// Build a minimal `ModuleInfo` with only the fields `collect_findings` needs.
    fn make_module(file_id: FileId, complexity: Vec<FunctionComplexity>) -> ModuleInfo {
        ModuleInfo {
            file_id,
            exports: vec![],
            imports: vec![],
            re_exports: vec![],
            dynamic_imports: vec![],
            dynamic_import_patterns: vec![],
            require_calls: vec![],
            package_path_references: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: vec![],
            unknown_suppression_kinds: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            line_offsets: vec![0],
            complexity,
            flag_uses: vec![],
            class_heritage: vec![],
            injection_tokens: vec![],
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
        }
    }

    fn make_fc(name: &str, cyclomatic: u16, cognitive: u16, line_count: u32) -> FunctionComplexity {
        FunctionComplexity {
            name: name.to_string(),
            line: 1,
            col: 0,
            cyclomatic,
            cognitive,
            line_count,
            param_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }
    }

    fn make_fc_with_contributions(
        name: &str,
        cyclomatic: u16,
        cognitive: u16,
    ) -> FunctionComplexity {
        use fallow_types::extract::{
            ComplexityContribution, ComplexityContributionKind, ComplexityMetric,
        };
        let mut fc = make_fc(name, cyclomatic, cognitive, 50);
        fc.contributions = vec![ComplexityContribution {
            line: 2,
            col: 4,
            metric: ComplexityMetric::Cyclomatic,
            kind: ComplexityContributionKind::If,
            weight: 1,
            nesting: 0,
        }];
        fc
    }

    #[test]
    fn collect_findings_omits_contributions_without_breakdown_flag() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc_with_contributions("complexFn", 25, 5)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);
        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].contributions.is_empty(),
            "contributions must be omitted without the breakdown flag"
        );
    }

    #[test]
    fn collect_findings_includes_contributions_with_breakdown_flag() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc_with_contributions("complexFn", 25, 5)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);
        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            true,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].contributions.len(),
            1,
            "contributions must flow through when the breakdown flag is set"
        );
    }

    #[test]
    fn build_ignore_set_empty_patterns() {
        let set = build_ignore_set(&[]);
        assert!(set.is_empty());
    }

    #[test]
    fn build_ignore_set_matches_glob() {
        let patterns = vec!["src/generated/**".to_string()];
        let set = build_ignore_set(&patterns);
        assert!(set.is_match(Path::new("src/generated/types.ts")));
        assert!(!set.is_match(Path::new("src/utils.ts")));
    }

    #[test]
    fn build_ignore_set_multiple_patterns() {
        let patterns = vec!["*.test.ts".to_string(), "dist/**".to_string()];
        let set = build_ignore_set(&patterns);
        assert!(set.is_match(Path::new("foo.test.ts")));
        assert!(set.is_match(Path::new("dist/index.js")));
        assert!(!set.is_match(Path::new("src/index.ts")));
    }

    #[test]
    #[should_panic(expected = "validated at config load time")]
    fn build_ignore_set_panics_on_unvalidated_invalid_pattern() {
        let patterns = vec!["[invalid".to_string(), "*.js".to_string()];
        let _ = build_ignore_set(&patterns);
    }

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
            exceeded,
            severity: FindingSeverity::Moderate,
            crap: exceeded.includes_crap().then_some(30.0),
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
        }
    }

    #[test]
    fn sort_findings_by_severity_surfaces_crap_before_single_metric_findings() {
        let mut findings = vec![
            make_finding("cyclomatic", ExceededThreshold::Cyclomatic),
            make_finding("cognitive", ExceededThreshold::Cognitive),
            make_finding("both", ExceededThreshold::Both),
            make_finding("crap", ExceededThreshold::Crap),
            make_finding("cyclomatic_crap", ExceededThreshold::CyclomaticCrap),
            make_finding("all", ExceededThreshold::All),
        ];

        sort_findings(&mut findings, &SortBy::Severity);

        let names = findings
            .iter()
            .map(|finding| finding.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "all",
                "cyclomatic_crap",
                "crap",
                "both",
                "cyclomatic",
                "cognitive",
            ]
        );
    }

    #[test]
    fn collect_findings_empty_modules() {
        let (findings, files, functions) = collect_findings(
            &[],
            &FxHashMap::default(),
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert!(findings.is_empty());
        assert_eq!(files, 0);
        assert_eq!(functions, 0);
    }

    #[test]
    fn collect_findings_below_threshold() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(FileId(0), vec![make_fc("doStuff", 5, 3, 10)])];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, files, functions) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert!(findings.is_empty());
        assert_eq!(files, 1);
        assert_eq!(functions, 1);
    }

    #[test]
    fn collect_findings_exceeds_cyclomatic_only() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc("complexFn", 25, 5, 50)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cyclomatic, 25);
        assert!(matches!(
            findings[0].exceeded,
            ExceededThreshold::Cyclomatic
        ));
    }

    #[test]
    fn collect_findings_exceeds_cognitive_only() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(FileId(0), vec![make_fc("nestedFn", 5, 20, 30)])];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].exceeded, ExceededThreshold::Cognitive));
    }

    #[test]
    fn collect_findings_exceeds_both() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc("terribleFn", 25, 20, 100)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].exceeded, ExceededThreshold::Both));
    }

    #[test]
    fn collect_findings_multiple_functions_per_file() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![
                make_fc("ok", 5, 3, 10),
                make_fc("bad", 25, 20, 50),
                make_fc("also_bad", 21, 5, 30),
            ],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, files, functions) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 2);
        assert_eq!(files, 1);
        assert_eq!(functions, 3);
    }

    #[test]
    fn collect_findings_ignores_matching_files() {
        let path = PathBuf::from("/project/src/generated/types.ts");
        let modules = vec![make_module(FileId(0), vec![make_fc("genFn", 25, 20, 50)])];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let ignore_set = build_ignore_set(&["src/generated/**".to_string()]);
        let (findings, files, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &ignore_set,
            None,
            None,
            20,
            15,
            false,
        );
        assert!(findings.is_empty());
        assert_eq!(files, 0);
    }

    #[test]
    fn collect_findings_filters_by_changed_files() {
        let path_a = PathBuf::from("/project/src/a.ts");
        let path_b = PathBuf::from("/project/src/b.ts");
        let modules = vec![
            make_module(FileId(0), vec![make_fc("fnA", 25, 20, 50)]),
            make_module(FileId(1), vec![make_fc("fnB", 25, 20, 50)]),
        ];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path_a);
        file_paths.insert(FileId(1), &path_b);

        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        let (findings, files, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            Some(&changed),
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].name, "fnA");
        assert_eq!(files, 1);
    }

    fn build_diff(text: &str) -> crate::report::ci::diff_filter::DiffIndex {
        crate::report::ci::diff_filter::DiffIndex::from_unified_diff(text)
    }

    #[test]
    fn filter_complexity_findings_by_diff_keeps_hotspot_overlapping_diff_line() {
        let mut findings = vec![ComplexityViolation {
            path: PathBuf::from("/project/src/big.ts"),
            name: "wide_fn".into(),
            line: 10,
            col: 0,
            cyclomatic: 30,
            cognitive: 30,
            line_count: 110,
            param_count: 0,
            exceeded: ExceededThreshold::Both,
            severity: FindingSeverity::High,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
        }];
        let diff = build_diff(
            "diff --git a/src/big.ts b/src/big.ts\n\
             --- a/src/big.ts\n\
             +++ b/src/big.ts\n\
             @@ -114,1 +114,2 @@\n\
              ctx\n\
             +touched\n",
        );
        filter_complexity_findings_by_diff(&mut findings, &diff, Path::new("/project"));
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn filter_complexity_findings_by_diff_drops_finding_outside_diff() {
        let mut findings = vec![ComplexityViolation {
            path: PathBuf::from("/project/src/elsewhere.ts"),
            name: "outside".into(),
            line: 10,
            col: 0,
            cyclomatic: 30,
            cognitive: 30,
            line_count: 5,
            param_count: 0,
            exceeded: ExceededThreshold::Both,
            severity: FindingSeverity::High,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
        }];
        let diff = build_diff(
            "diff --git a/src/big.ts b/src/big.ts\n\
             --- a/src/big.ts\n\
             +++ b/src/big.ts\n\
             @@ -114,1 +114,2 @@\n\
              ctx\n\
             +touched\n",
        );
        filter_complexity_findings_by_diff(&mut findings, &diff, Path::new("/project"));
        assert!(findings.is_empty());
    }

    #[test]
    fn filter_complexity_findings_by_diff_handles_zero_line_count() {
        let mut findings = vec![ComplexityViolation {
            path: PathBuf::from("/project/src/a.ts"),
            name: "zero_extent".into(),
            line: 5,
            col: 0,
            cyclomatic: 30,
            cognitive: 30,
            line_count: 0,
            param_count: 0,
            exceeded: ExceededThreshold::Both,
            severity: FindingSeverity::High,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
        }];
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -4,1 +4,2 @@\n\
              ctx\n\
             +touched\n",
        );
        filter_complexity_findings_by_diff(&mut findings, &diff, Path::new("/project"));
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn filter_hotspots_by_diff_uses_file_level_membership() {
        use crate::health_types::HotspotEntry;
        let mut hotspots = vec![
            HotspotEntry {
                path: PathBuf::from("/project/src/touched.ts"),
                score: 90.0,
                commits: 50,
                weighted_commits: 25.0,
                lines_added: 1000,
                lines_deleted: 500,
                complexity_density: 0.4,
                fan_in: 5,
                trend: fallow_core::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            },
            HotspotEntry {
                path: PathBuf::from("/project/src/untouched.ts"),
                score: 90.0,
                commits: 50,
                weighted_commits: 25.0,
                lines_added: 1000,
                lines_deleted: 500,
                complexity_density: 0.4,
                fan_in: 5,
                trend: fallow_core::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            },
        ];
        let diff = build_diff(
            "diff --git a/src/touched.ts b/src/touched.ts\n\
             --- a/src/touched.ts\n\
             +++ b/src/touched.ts\n\
             @@ -0,0 +1,1 @@\n\
             +new\n",
        );
        filter_hotspots_by_diff(&mut hotspots, &diff, Path::new("/project"));
        assert_eq!(hotspots.len(), 1);
        assert_eq!(hotspots[0].path, PathBuf::from("/project/src/touched.ts"));
    }

    #[test]
    fn filter_large_functions_by_diff_uses_range_overlap() {
        use crate::health_types::LargeFunctionEntry;
        let mut entries = vec![
            LargeFunctionEntry {
                path: PathBuf::from("/project/src/a.ts"),
                name: "kept".into(),
                line: 10,
                line_count: 100,
            },
            LargeFunctionEntry {
                path: PathBuf::from("/project/src/a.ts"),
                name: "dropped".into(),
                line: 500,
                line_count: 100,
            },
        ];
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -49,1 +49,2 @@\n\
              ctx\n\
             +touched\n",
        );
        filter_large_functions_by_diff(&mut entries, &diff, Path::new("/project"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "kept");
    }

    #[test]
    fn collect_findings_skips_module_without_path() {
        let modules = vec![make_module(FileId(99), vec![make_fc("orphan", 25, 20, 50)])];
        let file_paths = FxHashMap::default();

        let (findings, files, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert!(findings.is_empty());
        assert_eq!(files, 0);
    }

    #[test]
    fn collect_findings_at_exact_threshold_not_reported() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc("borderline", 20, 15, 20)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn collect_findings_preserves_function_metadata() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![FunctionComplexity {
                name: "processData".to_string(),
                line: 42,
                col: 8,
                cyclomatic: 25,
                cognitive: 18,
                line_count: 75,
                param_count: 2,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.name, "processData");
        assert_eq!(f.line, 42);
        assert_eq!(f.col, 8);
        assert_eq!(f.cyclomatic, 25);
        assert_eq!(f.cognitive, 18);
        assert_eq!(f.line_count, 75);
        assert_eq!(f.path, PathBuf::from("/project/src/a.ts"));
    }

    #[test]
    fn merge_crap_findings_disambiguates_same_line_functions() {
        let path = PathBuf::from("/project/src/curried.ts");
        let outer = FunctionComplexity {
            name: "handler".to_string(),
            line: 1,
            col: 23,
            cyclomatic: 1,
            cognitive: 0,
            line_count: 11,
            param_count: 1,
            source_hash: None,
            contributions: Vec::new(),
        };
        let inner = FunctionComplexity {
            name: "<arrow>".to_string(),
            line: 1,
            col: 43,
            cyclomatic: 7,
            cognitive: 0,
            line_count: 10,
            param_count: 1,
            source_hash: None,
            contributions: Vec::new(),
        };
        let modules = vec![make_module(FileId(0), vec![inner.clone(), outer.clone()])];
        let mut file_paths: FxHashMap<FileId, &PathBuf> = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let mut findings: Vec<ComplexityViolation> = Vec::new();

        let mut per_function_crap: FxHashMap<PathBuf, Vec<scoring::PerFunctionCrap>> =
            FxHashMap::default();
        per_function_crap.insert(
            path.clone(),
            vec![
                scoring::PerFunctionCrap {
                    line: inner.line,
                    col: inner.col,
                    crap: 56.0,
                    coverage_pct: None,
                    coverage_tier: crate::health_types::CoverageTier::None,
                    coverage_source: crate::health_types::CoverageSource::Estimated,
                },
                scoring::PerFunctionCrap {
                    line: outer.line,
                    col: outer.col,
                    crap: 2.0,
                    coverage_pct: None,
                    coverage_tier: crate::health_types::CoverageTier::None,
                    coverage_source: crate::health_types::CoverageSource::Estimated,
                },
            ],
        );

        merge_crap_findings(
            &mut findings,
            &CrapFindingMergeInput {
                modules: &modules,
                file_paths: &file_paths,
                config_root: Path::new("/project"),
                ignore_set: &globset::GlobSet::empty(),
                changed_files: None,
                ws_roots: None,
                per_function_crap: &per_function_crap,
                template_inherit_provenance: &FxHashMap::default(),
                max_crap: 30.0,
                max_cyclomatic: 20,
                max_cognitive: 15,
                complexity_breakdown: false,
            },
        );

        assert_eq!(
            findings.len(),
            1,
            "expected one CRAP finding for inner arrow"
        );
        let f = &findings[0];
        assert_eq!(f.name, "<arrow>", "name must come from inner arrow");
        assert_eq!(f.line, 1);
        assert_eq!(f.col, 43, "col must disambiguate same-line arrows");
        assert_eq!(f.cyclomatic, 7, "cyclomatic must come from inner arrow");
        assert_eq!(f.cognitive, 0);
        assert_eq!(
            f.crap,
            Some(56.0),
            "CRAP must match the function it's reported against"
        );
        let cc = f64::from(f.cyclomatic);
        #[expect(
            clippy::suboptimal_flops,
            reason = "cc * cc + cc matches the CRAP formula specification"
        )]
        let expected_crap = cc * cc + cc;
        assert!(
            (f.crap.unwrap() - expected_crap).abs() < 0.01,
            "CRAP must be consistent with reported CC: cc={cc}, crap={:?}, expected={expected_crap}",
            f.crap,
        );
    }

    #[test]
    fn merge_crap_findings_picks_outer_when_outer_exceeds() {
        let path = PathBuf::from("/project/src/curried_outer.ts");
        let outer = FunctionComplexity {
            name: "complex".to_string(),
            line: 5,
            col: 10,
            cyclomatic: 8,
            cognitive: 0,
            line_count: 20,
            param_count: 1,
            source_hash: None,
            contributions: Vec::new(),
        };
        let inner = FunctionComplexity {
            name: "<arrow>".to_string(),
            line: 5,
            col: 30,
            cyclomatic: 1,
            cognitive: 0,
            line_count: 1,
            param_count: 1,
            source_hash: None,
            contributions: Vec::new(),
        };
        let modules = vec![make_module(FileId(0), vec![inner.clone(), outer.clone()])];
        let mut file_paths: FxHashMap<FileId, &PathBuf> = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let mut findings: Vec<ComplexityViolation> = Vec::new();
        let mut per_function_crap: FxHashMap<PathBuf, Vec<scoring::PerFunctionCrap>> =
            FxHashMap::default();
        per_function_crap.insert(
            path.clone(),
            vec![
                scoring::PerFunctionCrap {
                    line: inner.line,
                    col: inner.col,
                    crap: 2.0,
                    coverage_pct: None,
                    coverage_tier: crate::health_types::CoverageTier::None,
                    coverage_source: crate::health_types::CoverageSource::Estimated,
                },
                scoring::PerFunctionCrap {
                    line: outer.line,
                    col: outer.col,
                    crap: 72.0,
                    coverage_pct: None,
                    coverage_tier: crate::health_types::CoverageTier::None,
                    coverage_source: crate::health_types::CoverageSource::Estimated,
                },
            ],
        );

        merge_crap_findings(
            &mut findings,
            &CrapFindingMergeInput {
                modules: &modules,
                file_paths: &file_paths,
                config_root: Path::new("/project"),
                ignore_set: &globset::GlobSet::empty(),
                changed_files: None,
                ws_roots: None,
                per_function_crap: &per_function_crap,
                template_inherit_provenance: &FxHashMap::default(),
                max_crap: 30.0,
                max_cyclomatic: 20,
                max_cognitive: 15,
                complexity_breakdown: false,
            },
        );

        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.name, "complex");
        assert_eq!(f.col, 10);
        assert_eq!(f.cyclomatic, 8);
        assert_eq!(f.crap, Some(72.0));
    }

    fn fx_summary(
        tracked: usize,
        hit: usize,
        unhit: usize,
        untracked: usize,
    ) -> crate::health_types::RuntimeCoverageSummary {
        #[expect(
            clippy::cast_precision_loss,
            reason = "test fixture totals are tiny — f64 precision is fine"
        )]
        let coverage_percent = if tracked == 0 {
            0.0
        } else {
            (hit as f64 / tracked as f64) * 100.0
        };
        crate::health_types::RuntimeCoverageSummary {
            data_source: crate::health_types::RuntimeCoverageDataSource::Local,
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
    ) -> crate::health_types::RuntimeCoverageEvidence {
        crate::health_types::RuntimeCoverageEvidence {
            static_status: static_status.to_owned(),
            test_coverage: test_coverage.to_owned(),
            v8_tracking: v8_tracking.to_owned(),
            untracked_reason: None,
            observation_days: 7,
            deployments_observed: 2,
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

    #[test]
    fn runtime_coverage_top_applies_after_baseline_filtering() {
        let root = Path::new("/project");
        let baseline = HealthBaselineData {
            findings: vec![],
            finding_counts: std::collections::BTreeMap::new(),
            runtime_coverage_findings: vec![
                "fallow:prod:aaaaaaaa".to_owned(),
                "fallow:prod:bbbbbbbb".to_owned(),
            ],
            runtime_coverage_source_hashes: vec![],
            target_keys: vec![],
        };
        let mut report = crate::health_types::RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected,
            signals: Vec::new(),
            summary: fx_summary(3, 0, 2, 1),
            findings: vec![
                crate::health_types::RuntimeCoverageFinding {
                    id: "fallow:prod:aaaaaaaa".to_owned(),
                    stable_id: None,
                    path: PathBuf::from("/project/src/a.ts"),
                    function: "alpha".to_owned(),
                    line: 10,
                    verdict: crate::health_types::RuntimeCoverageVerdict::ReviewRequired,
                    invocations: Some(0),
                    confidence: crate::health_types::RuntimeCoverageConfidence::Medium,
                    evidence: fx_evidence("used", "not_covered", "tracked"),
                    actions: vec![],
                    source_hash: None,
                },
                crate::health_types::RuntimeCoverageFinding {
                    id: "fallow:prod:bbbbbbbb".to_owned(),
                    stable_id: None,
                    path: PathBuf::from("/project/src/b.ts"),
                    function: "beta".to_owned(),
                    line: 20,
                    verdict: crate::health_types::RuntimeCoverageVerdict::CoverageUnavailable,
                    invocations: None,
                    confidence: crate::health_types::RuntimeCoverageConfidence::None,
                    evidence: fx_evidence("used", "not_covered", "untracked"),
                    actions: vec![],
                    source_hash: None,
                },
                crate::health_types::RuntimeCoverageFinding {
                    id: "fallow:prod:cccccccc".to_owned(),
                    stable_id: None,
                    path: PathBuf::from("/project/src/c.ts"),
                    function: "gamma".to_owned(),
                    line: 30,
                    verdict: crate::health_types::RuntimeCoverageVerdict::ReviewRequired,
                    invocations: Some(0),
                    confidence: crate::health_types::RuntimeCoverageConfidence::Medium,
                    evidence: fx_evidence("used", "not_covered", "tracked"),
                    actions: vec![],
                    source_hash: None,
                },
            ],
            hot_paths: vec![
                crate::health_types::RuntimeCoverageHotPath {
                    id: "fallow:hot:11111111".to_owned(),
                    stable_id: None,
                    path: PathBuf::from("/project/src/hot-a.ts"),
                    function: "hotAlpha".to_owned(),
                    line: 1,
                    end_line: 5,
                    invocations: 500,
                    percentile: 99,
                    actions: vec![],
                },
                crate::health_types::RuntimeCoverageHotPath {
                    id: "fallow:hot:22222222".to_owned(),
                    stable_id: None,
                    path: PathBuf::from("/project/src/hot-b.ts"),
                    function: "hotBeta".to_owned(),
                    line: 2,
                    end_line: 8,
                    invocations: 250,
                    percentile: 50,
                    actions: vec![],
                },
            ],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root)
                .with_baseline(Some(&baseline))
                .with_top(Some(1)),
        );

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].function, "gamma");
        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected
        );
        assert_eq!(report.summary.functions_tracked, 3);
        assert_eq!(report.summary.functions_hit, 0);
        assert_eq!(report.summary.functions_unhit, 2);
        assert_eq!(report.summary.functions_untracked, 1);
        assert!((report.summary.coverage_percent - 0.0).abs() < 0.05);
        assert_eq!(report.hot_paths.len(), 1);
        assert_eq!(report.hot_paths[0].function, "hotAlpha");
    }

    #[test]
    fn runtime_coverage_baseline_refreshes_to_clean_when_only_baselined_findings_remain() {
        let root = Path::new("/project");
        let baseline = HealthBaselineData {
            findings: vec![],
            finding_counts: std::collections::BTreeMap::new(),
            runtime_coverage_findings: vec!["fallow:prod:aaaaaaaa".to_owned()],
            runtime_coverage_source_hashes: vec![],
            target_keys: vec![],
        };
        let mut report = crate::health_types::RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected,
            signals: Vec::new(),
            summary: fx_summary(2, 1, 1, 0),
            findings: vec![crate::health_types::RuntimeCoverageFinding {
                id: "fallow:prod:aaaaaaaa".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/a.ts"),
                function: "alpha".to_owned(),
                line: 10,
                verdict: crate::health_types::RuntimeCoverageVerdict::ReviewRequired,
                invocations: Some(0),
                confidence: crate::health_types::RuntimeCoverageConfidence::Medium,
                evidence: fx_evidence("used", "not_covered", "tracked"),
                actions: vec![],
                source_hash: None,
            }],
            hot_paths: vec![],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_baseline(Some(&baseline)),
        );

        assert!(report.findings.is_empty());
        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::Clean
        );
        assert_eq!(report.summary.functions_tracked, 2);
        assert_eq!(report.summary.functions_hit, 1);
        assert_eq!(report.summary.functions_unhit, 1);
        assert_eq!(report.summary.functions_untracked, 0);
        assert!((report.summary.coverage_percent - 50.0).abs() < 0.05);
    }

    #[test]
    fn runtime_coverage_changed_review_uses_hot_path_verdict() {
        let root = Path::new("/project");
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = crate::health_types::RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: crate::health_types::RuntimeCoverageReportVerdict::Clean,
            signals: Vec::new(),
            summary: fx_summary(2, 2, 0, 0),
            findings: vec![],
            hot_paths: vec![crate::health_types::RuntimeCoverageHotPath {
                id: "fallow:hot:33333333".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/hot.ts"),
                function: "renderHotPath".to_owned(),
                line: 7,
                end_line: 24,
                invocations: 9_500,
                percentile: 99,
                actions: vec![],
            }],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
        );

        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::HotPathTouched
        );
    }

    #[test]
    fn runtime_coverage_changed_review_ignores_unmodified_hot_paths() {
        let root = Path::new("/project");
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/other.ts"));
        let mut report = crate::health_types::RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: crate::health_types::RuntimeCoverageReportVerdict::Clean,
            signals: Vec::new(),
            summary: fx_summary(2, 2, 0, 0),
            findings: vec![],
            hot_paths: vec![crate::health_types::RuntimeCoverageHotPath {
                id: "fallow:hot:44444444".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/hot.ts"),
                function: "renderHotPath".to_owned(),
                line: 7,
                end_line: 24,
                invocations: 9_500,
                percentile: 90,
                actions: vec![],
            }],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
        );

        assert!(report.hot_paths.is_empty());
        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::Clean
        );
    }

    fn fx_runtime_coverage_report_with_hot_paths(
        hot_paths: Vec<crate::health_types::RuntimeCoverageHotPath>,
    ) -> crate::health_types::RuntimeCoverageReport {
        crate::health_types::RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: crate::health_types::RuntimeCoverageReportVerdict::Clean,
            signals: Vec::new(),
            summary: fx_summary(2, 2, 0, 0),
            findings: vec![],
            hot_paths,
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
        }
    }

    fn fx_hot_path(
        id: &str,
        path: &str,
        line: u32,
        end_line: u32,
    ) -> crate::health_types::RuntimeCoverageHotPath {
        crate::health_types::RuntimeCoverageHotPath {
            id: id.to_owned(),
            stable_id: None,
            path: PathBuf::from(path),
            function: "renderHotPath".to_owned(),
            line,
            end_line,
            invocations: 9_500,
            percentile: 99,
            actions: vec![],
        }
    }

    #[test]
    fn runtime_coverage_diff_index_keeps_hot_paths_with_added_line_in_range() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -10,1 +10,2 @@\n\
                    +  // touch the body\n\
                    line 11\n";
        let diff_index = crate::report::ci::diff_filter::DiffIndex::from_unified_diff(diff);
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:01010101",
            "src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
        );

        assert_eq!(report.hot_paths.len(), 1);
        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::HotPathTouched
        );
    }

    #[test]
    fn runtime_coverage_diff_index_drops_hot_paths_when_added_line_outside_range() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -50,1 +50,2 @@\n\
                    +  // unrelated change far below the hot function\n\
                    line 51\n";
        let diff_index = crate::report::ci::diff_filter::DiffIndex::from_unified_diff(diff);
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:02020202",
            "src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
        );

        assert!(report.hot_paths.is_empty());
        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::Clean
        );
    }

    #[test]
    fn runtime_coverage_diff_index_falls_back_to_single_line_when_end_line_zero() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -7,1 +7,2 @@\n\
                    +  // exactly the function's start line\n\
                    line 8\n";
        let diff_index = crate::report::ci::diff_filter::DiffIndex::from_unified_diff(diff);
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:03030303",
            "src/hot.ts",
            7,
            0,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
        );

        assert_eq!(report.hot_paths.len(), 1);
        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::HotPathTouched
        );
    }

    #[test]
    fn runtime_coverage_diff_index_resolves_absolute_hot_path_against_root() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -10,1 +10,2 @@\n\
                    +  // touched\n\
                    line 11\n";
        let diff_index = crate::report::ci::diff_filter::DiffIndex::from_unified_diff(diff);
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:04040404",
            "/project/src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
        );

        assert_eq!(report.hot_paths.len(), 1);
    }

    #[test]
    fn runtime_coverage_diff_index_authoritative_for_files_in_diff() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -50,1 +50,2 @@\n\
                    +  // outside the hot function\n\
                    line 51\n";
        let diff_index = crate::report::ci::diff_filter::DiffIndex::from_unified_diff(diff);
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:05050505",
            "src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root)
                .with_changed_files(Some(&changed_files))
                .with_diff_index(Some(&diff_index)),
        );

        assert!(report.hot_paths.is_empty());
        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::Clean
        );
    }

    #[test]
    fn runtime_coverage_per_file_fallback_to_changed_files_when_diff_omits_file() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/other.ts b/src/other.ts\n\
                    --- a/src/other.ts\n\
                    +++ b/src/other.ts\n\
                    @@ -1,1 +1,2 @@\n\
                    +  // unrelated\n\
                    line 2\n";
        let diff_index = crate::report::ci::diff_filter::DiffIndex::from_unified_diff(diff);
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:0a0a0a0a",
            "src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root)
                .with_changed_files(Some(&changed_files))
                .with_diff_index(Some(&diff_index)),
        );

        assert_eq!(report.hot_paths.len(), 1);
        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::HotPathTouched
        );
    }

    #[test]
    fn runtime_coverage_pr_context_promotes_hot_path_touched_above_cold_code() {
        let root = Path::new("/project");
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = crate::health_types::RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected,
            signals: Vec::new(),
            summary: fx_summary(2, 1, 1, 0),
            findings: vec![crate::health_types::RuntimeCoverageFinding {
                id: "fallow:prod:cold0001".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/cold.ts"),
                function: "coldFn".to_owned(),
                line: 4,
                verdict: crate::health_types::RuntimeCoverageVerdict::SafeToDelete,
                invocations: Some(0),
                confidence: crate::health_types::RuntimeCoverageConfidence::High,
                evidence: fx_evidence("unused", "not_covered", "tracked"),
                actions: vec![],
                source_hash: None,
            }],
            hot_paths: vec![fx_hot_path("fallow:hot:0b0b0b0b", "src/hot.ts", 7, 24)],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
        );

        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::HotPathTouched
        );
        assert_eq!(
            report.signals,
            vec![
                crate::health_types::RuntimeCoverageSignal::ColdCodeDetected,
                crate::health_types::RuntimeCoverageSignal::HotPathTouched,
            ]
        );
    }

    #[test]
    fn runtime_coverage_standalone_keeps_cold_code_primary_above_unchanged_hot_paths() {
        let root = Path::new("/project");
        let mut report = crate::health_types::RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: crate::health_types::RuntimeCoverageReportVerdict::Clean,
            signals: Vec::new(),
            summary: fx_summary(2, 1, 1, 0),
            findings: vec![crate::health_types::RuntimeCoverageFinding {
                id: "fallow:prod:cold0002".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/cold.ts"),
                function: "coldFn".to_owned(),
                line: 4,
                verdict: crate::health_types::RuntimeCoverageVerdict::SafeToDelete,
                invocations: Some(0),
                confidence: crate::health_types::RuntimeCoverageConfidence::High,
                evidence: fx_evidence("unused", "not_covered", "tracked"),
                actions: vec![],
                source_hash: None,
            }],
            hot_paths: vec![fx_hot_path("fallow:hot:0c0c0c0c", "src/hot.ts", 7, 24)],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
        };

        apply_runtime_coverage_filters(&mut report, &RuntimeCoverageFilterContext::new(root));

        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected
        );
        assert_eq!(
            report.signals,
            vec![crate::health_types::RuntimeCoverageSignal::ColdCodeDetected]
        );
        assert_eq!(report.hot_paths.len(), 1);
    }

    #[test]
    fn runtime_coverage_license_grace_outranks_pr_context_signals() {
        let root = Path::new("/project");
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = crate::health_types::RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: crate::health_types::RuntimeCoverageReportVerdict::LicenseExpiredGrace,
            signals: Vec::new(),
            summary: fx_summary(2, 1, 1, 0),
            findings: vec![],
            hot_paths: vec![fx_hot_path("fallow:hot:0d0d0d0d", "src/hot.ts", 7, 24)],
            blast_radius: vec![],
            importance: vec![],
            watermark: Some(crate::health_types::RuntimeCoverageWatermark::LicenseExpiredGrace),
            warnings: vec![],
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
        );

        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::LicenseExpiredGrace
        );
        assert!(
            report
                .signals
                .contains(&crate::health_types::RuntimeCoverageSignal::LicenseExpiredGrace)
        );
        assert!(
            report
                .signals
                .contains(&crate::health_types::RuntimeCoverageSignal::HotPathTouched)
        );
    }

    #[test]
    fn retain_hot_paths_drops_when_diff_touches_file_but_no_added_lines() {
        let root = Path::new("/project");
        let diff = crate::report::ci::diff_filter::DiffIndex::from_unified_diff(
            "diff --git a/src/hot.ts b/src/hot.ts\n\
             --- a/src/hot.ts\n\
             +++ b/src/hot.ts\n\
             @@ -10,3 +10,1 @@\n\
             -one\n\
             -two\n\
             -three\n\
             ctx\n",
        );
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:deletiononly",
            "src/hot.ts",
            10,
            12,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root)
                .with_diff_index(Some(&diff))
                .with_changed_files(Some(&changed_files)),
        );

        assert!(
            report.hot_paths.is_empty(),
            "diff touched the file with no added lines: must drop, not fall through to changed_files"
        );
    }

    #[test]
    fn runtime_coverage_changed_files_matches_relative_hot_path_against_absolute_set() {
        let root = Path::new("/project");
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:06060606",
            "src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
        );

        assert_eq!(report.hot_paths.len(), 1);
    }

    fn fx_low_traffic_runtime_result() -> HealthResult {
        HealthResult {
            report: crate::health_types::HealthReport {
                runtime_coverage: Some(crate::health_types::RuntimeCoverageReport {
                    schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
                    verdict: crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected,
                    signals: Vec::new(),
                    summary: fx_summary(1, 0, 1, 0),
                    findings: vec![crate::health_types::RuntimeCoverageFinding {
                        id: "fallow:prod:lowtraffic".to_owned(),
                        stable_id: None,
                        path: PathBuf::from("/project/src/cold.ts"),
                        function: "coldPath".to_owned(),
                        line: 14,
                        verdict: crate::health_types::RuntimeCoverageVerdict::LowTraffic,
                        invocations: Some(1),
                        confidence: crate::health_types::RuntimeCoverageConfidence::Low,
                        evidence: fx_evidence("used", "not_covered", "tracked"),
                        actions: vec![],
                        source_hash: None,
                    }],
                    hot_paths: vec![],
                    blast_radius: vec![],
                    importance: vec![],
                    watermark: None,
                    warnings: vec![],
                }),
                ..crate::health_types::HealthReport::default()
            },
            grouping: None,
            group_resolver: None,
            config: test_resolved_config(),
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
                    min_score: None,
                    min_severity: None,
                    report_only: false,
                    summary: false,
                    summary_heading: true,
                    show_explain_tip: true,
                    skip_score_and_trend: false,
                },
            ),
            ExitCode::from(1),
        );
    }

    fn fx_health_score(score: f64, grade: &'static str) -> crate::health_types::HealthScore {
        crate::health_types::HealthScore {
            formula_version: 2,
            score,
            grade,
            penalties: crate::health_types::HealthScorePenalties {
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
            },
        }
    }

    fn fx_gate_result(
        findings: Vec<crate::health_types::HealthFinding>,
        score: Option<crate::health_types::HealthScore>,
    ) -> HealthResult {
        HealthResult {
            report: crate::health_types::HealthReport {
                findings,
                health_score: score,
                ..crate::health_types::HealthReport::default()
            },
            grouping: None,
            group_resolver: None,
            config: test_resolved_config(),
            elapsed: Duration::default(),
            timings: None,
            coverage_gaps_has_findings: false,
            should_fail_on_coverage_gaps: false,
        }
    }

    fn moderate_finding() -> crate::health_types::HealthFinding {
        make_finding("moderate", ExceededThreshold::Cyclomatic).into()
    }

    fn critical_finding() -> crate::health_types::HealthFinding {
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
                min_score,
                min_severity,
                report_only,
                summary: false,
                summary_heading: true,
                show_explain_tip: true,
                skip_score_and_trend: false,
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

    fn make_class_finding(
        path: &str,
        name: &str,
        line: u32,
        cyclomatic: u16,
        cognitive: u16,
    ) -> ComplexityViolation {
        ComplexityViolation {
            path: PathBuf::from(path),
            name: name.to_string(),
            line,
            col: 0,
            cyclomatic,
            cognitive,
            line_count: 20,
            param_count: 0,
            exceeded: ExceededThreshold::Both,
            severity: FindingSeverity::Moderate,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
        }
    }

    fn make_template_finding(
        path: &str,
        line: u32,
        cyclomatic: u16,
        cognitive: u16,
    ) -> ComplexityViolation {
        ComplexityViolation {
            path: PathBuf::from(path),
            name: "<template>".to_string(),
            line,
            col: 0,
            cyclomatic,
            cognitive,
            line_count: 30,
            param_count: 0,
            exceeded: ExceededThreshold::Both,
            severity: FindingSeverity::Moderate,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
        }
    }

    #[test]
    fn rollup_external_template_via_provenance_lookup() {
        let component_ts = PathBuf::from("/proj/src/host-game.component.ts");
        let template_html = PathBuf::from("/proj/src/host-game.component.html");
        let mut findings = vec![
            make_class_finding(component_ts.to_str().unwrap(), "handleClick", 42, 3, 4),
            make_template_finding(template_html.to_str().unwrap(), 1, 6, 10),
        ];
        let mut lookup = rustc_hash::FxHashMap::default();
        lookup.insert(template_html.clone(), component_ts.clone());
        append_component_rollup_findings(&mut findings, Some(&lookup), 8, 8);

        assert_eq!(findings.len(), 3, "rollup is strictly additive");
        let rollup = findings
            .iter()
            .find(|f| f.name == "<component>")
            .expect("rollup must be present");
        assert_eq!(rollup.path, component_ts);
        assert_eq!(rollup.cyclomatic, 9, "9 = worst class 3 + template 6");
        assert_eq!(rollup.cognitive, 14, "14 = worst class 4 + template 10");
        assert_eq!(rollup.line, 42, "anchored at worst class function line");
        let breakdown = rollup.component_rollup.as_ref().expect("breakdown present");
        assert_eq!(
            breakdown.component, "host-game.component",
            "component identifier is the .ts owner's file stem"
        );
        assert_eq!(breakdown.class_worst_function, "handleClick");
        assert_eq!(breakdown.class_cyclomatic, 3);
        assert_eq!(breakdown.template_cyclomatic, 6);
        assert_eq!(breakdown.template_path, template_html);
    }

    #[test]
    fn rollup_inline_template_owner_is_same_ts_file() {
        let component_ts = PathBuf::from("/proj/src/inline.component.ts");
        let mut findings = vec![
            make_class_finding(component_ts.to_str().unwrap(), "ngOnInit", 25, 5, 8),
            make_template_finding(component_ts.to_str().unwrap(), 10, 4, 6),
        ];
        append_component_rollup_findings(&mut findings, None, 8, 8);

        let rollup = findings
            .iter()
            .find(|f| f.name == "<component>")
            .expect("rollup must be present for inline-template case without provenance lookup");
        assert_eq!(rollup.cyclomatic, 9);
        assert_eq!(rollup.cognitive, 14);
        let breakdown = rollup.component_rollup.as_ref().unwrap();
        assert_eq!(breakdown.template_path, component_ts);
        assert_eq!(breakdown.component, "inline.component");
    }

    #[test]
    fn rollup_picks_worst_class_function_by_cyclomatic() {
        let component_ts = PathBuf::from("/proj/src/multi.component.ts");
        let template = PathBuf::from("/proj/src/multi.component.html");
        let mut findings = vec![
            make_class_finding(component_ts.to_str().unwrap(), "first", 10, 3, 4),
            make_class_finding(component_ts.to_str().unwrap(), "worst", 20, 8, 9),
            make_class_finding(component_ts.to_str().unwrap(), "middle", 30, 5, 6),
            make_template_finding(template.to_str().unwrap(), 1, 4, 6),
        ];
        let mut lookup = rustc_hash::FxHashMap::default();
        lookup.insert(template, component_ts);
        append_component_rollup_findings(&mut findings, Some(&lookup), 8, 8);

        let rollup = findings.iter().find(|f| f.name == "<component>").unwrap();
        assert_eq!(rollup.cyclomatic, 12, "8 (worst.cyc) + 4 (template.cyc)");
        let breakdown = rollup.component_rollup.as_ref().unwrap();
        assert_eq!(breakdown.class_worst_function, "worst");
        assert_eq!(breakdown.class_cyclomatic, 8);
    }

    #[test]
    fn rollup_skipped_when_no_template_finding() {
        let component_ts = "/proj/src/only-class.component.ts";
        let mut findings = vec![make_class_finding(component_ts, "Foo.method", 10, 5, 7)];
        let before = findings.len();
        append_component_rollup_findings(&mut findings, None, 30, 25);
        assert_eq!(findings.len(), before, "no template means no rollup");
    }

    #[test]
    fn rollup_skipped_when_no_class_findings() {
        let template_html = PathBuf::from("/proj/src/orphan.component.html");
        let component_ts = PathBuf::from("/proj/src/orphan.component.ts");
        let mut findings = vec![make_template_finding(
            template_html.to_str().unwrap(),
            1,
            6,
            10,
        )];
        let mut lookup = rustc_hash::FxHashMap::default();
        lookup.insert(template_html, component_ts);
        let before = findings.len();
        append_component_rollup_findings(&mut findings, Some(&lookup), 8, 8);
        assert_eq!(
            findings.len(),
            before,
            "no class methods above threshold means no rollup"
        );
    }

    #[test]
    fn rollup_skipped_when_multiple_templates_on_one_owner() {
        let component_ts = PathBuf::from("/proj/src/twin.component.ts");
        let mut findings = vec![
            make_class_finding(component_ts.to_str().unwrap(), "TwinA.fn", 10, 5, 7),
            make_template_finding(component_ts.to_str().unwrap(), 5, 3, 4),
            make_template_finding(component_ts.to_str().unwrap(), 50, 4, 5),
        ];
        let before = findings.len();
        append_component_rollup_findings(&mut findings, None, 30, 25);
        assert_eq!(
            findings.len(),
            before,
            "two templates on one owner is defensively skipped"
        );
    }

    #[test]
    fn rollup_external_template_skipped_when_lookup_missing() {
        let template_html = PathBuf::from("/proj/src/no-owner.component.html");
        let component_ts = "/proj/src/no-owner.component.ts";
        let mut findings = vec![
            make_class_finding(component_ts, "NoOwner.fn", 10, 5, 7),
            make_template_finding(template_html.to_str().unwrap(), 1, 6, 10),
        ];
        let before = findings.len();
        append_component_rollup_findings(&mut findings, None, 30, 25);
        assert_eq!(findings.len(), before);
    }
}
