//! Branching conservation for the review brief.
//!
//! A per-function complexity ceiling constrains a partition, not a quantity.
//! Splitting a function moves its branches into new functions instead of
//! removing them, so every per-unit metric improves while the total branching
//! is unchanged. This section reports the total against the number of functions
//! now holding it, so a reader can tell the two apart.
//!
//! Reviewer-private; never gates.

use fallow_types::extract::FileBranching;
use rustc_hash::FxHashMap;
use serde::Serialize;

/// Branching totals per root-relative path, for one revision.
pub type BranchingSnapshot = FxHashMap<String, FileBranching>;

/// How many branch points a total may move before the change is treated as
/// flat. Absolute rather than proportional: a small changeset is where a
/// two-branch move matters most, and a proportional band would blind exactly
/// that case.
pub const DEFAULT_BRANCHING_TOLERANCE: u32 = 2;

/// How many files the payload names before it starts counting instead.
const MAX_BY_FILE: usize = 5;

/// What the comparison could establish about the changeset.
///
/// There is deliberately no "branching removed" value. Increments outside every
/// function are invisible to the underlying count, so a fall in branch points
/// is not proof that branching was removed: it is equally consistent with a
/// branch having been hoisted to module scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum BranchingVerdict {
    /// Branching was demonstrably relocated rather than removed.
    BranchingMoved,
    /// Both the branching and the number of functions holding it are flat.
    BranchingUnchanged,
    /// Neither could be established. `reason` says why.
    Inconclusive,
}

/// Why the comparison abstained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum BranchingInconclusiveReason {
    /// The changeset moved branching and function count together in a way that
    /// matches feature work as readily as a split.
    NoTransferSignature,
    /// Too little surviving code carrying units to compare.
    SetTooSmall,
}

/// One metric across the two revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BranchingMetric {
    /// Value on the base revision.
    pub previous: u32,
    /// Value on the head revision.
    pub current: u32,
    /// `current - previous`. Signed, so a consumer never has to infer direction
    /// from a separate field.
    pub delta: i64,
}

impl BranchingMetric {
    fn new(previous: u32, current: u32) -> Self {
        Self {
            previous,
            current,
            delta: i64::from(current) - i64::from(previous),
        }
    }
}

/// Where a cognitive-complexity improvement came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CognitiveAttribution {
    /// Nesting depth was reset, which extraction does for free. No branching
    /// left.
    NestingReset,
    /// Branch points genuinely fell.
    BranchesRemoved,
    /// Both moved.
    Mixed,
}

/// The cognitive figure and what drove it.
///
/// `previous` and `current` exclude prop-count and hook-density increments.
/// Both are cognitive-only, and prop count records an excess over a floor, so
/// it is superlinear in a split and would move this number with branching and
/// nesting both flat. This therefore does not match the cognitive score the
/// complexity findings report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BranchingCognitive {
    /// Cognitive weight on the base revision.
    pub previous: u32,
    /// Cognitive weight on the head revision.
    pub current: u32,
    /// `current - previous`.
    pub delta: i64,
    /// Change in the summed nesting depth behind those increments.
    pub nesting_weight_delta: i64,
    /// What the improvement is attributable to, absent when cognitive did not
    /// fall. There is nothing to attribute when the number rose or held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attributed_to: Option<CognitiveAttribution>,
}

/// Size and composition of the compared set.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BranchingScope {
    /// Files carrying units on both revisions.
    pub files_both: u32,
    /// Files carrying units on the head revision only.
    pub files_added: u32,
    /// Files that carried units on the base revision only. Reported, and
    /// excluded from every headline number: a deleted file contributes its
    /// whole base-side total as a fall with no head counterpart.
    pub files_deleted: u32,
    /// Branch points on test-shaped paths within the head totals. Test code
    /// routinely dominates both terms, so a reader needs to see its share
    /// before reading the headline.
    pub test_branch_points: u32,
    /// Functions on test-shaped paths within the head totals.
    pub test_functions: u32,
    /// Share of head branch points owned by the single largest file, so a
    /// reader can see when one vendored or generated file owns the number.
    pub largest_file_share_of_branch_points: f64,
}

/// One file's contribution to the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BranchingFileDelta {
    /// Root-relative path.
    pub path: String,
    /// Change in branch points for this file.
    pub branch_points_delta: i64,
    /// Change in accounted functions for this file.
    pub functions_delta: i64,
}

