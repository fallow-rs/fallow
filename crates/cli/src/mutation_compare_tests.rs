#![expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]

use fallow_output::MutantLocation;

use super::*;

fn saved(id: &str, line: u32, status: &str, killers: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "mutant_id": id, "file": "src/add.js", "line": line, "column": 1,
        "span": [0, 1], "operator": "x", "status": status,
        "killed_by": killers.first(),
        "killing_tests": killers,
    })
}

fn write_report(
    mutants: &[serde_json::Value],
    completion: Option<&str>,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let mut value = serde_json::json!({ "kind": "mutation", "mutants": mutants });
    if let Some(status) = completion {
        value["completion"] = serde_json::json!({ "status": status, "reasons": [] });
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report.json");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    (dir, path)
}

fn current(id: &str, line: u32, status: MutantStatus) -> MutantResult {
    MutantResult {
        location: MutantLocation {
            mutant_id: id.to_owned(),
            file: "src/add.js".to_owned(),
            line,
            column: 1,
            span: (0, 1),
            operator: "x".to_owned(),
            function: None,
            function_line: None,
            rank: None,
            change: None,
        },
        status,
        prune_reason: None,
        killed_by: None,
        killing_tests: Vec::new(),
        kill_matrix_complete: false,
        covering_tests: Vec::new(),
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

#[test]
fn a_killed_mutant_that_survives_now_regresses_with_its_earlier_killers() {
    let (_dir, path) = write_report(
        &[
            saved("a", 1, "killed", &["t.test.js::b", "t.test.js::a"]),
            saved("b", 2, "timeout", &[]),
            saved("c", 3, "killed", &["t.test.js::a"]),
            saved("d", 4, "survived", &[]),
        ],
        Some("complete"),
    );
    let baseline = load(&path).unwrap();

    let compare = baseline.compare(&[
        current("a", 1, MutantStatus::Survived),
        current("b", 2, MutantStatus::NoCoverage),
        current("c", 3, MutantStatus::Killed),
        current("d", 4, MutantStatus::Killed),
    ]);

    assert_eq!(compare.compared, 4);
    assert_eq!(compare.regressed, 2);
    let ids: Vec<&str> = compare
        .mutants
        .iter()
        .map(|m| m.mutant_id.as_str())
        .collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(
        compare.mutants[0].previous_killing_tests,
        ["t.test.js::a", "t.test.js::b"]
    );
    assert_eq!(
        compare.mutants[0].previous_status,
        Some(MutantStatus::Killed)
    );
    assert_eq!(compare.mutants[0].status, Some(MutantStatus::Survived));
    assert!(compare.mutants[1].previous_killing_tests.is_empty());
    assert_eq!(compare.report_sha256.len(), 64);
}

#[test]
fn flaky_in_either_run_is_inconclusive_and_never_a_regression() {
    let (_dir, path) = write_report(
        &[
            saved("a", 1, "flaky", &[]),
            saved("b", 2, "killed", &["t.test.js::a"]),
        ],
        Some("complete"),
    );
    let baseline = load(&path).unwrap();

    let compare = baseline.compare(&[
        current("a", 1, MutantStatus::Survived),
        current("b", 2, MutantStatus::Flaky),
    ]);

    assert_eq!(compare.regressed, 0);
    assert_eq!(compare.inconclusive, 2);
    assert!(
        compare
            .mutants
            .iter()
            .all(|m| m.change == MutationCompareChange::Inconclusive)
    );
}

#[test]
fn an_id_in_one_run_only_is_added_or_removed() {
    let (_dir, path) = write_report(
        &[saved("old", 1, "killed", &["t.test.js::a"])],
        Some("complete"),
    );
    let baseline = load(&path).unwrap();

    let compare = baseline.compare(&[current("new", 2, MutantStatus::Survived)]);

    assert_eq!(compare.compared, 0);
    assert_eq!(
        (compare.regressed, compare.added, compare.removed),
        (0, 1, 1)
    );
    assert_eq!(compare.mutants[0].mutant_id, "old");
    assert_eq!(compare.mutants[0].change, MutationCompareChange::Removed);
    assert_eq!(compare.mutants[0].status, None);
    assert_eq!(compare.mutants[0].previous_killing_tests, ["t.test.js::a"]);
    assert_eq!(compare.mutants[1].change, MutationCompareChange::Added);
    assert_eq!(compare.mutants[1].previous_status, None);
}

#[test]
fn a_saved_report_that_is_not_complete_is_refused() {
    for completion in [Some("partial"), None] {
        let (_dir, path) = write_report(&[saved("a", 1, "killed", &[])], completion);
        let error = load(&path).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert_eq!(error.code.as_deref(), Some(INCOMPLETE_CODE));
    }
}

#[test]
fn a_plan_is_refused_as_a_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plan.json");
    std::fs::write(&path, r#"{"kind":"mutation-plan","mutants":[]}"#).unwrap();
    let error = load(&path).unwrap_err();
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_REPORT_INVALID")
    );
}

#[test]
fn human_output_names_the_earlier_killing_test() {
    let (_dir, path) = write_report(
        &[saved("a", 7, "killed", &["t.test.js::adds"])],
        Some("complete"),
    );
    let compare = load(&path)
        .unwrap()
        .compare(&[current("a", 7, MutantStatus::Survived)]);

    let text = render(&compare);

    assert!(text.contains("1  regressed"), "{text}");
    assert!(
        text.contains("src/add.js:7  a  killed -> survived, killed before by t.test.js::adds"),
        "{text}"
    );
}

#[test]
fn human_output_without_a_shared_mutant_does_not_claim_a_clean_compare() {
    let (_dir, path) = write_report(
        &[saved("old", 1, "killed", &["t.test.js::a"])],
        Some("complete"),
    );
    let compare = load(&path)
        .unwrap()
        .compare(&[current("new", 2, MutantStatus::Killed)]);

    let text = render(&compare);

    assert!(!text.contains("No mutant lost its detection."), "{text}");
    assert!(text.contains("No mutant id is in both runs"), "{text}");
}
