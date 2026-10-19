//! The dynamic half of mutation testing: the runner protocol, the embedded
//! companion, the transport, the scheduler and the result model.
//!
//! The static half (selection, the test map, pruning, ranking and the
//! instrumented code) is `fallow_engine::mutation`. The entry points are
//! [`crate::run_mutation`] and [`crate::plan_mutation`].

pub(crate) mod companion;
pub(crate) mod discovery;
pub(crate) mod last_report;
pub(crate) mod manifest;
pub(crate) mod node_args;
pub(crate) mod protocol;
pub(crate) mod report;
pub(crate) mod result_cache;
pub(crate) mod run_dir;
pub(crate) mod runner_select;
pub(crate) mod scheduler;
pub(crate) mod transport;
pub(crate) mod typecheck;
pub(crate) mod verdicts;
pub(crate) mod worktree;

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod e2e_tests;

use std::path::PathBuf;
use std::time::Duration;

pub use fallow_engine::mutation::Operator;
pub use last_report::{last_mutation_report_path, save_last_mutation_report};
pub use report::baseline_blocked_run_error;
pub use verdicts::{MAX_VERDICT_INPUT_BYTES, apply_mutation_verdicts, validate_mutation_verdicts};

use crate::AnalysisOptions;

/// The code that a mutation run mutates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationRunScope {
    /// The added lines against a base ref. `None` detects the base ref the
    /// same way as `audit`. An explicit `analysis.diff_file` wins over the
    /// base ref.
    Changed {
        /// The base ref.
        base: Option<String>,
    },
    /// Each source file of the project.
    All,
    /// The files at these paths, or below these directories. A relative
    /// path is relative to the project root.
    Paths(Vec<PathBuf>),
}

impl Default for MutationRunScope {
    fn default() -> Self {
        Self::Changed { base: None }
    }
}

/// A phase event of a mutation run, for a caller that shows progress.
///
/// [`crate::run_mutation_with_progress`] sends the events in run order. A
/// run with no covered mutant and no test file skips the runner phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MutationProgress {
    /// The plan is ready. The runner setup starts: the runtime, the
    /// companion install and the runner start.
    Setup {
        /// The planned mutants.
        mutants: usize,
    },
    /// The baseline run starts: the test files run with no active mutant.
    Baseline {
        /// The test files of the baseline run.
        test_files: usize,
    },
    /// The mutant runs. The first event has `done` 0, and each finished
    /// mutant sends one event.
    Mutants {
        /// The mutants that finished.
        done: usize,
        /// The mutants that this phase runs. A cached result is not in it.
        total: usize,
    },
    /// The retries of survivors and timeouts. The first event has `done` 0.
    Retries {
        /// The retries that finished.
        done: usize,
        /// The retries that this phase runs.
        total: usize,
    },
    /// The tests stopped. The worktree check, the survivor type check and
    /// the report start.
    Finalize,
}

/// Receives the [`MutationProgress`] events of a run. The scheduler sends
/// the mutant events from its worker threads.
pub type MutationProgressSink<'a> = &'a (dyn Fn(MutationProgress) + Sync);

/// Options of [`crate::run_mutation`] and [`crate::plan_mutation`].
#[derive(Debug, Clone, Default)]
pub struct MutationRunOptions {
    /// Shared project, config and cache options.
    pub analysis: AnalysisOptions,
    /// The code to mutate.
    pub scope: MutationRunScope,
    /// The operators to use. `None` uses each operator.
    pub operators: Option<Vec<Operator>>,
    /// Keep mutants near a file system, process, network or database sink.
    /// Only an explicit caller option can set this, never project config,
    /// because the run has no sandbox copy.
    pub allow_side_effects: bool,
    /// The maximum number of covered mutants to run, in rank order. A
    /// mutant that no dry-run test covers is `no_coverage` and does not
    /// count. The others become `pending`.
    pub max_mutants: Option<usize>,
    /// Run in a dirty git worktree, or outside a git worktree. The run still
    /// compares the worktree after the run.
    pub allow_dirty: bool,
    /// The JavaScript runtime. `None` uses the runtime of the runner from
    /// `PATH`: `node` for `node:test`, `bun` for `bun test`, and `node`,
    /// then `bun`, for Vitest.
    pub runtime: Option<PathBuf>,
    /// The number of companion processes. `None` uses half the logical
    /// cores.
    pub concurrency: Option<usize>,
    /// The time budget of the run. Mutants that did not start before it
    /// ends become `pending`.
    pub time_budget: Option<Duration>,
    /// The timeout of the companion `init` and the dry run. `None` uses
    /// 120 seconds.
    pub runner_timeout: Option<Duration>,
    /// The config file of the test runner, for example a separate Vitest
    /// config of the integration tests. A relative path is relative to the
    /// project root. The file must exist inside the project root. `None`
    /// uses the `mutation.runnerConfig` config key, else the default config
    /// discovery of the runner. Only the Vitest runner reads a config file.
    pub runner_config: Option<PathBuf>,
    /// The files that the caller writes after the run, for example the
    /// `--output-file` report. The worktree check leaves them out, so an old
    /// report does not make the worktree dirty. A relative path is relative
    /// to the process directory, as the caller writes it.
    pub output_paths: Vec<PathBuf>,
    /// Type-check each survived mutant through the TypeScript-Go companion
    /// of `--type-aware`. A survivor whose mutated file does not type-check
    /// becomes `compile_error` and leaves the score. The run fails before
    /// the tests start when no trusted companion exists.
    pub typecheck: bool,
}
