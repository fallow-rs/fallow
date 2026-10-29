//! Independent JSON contracts for mutation testing.
//!
//! A mutation report never contains source text, so a report can leave the
//! machine without the code of the project. A mutant is identified by its id, file, position and operator.
//! The `change` of a mutant uses a fixed vocabulary of operator tokens,
//! literals and placeholders, never project text.
//! The `inspect` command shows the code of one mutant.

use fallow_types::envelope::{ElapsedMs, ToolVersion};
use serde::{Deserialize, Serialize};

use crate::audit_test_reach::TestReachClass;
use crate::root_envelopes::serialize_named_json_output;

/// Current mutation report schema version.
pub const MUTATION_SCHEMA_VERSION: u32 = 1;
/// Current mutation plan schema version.
pub const MUTATION_PLAN_SCHEMA_VERSION: u32 = 1;
/// Current mutation inspect schema version.
pub const MUTATION_INSPECT_SCHEMA_VERSION: u32 = 1;

/// Version singleton for a mutation report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum MutationSchemaVersion {
    /// Initial independent contract.
    #[serde(rename = "1")]
    V1,
}

/// Version singleton for a mutation plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum MutationPlanSchemaVersion {
    /// Initial independent contract.
    #[serde(rename = "1")]
    V1,
}

/// The result of one mutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutantStatus {
    /// A covering test failed.
    Killed,
    /// The run passed its timeout. A retry in a new companion with twice the
    /// time also timed out, when the time budget allowed the retry. The
    /// mutant counts as detected.
    Timeout,
    /// Each covering test passed, also in a retry in a new companion when
    /// the time budget allowed the retry.
    Survived,
    /// No test reaches the mutant.
    NoCoverage,
    /// The runner failed for a reason outside the tests.
    RuntimeError,
    /// The second run in a new companion changed whether the tests detect
    /// the mutant.
    Flaky,
    /// A static rule removed the mutant.
    Pruned,
    /// The mutant did not run: the mutant limit or the time budget ended first.
    Pending,
    /// A verdict marks the mutant as equivalent to the original code: no
    /// test can detect it.
    Equivalent,
    /// Each covering test passed, but the mutated file does not type-check.
    /// Only a run with the type check of survivors gives this status. The
    /// mutant does not count in the score.
    CompileError,
}

/// The number of mutants for each status, and the mutation score.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationStatusCounts {
    /// Each mutant.
    pub total: usize,
    /// Killed mutants.
    pub killed: usize,
    /// Mutants that passed their timeout.
    pub timeout: usize,
    /// Mutants that each covering test let pass.
    pub survived: usize,
    /// Mutants that no test reaches.
    pub no_coverage: usize,
    /// Mutants with a runner fault.
    pub runtime_error: usize,
    /// Mutants where the second run in a new companion changed whether the
    /// tests detect the mutant.
    pub flaky: usize,
    /// Mutants that a static rule removed.
    pub pruned: usize,
    /// Mutants that did not run.
    pub pending: usize,
    /// Mutants that a verdict marks as equivalent.
    pub equivalent: usize,
    /// Survived mutants that do not type-check. A report from a version
    /// before `--typecheck` has no such field, so it reads as 0.
    #[serde(default)]
    pub compile_error: usize,
}

impl MutationStatusCounts {
    /// The mutants that ran against the tests: `killed`, `timeout`,
    /// `survived`, `runtime_error`, `flaky` and `compile_error`.
    #[must_use]
    pub const fn executed(&self) -> usize {
        self.killed
            + self.timeout
            + self.survived
            + self.runtime_error
            + self.flaky
            + self.compile_error
    }

    /// Adds one mutant with `status`.
    pub const fn add(&mut self, status: MutantStatus) {
        self.total += 1;
        match status {
            MutantStatus::Killed => self.killed += 1,
            MutantStatus::Timeout => self.timeout += 1,
            MutantStatus::Survived => self.survived += 1,
            MutantStatus::NoCoverage => self.no_coverage += 1,
            MutantStatus::RuntimeError => self.runtime_error += 1,
            MutantStatus::Flaky => self.flaky += 1,
            MutantStatus::Pruned => self.pruned += 1,
            MutantStatus::Pending => self.pending += 1,
            MutantStatus::Equivalent => self.equivalent += 1,
            MutantStatus::CompileError => self.compile_error += 1,
        }
    }

    /// Removes one mutant with `status`. The count never goes below zero.
    pub const fn remove(&mut self, status: MutantStatus) {
        let count = match status {
            MutantStatus::Killed => &mut self.killed,
            MutantStatus::Timeout => &mut self.timeout,
            MutantStatus::Survived => &mut self.survived,
            MutantStatus::NoCoverage => &mut self.no_coverage,
            MutantStatus::RuntimeError => &mut self.runtime_error,
            MutantStatus::Flaky => &mut self.flaky,
            MutantStatus::Pruned => &mut self.pruned,
            MutantStatus::Pending => &mut self.pending,
            MutantStatus::Equivalent => &mut self.equivalent,
            MutantStatus::CompileError => &mut self.compile_error,
        };
        if *count > 0 {
            *count -= 1;
            self.total -= 1;
        }
    }

