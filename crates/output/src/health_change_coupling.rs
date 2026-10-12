//! Change coupling section of the health report.

/// Hidden change coupling: pairs of files that change together in the git
/// history while no import or re-export chain connects them.
///
/// An agent that follows the import graph cannot find the second file of a
/// hidden pair. The section is advisory: it has no health score weight and
/// never gates.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeCouplingReport {
    /// History window that the section reads, for example `"12 months"`.
    /// Without `--since` the window is 12 months, wider than the hotspot
    /// window.
    pub since: String,
    /// Commits that changed at least one analyzed file and were not skipped.
    pub commits_analyzed: usize,
    /// Commits skipped because they changed more analyzed files than
    /// `thresholds.max_files_per_commit`.
    pub commits_skipped: usize,
    /// Hidden pairs in scope before `--top` cuts `pairs`.
    pub total_pairs: usize,
    /// Thresholds that this run applied.
    pub thresholds: ChangeCouplingThresholds,
    /// Hidden pairs, sorted by `degree` and then `shared_revisions` (both
    /// descending), then by path. Empty when the history cannot show
    /// coupling; `workspace_diagnostics` then tells why.
    pub pairs: Vec<HiddenCouplingPair>,
}

/// Thresholds that a hidden pair met.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeCouplingThresholds {
    /// Commits with more analyzed files than this are skipped.
    pub max_files_per_commit: usize,
    /// Minimum commits for each file of a pair.
    pub min_revisions: u32,
    /// Minimum commits that change both files of a pair.
    pub min_shared_revisions: u32,
    /// Minimum coupling degree, from 0 to 1.
    pub min_degree: f64,
    /// Degree at or above which a pair is `strong`.
    pub strong_degree: f64,
}

/// Two files that change together without a static edge between them.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HiddenCouplingPair {
    /// First file, relative to the project root. Sorts before `path_b`.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path_a: std::path::PathBuf,
    /// Second file, relative to the project root.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path_b: std::path::PathBuf,
    /// Commits that changed `path_a` in the window.
    pub revisions_a: u32,
    /// Commits that changed `path_b` in the window.
    pub revisions_b: u32,
    /// Commits that changed both files.
    pub shared_revisions: u32,
    /// Coupling degree from 0 to 1: `shared_revisions` divided by the mean of
    /// `revisions_a` and `revisions_b`.
    pub degree: f64,
    /// True when `degree` reaches `thresholds.strong_degree`. A strong pair
    /// also appears as a `resolve_hidden_coupling` refactoring target.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub strong: bool,
}
