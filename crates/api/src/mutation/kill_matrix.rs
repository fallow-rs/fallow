//! Conservative redundancy signals from complete observed kill evidence.

use std::collections::{BTreeMap, BTreeSet};

use fallow_engine::mutation::PlannedMutant;
use fallow_output::{
    MutantResult, MutantStatus, MutationBaselineFailure, MutationCompletion,
    MutationCompletionStatus, MutationKillMatrix, MutationKillMatrixStatus, MutationTestKillRow,
};
use rustc_hash::FxHashMap;

type KillSets = BTreeMap<String, BTreeSet<String>>;

#[derive(Default)]
struct Evidence {
    kills: KillSets,
    coverage: KillSets,
    file_tests: KillSets,
    incomplete: bool,
    missing_test_files: bool,
}

pub fn build(
    enabled: bool,
    results: &[MutantResult],
    baseline: &[MutationBaselineFailure],
    completion: &MutationCompletion,
    planned: &[PlannedMutant],
    test_files: Option<&FxHashMap<String, String>>,
) -> MutationKillMatrix {
    if !enabled {
        return MutationKillMatrix::default();
    }
    let empty_files = FxHashMap::default();
    let test_files = test_files.unwrap_or(&empty_files);
    let evidence = observe(results, baseline, test_files);
    let mut reasons = BTreeSet::new();
    if completion.status != MutationCompletionStatus::Complete {
        reasons.insert("partial-mutation-run".to_owned());
    }
    if !baseline.is_empty() {
        reasons.insert("baseline-failure".to_owned());
    }
    if evidence.incomplete {
        reasons.insert("incomplete-mutant-results".to_owned());
    }
    if evidence.missing_test_files {
        reasons.insert("missing-test-file-metadata".to_owned());
    }
    let complete = reasons.is_empty();
    let tests = rows(&evidence, planned, test_files, complete);
    let candidates = tests
        .iter()
        .filter(|row| complete && !row.kills.is_empty() && row.unique_kills.is_empty())
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
        reasons: reasons.into_iter().collect(),
        tests,
        candidates,
        coverage_without_kills,
    }
}

fn observe(
    results: &[MutantResult],
    baseline: &[MutationBaselineFailure],
    test_files: &FxHashMap<String, String>,
) -> Evidence {
    let excluded = |id: &str, file: &str| {
        baseline.iter().any(|failure| {
            failure.test_id.as_deref() == Some(id)
                || (failure.test_id.is_none() && failure.file.as_deref() == Some(file))
        })
    };
    let mut evidence = Evidence::default();
    for result in results {
        if matches!(
            result.status,
            MutantStatus::Pruned | MutantStatus::Equivalent | MutantStatus::NoCoverage
        ) {
            continue;
        }
        evidence.incomplete |= !result.kill_matrix_complete
            || !matches!(
                result.status,
                MutantStatus::Killed | MutantStatus::Survived | MutantStatus::CompileError
            );
        for test in result.covering_tests.iter().chain(&result.killing_tests) {
            let Some(file) = test_files.get(test).filter(|file| !file.is_empty()) else {
                evidence.missing_test_files = true;
                continue;
            };
            if excluded(test, file) {
                continue;
            }
            evidence.kills.entry(test.clone()).or_default();
            evidence
                .coverage
                .entry(test.clone())
                .or_default()
                .insert(result.location.file.clone());
            evidence
                .file_tests
                .entry(result.location.file.clone())
                .or_default()
                .insert(test.clone());
        }
        if result.status != MutantStatus::Killed {
            continue;
        }
        for test in &result.killing_tests {
            if let Some(kills) = evidence.kills.get_mut(test) {
                kills.insert(result.location.mutant_id.clone());
            }
        }
    }
    evidence
}

fn rows(
    evidence: &Evidence,
    planned: &[PlannedMutant],
    test_files: &FxHashMap<String, String>,
    complete: bool,
) -> Vec<MutationTestKillRow> {
    let mut killers_by_mutant: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (test, kills) in &evidence.kills {
        for mutant in kills {
            killers_by_mutant.entry(mutant).or_default().insert(test);
        }
    }
    evidence
        .kills
        .iter()
        .map(|(id, kills)| {
            let unique_kills = kills
                .iter()
                .filter(|mutant| {
                    killers_by_mutant
                        .get(mutant.as_str())
                        .is_some_and(|tests| tests.len() == 1)
                })
                .cloned()
                .collect();
            MutationTestKillRow {
                test_id: id.clone(),
                file: test_files[id].clone(),
                kills: kills.iter().cloned().collect(),
                unique_kills,
                subsuming_tests: subsumers(id, kills, &killers_by_mutant),
                notes: notes(id, &test_files[id], evidence, planned, complete),
            }
        })
        .collect()
}