    /// The mutation score over the executed mutants: `(killed + timeout) /
    /// (killed + timeout + survived)`, from 0 to 1. `None` when no mutant
    /// ran to a tested result.
    ///
    /// `no_coverage` mutants stay out of the score;
    /// [`MutationSummary::coverage`] reports them. Runtime errors, flaky, pruned,
    /// pending, equivalent and compile-error mutants never count.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "mutant counts are far below 2^52"
    )]
    pub fn score(&self) -> Option<f64> {
        let detected = self.killed + self.timeout;
        let executed = detected + self.survived;
        (executed > 0).then(|| detected as f64 / executed as f64)
    }
}

/// Whether the result is conclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutationCompletionStatus {
    /// Each planned mutant has a final status. An empty survivor list is
    /// conclusive.
    Complete,
    /// At least one planned mutant has no final status.
    Partial,
}

/// Why a run is partial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum MutationIncompleteReason {
    /// At least one mutant has the status `runtime_error`.
    RuntimeError,
    /// The mutant limit left covered mutants `pending`.
    MutantLimit,
    /// The time budget ended before each mutant ran.
    TimeBudget,
    /// The plan did not instrument at least one selected file.
    SkippedFiles,
    /// The runner never loaded at least one instrumented file that has a
    /// planned mutant and a static test file.
    UnloadedFiles,
    /// The runner does not list at least one static test file, so its tests
    /// never ran. A mutant that only such a file reaches is `no_coverage` or
    /// `survived` without proof. The `errors` entry
    /// `test-files-not-discovered` names the files.
    TestFilesNotDiscovered,
    /// A test or a test file failed before any mutant was active.
    BaselineFailure,
    /// The run changed the git worktree.
    WorktreeChanged,
    /// The type check of the survivors failed, so a survivor can be a
    /// mutant that does not type-check.
    TypecheckFailed,
    /// The end-of-run worktree check failed, so stray writes are unknown.
    WorktreeUnchecked,
}

/// The completion status of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationCompletion {
    /// `complete` or `partial`.
    pub status: MutationCompletionStatus,
    /// Why the run is partial, sorted. Empty for a complete run.
    pub reasons: Vec<MutationIncompleteReason>,
}

impl MutationCompletion {
    /// A completion with `reasons`. The status is `complete` only when the
    /// list is empty.
    #[must_use]
    pub fn from_reasons(mut reasons: Vec<MutationIncompleteReason>) -> Self {
        reasons.sort_unstable();
        reasons.dedup();
        let status = if reasons.is_empty() {
            MutationCompletionStatus::Complete
        } else {
            MutationCompletionStatus::Partial
        };
        Self { status, reasons }
    }
}

/// Which code a run mutates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutationScopeKind {
    /// The added lines against a base ref.
    Changed,
    /// Each source file of the project.
    All,
    /// The files at the given paths.
    Paths,
}

/// The scope of a plan or a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationScopeOutput {
    /// The kind of scope.
    pub kind: MutationScopeKind,
    /// The base ref of a `changed` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
}

/// The JavaScript runtime that ran the companion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationRuntimeOutput {
    /// `node` or `bun`.
    pub name: String,
    /// The version that the runtime reports.
    pub version: String,
}

/// The test runner of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationRunnerOutput {
    /// The runner name, for example `vitest`.
    pub name: String,
    /// The runner version from the project.
    pub version: String,
    /// The wire protocol version of the companion.
    pub protocol_version: u32,
    /// The runtime of the companion.
    pub runtime: MutationRuntimeOutput,
    /// The number of companion processes that ran mutants.
    pub concurrency: usize,
    /// Facts of the runner setup that limit the result, kebab-case.
    /// `typecheck-not-run`: the project config enables the Vitest type-check
    /// pass, and the run did not execute it, so a type-level kill is not
    /// counted. Absent when the runner states no fact.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// The source position of a mutant. No source text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutantLocation {
    /// The stable mutant id.
    pub mutant_id: String,
    /// The root-relative path, with forward slashes.
    pub file: String,
    /// The 1-based line.
    pub line: u32,
    /// The 1-based column.
    pub column: u32,
    /// The byte span in the original file.
    pub span: (u32, u32),
    /// The operator name, kebab-case.
    pub operator: String,
    /// The name of the enclosing function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// The 1-based first line of the enclosing function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_line: Option<u32>,
    /// The 1-based execution rank of a planned mutant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
    /// The edit of the mutant, without project text. Absent in a report
    /// that an older version wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<MutantChange>,
}

/// The edit of a mutant, without project text.
///
/// Each side is an operator token (for example `??` or `<=`), a fixed literal
/// (`true`, `undefined`, `""`, `"fallow"`, `void 0`), a regular expression
/// token (`^`, `$`, `[`, `[^`, `\d`), or a placeholder for a side that is
/// project code: `<condition>`, `<call>`, `<expression>` or `<string>`. An
/// empty side means that the edit removes the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutantChange {
    /// The token or placeholder before the edit.
    pub from: String,
    /// The token or placeholder after the edit.
    pub to: String,
}

