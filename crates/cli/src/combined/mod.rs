use std::process::ExitCode;
use std::time::Instant;

use fallow_config::OutputFormat;

use crate::check::{CheckOptions, CheckResult, IssueFilters, TraceOptions};
use crate::dupes::{DupesMode, DupesOptions, DupesResult};
use crate::health::{HealthOptions, HealthResult, SortBy};
use crate::regression;
use crate::report;
use crate::{AnalysisKind, load_config_for_analysis};

mod impact;
mod orientation;
mod output;

pub use orientation::print_entry_point_summary;

use impact::{record_combined_cache_state, record_combined_impact};
use output::{handle_regression_and_summary, print_combined_report};

pub struct CombinedOptions<'a> {
    pub root: &'a std::path::Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub fail_on_issues: bool,
    pub sarif_file: Option<&'a std::path::Path>,
    pub changed_since: Option<&'a str>,
    /// Import churn from a `fallow-churn/v1` file (`--churn-file`) for the
    /// health hotspots / ownership pass instead of `git log`. Resolved relative
    /// to `root` inside the health pipeline.
    pub churn_file: Option<&'a std::path::Path>,
    pub baseline: Option<&'a std::path::Path>,
    pub save_baseline: Option<&'a std::path::Path>,
    pub production: bool,
    pub production_dead_code: Option<bool>,
    pub production_health: Option<bool>,
    pub production_dupes: Option<bool>,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub group_by: Option<crate::GroupBy>,
    pub explain: bool,
    pub explain_skipped: bool,
    pub performance: bool,
    pub summary: bool,
    pub run_check: bool,
    pub run_dupes: bool,
    pub run_health: bool,
    pub dupes_mode: Option<DupesMode>,
    pub dupes_threshold: Option<f64>,
    pub dupes_min_tokens: Option<usize>,
    pub dupes_min_lines: Option<usize>,
    pub dupes_min_occurrences: Option<usize>,
    pub dupes_skip_local: bool,
    pub dupes_cross_language: bool,
    pub dupes_ignore_imports: bool,
    pub score: bool,
    pub trend: bool,
    pub save_snapshot: Option<&'a Option<String>>,
    pub include_entry_exports: bool,
    pub regression_opts: regression::RegressionOpts<'a>,
}

/// Resolve which analyses to run based on --only/--skip flags.
/// Precondition: only and skip must not both be non-empty (validated in main.rs).
pub fn resolve_analyses(only: &[AnalysisKind], skip: &[AnalysisKind]) -> (bool, bool, bool) {
    if !only.is_empty() {
        (
            only.contains(&AnalysisKind::DeadCode),
            only.contains(&AnalysisKind::Dupes),
            only.contains(&AnalysisKind::Health),
        )
    } else if !skip.is_empty() {
        (
            !skip.contains(&AnalysisKind::DeadCode),
            !skip.contains(&AnalysisKind::Dupes),
            !skip.contains(&AnalysisKind::Health),
        )
    } else {
        (true, true, true)
    }
}

