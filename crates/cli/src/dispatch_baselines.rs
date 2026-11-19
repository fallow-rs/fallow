//! Dispatch of the baselines command.

use super::*;

pub(super) fn dispatch_baselines_command(
    dispatch: &DispatchContext<'_>,
    subcommand: &BaselinesCli,
) -> ExitCode {
    let BaselinesCli::Prune {
        check,
        dead_code_baseline,
        health_baseline,
        dupes_baseline,
        coverage,
        coverage_root,
    } = subcommand;
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    if let Some(flag) = baselines::narrowing_flag(cli) {
        return emit_input_error(
            &format!(
                "`fallow baselines prune` runs one analysis over the whole project, in production mode with --production or config, so it does not accept `{flag}`. A narrowed run would remove valid entries."
            ),
            output,
        );
    }
    if let Some(flag) = cli_global_baseline_flag(cli) {
        return emit_input_error(
            &format!(
                "`fallow baselines prune` does not accept `{flag}`. Name the files with --dead-code-baseline, --health-baseline and --dupes-baseline, or with `audit.*Baseline` in the config."
            ),
            output,
        );
    }
    // The analyses of the prune run quietly, as the analyses of a baseline
    // save do. The `loaded config` line still follows `--quiet`.
    let quiet_inputs = dispatch.run_inputs.map(|inputs| {
        let mut inputs = inputs.clone();
        inputs.quiet = true;
        inputs
    });
    let quiet_dispatch = DispatchContext {
        run_inputs: quiet_inputs.as_ref(),
        ..*dispatch
    };
    let run = match quiet_dispatch.resolve_run() {
        Ok(run) => run,
        Err(code) => return code,
    };
    let config = run.config();
    let targets = baselines::prune_targets(
        root,
        &config.audit,
        [dead_code_baseline, health_baseline, dupes_baseline],
    );
    if targets.dead_code.is_none() && targets.health.is_none() && targets.dupes.is_none() {
        return emit_input_error(
            "no baseline to prune. Set `audit.deadCodeBaseline`, `audit.healthBaseline` or `audit.dupesBaseline` in the config, or pass --dead-code-baseline, --health-baseline or --dupes-baseline",
            output,
        );
    }
    let coverage_inputs = match resolve_coverage_inputs(
        coverage.as_deref(),
        coverage_root.as_deref(),
        output,
        || Ok(config.health.clone()),
    ) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };
    baselines::run_baselines_prune(&baselines::BaselinesPruneOptions {
        root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: cli.allow_remote_extends,
        check: *check,
        targets,
        production: run.production_modes(),
        run: &run,
        mode_flags: fallow_output::BaselineMode {
            production: cli.production,
            include_entry_exports: cli.include_entry_exports,
        },
        coverage: coverage_inputs.coverage.as_deref(),
        coverage_root: coverage_inputs.coverage_root.as_deref(),
        regression_opts: dispatch.regression_opts(true),
    })
}