/// One mutant of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutantResult {
    /// The mutant and its position.
    #[serde(flatten)]
    pub location: MutantLocation,
    /// The final status.
    pub status: MutantStatus,
    /// The static rule of a `pruned` mutant, kebab-case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prune_reason: Option<String>,
    /// The test that killed the mutant in this run. Absent for a kill by an
    /// error outside a test.
    ///
    /// This is evidence of one run, not a stable fact. A run stops at the
    /// first failing test, and it runs the fastest covering test first, so a
    /// second run on the same code can name another covering test. No
    /// fingerprint or baseline comparison uses this field.
    ///
    /// The retry of a survivor also runs the other tests of its test files,
    /// so this test can be absent from `covering_tests`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub killed_by: Option<String>,
    /// The tests that reached the mutant in the dry run, sorted by test id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub covering_tests: Vec<String>,
    /// True when the mutant runs while a module loads, outside each test.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub static_mutant: bool,
    /// The duration of one run in milliseconds. A `killed` mutant after a
    /// retry reports the retry. Each other status reports the first run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// How tests reach the code of the mutant in the module graph. Only a
    /// `no_coverage` mutant has this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_reach: Option<MutantTestReach>,
    /// True when the status comes from a first run and a retry in a new
    /// companion. A `survived` or `timeout` mutant without this field got no
    /// retry, because the time budget ended first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retried: bool,
    /// The runs that gave the status of a `retried` mutant: the first run,
    /// then the retry. Absent when the mutant got no retry, and absent for a
    /// `cached` mutant. Each duration is evidence of one run, so no
    /// fingerprint or baseline comparison uses this field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<MutantAttempt>,
    /// True when the status comes from the result cache of an earlier run.
    /// The mutant did not run in this run. `--no-cache` runs each mutant.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cached: bool,
    /// True when the status comes from the journal of an interrupted run
    /// (`--resume`). The mutant did not run in this run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resumed: bool,
    /// The TypeScript diagnostic codes of the new errors of a
    /// `compile_error` mutant, sorted, at most 8.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub type_error_codes: Vec<u32>,
}

/// One run of a retried mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutantAttempt {
    /// The status of this run alone.
    pub status: MutantStatus,
    /// The duration of this run in milliseconds. A watchdog timeout reports
    /// the wait of the watchdog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// The test that killed the mutant in this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub killed_by: Option<String>,
    /// How the run detected the mutant. Absent for a run that did not detect
    /// it, and absent when the runner did not state the kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<MutantFailureKind>,
    /// The first line of the reason of a `runtime_error` run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// How a run detected a mutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutantFailureKind {
    /// A test assertion failed.
    Assertion,
    /// A test or the module load threw an error that is not an assertion.
    Error,
    /// A test or the run passed its time limit.
    Timeout,
}

impl MutantFailureKind {
    /// The kind of a `failure_kind` value of the companion protocol. An
    /// unknown value gives `None`.
    #[must_use]
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "assertion" => Some(Self::Assertion),
            "error" => Some(Self::Error),
            "timeout" => Some(Self::Timeout),
            _ => None,
        }
    }
}

/// How tests reach the code of one mutant in the module graph. The class
/// gives a static reason why no test covers the mutant.
///
/// A mutant in the top-level statement that declares a value export gets the
/// class of that export. A mutant outside each such statement gets the class
/// of its file: `none-file` when no test loads the file, `mocked-only` when no
/// test loads the file and a mock explains an export of the file, and `none`
/// when a test loads the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutantTestReach {
    /// The reach class. The same open set as the test reach of `audit`.
    pub reach: TestReachClass,
    /// The export whose declaration holds the mutant. Absent for a mutant
    /// outside each value export declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_name: Option<String>,
    /// For `mocked-only`: the test files whose references a mock removed,
    /// sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<String>,
    /// For `mocked-only`: the mocked modules on the reference paths, sorted.
    /// A mocked module can be a barrel in front of the file of the mutant.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mocked_targets: Vec<String>,
}

/// The result cache facts of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationCacheOutput {
    /// The mutants whose status came from the cache. They did not run.
    pub hits: usize,
    /// The results that this run added to the cache.
    pub stored: usize,
    /// The test inputs that the cache key does not cover: modules that a
    /// test loads through a dynamic import that the module graph does not
    /// resolve (`dynamic-imports`), and files that a test reads
    /// (`file-reads`). A change to them does not invalidate a cached result.
    pub not_keyed: Vec<String>,
}

