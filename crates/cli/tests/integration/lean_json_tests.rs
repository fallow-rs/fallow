//! The `--lean` JSON profile and the `_meta.rules` trim of `--explain`.
//!
//! Each lean test runs the same command twice, with and without `--lean`, and
//! compares the two documents. So the tests prove what the lean pass removes
//! and that the moved action text comes back without loss.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "tests use expect and panic to keep fixture setup concise"
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use crate::common::{run_fallow, run_fallow_in_root, run_fallow_raw};

/// Files in the health fixture. More than the default lean table cap of 10.
const HEALTH_FIXTURE_FILES: usize = 14;

/// A project with one complex function and one unused export per file, so
/// health reports a finding and a file score per file, and dead code reports
/// an unused export per file.
fn health_fixture() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("src")).expect("create src");
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"lean-fixture","main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    let mut index = String::new();
    for file in 1..=HEALTH_FIXTURE_FILES {
        let mut source = format!("export function branchy{file}(value: number): number {{\n");
        source.push_str("  let total = 0;\n");
        for branch in 1..=24 {
            writeln!(source, "  if (value === {branch}) {{ total += {branch}; }}")
                .expect("write to string");
        }
        source.push_str("  return total;\n}\n");
        writeln!(source, "export const unused{file} = {file};").expect("write to string");
        std::fs::write(dir.join(format!("src/mod{file}.ts")), source).expect("write module");
        writeln!(
            index,
            "import {{ branchy{file} }} from './mod{file}';\nbranchy{file}(1);"
        )
        .expect("write to string");
    }
    std::fs::write(dir.join("src/index.ts"), index).expect("write index");
    tmp
}

/// A project with no dead-code finding.
fn clean_fixture() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("src")).expect("create src");
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"clean-fixture","main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::write(
        dir.join("src/index.ts"),
        "import { used } from './lib';\nused();\n",
    )
    .expect("write index");
    std::fs::write(dir.join("src/lib.ts"), "export const used = () => 1;\n").expect("write lib");
    tmp
}

fn parse(stdout: &str) -> Value {
    serde_json::from_str(stdout)
        .unwrap_or_else(|err| panic!("stdout must be one JSON document: {err}\n{stdout}"))
}

fn health_json(root: &Path, extra: &[&str]) -> Value {
    let mut args = vec![
        "--format",
        "json",
        "--quiet",
        "--complexity",
        "--file-scores",
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_in_root("health", root, &args);
    assert!(
        output.code == 0 || output.code == 1,
        "health must run: {}",
        output.stderr
    );
    parse(&output.stdout)
}

fn dead_code_json(extra: &[&str]) -> Value {
    let mut args = vec!["--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = run_fallow("dead-code", "basic-project", &args);
    assert_eq!(
        output.code, 1,
        "basic-project has findings: {}",
        output.stderr
    );
    parse(&output.stdout)
}

fn combined_json(root: &Path, extra: &[&str]) -> Value {
    let mut args = vec![
        "--root".to_string(),
        root.to_str().expect("UTF-8 temp path").to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];
    args.extend(extra.iter().map(ToString::to_string));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_fallow_raw(&refs);
    assert_eq!(output.code, 0, "combined JSON exits 0: {}", output.stderr);
    parse(&output.stdout)
}

/// Collect every `actions` array outside `_meta`, keyed by its JSON pointer.
fn collect_actions(value: &Value, pointer: &str, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if pointer.is_empty() && key == "_meta" {
                    continue;
                }
                let child_pointer = format!("{pointer}/{key}");
                if key == "actions" && child.is_array() {
                    out.insert(child_pointer.clone(), child.clone());
                }
                collect_actions(child, &child_pointer, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_actions(item, &format!("{pointer}/{index}"), out);
            }
        }
        _ => {}
    }
}

