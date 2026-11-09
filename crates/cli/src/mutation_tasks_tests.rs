#![expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]

use super::*;
use fallow_mutation::{MutationOptions, SourceKind, instrument};
use fallow_output::MutationStaleReason;

const SOURCE: &str = "export function add(a: number, b: number) {\n  return a + b;\n}\n\nexport function isPositive(n: number) {\n  return n > 0;\n}\n";

fn mutants(source: &str) -> Vec<fallow_mutation::Mutant> {
    instrument(
        source,
        "src/math.ts",
        SourceKind::Ts,
        &MutationOptions::default(),
    )
    .unwrap()
    .mutants
}

fn find(source: &str, original: &str, replacement: &str) -> fallow_mutation::Mutant {
    mutants(source)
        .into_iter()
        .find(|mutant| mutant.original == original && mutant.replacement == replacement)
        .unwrap()
}

fn entry(
    mutant: &fallow_mutation::Mutant,
    function: &str,
    status: &str,
    covering: &[&str],
) -> serde_json::Value {
    serde_json::json!({
        "mutant_id": mutant.id.as_str(), "file": "src/math.ts", "line": mutant.line,
        "column": mutant.column, "span": [mutant.span.0, mutant.span.1],
        "operator": "x", "function": function, "status": status,
        "covering_tests": covering,
    })
}

fn report(mutants: &[serde_json::Value], completion: &str) -> SavedReport {
    let value = serde_json::json!({
        "kind": "mutation",
        "schema_version": "2",
        "completion": { "status": completion, "reasons": [] },
        "mutants": mutants,
        "files": [{
            "path": "src/math.ts",
            "source_sha256": sha256_hex(SOURCE.as_bytes()),
            "static_test_files": ["test/math.test.ts", "test/a.test.ts"],
        }],
    });
    serde_json::from_value(value).unwrap()
}

fn project(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/math.ts"), source).unwrap();
    dir
}

#[test]
fn only_survived_and_uncovered_mutants_become_tasks_in_source_order() {
    let plus = find(SOURCE, "+", "-");
    let greater = find(SOURCE, ">", ">=");
    let other = mutants(SOURCE)
        .into_iter()
        .find(|mutant| mutant.id != plus.id && mutant.id != greater.id)
        .unwrap();
    let report = report(
        &[
            entry(&greater, "isPositive", "no_coverage", &[]),
            entry(&other, "add", "killed", &["test/math.test.ts::adds"]),
            entry(
                &plus,
                "add",
                "survived",
                &["test/math.test.ts::adds", "test/math.test.ts::adds twice"],
            ),
        ],
        "complete",
    );
    let dir = project(SOURCE);

    let batch = tasks(&report, dir.path()).unwrap();

    assert_eq!(
        batch
            .tasks
            .iter()
            .map(|task| task.mutant_id.as_str())
            .collect::<Vec<_>>(),
        [plus.id.as_str(), greater.id.as_str()]
    );
    let first = &batch.tasks[0];
    assert_eq!(first.status, MutantStatus::Survived);
    assert_eq!(first.original_lines.as_deref(), Some("  return a + b;"));
    assert_eq!(first.mutated_lines.as_deref(), Some("  return a - b;"));
    assert_eq!(first.covering_test_files, ["test/math.test.ts"]);
    assert_eq!(batch.tasks[1].status, MutantStatus::NoCoverage);
    assert!(batch.tasks[1].covering_test_files.is_empty());
    assert_eq!(
        batch.summary,
        MutationTasksSummary {
            tasks: 2,
            survived: 1,
            no_coverage: 1,
            stale: 0
        }
    );
    assert_eq!(
        batch.report_completion,
        Some(fallow_output::MutationCompletionStatus::Complete)
    );
}

#[test]
fn a_changed_file_gives_stale_tasks_without_code() {
    let plus = find(SOURCE, "+", "-");
    let report = report(&[entry(&plus, "add", "survived", &[])], "complete");
    let dir = project(&SOURCE.replace("a + b", "b + a"));

    let batch = tasks(&report, dir.path()).unwrap();

    let task = &batch.tasks[0];
    assert_eq!(task.stale_reason, Some(MutationStaleReason::SourceChanged));
    assert_eq!(task.original_lines, None);
    assert_eq!(batch.summary.stale, 1);
    let text = render_text(&batch);
    assert!(
        text.contains("1 open (1 survived); 1 stale, without code"),
        "{text}"
    );
    assert!(
        text.contains("stale: src/math.ts changed after the run"),
        "{text}"
    );
}

#[test]
fn a_plan_document_and_an_escaping_path_are_refused() {
    let plan: SavedReport =
        serde_json::from_value(serde_json::json!({"kind": "mutation-plan", "mutants": []}))
            .unwrap();
    let error = tasks(&plan, Path::new("/")).unwrap_err();
    assert!(error.message.contains("mutation plan"), "{}", error.message);

    let escaping: SavedReport = serde_json::from_value(serde_json::json!({
        "kind": "mutation",
        "mutants": [{"mutant_id": "m1", "file": "../x.ts", "line": 1, "column": 1,
                     "span": [0, 1], "operator": "x", "status": "survived"}],
    }))
    .unwrap();
    let error = tasks(&escaping, Path::new("/tmp")).unwrap_err();
    assert!(
        error.message.contains("not inside the project root"),
        "{}",
        error.message
    );
}

