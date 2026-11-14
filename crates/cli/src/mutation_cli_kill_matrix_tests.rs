//! The human output of `--kill-matrix-for` targets.

use fallow_output::{
    MutationBaselineFailure, MutationKillMatrixStatus, MutationKillMatrixTarget,
    MutationKillMatrixTargetStatus, MutationTargetOnlyKill,
};

use super::*;

#[test]
fn mutation_kill_matrix_for_human_report_names_targets_and_shared_kills() {
    let mut measured = report(
        vec![mutant("m1", "src/math.ts", 1, MutantStatus::Killed)],
        Vec::new(),
    );
    let mut matrix = kill_matrix_fixture();
    matrix.tests.truncate(2);
    matrix.coverage_without_kills.clear();
    matrix.targets = Some(vec![MutationKillMatrixTarget {
        target: "test/a.test.ts".to_owned(),
        test_ids: vec!["test/a.test.ts::adds".to_owned()],
        status: MutationKillMatrixTargetStatus::Complete,
        reasons: Vec::new(),
        baseline_failures: Vec::new(),
    }]);
    matrix.tests[0].subsuming_tests.clear();
    matrix.tests[0].notes = vec!["subsumers-not-computed".to_owned()];
    matrix.tests[0].target_only_kills = Some(vec![MutationTargetOnlyKill {
        mutant_id: "m1".to_owned(),
        targets: vec!["test/b.test.ts::adds".to_owned()],
    }]);
    matrix.tests[1].target_only_kills = Some(Vec::new());
    measured.kill_matrix = matrix;
    let text = run_text(&measured, &OPTIONS);
    for expected in [
        "Target test/a.test.ts: 1 test, complete\n",
        "Kills shared only with other targets:\n      m1 (also: test/b.test.ts::adds)",
        "Kills shared only with other targets: none",
        "Subsumers are not computed: the other tests stop at the first failure.",
    ] {
        assert!(text.contains(expected), "missing {expected:?}: {text}");
    }
    assert!(!text.contains("Individual subsumers"), "{text}");
}

#[test]
fn mutation_kill_matrix_for_human_report_shows_candidates_of_complete_targets() {
    let mut measured = report(
        vec![mutant("m1", "src/math.ts", 1, MutantStatus::Killed)],
        Vec::new(),
    );
    let mut matrix = kill_matrix_fixture();
    matrix.status = MutationKillMatrixStatus::Incomplete;
    matrix.reasons = vec!["incomplete-mutant-results".to_owned()];
    matrix.targets = Some(vec![
        MutationKillMatrixTarget {
            target: "test/a.test.ts".to_owned(),
            test_ids: vec!["test/a.test.ts::adds".to_owned()],
            status: MutationKillMatrixTargetStatus::Complete,
            reasons: Vec::new(),
            baseline_failures: Vec::new(),
        },
        MutationKillMatrixTarget {
            target: "test/c.test.ts".to_owned(),
            test_ids: vec!["test/c.test.ts::loads".to_owned()],
            status: MutationKillMatrixTargetStatus::Incomplete,
            reasons: vec!["incomplete-mutant-results".to_owned()],
            baseline_failures: Vec::new(),
        },
    ]);
    measured.kill_matrix = matrix;
    let text = run_text(&measured, &OPTIONS);
    for expected in [
        "Test kill evidence: incomplete",
        "Target test/a.test.ts: 1 test, complete\n",
        "Target test/c.test.ts: 1 test, incomplete (incomplete-mutant-results)\n",
        "Only the tests of complete targets can be candidates.",
        "Redundant-test candidates for review (1)",
    ] {
        assert!(text.contains(expected), "missing {expected:?}: {text}");
    }
    assert!(
        !text.contains("Candidates are unavailable because the kill matrix is incomplete."),
        "{text}"
    );
}

fn failing_target(target: &str, failures: &[&str]) -> MutationKillMatrixTarget {
    MutationKillMatrixTarget {
        target: target.to_owned(),
        test_ids: vec![format!("{target}::adds")],
        status: MutationKillMatrixTargetStatus::Incomplete,
        reasons: vec![
            "baseline-failure".to_owned(),
            "partial-mutation-run".to_owned(),
        ],
        baseline_failures: failures.iter().map(|id| (*id).to_owned()).collect(),
    }
}

#[test]
fn mutation_kill_matrix_for_human_report_names_the_baseline_failures_of_a_target() {
    let mut measured = report(
        vec![mutant("m1", "src/math.ts", 1, MutantStatus::Killed)],
        vec![MutationIncompleteReason::BaselineFailure],
    );
    measured.baseline_failures = vec![MutationBaselineFailure {
        file: Some("test/b.test.ts".to_owned()),
        test_id: Some("test/b.test.ts::fails".to_owned()),
        message: None,
        mutant_ids: Vec::new(),
    }];
    let mut matrix = kill_matrix_fixture();
    matrix.status = MutationKillMatrixStatus::Incomplete;
    matrix.reasons = vec![
        "baseline-failure".to_owned(),
        "partial-mutation-run".to_owned(),
    ];
    let many = [
        "test/c.test.ts::one",
        "test/c.test.ts::two",
        "test/c.test.ts::three",
        "test/c.test.ts::four",
        "test/c.test.ts::five",
    ];
    matrix.targets = Some(vec![
        MutationKillMatrixTarget {
            target: "test/a.test.ts".to_owned(),
            test_ids: vec!["test/a.test.ts::adds".to_owned()],
            status: MutationKillMatrixTargetStatus::Complete,
            reasons: Vec::new(),
            baseline_failures: Vec::new(),
        },
        failing_target("test/b.test.ts", &["test/b.test.ts::fails"]),
        failing_target("test/c.test.ts", &many),
    ]);
    measured.kill_matrix = matrix;
    let text = run_text(&measured, &OPTIONS);
    for expected in [
        "Target test/a.test.ts: 1 test, complete\n  Target test/b.test.ts",
        "Target test/b.test.ts: 1 test, incomplete (baseline-failure, partial-mutation-run)\n    Baseline failures: test/b.test.ts::fails\n",
        "    Baseline failures: test/c.test.ts::one, test/c.test.ts::two, test/c.test.ts::three, +2 more\n",
    ] {
        assert!(text.contains(expected), "missing {expected:?}: {text}");
    }
}