/// Put the text that the lean pass moved to `_meta.actions` back on each
/// action: `action.field ?? _meta.actions[type].field`.
fn restore_actions(lean: &Value) -> BTreeMap<String, Value> {
    let shared = lean
        .pointer("/_meta/actions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut actions = BTreeMap::new();
    collect_actions(lean, "", &mut actions);
    for list in actions.values_mut() {
        for action in list.as_array_mut().expect("actions is an array") {
            let Some(action) = action.as_object_mut() else {
                continue;
            };
            let Some(kind) = action.get("type").and_then(Value::as_str) else {
                continue;
            };
            let Some(defaults) = shared.get(kind).and_then(Value::as_object) else {
                continue;
            };
            for (field, text) in defaults {
                if !action.contains_key(field) {
                    action.insert(field.clone(), text.clone());
                }
            }
        }
    }
    actions
}

/// The lossless proof: the lean actions plus `_meta.actions` equal the full
/// actions, and the lean document really moved some text.
fn assert_actions_restore(full: &Value, lean: &Value) {
    let mut expected = BTreeMap::new();
    collect_actions(full, "", &mut expected);
    let mut lean_actions = BTreeMap::new();
    collect_actions(lean, "", &mut lean_actions);
    assert!(!expected.is_empty(), "the fixture must produce actions");
    assert_ne!(
        lean_actions, expected,
        "the lean document must move repeated action text"
    );
    let shared = lean
        .pointer("/_meta/actions")
        .and_then(Value::as_object)
        .expect("lean document has _meta.actions");
    for defaults in shared.values() {
        for text in defaults
            .as_object()
            .expect("one object per action type")
            .values()
        {
            assert!(text.is_string(), "_meta.actions holds text only: {text}");
        }
    }
    assert_eq!(
        restore_actions(lean),
        expected,
        "every action must come back from the lean document plus _meta.actions"
    );
}

fn non_empty_array_keys(object: &Value) -> Vec<String> {
    object
        .as_object()
        .expect("JSON object")
        .iter()
        .filter(|(_, value)| value.as_array().is_some_and(|items| !items.is_empty()))
        .map(|(key, _)| key.clone())
        .collect()
}

fn assert_no_empty_arrays(object: &Value, label: &str) {
    for (key, value) in object.as_object().expect("JSON object") {
        assert!(
            value.as_array().is_none_or(|items| !items.is_empty()),
            "{label}.{key} is an empty array in a lean document"
        );
    }
}

// trim 1: `_meta.rules` under `--explain`

/// The rule codes whose result array holds a finding in `results`.
fn reported_rule_codes(results: &Value) -> Vec<String> {
    let mut codes: Vec<String> = fallow_output::issue_output_contracts()
        .filter(|contract| {
            results
                .get(contract.result_key)
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
        })
        .map(|contract| contract.code.to_string())
        .collect();
    codes.sort();
    codes
}

fn rule_codes(meta: &Value) -> Vec<String> {
    let mut codes: Vec<String> = meta
        .get("rules")
        .and_then(Value::as_object)
        .map(|rules| rules.keys().cloned().collect())
        .unwrap_or_default();
    codes.sort();
    codes
}

#[test]
fn explain_rules_list_only_rules_with_findings() {
    let document = dead_code_json(&["--explain"]);
    let expected = reported_rule_codes(&document);
    assert!(!expected.is_empty(), "basic-project reports findings");
    assert_eq!(
        rule_codes(&document["_meta"]),
        expected,
        "_meta.rules must hold only the codes of non-empty result arrays"
    );
}

#[test]
fn explain_rules_are_absent_when_nothing_is_reported() {
    let project = clean_fixture();
    let output = run_fallow_in_root(
        "dead-code",
        project.path(),
        &["--format", "json", "--quiet", "--explain"],
    );
    assert_eq!(output.code, 0, "clean project: {}", output.stdout);
    let document = parse(&output.stdout);
    assert_eq!(document["total_issues"], 0);
    assert!(
        document["_meta"].get("rules").is_none(),
        "a run without findings lists no rules: {}",
        document["_meta"]
    );
    assert!(
        document["_meta"].get("docs").is_some(),
        "the other explain keys stay"
    );
}

#[test]
fn explain_rules_follow_findings_in_grouped_output() {
    let output = run_fallow(
        "dead-code",
        "basic-project",
        &[
            "--format",
            "json",
            "--quiet",
            "--explain",
            "--group-by",
            "directory",
        ],
    );
    let document = parse(&output.stdout);
    let mut expected: Vec<String> = document["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .flat_map(reported_rule_codes)
        .collect();
    expected.sort();
    expected.dedup();
    assert!(!expected.is_empty());
    assert_eq!(rule_codes(&document["_meta"]), expected);
}

#[test]
fn explain_rules_follow_findings_in_combined_check_section() {
    let project = health_fixture();
    let document = combined_json(project.path(), &["--explain"]);
    let expected = reported_rule_codes(&document["check"]);
    assert!(!expected.is_empty(), "the fixture has unused exports");
    assert_eq!(rule_codes(&document["_meta"]["check"]), expected);
}

// `--lean`

#[test]
fn lean_requires_json_output() {
    let output = run_fallow("dead-code", "basic-project", &["--lean", "--quiet"]);
    assert_eq!(output.code, 2, "a presentation flag misuse exits 2");
    assert!(output.stdout.is_empty(), "usage errors do not use stdout");
    assert!(
        output.stderr.contains("--lean requires JSON output"),
        "error names the fix: {}",
        output.stderr
    );
}

#[test]
fn full_tables_requires_lean() {
    let output = run_fallow(
        "health",
        "basic-project",
        &["--format", "json", "--full-tables", "--quiet"],
    );
    assert_eq!(output.code, 2, "clap rejects --full-tables without --lean");
    // `--format json` writes the usage error as a JSON error document.
    let error: Value = serde_json::from_str(&output.stdout).expect("a JSON error document");
    assert_eq!(error["error"], true);
    assert_eq!(error["exit_code"], 2);
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("--lean")),
        "the error names --lean: {error}"
    );
}

