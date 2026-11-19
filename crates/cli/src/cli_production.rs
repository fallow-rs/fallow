use std::process::ExitCode;

use fallow_config::{
    AnalysisSelection, CommandKind, OutputFormat, PlannedAnalysis, ResolvedConfig, RunPlan,
};
use fallow_engine::project_config::ConfigLoadError;
pub use fallow_engine::project_config::ProductionModes;
use fallow_engine::resolved_run::{AnalysisSlot, EmptyCause, ResolveError, ResolvedRun, Runnable};

impl crate::DispatchContext<'_> {
    /// Resolve the run of the command line once, and apply the side effects
    /// of the config load. An error prints on the error channel of the output
    /// format and gives exit code 2.
    pub(crate) fn resolve_run(&self) -> Result<ResolvedRun, ExitCode> {
        let pending = self.begin_run()?;
        self.finish_run(&pending)?;
        let PendingRun { result, .. } = pending;
        result.map_err(|err| self.emit_resolve_error(&err))
    }

    /// Resolve the run of the command line, and keep the side effects of the
    /// config load for [`Self::finish_run`]. A config that does not load, an
    /// invalid input and an empty selection exit 2 here. A config that fails
    /// validation and a failed workspace discovery exit 2 in
    /// [`Self::finish_run`]. So a command checks its flags after the config
    /// parses and before the config validates.
    pub(crate) fn begin_run(&self) -> Result<PendingRun, ExitCode> {
        let Some(inputs) = self.run_inputs else {
            return Err(crate::error::emit_input_error(
                "this command resolves no run",
                self.output,
            ));
        };
        match fallow_engine::resolved_run::resolve_run(inputs) {
            Err(err @ (ResolveError::ConfigInvalid { .. } | ResolveError::Workspace { .. })) => {
                Ok(PendingRun::new(Err(err)))
            }
            Err(err) => Err(self.emit_resolve_error(&err)),
            Ok(run) => Ok(PendingRun::new(Ok(run))),
        }
    }

    /// The run of `pending`. The first call applies the side effects of the
    /// config load, or prints the error that [`Self::begin_run`] kept.
    pub(crate) fn finish_run<'run>(
        &self,
        pending: &'run PendingRun,
    ) -> Result<&'run ResolvedRun, ExitCode> {
        match &pending.result {
            Ok(run) => {
                if !pending.finished.replace(true) {
                    crate::runtime_support::on_run_resolved(
                        run,
                        self.cli.config.as_deref(),
                        self.output,
                        self.quiet,
                    );
                }
                Ok(run)
            }
            Err(err) => Err(self.emit_resolve_error(err)),
        }
    }

    /// The run of `pending` without the side effects of the config load, or
    /// the error that [`Self::begin_run`] kept.
    pub(crate) fn checked_run<'run>(
        &self,
        pending: &'run PendingRun,
    ) -> Result<&'run ResolvedRun, ExitCode> {
        pending
            .result
            .as_ref()
            .map_err(|err| self.emit_resolve_error(err))
    }

    /// Run `command` with the run of `pending`, after [`Self::finish_run`].
    pub(crate) fn with_run(
        &self,
        pending: &PendingRun,
        command: impl FnOnce(&ResolvedRun) -> ExitCode,
    ) -> ExitCode {
        self.finish_run(pending).map_or_else(|code| code, command)
    }

    /// [`crate::dispatch::resolve_coverage_inputs`] with the health config of
    /// a pending run. The config load applies only when a coverage input
    /// comes from the config.
    pub(crate) fn health_coverage_inputs(
        &self,
        pending: &PendingRun,
        coverage: Option<&std::path::Path>,
        coverage_root: Option<&std::path::Path>,
    ) -> Result<crate::dispatch::ResolvedHealthCoverageInputs, ExitCode> {
        let health = || Ok(self.finish_run(pending)?.config().health.clone());
        crate::dispatch::resolve_coverage_inputs(coverage, coverage_root, self.output, health)
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
}

/// A resolved run whose config load side effects did not apply yet. See
/// [`crate::DispatchContext::begin_run`].
pub struct PendingRun {
    result: Result<ResolvedRun, ResolveError>,
    finished: std::cell::Cell<bool>,
}

impl PendingRun {
    const fn new(result: Result<ResolvedRun, ResolveError>) -> Self {
        Self {
            result,
            finished: std::cell::Cell::new(false),
        }
    }
}

/// The pending run of a command that runs the dead-code analysis: `list`,
/// `fix`, `viz`, `flags` and `suppressions`. The command reads the config at
/// the point where it loaded the config before, so its own checks of the
/// command line keep their order before the `loaded config` line.
#[derive(Clone, Copy)]
pub struct DeadCodeRun<'a> {
    dispatch: &'a crate::DispatchContext<'a>,
    pending: &'a PendingRun,
}

impl<'a> DeadCodeRun<'a> {
    /// The run of `pending`, with the side effects of `dispatch`.
    pub const fn new(dispatch: &'a crate::DispatchContext<'a>, pending: &'a PendingRun) -> Self {
        Self { dispatch, pending }
    }

    /// The dead-code analysis of the run, after the side effects of the
    /// config load. An error prints on the error channel of the dispatch
    /// output format and gives exit code 2.
    pub fn runnable(self) -> Result<Runnable<'a>, ExitCode> {
        let run = self.dispatch.finish_run(self.pending)?;
        dead_code_runnable(run, self.dispatch.output)
    }
}

/// The resolver of a command that reads its config after its own checks of
/// the command line. The dispatch passes `&|| dispatch.resolve_run()`.
pub type ResolveRun<'a> = &'a dyn Fn() -> Result<ResolvedRun, ExitCode>;

/// The dead-code analysis of `run`. A run that skips it prints an error on
/// the error channel of `output` and gives exit code 2.
pub fn dead_code_runnable(
    run: &ResolvedRun,
    output: OutputFormat,
) -> Result<Runnable<'_>, ExitCode> {
    runnable(run, PlannedAnalysis::DeadCode).ok_or_else(|| {
        crate::error::emit_input_error("the run skips the dead-code analysis", output)
    })
}

/// The resolved config of a dead-code command: the config of `run`, or the
/// config that `load` gives when the command has no run, as in a benchmark
/// hook.
pub fn dead_code_config(
    run: Option<DeadCodeRun<'_>>,
    load: impl FnOnce() -> Result<ResolvedConfig, ExitCode>,
) -> Result<(Option<Runnable<'_>>, ResolvedConfig), ExitCode> {
    match run {
        Some(run) => {
            let runnable = run.runnable()?;
            Ok((Some(runnable), runnable.config().clone()))
        }
        None => Ok((None, load()?)),
    }
}

/// A session with `config`, the config of the run after the settings of the
/// command. The settings cannot change a decided field of the run.
pub fn open_session(
    run: Option<Runnable<'_>>,
    config: &ResolvedConfig,
) -> fallow_engine::EngineResult<fallow_engine::session::AnalysisSession> {
    match run {
        Some(runnable) => runnable.open_session_adjusted(|decided| *decided = config.clone()),
        None => fallow_engine::session::AnalysisSession::from_resolved_config(config.clone()),
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

#[cfg(test)]
#[path = "selection_key_guard_tests.rs"]
mod selection_key_guard_tests;
