//! Redundancy signals of `--kill-matrix-for` target tests.
//!
//! The target tests of a mutant run without bail, so each target killer is
//! known. The other covering tests run with bail, so the evidence holds at
//! most one other killer: enough to show that a kill is not unique, but not
//! enough for subsumers.

use std::collections::{BTreeMap, BTreeSet};

use fallow_output::{
    MutantResult, MutantStatus, MutationBaselineFailure, MutationCompletion,
    MutationCompletionStatus, MutationIncompleteReason, MutationKillMatrix,
    MutationKillMatrixStatus, MutationKillMatrixTarget, MutationKillMatrixTargetStatus,
    MutationTargetOnlyKill, MutationTestKillRow,
};
use rustc_hash::{FxHashMap, FxHashSet};

use super::baseline_scope::Scope;
use super::{ResolvedTarget, baseline_excluded};

/// The note of each target row: bail hides the other killers.
const SUBSUMERS_NOT_COMPUTED: &str = "subsumers-not-computed";

const INCOMPLETE_MUTANT_RESULTS: &str = "incomplete-mutant-results";
const UNMATCHED_TARGET: &str = "unmatched-target";
const BASELINE_FAILURE: &str = "baseline-failure";
const PARTIAL_MUTATION_RUN: &str = "partial-mutation-run";

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
                    unmatched: None,
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
    let trusted = trusted_killers(baseline, test_files);
    let co_killer = |test: &str| !all_targets.contains(test) && trusted(test);
    for result in results {
        observe(result, &all_targets, &eligible, &co_killer, &mut evidence);
    }
    let mut run_wide = BTreeSet::new();
    // A partial run from baseline failures alone is partial only for the
    // targets that the failures affect.
    let baseline_only = completion.reasons == [MutationIncompleteReason::BaselineFailure];
    if completion.status != MutationCompletionStatus::Complete && !baseline_only {
        run_wide.insert(PARTIAL_MUTATION_RUN);
    }
    if evidence.missing_test_files {
        run_wide.insert("missing-test-file-metadata");
    }
    let scope = Scope::new(baseline, results, test_files);
    let targets: Vec<MutationKillMatrixTarget> = selection
        .targets
        .iter()
        .map(|target| {
            let failures = scope.failures(&target.test_ids);
            let mut reasons = run_wide.clone();
            if failures.is_some() {
                reasons.insert(BASELINE_FAILURE);
                if baseline_only {
                    reasons.insert(PARTIAL_MUTATION_RUN);
                }
            }
            target_entry(target, reasons, failures.unwrap_or_default(), &evidence)
        })
        .collect();
    let mut reasons = run_wide;
    if completion.status != MutationCompletionStatus::Complete {
        reasons.insert(PARTIAL_MUTATION_RUN);
    }
    if !baseline.is_empty() {
        reasons.insert(BASELINE_FAILURE);
    }
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
            row(id, kills, &evidence, test_files, row_complete, &trusted)
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

/// True for a test whose kill shows that a kill of a target test is shared.
/// A test that failed with no active mutant gives no kill evidence. A test in
/// the file of a failure can share state with it. Neither test is trusted.
fn trusted_killers<'a>(
    baseline: &'a [MutationBaselineFailure],
    test_files: &'a FxHashMap<String, String>,
) -> impl Fn(&str) -> bool + 'a {
    let failure_files: FxHashSet<&str> = baseline
        .iter()
        .filter_map(|failure| failure.file.as_deref())
        .collect();
    move |test| {
        let file = test_files.get(test).map_or("", String::as_str);
        !failure_files.contains(file) && !baseline_excluded(baseline, test, file)
    }
}

/// The output entry of one target with the status of its own evidence.
fn target_entry(
    target: &ResolvedTarget,
    mut reasons: BTreeSet<&'static str>,
    baseline_failures: Vec<String>,
    evidence: &Evidence,
) -> MutationKillMatrixTarget {
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
        reasons.extend(target.unmatched.and_then(|unmatched| unmatched.reason()));
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
        baseline_failures,
    }
}

