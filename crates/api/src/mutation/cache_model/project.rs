//! A template project on disk and its mutation runs.

use std::path::{Path, PathBuf};
use std::process::Command;

use fallow_output::{
    MutantStatus, MutationCompletionStatus, MutationIncompleteReason, MutationReport,
};

use super::template::{self, Runner};
use crate::mutation::{MutationRunOptions, MutationRunScope, Operator, ReachSelectionMode};
use crate::{AnalysisOptions, run_mutation};

/// With this variable set to `1`, a missing runner fails the test instead
/// of a skip. The CI job that installs the runners sets it.
const REQUIRED_ENV: &str = "FALLOW_MUTATION_E2E_REQUIRED";

/// The file that the runs mutate.
pub const SOURCE: &str = "src/math.ts";

#[expect(
    clippy::print_stderr,
    reason = "a skipped model test says why it did not run"
)]
fn skip(reason: &str) {
    assert!(
        std::env::var_os(REQUIRED_ENV).is_none_or(|value| value != "1"),
        "{REQUIRED_ENV}=1 is set, but {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn on_path(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// True when `node` is on `PATH` and has `module.registerHooks`, which the
/// runner trace uses.
fn node_has_sync_hooks() -> bool {
    Command::new("node")
        .args([
            "-e",
            "process.exit(typeof require('node:module').registerHooks === 'function' ? 0 : 1)",
        ])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The Vitest package of the companion, or `None` after a skip.
pub fn vitest_package() -> Option<PathBuf> {
    let Some(manifest_dir) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        skip("the result cache model needs CARGO_MANIFEST_DIR, which cargo sets");
        return None;
    };
    let package = Path::new(&manifest_dir).join("../../tools/mutation-runner/node_modules/vitest");
    if package.join("package.json").is_file() && node_has_sync_hooks() {
        return Some(dunce::canonicalize(package).expect("vitest path"));
    }
    skip(&format!(
        "the result cache model needs `node` 22.15 or later on PATH (for `module.registerHooks`) and `npm ci` in {}",
        package.display()
    ));
    None
}

/// The runners that this machine can run, after a skip for each other one.
pub fn available_runners() -> Vec<Runner> {
    let mut runners = vec![Runner::Vitest];
    if on_path("bun") {
        runners.push(Runner::Bun);
    } else {
        skip("the result cache model of `bun test` needs `bun` on PATH");
    }
    runners
}

/// The status, the killing test, the covering tests and the static flag of
/// each mutant of one run. The covering tests and the static flag come from
/// the dry run, so a stale dry-run entry shows also when the status stays.
pub type Observation = Vec<(String, MutantStatus, Option<String>, Vec<String>, bool)>;

/// The facts of one mutation run.
pub struct Run {
    pub observation: Observation,
    pub hits: usize,
    pub stored: usize,
    /// The test files whose dry-run facts came from the cache.
    pub dry_run_hits: usize,
    /// The test files that the first dry run ran.
    pub dry_run_misses: usize,
    /// The test files that the reach record left out of the dry run.
    pub reach_dropped: usize,
}

/// A template project in a temporary directory.
pub struct Project {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
    /// The reach selection of a cached run. A cache-free run has none.
    reach: ReachSelectionMode,
}

impl Project {
    pub fn new(runner: Runner, vitest: &Path) -> Self {
        let dir = tempfile::tempdir().expect("dir");
        let root = dunce::canonicalize(dir.path()).expect("root");
        template::write(&root, runner, vitest);
        Self {
            _dir: dir,
            root,
            reach: ReachSelectionMode::Off,
        }
    }

    /// The project with `reach` for each cached run.
    pub fn with_reach(self, reach: ReachSelectionMode) -> Self {
        Self { reach, ..self }
    }

    /// One mutation run of the arithmetic operator on [`SOURCE`], with the
    /// result cache on (`cached`) or off.
    pub fn run(&self, cached: bool) -> Run {
        let options = MutationRunOptions {
            analysis: AnalysisOptions {
                root: Some(self.root.clone()),
                no_cache: !cached,
                ..AnalysisOptions::default()
            },
            scope: MutationRunScope::Paths(vec![PathBuf::from(SOURCE)]),
            operators: Some(vec![Operator::ArithmeticOperator]),
            allow_dirty: true,
            concurrency: Some(1),
            reach_selection: if cached {
                self.reach
            } else {
                ReachSelectionMode::Off
            },
            ..MutationRunOptions::default()
        };
        let report = run_mutation(&options).expect("the mutation run completes");
        check_complete(&report);
        let (hits, stored, dry_run_hits, dry_run_misses) =
            report.cache.as_ref().map_or((0, 0, 0, 0), |cache| {
                (
                    cache.hits,
                    cache.stored,
                    cache.dry_run_hits,
                    cache.dry_run_misses,
                )
            });
        let reach_dropped = report
            .cache
            .as_ref()
            .and_then(|cache| cache.reach_selection.as_ref())
            .filter(|reach| matches!(reach.mode, fallow_output::MutationReachSelectionMode::On))
            .map_or(0, |reach| reach.dropped);
        let mut observation: Observation = report
            .mutants
            .into_iter()
            .map(|mutant| {
                let mut covering = mutant.covering_tests;
                covering.sort();
                (
                    mutant.location.mutant_id,
                    mutant.status,
                    mutant.killed_by,
                    covering,
                    mutant.static_mutant,
                )
            })
            .collect();
        observation.sort();
        Run {
            observation,
            hits,
            stored,
            dry_run_hits,
            dry_run_misses,
            reach_dropped,
        }
    }
}

/// Checks that each mutant ran. A failed baseline test makes the run
/// partial, which the steps with a failed test expect.
fn check_complete(report: &MutationReport) {
    assert!(report.errors.is_empty(), "{:#?}", report.errors);
    let complete = report.completion.status == MutationCompletionStatus::Complete
        || report
            .completion
            .reasons
            .iter()
            .all(|reason| *reason == MutationIncompleteReason::BaselineFailure);
    assert!(complete, "{:#?}", report.completion);
    assert!(!report.mutants.is_empty(), "the run has no mutant");
}