fn notes(
    id: &str,
    file: &str,
    evidence: &Evidence,
    planned: &[PlannedMutant],
    complete: bool,
) -> Vec<String> {
    let mut notes = BTreeSet::new();
    if !complete {
        notes.insert("incomplete-observed-evidence".to_owned());
    }
    if evidence.coverage.get(id).into_iter().flatten().any(|file| {
        evidence
            .file_tests
            .get(file)
            .is_some_and(|tests| tests.len() == 1)
    }) {
        notes.insert("observed-instrumented-file-only-test".to_owned());
    }
    if planned
        .iter()
        .filter_map(|mutant| mutant.test_reach.as_ref())
        .any(|reach| {
            reach.export_name.is_some()
                && reach.export_test_files.len() == 1
                && reach.export_test_files[0] == file
                && matches!(
                    reach.reach,
                    fallow_engine::module_graph::ExportTestReach::Direct
                        | fallow_engine::module_graph::ExportTestReach::Transitive
                )
        })
    {
        notes.insert("static-test-file-only-export-reach".to_owned());
    }
    if !planned
        .iter()
        .filter_map(|mutant| mutant.test_reach.as_ref())
        .any(|reach| {
            reach.export_name.is_some()
                && reach
                    .export_test_files
                    .iter()
                    .any(|test_file| test_file == file)
        })
    {
        notes.insert("static-export-reach-unavailable".to_owned());
    }
    notes.into_iter().collect()
}

