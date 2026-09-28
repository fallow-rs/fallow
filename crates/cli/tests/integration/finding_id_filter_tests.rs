//! `fallow dead-code --finding-id`: report only the requested findings, and
//! tell a consumer whether a missing id is absent or only not visible.
//!
//! A consumer that stores a verdict per finding id asks "does this finding
//! still exist?". The answer "missing" is safe to read as "resolved" only when
//! `finding_id_query.conclusive` is true. A scoped run, a baseline or a
//! changed-file filter can hide a finding that still exists, so those runs
//! must never be conclusive.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::common::{commit_all, copy_fixture, git, parse_json, run_fallow_raw};

const BASIC: &str = "finding-ids-basic";
const UNKNOWN_ID: &str = "dc1:unused-export:0000000000000000";

/// The dead-code arrays the basic fixture reports.
const ARRAYS: &[&str] = &[
    "unused_files",
    "unused_exports",
    "unused_types",
    "unused_dependencies",
    "unused_enum_members",
    "unused_class_members",
    "stale_suppressions",
];

struct Run {
    code: i32,
    json: Value,
}

fn dead_code(root: &Path, extra: &[&str]) -> Run {
    let mut args = vec![
        "dead-code",
        "--root",
        root.to_str().expect("temp path is UTF-8"),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_raw(&args);
    assert!(
        output.code == 0 || output.code == 1,
        "dead-code failed with {}: {}",
        output.code,
        output.stderr
    );
    Run {
        code: output.code,
        json: parse_json(&output),
    }
}

/// Every reported finding, keyed by id.
fn findings_by_id(json: &Value) -> Vec<(String, Value)> {
    let mut findings = Vec::new();
    for array in ARRAYS {
        for item in json[*array].as_array().into_iter().flatten() {
            let id = item["finding_id"]
                .as_str()
                .unwrap_or_else(|| panic!("finding without finding_id: {item}"))
                .to_owned();
            findings.push((id, item.clone()));
        }
    }
    findings
}

fn reported_ids(json: &Value) -> BTreeSet<String> {
    findings_by_id(json).into_iter().map(|(id, _)| id).collect()
}

fn id_of(json: &Value, array: &str, field: &str, name: &str) -> String {
    json[array]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item.get(field).and_then(Value::as_str) == Some(name))
        .and_then(|item| item["finding_id"].as_str())
        .unwrap_or_else(|| panic!("no {array} finding with {field} = {name}"))
        .to_owned()
}

fn tie_id(json: &Value) -> String {
    findings_by_id(json)
        .into_iter()
        .map(|(id, _)| id)
        .find(|id| id.contains('~'))
        .unwrap_or_else(|| panic!("fixture reports no tie id: {json}"))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("not an array: {value}"))
        .iter()
        .map(|item| item.as_str().expect("string").to_owned())
        .collect()
}

fn query(json: &Value) -> &Value {
    json.get("finding_id_query")
        .unwrap_or_else(|| panic!("no finding_id_query: {json}"))
}

fn reasons(json: &Value) -> Vec<String> {
    strings(&query(json)["inconclusive_reasons"])
}

#[test]
fn the_filter_reports_exactly_the_requested_ids() {
    let dir = copy_fixture(BASIC);
    let full = dead_code(dir.path(), &[]).json;
    let export = id_of(&full, "unused_exports", "export_name", "helper");
    let tie = tie_id(&full);

    let run = dead_code(dir.path(), &["--finding-id", &export, "--finding-id", &tie]);

    let expected: BTreeSet<String> = [export.clone(), tie.clone()].into_iter().collect();
    assert_eq!(reported_ids(&run.json), expected);
    assert_eq!(run.json["total_issues"], 2);
    let query = query(&run.json);
    assert_eq!(
        strings(&query["requested"]),
        vec![export.clone(), tie.clone()]
    );
    assert_eq!(strings(&query["found"]), vec![export, tie]);
    assert!(strings(&query["missing"]).is_empty());
    assert_eq!(query["conclusive"], true);
    assert!(reasons(&run.json).is_empty());
    assert_eq!(run.code, 1, "an error-severity finding was reported");
}

