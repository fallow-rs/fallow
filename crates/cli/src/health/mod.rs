pub mod coverage;
mod grouping;
mod hotspots;
pub mod ownership;
// `mod health` is itself private at the lib root, so `pub mod` here is
// effectively `pub(crate)`; clippy's `redundant_pub_crate` rejects literal
// `pub(crate)` when the parent is non-pub. The same reasoning applies to
// `pub fn` items inside this module.
pub mod scoring;
mod targets;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use colored::Colorize;
use fallow_config::{OutputFormat, ResolvedConfig};
use rustc_hash::FxHashSet;

use crate::baseline::{
    HealthBaselineData, filter_new_health_findings, filter_new_health_targets,
    filter_new_runtime_coverage_findings,
};
use crate::check::{get_changed_files, resolve_workspace_scope};
use crate::error::emit_error;
pub use crate::health_types::*;
use crate::report;
use crate::vital_signs;

use hotspots::compute_hotspots;
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
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub baseline: Option<&'a std::path::Path>,
    pub save_baseline: Option<&'a std::path::Path>,
    pub complexity: bool,
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
    /// Paid runtime coverage sidecar input.
    pub runtime_coverage: Option<RuntimeCoverageOptions>,
}

struct HealthPipelineTimings {
    config: f64,
    discover: f64,
    parse: f64,
}

struct HealthPipelineInput {
    config: ResolvedConfig,
    files: Vec<fallow_types::discover::DiscoveredFile>,
    modules: Vec<fallow_types::extract::ModuleInfo>,
    timings: HealthPipelineTimings,
    pre_computed_analysis: Option<fallow_core::AnalysisOutput>,
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
    execute_health_inner(
        opts,
        HealthPipelineInput {
            config,
            files: shared.files,
            modules: shared.modules,
            timings: HealthPipelineTimings {
                config: 0.0,
                discover: 0.0,
                parse: 0.0,
            },
            pre_computed_analysis: shared.analysis_output,
        },
    )
}

