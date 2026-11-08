//! Redundancy signals of `--kill-matrix-for` target tests.
//!
//! The target tests of a mutant run without bail, so each target killer is
//! known. The other covering tests run with bail, so the evidence holds at
//! most one other killer: enough to show that a kill is not unique, but not
//! enough for subsumers.

use std::collections::{BTreeMap, BTreeSet};

use fallow_output::{
    MutantResult, MutantStatus, MutationBaselineFailure, MutationCompletion,
    MutationCompletionStatus, MutationKillMatrix, MutationKillMatrixStatus,
    MutationKillMatrixTarget, MutationKillMatrixTargetStatus, MutationTargetOnlyKill,
    MutationTestKillRow,
};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{ResolvedTarget, baseline_excluded};

/// The note of each target row: bail hides the other killers.
const SUBSUMERS_NOT_COMPUTED: &str = "subsumers-not-computed";

const INCOMPLETE_MUTANT_RESULTS: &str = "incomplete-mutant-results";
const UNMATCHED_TARGET: &str = "unmatched-target";

/// The targets of a run and the dry-run tests that they select.
pub struct Selection {
    targets: Vec<ResolvedTarget>,
}

impl Selection {
    /// The resolved targets. Without a dry run, `resolved` is empty and each
    /// requested target selects no test.
    pub fn new(requested: &[String], resolved: &[ResolvedTarget]) -> Self {
        let targets = if resolved.is_empty() {
            requested
                .iter()
                .map(|target| ResolvedTarget {
                    target: target.clone(),
                    test_ids: Vec::new(),
                })
                .collect()
        } else {
            resolved.to_vec()
        };
        Self { targets }
    }

    fn test_ids(&self) -> impl Iterator<Item = &String> {
        self.targets.iter().flat_map(|target| &target.test_ids)
    }
}

/// The observed kills of the target tests.
#[derive(Default)]
struct Evidence {
    /// The kills of each target test with coverage.
    kills: BTreeMap<String, BTreeSet<String>>,
    /// The target killers of each mutant with no other killer.
    target_only: BTreeMap<String, BTreeSet<String>>,
    incomplete: bool,
    /// The selected tests that cover a mutant with incomplete evidence.
    incomplete_tests: BTreeSet<String>,
    /// An incomplete mutant names no selected test, so no target is safe.
    incomplete_everywhere: bool,
    missing_test_files: bool,
}

pub fn build(
    selection: &Selection,
    results: &[MutantResult],
    baseline: &[MutationBaselineFailure],
    completion: &MutationCompletion,
    test_files: &FxHashMap<String, String>,
) -> MutationKillMatrix {
    let all_targets: FxHashSet<&str> = selection.test_ids().map(String::as_str).collect();
    let mut evidence = Evidence::default();
    let mut eligible: FxHashSet<&str> = FxHashSet::default();
    for test in selection.test_ids() {
        match test_files.get(test).filter(|file| !file.is_empty()) {
            Some(file) if !baseline_excluded(baseline, test, file) => {
                eligible.insert(test.as_str());
            }
            Some(_) => {}
            None => evidence.missing_test_files = true,
        }
    }
    for result in results {
        observe(result, &all_targets, &eligible, &mut evidence);
    }
    let mut reasons = BTreeSet::new();
    if completion.status != MutationCompletionStatus::Complete {
        reasons.insert("partial-mutation-run");
    }
    if !baseline.is_empty() {
        reasons.insert("baseline-failure");
    }
    if evidence.missing_test_files {
        reasons.insert("missing-test-file-metadata");
    }
    let run_wide: Vec<&str> = reasons.iter().copied().collect();
    let targets: Vec<MutationKillMatrixTarget> = selection
        .targets
        .iter()
        .map(|target| target_entry(target, &run_wide, &evidence))
        .collect();
    if evidence.incomplete {
        reasons.insert(INCOMPLETE_MUTANT_RESULTS);
    }
    if targets.iter().any(|target| target.test_ids.is_empty()) {
        reasons.insert(UNMATCHED_TARGET);
    }
    let complete = reasons.is_empty();
    // A test has complete evidence when one complete target selects it.
    let complete_tests: FxHashSet<&str> = targets
        .iter()
        .filter(|target| target.status == MutationKillMatrixTargetStatus::Complete)
        .flat_map(|target| target.test_ids.iter().map(String::as_str))
        .collect();
    let tests: Vec<MutationTestKillRow> = evidence
        .kills
        .iter()
        .map(|(id, kills)| {
            let row_complete = complete_tests.contains(id.as_str());
            row(id, kills, &evidence, test_files, row_complete)
        })
        .collect();
    let candidates = tests
        .iter()
        .filter(|row| {
            complete_tests.contains(row.test_id.as_str())
                && !row.kills.is_empty()
                && row.unique_kills.is_empty()
        })
        .map(|row| row.test_id.clone())
        .collect();
    let coverage_without_kills = tests
        .iter()
        .filter(|row| row.kills.is_empty())
        .map(|row| row.test_id.clone())
        .collect();
    MutationKillMatrix {
        status: if complete {
            MutationKillMatrixStatus::Complete
        } else {
            MutationKillMatrixStatus::Incomplete
        },
        reasons: reasons.into_iter().map(str::to_owned).collect(),
        tests,
        candidates,
        coverage_without_kills,
        targets: Some(targets),
    }
}