pub fn run_combined(opts: &CombinedOptions<'_>) -> ExitCode {
    let start = Instant::now();
    let mut check_result: Option<CheckResult> = None;
    let mut dupes_result: Option<DupesResult> = None;
    let mut health_result: Option<HealthResult> = None;

    let filters = IssueFilters::default();
    let trace_opts = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: None,
        performance: opts.performance,
    };
    let check_opts = if opts.run_check {
        Some(CheckOptions {
            root: opts.root,
            config_path: opts.config_path,
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            quiet: opts.quiet,
            fail_on_issues: opts.fail_on_issues,
            filters: &filters,
            changed_since: opts.changed_since,
            diff_index: None,
            use_shared_diff_index: true,
            baseline: opts.baseline,
            save_baseline: opts.save_baseline,
            sarif_file: opts.sarif_file,
            production: opts.production_dead_code.unwrap_or(opts.production),
            production_override: opts.production_dead_code,
            workspace: opts.workspace,
            changed_workspaces: opts.changed_workspaces,
            group_by: opts.group_by,
            include_dupes: false,
            trace_opts: &trace_opts,
            explain: opts.explain,
            top: None,
            file: &[],
            include_entry_exports: opts.include_entry_exports,
            summary: opts.summary,
            regression_opts: opts.regression_opts,
            retain_modules_for_health: opts.run_health,
            defer_performance: true,
        })
    } else {
        None
    };

    if let (Some(check_opts), true) = (check_opts.as_ref(), opts.run_dupes) {
        let (check_res, dupes_res) = rayon::join(
            || crate::check::execute_check(check_opts),
            || run_combined_dupes(opts, None),
        );
        match check_res {
            Ok(result) => check_result = Some(result),
            Err(code) => return code,
        }
        match dupes_res {
            Ok(result) => dupes_result = result,
            Err(code) => return code,
        }
    } else {
        if let Some(check_opts) = check_opts.as_ref() {
            match crate::check::execute_check(check_opts) {
                Ok(result) => check_result = Some(result),
                Err(code) => return code,
            }
        }
        if opts.run_dupes {
            match run_combined_dupes(opts, check_result.as_ref()) {
                Ok(result) => dupes_result = result,
                Err(code) => return code,
            }
        }
    }

    record_combined_cache_state(opts, check_result.as_ref());

    if opts.performance
        && let Some(ref mut check) = check_result
        && let Some(ref mut timings) = check.timings
    {
        timings.duplication_ms = dupes_result
            .as_ref()
            .map(|dupes| dupes.elapsed.as_secs_f64() * 1000.0);
        report::print_performance(timings, opts.output);
    }

    if opts.run_health {
        let health_opts = build_health_opts(opts);
        let check_production = opts.production_dead_code.unwrap_or(opts.production);
        let health_production = opts.production_health.unwrap_or(opts.production);
        let shared = if check_production == health_production {
            check_result.as_mut().and_then(|r| r.shared_parse.take())
        } else {
            None
        };
        let health_run = if let Some(shared_data) = shared {
            crate::health::execute_health_with_shared_parse(&health_opts, shared_data)
        } else {
            crate::health::execute_health(&health_opts)
        };
        match health_run {
            Ok(result) => {
                health_result = Some(result);
            }
            Err(code) => return code,
        }
    }

    let total_elapsed = start.elapsed();

    let mut max_exit = match print_combined_report(
        opts,
        check_result.as_ref(),
        dupes_result.as_ref(),
        health_result.as_ref(),
        total_elapsed,
    ) {
        Ok(exit) => exit,
        Err(code) => return code,
    };

    handle_regression_and_summary(
        &mut max_exit,
        opts.quiet,
        opts.root,
        check_result.as_ref(),
        dupes_result.as_ref(),
        health_result.as_ref(),
    );

    record_combined_impact(
        opts,
        check_result.as_ref(),
        dupes_result.as_ref(),
        health_result.as_ref(),
    );

    ExitCode::from(max_exit)
}

/// Build the dupes options and dispatch to either `execute_dupes` or
/// `execute_dupes_with_files` depending on whether dead-code already produced
/// a reusable file list (set when both health and dupes share dead-code's
/// production setting). Extracted out of `run_combined` to keep that function
/// under the cognitive-complexity / line-count limits.
fn run_combined_dupes(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
) -> Result<Option<DupesResult>, ExitCode> {
    let dupes_cfg = load_config_for_analysis(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production_dupes
            .or_else(|| opts.production.then_some(true)),
        opts.quiet,
        fallow_config::ProductionAnalysis::Dupes,
    )?
    .duplicates;

    let dupes_opts = DupesOptions {
        root: opts.root,
        config_path: opts.config_path,
        output: opts.output,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        mode: Some(
            opts.dupes_mode
                .unwrap_or_else(|| DupesMode::from(dupes_cfg.mode)),
        ),
        min_tokens: Some(opts.dupes_min_tokens.unwrap_or(dupes_cfg.min_tokens)),
        min_lines: Some(opts.dupes_min_lines.unwrap_or(dupes_cfg.min_lines)),
        min_occurrences: Some(
            opts.dupes_min_occurrences
                .unwrap_or(dupes_cfg.min_occurrences),
        ),
        threshold: Some(opts.dupes_threshold.unwrap_or(dupes_cfg.threshold)),
        skip_local: opts.dupes_skip_local || dupes_cfg.skip_local,
        cross_language: opts.dupes_cross_language || dupes_cfg.cross_language,
        ignore_imports: opts.dupes_ignore_imports || dupes_cfg.ignore_imports,
        top: None,
        baseline_path: None,
        save_baseline_path: None,
        production: opts.production_dupes.unwrap_or(opts.production),
        production_override: opts.production_dupes,
        trace: None,
        changed_since: opts.changed_since,
        diff_index: None,
        use_shared_diff_index: true,
        changed_files: None,
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        explain: opts.explain,
        explain_skipped: opts.explain_skipped,
        summary: opts.summary,
        group_by: opts.group_by,
        performance: false,
    };

    let check_production = opts.production_dead_code.unwrap_or(opts.production);
    let health_production = opts.production_health.unwrap_or(opts.production);
    let dupes_production = opts.production_dupes.unwrap_or(opts.production);
    let share_files_with_dupes = opts.run_health
        && check_production == health_production
        && check_production == dupes_production;
    let dupes_files = if share_files_with_dupes {
        check_result.and_then(|r| r.shared_parse.as_ref().map(|sp| sp.files.clone()))
    } else {
        None
    };

    let dupes_run = if let Some(files) = dupes_files {
        crate::dupes::execute_dupes_with_files(&dupes_opts, files)
    } else {
        crate::dupes::execute_dupes(&dupes_opts)
    };
    dupes_run.map(Some)
}

