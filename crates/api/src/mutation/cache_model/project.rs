//! A template project on disk and its mutation runs.

use std::path::{Path, PathBuf};
use std::process::Command;

use fallow_output::{MutantStatus, MutationCompletionStatus, MutationReport};

use super::template::{self, Runner};
use crate::mutation::{MutationRunOptions, MutationRunScope, Operator};
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

/// The Vitest package of the companion, or `None` after a skip.
pub fn vitest_package() -> Option<PathBuf> {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")?;
    let package = Path::new(&manifest_dir).join("../../tools/mutation-runner/node_modules/vitest");
    if package.join("package.json").is_file() && on_path("node") {
        return Some(dunce::canonicalize(package).expect("vitest path"));
    }
    skip(&format!(
        "the result cache model needs `node` on PATH and `npm ci` in {}",
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

/// The status of each mutant of one run.
pub type Observation = Vec<(String, MutantStatus, Option<String>)>;

/// The facts of one mutation run.
pub struct Run {
    pub observation: Observation,
    pub hits: usize,
    pub stored: usize,
}

/// A template project in a temporary directory.
pub struct Project {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
}

impl Project {
    pub fn new(runner: Runner, vitest: &Path) -> Self {
        let dir = tempfile::tempdir().expect("dir");
        let root = dunce::canonicalize(dir.path()).expect("root");
        template::write(&root, runner, vitest);
        Self { _dir: dir, root }
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
            ..MutationRunOptions::default()
        };
        let report = run_mutation(&options).expect("the mutation run completes");
        check_complete(&report);
        let (hits, stored) = report
            .cache
            .as_ref()
            .map_or((0, 0), |cache| (cache.hits, cache.stored));
        let mut observation: Observation = report
            .mutants
            .into_iter()
            .map(|mutant| (mutant.location.mutant_id, mutant.status, mutant.killed_by))
            .collect();
        observation.sort();
        Run {
            observation,
            hits,
            stored,
        }
    }
}

fn check_complete(report: &MutationReport) {
    assert!(report.errors.is_empty(), "{:#?}", report.errors);
    assert_eq!(
        report.completion.status,
        MutationCompletionStatus::Complete,
        "{:#?}",
        report.completion
    );
    assert!(!report.mutants.is_empty(), "the run has no mutant");
}