/// The output entry of one target with the status of its own evidence.
fn target_entry(
    target: &ResolvedTarget,
    run_wide: &[&str],
    evidence: &Evidence,
) -> MutationKillMatrixTarget {
    let mut reasons: BTreeSet<&str> = run_wide.iter().copied().collect();
    if evidence.incomplete_everywhere
        || target
            .test_ids
            .iter()
            .any(|test| evidence.incomplete_tests.contains(test))
    {
        reasons.insert(INCOMPLETE_MUTANT_RESULTS);
    }
    if target.test_ids.is_empty() {
        reasons.insert(UNMATCHED_TARGET);
    }
    MutationKillMatrixTarget {
        target: target.target.clone(),
        test_ids: target.test_ids.clone(),
        status: if reasons.is_empty() {
            MutationKillMatrixTargetStatus::Complete
        } else {
            MutationKillMatrixTargetStatus::Incomplete
        },
        reasons: reasons.into_iter().map(str::to_owned).collect(),
    }
}

/// Adds the evidence of one mutant. `all_targets` holds each selected test,
/// and `eligible` the selected tests that passed with no active mutant.
fn observe(
    result: &MutantResult,
    all_targets: &FxHashSet<&str>,
    eligible: &FxHashSet<&str>,
    evidence: &mut Evidence,
) {
    if matches!(
        result.status,
        MutantStatus::Pruned | MutantStatus::Equivalent | MutantStatus::NoCoverage
    ) {
        return;
    }
    // A timeout of the other tests after a target survivor keeps complete
    // target evidence. Each status without a final result does not.
    let incomplete = !result.kill_matrix_complete
        || matches!(
            result.status,
            MutantStatus::Pending | MutantStatus::Flaky | MutantStatus::RuntimeError
        );
    if incomplete {
        evidence.incomplete = true;
        let touched: Vec<&String> = result
            .covering_tests
            .iter()
            .chain(&result.killing_tests)
            .filter(|test| all_targets.contains(test.as_str()))
            .collect();
        // Without a known target test, the mutant can touch each target.
        if touched.is_empty() {
            evidence.incomplete_everywhere = true;
        }
        evidence
            .incomplete_tests
            .extend(touched.into_iter().cloned());
    }
    for test in &result.covering_tests {
        if eligible.contains(test.as_str()) {
            evidence.kills.entry(test.clone()).or_default();
        }
    }
    if result.status != MutantStatus::Killed {
        return;
    }
    let killers: BTreeSet<&String> = result
        .killing_tests
        .iter()
        .filter(|test| eligible.contains(test.as_str()))
        .collect();
    let other_killer = result
        .killing_tests
        .iter()
        .any(|test| !all_targets.contains(test.as_str()));
    let mutant = &result.location.mutant_id;
    for killer in &killers {
        evidence
            .kills
            .entry((*killer).clone())
            .or_default()
            .insert(mutant.clone());
    }
    if !other_killer && !killers.is_empty() {
        evidence
            .target_only
            .insert(mutant.clone(), killers.into_iter().cloned().collect());
    }
}

