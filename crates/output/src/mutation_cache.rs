//! The cache facts of a `fallow mutation` run: the result cache, the
//! dry-run cache and the reach selection.

use serde::Serialize;

/// The result cache facts of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationCacheOutput {
    /// The mutants whose status came from the cache. They did not run.
    pub hits: usize,
    /// The results that this run added to the cache.
    pub stored: usize,
    /// The test files whose dry-run facts came from the cache. The first dry
    /// run of the run did not run them. The key parts are in the "Dry-run
    /// cache" section of the mutation testing docs.
    pub dry_run_hits: usize,
    /// The test files with no valid dry-run facts in the cache. The first
    /// dry run of the run ran them.
    pub dry_run_misses: usize,
    /// The count of `dry_run_misses` for each reason: `no-entry` (no stored
    /// facts with the key of the run: a new or changed file, or a file whose
    /// last dry run had a failed test or an error), `unknown-mutant` (the
    /// facts name a mutant outside the plan) and `bound-file-changed` (a test
    /// file of the same earlier dry run that can give the static hits or
    /// loaded files of the facts changed its key, or is not in the dry run).
    /// Absent when no file missed.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub dry_run_miss_reasons: std::collections::BTreeMap<String, usize>,
    /// The test files whose dry-run facts this run added to the cache. A
    /// file with a failed test or an error is not stored.
    pub dry_run_stored: usize,
    /// The test inputs that the cache key does not cover: modules that a
    /// test loads through a dynamic import that the module graph does not
    /// resolve (`dynamic-imports`), and files that a test reads
    /// (`file-reads`). A change to them does not invalidate a cached result.
    pub not_keyed: Vec<String>,
    /// The selection of the dry-run test files by recorded function reach.
    /// Absent when the run does not select by reach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach_selection: Option<MutationReachSelectionOutput>,
}

/// The selection of the dry-run test files by recorded function reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationReachSelectionOutput {
    /// The mode of the run. In a `check` run, the dry run ran each test
    /// file, and `violations` compares the covering tests with the dropped
    /// files.
    pub mode: MutationReachSelectionMode,
    /// Why the run did not select by reach, when it did not:
    /// `kill-matrix-targets` (a target needs the dry run of its file) or
    /// `runner-shares-modules` (the workers of the runner keep the modules
    /// of one test file for the next one, for example Vitest with
    /// `isolate: false`, so no record is complete).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<String>,
    /// The test files that the reach record left out of the dry run. In a
    /// check run, the files that it would leave out.
    pub dropped: usize,
    /// The paths of the dropped test files, relative to the project root,
    /// sorted.
    pub dropped_files: Vec<String>,
    /// The count of test files that stayed in the dry run, for each reason:
    /// `no-record`, `environment`, `runner-config-changed`, `new-reach-file`,
    /// `unknown-loaded-file`,
    /// `changed-file-without-probes`, `changed-skeleton`,
    /// `changed-entered-function`, `changed-binding`, `mutant-in-loaded-code`,
    /// `loads-mutated-declarator`,
    /// `mutant-in-entered-function` and `mutant-in-file-without-probes`.
    pub kept: std::collections::BTreeMap<String, usize>,
    /// The test files whose reach record this run wrote.
    pub recorded: usize,
    /// In a check run, each mutant that a dropped test file covers. A
    /// non-empty list is a defect of the selection.
    pub violations: Vec<MutationReachViolation>,
}

/// The mode of the reach selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum MutationReachSelectionMode {
    /// Leave out the dropped test files.
    On,
    /// Dry-run each test file and compare.
    Check,
}

/// A mutant that a test file covers although the reach record drops the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationReachViolation {
    /// The dropped test file, relative to the project root.
    pub test_file: String,
    /// The mutant that a test of the file covers.
    pub mutant_id: String,
}
