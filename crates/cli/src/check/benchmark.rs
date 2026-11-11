//! The dead-code JSON report that the CLI benchmarks render.

use super::*;

pub fn benchmark_dead_code_json(
    root: &std::path::Path,
    threads: usize,
) -> Result<(usize, usize), ExitCode> {
    let config_path = None;
    let filters = IssueFilters::default();
    let trace_opts = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: None,
        impact_closure: None,
        symbol_impact: None,
        performance: false,
    };
    let result = execute_check(&CheckOptions {
        root,
        config_path: &config_path,
        output: OutputFormat::Json,
        json_style: crate::json_style::JsonStyle::Compact,
        no_cache: true,
        threads,
        quiet: true,
        allow_remote_extends: false,
        fail_on_issues: false,
        filters: &filters,
        changed_since: None,
        change_scope_owner: fallow_engine::change_scope::ChangeScopeOwner::Run,
        no_package_baselines: false,
        diff_index: None,
        use_shared_diff_index: true,
        baseline: None,
        baseline_flag: "--baseline",
        bare_run: false,
        save_baseline: None,
        fail_on_stale_baseline: false,
        sarif_file: None,
        production: false,
        production_override: Some(false),
        workspace: None,
        changed_workspaces: None,
        group_by: None,
        include_dupes: false,
        type_aware: None,
        type_aware_config_override: None,
        type_aware_projects: &[],
        type_aware_require: None,
        trace_opts: &trace_opts,
        explain: false,
        top: None,
        file: &[],
        scope: None,
        finding_ids: None,
        include_entry_exports: false,
        show_cascade: false,
        fail_on_parse_error: false,
        summary: false,
        regression_opts: RegressionOpts {
            fail_on_regression: false,
            tolerance: crate::regression::Tolerance::Absolute(0),
            regression_baseline_file: None,
            save_target: crate::regression::SaveRegressionTarget::None,
            scoped: false,
            quiet: true,
            output: OutputFormat::Json,
        },
        retain_modules_for_health: false,
        defer_performance: false,
        analysis_snapshot: fallow_config::AnalysisSnapshot::Current,
        explain_skipped: false,
        surface: CheckSurface::DeadCode,
    })?;
    let rendered = report::render_check_json(&report::CheckJsonRenderInput {
        package_baselines: &result.package_baselines,
        results: &result.results,
        root: &result.config.root,
        elapsed: result.elapsed,
        type_aware: result.type_aware_meta.as_ref(),
        regression: result.regression.as_ref(),
        baseline_matched: result.baseline_matched,
        baseline_staleness: envelope_baseline_staleness(&result),
        gate_outcomes: None,
        config_fixable: result.config_fixable,
        workspace_diagnostics: &result.workspace_diagnostics,
        json_style: crate::json_style::JsonStyle::Compact,
    })
    .map_err(|_| exit_invalid_input())?;
    Ok((result.results.total_issues(), rendered.len()))
}
