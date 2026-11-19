use std::process::ExitCode;

use fallow_config::{AnalysisSelection, CommandKind, OutputFormat, PlannedAnalysis, RunPlan};
pub use fallow_engine::project_config::ProductionModes;
use fallow_engine::project_config::{ConfigLoadError, ProductionFlags};
use fallow_engine::resolved_run::{AnalysisSlot, EmptyCause, ResolveError, ResolvedRun, Runnable};

use super::Cli;
use crate::runtime_support::load_config_file;

impl crate::DispatchContext<'_> {
    /// Resolve the run of the command line once, and apply the side effects
    /// of the config load. An error prints on the error channel of the output
    /// format and gives exit code 2.
    pub(crate) fn resolve_run(&self) -> Result<ResolvedRun, ExitCode> {
        let Some(inputs) = self.run_inputs else {
            return Err(crate::error::emit_input_error(
                "this command resolves no run",
                self.output,
            ));
        };
        match fallow_engine::resolved_run::resolve_run(inputs) {
            Ok(run) => {
                crate::runtime_support::on_run_resolved(
                    &run,
                    self.cli.config.as_deref(),
                    self.output,
                    self.quiet,
                );
                Ok(run)
            }
            Err(err) => Err(self.emit_resolve_error(&err)),
        }
    }

    fn emit_resolve_error(&self, err: &ResolveError) -> ExitCode {
        let message = match (err, self.cli.config.as_deref()) {
            // The message names an explicit config file as the user wrote it.
            (
                ResolveError::ConfigLoad(ConfigLoadError::Parse {
                    path: Some(_),
                    message,
                    ..
                }),
                Some(given),
            ) => format!("failed to load config '{}': {message}", given.display()),
            (ResolveError::ConfigLoad(load), _) => {
                crate::runtime_support::config_load_error_message(load)
            }
            (ResolveError::NothingSelected { command, cause }, _) => {
                if *command == CommandKind::Combined
                    && *cause == EmptyCause::Config
                    && self.cli.dupes_baseline.is_some()
                {
                    eprintln!("{}", crate::combined::INERT_DUPES_BASELINE_NOTE);
                }
                nothing_selected_message(*command, *cause)
                    .map_or_else(|| err.message(), str::to_owned)
            }
            (_, _) => err.message(),
        };
        crate::error::emit_input_error(&message, self.output)
    }

    pub(crate) fn production_modes(
        &self,
        dead_code: bool,
        health: bool,
        dupes: bool,
    ) -> Result<ProductionModes, ExitCode> {
        resolve_production_modes(self.cli, self.root, self.output, dead_code, health, dupes)
    }

    pub(crate) fn production_for(
        &self,
        analysis: fallow_config::ProductionAnalysis,
    ) -> Result<bool, ExitCode> {
        self.production_modes(false, false, false)
            .map(|modes| modes.for_analysis(analysis))
    }
}

/// The analysis of a resolved run, when the run executes it.
pub fn runnable(run: &ResolvedRun, analysis: PlannedAnalysis) -> Option<Runnable<'_>> {
    match run.analysis(analysis) {
        AnalysisSlot::Runs(runnable) => Some(runnable),
        AnalysisSlot::Skipped { .. } => None,
    }
}

/// The run plan of a resolved run: the selection of each analysis.
pub fn run_plan(run: &ResolvedRun) -> RunPlan {
    let selection = |analysis| match run.analysis(analysis) {
        AnalysisSlot::Runs(_) => AnalysisSelection::Run,
        AnalysisSlot::Skipped { reason } => *reason.value(),
    };
    RunPlan {
        dead_code: selection(PlannedAnalysis::DeadCode),
        duplication: selection(PlannedAnalysis::Duplication),
        health: selection(PlannedAnalysis::Health),
    }
}

/// The CLI text of a run that selects no analysis.
const fn nothing_selected_message(command: CommandKind, cause: EmptyCause) -> Option<&'static str> {
    match (command, cause) {
        (CommandKind::Combined, EmptyCause::Request) => Some(
            "--skip leaves no analysis to run: pass at most two of dead-code, dupes and health",
        ),
        (CommandKind::Combined, EmptyCause::Config) => Some(
            "--skip leaves no analysis to run: duplicates.enabled is false in the config, so dupes does not run. Keep dead-code or health, or pass --only dupes",
        ),
        (CommandKind::Audit, _) => Some(
            "this audit selects no sub-analysis: --skip, the audit.deadCode, audit.health and audit.dupes config keys, and duplicates.enabled leave nothing to run",
        ),
        _ => None,
    }
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