#[test]
fn the_filter_accepts_a_comma_separated_list() {
    let dir = copy_fixture(BASIC);
    let full = dead_code(dir.path(), &[]).json;
    let export = id_of(&full, "unused_exports", "export_name", "helper");
    let file = id_of(&full, "unused_files", "path", "src/orphan.ts");

    let run = dead_code(dir.path(), &["--finding-id", &format!("{export},{file}")]);

    let expected: BTreeSet<String> = [export, file].into_iter().collect();
    assert_eq!(reported_ids(&run.json), expected);
}

#[test]
fn a_filtered_finding_is_equal_to_the_same_finding_in_the_full_run() {
    let dir = copy_fixture(BASIC);
    let full = dead_code(dir.path(), &[]).json;
    let tie = tie_id(&full);

    let run = dead_code(dir.path(), &["--finding-id", &tie]);

    let full_item = findings_by_id(&full)
        .into_iter()
        .find(|(id, _)| *id == tie)
        .map(|(_, item)| item);
    let filtered_item = findings_by_id(&run.json)
        .into_iter()
        .find(|(id, _)| *id == tie)
        .map(|(_, item)| item);
    assert!(full_item.is_some());
    assert_eq!(filtered_item, full_item);
}

#[test]
fn an_unknown_id_is_missing_and_conclusive_on_a_full_run() {
    let dir = copy_fixture(BASIC);

    let run = dead_code(dir.path(), &["--finding-id", UNKNOWN_ID]);

    assert!(reported_ids(&run.json).is_empty());
    assert_eq!(run.json["total_issues"], 0);
    let query = query(&run.json);
    assert!(strings(&query["found"]).is_empty());
    assert_eq!(strings(&query["missing"]), vec![UNKNOWN_ID.to_owned()]);
    assert!(strings(&query["filtered"]).is_empty());
    assert_eq!(query["conclusive"], true);
    assert_eq!(run.code, 0, "nothing was reported");
}

#[test]
fn a_fixed_finding_is_missing_and_conclusive() {
    let dir = copy_fixture(BASIC);
    let full = dead_code(dir.path(), &[]).json;
    let helper = id_of(&full, "unused_exports", "export_name", "helper");
    let unused_fn = id_of(&full, "unused_exports", "export_name", "unusedFn");
    let utils = dir.path().join("src/utils.ts");
    let source = std::fs::read_to_string(&utils).expect("read utils");
    std::fs::write(
        &utils,
        source.replace("export const helper", "const helper"),
    )
    .expect("write utils");

    let run = dead_code(
        dir.path(),
        &["--finding-id", &helper, "--finding-id", &unused_fn],
    );

    let query = query(&run.json);
    assert_eq!(strings(&query["found"]), vec![unused_fn]);
    assert_eq!(strings(&query["missing"]), vec![helper]);
    assert_eq!(query["conclusive"], true);
}

#[test]
fn a_scope_that_excludes_a_finding_is_not_conclusive() {
    let dir = copy_fixture(BASIC);
    let full = dead_code(dir.path(), &[]).json;
    let helper = id_of(&full, "unused_exports", "export_name", "helper");

    let run = dead_code(dir.path(), &["src/orphan.ts", "--finding-id", &helper]);

    let query = query(&run.json);
    assert_eq!(strings(&query["missing"]), vec![helper.clone()]);
    assert_eq!(strings(&query["filtered"]), vec![helper]);
    assert_eq!(query["conclusive"], false);
    assert!(reasons(&run.json).contains(&"scope".to_owned()));
}

#[test]
fn a_scoped_run_is_never_conclusive() {
    let dir = copy_fixture(BASIC);

    let run = dead_code(dir.path(), &["src", "--finding-id", UNKNOWN_ID]);

    assert_eq!(query(&run.json)["conclusive"], false);
    assert_eq!(reasons(&run.json), vec!["scope".to_owned()]);
}

