//! The runtime coverage step of `fallow security --runtime-coverage`.
//!
//! The step runs the health analysis over the files and modules of the
//! dead-code analysis of the security run, and returns the runtime coverage
//! report that ranks the security candidates.

use std::path::Path;
use std::process::ExitCode;

use fallow_output::RuntimeCoverageReport;
use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::ModuleInfo;

use super::SecurityOptions;
use crate::health::HealthOptions;

/// The runtime coverage report of the capture at `path`, over the files and
/// modules of the dead-code analysis of the run.
pub(super) fn analyze_security_runtime(
    opts: &SecurityOptions<'_>,
    path: &Path,
    modules: Vec<ModuleInfo>,
    files: Vec<DiscoveredFile>,
    analysis_output: fallow_engine::dead_code::DeadCodeAnalysisArtifacts,
) -> Result<Option<RuntimeCoverageReport>, ExitCode> {
    let runtime_coverage = crate::health::coverage::prepare_options(
        path,
        opts.min_invocations_hot,
        None,
        None,
        opts.output,
    )?;
    let health_opts = security_runtime_health_options(opts, runtime_coverage);
    let (config, config_ms) = security_runtime_health_config(opts, &health_opts)?;
    let result = crate::health::execute_health_with_shared_parse_and_config(
        &health_opts,
        fallow_engine::health::HealthSharedParseData {
            files,
            modules,
            dead_code_results: None,
            workspaces: Vec::new(),
            analysis_output: Some(analysis_output),
        },
        None,
        config,
        config_ms,
    )?;
    Ok(result.report.runtime_coverage)
}

/// The config of the runtime coverage step of `fallow security`: the config
/// of the dead-code analysis of the run. The step reuses the files and
/// modules of that analysis, so it uses the same production mode. The config
/// also keeps the environment of the run, for example `FALLOW_CACHE_DIR`.
fn security_runtime_health_config(
    opts: &SecurityOptions<'_>,
    health_opts: &HealthOptions<'_>,
) -> Result<(fallow_config::ResolvedConfig, f64), ExitCode> {
    let runnable = crate::cli_production::dead_code_runnable(opts.run, opts.output)?;
    crate::health::load_health_config_in(health_opts, Some(runnable))
}

/// Build the `HealthOptions` used to compute runtime coverage context for
/// security findings (complexity/hotspot/ownership all disabled), in the
/// production mode of the dead-code analysis of the run.
fn security_runtime_health_options<'a>(
    opts: &SecurityOptions<'a>,
    runtime_coverage: fallow_engine::health::RuntimeCoverageOptions,
) -> HealthOptions<'a> {
    let production = *opts
        .run
        .production(fallow_config::PlannedAnalysis::DeadCode)
        .value();
    HealthOptions {
        root: opts.root,
        config_path: opts.run.config_file().value(),
        output: opts.output,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        thresholds: fallow_engine::health::HealthThresholdOverrides::default(),
        top: None,
        sort: fallow_engine::health::HealthSort::Cyclomatic,
        production,
        production_override: Some(production),
        allow_remote_extends: opts.allow_remote_extends,
        changed_since: opts.changed_since,
        diff_index: None,
        use_shared_diff_index: opts.use_shared_diff_index,
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        baseline: None,
        save_baseline: None,
        baseline_mode: fallow_engine::baseline::HealthBaselineMode::default(),
        baseline_mode_explicit: false,
        complexity: false,
        file_scores: false,
        coverage_gaps: false,
        config_activates_coverage_gaps: false,
        hotspots: false,
        ownership: false,
        ownership_emails: None,
        targets: false,
        css: false,
        css_deep: false,
        force_full: false,
        score_only_output: false,
        enforce_coverage_gap_gate: false,
        effort: None,
        score: false,
        gates: fallow_engine::health::HealthGateOptions::default(),
        since: None,
        min_commits: None,
        explain: false,
        summary: false,
        save_snapshot: None,
        trend: false,
        coverage_inputs: fallow_engine::health::HealthCoverageInputs::default(),
        performance: false,
        runtime_coverage: Some(runtime_coverage),
        churn_file: None,
        analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity::default(),
        complexity_breakdown: false,
        group_by: None,
        group_filter: None,
        trend_from: None,
        scope: opts.scope.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::OutputFormat;

    /// A security run of `root` with `env` as the environment of the run.
    fn run_with_env(
        root: &Path,
        env: &[(&str, &str)],
    ) -> &'static fallow_engine::resolved_run::ResolvedRun {
        use clap::Parser as _;
        let root_arg = root.display().to_string();
        let cli = crate::cli_args::Cli::try_parse_from(["fallow", "security", "--root", &root_arg])
            .expect("argv parses");
        let env = fallow_config::EnvSnapshot::from_pairs(env, None);
        let inputs =
            crate::run_inputs::command_run_inputs(&cli, env).expect("security resolves a run");
        let run = fallow_engine::resolved_run::resolve_run(&inputs).expect("fixture run");
        Box::leak(Box::new(run))
    }

    #[test]
    fn runtime_health_step_reads_the_cache_dir_of_the_run() {
        let root = super::super::tests::leak_fixture_root();
        let cache = tempfile::tempdir().expect("tempdir");
        let cache_dir = cache.path().join("run-cache");
        let cache_arg = cache_dir.display().to_string();
        let mut opts = super::super::tests::run_opts(&root, OutputFormat::Json, false);
        opts.run = run_with_env(&root, &[("FALLOW_CACHE_DIR", &cache_arg)]);
        let runtime_coverage = fallow_engine::health::RuntimeCoverageOptions {
            path: root.join("coverage.json"),
            min_invocations_hot: opts.min_invocations_hot,
            min_observation_volume: None,
            low_traffic_threshold: None,
            license_jwt: String::new(),
            watermark: None,
        };
        let health_opts = security_runtime_health_options(&opts, runtime_coverage);

        let (config, _) =
            security_runtime_health_config(&opts, &health_opts).expect("health config loads");

        assert_eq!(config.cache_dir, cache_dir);
    }
}
