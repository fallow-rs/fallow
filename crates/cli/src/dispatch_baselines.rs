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
        return emit_error(
            &format!(
                "`fallow baselines prune` runs one analysis over the whole project, in production mode with --production or config, so it does not accept `{flag}`. A narrowed run would remove valid entries."
            ),
            2,
            output,
        );
    }
    if let Some(flag) = cli_global_baseline_flag(cli) {
        return emit_error(
            &format!(
                "`fallow baselines prune` does not accept `{flag}`. Name the files with --dead-code-baseline, --health-baseline and --dupes-baseline, or with `audit.*Baseline` in the config."
            ),
            2,
            output,
        );
    }
    let config = match load_config(
        root,
        &cli.config,
        LoadConfigArgs {
            output,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            production: false,
            quiet: dispatch.quiet,
            allow_remote_extends: cli.allow_remote_extends,
        },
    ) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let targets = baselines::prune_targets(
        root,
        &config.audit,
        [dead_code_baseline, health_baseline, dupes_baseline],
    );
    if targets.dead_code.is_none() && targets.health.is_none() && targets.dupes.is_none() {
        return emit_error(
            "no baseline to prune. Set `audit.deadCodeBaseline`, `audit.healthBaseline` or `audit.dupesBaseline` in the config, or pass --dead-code-baseline, --health-baseline or --dupes-baseline",
            2,
            output,
        );
    }
    let production = match resolve_production_modes(cli, root, output, false, false, false) {
        Ok(production) => production,
        Err(code) => return code,
    };
    let coverage_inputs = match resolve_coverage_inputs(
        coverage.as_deref(),
        coverage_root.as_deref(),
        output,
        || Ok(config.health),
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
        production,
        mode_flags: fallow_output::BaselineMode {
            production: cli.production,
            include_entry_exports: cli.include_entry_exports,
        },
        coverage: coverage_inputs.coverage.as_deref(),
        coverage_root: coverage_inputs.coverage_root.as_deref(),
        regression_opts: dispatch.regression_opts(true),
    })
}