/// The resume facts of a `--resume` run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationResumeOutput {
    /// The path of the outcome journal, relative to the project root. When
    /// the cache directory is outside the root, the path is absolute: the
    /// report has no other cache path, so no other rule applies. The file
    /// stays only when the run did not complete. A run with
    /// `completion.status: complete` removes it.
    pub journal: String,
    /// The mutants whose status came from the journal. They did not run.
    pub reused: usize,
    /// The journal results whose key does not match this run, because an
    /// input of the mutant changed. These results were not reused from the
    /// journal. The mutant ran again, or its status came from the result
    /// cache.
    pub rejected: usize,
    /// Why the run did not read a journal and started from the beginning,
    /// for example because no journal exists. Absent when the run read the
    /// journal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The results of the mutants of one function.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationFunctionResult {
    /// The root-relative path, with forward slashes.
    pub file: String,
    /// The function name. `null` for code outside each function, and for an
    /// anonymous function.
    pub name: Option<String>,
    /// The 1-based first line. `null` for code outside each function.
    pub line: Option<u32>,
    /// The 1-based last line. `null` for code outside each function.
    pub end_line: Option<u32>,
    /// True when an added line of the diff is in the function.
    pub in_diff: bool,
    /// The cyclomatic complexity, when the analysis measured the function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cyclomatic: Option<u16>,
    /// The number of mutants for each status.
    pub counts: MutationStatusCounts,
    /// The mutation score of the function.
    pub score: Option<f64>,
    /// The ids of the survived mutants, in rank order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub survived: Vec<String>,
}

/// The totals of a run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationSummary {
    /// The number of mutants for each status.
    pub counts: MutationStatusCounts,
    /// The mutation score over the executed mutants, a fraction from 0 to 1:
    /// `(killed + timeout) / (killed + timeout + survived)`. `null` when no
    /// mutant ran to a tested result. `--min-score` and `mutation.minScore`
    /// compare this value in percent.
    pub score: Option<f64>,
    /// The share of mutants that a test reaches, a fraction from 0 to 1:
    /// `covered / (covered + no_coverage)` over the whole plan. `covered`
    /// counts each executed mutant with a covering test and each `pending`
    /// mutant that a passing dry-run test covers. A `pending` mutant that no
    /// dry-run test covers is `no_coverage`, so a run that `--max-mutants`
    /// caps reports the coverage of the full run. `equivalent` and `pruned`
    /// mutants never count. `null` when no mutant is covered or unreached.
    pub coverage: Option<f64>,
}

impl MutationSummary {
    /// The totals, the score and the coverage of `results`.
    #[must_use]
    pub fn from_results(results: &[MutantResult]) -> Self {
        let mut counts = MutationStatusCounts::default();
        let (mut covered, mut uncovered) = (0usize, 0usize);
        for result in results {
            counts.add(result.status);
            match result.status {
                MutantStatus::NoCoverage => uncovered += 1,
                MutantStatus::Pending => covered += 1,
                MutantStatus::Killed
                | MutantStatus::Timeout
                | MutantStatus::Survived
                | MutantStatus::RuntimeError
                | MutantStatus::Flaky
                | MutantStatus::CompileError
                    if !result.covering_tests.is_empty() || result.static_mutant =>
                {
                    covered += 1;
                }
                _ => {}
            }
        }
        Self {
            counts,
            score: counts.score(),
            coverage: ratio(covered, covered + uncovered),
        }
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "mutant counts are far below 2^52"
)]
fn ratio(part: usize, whole: usize) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

/// A test or a test file that failed with no active mutant. Its tests do not
/// decide a mutant status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationBaselineFailure {
    /// The test file, root-relative. `null` for an error outside each test
    /// file.
    pub file: Option<String>,
    /// The test id, for a failed test.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_id: Option<String>,
    /// The error message, for an error outside a test, or the reason for a
    /// test that did not pass in the dry run of a restarted companion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The mutants that ran without this test, because the test did not pass
    /// in the dry run of a restarted companion. Each of them ran with its
    /// other covering tests. Absent for a failure of the first dry run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mutant_ids: Vec<String>,
}

/// A selected file that the plan did not instrument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationSkippedFile {
    /// The root-relative path, with forward slashes.
    pub path: String,
    /// Why the plan skipped the file, kebab-case.
    pub reason: String,
}

/// The git worktree check of a run: a run refuses a dirty worktree unless
/// `--allow-dirty` is set, and it compares the worktree after the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationWorktreeCheck {
    /// False when the root is not in a git worktree, so no check ran.
    pub checked: bool,
    /// True when the caller allowed a dirty worktree.
    pub allow_dirty: bool,
    /// The paths that were dirty before the run, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirty_at_start: Vec<String>,
    /// The paths that the run changed, sorted. A non-empty list is an error.
    pub changed_paths: Vec<String>,
}

/// An error of a run that did not stop the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationRunIssue {
    /// A stable kebab-case code: `cache-write-failed`,
    /// `journal-write-failed`, `runtime-error`, `static-map-miss`,
    /// `test-files-not-discovered`, `typecheck-failed`, `worktree-changed`
    /// or `worktree-check-failed`.
    pub code: String,
    /// A description.
    pub message: String,
    /// The mutant of the issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutant_id: Option<String>,
    /// The last part of the companion stderr.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
}

/// The wall time of each phase, in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationTimings {
    /// The analysis and the plan.
    pub plan_ms: u64,
    /// The companion install, the run directory, and the first `init`.
    pub setup_ms: u64,
    /// The dry run.
    pub dry_run_ms: u64,
    /// The mutant runs.
    pub mutants_ms: u64,
    /// The second runs of survived and timed-out mutants.
    pub retry_ms: u64,
    /// The type check of the survivors. Zero when the run did not check
    /// them.
    pub typecheck_ms: u64,
}