fn build_health_opts<'a>(opts: &'a CombinedOptions<'a>) -> HealthOptions<'a> {
    HealthOptions {
        root: opts.root,
        config_path: opts.config_path,
        output: opts.output,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        max_cyclomatic: None,
        max_cognitive: None,
        max_crap: None,
        top: None,
        sort: SortBy::Cyclomatic,
        production: opts.production_health.unwrap_or(opts.production),
        production_override: opts.production_health,
        changed_since: opts.changed_since,
        diff_index: None,
        use_shared_diff_index: true,
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        baseline: None,
        save_baseline: None,
        complexity: true,
        complexity_breakdown: false,
        file_scores: true,
        coverage_gaps: false,
        config_activates_coverage_gaps: false,
        hotspots: true,
        ownership: false,
        ownership_emails: None,
        targets: true,
        force_full: false,
        score_only_output: false,
        enforce_coverage_gap_gate: false,
        effort: None,
        score: opts.score || opts.trend,
        min_score: None,
        since: None,
        min_commits: None,
        explain: opts.explain,
        summary: opts.summary,
        save_snapshot: opts
            .save_snapshot
            .map(|opt| std::path::PathBuf::from(opt.as_deref().unwrap_or_default())),
        trend: opts.trend,
        group_by: opts.group_by,
        coverage: None,
        coverage_root: None,
        performance: opts.performance,
        min_severity: None,
        report_only: false,
        runtime_coverage: None,
        churn_file: opts.churn_file,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::ExitCode;

    use crate::AnalysisKind;

    use super::orientation::is_test_path;
    use super::output::exit_code_to_u8;
    use super::resolve_analyses;

    #[test]
    fn resolve_analyses_defaults_to_all_and_honors_only() {
        assert_eq!(resolve_analyses(&[], &[]), (true, true, true));
        assert_eq!(
            resolve_analyses(&[AnalysisKind::DeadCode, AnalysisKind::Health], &[]),
            (true, false, true)
        );
        assert_eq!(
            resolve_analyses(&[AnalysisKind::Dupes], &[]),
            (false, true, false)
        );
    }

    #[test]
    fn resolve_analyses_honors_skip_when_only_is_empty() {
        assert_eq!(
            resolve_analyses(&[], &[AnalysisKind::Dupes]),
            (true, false, true)
        );
        assert_eq!(
            resolve_analyses(&[], &[AnalysisKind::DeadCode, AnalysisKind::Health]),
            (false, true, false)
        );
    }

    #[test]
    fn test_path_filter_recognizes_directories_and_filename_markers() {
        for path in [
            "src/__tests__/button.ts",
            "src/fixtures/data.ts",
            "apps/web/e2e/login.ts",
            "src/components/button.test.ts",
            "src/components/button.stories.tsx",
            "src/components/button.fixture.ts",
            "src/a12.ts",
        ] {
            assert!(is_test_path(Path::new(path)), "{path} should be test-like");
        }

        for path in [
            "src/components/button.ts",
            "src/routes/story.ts",
            "src/api/version.ts",
        ] {
            assert!(
                !is_test_path(Path::new(path)),
                "{path} should be production-like"
            );
        }
    }

    #[test]
    fn exit_code_to_u8_distinguishes_success_from_failure() {
        assert_eq!(exit_code_to_u8(ExitCode::SUCCESS), 0);
        assert_eq!(exit_code_to_u8(ExitCode::from(1)), 1);
        assert_eq!(exit_code_to_u8(ExitCode::from(2)), 1);
    }
}