/// The brief's branching section.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BranchingReport {
    /// What the comparison established.
    pub verdict: BranchingVerdict,
    /// Why it abstained, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<BranchingInconclusiveReason>,
    /// The band inside which a move counts as flat. Published because a verdict
    /// against an unpublished threshold is not reproducible by a consumer.
    pub tolerance: u32,
    /// Size and composition of the compared set.
    pub scope: BranchingScope,
    /// The conserved quantity, over the surviving partition.
    pub branch_points: BranchingMetric,
    /// The number of functions holding it, over the surviving partition.
    pub functions: BranchingMetric,
    /// Highest single-function score. Reported, never a verdict input: a split
    /// lowers it by construction, which is the whole reason this section exists.
    pub peak_unit_cyclomatic: BranchingMetric,
    /// Base-side branch points of the deleted partition.
    pub branch_points_in_deleted_files: u32,
    /// The cognitive figure and what drove it.
    pub cognitive: BranchingCognitive,
    /// The files that moved the numbers most, largest absolute branch-point
    /// change first.
    pub by_file: Vec<BranchingFileDelta>,
    /// Files with a change that the list did not name.
    pub by_file_omitted: u32,
}

/// Totals over one partition of one revision.
#[derive(Default, Clone, Copy)]
struct Totals {
    branch_points: u32,
    functions: u32,
    peak: u16,
    cognitive: u32,
    nesting: u32,
}

impl Totals {
    fn add(&mut self, file: FileBranching) {
        self.branch_points += file.branch_points;
        self.functions += file.functions;
        self.peak = self.peak.max(file.peak_cyclomatic);
        self.cognitive += file.cognitive;
        self.nesting += file.cognitive_nesting_weight;
    }
}

/// The accounting set split by file disposition, with the per-file deltas.
///
/// Disposition comes from which side carries units for a path, not from git:
/// both revisions are analyzed in full, so presence in each map already answers
/// it, and no second status query can disagree with the totals being compared.
struct Partition {
    surviving_base: Totals,
    surviving_head: Totals,
    added: Totals,
    deleted: Totals,
    scope: BranchingScope,
    deltas: Vec<BranchingFileDelta>,
    moved_in_place: bool,
}

fn partition(
    base: &BranchingSnapshot,
    head: &BranchingSnapshot,
    tolerance: u32,
    is_test_path: &dyn Fn(&str) -> bool,
) -> Partition {
    let mut out = Partition {
        surviving_base: Totals::default(),
        surviving_head: Totals::default(),
        added: Totals::default(),
        deleted: Totals::default(),
        scope: BranchingScope {
            files_both: 0,
            files_added: 0,
            files_deleted: 0,
            test_branch_points: 0,
            test_functions: 0,
            largest_file_share_of_branch_points: 0.0,
        },
        deltas: Vec::new(),
        moved_in_place: false,
    };
    let mut test_totals = Totals::default();

    for (path, head_file) in head {
        out.surviving_head.add(*head_file);
        if is_test_path(path) {
            test_totals.add(*head_file);
        }
        let Some(base_file) = base.get(path) else {
            out.scope.files_added += 1;
            out.added.add(*head_file);
            out.deltas.push(BranchingFileDelta {
                path: path.clone(),
                branch_points_delta: i64::from(head_file.branch_points),
                functions_delta: i64::from(head_file.functions),
            });
            continue;
        };
        out.scope.files_both += 1;
        out.surviving_base.add(*base_file);
        let branch_delta = i64::from(head_file.branch_points) - i64::from(base_file.branch_points);
        let function_delta = i64::from(head_file.functions) - i64::from(base_file.functions);
        // Branching held while the file grew functions and its worst one got
        // smaller: the branches were repartitioned inside this file.
        if branch_delta.unsigned_abs() <= u64::from(tolerance)
            && function_delta > 0
            && head_file.peak_cyclomatic < base_file.peak_cyclomatic
        {
            out.moved_in_place = true;
        }
        if branch_delta != 0 || function_delta != 0 {
            out.deltas.push(BranchingFileDelta {
                path: path.clone(),
                branch_points_delta: branch_delta,
                functions_delta: function_delta,
            });
        }
    }

    for (path, base_file) in base {
        if !head.contains_key(path) {
            out.scope.files_deleted += 1;
            out.deleted.add(*base_file);
        }
    }

    out.scope.test_branch_points = test_totals.branch_points;
    out.scope.test_functions = test_totals.functions;
    out
}