/// Adds the evidence of one mutant. `all_targets` holds each selected test,
/// `eligible` the selected tests that passed with no active mutant, and
/// `co_killer` the other tests whose kill makes a target kill shared.
fn observe(
    result: &MutantResult,
    all_targets: &FxHashSet<&str>,
    eligible: &FxHashSet<&str>,
    co_killer: &impl Fn(&str) -> bool,
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
    let other_killer = result.killing_tests.iter().any(|test| co_killer(test));
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
    trusted: &impl Fn(&str) -> bool,
) -> MutationTestKillRow {
    let mut unique_kills = Vec::new();
    let mut target_only_kills = Vec::new();
    for mutant in kills {
        let Some(killers) = evidence.target_only.get(mutant) else {
            continue;
        };
        let partners: Vec<String> = killers
            .iter()
            .filter(|killer| killer.as_str() != id && trusted(killer))
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
            pending_reason: None,
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
            equivalence_hint: None,
        }
    }

    fn matrix(results: &[MutantResult]) -> MutationKillMatrix {
        let resolved = [ResolvedTarget {
            target: "test/a.ts".to_owned(),
            test_ids: vec![A.to_owned(), B.to_owned()],
            unmatched: None,
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
                unmatched: None,
            },
            ResolvedTarget {
                target: "test/c.ts".to_owned(),
                test_ids: vec![C.to_owned()],
                unmatched: None,
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
    fn an_unmatched_target_names_its_reason_and_keeps_the_candidates_of_the_others() {
        use crate::mutation::scheduler::targets::Unmatched;
        let resolved = [
            ResolvedTarget {
                target: "test/a.ts".to_owned(),
                test_ids: vec![A.to_owned(), B.to_owned()],
                unmatched: None,
            },
            ResolvedTarget {
                target: "test/cli.ts".to_owned(),
                test_ids: Vec::new(),
                unmatched: Some(Unmatched::NoPlannedCode),
            },
            ResolvedTarget {
                target: "src/a.ts".to_owned(),
                test_ids: Vec::new(),
                unmatched: Some(Unmatched::NotATestFile),
            },
            ResolvedTarget {
                target: "test/a.ts::gone".to_owned(),
                test_ids: Vec::new(),
                unmatched: Some(Unmatched::NoTest),
            },
        ];
        let requested: Vec<String> = resolved.iter().map(|t| t.target.clone()).collect();
        let matrix = build(
            &Selection::new(&requested, &resolved),
            &[result("m1", MutantStatus::Killed, &[A, B])],
            &[],
            &MutationCompletion::from_reasons(Vec::new()),
            &files(),
        );
        assert_eq!(matrix.reasons, ["unmatched-target"]);
        assert_eq!(matrix.candidates, [A, B]);
        let targets = matrix.targets.expect("target mode");
        assert_eq!(targets[0].status, MutationKillMatrixTargetStatus::Complete);
        let reasons: Vec<&[String]> = targets[1..].iter().map(|t| t.reasons.as_slice()).collect();
        assert_eq!(
            reasons,
            [
                &["no-planned-code".to_owned(), "unmatched-target".to_owned()][..],
                &["not-a-test-file".to_owned(), "unmatched-target".to_owned()][..],
                &["unmatched-target".to_owned()][..],
            ]
        );
        assert!(
            targets[1..]
                .iter()
                .all(|t| t.status == MutationKillMatrixTargetStatus::Incomplete)
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
            unmatched: None,
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
            unmatched: None,
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
        let target = &matrix.targets.expect("target mode")[0];
        assert_eq!(target.status, MutationKillMatrixTargetStatus::Incomplete);
        assert_eq!(target.reasons, ["baseline-failure"]);
        assert_eq!(target.baseline_failures, [B]);
    }

    const BROKEN: &str = "test/c.ts::broken";

    fn two_targets() -> [ResolvedTarget; 2] {
        [
            ResolvedTarget {
                target: "test/a.ts".to_owned(),
                test_ids: vec![A.to_owned(), B.to_owned()],
                unmatched: None,
            },
            ResolvedTarget {
                target: "test/c.ts".to_owned(),
                test_ids: vec![C.to_owned()],
                unmatched: None,
            },
        ]
    }

    fn covered(id: &str, killers: &[&str], covering: &[&str]) -> MutantResult {
        let mut result = result(id, MutantStatus::Killed, killers);
        result.covering_tests = covering.iter().map(|s| (*s).to_owned()).collect();
        result
    }

    fn failure(file: &str, test_id: Option<&str>, mutant_ids: &[&str]) -> MutationBaselineFailure {
        MutationBaselineFailure {
            file: Some(file.to_owned()),
            test_id: test_id.map(str::to_owned),
            message: None,
            mutant_ids: mutant_ids.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn two_target_matrix(
        results: &[MutantResult],
        baseline: &[MutationBaselineFailure],
        completion: Vec<MutationIncompleteReason>,
    ) -> MutationKillMatrix {
        let resolved = two_targets();
        let mut files = files();
        files.insert(BROKEN.to_owned(), "test/c.ts".to_owned());
        build(
            &Selection::new(&["test/a.ts".to_owned(), "test/c.ts".to_owned()], &resolved),
            results,
            baseline,
            &MutationCompletion::from_reasons(completion),
            &files,
        )
    }

    fn statuses(matrix: &MutationKillMatrix) -> Vec<(MutationKillMatrixTargetStatus, Vec<String>)> {
        matrix
            .targets
            .iter()
            .flatten()
            .map(|target| (target.status, target.reasons.clone()))
            .collect()
    }

    fn scoped() -> Vec<String> {
        vec![
            "baseline-failure".to_owned(),
            "partial-mutation-run".to_owned(),
        ]
    }

    #[test]
    fn a_baseline_failure_in_another_test_file_keeps_the_other_target_complete() {
        for (broken, name) in [
            (failure("test/c.ts", Some(BROKEN), &[]), BROKEN),
            (failure("test/c.ts", None, &[]), "test/c.ts"),
        ] {
            let matrix = two_target_matrix(
                &[covered("m1", &[A, B], &[A, B]), covered("m2", &[C], &[C])],
                &[broken],
                vec![MutationIncompleteReason::BaselineFailure],
            );
            assert_eq!(matrix.status, MutationKillMatrixStatus::Incomplete);
            assert_eq!(matrix.reasons, scoped(), "the top-level reasons stay");
            assert_eq!(
                statuses(&matrix),
                [
                    (MutationKillMatrixTargetStatus::Complete, Vec::new()),
                    (MutationKillMatrixTargetStatus::Incomplete, scoped()),
                ]
            );
            assert_eq!(matrix.candidates, [A, B]);
            let targets = matrix.targets.expect("target mode");
            assert!(targets[0].baseline_failures.is_empty());
            assert_eq!(targets[1].baseline_failures, [name]);
        }
    }

    #[test]
    fn a_control_run_failure_that_kills_with_a_target_test_gives_no_shared_kill() {
        // OTHER passed the dry run and failed a later control run.
        let matrix = two_target_matrix(
            &[
                covered("m1", &[A, OTHER], &[A, OTHER]),
                covered("m2", &[A, B], &[A, B]),
                covered("m3", &[C], &[C]),
            ],
            &[failure("test/other.ts", Some(OTHER), &["m9"])],
            vec![MutationIncompleteReason::BaselineFailure],
        );
        let a = &matrix.tests[0];
        assert_eq!(a.test_id, A);
        assert_eq!(a.unique_kills, ["m1"], "a failing test is no co-killer");
        assert_eq!(
            statuses(&matrix),
            [
                (MutationKillMatrixTargetStatus::Incomplete, scoped()),
                (MutationKillMatrixTargetStatus::Complete, Vec::new()),
            ]
        );
        assert!(matrix.candidates.is_empty());
        assert_eq!(
            matrix.targets.expect("target mode")[0].baseline_failures,
            [OTHER]
        );
    }

    #[test]
    fn a_control_run_failure_on_a_covered_mutant_makes_only_that_target_incomplete() {
        let covering = [
            covered("m1", &[A, B], &[A, B]),
            covered("m2", &[C], &[C, OTHER]),
        ];
        let listed = [covered("m1", &[A, B], &[A, B]), covered("m2", &[C], &[C])];
        for (results, failure) in [
            (&covering, failure("test/other.ts", Some(OTHER), &[])),
            (&listed, failure("test/other.ts", Some(OTHER), &["m2"])),
        ] {
            let matrix = two_target_matrix(
                results,
                &[failure],
                vec![MutationIncompleteReason::BaselineFailure],
            );
            assert_eq!(
                statuses(&matrix),
                [
                    (MutationKillMatrixTargetStatus::Complete, Vec::new()),
                    (MutationKillMatrixTargetStatus::Incomplete, scoped()),
                ]
            );
            assert_eq!(matrix.candidates, [A, B]);
        }
    }

    #[test]
    fn a_target_kill_from_a_test_file_with_a_baseline_failure_is_not_shared() {
        // BROKEN shares test/c.ts with C, so the kill of C is suspect.
        let matrix = two_target_matrix(
            &[covered("m1", &[A, C], &[A, C]), covered("m2", &[B], &[B])],
            &[failure("test/c.ts", Some(BROKEN), &[])],
            vec![MutationIncompleteReason::BaselineFailure],
        );
        assert_eq!(
            statuses(&matrix),
            [
                (MutationKillMatrixTargetStatus::Complete, Vec::new()),
                (MutationKillMatrixTargetStatus::Incomplete, scoped()),
            ]
        );
        let a = &matrix.tests[0];
        assert_eq!(a.test_id, A);
        assert_eq!(a.unique_kills, ["m1"]);
        assert_eq!(a.target_only_kills, Some(Vec::new()));
        let c = &matrix.tests[2];
        assert_eq!(c.test_id, C);
        assert_eq!(
            c.target_only_kills,
            Some(vec![MutationTargetOnlyKill {
                mutant_id: "m1".to_owned(),
                targets: vec![A.to_owned()],
            }])
        );
        assert!(matrix.candidates.is_empty());
    }

    #[test]
    fn a_passing_test_in_a_file_with_a_baseline_failure_is_no_co_killer() {
        let matrix = two_target_matrix(
            &[
                covered("m1", &[A, OTHER], &[A, OTHER]),
                covered("m2", &[B, OTHER], &[B, OTHER]),
                covered("m3", &[C], &[C]),
            ],
            &[failure("test/other.ts", Some("test/other.ts::broken"), &[])],
            vec![MutationIncompleteReason::BaselineFailure],
        );
        assert_eq!(matrix.tests[0].unique_kills, ["m1"]);
        assert_eq!(matrix.tests[1].unique_kills, ["m2"]);
        assert!(matrix.candidates.is_empty());
    }

    #[test]
    fn another_completion_reason_keeps_partial_mutation_run_on_each_target() {
        let matrix = two_target_matrix(
            &[covered("m1", &[A, B], &[A, B]), covered("m2", &[C], &[C])],
            &[failure("test/c.ts", Some(BROKEN), &[])],
            vec![
                MutationIncompleteReason::BaselineFailure,
                MutationIncompleteReason::TimeBudget,
            ],
        );
        assert_eq!(matrix.reasons, scoped());
        assert_eq!(
            statuses(&matrix),
            [
                (
                    MutationKillMatrixTargetStatus::Incomplete,
                    vec!["partial-mutation-run".to_owned()]
                ),
                (MutationKillMatrixTargetStatus::Incomplete, scoped()),
            ]
        );
        assert!(matrix.candidates.is_empty());
    }
}