/// Why the type check did not check a survivor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum MutationTypecheckSkipReason {
    /// No TypeScript project includes the file.
    NoProject,
    /// The checker does not check the file: a `// @ts-nocheck` comment, or a
    /// JavaScript file without `checkJs`.
    NotTypeChecked,
    /// The file changed after the plan.
    SourceChanged,
    /// The file does not exist.
    FileMissing,
    /// The checker cannot read the file type: a Vue, Svelte or Astro
    /// component file.
    UnsupportedFile,
}

/// The number of survivors with one skip reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationTypecheckSkip {
    /// Why the type check did not check these survivors.
    pub reason: MutationTypecheckSkipReason,
    /// The number of survivors.
    pub count: usize,
}

/// The type check of the survivors (`--typecheck`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationTypecheckOutput {
    /// True when the type check gave a result for each survivor that it
    /// received. False when the companion failed; the survivors then keep
    /// the status `survived`.
    pub completed: bool,
    /// The survivors that the type check received.
    pub survivors: usize,
    /// The survivors that the checker checked.
    pub checked: usize,
    /// The checked survivors that do not type-check. They have the status
    /// `compile_error`.
    pub compile_errors: usize,
    /// The survivors that the checker did not check, by reason.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<MutationTypecheckSkip>,
}

/// The envelope of `fallow mutation`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationReport {
    /// Independent schema version.
    pub schema_version: MutationSchemaVersion,
    /// The Fallow version.
    pub version: ToolVersion,
    /// The total wall time.
    pub elapsed_ms: ElapsedMs,
    /// Whether the result is conclusive.
    pub completion: MutationCompletion,
    /// The mutated code.
    pub scope: MutationScopeOutput,
    /// The runner. Absent when no companion started, for example when no
    /// test file reaches a mutant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<MutationRunnerOutput>,
    /// The result cache. Absent with `--no-cache`, and when no companion
    /// started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<MutationCacheOutput>,
    /// The resume facts. Absent without `--resume`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<MutationResumeOutput>,
    /// The totals.
    pub summary: MutationSummary,
    /// Each mutant, ranked mutants first.
    pub mutants: Vec<MutantResult>,
    /// The results per enclosing function, by file and line.
    pub functions: Vec<MutationFunctionResult>,
    /// True when the project has no test files.
    pub no_test_files: bool,
    /// The tests and test files that failed with no active mutant.
    pub baseline_failures: Vec<MutationBaselineFailure>,
    /// The selected files that the plan did not instrument.
    pub skipped_files: Vec<MutationSkippedFile>,
    /// The paths of a `paths` scope that select no file.
    pub unmatched_paths: Vec<String>,
    /// The instrumented files that the runner never loaded.
    pub unloaded_paths: Vec<String>,
    /// The git worktree check.
    pub worktree: MutationWorktreeCheck,
    /// The errors that did not stop the run.
    pub errors: Vec<MutationRunIssue>,
    /// The wall time of each phase.
    pub timings: MutationTimings,
    /// The mutated files with their SHA-256 digest at plan time, sorted by
    /// path. `fallow mutation inspect` compares the digest with the current
    /// file.
    pub files: Vec<MutationPlanFile>,
    /// The join with a verdict document. Absent when the run read no verdict
    /// document (`--verdicts` not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdicts: Option<MutationVerdictJoin>,
    /// The type check of the survivors. Absent when the run did not check
    /// them (`--typecheck` not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typecheck: Option<MutationTypecheckOutput>,
}

/// Version singleton for a mutation verdict document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum MutationVerdictSchemaVersion {
    /// Initial independent contract.
    #[serde(rename = "1")]
    V1,
}

/// The judgment of a mutation verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum MutationVerdictKind {
    /// The mutant behaves the same as the original code, so no test can kill
    /// it. It gets the status `equivalent` and leaves the score.
    Equivalent,
}

/// One reviewed mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MutationVerdict {
    /// The review key of the mutant. It is the `mutant_id`: the id hashes the
    /// path, the text of the enclosing function, the relative span, the
    /// operator and the replacement, so it stays the same until the function
    /// changes.
    pub review_key: String,
    /// The judgment.
    pub verdict: MutationVerdictKind,
    /// A short reason, for human review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A versioned verdict document, authored by a person or an agent, for
/// `fallow mutation --verdicts <file>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MutationVerdictInput {
    /// Independent verdict document schema version.
    pub schema_version: MutationVerdictSchemaVersion,
    /// The verdicts. Each `review_key` occurs one time.
    pub verdicts: Vec<MutationVerdict>,
}

