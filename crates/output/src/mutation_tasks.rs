//! The output types of `fallow mutation tasks`: the open tasks of a saved
//! mutation report as one batch.

use fallow_types::envelope::ToolVersion;
use serde::Serialize;

use crate::mutation::{
    MutantEquivalenceHint, MutantStatus, MutationCompletionStatus, MutationScopeKind,
    MutationStaleReason,
};
use crate::mutation_replay::MutationTaskWitness;
use crate::root_envelopes::serialize_named_json_output;

/// Version singleton for the open tasks of a mutation report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum MutationTasksSchemaVersion {
    /// The first version.
    #[serde(rename = "1")]
    V1,
}

/// One open task: a `survived` or `no_coverage` mutant of a saved report.
/// A task holds no source text other than the source lines of the mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationTask {
    /// The id of the mutant. `fallow mutation inspect` takes this id.
    pub mutant_id: String,
    /// The project-root-relative path of the mutated file.
    pub file: String,
    /// The 1-based line of the mutant.
    pub line: u32,
    /// The 1-based column of the mutant.
    pub column: u32,
    /// The operator, kebab-case.
    pub operator: String,
    /// The name of the enclosing function. Absent at the top level of a
    /// module and for an anonymous function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// `survived` or `no_coverage`.
    pub status: MutantStatus,
    /// The source lines that contain the mutant, from the current file.
    /// `null` when the task is stale.
    pub original_lines: Option<String>,
    /// The same lines with the mutation applied. `null` when the task is
    /// stale.
    pub mutated_lines: Option<String>,
    /// An advisory hint that the mutant may be equivalent. The task stays
    /// open: a person decides, and records an equivalent mutant in a verdict
    /// file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equivalence_hint: Option<MutantEquivalenceHint>,
    /// Why the current file no longer gives the mutant. Absent for a fresh
    /// task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_reason: Option<MutationStaleReason>,
    /// The test files of the tests that reached the mutant in the dry run,
    /// sorted. Empty for `no_coverage`. `fallow mutation inspect` gives the
    /// test ids and the static test files of one mutant.
    pub covering_test_files: Vec<String>,
    /// The witness of the task: a recorded input that shows a difference, with
    /// the results of both sides. Present only after a `--record-args
    /// values` run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness: Option<MutationTaskWitness>,
}

/// The number of tasks for each status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationTasksSummary {
    /// The number of tasks.
    pub tasks: usize,
    /// The tasks with the status `survived`.
    pub survived: usize,
    /// The tasks with the status `no_coverage`.
    pub no_coverage: usize,
    /// The tasks whose file changed after the report. They have no code.
    pub stale: usize,
}

/// The scope of the run of a tasks batch. A mutant of a file in `files`
/// that is not a task is gone from the open tasks: a test killed it, or the
/// report gives it another status that is not open. In a `partial` report
/// (see `report_completion`) that status can be `pending`. A mutant of a file
/// outside `files` has no result in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationTasksScope {
    /// The kind of scope of the run.
    pub kind: MutationScopeKind,
    /// The base ref of a `changed` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    /// The project-root-relative paths of the files with a mutant result in
    /// the report, sorted. A result is each status other than `pruned` and
    /// `pending`. The list also has the files without a task.
    pub files: Vec<String>,
}

/// The envelope of `fallow mutation tasks`: the open tasks of one saved
/// report, as one batch. It runs no user code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationTasksOutput {
    /// Independent schema version.
    pub schema_version: MutationTasksSchemaVersion,
    /// The Fallow version.
    pub version: ToolVersion,
    /// The completion status of the report. A `partial` report can miss
    /// tasks: a `pending` mutant is not a task.
    pub report_completion: Option<MutationCompletionStatus>,
    /// The scope of the run of the report. Absent when the report has no
    /// scope of a kind that this version knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<MutationTasksScope>,
    /// The counts of the tasks.
    pub summary: MutationTasksSummary,
    /// The tasks, sorted by file, line, column and mutant id.
    pub tasks: Vec<MutationTask>,
}

/// Serialize the open tasks of a mutation report with their root
/// discriminator.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_mutation_tasks_json_output(
    output: MutationTasksOutput,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "mutation-tasks")
}
