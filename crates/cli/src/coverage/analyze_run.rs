//! The resolved runs of `fallow coverage analyze`.

use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_engine::resolved_run::ResolvedRun;

use super::{AnalyzeArgs, local_health_options};
use crate::coverage::RunContext;
use crate::error::emit_input_error;

/// One dead-code analysis run, kept together with the session so callers can
/// still read the resolved root and config after the artifacts are produced.
pub(super) struct StaticAnalysisRun {
    pub(super) session: fallow_engine::session::AnalysisSession,
    pub(super) artifacts: fallow_engine::dead_code::DeadCodeAnalysisArtifacts,
}

/// The run of a static analysis. `production` is a given production mode;
/// `None` lets the `--production` flag and then the config decide.
pub(super) fn static_run(
    ctx: &RunContext<'_>,
    production: Option<bool>,
) -> Result<ResolvedRun, ExitCode> {
    ctx.resolve(|inputs| {
        if let Some(value) = production {
            inputs.production = Some(fallow_types::sourced::Given {
                value,
                origin: fallow_types::sourced::Origin::Flag("--production"),
            });
        }
    })
    .map_err(|message| emit_input_error(&message, ctx.output))
}

/// The run of the command line, with the side effects of the config load.
pub(super) fn command_run(ctx: &RunContext<'_>, production: bool) -> Result<ResolvedRun, ExitCode> {
    let run = static_run(ctx, production.then_some(true))?;
    crate::runtime_support::on_run_resolved(
        &run,
        ctx.config_path.as_deref(),
        ctx.output,
        ctx.quiet,
    );
    Ok(run)
}

/// Run dead-code analysis over the project in the production mode of `run`.
pub(super) fn run_static_analysis(
    run: &ResolvedRun,
    output: OutputFormat,
) -> Result<StaticAnalysisRun, ExitCode> {
    let session = crate::cli_production::dead_code_runnable(run, output)?.open_session();
    let artifacts = session
        .analyze_dead_code_with_artifacts(true, true)
        .map_err(|err| emit_input_error(&format!("analysis failed: {err}"), output))?;
    Ok(StaticAnalysisRun { session, artifacts })
}

/// The health result of a local `coverage analyze` run, in the production
/// mode of the dead-code analysis of the run.
pub(super) fn local_health(
    args: &AnalyzeArgs,
    ctx: &RunContext<'_>,
    runtime_coverage: fallow_engine::health::RuntimeCoverageOptions,
) -> Result<crate::health::HealthResult, ExitCode> {
    let run = command_run(ctx, args.production)?;
    let dead_code = crate::cli_production::dead_code_runnable(&run, ctx.output)?;
    let mut options = local_health_options(args, ctx, runtime_coverage);
    options.production = *dead_code.production().value();
    crate::health::execute_health_in(&options, Some(dead_code))
}
