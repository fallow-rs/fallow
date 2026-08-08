//! Shared duplicate-code output contracts.

use std::cmp::{Ordering, Reverse};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::serde_path;

/// A single instance of duplicated code at a specific location.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneGroup {
    /// All instances where this duplicated code appears.
    pub instances: Vec<CloneInstance>,
    /// Number of tokens in the duplicated block.
    pub token_count: usize,
    /// Number of lines in the duplicated block.
    pub line_count: usize,
    /// Lowest all-pairs similarity for a near-miss clone group. Exact clone
    /// groups omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub similarity: Option<f64>,
}

/// Whether a clone group is exact or a near-miss match.
///
/// This is derived from the serialized clone-group fields and does not add a
/// separate discriminator to the output contract.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CloneGroupKind {
    /// An exact or normalization-equivalent clone group.
    Exact,
    /// A near-miss group and its lowest all-pairs similarity.
    Near {
        /// Lowest all-pairs similarity for the group.
        similarity: f64,
    },
}

impl CloneGroup {
    /// Return the semantic kind represented by this clone group.
    #[must_use]
    pub fn kind(&self) -> CloneGroupKind {
        self.similarity
            .map_or(CloneGroupKind::Exact, |similarity| CloneGroupKind::Near {
                similarity,
            })
    }

    /// Maximum directory-tree or same-file line distance between instances.
    #[must_use]
    pub fn spread(&self) -> usize {
        clone_group_spread(&self.instances)
    }
}

const SAME_FILE_SPREAD_STEP: usize = 250;
const MAX_RANKED_SPREAD: usize = 8;
const SPREAD_RANK_WEIGHTS: [u64; MAX_RANKED_SPREAD + 1] = [
    1_000_000_000,
    1_047_319_732,
    1_075_000_000,
    1_094_639_463,
    1_109_873_014,
    1_122_319_732,
    1_132_843_281,
    1_141_959_195,
    1_150_000_000,
];

/// Compute the maximum distance between clone instances.
///
/// Instances in different files use lexical parent-directory distance.
/// Instances in the same file use the non-overlapping line gap, rounded up in
/// 250-line steps. The returned value is not capped; only ranking caps spread.
#[must_use]
pub fn clone_group_spread(instances: &[CloneInstance]) -> usize {
    if instances.len() < 2 {
        return 0;
    }

    let mut by_file = instances.iter().collect::<Vec<_>>();
    by_file.sort_unstable_by(|left, right| left.file.cmp(&right.file));

    let mut same_file_max = 0;
    let mut parent_components = Vec::new();
    let mut start = 0;
    while start < by_file.len() {
        let mut end = start + 1;
        while end < by_file.len() && by_file[end].file == by_file[start].file {
            end += 1;
        }

        parent_components.push(path_parent_components(&by_file[start].file));
        if end - start >= 2 {
            let min_end = by_file[start..end]
                .iter()
                .map(|instance| instance.end_line)
                .min()
                .unwrap_or(0);
            let max_start = by_file[start..end]
                .iter()
                .map(|instance| instance.start_line)
                .max()
                .unwrap_or(0);
            let gap = max_start.saturating_sub(min_end).saturating_sub(1);
            same_file_max = same_file_max.max(gap.div_ceil(SAME_FILE_SPREAD_STEP));
        }
        start = end;
    }

    same_file_max.max(directory_tree_diameter(&parent_components))
}

/// Compare clone groups in shared spread-aware priority order.
#[must_use]
pub fn compare_clone_groups(left: &CloneGroup, right: &CloneGroup) -> Ordering {
    clone_group_rank_key(left).cmp(&clone_group_rank_key(right))
}

#[cfg(test)]
fn instance_pair_spread(left: &CloneInstance, right: &CloneInstance) -> usize {
    if left.file == right.file {
        return same_file_spread(left, right);
    }
    directory_distance(&left.file, &right.file)
}