fn row(
    id: &str,
    kills: &BTreeSet<String>,
    evidence: &Evidence,
    test_files: &FxHashMap<String, String>,
    complete: bool,
) -> MutationTestKillRow {
    let mut unique_kills = Vec::new();
    let mut target_only_kills = Vec::new();
    for mutant in kills {
        let Some(killers) = evidence.target_only.get(mutant) else {
            continue;
        };
        let partners: Vec<String> = killers
            .iter()
            .filter(|killer| killer.as_str() != id)
            .cloned()
            .collect();
        if partners.is_empty() {
            unique_kills.push(mutant.clone());
        } else {
            target_only_kills.push(MutationTargetOnlyKill {
                mutant_id: mutant.clone(),
                targets: partners,
            });
        }
    }
    let mut notes = vec![SUBSUMERS_NOT_COMPUTED.to_owned()];
    if !complete {
        notes.insert(0, "incomplete-observed-evidence".to_owned());
    }
    MutationTestKillRow {
        test_id: id.to_owned(),
        file: test_files.get(id).cloned().unwrap_or_default(),
        kills: kills.iter().cloned().collect(),
        unique_kills,
        subsuming_tests: Vec::new(),
        notes,
        target_only_kills: Some(target_only_kills),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{MutantLocation, MutationIncompleteReason};

    const A: &str = "test/a.ts::a";
    const B: &str = "test/a.ts::b";
    const C: &str = "test/c.ts::c";
    const OTHER: &str = "test/other.ts::other";

    fn files() -> FxHashMap<String, String> {
        FxHashMap::from_iter([
            (A.to_owned(), "test/a.ts".to_owned()),
            (B.to_owned(), "test/a.ts".to_owned()),
            (C.to_owned(), "test/c.ts".to_owned()),
            (OTHER.to_owned(), "test/other.ts".to_owned()),
        ])
    }

    fn result(id: &str, status: MutantStatus, killers: &[&str]) -> MutantResult {
        MutantResult {
            location: MutantLocation {
                mutant_id: id.to_owned(),
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
            killed_by: killers.first().map(|s| (*s).to_owned()),
            killing_tests: killers.iter().map(|s| (*s).to_owned()).collect(),
            kill_matrix_complete: true,
            covering_tests: vec![A.to_owned(), B.to_owned(), OTHER.to_owned()],
            static_mutant: false,
            duration_ms: None,
            test_reach: None,
            retried: false,
            attempts: Vec::new(),
            cached: false,
            resumed: false,
            type_error_codes: Vec::new(),
            equivalent_reason: None,
        }
    }

    fn matrix(results: &[MutantResult]) -> MutationKillMatrix {
        let resolved = [ResolvedTarget {
            target: "test/a.ts".to_owned(),
            test_ids: vec![A.to_owned(), B.to_owned()],
        }];
        build(
            &Selection::new(&["test/a.ts".to_owned()], &resolved),
            results,
            &[],
            &MutationCompletion::from_reasons(Vec::new()),
            &files(),
        )
    }

    #[test]
    fn unique_shared_and_other_kills_are_kept_apart() {
        let matrix = matrix(&[
            result("m1", MutantStatus::Killed, &[A]),
            result("m2", MutantStatus::Killed, &[A, B]),
            result("m3", MutantStatus::Killed, &[B, OTHER]),
            result("m4", MutantStatus::Survived, &[]),
        ]);
        assert_eq!(matrix.status, MutationKillMatrixStatus::Complete);
        assert_eq!(
            matrix
                .tests
                .iter()
                .map(|row| row.test_id.as_str())
                .collect::<Vec<_>>(),
            [A, B],
            "only target tests get a row"
        );
        let (a, b) = (&matrix.tests[0], &matrix.tests[1]);
        assert_eq!(a.kills, ["m1", "m2"]);
        assert_eq!(a.unique_kills, ["m1"]);
        assert_eq!(
            a.target_only_kills,
            Some(vec![MutationTargetOnlyKill {
                mutant_id: "m2".to_owned(),
                targets: vec![B.to_owned()],
            }])
        );
        assert_eq!(b.kills, ["m2", "m3"]);
        assert!(b.unique_kills.is_empty(), "another test also kills m3");
        assert_eq!(
            b.target_only_kills
                .as_ref()
                .map(|kills| kills[0].targets.clone()),
            Some(vec![A.to_owned()])
        );
        assert!(a.subsuming_tests.is_empty());
        assert_eq!(a.notes, [SUBSUMERS_NOT_COMPUTED]);
        assert_eq!(matrix.candidates, [B]);
        assert!(matrix.coverage_without_kills.is_empty());
        assert_eq!(matrix.targets.as_ref().map(Vec::len), Some(1));
    }

    #[test]
    fn a_target_without_kills_is_coverage_without_kills_and_no_candidate() {
        let matrix = matrix(&[result("m1", MutantStatus::Killed, &[OTHER])]);
        assert_eq!(matrix.coverage_without_kills, [A, B]);
        assert!(matrix.candidates.is_empty());
    }

    #[test]
    fn incomplete_or_unmatched_evidence_gives_no_candidates() {
        let mut timed_out = result("m1", MutantStatus::Killed, &[A, B]);
        timed_out.kill_matrix_complete = false;
        let incomplete = matrix(&[timed_out]);
        assert_eq!(incomplete.status, MutationKillMatrixStatus::Incomplete);
        assert_eq!(incomplete.reasons, ["incomplete-mutant-results"]);
        assert!(incomplete.candidates.is_empty());
        let target = &incomplete.targets.as_ref().expect("target mode")[0];
        assert_eq!(target.status, MutationKillMatrixTargetStatus::Incomplete);
        assert_eq!(target.reasons, ["incomplete-mutant-results"]);
        assert_eq!(
            incomplete.tests[0].notes,
            ["incomplete-observed-evidence", SUBSUMERS_NOT_COMPUTED]
        );

        // A timeout of the other tests after a target survivor keeps the
        // target evidence complete.
        let other_timeout = matrix(&[
            result("m1", MutantStatus::Killed, &[A, B]),
            result("m2", MutantStatus::Timeout, &[]),
        ]);
        assert_eq!(other_timeout.status, MutationKillMatrixStatus::Complete);

        let unmatched = build(
            &Selection::new(&["test/none.ts".to_owned()], &[]),
            &[result("m1", MutantStatus::Killed, &[A])],
            &[],
            &MutationCompletion::from_reasons(Vec::new()),
            &files(),
        );
        assert_eq!(unmatched.reasons, ["unmatched-target"]);
        assert!(unmatched.tests.is_empty());
        assert_eq!(
            unmatched.targets.expect("target mode")[0].reasons,
            ["unmatched-target"]
        );
    }

    #[test]
    fn an_incomplete_mutant_outside_a_target_keeps_its_candidates() {
        let mut other_target = result("m2", MutantStatus::Killed, &[C]);
        other_target.kill_matrix_complete = false;
        other_target.covering_tests = vec![C.to_owned()];
        let resolved = [
            ResolvedTarget {
                target: "test/a.ts".to_owned(),
                test_ids: vec![A.to_owned(), B.to_owned()],
            },
            ResolvedTarget {
                target: "test/c.ts".to_owned(),
                test_ids: vec![C.to_owned()],
            },
        ];
        let matrix = build(
            &Selection::new(&["test/a.ts".to_owned(), "test/c.ts".to_owned()], &resolved),
            &[result("m1", MutantStatus::Killed, &[A, B]), other_target],
            &[],
            &MutationCompletion::from_reasons(Vec::new()),
            &files(),
        );
        assert_eq!(matrix.status, MutationKillMatrixStatus::Incomplete);
        assert_eq!(matrix.reasons, ["incomplete-mutant-results"]);
        assert_eq!(matrix.candidates, [A, B], "m2 does not touch test/a.ts");
        let targets = matrix.targets.expect("target mode");
        assert_eq!(targets[0].status, MutationKillMatrixTargetStatus::Complete);
        assert!(targets[0].reasons.is_empty());
        assert_eq!(
            targets[1].status,
            MutationKillMatrixTargetStatus::Incomplete
        );
        assert_eq!(targets[1].reasons, ["incomplete-mutant-results"]);
        assert_eq!(matrix.tests[0].notes, [SUBSUMERS_NOT_COMPUTED]);
        assert_eq!(
            matrix.tests[2].notes,
            ["incomplete-observed-evidence", SUBSUMERS_NOT_COMPUTED]
        );
    }

    #[test]
    fn an_incomplete_mutant_without_a_known_target_test_makes_each_target_incomplete() {
        let mut unknown = result("m2", MutantStatus::RuntimeError, &[]);
        unknown.covering_tests.clear();
        let matrix = matrix(&[result("m1", MutantStatus::Killed, &[A, B]), unknown]);
        assert_eq!(matrix.reasons, ["incomplete-mutant-results"]);
        assert!(matrix.candidates.is_empty());
        let targets = matrix.targets.expect("target mode");
        assert_eq!(
            targets[0].status,
            MutationKillMatrixTargetStatus::Incomplete
        );
        assert_eq!(targets[0].reasons, ["incomplete-mutant-results"]);
    }

    #[test]
    fn a_run_wide_reason_makes_each_target_incomplete() {
        let resolved = [ResolvedTarget {
            target: "test/a.ts".to_owned(),
            test_ids: vec![A.to_owned(), B.to_owned()],
        }];
        let matrix = build(
            &Selection::new(&["test/a.ts".to_owned()], &resolved),
            &[result("m1", MutantStatus::Killed, &[A, B])],
            &[],
            &MutationCompletion::from_reasons(vec![MutationIncompleteReason::TimeBudget]),
            &files(),
        );
        assert_eq!(matrix.reasons, ["partial-mutation-run"]);
        assert!(matrix.candidates.is_empty());
        let targets = matrix.targets.expect("target mode");
        assert_eq!(
            targets[0].status,
            MutationKillMatrixTargetStatus::Incomplete
        );
        assert_eq!(targets[0].reasons, ["partial-mutation-run"]);
    }

    #[test]
    fn a_baseline_failing_target_gets_no_row_and_its_kill_is_not_shared() {
        let resolved = [ResolvedTarget {
            target: "test/a.ts".to_owned(),
            test_ids: vec![A.to_owned(), B.to_owned()],
        }];
        let failure = MutationBaselineFailure {
            file: Some("test/a.ts".to_owned()),
            test_id: Some(B.to_owned()),
            message: None,
            mutant_ids: Vec::new(),
        };
        let matrix = build(
            &Selection::new(&["test/a.ts".to_owned()], &resolved),
            &[result("m1", MutantStatus::Killed, &[A, B])],
            &[failure],
            &MutationCompletion::from_reasons(Vec::new()),
            &files(),
        );
        assert_eq!(matrix.tests.len(), 1);
        assert_eq!(matrix.tests[0].unique_kills, ["m1"]);
        assert_eq!(matrix.status, MutationKillMatrixStatus::Incomplete);
        assert!(matrix.candidates.is_empty());
    }
}