pub fn execute_health(opts: &HealthOptions<'_>) -> Result<HealthResult, ExitCode> {
    scoring::validate_coverage_root_absolute(opts.coverage_root)
        .map_err(|e| emit_error(&e, 2, opts.output))?;
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

    // Discover and parse files
    let t = Instant::now();
    let files = fallow_core::discover::discover_files_with_plugin_scopes(&config);
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;

    let cache = if config.no_cache {
        None
    } else {
        fallow_core::cache::CacheStore::load(&config.cache_dir)
    };
    let t = Instant::now();
    let parse_result = fallow_core::extract::parse_all_files(&files, cache.as_ref(), true);
    let parse_ms = t.elapsed().as_secs_f64() * 1000.0;

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
            },
        pre_computed_analysis,
    } = input;

    // Resolve thresholds: CLI flags override config
    let max_cyclomatic = opts.max_cyclomatic.unwrap_or(config.health.max_cyclomatic);
    let max_cognitive = opts.max_cognitive.unwrap_or(config.health.max_cognitive);
    let max_crap = opts.max_crap.unwrap_or(config.health.max_crap);
    // CRAP findings require per-function CRAP data, which is computed as a
    // side effect of file scoring. Enforce CRAP checks only when the
    // configured threshold is positive; a zero or negative threshold would
    // flag every function and is treated as "disabled".
    let enforce_crap = max_crap > 0.0;

    let ignore_set = build_ignore_set(&config.health.ignore);
    let changed_files = opts
        .changed_since
        .and_then(|git_ref| get_changed_files(opts.root, git_ref));
    let ws_roots = resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;

    // Validate `--group-by` upfront so misconfigured invocations
    // (`--group-by package` on a non-monorepo, missing CODEOWNERS) fail
    // before any expensive pipeline stage runs and emit exactly one
    // structured error rather than chaining a later git/hotspot error
    // with the group-by error in the same JSON stream.
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

    // Build FileId -> path lookup for O(1) access
    let file_paths: rustc_hash::FxHashMap<_, _> = files.iter().map(|f| (f.id, &f.path)).collect();

    // Collect and filter complexity findings.
    //
    // The workspace filter is pushed inside `collect_findings` so the
    // `files_analyzed` and `total_functions` counters (which feed the report
    // summary) reflect the workspace subset rather than the entire monorepo.
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
    );
    let mut findings = findings;
    let complexity_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Coverage gaps have two separate concerns:
    // - reporting: include the section in the rendered health output
    // - gating: fail the command when config severity is `error`
    //
    // Config severity may enable reporting for top-level `health` when the user
    // did not explicitly choose sections, but it must not override callers that
    // intentionally set `coverage_gaps: false` (combined mode, audit, score-only).
    let config_coverage_enabled = config.rules.coverage_gaps != fallow_config::Severity::Off;
    let report_coverage_gaps =
        opts.coverage_gaps || (opts.config_activates_coverage_gaps && config_coverage_enabled);
    let enforce_coverage_gaps = opts.enforce_coverage_gap_gate
        && config.rules.coverage_gaps == fallow_config::Severity::Error;

    // Load Istanbul coverage data for accurate CRAP scoring.
    // Priority: explicit --coverage flag > auto-detected coverage-final.json.
    let istanbul_coverage = if let Some(coverage_path) = opts.coverage {
        match scoring::load_istanbul_coverage(coverage_path, opts.coverage_root, Some(&config.root))
        {
            Ok(cov) => Some(cov),
            Err(e) => {
                emit_error(&format!("coverage: {e}"), 2, opts.output);
                return Err(ExitCode::from(2));
            }
        }
    } else if let Some(auto_path) = scoring::auto_detect_coverage(&config.root) {
        // Auto-detected coverage file: best-effort, don't fail if it can't be parsed.
        // Note in CI environments so pipelines know scores may vary with coverage presence.
        if std::env::var("CI").is_ok_and(|v| !v.is_empty()) {
            eprintln!(
                "note: using auto-detected coverage at {}; pass --coverage explicitly for deterministic CI scores",
                auto_path.display()
            );
        }
        scoring::load_istanbul_coverage(&auto_path, opts.coverage_root, Some(&config.root)).ok()
    } else {
        None
    };

    // Compute file-level health scores (needed by hotspots and targets too).
    // `enforce_crap` requires per-function CRAP data emitted as a side effect
    // of file scoring, so it forces the file-score pipeline on even for
    // otherwise-complexity-only runs.
    let needs_file_scores = opts.file_scores
        || report_coverage_gaps
        || enforce_coverage_gaps
        || opts.hotspots
        || opts.targets
        || opts.force_full
        || enforce_crap;
    let needs_analysis_output = needs_file_scores || opts.runtime_coverage.is_some();
    let mut shared_analysis_output = if needs_analysis_output {
        if let Some(pre) = pre_computed_analysis {
            Some(pre)
        } else {
            Some(
                fallow_core::analyze_with_parse_result(&config, &modules)
                    .map_err(|e| emit_error(&format!("analysis failed: {e}"), 2, opts.output))?,
            )
        }
    } else {
        None
    };

    let mut runtime_coverage = if let Some(ref production_options) = opts.runtime_coverage {
        let analysis_output = shared_analysis_output
            .as_ref()
            .expect("runtime coverage requires analysis output");
        Some(coverage::analyze(
            production_options,
            &config.root,
            &modules,
            analysis_output,
            istanbul_coverage.as_ref(),
            &file_paths,
            &ignore_set,
            changed_files.as_ref(),
            ws_roots.as_deref(),
            opts.top,
            config.codeowners.as_deref(),
            opts.quiet,
            opts.output,
        )?)
    } else {
        None
    };

    let precomputed_for_scores = if needs_file_scores {
        shared_analysis_output.take()
    } else {
        None
    };

    // Run file scoring and churn fetch in parallel when both are needed.
    // Churn fetch involves a `git log` shell-out that dominates health timing,
    // so keep it tied to sections that actually consume churn data.
    let needs_churn = opts.hotspots || opts.targets;
    let (file_score_result, file_scores_ms, churn_fetch) = if needs_file_scores && needs_churn {
        std::thread::scope(|s| {
            let churn_handle = s.spawn(|| hotspots::fetch_churn_data(opts, &config.cache_dir));
            let t = Instant::now();
            let score_result = compute_filtered_file_scores(
                &config,
                &modules,
                &file_paths,
                changed_files.as_ref(),
                ws_roots.as_deref(),
                &ignore_set,
                opts.output,
                istanbul_coverage.as_ref(),
                precomputed_for_scores,
            );
            let fs_ms = t.elapsed().as_secs_f64() * 1000.0;
            let churn = churn_handle.join().expect("churn thread panicked");
            (score_result, fs_ms, churn)
        })
    } else {
        let t = Instant::now();
        let score_result = if needs_file_scores {
            compute_filtered_file_scores(
                &config,
                &modules,
                &file_paths,
                changed_files.as_ref(),
                ws_roots.as_deref(),
                &ignore_set,
                opts.output,
                istanbul_coverage.as_ref(),
                precomputed_for_scores,
            )
        } else {
            Ok((None, None, None))
        };
        let fs_ms = t.elapsed().as_secs_f64() * 1000.0;
        let churn = if needs_churn {
            hotspots::fetch_churn_data(opts, &config.cache_dir)
        } else {
            None
        };
        (score_result, fs_ms, churn)
    };
    let (git_churn_ms, git_churn_cache_hit) = churn_fetch
        .as_ref()
        .map_or((0.0, false), |cf| (cf.git_log_ms, cf.cache_hit));
    let (score_output, files_scored, average_maintainability) = file_score_result?;

    // Print churn cache note on cold miss (only when cache is enabled)
    if let Some(ref cf) = churn_fetch
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

    let file_scores_slice = score_output
        .as_ref()
        .map_or(&[] as &[_], |o| o.scores.as_slice());

    // Merge per-function CRAP data into complexity findings. Functions that
    // only exceed `--max-crap` (and not cyclomatic/cognitive) are added as
    // new findings; functions already flagged for complexity get the CRAP
    // fields populated so reports can surface the combined risk.
    if enforce_crap && let Some(ref score_out) = score_output {
        merge_crap_findings(
            &mut findings,
            &modules,
            &file_paths,
            &config.root,
            &ignore_set,
            changed_files.as_ref(),
            ws_roots.as_deref(),
            &score_out.per_function_crap,
            max_crap,
            max_cyclomatic,
            max_cognitive,
        );
    }
    sort_findings(&mut findings, &opts.sort);
    let total_above_threshold = findings.len();

    // Count severity tiers before baseline filtering and --top truncation
    let (mut sev_critical, mut sev_high, mut sev_moderate) = (0usize, 0usize, 0usize);
    for f in &findings {
        match f.severity {
            FindingSeverity::Critical => sev_critical += 1,
            FindingSeverity::High => sev_high += 1,
            FindingSeverity::Moderate => sev_moderate += 1,
        }
    }

    // Load baseline for filtering (save happens after targets are computed)
    let loaded_baseline = if let Some(load_path) = opts.baseline {
        Some(load_health_baseline(
            load_path,
            &mut findings,
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

    // Compute hotspot analysis using pre-fetched churn data
    let t = Instant::now();
    let (hotspots, hotspot_summary) = if let Some(churn_data) = churn_fetch {
        compute_hotspots(
            opts,
            &config,
            file_scores_slice,
            &ignore_set,
            ws_roots.as_deref(),
            churn_data,
        )
    } else {
        (Vec::new(), None)
    };
    let hotspots_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Compute refactoring targets
    let t = Instant::now();
    let (targets, target_thresholds) = compute_targets(
        opts,
        score_output.as_ref(),
        file_scores_slice,
        &hotspots,
        loaded_baseline.as_ref(),
        &config.root,
    );
    let targets_ms = t.elapsed().as_secs_f64() * 1000.0;

    if let Some(report) = runtime_coverage.as_mut() {
        apply_runtime_coverage_filters(
            report,
            loaded_baseline.as_ref(),
            &config.root,
            opts.top,
            changed_files.as_ref(),
        );
    }

    if let Some(save_path) = opts.save_baseline {
        save_health_baseline(
            save_path,
            &findings,
            runtime_coverage
                .as_ref()
                .map_or(&[], |report| report.findings.as_slice()),
            &targets,
            &config.root,
            opts.quiet,
            opts.output,
        )?;
    }

    let candidate_paths = collect_candidate_paths(
        &files,
        &config,
        changed_files.as_ref(),
        ws_roots.as_deref(),
        &ignore_set,
    );

    // Compute vital signs (always needed for report summary).
    //
    // When `--workspace` (or `--changed-workspaces`) restricts the run to a
    // subset of files, every per-module aggregate, dead-code denominator,
    // and total-file count must reflect that subset rather than the full
    // monorepo. The same `ws_roots` slice that already filters findings,
    // file scores, and hotspots is forwarded here so the produced
    // `vital_signs` and downstream `health_score` are coherent with what
    // the user asked for. Use the already-filtered candidate path set so
    // workspace, changed-since, and health ignore filters all share one scope.
    let project_subset = if candidate_paths.len() == files.len() {
        SubsetFilter::Full
    } else {
        SubsetFilter::Paths(&candidate_paths)
    };
    let total_files_scoped = candidate_paths.len();
    let (mut vital_signs, mut counts) = compute_vital_signs_and_counts(
        score_output.as_ref(),
        &modules,
        &file_paths,
        needs_file_scores,
        file_scores_slice,
        opts.hotspots || opts.targets,
        &hotspots,
        total_files_scoped,
        &project_subset,
    );

    // Run duplication analysis when --score is active to populate the duplication penalty.
    let t = Instant::now();
    if opts.score {
        let scoped_files = filter_files_to_paths(&files, &candidate_paths);
        let dupes_report = if opts.no_cache {
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
        };
        apply_duplication_metrics(&mut vital_signs, &mut counts, &dupes_report);
    }
    let duplication_ms = t.elapsed().as_secs_f64() * 1000.0;

    let health_score = if opts.score {
        Some(vital_signs::compute_health_score(
            &vital_signs,
            total_files_scoped,
        ))
    } else {
        None
    };

    // Collect large functions (>60 LOC) when the risk profile warrants it
    let large_functions = collect_large_functions(
        &vital_signs,
        &modules,
        &file_paths,
        &config.root,
        &ignore_set,
        changed_files.as_ref(),
        ws_roots.as_deref(),
    );

    // Determine coverage model for snapshot and report
    let active_coverage_model = if istanbul_coverage.is_some() {
        Some(crate::health_types::CoverageModel::Istanbul)
    } else {
        Some(crate::health_types::CoverageModel::StaticEstimated)
    };

    if let Some(ref snapshot_path) = opts.save_snapshot {
        save_snapshot(
            opts,
            snapshot_path,
            &vital_signs,
            &counts,
            hotspot_summary.as_ref(),
            health_score.as_ref(),
            active_coverage_model,
        )?;
    }

    let health_trend = compute_health_trend(opts, &vital_signs, &counts, health_score.as_ref());

    // Assemble final report
    let coverage_gaps_has_findings = score_output
        .as_ref()
        .is_some_and(|output| !output.coverage.report.is_empty());

    // Build per-group output when `--group-by` resolved a resolver upfront.
    // The grouping borrows the post-baseline / post-truncation findings,
    // hotspots, file scores, large functions, and targets so each group's
    // data is consistent with the project-level report.
    let grouping = if let Some(ref resolver) = group_resolver {
        Some(grouping::build_health_grouping(
            resolver,
            &config.root,
            &files,
            &modules,
            &file_paths,
            &candidate_paths,
            score_output.as_ref(),
            file_scores_slice,
            &findings,
            &hotspots,
            &large_functions,
            &targets,
            opts.score,
            opts.score.then_some(&config.duplicates),
            needs_file_scores,
            opts.hotspots || opts.targets,
            !opts.score_only_output,
        ))
    } else {
        None
    };

    let report = assemble_health_report(
        opts,
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
        Some(HealthTimings {
            config_ms,
            discover_ms,
            parse_ms,
            complexity_ms,
            file_scores_ms,
            git_churn_ms,
            git_churn_cache_hit,
            hotspots_ms,
            duplication_ms,
            targets_ms,
            total_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
    } else {
        None
    };

    Ok(HealthResult {
        report,
        grouping,
        group_resolver,
        config,
        elapsed: start.elapsed(),
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps: enforce_coverage_gaps,
        baseline_active: opts.baseline.is_some() || opts.save_baseline.is_some(),
    })
}

fn apply_runtime_coverage_filters(
    report: &mut crate::health_types::RuntimeCoverageReport,
    baseline: Option<&HealthBaselineData>,
    root: &std::path::Path,
    top: Option<usize>,
    changed_files: Option<&FxHashSet<PathBuf>>,
) {
    if let Some(baseline) = baseline {
        report.findings = filter_new_runtime_coverage_findings(
            std::mem::take(&mut report.findings),
            baseline,
            root,
        );
    }

    if let Some(changed_files) = changed_files {
        report
            .hot_paths
            .retain(|hot_path| changed_files.contains(&hot_path.path));
    }

    refresh_runtime_coverage_verdict(report, changed_files.is_some());

    if let Some(top) = top {
        report.findings.truncate(top);
        report.hot_paths.truncate(top);
    }
}

fn refresh_runtime_coverage_verdict(
    report: &mut crate::health_types::RuntimeCoverageReport,
    changed_review: bool,
) {
    let has_cold_signal = report.findings.iter().any(|finding| {
        matches!(
            finding.verdict,
            crate::health_types::RuntimeCoverageVerdict::SafeToDelete
                | crate::health_types::RuntimeCoverageVerdict::ReviewRequired
                | crate::health_types::RuntimeCoverageVerdict::LowTraffic
        )
    });
    let has_changed_hot_path = changed_review && !report.hot_paths.is_empty();

    report.verdict = if matches!(
        report.verdict,
        crate::health_types::RuntimeCoverageReportVerdict::LicenseExpiredGrace
    ) || matches!(
        report.watermark,
        Some(crate::health_types::RuntimeCoverageWatermark::LicenseExpiredGrace)
    ) {
        crate::health_types::RuntimeCoverageReportVerdict::LicenseExpiredGrace
    } else if has_cold_signal {
        crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected
    } else if has_changed_hot_path {
        crate::health_types::RuntimeCoverageReportVerdict::HotPathTouched
    } else {
        crate::health_types::RuntimeCoverageReportVerdict::Clean
    };
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
    // Update duplicated_lines on both the snapshot counts and the embedded
    // vital signs counts so JSON consumers can see the raw numerator
    // alongside the percentage.
    counts.duplicated_lines = Some(dupes_report.stats.duplicated_lines);
    if let Some(ref mut vc) = vital_signs.counts {
        vc.duplicated_lines = Some(dupes_report.stats.duplicated_lines);
    }
}

/// Sort findings by the specified criteria.
fn sort_findings(findings: &mut [HealthFinding], sort: &SortBy) {
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
#[expect(
    clippy::too_many_arguments,
    reason = "filter pipeline requires all these inputs"
)]
fn compute_filtered_file_scores(
    config: &ResolvedConfig,
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    ignore_set: &globset::GlobSet,
    output: OutputFormat,
    istanbul_coverage: Option<&scoring::IstanbulCoverage>,
    pre_computed: Option<fallow_core::AnalysisOutput>,
) -> Result<FileScoreResult, ExitCode> {
    let analysis_output = if let Some(pre) = pre_computed {
        pre
    } else {
        fallow_core::analyze_with_parse_result(config, modules)
            .map_err(|e| emit_error(&format!("analysis failed: {e}"), 2, output))?
    };
    match compute_file_scores(
        modules,
        file_paths,
        changed_files,
        analysis_output,
        istanbul_coverage,
    ) {
        Ok(mut output) => {
            if let Some(ws) = ws_roots {
                output
                    .scores
                    .retain(|s| ws.iter().any(|r| s.path.starts_with(r)));
            }
            if !ignore_set.is_empty() {
                output.scores.retain(|s| {
                    let relative = s.path.strip_prefix(&config.root).unwrap_or(&s.path);
                    !ignore_set.is_match(relative)
                });
            }
            filter_coverage_gaps(
                &mut output.coverage.report,
                &mut output.coverage.runtime_paths,
                config,
                changed_files,
                ws_roots,
                ignore_set,
            );
            // Compute average BEFORE --top truncation so it reflects the full project
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
) -> (Vec<RefactoringTarget>, Option<TargetThresholds>) {
    if !opts.targets {
        return (Vec::new(), None);
    }
    let Some(output) = score_output else {
        return (Vec::new(), None);
    };
    let target_aux = TargetAuxData::from(output);
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
        path_in_health_scope(&item.path, config, changed_files, ws_roots, ignore_set)
    });
    coverage_gaps.exports.retain(|item| {
        path_in_health_scope(&item.path, config, changed_files, ws_roots, ignore_set)
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
#[expect(
    clippy::too_many_arguments,
    reason = "vital signs aggregate inputs from many pipeline stages"
)]
fn compute_vital_signs_and_counts(
    score_output: Option<&scoring::FileScoreOutput>,
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
    needs_file_scores: bool,
    file_scores_slice: &[FileHealthScore],
    needs_hotspots: bool,
    hotspots: &[HotspotEntry],
    total_files: usize,
    subset: &SubsetFilter<'_>,
) -> (
    crate::health_types::VitalSigns,
    crate::health_types::VitalSignsCounts,
) {
    let analysis_counts =
        score_output.map(|o| o.analysis_snapshot.counts_for(subset, &o.analysis_counts));
    let module_filter_set: Option<rustc_hash::FxHashSet<fallow_core::discover::FileId>> =
        if subset.is_full() {
            None
        } else {
            Some(
                modules
                    .iter()
                    .filter_map(|m| {
                        let path = file_paths.get(&m.file_id)?;
                        if subset.matches(path) {
                            Some(m.file_id)
                        } else {
                            None
                        }
                    })
                    .collect(),
            )
        };
    let vs_input = vital_signs::VitalSignsInput {
        modules,
        module_filter: module_filter_set.as_ref(),
        file_scores: if needs_file_scores {
            Some(file_scores_slice)
        } else {
            None
        },
        // Some(&[]) when pipeline ran but returned 0 results (-> hotspot_count: 0),
        // None when pipeline was not invoked (-> hotspot_count: null in snapshot).
        hotspots: if needs_hotspots { Some(hotspots) } else { None },
        total_files,
        analysis_counts,
    };
    let signs = vital_signs::compute_vital_signs(&vs_input);
    let counts = vital_signs::build_counts(&vs_input);
    (signs, counts)
}

/// Save a vital signs snapshot to disk if requested.
fn save_snapshot(
    opts: &HealthOptions<'_>,
    snapshot_path: &std::path::Path,
    vital_signs: &crate::health_types::VitalSigns,
    counts: &crate::health_types::VitalSignsCounts,
    hotspot_summary: Option<&crate::health_types::HotspotSummary>,
    health_score: Option<&crate::health_types::HealthScore>,
    coverage_model: Option<crate::health_types::CoverageModel>,
) -> Result<(), ExitCode> {
    let shallow = hotspot_summary.is_some_and(|s| s.shallow_clone);
    let snapshot = vital_signs::build_snapshot(
        vital_signs.clone(),
        counts.clone(),
        opts.root,
        shallow,
        health_score,
        coverage_model,
    );
    let explicit = if snapshot_path.as_os_str().is_empty() {
        None
    } else {
        Some(snapshot_path)
    };
    match vital_signs::save_snapshot(&snapshot, opts.root, explicit) {
        Ok(saved_path) => {
            if !opts.quiet {
                eprintln!("Saved vital signs snapshot to {}", saved_path.display());
            }
            Ok(())
        }
        Err(e) => Err(emit_error(&e, 2, opts.output)),
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
    findings: Vec<HealthFinding>,
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

/// Assemble the final `HealthReport` from all computed data.
fn assemble_health_report(
    opts: &HealthOptions<'_>,
    assembly: HealthReportAssembly,
) -> HealthReport {
    let HealthReportAssembly {
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
        has_istanbul_coverage,
        runtime_coverage,
        large_functions,
        sev_critical,
        sev_high,
        sev_moderate,
    } = assembly;
    let coverage_gaps = if report_coverage_gaps {
        score_output.as_ref().map(|o| o.coverage.report.clone())
    } else {
        None
    };

    // Extract Istanbul match stats before score_output is consumed
    let (ist_matched, ist_total) = score_output
        .as_ref()
        .map_or((0, 0), |o| (o.istanbul_matched, o.istanbul_total));

    // Extract file scores for the report (apply --top after hotspot/target computation)
    let file_scores = if opts.score_only_output {
        Vec::new()
    } else if opts.file_scores {
        let mut scores = score_output.map(|o| o.scores).unwrap_or_default();
        if let Some(top) = opts.top {
            scores.truncate(top);
        }
        scores
    } else {
        Vec::new()
    };

    // If hotspots were only computed for targets, don't include them in the report
    let (report_hotspots, report_hotspot_summary) = if opts.hotspots {
        (hotspots, hotspot_summary)
    } else {
        (Vec::new(), None)
    };

    let summary_files_scored = if opts.score_only_output || !opts.file_scores {
        None
    } else {
        files_scored
    };
    let summary_average_maintainability = if opts.score_only_output || !opts.file_scores {
        None
    } else {
        average_maintainability
    };
    let summary_coverage_model = if opts.score_only_output {
        None
    } else if opts.file_scores || report_coverage_gaps || opts.hotspots || opts.targets {
        Some(if has_istanbul_coverage {
            crate::health_types::CoverageModel::Istanbul
        } else {
            crate::health_types::CoverageModel::StaticEstimated
        })
    } else {
        None
    };
    let summary_istanbul_matched = if opts.score_only_output || !has_istanbul_coverage {
        None
    } else {
        Some(ist_matched)
    };
    let summary_istanbul_total = if opts.score_only_output || !has_istanbul_coverage {
        None
    } else {
        Some(ist_total)
    };

    HealthReport {
        summary: HealthSummary {
            files_analyzed,
            functions_analyzed: total_functions,
            functions_above_threshold: total_above_threshold,
            max_cyclomatic_threshold: max_cyclomatic,
            max_cognitive_threshold: max_cognitive,
            max_crap_threshold: max_crap,
            files_scored: summary_files_scored,
            average_maintainability: summary_average_maintainability,
            coverage_model: summary_coverage_model,
            istanbul_matched: summary_istanbul_matched,
            istanbul_total: summary_istanbul_total,
            severity_critical_count: sev_critical,
            severity_high_count: sev_high,
            severity_moderate_count: sev_moderate,
        },
        vital_signs: if opts.score_only_output {
            None
        } else {
            Some(vital_signs)
        },
        health_score,
        findings: if opts.complexity {
            findings
        } else {
            Vec::new()
        },
        file_scores,
        coverage_gaps: if opts.score_only_output {
            None
        } else {
            coverage_gaps
        },
        hotspots: report_hotspots,
        hotspot_summary: if opts.score_only_output {
            None
        } else {
            report_hotspot_summary
        },
        runtime_coverage,
        large_functions: if opts.score_only_output {
            Vec::new()
        } else {
            large_functions
        },
        targets: if opts.score_only_output {
            Vec::new()
        } else {
            targets
        },
        target_thresholds: if opts.score_only_output {
            None
        } else {
            target_thresholds
        },
        health_trend,
    }
}

/// Collect functions exceeding 60 LOC when the unit size risk profile warrants it.
///
/// Only populated when `very_high_risk >= 3%` in the unit size profile (same threshold
/// that triggers showing the risk profile line). Sorted by line count descending.
fn collect_large_functions(
    vital_signs: &crate::health_types::VitalSigns,
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
    config_root: &std::path::Path,
    ignore_set: &globset::GlobSet,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
) -> Vec<LargeFunctionEntry> {
    let dominated = vital_signs
        .unit_size_profile
        .as_ref()
        .is_some_and(|p| p.very_high_risk >= 3.0);
    if !dominated {
        return Vec::new();
    }

    let mut entries = Vec::new();
    for module in modules {
        let Some(&path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let relative = path.strip_prefix(config_root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        if let Some(changed) = changed_files
            && !changed.contains(path.as_path())
        {
            continue;
        }
        if let Some(ws) = ws_roots
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
fn build_ignore_set(patterns: &[String]) -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        match globset::Glob::new(pattern) {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(e) => {
                eprintln!("Warning: Invalid health ignore pattern '{pattern}': {e}");
            }
        }
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
) -> (Vec<HealthFinding>, usize, usize) {
    let mut files_analyzed = 0usize;
    let mut total_functions = 0usize;
    let mut findings: Vec<HealthFinding> = Vec::new();

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
                findings.push(HealthFinding {
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
                });
            }
        }
    }

    (findings, files_analyzed, total_functions)
}

/// Merge per-function CRAP data into an existing complexity findings vector.
///
/// Functions that only exceed `--max-crap` (without exceeding cyclomatic or
/// cognitive) become new findings. Functions that already produced a finding
/// for cyclomatic/cognitive get their `crap` and `coverage_pct` fields
/// populated, and the `exceeded` discriminant plus `severity` are recomputed
/// to reflect CRAP's contribution.
#[expect(
    clippy::too_many_arguments,
    reason = "CRAP merge needs the same filter pipeline as collect_findings"
)]
fn merge_crap_findings(
    findings: &mut Vec<HealthFinding>,
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
    config_root: &std::path::Path,
    ignore_set: &globset::GlobSet,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    per_function_crap: &rustc_hash::FxHashMap<std::path::PathBuf, Vec<scoring::PerFunctionCrap>>,
    max_crap: f64,
    max_cyclomatic: u16,
    max_cognitive: u16,
) {
    // Index existing findings by (path, line, col) so we can update in place.
    // The column is part of the key because curried arrows like
    // `(x) => (y) => {...}` produce two `FunctionComplexity` records on the
    // same line; keying on (path, line) alone would collide and let one
    // function's CRAP score be attached to the other function's identity.
    let finding_index: rustc_hash::FxHashMap<(std::path::PathBuf, u32, u32), usize> = findings
        .iter()
        .enumerate()
        .map(|(idx, f)| ((f.path.clone(), f.line, f.col), idx))
        .collect();
    // Build (path -> (line, col) -> &FunctionComplexity) for creating new
    // findings for CRAP-only violations. Same rationale: (line, col) is the
    // unique anchor for a function so nested arrows on the same line don't
    // overwrite each other.
    let mut complexity_by_pos: rustc_hash::FxHashMap<
        &std::path::Path,
        rustc_hash::FxHashMap<(u32, u32), &fallow_types::extract::FunctionComplexity>,
    > = rustc_hash::FxHashMap::default();
    for module in modules {
        let Some(&path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let entry = complexity_by_pos.entry(path.as_path()).or_default();
        for fc in &module.complexity {
            entry.insert((fc.line, fc.col), fc);
        }
    }
    // Track suppressions per file so we can honor `// fallow-ignore-*
    // complexity` for CRAP-only findings too.
    let suppressions_by_path: rustc_hash::FxHashMap<&std::path::Path, _> = modules
        .iter()
        .filter_map(|m| {
            file_paths
                .get(&m.file_id)
                .map(|p| (p.as_path(), &m.suppressions))
        })
        .collect();

    let mut new_findings: Vec<HealthFinding> = Vec::new();
    for (path, per_fn) in per_function_crap {
        // Apply the same filters as collect_findings so CRAP findings respect
        // `ignore`, `--changed-since`, and `--workspace`.
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

        for pf in per_fn {
            if pf.crap < max_crap {
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
                // Skip functions that already exceed cyclomatic/cognitive:
                // `collect_findings` would have already produced a finding
                // for them, so finding_index lookup above should have hit.
                // A missing entry here means the function passed those
                // thresholds but still exceeds CRAP on its own.
                let exceeds_cyclomatic = fc.cyclomatic > max_cyclomatic;
                let exceeds_cognitive = fc.cognitive > max_cognitive;
                new_findings.push(HealthFinding {
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
                });
            }
        }
    }
    findings.extend(new_findings);
}

/// Save health baseline to disk.
fn save_health_baseline(
    save_path: &std::path::Path,
    findings: &[HealthFinding],
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
    findings: &mut Vec<HealthFinding>,
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
    // `execute_health` validates `--group-by` early and produces per-group
    // output on `HealthResult.grouping`, so the explicit `build_ownership_resolver`
    // probe in this dispatcher is no longer needed.
    let result = match execute_health(opts) {
        Ok(r) => r,
        Err(code) => return code,
    };
    if let Some(ref timings) = result.timings {
        report::print_health_performance(timings, opts.output);
    }
    print_health_result(
        &result,
        opts.quiet,
        opts.explain,
        opts.min_score,
        opts.min_severity,
        opts.summary,
        true,
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
    /// True when this run was invoked with `--baseline` and/or
    /// `--save-baseline`. Used to gate JSON `suppress-line` action emission:
    /// teams managing suppressions through a baseline file should not be
    /// nudged toward inline `// fallow-ignore-next-line` comments, which
    /// become dead annotations once the baseline is regenerated.
    pub baseline_active: bool,
}

/// Compute the JSON `actions` injection options for a health result.
///
/// Returns the options that should drive `inject_health_actions` /
/// `build_health_json`: the same logic for standalone health, combined
/// mode, audit mode, and the programmatic NAPI surface, so every caller
/// produces identical JSON for identical inputs.
#[must_use]
pub fn health_action_opts(result: &HealthResult) -> crate::report::HealthActionOptions {
    if result.baseline_active {
        crate::report::HealthActionOptions {
            omit_suppress_line: true,
            omit_reason: Some("baseline-active"),
        }
    } else if !result.config.health.suggest_inline_suppression {
        crate::report::HealthActionOptions {
            omit_suppress_line: true,
            omit_reason: Some("config-disabled"),
        }
    } else {
        crate::report::HealthActionOptions::default()
    }
}

/// Print health results and return appropriate exit code.
pub fn print_health_result(
    result: &HealthResult,
    quiet: bool,
    explain: bool,
    min_score: Option<f64>,
    min_severity: Option<FindingSeverity>,
    summary: bool,
    show_explain_tip: bool,
) -> ExitCode {
    let ctx = report::ReportContext {
        root: &result.config.root,
        rules: &result.config.rules,
        elapsed: result.elapsed,
        quiet,
        explain,
        group_by: None,
        top: None,
        summary,
        show_explain_tip,
        baseline_matched: None,
        health_action_opts: health_action_opts(result),
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

    // Check --min-score threshold
    if let Some(threshold) = min_score
        && let Some(ref hs) = result.report.health_score
        && hs.score < threshold
    {
        if !quiet {
            eprintln!(
                "Health score {:.1} ({}) is below minimum threshold {:.0}",
                hs.score, hs.grade, threshold
            );
        }
        return ExitCode::from(1);
    }

    // Check findings against --min-severity filter
    let has_failing_findings = if let Some(min_sev) = min_severity {
        result.report.findings.iter().any(|f| f.severity >= min_sev)
    } else {
        !result.report.findings.is_empty()
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
    if has_failing_findings || has_failing_runtime_coverage {
        return ExitCode::from(1);
    }

    if result.should_fail_on_coverage_gaps && result.coverage_gaps_has_findings {
        return ExitCode::from(1);
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
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            content_hash: 0,
            suppressions: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            line_offsets: vec![0],
            complexity,
            flag_uses: vec![],
            class_heritage: vec![],
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
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
        }
    }

    // ── build_ignore_set ────────────────────────────────────────

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
    fn build_ignore_set_skips_invalid_patterns() {
        // "[invalid" is not a valid glob — should be skipped, not panic
        let patterns = vec!["[invalid".to_string(), "*.js".to_string()];
        let set = build_ignore_set(&patterns);
        // The valid pattern should still work
        assert!(set.is_match(Path::new("foo.js")));
    }

    fn make_finding(name: &str, exceeded: ExceededThreshold) -> HealthFinding {
        HealthFinding {
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

    // ── collect_findings ────────────────────────────────────────

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
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].name, "fnA");
        assert_eq!(files, 1);
    }

    #[test]
    fn collect_findings_skips_module_without_path() {
        // Module with FileId(99) has no entry in file_paths
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
        );
        assert!(findings.is_empty());
        assert_eq!(files, 0);
    }

    #[test]
    fn collect_findings_at_exact_threshold_not_reported() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            // Exactly at thresholds — should NOT be reported (> not >=)
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

    // Regression test for issue #181: curried arrows like
    // `(x) => (y) => {...}` produce two FunctionComplexity records on the
    // same line. Indexing CRAP findings by (path, line) alone collided so
    // the inner arrow's CRAP score was attached to the outer arrow's
    // identity (CC=1, CRAP=56, mathematically impossible because CRAP at
    // 0% coverage is CC^2 + CC).
    #[test]
    fn merge_crap_findings_disambiguates_same_line_functions() {
        let path = PathBuf::from("/project/src/curried.ts");
        // Outer arrow: cyclomatic=1, body is just `return inner_arrow`.
        // Inner arrow: cyclomatic=7, body has the switch cases.
        let outer = FunctionComplexity {
            name: "handler".to_string(),
            line: 1,
            col: 23,
            cyclomatic: 1,
            cognitive: 0,
            line_count: 11,
            param_count: 1,
        };
        let inner = FunctionComplexity {
            name: "<arrow>".to_string(),
            line: 1,
            col: 43,
            cyclomatic: 7,
            cognitive: 0,
            line_count: 10,
            param_count: 1,
        };
        // Inner is pushed first because pop_function() is LIFO; matches the
        // production order out of `complexity.rs`.
        let modules = vec![make_module(FileId(0), vec![inner.clone(), outer.clone()])];
        let mut file_paths: FxHashMap<FileId, &PathBuf> = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        // Neither function exceeds the cyclomatic (20) or cognitive (15)
        // thresholds, so collect_findings would emit zero findings. The CRAP
        // pass is what surfaces the inner arrow.
        let mut findings: Vec<HealthFinding> = Vec::new();

        let mut per_function_crap: FxHashMap<PathBuf, Vec<scoring::PerFunctionCrap>> =
            FxHashMap::default();
        per_function_crap.insert(
            path.clone(),
            vec![
                // Order matches `module.complexity` order (inner first).
                scoring::PerFunctionCrap {
                    line: inner.line,
                    col: inner.col,
                    crap: 56.0,
                    coverage_pct: None,
                    coverage_tier: crate::health_types::CoverageTier::None,
                },
                scoring::PerFunctionCrap {
                    line: outer.line,
                    col: outer.col,
                    crap: 2.0,
                    coverage_pct: None,
                    coverage_tier: crate::health_types::CoverageTier::None,
                },
            ],
        );

        merge_crap_findings(
            &mut findings,
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            &per_function_crap,
            30.0,
            20,
            15,
        );

        // Only the inner arrow's CRAP exceeds the threshold (56 >= 30); the
        // outer arrow's CRAP (2) does not. So exactly one finding emitted.
        assert_eq!(
            findings.len(),
            1,
            "expected one CRAP finding for inner arrow"
        );
        let f = &findings[0];
        // The bug attached inner CRAP (56) to outer identity (handler/CC=1).
        // After the fix, identity and CRAP must come from the same function.
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
        // Sanity check: CRAP at 0% coverage equals CC^2 + CC. CC=7 implies 56.
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

    // Companion regression test: when the OUTER arrow is the one above the
    // CRAP threshold (i.e. its CC is large), the OUTER's identity must be
    // reported with the OUTER's CRAP, even though both arrows still sit on
    // the same line.
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
        };
        let inner = FunctionComplexity {
            name: "<arrow>".to_string(),
            line: 5,
            col: 30,
            cyclomatic: 1,
            cognitive: 0,
            line_count: 1,
            param_count: 1,
        };
        let modules = vec![make_module(FileId(0), vec![inner.clone(), outer.clone()])];
        let mut file_paths: FxHashMap<FileId, &PathBuf> = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let mut findings: Vec<HealthFinding> = Vec::new();
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
                },
                scoring::PerFunctionCrap {
                    line: outer.line,
                    col: outer.col,
                    crap: 72.0,
                    coverage_pct: None,
                    coverage_tier: crate::health_types::CoverageTier::None,
                },
            ],
        );

        merge_crap_findings(
            &mut findings,
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            &per_function_crap,
            30.0,
            20,
            15,
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
            target_keys: vec![],
        };
        let mut report = crate::health_types::RuntimeCoverageReport {
            verdict: crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected,
            summary: fx_summary(3, 0, 2, 1),
            findings: vec![
                crate::health_types::RuntimeCoverageFinding {
                    id: "fallow:prod:aaaaaaaa".to_owned(),
                    path: PathBuf::from("/project/src/a.ts"),
                    function: "alpha".to_owned(),
                    line: 10,
                    verdict: crate::health_types::RuntimeCoverageVerdict::ReviewRequired,
                    invocations: Some(0),
                    confidence: crate::health_types::RuntimeCoverageConfidence::Medium,
                    evidence: fx_evidence("used", "not_covered", "tracked"),
                    actions: vec![],
                },
                crate::health_types::RuntimeCoverageFinding {
                    id: "fallow:prod:bbbbbbbb".to_owned(),
                    path: PathBuf::from("/project/src/b.ts"),
                    function: "beta".to_owned(),
                    line: 20,
                    verdict: crate::health_types::RuntimeCoverageVerdict::CoverageUnavailable,
                    invocations: None,
                    confidence: crate::health_types::RuntimeCoverageConfidence::None,
                    evidence: fx_evidence("used", "not_covered", "untracked"),
                    actions: vec![],
                },
                crate::health_types::RuntimeCoverageFinding {
                    id: "fallow:prod:cccccccc".to_owned(),
                    path: PathBuf::from("/project/src/c.ts"),
                    function: "gamma".to_owned(),
                    line: 30,
                    verdict: crate::health_types::RuntimeCoverageVerdict::ReviewRequired,
                    invocations: Some(0),
                    confidence: crate::health_types::RuntimeCoverageConfidence::Medium,
                    evidence: fx_evidence("used", "not_covered", "tracked"),
                    actions: vec![],
                },
            ],
            hot_paths: vec![
                crate::health_types::RuntimeCoverageHotPath {
                    id: "fallow:hot:11111111".to_owned(),
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

        apply_runtime_coverage_filters(&mut report, Some(&baseline), root, Some(1), None);

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
            target_keys: vec![],
        };
        let mut report = crate::health_types::RuntimeCoverageReport {
            verdict: crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected,
            summary: fx_summary(2, 1, 1, 0),
            findings: vec![crate::health_types::RuntimeCoverageFinding {
                id: "fallow:prod:aaaaaaaa".to_owned(),
                path: PathBuf::from("/project/src/a.ts"),
                function: "alpha".to_owned(),
                line: 10,
                verdict: crate::health_types::RuntimeCoverageVerdict::ReviewRequired,
                invocations: Some(0),
                confidence: crate::health_types::RuntimeCoverageConfidence::Medium,
                evidence: fx_evidence("used", "not_covered", "tracked"),
                actions: vec![],
            }],
            hot_paths: vec![],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
        };

        apply_runtime_coverage_filters(&mut report, Some(&baseline), root, None, None);

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
            verdict: crate::health_types::RuntimeCoverageReportVerdict::Clean,
            summary: fx_summary(2, 2, 0, 0),
            findings: vec![],
            hot_paths: vec![crate::health_types::RuntimeCoverageHotPath {
                id: "fallow:hot:33333333".to_owned(),
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

        apply_runtime_coverage_filters(&mut report, None, root, None, Some(&changed_files));

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
            verdict: crate::health_types::RuntimeCoverageReportVerdict::Clean,
            summary: fx_summary(2, 2, 0, 0),
            findings: vec![],
            hot_paths: vec![crate::health_types::RuntimeCoverageHotPath {
                id: "fallow:hot:44444444".to_owned(),
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

        apply_runtime_coverage_filters(&mut report, None, root, None, Some(&changed_files));

        assert!(report.hot_paths.is_empty());
        assert_eq!(
            report.verdict,
            crate::health_types::RuntimeCoverageReportVerdict::Clean
        );
    }

    #[test]
    fn print_health_result_fails_on_low_traffic_runtime_coverage() {
        let result = HealthResult {
            report: crate::health_types::HealthReport {
                runtime_coverage: Some(crate::health_types::RuntimeCoverageReport {
                    verdict: crate::health_types::RuntimeCoverageReportVerdict::ColdCodeDetected,
                    summary: fx_summary(1, 0, 1, 0),
                    findings: vec![crate::health_types::RuntimeCoverageFinding {
                        id: "fallow:prod:lowtraffic".to_owned(),
                        path: PathBuf::from("/project/src/cold.ts"),
                        function: "coldPath".to_owned(),
                        line: 14,
                        verdict: crate::health_types::RuntimeCoverageVerdict::LowTraffic,
                        invocations: Some(1),
                        confidence: crate::health_types::RuntimeCoverageConfidence::Low,
                        evidence: fx_evidence("used", "not_covered", "tracked"),
                        actions: vec![],
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
            baseline_active: false,
        };

        assert_eq!(
            print_health_result(&result, true, false, None, None, false, true),
            ExitCode::from(1),
        );
    }

    fn test_health_result(baseline_active: bool, suggest_inline_suppression: bool) -> HealthResult {
        let mut config = test_resolved_config();
        config.health.suggest_inline_suppression = suggest_inline_suppression;
        HealthResult {
            report: crate::health_types::HealthReport::default(),
            config,
            elapsed: Duration::default(),
            timings: None,
            coverage_gaps_has_findings: false,
            should_fail_on_coverage_gaps: false,
            baseline_active,
            grouping: None,
            group_resolver: None,
        }
    }

    #[test]
    fn health_action_opts_default_keeps_suppress_line() {
        let opts = health_action_opts(&test_health_result(false, true));
        assert!(!opts.omit_suppress_line);
        assert_eq!(opts.omit_reason, None);
    }

    #[test]
    fn health_action_opts_baseline_active_omits_suppress_line() {
        let opts = health_action_opts(&test_health_result(true, true));
        assert!(opts.omit_suppress_line);
        assert_eq!(opts.omit_reason, Some("baseline-active"));
    }

    #[test]
    fn health_action_opts_config_disabled_omits_suppress_line() {
        let opts = health_action_opts(&test_health_result(false, false));
        assert!(opts.omit_suppress_line);
        assert_eq!(opts.omit_reason, Some("config-disabled"));
    }

    #[test]
    fn health_action_opts_baseline_wins_over_config_for_reason() {
        // When both signals fire, baseline takes precedence in the reason
        // string. The behaviour (omit_suppress_line) is identical, but the
        // reason is the more actionable hint for an operator wondering why
        // the actions array is missing suppress-line.
        let opts = health_action_opts(&test_health_result(true, false));
        assert!(opts.omit_suppress_line);
        assert_eq!(opts.omit_reason, Some("baseline-active"));
    }

    #[test]
    fn baseline_active_set_when_save_baseline_used_alone() {
        // Regression guard: --save-baseline alone (without --baseline) is
        // ALSO a "managing suppressions through baseline" intent and must
        // omit suppress-line so the regenerated baseline is the only
        // suppression mechanism.
        // Smoke-test the propagation by constructing HealthResult directly,
        // since execute_health is integration-tested separately.
        let result = HealthResult {
            report: crate::health_types::HealthReport::default(),
            config: test_resolved_config(),
            elapsed: Duration::default(),
            timings: None,
            coverage_gaps_has_findings: false,
            should_fail_on_coverage_gaps: false,
            baseline_active: true,
            grouping: None,
            group_resolver: None,
        };
        assert!(health_action_opts(&result).omit_suppress_line);
    }
}
