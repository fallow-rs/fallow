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
    /// The branch-point count fell. Deliberately a statement about the count
    /// and not about the program: increments outside every function are
    /// invisible here, which is the same reason there is no "branching
    /// removed" verdict.
    FewerBranchPoints,
    /// Both moved, or neither did. The second case is cognitive falling while
    /// branching and nesting both held, where naming either cause would assert
    /// something the numbers do not show.
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
    /// Files that carried units on the base revision only, whether they were
    /// deleted or merely lost every accounted unit. Reported, and excluded from
    /// every headline number: such a file contributes its whole base-side total
    /// as a fall with no head counterpart.
    pub files_only_in_base: u32,
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

/// One file present on both revisions whose branching held while it gained
/// functions and its largest function shrank.
///
/// Local by construction: nothing here depends on any other file, so unrelated
/// work in the changeset cannot make it more or less true. A set-level
/// classifier cannot make this claim, because a changeset contains arbitrary
/// other work and an aggregate cannot attribute.
///
/// It is a description, not an inference. The three conditions are the
/// signature a split leaves, and they are also satisfiable without one: the
/// peak is a file-level maximum (`FileBranching::peak_cyclomatic`), so it can
/// fall because the largest function left the file while arriving helpers
/// happen to carry the branching it took with it. Both numbers are reported so
/// a reader can see that for themselves, and the rendered text states what was
/// measured rather than concluding a refactor happened.
///
/// Files carrying synthetic template units are excluded, because those units
/// are outside every count here, so the numbers would not describe the file a
/// reader opens. Test paths are excluded too: their totals are reported in
/// `BranchingScope` instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SplitInPlace {
    /// Root-relative path.
    pub path: String,
    /// Branch points on the base revision.
    pub branch_points_before: u32,
    /// And on head. Within `tolerance` of `branch_points_before`, which is what
    /// "held" means here. Both are reported because one number alone cannot be
    /// checked.
    pub branch_points_after: u32,
    /// Accounted functions before the split.
    pub functions_before: u32,
    /// Accounted functions after it.
    pub functions_after: u32,
    /// Highest single-function cyclomatic score before.
    pub peak_before: u16,
    /// And after. It falls by construction when a function is split, which is
    /// why it is evidence here and never a metric to celebrate.
    pub peak_after: u16,
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
    /// Files carrying the split signature: branching within `tolerance` of
    /// where it was, more functions, a smaller largest function. Empty when
    /// none do, which is the common case and is not itself a finding. See
    /// `SplitInPlace` for why this describes a shape rather than asserting a
    /// refactor.
    pub split_in_place: Vec<SplitInPlace>,
    /// The band inside which a file's branching counts as held. Published
    /// because a claim against an unpublished threshold is not reproducible by
    /// a consumer.
    pub tolerance: u32,
    /// Size and composition of the compared set.
    pub scope: BranchingScope,
    /// Branch points over the accounting set. The two sides cover different
    /// populations by construction: `current` includes files the changeset
    /// added, `previous` cannot. Files present only on the base revision are in
    /// neither, and are reported in `branch_points_only_in_base`.
    pub branch_points: BranchingMetric,
    /// Functions over the same two populations, with the same asymmetry.
    pub functions: BranchingMetric,
    /// Highest single-function score across the set. Reported as context, never
    /// as evidence on its own: a split lowers it by construction, which is the
    /// whole reason this section exists.
    pub peak_unit_cyclomatic: BranchingMetric,
    /// Base-side branch points of the files that have no head entry.
    pub branch_points_only_in_base: u32,
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
    deleted: Totals,
    scope: BranchingScope,
    deltas: Vec<BranchingFileDelta>,
    split_in_place: Vec<SplitInPlace>,
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
        deleted: Totals::default(),
        scope: BranchingScope {
            files_both: 0,
            files_added: 0,
            files_only_in_base: 0,
            test_branch_points: 0,
            test_functions: 0,
            largest_file_share_of_branch_points: 0.0,
        },
        deltas: Vec::new(),
        split_in_place: Vec::new(),
    };
    let mut test_totals = Totals::default();

    for (path, head_file) in head {
        out.surviving_head.add(*head_file);
        if is_test_path(path) {
            test_totals.add(*head_file);
        }
        let Some(base_file) = base.get(path) else {
            out.scope.files_added += 1;
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
            && !is_test_path(path)
            && !is_excluded_from_the_claim(path)
            && !head_file.has_synthetic_units
            && !base_file.has_synthetic_units
        {
            out.split_in_place.push(SplitInPlace {
                path: path.clone(),
                branch_points_before: base_file.branch_points,
                branch_points_after: head_file.branch_points,
                functions_before: base_file.functions,
                functions_after: head_file.functions,
                peak_before: base_file.peak_cyclomatic,
                peak_after: head_file.peak_cyclomatic,
            });
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
            out.scope.files_only_in_base += 1;
            out.deleted.add(*base_file);
        }
    }

    out.scope.test_branch_points = test_totals.branch_points;
    out.scope.test_functions = test_totals.functions;
    out
}

