//! Mutation evidence in a health report, read from a saved `fallow mutation`
//! report with `fallow health --mutation-report <file>`.
//!
//! The mutation score is a separate value. It never changes the CRAP score of
//! a finding or the composite health score.

use serde::{Deserialize, Serialize};

use crate::{MutationCompletionStatus, MutationStatusCounts};

/// Whether the mutation evidence still describes the current source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HealthMutationState {
    /// The file has the SHA-256 digest of the report.
    Fresh,
    /// The file changed, is missing, or has no digest in the report. The
    /// evidence has no score and no counts.
    Stale,
}

/// The mutation evidence of one complexity finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthFindingMutation {
    /// Whether the evidence describes the current file.
    pub state: HealthMutationState,
    /// Why the evidence is stale. Absent when the state is `fresh`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_reason: Option<HealthMutationStaleReason>,
    /// The mutation score of the function, a fraction from 0 to 1:
    /// `(killed + timeout) / (killed + timeout + survived)`. Absent when the
    /// state is `stale`, and when no mutant of the function ran to a tested
    /// result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// The number of mutants of the function for each status. Absent when
    /// the state is `stale`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<MutationStatusCounts>,
}

/// Why mutation evidence is stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum HealthMutationStaleReason {
    /// The file does not exist or cannot be read.
    FileMissing,
    /// The report has no digest for the file.
    NoDigest,
    /// The SHA-256 digest of the file differs from the report.
    SourceChanged,
}

/// One file of the mutation report that does not match the current source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthMutationStaleFile {
    /// The root-relative path, with forward slashes.
    pub path: String,
    /// Why the file is stale.
    pub reason: HealthMutationStaleReason,
}

/// The join of a saved mutation report with the complexity findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthMutationSummary {
    /// The completion status of the run that wrote the report. Only
    /// `complete` makes an empty survivor list conclusive. Absent when the
    /// report has no completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<MutationCompletionStatus>,
    /// The number of function results in the report.
    pub functions: usize,
    /// The number of complexity findings with fresh mutation evidence.
    pub findings_fresh: usize,
    /// The number of complexity findings with stale mutation evidence.
    pub findings_stale: usize,
    /// The number of complexity findings that no function result of the
    /// report matches. A finding matches when the path, the first line and
    /// the name are equal.
    pub findings_unmatched: usize,
    /// The report files that do not match the current source, sorted by
    /// path. Their functions have no score.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale_files: Vec<HealthMutationStaleFile>,
}