#[test]
fn lean_is_rejected_on_commands_without_a_lean_envelope() {
    for command in ["dupes", "audit", "architecture"] {
        let output = run_fallow(
            command,
            "basic-project",
            &["--format", "json", "--lean", "--quiet"],
        );
        assert_eq!(output.code, 2, "{command} rejects --lean");
        assert!(
            output.stderr.contains("--lean applies to"),
            "{command} names where --lean applies: {}",
            output.stderr
        );
    }
}

#[test]
fn full_tables_and_top_are_rejected_together() {
    let project = health_fixture();
    let output = run_fallow_in_root(
        "health",
        project.path(),
        &[
            "--format",
            "json",
            "--lean",
            "--full-tables",
            "--top",
            "3",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 2,
        "--top cuts rows that --full-tables cannot restore"
    );
    assert!(output.stderr.contains("--full-tables"), "{}", output.stderr);
}

#[test]
fn report_refuses_a_lean_document() {
    let project = health_fixture();
    let saved = project.path().join("lean.json");
    let lean = run_fallow_in_root(
        "dead-code",
        project.path(),
        &["--format", "json", "--lean", "--quiet"],
    );
    std::fs::write(&saved, &lean.stdout).expect("write the lean document");
    let output = run_fallow_raw(&[
        "report",
        "--from",
        saved.to_str().expect("utf-8 path"),
        "--format",
        "markdown",
        "--quiet",
    ]);
    assert_eq!(output.code, 2, "a lean document is not a full run");
    let text = format!("{}{}", output.stdout, output.stderr);
    assert!(text.contains("--lean"), "the error names --lean: {text}");
}

#[test]
fn lean_dead_code_drops_empty_arrays_and_zero_counts() {
    let full = dead_code_json(&[]);
    let lean = dead_code_json(&["--lean"]);

    assert_eq!(lean["profile"], "lean", "a lean document says so");
    assert!(
        full.get("profile").is_none(),
        "a full document has no profile"
    );
    for key in [
        "kind",
        "schema_version",
        "version",
        "elapsed_ms",
        "total_issues",
    ] {
        assert!(lean.get(key).is_some(), "lean keeps {key}");
    }
    assert_eq!(lean["schema_version"], full["schema_version"]);
    assert_eq!(lean["total_issues"], full["total_issues"]);
    assert_no_empty_arrays(&lean, "root");
    assert_eq!(
        non_empty_array_keys(&lean)
            .into_iter()
            .filter(|key| key != "workspace_diagnostics")
            .collect::<Vec<_>>(),
        non_empty_array_keys(&full)
            .into_iter()
            .filter(|key| key != "workspace_diagnostics")
            .collect::<Vec<_>>(),
        "every finding array with findings stays"
    );
    for key in non_empty_array_keys(&full) {
        if key == "workspace_diagnostics" {
            continue;
        }
        assert_eq!(
            lean[&key].as_array().map(Vec::len),
            full[&key].as_array().map(Vec::len),
            "{key} keeps every finding"
        );
    }

    let summary = lean["summary"].as_object().expect("summary");
    assert!(!summary.is_empty(), "non-zero counts stay");
    for (key, count) in summary {
        assert_ne!(count, 0, "summary.{key} is zero in a lean document");
        assert_eq!(
            count, &full["summary"][key],
            "summary.{key} keeps its value"
        );
    }
    for (key, count) in full["summary"].as_object().expect("summary") {
        if count != 0 {
            assert_eq!(&lean["summary"][key], count, "summary.{key} stays");
        }
    }
}

#[test]
fn lean_groups_workspace_diagnostics_by_kind() {
    let full = dead_code_json(&[]);
    let lean = dead_code_json(&["--lean"]);
    let entries = full["workspace_diagnostics"]
        .as_array()
        .expect("basic-project has workspace diagnostics");
    let grouped = lean["workspace_diagnostics"]
        .as_array()
        .expect("lean keeps a diagnostics array");
    assert!(grouped.len() < entries.len(), "repeated kinds collapse");

    let mut kinds: Vec<&str> = Vec::new();
    for entry in entries {
        let kind = entry["kind"].as_str().expect("kind");
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    assert_eq!(
        grouped
            .iter()
            .map(|entry| entry["kind"].as_str().expect("kind"))
            .collect::<Vec<_>>(),
        kinds,
        "one entry per kind, in first-seen order"
    );
    for group in grouped {
        let kind = group["kind"].as_str().expect("kind");
        let same_kind: Vec<&Value> = entries.iter().filter(|e| e["kind"] == kind).collect();
        // An absent count means 1.
        let count = group.get("count").and_then(Value::as_u64).unwrap_or(1);
        assert_eq!(count, same_kind.len() as u64, "{kind} count");
        let mut first = same_kind[0].clone();
        let degrades = same_kind.iter().any(|e| e["degrades_analysis"] == true);
        if degrades {
            first["degrades_analysis"] = Value::Bool(true);
        }
        if same_kind.len() > 1 {
            first["count"] = Value::from(same_kind.len());
        }
        assert_eq!(group, &first, "{kind} keeps the first entry");
    }
}

#[test]
fn lean_dead_code_actions_are_lossless() {
    let full = dead_code_json(&[]);
    let lean = dead_code_json(&["--lean"]);
    assert_actions_restore(&full, &lean);
}

#[test]
fn lean_grouped_dead_code_drops_empty_group_arrays() {
    let args = ["--group-by", "directory"];
    let full = dead_code_json(&args);
    let mut lean_args = args.to_vec();
    lean_args.push("--lean");
    let lean = dead_code_json(&lean_args);
    assert_eq!(lean["kind"], "dead-code-grouped");
    assert_eq!(lean["profile"], "lean");
    let full_groups = full["groups"].as_array().expect("groups");
    let lean_groups = lean["groups"].as_array().expect("groups");
    assert_eq!(lean_groups.len(), full_groups.len());
    for (lean_group, full_group) in lean_groups.iter().zip(full_groups) {
        assert_no_empty_arrays(lean_group, "groups[]");
        assert_eq!(lean_group["key"], full_group["key"]);
        assert_eq!(lean_group["total_issues"], full_group["total_issues"]);
    }
    assert_actions_restore(&full, &lean);
}

#[test]
fn lean_health_caps_tables_and_counts_the_omitted_rows() {
    let project = health_fixture();
    let full = health_json(project.path(), &[]);
    let lean = health_json(project.path(), &["--lean"]);

    let full_scores = full["file_scores"].as_array().expect("file scores");
    assert_eq!(full_scores.len(), HEALTH_FIXTURE_FILES);
    let lean_scores = lean["file_scores"].as_array().expect("file scores");
    assert_eq!(lean_scores.len(), 10, "the default cap is 10 rows");
    assert_eq!(
        lean_scores.as_slice(),
        &full_scores[..10],
        "the cap keeps the first, most important rows"
    );
    assert_eq!(lean["file_scores_omitted"], HEALTH_FIXTURE_FILES - 10);
    assert_eq!(
        lean["findings"].as_array().map(Vec::len),
        full["findings"].as_array().map(Vec::len),
        "findings stay complete"
    );
    assert_eq!(lean["profile"], "lean");
    assert_eq!(lean["schema_version"], full["schema_version"]);
}

#[test]
fn lean_full_tables_keeps_every_row() {
    let project = health_fixture();
    let lean = health_json(project.path(), &["--lean", "--full-tables"]);
    assert_eq!(
        lean["file_scores"].as_array().map(Vec::len),
        Some(HEALTH_FIXTURE_FILES)
    );
    assert!(lean.get("file_scores_omitted").is_none());
}

#[test]
fn lean_health_top_sets_the_table_cap() {
    let project = health_fixture();
    let top = (HEALTH_FIXTURE_FILES - 2).to_string();
    let lean = health_json(project.path(), &["--lean", "--top", &top]);
    assert_eq!(
        lean["file_scores"].as_array().map(Vec::len),
        Some(HEALTH_FIXTURE_FILES - 2),
        "--top raises the cap above the default"
    );
    assert!(lean.get("file_scores_omitted").is_none());
}

#[test]
fn lean_health_actions_are_lossless() {
    let project = health_fixture();
    let full = health_json(project.path(), &[]);
    let lean = health_json(project.path(), &["--lean", "--full-tables"]);
    assert_actions_restore(&full, &lean);
    assert!(
        lean.pointer("/_meta/actions/refactor-function/note")
            .is_some(),
        "the constant refactor note moves to _meta.actions: {}",
        lean["_meta"]
    );
}

#[test]
fn lean_combined_trims_every_section() {
    let project = health_fixture();
    let full = combined_json(project.path(), &[]);
    let lean = combined_json(project.path(), &["--lean"]);

    assert_eq!(lean["kind"], "combined");
    assert_eq!(lean["profile"], "lean");
    assert_eq!(lean["schema_version"], full["schema_version"]);
    assert_no_empty_arrays(&lean, "root");
    for section in ["check", "dupes", "health"] {
        if let Some(value) = lean.get(section) {
            assert_no_empty_arrays(value, section);
        }
    }
    for (key, count) in lean["check"]["summary"].as_object().expect("summary") {
        assert_ne!(count, 0, "check.summary.{key} is zero in a lean document");
    }
    assert_eq!(
        lean["health"]["file_scores"].as_array().map(Vec::len),
        Some(10)
    );
    assert_eq!(
        lean["health"]["file_scores_omitted"],
        HEALTH_FIXTURE_FILES - 10
    );
    assert_eq!(lean["check"]["total_issues"], full["check"]["total_issues"]);

    let full_tables = combined_json(project.path(), &["--lean", "--full-tables"]);
    assert_actions_restore(&full, &full_tables);
}

#[test]
fn lean_pretty_indents_the_lean_document() {
    let compact = run_fallow(
        "dead-code",
        "basic-project",
        &["--format", "json", "--quiet", "--lean"],
    );
    let pretty = run_fallow(
        "dead-code",
        "basic-project",
        &["--format", "json", "--quiet", "--lean", "--pretty"],
    );
    assert_eq!(compact.stdout.lines().count(), 1);
    assert!(pretty.stdout.lines().count() > 1);
    let mut compact = parse(&compact.stdout);
    let mut pretty = parse(&pretty.stdout);
    for value in [&mut compact, &mut pretty] {
        let object = value.as_object_mut().expect("object");
        object.remove("elapsed_ms");
        if let Some(meta) = object.get_mut("_meta").and_then(Value::as_object_mut) {
            meta.remove("telemetry");
        }
    }
    assert_eq!(compact, pretty, "--pretty changes only the layout");
}

// schema

fn output_schema() -> Value {
    let path = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("../../docs/output-schema.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("read schema"))
        .expect("parse schema")
}

fn assert_valid(validator: &jsonschema::Validator, document: &Value, label: &str) {
    let errors: Vec<String> = validator
        .iter_errors(document)
        .take(10)
        .map(|err| format!("  at {} : {err}", err.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "{label} does not conform to docs/output-schema.json:\n{}",
        errors.join("\n")
    );
}

#[test]
fn lean_documents_conform_to_the_output_schema() {
    let schema = output_schema();
    let validator = jsonschema::draft7::new(&schema).expect("compile the output schema");
    let project = health_fixture();
    let documents = [
        ("dead-code", dead_code_json(&["--lean"])),
        (
            "dead-code-grouped",
            dead_code_json(&["--lean", "--group-by", "directory"]),
        ),
        ("health", health_json(project.path(), &["--lean"])),
        ("combined", combined_json(project.path(), &["--lean"])),
    ];
    let lean_only = serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$ref": "#/definitions/LeanOutput",
        "definitions": schema["definitions"],
    });
    let lean_validator = jsonschema::draft7::new(&lean_only).expect("compile LeanOutput");
    for (kind, document) in &documents {
        assert_eq!(document["kind"], *kind);
        assert_valid(&lean_validator, document, &format!("lean {kind}"));
        // `LeanOutput` is not a root branch, so the root `FallowJsonOutput`
        // union stays the same for typed consumers.
        assert!(
            !validator.is_valid(document),
            "a lean {kind} document must not pass as a full document"
        );
    }

    let full = dead_code_json(&[]);
    assert_valid(&validator, &full, "full dead-code");
    assert!(
        !lean_validator.is_valid(&full),
        "a full document must not pass as a lean document"
    );
}
