//! Shared duplicate-code output contracts.

use std::path::PathBuf;

use serde::Serialize;

use crate::serde_path;

/// A single instance of duplicated code at a specific location.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneInstance {
    /// Path to the file containing this clone instance.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// 1-based start line of the clone.
    pub start_line: usize,
    /// 1-based end line of the clone.
    pub end_line: usize,
    /// 0-based start column.
    pub start_col: usize,
    /// 0-based end column.
    pub end_col: usize,
    /// The actual source code fragment.
    pub fragment: String,
}

/// A group of code clones -- the same (or normalized-equivalent) code appearing
/// in multiple places.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneGroup {
    /// All instances where this duplicated code appears.
    pub instances: Vec<CloneInstance>,
    /// Number of tokens in the duplicated block.
    pub token_count: usize,
    /// Number of lines in the duplicated block.
    pub line_count: usize,
}

/// The kind of refactoring suggested for a clone family.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum RefactoringKind {
    /// Extract a shared function/utility.
    ExtractFunction,
    /// Extract a shared module.
    ExtractModule,
}

/// A refactoring suggestion for a clone family.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RefactoringSuggestion {
    /// What kind of refactoring is suggested.
    pub kind: RefactoringKind,
    /// Human-readable description of the suggestion.
    pub description: String,
    /// Estimated lines that could be eliminated.
    pub estimated_savings: usize,
}

/// A clone family: a set of clone groups that share the same file set.
///
/// When multiple clone groups are all duplicated between the same set of files,
/// they form a family, indicating a deeper structural relationship that should
/// be refactored together rather than group-by-group.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneFamily {
    /// The files involved in this family (sorted for stable output).
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub files: Vec<PathBuf>,
    /// Clone groups belonging to this family.
    pub groups: Vec<CloneGroup>,
    /// Total number of duplicated lines across all groups.
    pub total_duplicated_lines: usize,
    /// Total number of duplicated tokens across all groups.
    pub total_duplicated_tokens: usize,
    /// Refactoring suggestions for this family.
    pub suggestions: Vec<RefactoringSuggestion>,
}

/// A detected mirrored directory pattern: two directory prefixes that contain
/// identical files (e.g., `src/` and `deno/lib/`).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MirroredDirectory {
    /// First directory path (lexically smaller).
    pub dir_a: String,
    /// Second directory path.
    pub dir_b: String,
    /// Filenames shared between the two directories.
    pub shared_files: Vec<String>,
    /// Total duplicated lines across all shared files.
    pub total_lines: usize,
}

/// Number of files skipped by one built-in duplicates ignore pattern.
#[derive(Debug, Clone, Default)]
pub struct DefaultIgnoreSkipCount {
    /// Glob pattern that matched skipped files.
    pub pattern: &'static str,
    /// Number of files skipped by this pattern.
    pub count: usize,
}

/// Human-format-only skipped-file stats for built-in duplicates ignores.
#[derive(Debug, Clone, Default)]
pub struct DefaultIgnoreSkips {
    /// Total number of files skipped by built-in duplicates ignores.
    pub total: usize,
    /// Per-pattern skip counts, in default pattern order.
    pub by_pattern: Vec<DefaultIgnoreSkipCount>,
}

/// Overall duplication analysis report.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicationReport {
    /// All detected clone groups. Each group contains 2+ instances of identical
    /// or near-identical code.
    pub clone_groups: Vec<CloneGroup>,
    /// Clone families: groups of clone groups sharing the same file set,
    /// indicating systematic duplication patterns.
    pub clone_families: Vec<CloneFamily>,
    /// Detected mirrored directory trees (directories with many identical files).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mirrored_directories: Vec<MirroredDirectory>,
    /// Aggregate statistics.
    pub stats: DuplicationStats,
}

impl DuplicationReport {
    /// Sort all result arrays for deterministic output ordering.
    ///
    /// Clone groups are sorted by their first instance's file path and line, and
    /// instances within each group are sorted by file path then line. Clone
    /// families are sorted by their file set.
    pub fn sort(&mut self) {
        for group in &mut self.clone_groups {
            group
                .instances
                .sort_by(|a, b| a.file.cmp(&b.file).then(a.start_line.cmp(&b.start_line)));
        }
        self.clone_groups
            .sort_by(|a, b| match (a.instances.first(), b.instances.first()) {
                (Some(ai), Some(bi)) => ai
                    .file
                    .cmp(&bi.file)
                    .then(ai.start_line.cmp(&bi.start_line)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            });

        for family in &mut self.clone_families {
            for group in &mut family.groups {
                group
                    .instances
                    .sort_by(|a, b| a.file.cmp(&b.file).then(a.start_line.cmp(&b.start_line)));
            }
            family
                .groups
                .sort_by(|a, b| match (a.instances.first(), b.instances.first()) {
                    (Some(ai), Some(bi)) => ai
                        .file
                        .cmp(&bi.file)
                        .then(ai.start_line.cmp(&bi.start_line)),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                });
        }
        self.clone_families.sort_by(|a, b| a.files.cmp(&b.files));
    }
}

/// Aggregate duplication statistics.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicationStats {
    /// Total files analyzed.
    pub total_files: usize,
    /// Files containing at least one clone instance.
    pub files_with_clones: usize,
    /// Total lines across all analyzed files.
    pub total_lines: usize,
    /// Lines that are part of at least one clone.
    pub duplicated_lines: usize,
    /// Total tokens across all analyzed files.
    pub total_tokens: usize,
    /// Tokens that are part of at least one clone.
    pub duplicated_tokens: usize,
    /// Number of clone groups in the reported `clone_groups[]` array.
    /// Matches `clone_groups[].length` post `minOccurrences` filtering; the
    /// count of groups hidden by the filter is exposed in
    /// `clone_groups_below_min_occurrences`.
    pub clone_groups: usize,
    /// Total clone instances across all reported groups. Matches the sum of
    /// `clone_groups[].locations[].length` post `minOccurrences` filtering.
    pub clone_instances: usize,
    /// Percentage of duplicated lines (0.0 to 100.0). Always reflects the FULL
    /// corpus, computed BEFORE the `minOccurrences` filter so trend lines and
    /// `threshold` gates stay stable when the filter changes.
    pub duplication_percentage: f64,
    /// Number of clone groups hidden by `duplicates.minOccurrences`. Absent (or
    /// `0`) when the filter is at its default of `2` and nothing was hidden.
    /// Pre-filter clone group count = `clone_groups +
    /// clone_groups_below_min_occurrences`.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub clone_groups_below_min_occurrences: usize,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires &T signature"
)]
const fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}