#[test]
fn a_baseline_run_is_not_conclusive() {
    let dir = copy_fixture(BASIC);
    let full = dead_code(dir.path(), &[]).json;
    let helper = id_of(&full, "unused_exports", "export_name", "helper");
    let baseline = dir.path().join("fallow-baseline.json");
    let baseline_arg = baseline.to_str().expect("temp path is UTF-8");
    dead_code(dir.path(), &["--save-baseline", baseline_arg]);

    let run = dead_code(
        dir.path(),
        &["--baseline", baseline_arg, "--finding-id", &helper],
    );

    let query = query(&run.json);
    assert_eq!(strings(&query["missing"]), vec![helper.clone()]);
    assert_eq!(strings(&query["filtered"]), vec![helper]);
    assert_eq!(query["conclusive"], false);
    assert!(reasons(&run.json).contains(&"baseline".to_owned()));
    assert_eq!(run.code, 0);
}

#[test]
fn a_changed_since_run_is_not_conclusive() {
    let dir = copy_fixture(BASIC);
    let full = dead_code(dir.path(), &[]).json;
    let helper = id_of(&full, "unused_exports", "export_name", "helper");
    git(dir.path(), &["init", "-b", "main"]);
    commit_all(dir.path(), "initial");

    let run = dead_code(
        dir.path(),
        &["--changed-since", "HEAD", "--finding-id", &helper],
    );

    let query = query(&run.json);
    assert_eq!(strings(&query["missing"]), vec![helper]);
    assert_eq!(query["conclusive"], false);
    assert!(reasons(&run.json).contains(&"changed-since".to_owned()));
}

#[test]
fn an_issue_type_filter_is_not_conclusive() {
    let dir = copy_fixture(BASIC);
    let full = dead_code(dir.path(), &[]).json;
    let helper = id_of(&full, "unused_exports", "export_name", "helper");

    let run = dead_code(dir.path(), &["--unused-files", "--finding-id", &helper]);

    assert_eq!(query(&run.json)["conclusive"], false);
    assert!(reasons(&run.json).contains(&"issue-type-filter".to_owned()));
}

#[test]
fn a_rule_set_to_off_is_not_conclusive() {
    let dir = copy_fixture(BASIC);
    let full = dead_code(dir.path(), &[]).json;
    let helper = id_of(&full, "unused_exports", "export_name", "helper");
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{ "rules": { "unused-exports": "off" } }"#,
    )
    .expect("write config");

    let run = dead_code(dir.path(), &["--finding-id", &helper]);

    let query = query(&run.json);
    assert_eq!(strings(&query["missing"]), vec![helper]);
    assert_eq!(query["conclusive"], false);
    assert_eq!(reasons(&run.json), vec!["rule-off".to_owned()]);
}

#[test]
fn a_rule_set_to_off_in_an_override_is_not_conclusive() {
    let dir = copy_fixture(BASIC);
    let full = dead_code(dir.path(), &[]).json;
    let helper = id_of(&full, "unused_exports", "export_name", "helper");
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{ "overrides": [{ "files": ["src/utils.ts"], "rules": { "unused-exports": "off" } }] }"#,
    )
    .expect("write config");

    let run = dead_code(dir.path(), &["--finding-id", &helper]);

    let query = query(&run.json);
    assert_eq!(strings(&query["missing"]), vec![helper]);
    assert_eq!(query["conclusive"], false);
    assert!(reasons(&run.json).contains(&"rule-off".to_owned()));
}

#[test]
fn a_rule_that_is_off_for_another_issue_type_keeps_the_query_conclusive() {
    let dir = copy_fixture(BASIC);
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{ "rules": { "unused-files": "off" } }"#,
    )
    .expect("write config");

    let run = dead_code(dir.path(), &["--finding-id", UNKNOWN_ID]);

    assert_eq!(query(&run.json)["conclusive"], true);
}

