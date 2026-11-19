use std::process::ExitCode;

use fallow_config::{OutputFormat, RunPlan, RunRequest};
use fallow_engine::project_config::ProductionFlags;
pub use fallow_engine::project_config::ProductionModes;

use super::Cli;
use crate::runtime_support::load_config_file;

/// The run plan of the project config for `request`. A project without a
/// config file has the default config. The config loads only when the plan
/// depends on it.
pub fn load_run_plan(
    cli: &Cli,
    root: &std::path::Path,
    output: OutputFormat,
    request: RunRequest,
) -> Result<RunPlan, ExitCode> {
    let config = if request.reads_config() {
        load_config_file(
            root,
            cli.config.as_deref(),
            cli.allow_remote_extends,
            output,
        )?
        .config
    } else {
        fallow_config::FallowConfig::default()
    };
    Ok(fallow_config::plan_run(config.plan_sections(), request))
}

/// The production mode of each analysis: the flag of the analysis, then
/// `--production` or `--no-production`, then the environment variable of the
/// analysis, then `FALLOW_PRODUCTION`, then the config.
pub fn resolve_production_modes(
    cli: &Cli,
    root: &std::path::Path,
    output: OutputFormat,
    production_dead_code: bool,
    production_health: bool,
    production_dupes: bool,
) -> Result<ProductionModes, ExitCode> {
    let config = load_config_file(
        root,
        cli.config.as_deref(),
        cli.allow_remote_extends,
        output,
    )?
    .config;
    let flags = ProductionFlags {
        global: if cli.production {
            Some(true)
        } else if cli.no_production {
            Some(false)
        } else {
            None
        },
        dead_code: production_dead_code.then_some(true),
        health: production_health.then_some(true),
        dupes: production_dupes.then_some(true),
    };
    let env = fallow_config::ProductionEnv::from_process();
    let env = ProductionFlags {
        global: env.global,
        dead_code: env.dead_code,
        health: env.health,
        dupes: env.dupes,
    };
    Ok(flags.with_fallback(env).effective_modes(&config.production))
}

#[cfg(test)]
#[path = "selection_key_guard_tests.rs"]
mod selection_key_guard_tests;
