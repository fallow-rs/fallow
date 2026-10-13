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
pub(crate) mod protocol;
pub(crate) mod report;
pub(crate) mod run_dir;
pub(crate) mod scheduler;
pub(crate) mod transport;
pub(crate) mod verdicts;
pub(crate) mod worktree;

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod e2e_tests;

use std::path::PathBuf;
use std::time::Duration;

pub use fallow_engine::mutation::Operator;
pub use last_report::{last_mutation_report_path, save_last_mutation_report};
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
    /// The maximum number of mutants to run. The others become `pending`.
    pub max_mutants: Option<usize>,
    /// Run in a dirty git worktree, or outside a git worktree. The run still
    /// compares the worktree after the run.
    pub allow_dirty: bool,
    /// The JavaScript runtime. `None` uses `node`, then `bun`, from `PATH`.
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
    /// The files that the caller writes after the run, for example the
    /// `--output-file` report. The worktree check leaves them out, so an old
    /// report does not make the worktree dirty. A relative path is relative
    /// to the process directory, as the caller writes it.
    pub output_paths: Vec<PathBuf>,
}
