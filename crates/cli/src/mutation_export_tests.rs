#![expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]

use super::*;
use fallow_mutation::{MutationOptions, SourceKind, instrument};

/// A two-byte and an astral character before the mutants on the same line.
const SOURCE: &str = "export function add(a: number, b: number) {\n  /* \u{e9}\u{1f600} */ return a + b;\n}\n\nexport function isPositive(n: number) {\n  return n > 0;\n}\n";

fn mutants() -> Vec<fallow_mutation::Mutant> {
    instrument(
        SOURCE,
        "src/math.ts",
        SourceKind::Ts,
        &MutationOptions::default(),
    )
    .unwrap()
    .mutants
}

fn find(original: &str, replacement: &str) -> fallow_mutation::Mutant {
    mutants()
        .into_iter()
        .find(|mutant| mutant.original == original && mutant.replacement == replacement)
        .unwrap()
}

fn entry(mutant: &fallow_mutation::Mutant, status: &str, extra: &Value) -> Value {
    let mut value = json!({
        "mutant_id": mutant.id.as_str(), "file": "src/math.ts", "line": mutant.line,
        "column": mutant.column, "span": [mutant.span.0, mutant.span.1],
        "operator": "arithmetic", "status": status,
    });
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    value
}

fn report(mutants: &[Value], completion: &Value) -> SavedReport {
    serde_json::from_value(json!({
        "kind": "mutation",
        "schema_version": "2",
        "completion": completion,
        "mutants": mutants,
        "files": [{
            "path": "src/math.ts",
            "source_sha256": sha256_hex(SOURCE.as_bytes()),
            "static_test_files": ["test/math.test.ts"],
        }],
    }))
    .unwrap()
}

fn complete() -> Value {
    json!({ "status": "complete", "reasons": [] })
}

fn project(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/math.ts"), source).unwrap();
    dir
}

fn exported(mutants: &[Value], completion: &Value) -> Value {
    let dir = project(SOURCE);
    export(&report(mutants, completion), dir.path()).unwrap()
}

fn only_mutant(document: &Value) -> &Value {
    &document["files"]["src/math.ts"]["mutants"][0]
}

#[test]
fn each_fallow_status_maps_to_a_schema_status() {
    let plus = find("+", "-");
    let cases = [
        ("killed", "Killed", false),
        ("timeout", "Timeout", false),
        ("survived", "Survived", false),
        ("no_coverage", "NoCoverage", false),
        ("runtime_error", "RuntimeError", true),
        ("compile_error", "CompileError", true),
        ("flaky", "Ignored", true),
        ("pruned", "Ignored", true),
        ("equivalent", "Ignored", true),
        ("pending", "Pending", true),
    ];
    for (status, expected, has_reason) in cases {
        let document = exported(&[entry(&plus, status, &json!({}))], &complete());
        let mutant = only_mutant(&document);
        assert_eq!(mutant["status"], expected, "status {status}");
        assert_eq!(
            mutant.get("statusReason").is_some(),
            has_reason,
            "reason of status {status}"
        );
    }
}

#[test]
fn ignored_statuses_say_why() {
    let plus = find("+", "-");
    let reason = |status: &str, extra: &Value| -> String {
        let document = exported(&[entry(&plus, status, extra)], &complete());
        only_mutant(&document)["statusReason"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(reason("flaky", &json!({})).starts_with("Flaky:"));
    assert!(reason("equivalent", &json!({})).starts_with("Equivalent:"));
    assert!(
        reason("pruned", &json!({ "prune_reason": "logging-only" })).contains("`logging-only`")
    );
    assert!(reason("compile_error", &json!({ "type_error_codes": [2322] })).contains("TS2322"));
}

#[test]
fn location_columns_count_utf16_units_from_one() {
    let plus = find("+", "-");
    let document = exported(&[entry(&plus, "survived", &json!({}))], &complete());
    let line = SOURCE.lines().nth(1).unwrap();
    let column = line[..line.find('+').unwrap()].encode_utf16().count() + 1;
    assert_eq!(
        only_mutant(&document)["location"],
        json!({
            "start": { "line": 2, "column": column },
            "end": { "line": 2, "column": column + 1 },
        })
    );
    assert_eq!(only_mutant(&document)["replacement"], "-");
    assert_eq!(document["files"]["src/math.ts"]["source"], SOURCE);
    assert_eq!(document["files"]["src/math.ts"]["language"], "typescript");
}

#[test]
fn tests_are_listed_by_file_and_a_kill_names_its_tests() {
    let plus = find("+", "-");
    let document = exported(
        &[entry(
            &plus,
            "killed",
            &json!({
                "covering_tests": ["test/math.test.ts::adds", "test/math.test.ts::adds twice"],
                "killed_by": "test/math.test.ts::adds",
            }),
        )],
        &complete(),
    );
    let mutant = only_mutant(&document);
    assert_eq!(mutant["killedBy"], json!(["test/math.test.ts::adds"]));
    assert_eq!(
        mutant["coveredBy"],
        json!(["test/math.test.ts::adds", "test/math.test.ts::adds twice"])
    );
    assert_eq!(
        document["testFiles"],
        json!({
            "test/math.test.ts": { "tests": [
                { "id": "test/math.test.ts::adds", "name": "adds" },
                { "id": "test/math.test.ts::adds twice", "name": "adds twice" },
            ]}
        })
    );
}

#[test]
fn a_partial_run_marks_survivors_and_uncovered_mutants_as_not_conclusive() {
    let plus = find("+", "-");
    let greater = find(">", ">=");
    let completion = json!({ "status": "partial", "reasons": ["time-budget"] });
    let document = exported(
        &[
            entry(&plus, "survived", &json!({})),
            entry(&greater, "no_coverage", &json!({})),
        ],
        &completion,
    );
    for mutant in document["files"]["src/math.ts"]["mutants"]
        .as_array()
        .unwrap()
    {
        assert!(
            mutant["statusReason"]
                .as_str()
                .unwrap()
                .contains("partial (time-budget)"),
            "{mutant}"
        );
    }
    assert_eq!(document["config"]["completion"], completion);
}

#[test]
fn a_complete_run_adds_no_note_to_a_survivor() {
    let plus = find("+", "-");
    let document = exported(&[entry(&plus, "survived", &json!({}))], &complete());
    assert!(only_mutant(&document).get("statusReason").is_none());
    assert_eq!(document["schemaVersion"], "2");
}

#[test]
fn a_plan_or_a_changed_file_is_refused() {
    let plus = find("+", "-");
    let mut plan = entry(&plus, "survived", &json!({}));
    plan.as_object_mut().unwrap().remove("status");
    let dir = project(SOURCE);
    assert!(export(&report(&[plan], &complete()), dir.path()).is_err());

    let changed = project(&SOURCE.replace("a + b", "a + b + 0"));
    let error = export(
        &report(&[entry(&plus, "survived", &json!({}))], &complete()),
        changed.path(),
    )
    .unwrap_err();
    assert!(error.message.contains("src/math.ts"), "{}", error.message);
}