/// Paths whose numbers describe machinery rather than authored code.
///
/// Deliberately a short, unambiguous list rather than a general heuristic: a
/// wrong exclusion here silently drops a real finding, and the rendered list
/// only has room for two files, so a vendored bundle taking a slot costs a
/// reader the production file they needed.
fn is_excluded_from_the_claim(path: &str) -> bool {
    [
        "/generated/",
        "/vendor/",
        "/dist/",
        "/node_modules/",
        "/__generated__/",
    ]
    .iter()
    .any(|marker| path.contains(marker))
        || path.starts_with("generated/")
        || path.starts_with("vendor/")
        || path.starts_with("dist/")
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
            deleted,
            mut scope,
            mut deltas,
            mut split_in_place,
        } = partition(base, head, tolerance, is_test_path);

        scope.largest_file_share_of_branch_points = if surviving_head.branch_points == 0 {
            0.0
        } else {
            f64::from(scope_largest(head)) / f64::from(surviving_head.branch_points)
        };

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

        split_in_place.sort_by(|a, b| {
            (b.functions_after - b.functions_before)
                .cmp(&(a.functions_after - a.functions_before))
                .then_with(|| a.path.cmp(&b.path))
        });

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
            split_in_place,
            tolerance,
            scope,
            branch_points,
            functions,
            peak_unit_cyclomatic,
            branch_points_only_in_base: deleted.branch_points,
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

    /// Whether the section says anything a reader needs.
    ///
    /// Only a file that demonstrably split in place qualifies. The set totals
    /// alone are context, not news, and the human brief stays silent on them,
    /// matching every sibling section.
    #[must_use]
    pub fn is_reportable(&self) -> bool {
        !self.split_in_place.is_empty()
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
        (true, false) => CognitiveAttribution::FewerBranchPoints,
        (false, true) => CognitiveAttribution::NestingReset,
        // Both moved, or neither did. The second case is cognitive falling with
        // branching and nesting both held, where naming either cause would
        // assert something the numbers do not show.
        (true, true) | (false, false) => CognitiveAttribution::Mixed,
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
            has_synthetic_units: false,
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
    fn a_file_that_splits_in_place_is_named() {
        let base = snapshot(&[("src/a.ts", file(39, 1, 40))]);
        let head = snapshot(&[("src/a.ts", file(39, 8, 6))]);

        let report = compare(&base, &head);

        assert_eq!(report.split_in_place.len(), 1);
        let split = &report.split_in_place[0];
        assert_eq!(split.path, "src/a.ts");
        assert_eq!(
            (split.branch_points_before, split.branch_points_after),
            (39, 39)
        );
        assert_eq!((split.functions_before, split.functions_after), (1, 8));
        assert_eq!((split.peak_before, split.peak_after), (40, 6));
        assert!(report.is_reportable());
    }

    #[test]
    fn the_claim_is_local_so_unrelated_work_cannot_change_it() {
        // The routes that defeated a changeset-level classifier: an unrelated
        // pair of moves that cancel in the sum, a large deleted file, an empty
        // added file, and branching arriving elsewhere. None of them can touch
        // a statement about one file.
        let base = snapshot(&[
            ("src/a.ts", file(39, 1, 40)),
            ("src/b.ts", file(12, 3, 5)),
            ("src/d.ts", file(0, 1, 1)),
            ("src/gone.ts", file(300, 9, 40)),
        ]);
        let head = snapshot(&[
            ("src/a.ts", file(39, 8, 6)),
            ("src/b.ts", file(4, 3, 5)),
            ("src/d.ts", file(208, 1, 90)),
            ("src/added.ts", file(0, 1, 1)),
        ]);

        let report = compare(&base, &head);

        assert_eq!(
            report.split_in_place.len(),
            1,
            "only src/a.ts split; nothing else in the changeset makes that more or less true"
        );
        assert_eq!(report.split_in_place[0].path, "src/a.ts");
    }

    #[test]
    fn a_split_that_lands_in_a_test_file_is_not_an_in_place_split() {
        // The source file did not hold its branching, it lost it.
        let base = snapshot(&[("src/pricing.ts", file(48, 4, 20))]);
        let head = snapshot(&[
            ("src/pricing.ts", file(0, 1, 1)),
            ("src/pricing.test.ts", file(48, 16, 4)),
        ]);

        let report = compare(&base, &head);

        assert!(report.split_in_place.is_empty());
        assert!(!report.is_reportable());
        assert_eq!(
            report.scope.test_branch_points, 48,
            "still reported as scope"
        );
    }

    #[test]
    fn a_file_whose_branching_fell_reports_both_numbers() {
        // Rendering only the base value would read as "2 branch points held"
        // for a file that now has none.
        let base = snapshot(&[("src/a.ts", file(2, 1, 3))]);
        let head = snapshot(&[("src/a.ts", file(0, 2, 1))]);

        let report = compare(&base, &head);

        let split = &report.split_in_place[0];
        assert_eq!(
            (split.branch_points_before, split.branch_points_after),
            (2, 0)
        );
    }

    #[test]
    fn a_test_file_never_carries_the_claim() {
        // Its totals are reported in scope instead, so a test file cannot take
        // a slot from production code in the rendered list.
        let base = snapshot(&[("src/a.test.ts", file(30, 1, 31))]);
        let head = snapshot(&[("src/a.test.ts", file(30, 8, 6))]);

        let report = compare(&base, &head);

        assert!(report.split_in_place.is_empty());
        assert_eq!(report.scope.test_branch_points, 30);
    }

    #[test]
    fn a_file_with_synthetic_template_units_never_carries_the_claim() {
        // Template units sit outside every count here, so the numbers would not
        // describe the file a reader opens.
        let with_template = |branch_points: u32, functions: u32, peak: u16| FileBranching {
            branch_points,
            functions,
            peak_cyclomatic: peak,
            cognitive: branch_points,
            cognitive_nesting_weight: 0,
            has_synthetic_units: true,
        };
        let base = snapshot(&[("src/App.vue", with_template(6, 1, 7))]);
        let head = snapshot(&[("src/App.vue", with_template(6, 4, 3))]);

        let report = compare(&base, &head);

        assert!(report.split_in_place.is_empty());
    }

    #[test]
    fn generated_and_vendored_paths_never_carry_the_claim() {
        // The rendered list holds two files, so machinery taking a slot costs a
        // reader the production file they needed.
        for path in [
            "src/__generated__/schema.ts",
            "vendor/bundle.js",
            "dist/main.js",
            "packages/app/generated/api.ts",
        ] {
            let base = snapshot(&[(path, file(30, 1, 31))]);
            let head = snapshot(&[(path, file(30, 8, 6))]);
            assert!(
                compare(&base, &head).split_in_place.is_empty(),
                "{path} should not carry the claim"
            );
        }
    }

    #[test]
    fn a_split_with_a_little_glue_branching_still_counts() {
        let base = snapshot(&[("src/a.ts", file(30, 1, 31))]);
        let head = snapshot(&[("src/a.ts", file(32, 6, 8))]);

        let report = compare(&base, &head);

        assert_eq!(report.split_in_place.len(), 1);
    }

    #[test]
    fn branching_arriving_in_a_file_is_not_a_split() {
        let base = snapshot(&[("src/a.ts", file(10, 1, 11))]);
        let head = snapshot(&[("src/a.ts", file(40, 5, 12))]);

        let report = compare(&base, &head);

        assert!(report.split_in_place.is_empty());
    }

    #[test]
    fn a_file_whose_peak_held_is_not_a_split() {
        let base = snapshot(&[("src/a.ts", file(30, 2, 20))]);
        let head = snapshot(&[("src/a.ts", file(30, 6, 20))]);

        let report = compare(&base, &head);

        assert!(
            report.split_in_place.is_empty(),
            "functions rose and branching held, but the worst function is untouched"
        );
    }

    #[test]
    fn splits_are_ordered_by_how_far_the_file_was_partitioned() {
        let base = snapshot(&[("src/a.ts", file(9, 1, 10)), ("src/b.ts", file(20, 1, 21))]);
        let head = snapshot(&[("src/a.ts", file(9, 3, 4)), ("src/b.ts", file(20, 9, 5))]);

        let report = compare(&base, &head);

        assert_eq!(
            report
                .split_in_place
                .iter()
                .map(|s| s.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/b.ts", "src/a.ts"]
        );
    }

    #[test]
    fn the_set_totals_still_describe_the_changeset() {
        let base = snapshot(&[
            ("src/a.ts", file(30, 5, 8)),
            ("src/gone.ts", file(300, 9, 40)),
        ]);
        let head = snapshot(&[("src/a.ts", file(30, 5, 8)), ("src/new.ts", file(9, 6, 4))]);

        let report = compare(&base, &head);

        assert_eq!(report.branch_points.previous, 30);
        assert_eq!(report.branch_points.current, 39);
        assert_eq!(report.functions.delta, 6);
        assert_eq!(report.scope.files_added, 1);
        assert_eq!(report.scope.files_only_in_base, 1);
        assert_eq!(report.branch_points_only_in_base, 300);
        assert!(
            !report.is_reportable(),
            "totals alone are context, not news"
        );
    }

    #[test]
    fn an_empty_accounting_set_reports_nothing() {
        let report = compare(&snapshot(&[]), &snapshot(&[]));

        assert!(report.split_in_place.is_empty());
        assert_eq!(report.branch_points.current, 0);
        assert!(!report.is_reportable());
    }

    #[test]
    fn a_cognitive_win_with_branching_held_is_a_nesting_reset() {
        let base = snapshot(&[(
            "src/a.ts",
            FileBranching {
                branch_points: 10,
                functions: 1,
                peak_cyclomatic: 11,
                cognitive: 40,
                cognitive_nesting_weight: 30,
                has_synthetic_units: false,
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
                has_synthetic_units: false,
            },
        )]);

        let report = compare(&base, &head);

        assert_eq!(report.cognitive.delta, -28);
        assert_eq!(
            report.cognitive.attributed_to,
            Some(CognitiveAttribution::NestingReset)
        );
    }

    #[test]
    fn a_cognitive_rise_is_attributed_to_nothing() {
        let base = snapshot(&[("src/a.ts", file(7, 1, 8))]);
        let head = snapshot(&[("src/a.ts", file(11, 5, 5))]);

        let report = compare(&base, &head);

        assert!(report.cognitive.delta > 0);
        assert_eq!(report.cognitive.attributed_to, None);
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
            "{}",
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
}