fn scope_largest(head: &BranchingSnapshot) -> u32 {
    head.values()
        .map(|file| file.branch_points)
        .max()
        .unwrap_or(0)
}

impl BranchingReport {
    /// Compare two revisions over the accounting set.
    ///
    /// `is_test_path` classifies a root-relative path; the caller owns that
    /// heuristic. Both maps must already be restricted to the accounting set
    /// and keyed in the same space, with base-side renames remapped onto head
    /// paths.
    #[must_use]
    pub fn compare(
        base: &BranchingSnapshot,
        head: &BranchingSnapshot,
        tolerance: u32,
        is_test_path: &dyn Fn(&str) -> bool,
    ) -> Self {
        let Partition {
            surviving_base,
            surviving_head,
            added,
            deleted,
            mut scope,
            mut deltas,
            moved_in_place,
        } = partition(base, head, tolerance, is_test_path);

        scope.largest_file_share_of_branch_points = if surviving_head.branch_points == 0 {
            0.0
        } else {
            f64::from(scope_largest(head)) / f64::from(surviving_head.branch_points)
        };

        // The surviving partition is `both` plus `added` on the head side, and
        // `both` alone on the base side, so the two sides describe different
        // populations. Every added file inflates the head function count while
        // carrying almost no branches, which is why the verdict rests on a
        // transfer test rather than on the sign of these deltas.
        let both_branch_delta = i64::from(
            surviving_head
                .branch_points
                .saturating_sub(added.branch_points),
        ) - i64::from(surviving_base.branch_points);
        // A transfer out of the surviving files, not merely new code arriving:
        // the added files must carry more than the tolerance, the pre-existing
        // files must have fallen beyond it, and the two must approximately
        // cancel. Without all three, an ordinary commit that adds a file reads
        // as a split.
        let moved_out = added.branch_points > tolerance
            && both_branch_delta <= -i64::from(tolerance)
            && (both_branch_delta + i64::from(added.branch_points)).unsigned_abs()
                <= u64::from(tolerance);

        let branch_points =
            BranchingMetric::new(surviving_base.branch_points, surviving_head.branch_points);
        let functions = BranchingMetric::new(surviving_base.functions, surviving_head.functions);
        let peak_unit_cyclomatic = BranchingMetric::new(
            u32::from(surviving_base.peak),
            u32::from(surviving_head.peak),
        );
        let cognitive_delta =
            i64::from(surviving_head.cognitive) - i64::from(surviving_base.cognitive);
        let nesting_weight_delta =
            i64::from(surviving_head.nesting) - i64::from(surviving_base.nesting);

        let (verdict, reason) = if surviving_head.functions == 0 && surviving_base.functions == 0 {
            (
                BranchingVerdict::Inconclusive,
                Some(BranchingInconclusiveReason::SetTooSmall),
            )
        } else if moved_in_place || moved_out {
            (BranchingVerdict::BranchingMoved, None)
        } else if branch_points.delta.unsigned_abs() <= u64::from(tolerance)
            && functions.delta.unsigned_abs() <= u64::from(tolerance)
        {
            (BranchingVerdict::BranchingUnchanged, None)
        } else {
            (
                BranchingVerdict::Inconclusive,
                Some(BranchingInconclusiveReason::NoTransferSignature),
            )
        };

        deltas.sort_by(|a, b| {
            b.branch_points_delta
                .abs()
                .cmp(&a.branch_points_delta.abs())
                .then_with(|| b.functions_delta.abs().cmp(&a.functions_delta.abs()))
                .then_with(|| a.path.cmp(&b.path))
        });
        let by_file_omitted = u32::try_from(deltas.len().saturating_sub(MAX_BY_FILE)).unwrap_or(0);
        deltas.truncate(MAX_BY_FILE);

        Self {
            verdict,
            reason,
            tolerance,
            scope,
            branch_points,
            functions,
            peak_unit_cyclomatic,
            branch_points_in_deleted_files: deleted.branch_points,
            cognitive: BranchingCognitive {
                previous: surviving_base.cognitive,
                current: surviving_head.cognitive,
                delta: cognitive_delta,
                nesting_weight_delta,
                attributed_to: attribute_cognitive(
                    cognitive_delta,
                    branch_points.delta,
                    nesting_weight_delta,
                    tolerance,
                ),
            },
            by_file: deltas,
            by_file_omitted,
        }
    }