fn subsumers(
    id: &str,
    kills: &BTreeSet<String>,
    killers: &BTreeMap<&str, BTreeSet<&str>>,
) -> Vec<String> {
    let mut sets = kills
        .iter()
        .filter_map(|mutant| killers.get(mutant.as_str()));
    let Some(first) = sets.next() else {
        return Vec::new();
    };
    let mut shared = first.clone();
    for set in sets {
        shared.retain(|test| set.contains(test));
    }
    shared.remove(id);
    shared.into_iter().map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{MutantLocation, MutantStatus};

    fn build(
        enabled: bool,
        results: &[MutantResult],
        baseline: &[MutationBaselineFailure],
        completion: &MutationCompletion,
        planned: &[PlannedMutant],
    ) -> MutationKillMatrix {
        let files = FxHashMap::from_iter([
            ("test/a.ts::a".to_owned(), "test/a.ts".to_owned()),
            ("test/b.ts::b".to_owned(), "test/b.ts".to_owned()),
            ("test/c.ts::c".to_owned(), "test/c.ts".to_owned()),
        ]);
        super::build(
            enabled,
            results,
            baseline,
            completion,
            planned,
            Some(&files),
        )
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
            covering_tests: vec![
                "test/a.ts::a".to_owned(),
                "test/b.ts::b".to_owned(),
                "test/c.ts::c".to_owned(),
            ],
            static_mutant: false,
            duration_ms: None,
            test_reach: None,
            retried: false,
            attempts: Vec::new(),
            cached: false,
            resumed: false,
            type_error_codes: Vec::new(),
        }
    }

    #[test]
    fn strict_subset_is_a_candidate_and_unique_kills_are_protected() {
        let matrix = build(
            true,
            &[
                result("m2", MutantStatus::Killed, &["test/b.ts::b"]),
                result(
                    "m1",
                    MutantStatus::Killed,
                    &["test/b.ts::b", "test/a.ts::a"],
                ),
            ],
            &[],
            &MutationCompletion::from_reasons(Vec::new()),
            &[],
        );
        assert_eq!(matrix.candidates, ["test/a.ts::a"]);
        assert_eq!(matrix.tests[0].kills, ["m1"]);
        assert_eq!(matrix.tests[0].subsuming_tests, ["test/b.ts::b"]);
        assert_eq!(matrix.tests[1].unique_kills, ["m2"]);
        assert_eq!(matrix.coverage_without_kills, ["test/c.ts::c"]);
    }
    #[test]
    fn disabled_incomplete_and_baseline_evidence_cannot_produce_candidates() {
        let complete = MutationCompletion::from_reasons(Vec::new());
        let source = vec![result(
            "m1",
            MutantStatus::Killed,
            &["test/a.ts::a", "test/b.ts::b"],
        )];
        let valid = build(true, &source, &[], &complete, &[]);
        assert_eq!(valid.candidates, ["test/a.ts::a", "test/b.ts::b"]);
        let disabled = build(false, &source, &[], &complete, &[]);
        assert_eq!(disabled.status, MutationKillMatrixStatus::Unavailable);
        assert!(disabled.candidates.is_empty());
        let mut incomplete = source.clone();
        incomplete[0].kill_matrix_complete = false;
        let partial = build(true, &incomplete, &[], &complete, &[]);
        assert_eq!(partial.status, MutationKillMatrixStatus::Incomplete);
        assert_eq!(partial.reasons, ["incomplete-mutant-results"]);
        assert!(partial.candidates.is_empty());
        assert_eq!(partial.tests[0].kills, ["m1"]);
        for test_id in [Some("test/b.ts::b".to_owned()), None] {
            let failure = MutationBaselineFailure {
                file: Some("test/b.ts".to_owned()),
                test_id,
                message: None,
                mutant_ids: vec!["m1".to_owned()],
            };
            let matrix = build(true, &source, &[failure], &complete, &[]);
            assert_eq!(
                matrix
                    .tests
                    .iter()
                    .map(|row| row.test_id.as_str())
                    .collect::<Vec<_>>(),
                ["test/a.ts::a", "test/c.ts::c"]
            );
            assert_eq!(matrix.tests[0].unique_kills, ["m1"]);
            assert!(matrix.tests[0].subsuming_tests.is_empty());
            assert!(matrix.candidates.is_empty());
        }
    }

    #[test]
    fn collective_coverage_qualifies_and_timeouts_and_survivors_do_not_kill() {
        let matrix = build(
            true,
            &[
                result(
                    "m1",
                    MutantStatus::Killed,
                    &["test/a.ts::a", "test/b.ts::b"],
                ),
                result(
                    "m2",
                    MutantStatus::Killed,
                    &["test/a.ts::a", "test/c.ts::c"],
                ),
                result("m3", MutantStatus::Survived, &["test/a.ts::a"]),
            ],
            &[],
            &MutationCompletion::from_reasons(Vec::new()),
            &[],
        );
        assert_eq!(
            matrix.candidates,
            ["test/a.ts::a", "test/b.ts::b", "test/c.ts::c"]
        );
        assert_eq!(matrix.tests[0].kills, ["m1", "m2"]);
        assert!(matrix.tests[0].subsuming_tests.is_empty());
        assert_eq!(matrix.tests[1].subsuming_tests, ["test/a.ts::a"]);
        let timeout = build(
            true,
            &[result("m4", MutantStatus::Timeout, &["test/a.ts::a"])],
            &[],
            &MutationCompletion::from_reasons(Vec::new()),
            &[],
        );
        assert_eq!(timeout.status, MutationKillMatrixStatus::Incomplete);
        assert_eq!(
            timeout.coverage_without_kills,
            ["test/a.ts::a", "test/b.ts::b", "test/c.ts::c"]
        );
        assert!(timeout.candidates.is_empty());
    }
    #[test]
    fn prefixed_ids_use_runner_files_for_file_level_baseline_exclusion() {
        let first = "alpha::test/a.ts::a";
        let second = "beta::test/b.ts::b";
        let mut mutant = result("m1", MutantStatus::Killed, &[first, second]);
        mutant.covering_tests = mutant.killing_tests.clone();
        let files = FxHashMap::from_iter([
            (first.to_owned(), "test/a.ts".to_owned()),
            (second.to_owned(), "test/b.ts".to_owned()),
        ]);
        let completion = MutationCompletion::from_reasons(Vec::new());
        let valid = super::build(true, &[mutant.clone()], &[], &completion, &[], Some(&files));
        assert_eq!(valid.candidates, [first, second]);
        assert_eq!(
            valid
                .tests
                .iter()
                .map(|row| row.file.as_str())
                .collect::<Vec<_>>(),
            ["test/a.ts", "test/b.ts"]
        );
        let failure = MutationBaselineFailure {
            file: Some("test/a.ts".to_owned()),
            test_id: None,
            message: None,
            mutant_ids: Vec::new(),
        };
        let excluded = super::build(true, &[mutant], &[failure], &completion, &[], Some(&files));
        assert_eq!(
            excluded
                .tests
                .iter()
                .map(|row| row.test_id.as_str())
                .collect::<Vec<_>>(),
            [second]
        );
        assert_eq!(excluded.tests[0].unique_kills, ["m1"]);
        assert!(excluded.tests[0].subsuming_tests.is_empty());
        assert!(excluded.candidates.is_empty());
    }

    #[test]
    fn missing_runner_file_metadata_omits_unsupported_rows_and_suppresses_candidates() {
        let first = "alpha::test/a.ts::a";
        let second = "beta::test/b.ts::b";
        let mut mutant = result("m1", MutantStatus::Killed, &[first, second]);
        mutant.covering_tests = mutant.killing_tests.clone();
        let files = FxHashMap::from_iter([
            (first.to_owned(), "test/a.ts".to_owned()),
            (second.to_owned(), "test/b.ts".to_owned()),
        ]);
        let completion = MutationCompletion::from_reasons(Vec::new());
        let valid = super::build(true, &[mutant.clone()], &[], &completion, &[], Some(&files));
        assert_eq!(valid.candidates, [first, second]);
        let unavailable = super::build(true, &[mutant], &[], &completion, &[], None);
        assert_eq!(unavailable.status, MutationKillMatrixStatus::Incomplete);
        assert_eq!(unavailable.reasons, ["missing-test-file-metadata"]);
        assert!(unavailable.tests.is_empty());
        assert!(unavailable.candidates.is_empty());
    }
}