#[test]
fn include_entry_exports_is_not_conclusive() {
    let dir = copy_fixture(BASIC);

    let run = dead_code(
        dir.path(),
        &["--include-entry-exports", "--finding-id", UNKNOWN_ID],
    );

    assert_eq!(query(&run.json)["conclusive"], false);
    assert!(reasons(&run.json).contains(&"include-entry-exports".to_owned()));
}

fn fingerprint(root: &Path, extra: &[&str]) -> String {
    let mut args = vec!["--finding-id", UNKNOWN_ID];
    args.extend_from_slice(extra);
    let run = dead_code(root, &args);
    query(&run.json)["analysis_fingerprint"]
        .as_str()
        .unwrap_or_else(|| panic!("no analysis_fingerprint: {}", run.json))
        .to_owned()
}

#[test]
fn the_fingerprint_is_stable_for_the_same_inputs() {
    let first = copy_fixture(BASIC);
    let second = copy_fixture(BASIC);

    let value = fingerprint(first.path(), &[]);

    assert!(value.starts_with("af1:"), "{value}");
    assert_eq!(value, fingerprint(first.path(), &[]));
    assert_eq!(
        value,
        fingerprint(second.path(), &[]),
        "the checkout path is not an input"
    );
}

#[test]
fn a_source_edit_keeps_the_fingerprint() {
    let dir = copy_fixture(BASIC);
    let before = fingerprint(dir.path(), &[]);
    let utils = dir.path().join("src/utils.ts");
    let source = std::fs::read_to_string(&utils).expect("read utils");
    std::fs::write(
        &utils,
        source.replace("export const helper", "const helper"),
    )
    .expect("write utils");

    assert_eq!(fingerprint(dir.path(), &[]), before);
}

#[test]
fn config_and_ignore_changes_change_the_fingerprint() {
    let dir = copy_fixture(BASIC);
    let base = fingerprint(dir.path(), &[]);

    assert_ne!(
        fingerprint(dir.path(), &["--include-entry-exports"]),
        base,
        "--include-entry-exports"
    );

    std::fs::write(dir.path().join(".gitignore"), "src/orphan.ts\n").expect("write gitignore");
    let with_gitignore = fingerprint(dir.path(), &[]);
    assert_ne!(with_gitignore, base, "a new .gitignore");
    std::fs::write(dir.path().join(".gitignore"), "src/lib.ts\n").expect("edit gitignore");
    assert_ne!(
        fingerprint(dir.path(), &[]),
        with_gitignore,
        "a .gitignore edit"
    );
    std::fs::remove_file(dir.path().join(".gitignore")).expect("remove gitignore");
    assert_eq!(
        fingerprint(dir.path(), &[]),
        base,
        "back to the same inputs"
    );

    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{ "ignorePatterns": ["src/orphan.ts"] }"#,
    )
    .expect("write config");
    let with_patterns = fingerprint(dir.path(), &[]);
    assert_ne!(with_patterns, base, "ignorePatterns");
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{ "ignorePatterns": ["src/lib.ts"] }"#,
    )
    .expect("edit config");
    assert_ne!(
        fingerprint(dir.path(), &[]),
        with_patterns,
        "an ignorePatterns change"
    );
}

#[test]
fn a_run_without_the_flag_has_no_query_field() {
    let dir = copy_fixture(BASIC);

    let run = dead_code(dir.path(), &[]);

    assert!(run.json.get("finding_id_query").is_none());
}

#[test]
fn a_malformed_id_is_refused() {
    let dir = copy_fixture(BASIC);
    for bad in [
        "unused-export:helper",
        "dc1:unused-export:XYZ",
        "dc2:unused-export:0000000000000000",
        "dc1:unused-export:0000000000000000~",
        "",
    ] {
        let output = run_fallow_raw(&[
            "dead-code",
            "--root",
            dir.path().to_str().expect("temp path is UTF-8"),
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--finding-id",
            bad,
        ]);
        assert_eq!(output.code, 2, "{bad:?} was accepted: {}", output.stdout);
    }
}