    /// Whether the section says anything a reader needs. A flat or abstaining
    /// comparison is rendered as nothing in the human brief, matching every
    /// sibling section.
    #[must_use]
    pub const fn is_reportable(&self) -> bool {
        matches!(self.verdict, BranchingVerdict::BranchingMoved)
    }
}

/// Split a cognitive improvement between nesting and branching.
///
/// Extraction rebases nesting to zero on every new frame, so it lowers cognitive
/// without removing a single branch. Flattening an else ladder into guard
/// clauses also leaves branch points flat, because an `else if` carries the same
/// increment a plain `if` does, so `nesting-reset` covers both. The function
/// count is what separates them, and it is reported beside this.
///
/// Returns `None` unless cognitive actually fell. Attributing a rise to
/// "branches removed" reads as the opposite of what happened, which is what a
/// real split through a nullish-coalescing chain produces: branching up,
/// cognitive up, and nothing to attribute.
fn attribute_cognitive(
    cognitive_delta: i64,
    branch_delta: i64,
    nesting_weight_delta: i64,
    tolerance: u32,
) -> Option<CognitiveAttribution> {
    if cognitive_delta >= 0 {
        return None;
    }
    let branches_removed = branch_delta < -i64::from(tolerance);
    let nesting_reset = nesting_weight_delta < 0;
    Some(match (branches_removed, nesting_reset) {
        (true, true) => CognitiveAttribution::Mixed,
        (true, false) => CognitiveAttribution::BranchesRemoved,
        _ => CognitiveAttribution::NestingReset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(branch_points: u32, functions: u32, peak: u16) -> FileBranching {
        FileBranching {
            branch_points,
            functions,
            peak_cyclomatic: peak,
            cognitive: branch_points,
            cognitive_nesting_weight: 0,
        }
    }

    fn snapshot(entries: &[(&str, FileBranching)]) -> BranchingSnapshot {
        entries
            .iter()
            .map(|(path, totals)| ((*path).to_string(), *totals))
            .collect()
    }

    fn compare(base: &BranchingSnapshot, head: &BranchingSnapshot) -> BranchingReport {
        BranchingReport::compare(base, head, DEFAULT_BRANCHING_TOLERANCE, &|path| {
            path.contains(".test.")
        })
    }

    #[test]
    fn a_split_inside_one_file_is_a_move() {
        let base = snapshot(&[("src/a.ts", file(39, 1, 40))]);
        let head = snapshot(&[("src/a.ts", file(39, 8, 6))]);

        let report = compare(&base, &head);

        assert_eq!(report.verdict, BranchingVerdict::BranchingMoved);
        assert_eq!(report.branch_points.delta, 0, "the branching is conserved");
        assert_eq!(report.functions.delta, 7, "seven new functions hold it");
        assert_eq!(report.peak_unit_cyclomatic.delta, -34);
        assert!(report.is_reportable());
    }

    #[test]
    fn extraction_into_a_new_file_is_a_move() {
        let base = snapshot(&[("src/a.ts", file(30, 1, 31))]);
        let head = snapshot(&[
            ("src/a.ts", file(10, 1, 11)),
            ("src/helpers.ts", file(20, 4, 6)),
        ]);

        let report = compare(&base, &head);

        assert_eq!(report.verdict, BranchingVerdict::BranchingMoved);
        assert_eq!(report.scope.files_added, 1);
    }

    #[test]
    fn adding_a_branching_feature_is_not_a_move() {
        // The failure mode the transfer test exists to prevent: an ordinary
        // commit adds files, so unit count rises with branching flat, which a
        // naive "D flat while U rose" rule would brand a split.
        let base = snapshot(&[("src/a.ts", file(30, 5, 8))]);
        let head = snapshot(&[
            ("src/a.ts", file(30, 5, 8)),
            ("src/feature.ts", file(9, 6, 4)),
        ]);

        let report = compare(&base, &head);

        assert_ne!(report.verdict, BranchingVerdict::BranchingMoved);
        assert!(!report.is_reportable());
    }

    #[test]
    fn callbacks_added_alongside_a_feature_stay_unclassified() {
        // New code is dominated by zero-branch callbacks, so `dD` near zero
        // with `dU` up is the default state of a commit that adds a file.
        let base = snapshot(&[("src/a.ts", file(30, 5, 8))]);
        let head = snapshot(&[("src/a.ts", file(30, 5, 8)), ("src/new.ts", file(1, 20, 2))]);

        let report = compare(&base, &head);

        assert_eq!(report.verdict, BranchingVerdict::Inconclusive);
        assert_eq!(
            report.reason,
            Some(BranchingInconclusiveReason::NoTransferSignature)
        );
        assert_eq!(
            report.functions.delta, 20,
            "the numbers stay populated while abstaining"
        );
    }

    #[test]
    fn a_deleted_file_is_reported_and_kept_out_of_the_headline() {
        let base = snapshot(&[
            ("src/a.ts", file(30, 5, 8)),
            ("src/gone.ts", file(300, 9, 40)),
        ]);
        let head = snapshot(&[("src/a.ts", file(30, 5, 8))]);

        let report = compare(&base, &head);

        assert_eq!(report.scope.files_deleted, 1);
        assert_eq!(report.branch_points_in_deleted_files, 300);
        assert_eq!(
            report.branch_points.delta, 0,
            "the deleted file's branches never enter the comparison"
        );
        assert_eq!(report.verdict, BranchingVerdict::BranchingUnchanged);
    }

    #[test]
    fn a_flat_changeset_is_unchanged_and_not_rendered() {
        let base = snapshot(&[("src/a.ts", file(12, 3, 5))]);
        let head = snapshot(&[("src/a.ts", file(12, 3, 5))]);

        let report = compare(&base, &head);

        assert_eq!(report.verdict, BranchingVerdict::BranchingUnchanged);
        assert!(!report.is_reportable());
    }

    #[test]
    fn an_empty_accounting_set_abstains() {
        let report = compare(&snapshot(&[]), &snapshot(&[]));

        assert_eq!(report.verdict, BranchingVerdict::Inconclusive);
        assert_eq!(
            report.reason,
            Some(BranchingInconclusiveReason::SetTooSmall)
        );
    }

    #[test]
    fn a_fall_in_one_file_and_a_rise_in_another_is_visible_per_file() {
        // A set-level scalar cannot localize, so the per-file list is what
        // keeps a mixed changeset readable.
        let base = snapshot(&[("src/a.ts", file(20, 4, 9)), ("src/b.ts", file(10, 3, 6))]);
        let head = snapshot(&[("src/a.ts", file(10, 4, 9)), ("src/b.ts", file(40, 3, 6))]);

        let report = compare(&base, &head);

        assert_eq!(report.by_file.len(), 2);
        assert_eq!(report.by_file[0].path, "src/b.ts", "largest move first");
        assert_eq!(report.by_file[0].branch_points_delta, 30);
        assert_eq!(report.by_file[1].branch_points_delta, -10);
    }

    #[test]
    fn test_paths_are_reported_separately() {
        let base = snapshot(&[("src/a.ts", file(10, 2, 6))]);
        let head = snapshot(&[
            ("src/a.ts", file(10, 2, 6)),
            ("src/a.test.ts", file(40, 30, 3)),
        ]);

        let report = compare(&base, &head);

        assert_eq!(report.scope.test_branch_points, 40);
        assert_eq!(report.scope.test_functions, 30);
        assert_eq!(
            report.branch_points.current, 50,
            "the headline still totals"
        );
    }

    #[test]
    fn one_dominant_file_is_visible_in_the_share() {
        let base = snapshot(&[("src/a.ts", file(1, 1, 2))]);
        let head = snapshot(&[
            ("src/a.ts", file(1, 1, 2)),
            ("src/vendor/bundle.js", file(99, 5, 40)),
        ]);

        let report = compare(&base, &head);

        assert!(
            (report.scope.largest_file_share_of_branch_points - 0.99).abs() < 1e-9,
            "one file owns the number: {}",
            report.scope.largest_file_share_of_branch_points
        );
    }

    #[test]
    fn the_file_list_is_capped_and_the_remainder_counted() {
        let base = snapshot(&[]);
        let head = snapshot(&[
            ("src/a.ts", file(9, 1, 3)),
            ("src/b.ts", file(8, 1, 3)),
            ("src/c.ts", file(7, 1, 3)),
            ("src/d.ts", file(6, 1, 3)),
            ("src/e.ts", file(5, 1, 3)),
            ("src/f.ts", file(4, 1, 3)),
            ("src/g.ts", file(3, 1, 3)),
        ]);

        let report = compare(&base, &head);

        assert_eq!(report.by_file.len(), 5);
        assert_eq!(report.by_file_omitted, 2);
        assert_eq!(report.by_file[0].path, "src/a.ts");
    }

    #[test]
    fn a_cognitive_win_with_flat_branching_is_a_nesting_reset() {
        let base = snapshot(&[(
            "src/a.ts",
            FileBranching {
                branch_points: 10,
                functions: 1,
                peak_cyclomatic: 11,
                cognitive: 40,
                cognitive_nesting_weight: 30,
            },
        )]);
        let head = snapshot(&[(
            "src/a.ts",
            FileBranching {
                branch_points: 10,
                functions: 6,
                peak_cyclomatic: 4,
                cognitive: 12,
                cognitive_nesting_weight: 2,
            },
        )]);

        let report = compare(&base, &head);

        assert_eq!(report.verdict, BranchingVerdict::BranchingMoved);
        assert_eq!(report.cognitive.delta, -28);
        assert_eq!(
            report.cognitive.attributed_to,
            Some(CognitiveAttribution::NestingReset),
            "the cognitive win came from repartitioning, not from removing branches"
        );
    }

    #[test]
    fn a_cognitive_win_with_branching_gone_is_attributed_to_branches() {
        let base = snapshot(&[(
            "src/a.ts",
            FileBranching {
                branch_points: 30,
                functions: 2,
                peak_cyclomatic: 20,
                cognitive: 30,
                cognitive_nesting_weight: 0,
            },
        )]);
        let head = snapshot(&[(
            "src/a.ts",
            FileBranching {
                branch_points: 5,
                functions: 2,
                peak_cyclomatic: 4,
                cognitive: 5,
                cognitive_nesting_weight: 0,
            },
        )]);

        let report = compare(&base, &head);

        assert_eq!(
            report.cognitive.attributed_to,
            Some(CognitiveAttribution::BranchesRemoved)
        );
    }

    #[test]
    fn a_cognitive_rise_is_attributed_to_nothing() {
        // Measured on a real split routed through a nullish-coalescing chain:
        // branching up, cognitive up. Labelling that "branches removed" reads
        // as the opposite of what happened.
        let base = snapshot(&[(
            "src/a.ts",
            FileBranching {
                branch_points: 7,
                functions: 1,
                peak_cyclomatic: 8,
                cognitive: 10,
                cognitive_nesting_weight: 3,
            },
        )]);
        let head = snapshot(&[(
            "src/a.ts",
            FileBranching {
                branch_points: 11,
                functions: 5,
                peak_cyclomatic: 5,
                cognitive: 11,
                cognitive_nesting_weight: 3,
            },
        )]);

        let report = compare(&base, &head);

        assert_eq!(report.cognitive.delta, 1);
        assert_eq!(report.cognitive.attributed_to, None);
        assert_eq!(
            report.verdict,
            BranchingVerdict::Inconclusive,
            "branching rose beyond tolerance, so this is not a clean transfer"
        );
    }

    #[test]
    fn every_region_of_the_verdict_space_is_assigned() {
        // The verdict must be total: no combination of deltas may fall through
        // without a value.
        for base_branches in [0_u32, 5, 40] {
            for head_branches in [0_u32, 5, 40] {
                for base_functions in [0_u32, 1, 9] {
                    for head_functions in [0_u32, 1, 9] {
                        let base =
                            snapshot(&[("src/a.ts", file(base_branches, base_functions, 9))]);
                        let head =
                            snapshot(&[("src/a.ts", file(head_branches, head_functions, 4))]);
                        let report = compare(&base, &head);
                        assert_eq!(
                            report.reason.is_some(),
                            report.verdict == BranchingVerdict::Inconclusive,
                            "a reason is present exactly when the verdict abstains: \
                             {base_branches}/{base_functions} to {head_branches}/{head_functions}"
                        );
                    }
                }
            }
        }
    }
}
