#![expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]

use super::*;

const SOURCE: &str = "export const add = (a, b) => a + b;\n";

fn project(report: &serde_json::Value) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/add.js"), SOURCE).unwrap();
    let path = dir.path().join("report.json");
    std::fs::write(&path, serde_json::to_vec(report).unwrap()).unwrap();
    (dir, path)
}

fn mutant(id: &str, line: u32, status: &str) -> serde_json::Value {
    serde_json::json!({
        "mutant_id": id, "file": "src/add.js", "line": line, "column": 1,
        "span": [0, 1], "operator": "x", "status": status,
    })
}

fn report_value(mutants: &[serde_json::Value], sha: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "mutation",
        "mutants": mutants,
        "files": [{ "path": "src/add.js", "source_sha256": sha }],
    })
}

fn current_sha() -> String {
    sha256_hex(SOURCE.as_bytes())
}

#[test]
fn without_ids_the_open_tasks_are_selected() {
    let value = report_value(
        &[
            mutant("a", 1, "survived"),
            mutant("b", 1, "killed"),
            mutant("c", 1, "no_coverage"),
        ],
        &current_sha(),
    );
    let (dir, path) = project(&value);

    let selection = select(&path, dir.path(), &[]).unwrap();

    assert_eq!(selection.mutant_ids(), ["a", "c"]);
    assert_eq!(selection.files(), [dir.path().join("src/add.js")]);
}

#[test]
fn named_ids_select_any_status_and_an_unknown_id_is_refused() {
    let value = report_value(
        &[mutant("a", 1, "survived"), mutant("b", 1, "killed")],
        &current_sha(),
    );
    let (dir, path) = project(&value);

    let selection = select(&path, dir.path(), &["b".to_owned()]).unwrap();
    assert_eq!(selection.mutant_ids(), ["b"]);

    let error = select(&path, dir.path(), &["b".to_owned(), "zz".to_owned()]).unwrap_err();
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_UNKNOWN_MUTANT")
    );
    assert!(error.message.contains("`zz`"), "{}", error.message);
}

#[test]
fn a_report_without_open_tasks_and_a_changed_source_are_refused() {
    let (dir, path) = project(&report_value(&[mutant("a", 1, "killed")], &current_sha()));
    let error = select(&path, dir.path(), &[]).unwrap_err();
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_NOTHING_TO_RECHECK")
    );

    let (dir, path) = project(&report_value(&[mutant("a", 1, "survived")], "0000"));
    let error = select(&path, dir.path(), &[]).unwrap_err();
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_SOURCE_CHANGED")
    );
    assert!(error.message.contains("src/add.js"), "{}", error.message);
}

#[test]
fn each_status_maps_to_one_outcome() {
    use MutantStatus as S;
    use MutationRecheckOutcome as O;
    let cases = [
        (S::Killed, false, O::Killed),
        (S::Timeout, false, O::Killed),
        (S::Killed, true, O::Killed),
        (S::Survived, false, O::Survived),
        (S::NoCoverage, false, O::NoCoverage),
        (S::Survived, true, O::TestsFailOnOriginal),
        (S::NoCoverage, true, O::TestsFailOnOriginal),
        (S::RuntimeError, false, O::Unsettled),
        (S::Flaky, false, O::Unsettled),
        (S::Pending, false, O::Unsettled),
    ];
    for (status, fails, expected) in cases {
        assert_eq!(outcome(status, fails), expected, "{status:?} {fails}");
    }
}

#[test]
fn the_summary_names_the_next_step() {
    let mut recheck = MutationRecheck {
        report_sha256: String::new(),
        killed: 2,
        survived: 1,
        no_coverage: 0,
        tests_fail_on_original: 0,
        unsettled: 0,
        mutants: Vec::new(),
    };
    let text = render(&recheck);
    assert!(text.contains("     2  killed\n"), "{text}");
    assert!(text.contains("     1  still survived\n"), "{text}");
    assert!(text.contains("fallow mutation tasks"), "{text}");

    recheck.survived = 0;
    assert!(render(&recheck).contains("Each rechecked mutant is killed."));

    recheck.tests_fail_on_original = 1;
    assert!(render(&recheck).contains("fails on the original code"));
}