#[test]
fn a_project_prefixed_test_id_maps_to_its_static_test_file() {
    let files = covering_test_files(
        &[
            "web::test/math.test.ts::adds".to_owned(),
            "test/other.test.ts::x".to_owned(),
            "api::test/api.test.ts::y".to_owned(),
            "no file here::x".to_owned(),
        ],
        &["test/math.test.ts".to_owned()],
    );
    assert_eq!(
        files,
        [
            "test/api.test.ts",
            "test/math.test.ts",
            "test/other.test.ts"
        ]
    );
}

#[test]
fn the_text_batch_has_a_header_and_one_dedented_item_per_task() {
    let plus = find(SOURCE, "+", "-");
    let greater = find(SOURCE, ">", ">=");
    let report = report(
        &[
            entry(&plus, "add", "survived", &["test/math.test.ts::adds"]),
            entry(&greater, "isPositive", "no_coverage", &[]),
        ],
        "partial",
    );
    let dir = project(SOURCE);
    let batch = tasks(&report, dir.path()).unwrap();

    let text = render_text(&batch);

    assert_eq!(
        text,
        format!(
            "Mutation tasks: 2 open (1 survived, 1 without coverage)\n\
             \n\
             Each item is a small change to the code that made no test fail. Write tests that pass on the current code and fail on the changed code. One test can cover several items. Skip an item when no input can detect the change; a verdict document (`--verdicts`) can mark it as equivalent.\n\
             \n\
             The report is partial, so this list can miss a mutant that did not run.\n\
             \n\
             1. src/math.ts:2, function add (survived) [{plus}]\n   \
             current: return a + b;\n   \
             changed: return a - b;\n   \
             tests: test/math.test.ts\n\
             \n\
             2. src/math.ts:6, function isPositive (no test reaches it) [{greater}]\n   \
             current: return n > 0;\n   \
             changed: return n >= 0;\n",
            plus = plus.id.as_str(),
            greater = greater.id.as_str(),
        )
    );
}

#[test]
fn the_markdown_batch_fences_the_code_longer_than_its_backticks() {
    let source = "export const quote = (a: number) => a > 1 && `x`;\n";
    let plus = find(source, ">", ">=");
    let value = serde_json::json!({
        "kind": "mutation",
        "mutants": [{"mutant_id": plus.id.as_str(), "file": "src/math.ts", "line": 1,
                     "column": 1, "span": [plus.span.0, plus.span.1], "operator": "x",
                     "status": "survived"}],
        "files": [{"path": "src/math.ts", "source_sha256": sha256_hex(source.as_bytes())}],
    });
    let report: SavedReport = serde_json::from_value(value).unwrap();
    let dir = project(source);
    let batch = tasks(&report, dir.path()).unwrap();

    let markdown = render_markdown(&batch);

    assert!(
        markdown.starts_with("## Mutation tasks\n\n1 open (1 survived)\n"),
        "{markdown}"
    );
    let item = format!(
        "1. `src/math.ts:1` (survived) `{}`\n\n   ```text\n   current: export const quote = (a: number) => a > 1 && `x`;\n",
        plus.id.as_str()
    );
    assert!(markdown.contains(&item), "{markdown}");
    assert!(
        markdown.contains("changed: export const quote = (a: number) => a >= 1 && `x`;\n   ```\n"),
        "{markdown}"
    );
}

#[test]
fn an_empty_batch_says_so() {
    let batch = tasks(&report(&[], "complete"), Path::new("/")).unwrap();
    assert_eq!(batch.summary.tasks, 0);
    assert_eq!(
        render_text(&batch),
        "No open mutation tasks: no mutant survived and each mutant has coverage.\n"
    );
}

#[test]
fn a_task_carries_the_equivalence_hint_of_its_survivor() {
    let plus = find(SOURCE, "+", "-");
    let greater = find(SOURCE, ">", ">=");
    let mut hinted = entry(&plus, "add", "survived", &["test/math.test.ts::add"]);
    hinted["equivalence_hint"] = serde_json::json!("type-non-nullish");
    let plain = entry(&greater, "isPositive", "survived", &[]);
    let dir = project(SOURCE);
    let output = tasks(&report(&[hinted, plain], "complete"), dir.path()).unwrap();
    let hints: Vec<_> = output
        .tasks
        .iter()
        .map(|task| (task.mutant_id.as_str(), task.equivalence_hint))
        .collect();
    assert!(hints.contains(&(
        plus.id.as_str(),
        Some(fallow_output::MutantEquivalenceHint::TypeNonNullish)
    )));
    assert!(hints.contains(&(greater.id.as_str(), None)));
    let text = render_text(&output);
    assert_eq!(
        text.matches("hint: the type of the receiver").count(),
        1,
        "{text}"
    );
    let json = serde_json::to_value(&output).unwrap();
    let json_hints = json["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|task| task["equivalence_hint"] == "type-non-nullish")
        .count();
    assert_eq!(json_hints, 1, "{json}");
}