#[cfg(test)]
fn same_file_spread(left: &CloneInstance, right: &CloneInstance) -> usize {
    let gap = if left.end_line < right.start_line {
        right
            .start_line
            .saturating_sub(left.end_line)
            .saturating_sub(1)
    } else if right.end_line < left.start_line {
        left.start_line
            .saturating_sub(right.end_line)
            .saturating_sub(1)
    } else {
        0
    };
    gap.div_ceil(SAME_FILE_SPREAD_STEP)
}

fn path_parent_components(path: &Path) -> Vec<Component<'_>> {
    path.parent()
        .unwrap_or_else(|| Path::new(""))
        .components()
        .collect()
}

fn directory_tree_diameter(paths: &[Vec<Component<'_>>]) -> usize {
    if paths.len() < 2 {
        return 0;
    }

    let endpoint = farthest_path(paths, 0).0;
    farthest_path(paths, endpoint).1
}

fn farthest_path(paths: &[Vec<Component<'_>>], origin: usize) -> (usize, usize) {
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| (index, component_distance(&paths[origin], path)))
        .max_by_key(|&(index, distance)| (distance, index))
        .unwrap_or((origin, 0))
}

fn component_distance(left: &[Component<'_>], right: &[Component<'_>]) -> usize {
    let shared = left
        .iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count();
    left.len()
        .saturating_sub(shared)
        .saturating_add(right.len().saturating_sub(shared))
}

#[cfg(test)]
fn directory_distance(left: &Path, right: &Path) -> usize {
    component_distance(
        &path_parent_components(left),
        &path_parent_components(right),
    )
}

type CloneGroupRankKey = (
    Reverse<u128>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    bool,
    PathBuf,
    usize,
);

fn clone_group_rank_key(group: &CloneGroup) -> CloneGroupRankKey {
    let spread = group.spread();
    let weight = SPREAD_RANK_WEIGHTS[spread.min(MAX_RANKED_SPREAD)];
    let token_count = u128::try_from(group.token_count).unwrap_or(u128::MAX);
    let instance_count = u128::try_from(group.instances.len()).unwrap_or(u128::MAX);
    let score = token_count
        .saturating_mul(instance_count)
        .saturating_mul(u128::from(weight));
    let first = group.instances.iter().min_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then(left.start_line.cmp(&right.start_line))
    });
    (
        Reverse(score),
        Reverse(spread),
        Reverse(group.token_count),
        Reverse(group.instances.len()),
        Reverse(group.line_count),
        first.is_none(),
        first.map_or_else(PathBuf::new, |instance| instance.file.clone()),
        first.map_or(0, |instance| instance.start_line),
    )
}

fn sort_clone_groups(groups: &mut [CloneGroup]) {
    groups.sort_by_cached_key(clone_group_rank_key);
}

/// The kind of refactoring suggested for a clone family.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum RefactoringKind {
    /// Extract a shared function/utility.
    ExtractFunction,
    /// Extract a shared module.
    ExtractModule,
}

/// A refactoring suggestion for a clone family.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Clone groups use spread-aware priority order, instances use file path and
    /// line order, and clone families use their file set.
    pub fn sort(&mut self) {
        for group in &mut self.clone_groups {
            group
                .instances
                .sort_by(|a, b| a.file.cmp(&b.file).then(a.start_line.cmp(&b.start_line)));
        }
        sort_clone_groups(&mut self.clone_groups);

        for family in &mut self.clone_families {
            for group in &mut family.groups {
                group
                    .instances
                    .sort_by(|a, b| a.file.cmp(&b.file).then(a.start_line.cmp(&b.start_line)));
            }
            sort_clone_groups(&mut family.groups);
        }
        self.clone_families.sort_by(|a, b| a.files.cmp(&b.files));
    }
}