/// The result of the join of a run with a verdict document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationVerdictJoin {
    /// The SHA-256 digest of the verdict document, lowercase hex.
    pub verdicts_sha256: String,
    /// The mutants that got the status `equivalent`, sorted.
    pub applied: Vec<String>,
    /// The verdicts that a test contradicts, sorted: the mutant was killed or
    /// timed out, so it is not equivalent. The status stays.
    pub contradicted: Vec<String>,
    /// The verdicts for mutants with no tested result (`pending`,
    /// `runtime_error` or `pruned`), sorted. The status stays.
    pub deferred: Vec<String>,
    /// The verdicts that match no mutant of this run, sorted. A verdict
    /// document can hold keys for code outside the scope of the run, and an
    /// edit of a function gives its mutants new keys.
    pub unmatched: Vec<String>,
}

/// The status of a mutant in a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutationPlanDisposition {
    /// A run executes the mutant.
    Planned,
    /// The mutant is after the mutant limit. The plan limit comes before
    /// the coverage is known: a run executes a pending mutant in the place
    /// of a planned mutant that no test covers.
    Pending,
    /// No test file reaches the file of the mutant.
    NoCoverage,
    /// A static rule removes the mutant.
    Pruned,
}

/// One mutant of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationPlanMutant {
    /// The mutant and its position.
    #[serde(flatten)]
    pub location: MutantLocation,
    /// The status in the plan.
    pub disposition: MutationPlanDisposition,
    /// The static rule of a `pruned` mutant, kebab-case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prune_reason: Option<String>,
    /// How tests reach the code of the mutant in the module graph. Only a
    /// `no_coverage` mutant has this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_reach: Option<MutantTestReach>,
}

/// One instrumented file of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationPlanFile {
    /// The root-relative path, with forward slashes.
    pub path: String,
    /// The SHA-256 digest of the file, lowercase hex.
    pub source_sha256: String,
    /// The test files that reach the file in the module graph, sorted.
    pub static_test_files: Vec<String>,
}

/// The totals of a plan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationPlanCountsOutput {
    /// Each mutant.
    pub total: usize,
    /// Planned mutants.
    pub planned: usize,
    /// Mutants after the mutant limit.
    pub pending: usize,
    /// Mutants that no test file reaches.
    pub no_coverage: usize,
    /// Mutants in unused files.
    pub pruned_dead_code: usize,
    /// Mutants in generated files.
    pub pruned_generated: usize,
    /// Mutants near a file system, process, network or database sink.
    pub pruned_side_effect_sink: usize,
    /// Mutants in framework config exports.
    pub pruned_framework_config: usize,
}

/// The envelope of `fallow mutation --plan`. It runs no user code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationPlanOutput {
    /// Independent schema version.
    pub schema_version: MutationPlanSchemaVersion,
    /// The Fallow version.
    pub version: ToolVersion,
    /// The total wall time.
    pub elapsed_ms: ElapsedMs,
    /// The mutated code.
    pub scope: MutationScopeOutput,
    /// The totals.
    pub counts: MutationPlanCountsOutput,
    /// Each mutant, ranked mutants first.
    pub mutants: Vec<MutationPlanMutant>,
    /// The instrumented files.
    pub files: Vec<MutationPlanFile>,
    /// The union of the static test files, sorted.
    pub test_files: Vec<String>,
    /// True when the project has no test files.
    pub no_test_files: bool,
    /// True when the plan applied the `framework-config` prune rule.
    pub framework_config_checked: bool,
    /// The selected files that the plan did not instrument.
    pub skipped_files: Vec<MutationSkippedFile>,
    /// The paths of a `paths` scope that select no file.
    pub unmatched_paths: Vec<String>,
}

/// Version singleton for a mutation inspect packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum MutationInspectSchemaVersion {
    /// Initial independent contract.
    #[serde(rename = "1")]
    V1,
}

/// Whether the saved report still describes the current file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutationInspectStatus {
    /// The file has the digest of the report, and the mutant is reproduced.
    Fresh,
    /// The file changed, is missing, or no longer gives the mutant. The
    /// packet then has no code.
    Stale,
}

/// Why an inspect packet is stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum MutationStaleReason {
    /// The file does not exist or cannot be read.
    FileMissing,
    /// The report has no digest for the file.
    NoDigest,
    /// The SHA-256 digest of the file differs from the report.
    SourceChanged,
    /// The file has the digest of the report, but the instrumentation does
    /// not give the mutant.
    MutantNotReproduced,
}

/// The code of one mutant, from the current file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationInspectCode {
    /// The original text of the span.
    pub original: String,
    /// The text that replaces the span.
    pub replacement: String,
    /// The source lines that contain the span, before the mutation.
    pub original_lines: String,
    /// The same lines with the replacement applied.
    pub mutated_lines: String,
}

/// The envelope of `fallow mutation inspect`. It runs no user code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationInspectOutput {
    /// Independent schema version.
    pub schema_version: MutationInspectSchemaVersion,
    /// The Fallow version.
    pub version: ToolVersion,
    /// Whether the report still describes the current file.
    pub status: MutationInspectStatus,
    /// Why the packet is stale. Absent for a fresh packet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_reason: Option<MutationStaleReason>,
    /// The mutant and its position, from the report.
    #[serde(flatten)]
    pub location: MutantLocation,
    /// The status of the mutant in the report. Absent for a plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_status: Option<MutantStatus>,
    /// The SHA-256 digest of the file in the report.
    pub report_sha256: Option<String>,
    /// The SHA-256 digest of the current file. `null` when the file is
    /// missing.
    pub current_sha256: Option<String>,
    /// The original and mutated code. `null` for a stale packet.
    pub code: Option<MutationInspectCode>,
    /// The tests that reached the mutant in the dry run, sorted.
    pub covering_tests: Vec<String>,
    /// The test files that reach the file in the module graph, sorted.
    pub static_test_files: Vec<String>,
    /// The test that killed the mutant in the run of the report.
    pub killed_by: Option<String>,
}