/// Aggregate duplication statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Tokens in redundant clone copies, excluding one retained copy per group.
    pub duplicated_tokens: usize,
    /// Number of clone groups in the reported `clone_groups[]` array after
    /// filtering and optional `--top` truncation.
    pub clone_groups: usize,
    /// Total clone instances across all reported groups after filtering and
    /// optional `--top` truncation.
    pub clone_instances: usize,
    /// Percentage of duplicated lines (0.0 to 100.0). `--top` does not change
    /// this scoped corpus metric.
    pub duplication_percentage: f64,
    /// Number of clone groups hidden by `duplicates.minOccurrences`. Absent (or
    /// `0`) when the filter is at its default of `2` and nothing was hidden.
    /// This counter covers only the minimum-occurrence filter.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub clone_groups_below_min_occurrences: usize,
    /// Number of clone groups hidden by `duplicates.ignoredClones`.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub clone_groups_ignored: usize,
    /// Near-miss candidate comparisons skipped by bounded-work limits.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub near_candidates_skipped: usize,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires &T signature"
)]
const fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn pairwise_clone_group_spread(instances: &[CloneInstance]) -> usize {
        let mut spread = 0;
        for (index, left) in instances.iter().enumerate() {
            for right in &instances[index + 1..] {
                spread = spread.max(instance_pair_spread(left, right));
            }
        }
        spread
    }

    fn instance(file: &str, start_line: usize, end_line: usize) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(file),
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn group(instances: Vec<CloneInstance>, token_count: usize, line_count: usize) -> CloneGroup {
        CloneGroup {
            instances,
            token_count,
            line_count,
            similarity: None,
        }
    }

    #[test]
    fn spread_counts_non_shared_parent_components() {
        let clone = group(
            vec![
                instance("/repo/packages/a/src/a.ts", 1, 10),
                instance("/repo/packages/b/src/b.ts", 1, 10),
            ],
            100,
            10,
        );
        assert_eq!(clone.spread(), 4);
    }

    #[test]
    fn spread_is_zero_for_different_files_in_the_same_directory() {
        let clone = group(
            vec![
                instance("/repo/src/a.ts", 1, 10),
                instance("/repo/src/b.ts", 1, 10),
            ],
            100,
            10,
        );
        assert_eq!(clone.spread(), 0);
    }

    #[test]
    fn adjacent_same_file_instances_have_zero_spread() {
        let clone = group(
            vec![
                instance("/repo/src/a.ts", 1, 10),
                instance("/repo/src/a.ts", 11, 20),
            ],
            100,
            10,
        );
        assert_eq!(clone.spread(), 0);
    }

    #[test]
    fn same_file_spread_uses_ceiling_rounded_intervening_lines() {
        let clone = group(
            vec![
                instance("/repo/src/a.ts", 1, 10),
                instance("/repo/src/a.ts", 260, 269),
                instance("/repo/src/a.ts", 261, 270),
                instance("/repo/src/a.ts", 262, 271),
            ],
            100,
            10,
        );
        assert_eq!(clone_group_spread(&clone.instances[..2]), 1);
        assert_eq!(clone_group_spread(&clone.instances[..3]), 1);
        assert_eq!(clone.spread(), 2);
    }

    #[test]
    fn overlapping_same_file_instances_have_zero_spread() {
        let clone = group(
            vec![
                instance("/repo/src/a.ts", 1, 20),
                instance("/repo/src/a.ts", 10, 30),
            ],
            100,
            20,
        );
        assert_eq!(clone.spread(), 0);
    }

    #[test]
    fn directory_diameter_handles_ties_and_mixed_roots() {
        let instances = vec![
            instance("src/a.ts", 1, 10),
            instance("packages/a/b.ts", 1, 10),
            instance("packages/c/d.ts", 1, 10),
            instance("/repo/src/e.ts", 1, 10),
        ];

        assert_eq!(
            clone_group_spread(&instances),
            pairwise_clone_group_spread(&instances)
        );
    }

    #[cfg(windows)]
    #[test]
    fn directory_diameter_handles_windows_prefixes() {
        let instances = vec![
            instance(r"C:\repo\src\a.ts", 1, 10),
            instance(r"C:\repo\packages\b.ts", 1, 10),
            instance(r"D:\other\c.ts", 1, 10),
        ];

        assert_eq!(
            clone_group_spread(&instances),
            pairwise_clone_group_spread(&instances)
        );
    }

    #[test]
    fn clone_group_kind_does_not_change_serialized_contract() {
        let mut clone = group(vec![instance("src/a.ts", 1, 10)], 20, 10);
        assert_eq!(clone.kind(), CloneGroupKind::Exact);
        assert!(serde_json::to_value(&clone).unwrap()["similarity"].is_null());

        clone.similarity = Some(0.85);
        assert_eq!(clone.kind(), CloneGroupKind::Near { similarity: 0.85 });
        assert_eq!(serde_json::to_value(&clone).unwrap()["similarity"], 0.85);
    }

    mod proptests {
        use super::*;

        proptest! {
            #[test]
            fn optimized_spread_matches_pairwise_reference(
                entries in prop::collection::vec((0_u8..8, 1_usize..4_000, 1_usize..500), 0..80)
            ) {
                let paths = [
                    "src/a.ts",
                    "src/b.ts",
                    "packages/a/src/c.ts",
                    "packages/b/src/d.ts",
                    "packages/b/test/e.ts",
                    "/repo/apps/web/f.ts",
                    "/repo/crates/core/g.ts",
                    "h.ts",
                ];
                let instances = entries
                    .into_iter()
                    .map(|(path, start, len)| instance(paths[usize::from(path)], start, start + len))
                    .collect::<Vec<_>>();

                prop_assert_eq!(
                    clone_group_spread(&instances),
                    pairwise_clone_group_spread(&instances)
                );
            }
        }
    }

    #[test]
    fn ranking_uses_spread_without_overriding_a_larger_base_score() {
        let distant = group(
            vec![
                instance("/repo/a/b/c/d/e/a.ts", 1, 10),
                instance("/repo/f/g/h/i/j/b.ts", 1, 10),
            ],
            100,
            10,
        );
        let slightly_larger_local = group(
            vec![
                instance("/repo/src/a.ts", 1, 10),
                instance("/repo/src/b.ts", 1, 10),
            ],
            116,
            10,
        );
        assert_eq!(distant.spread(), 10);
        assert_eq!(
            compare_clone_groups(&distant, &slightly_larger_local),
            Ordering::Greater
        );

        let slightly_smaller_local = group(
            vec![
                instance("/repo/src/c.ts", 1, 10),
                instance("/repo/src/d.ts", 1, 10),
            ],
            114,
            10,
        );
        assert_eq!(
            compare_clone_groups(&distant, &slightly_smaller_local),
            Ordering::Less
        );
    }

    #[test]
    fn report_sort_uses_canonical_location_as_final_tiebreaker() {
        let later = group(
            vec![
                instance("/repo/src/z.ts", 1, 10),
                instance("/repo/src/y.ts", 1, 10),
            ],
            100,
            10,
        );
        let earlier = group(
            vec![
                instance("/repo/src/a.ts", 20, 29),
                instance("/repo/src/b.ts", 20, 29),
            ],
            100,
            10,
        );
        let mut report = DuplicationReport {
            clone_groups: vec![later, earlier],
            ..DuplicationReport::default()
        };
        report.sort();
        assert_eq!(
            report.clone_groups[0].instances[0].file,
            Path::new("/repo/src/a.ts")
        );
    }

    #[test]
    fn exact_clone_similarity_is_omitted() {
        let clone = group(Vec::new(), 100, 10);
        let value = serde_json::to_value(clone).unwrap();
        assert!(value.get("similarity").is_none());
    }

    #[test]
    fn near_clone_similarity_is_serialized() {
        let mut clone = group(Vec::new(), 100, 10);
        clone.similarity = Some(0.85);
        let value = serde_json::to_value(clone).unwrap();
        assert_eq!(value["similarity"], 0.85);
    }

    #[test]
    fn optional_duplication_stats_are_omitted_at_zero() {
        let empty = serde_json::to_value(DuplicationStats::default()).unwrap();
        assert!(empty.get("clone_groups_ignored").is_none());
        assert!(empty.get("near_candidates_skipped").is_none());

        let populated = serde_json::to_value(DuplicationStats {
            clone_groups_ignored: 2,
            near_candidates_skipped: 3,
            ..DuplicationStats::default()
        })
        .unwrap();
        assert_eq!(populated["clone_groups_ignored"], 2);
        assert_eq!(populated["near_candidates_skipped"], 3);
    }
}