/// Serialize a mutation report with its root discriminator.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_mutation_json_output(
    output: MutationReport,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "mutation")
}

/// Serialize a mutation plan with its root discriminator.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_mutation_plan_json_output(
    output: MutationPlanOutput,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "mutation-plan")
}

/// Serialize a mutation inspect packet with its root discriminator.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_mutation_inspect_json_output(
    output: MutationInspectOutput,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "mutation-inspect")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(statuses: &[MutantStatus]) -> MutationStatusCounts {
        let mut counts = MutationStatusCounts::default();
        for status in statuses {
            counts.add(*status);
        }
        counts
    }

    #[test]
    fn the_score_counts_executed_mutants_and_coverage_counts_reached_mutants() {
        let counts = counts(&[
            MutantStatus::Killed,
            MutantStatus::Killed,
            MutantStatus::Timeout,
            MutantStatus::Survived,
            MutantStatus::NoCoverage,
            MutantStatus::NoCoverage,
            MutantStatus::RuntimeError,
            MutantStatus::Flaky,
            MutantStatus::Pruned,
            MutantStatus::Pending,
        ]);
        assert_eq!(counts.total, 10);
        assert_eq!(counts.killed, 2);
        assert_eq!(counts.timeout, 1);
        assert_eq!(counts.survived, 1);
        assert_eq!(counts.no_coverage, 2);
        assert_eq!(counts.runtime_error, 1);
        assert_eq!(counts.flaky, 1);
        assert_eq!(counts.pruned, 1);
        assert_eq!(counts.pending, 1);
        assert_eq!(counts.score(), Some(0.75));
    }

    fn result(status: MutantStatus, covering_tests: &[&str]) -> MutantResult {
        MutantResult {
            location: MutantLocation {
                mutant_id: "m".to_owned(),
                file: "src/a.ts".to_owned(),
                line: 1,
                column: 1,
                span: (0, 1),
                operator: "arithmetic-operator".to_owned(),
                function: None,
                function_line: None,
                rank: None,
                change: None,
            },
            status,
            prune_reason: None,
            killed_by: None,
            covering_tests: covering_tests
                .iter()
                .map(|test| (*test).to_owned())
                .collect(),
            static_mutant: false,
            duration_ms: None,
            test_reach: None,
            retried: false,
            attempts: Vec::new(),
            cached: false,
            resumed: false,
            type_error_codes: Vec::new(),
        }
    }

    #[test]
    fn coverage_counts_executed_mutants_with_a_test_and_pending_mutants_as_covered() {
        let summary = MutationSummary::from_results(&[
            result(MutantStatus::Killed, &["t1"]),
            result(MutantStatus::Survived, &["t1"]),
            result(MutantStatus::RuntimeError, &["t2"]),
            result(MutantStatus::Pending, &[]),
            result(MutantStatus::NoCoverage, &[]),
            result(MutantStatus::Equivalent, &["t1"]),
            result(MutantStatus::Pruned, &[]),
        ]);
        assert_eq!(summary.score, Some(0.5));
        assert_eq!(summary.coverage, Some(0.8));
        assert_eq!(
            MutationSummary::from_results(&[result(MutantStatus::Pruned, &[])]).coverage,
            None
        );
    }

    #[test]
    fn a_capped_run_reports_the_coverage_of_the_full_run() {
        let full = MutationSummary::from_results(&[
            result(MutantStatus::Killed, &["t1"]),
            result(MutantStatus::Survived, &["t1"]),
            result(MutantStatus::NoCoverage, &[]),
            result(MutantStatus::NoCoverage, &[]),
        ]);
        let capped = MutationSummary::from_results(&[
            result(MutantStatus::Killed, &["t1"]),
            result(MutantStatus::Pending, &[]),
            result(MutantStatus::NoCoverage, &[]),
            result(MutantStatus::NoCoverage, &[]),
        ]);
        assert_eq!(full.coverage, Some(0.5));
        assert_eq!(capped.coverage, Some(0.5));
        assert_eq!(full.score, Some(0.5));
        assert_eq!(capped.score, Some(1.0));
    }

    #[test]
    fn equivalent_mutants_leave_the_score() {
        let mut counts = counts(&[
            MutantStatus::Killed,
            MutantStatus::Survived,
            MutantStatus::Survived,
        ]);
        assert_eq!(counts.score(), Some(1.0 / 3.0));
        counts.remove(MutantStatus::Survived);
        counts.add(MutantStatus::Equivalent);
        assert_eq!(counts.total, 3);
        assert_eq!(counts.survived, 1);
        assert_eq!(counts.equivalent, 1);
        assert_eq!(counts.score(), Some(0.5));
        counts.remove(MutantStatus::Timeout);
        assert_eq!(counts.total, 3, "removing an absent status changes nothing");
    }

    #[test]
    fn verdict_documents_reject_unknown_fields_and_verdicts() {
        let valid: MutationVerdictInput = serde_json::from_value(serde_json::json!({
            "schema_version": "1",
            "verdicts": [{ "review_key": "abc", "verdict": "equivalent", "note": "same result" }]
        }))
        .expect("valid document");
        assert_eq!(valid.verdicts[0].verdict, MutationVerdictKind::Equivalent);
        for invalid in [
            serde_json::json!({ "schema_version": "1", "verdicts": [{ "review_key": "a", "verdict": "killed" }] }),
            serde_json::json!({ "schema_version": "1", "verdicts": [{ "review_key": "a", "verdict": "equivalent", "extra": 1 }] }),
            serde_json::json!({ "schema_version": "2", "verdicts": [] }),
            serde_json::json!({ "verdicts": [] }),
        ] {
            assert!(
                serde_json::from_value::<MutationVerdictInput>(invalid.clone()).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn the_score_is_null_when_no_mutant_counts() {
        let excluded = counts(&[
            MutantStatus::RuntimeError,
            MutantStatus::Pruned,
            MutantStatus::Flaky,
        ]);
        assert_eq!(excluded.score(), None);
        assert_eq!(counts(&[MutantStatus::Survived]).score(), Some(0.0));
        assert_eq!(counts(&[MutantStatus::Timeout]).score(), Some(1.0));
    }

    #[test]
    fn completion_is_partial_only_with_a_reason() {
        let complete = MutationCompletion::from_reasons(Vec::new());
        assert_eq!(complete.status, MutationCompletionStatus::Complete);
        let partial = MutationCompletion::from_reasons(vec![
            MutationIncompleteReason::TimeBudget,
            MutationIncompleteReason::RuntimeError,
            MutationIncompleteReason::TimeBudget,
        ]);
        assert_eq!(partial.status, MutationCompletionStatus::Partial);
        assert_eq!(
            partial.reasons,
            vec![
                MutationIncompleteReason::RuntimeError,
                MutationIncompleteReason::TimeBudget
            ]
        );
    }

    #[test]
    fn a_mutant_result_serializes_flat_with_snake_case_status_and_no_source_text() {
        let result = MutantResult {
            location: MutantLocation {
                mutant_id: "m1".to_owned(),
                file: "src/math.ts".to_owned(),
                line: 2,
                column: 10,
                span: (40, 45),
                operator: "arithmetic".to_owned(),
                function: Some("add".to_owned()),
                function_line: Some(1),
                rank: Some(1),
                change: None,
            },
            status: MutantStatus::NoCoverage,
            prune_reason: None,
            killed_by: None,
            covering_tests: Vec::new(),
            static_mutant: false,
            duration_ms: None,
            test_reach: None,
            retried: false,
            attempts: Vec::new(),
            cached: false,
            resumed: false,
            type_error_codes: Vec::new(),
        };
        let value = serde_json::to_value(&result).expect("serializes");
        assert_eq!(
            value,
            serde_json::json!({
                "mutant_id": "m1",
                "file": "src/math.ts",
                "line": 2,
                "column": 10,
                "span": [40, 45],
                "operator": "arithmetic",
                "function": "add",
                "function_line": 1,
                "rank": 1,
                "status": "no_coverage"
            })
        );
    }
    #[test]
    fn a_retried_mutant_serializes_each_attempt_in_run_order() {
        let attempt = |status, duration_ms, failure_kind| MutantAttempt {
            status,
            duration_ms: Some(duration_ms),
            killed_by: None,
            failure_kind,
            detail: None,
        };
        let result = MutantResult {
            location: MutantLocation {
                mutant_id: "m1".to_owned(),
                file: "src/math.ts".to_owned(),
                line: 2,
                column: 10,
                span: (40, 45),
                operator: "arithmetic".to_owned(),
                function: None,
                function_line: None,
                rank: None,
                change: None,
            },
            status: MutantStatus::Flaky,
            prune_reason: None,
            killed_by: None,
            covering_tests: Vec::new(),
            static_mutant: false,
            duration_ms: Some(5000),
            test_reach: None,
            retried: true,
            attempts: vec![
                attempt(
                    MutantStatus::Timeout,
                    5000,
                    Some(MutantFailureKind::Timeout),
                ),
                attempt(MutantStatus::Survived, 12, None),
            ],
            cached: false,
            resumed: false,
            type_error_codes: Vec::new(),
        };
        let value = serde_json::to_value(&result).expect("serializes");
        assert_eq!(
            value["attempts"],
            serde_json::json!([
                {"status": "timeout", "duration_ms": 5000, "failure_kind": "timeout"},
                {"status": "survived", "duration_ms": 12}
            ])
        );
        assert_eq!(value["retried"], serde_json::json!(true));
    }
}
