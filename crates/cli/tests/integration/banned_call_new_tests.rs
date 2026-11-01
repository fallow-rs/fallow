//! `banned-call` and `boundaries.calls.forbidden` read `new` expressions.
//!
//! A `new` site reports the subject string `new <path>`. The call form keeps
//! its own string, so a call finding keeps its id, fingerprint, baseline key
//! and audit key when a `new` site appears in the same file.

use std::path::Path;

use serde_json::Value;

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_in_root};

const CLOCK_RULE: &str = r#"{ "id": "no-clock", "kind": "banned-call", "callees": ["Date", "Date.now"], "message": "No wall clock." }"#;

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, body).expect("write file");
}

/// A project whose entry is `src/tick.js`, with one rule pack.
fn project(source: &str, rules: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "repro", "private": true, "type": "module", "main": "src/tick.js" }"#,
    );
    write(
        root,
        "pack.json",
        &format!(r#"{{ "version": 1, "name": "repro", "rules": [{rules}] }}"#),
    );
    write(
        root,
        ".fallowrc.json",
        r#"{ "rulePacks": ["pack.json"], "rules": { "policy-violation": "error" } }"#,
    );
    write(root, "src/tick.js", source);
    dir
}

fn architecture(root: &Path, args: &[&str]) -> CommandOutput {
    let mut all = vec!["--no-cache", "--quiet"];
    all.extend_from_slice(args);
    run_fallow_in_root("architecture", root, &all)
}

fn policy_json(root: &Path) -> Value {
    parse_json(&architecture(root, &["--policy", "--format", "json"]))
}

/// `(line, matched)` of each policy finding, in report order.
fn policy_lines(json: &Value) -> Vec<(u64, String)> {
    json["policy_violations"]
        .as_array()
        .expect("policy_violations")
        .iter()
        .map(|finding| {
            (
                finding["line"].as_u64().expect("line"),
                finding["matched"].as_str().expect("matched").to_owned(),
            )
        })
        .collect()
}

fn pair(line: u64, matched: &str) -> (u64, String) {
    (line, matched.to_owned())
}

#[test]
fn issue_repro_reports_the_new_expression() {
    let dir = project(
        "export const a = Date.now();\nexport const b = new Date();\nexport const c = Date();\n",
        CLOCK_RULE,
    );
    let output = architecture(dir.path(), &["--policy", "--format", "json"]);

    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert_eq!(
        policy_lines(&parse_json(&output)),
        vec![pair(1, "Date.now"), pair(2, "new Date"), pair(3, "Date")],
    );
}

#[test]
fn import_resolved_new_reports_every_import_style() {
    let rule =
        r#"{ "id": "no-three", "kind": "banned-call", "callees": ["three.Mesh", "three.Group"] }"#;
    let named = project(
        "import { Mesh } from \"three\";\nexport const m = new Mesh();\n",
        rule,
    );
    assert_eq!(
        policy_lines(&policy_json(named.path())),
        vec![pair(2, "new Mesh")]
    );

    let bare = project(
        "import { Mesh } from \"three\";\nexport const m = new Mesh;\n",
        rule,
    );
    assert_eq!(
        policy_lines(&policy_json(bare.path())),
        vec![pair(2, "new Mesh")]
    );

    let namespace = project(
        "import * as T from \"three\";\nexport const m = new T.Mesh();\nexport const g = new (T.Group)();\n",
        rule,
    );
    assert_eq!(
        policy_lines(&policy_json(namespace.path())),
        vec![pair(2, "new T.Mesh"), pair(3, "new T.Group")]
    );
}

#[test]
fn wildcard_pattern_reports_new_of_a_package_class() {
    let dir = project(
        "import { Client } from \"pg\";\nexport const c = new Client();\n",
        r#"{ "id": "no-pg", "kind": "banned-call", "callees": ["pg.*"] }"#,
    );
    let json = policy_json(dir.path());
    assert_eq!(policy_lines(&json), vec![pair(2, "new Client")]);
}

#[test]
fn matching_stays_segment_aware_for_new() {
    let dir = project(
        "export const a = new Date.Foo();\nexport const b = new Dates();\nexport const c = new Date();\n",
        r#"{ "id": "no-date", "kind": "banned-call", "callees": ["Date"] }"#,
    );
    // Line 3 is the positive control: the rule does report a true match.
    assert_eq!(
        policy_lines(&policy_json(dir.path())),
        vec![pair(3, "new Date")]
    );
}

#[test]
fn local_class_matches_a_bare_pattern_the_same_as_the_call_form() {
    let dir = project(
        "class Worker {}\nexport const w = new Worker();\nfunction fetch() {}\nexport const f = fetch();\n",
        r#"{ "id": "no-io", "kind": "banned-call", "callees": ["Worker", "fetch"] }"#,
    );
    assert_eq!(
        policy_lines(&policy_json(dir.path())),
        vec![pair(2, "new Worker"), pair(4, "fetch")]
    );
}

#[test]
fn a_pattern_that_holds_the_new_prefix_loads_and_matches_nothing() {
    let dir = project(
        "export const a = new Date();\nexport const b = Date();\nexport const c = Math.random();\n",
        r#"{ "id": "literal", "kind": "banned-call", "callees": ["new Date", "Math.random"] }"#,
    );
    // Line 3 is the positive control: the pack loaded and the rule ran.
    assert_eq!(
        policy_lines(&policy_json(dir.path())),
        vec![pair(3, "Math.random")]
    );
}

#[test]
fn shipped_template_reports_new_function_under_the_call_rule_only() {
    let dir = project("export const a = new Function(\"return 1\");\n", CLOCK_RULE);
    let init = run_fallow_in_root(
        "rule-pack",
        dir.path(),
        &[
            "init",
            "safe",
            "--template",
            "ai-safe-repo",
            "--no-config",
            "--dir",
            ".",
        ],
    );
    assert_eq!(
        init.code, 0,
        "stdout: {} stderr: {}",
        init.stdout, init.stderr
    );
    write(
        dir.path(),
        ".fallowrc.json",
        r#"{ "rulePacks": ["safe.jsonc"], "rules": { "policy-violation": "error" } }"#,
    );
    let json = policy_json(dir.path());
    let found: Vec<(String, String)> = json["policy_violations"]
        .as_array()
        .expect("policy_violations")
        .iter()
        .map(|f| {
            (
                f["rule_id"].as_str().expect("rule_id").to_owned(),
                f["matched"].as_str().expect("matched").to_owned(),
            )
        })
        .collect();
    // The `no-shell` rule of the template is a `banned-effect` rule, so it must
    // not read the new expression.
    assert_eq!(
        found,
        vec![("no-dynamic-code".to_owned(), "new Function".to_owned())]
    );
}

#[test]
fn banned_effect_output_does_not_change_for_new_expressions() {
    let dir = project(
        "export const a = new RegExp(\"x\");\nexport const b = new Array(3);\nexport const c = new Function(\"x\");\nexport const d = RegExp(\"y\");\n",
        r#"{ "id": "no-process", "kind": "banned-effect", "effects": ["process"] }"#,
    );
    // Line 4 is the positive control: the call form still reports.
    assert_eq!(
        policy_lines(&policy_json(dir.path())),
        vec![pair(4, "process: RegExp")]
    );
}

#[test]
fn suppression_on_the_call_line_covers_that_finding_only() {
    let dir = project(
        "export const a = new Date();\n// fallow-ignore-next-line policy-violation\nexport const b = Date();\n",
        CLOCK_RULE,
    );
    assert_eq!(
        policy_lines(&policy_json(dir.path())),
        vec![pair(1, "new Date")]
    );
}

#[test]
fn one_line_with_both_forms_gives_two_findings_and_one_comment_covers_both() {
    let plain = project("export const a = new Date(Date());\n", CLOCK_RULE);
    assert_eq!(
        policy_lines(&policy_json(plain.path())),
        vec![pair(1, "new Date"), pair(1, "Date")]
    );

    let suppressed = project(
        "// fallow-ignore-next-line policy-violation\nexport const a = new Date(Date());\nexport const b = Date.now();\n",
        CLOCK_RULE,
    );
    // Line 3 is the positive control: the comment covers only the next line.
    assert_eq!(
        policy_lines(&policy_json(suppressed.path())),
        vec![pair(3, "Date.now")]
    );
}

/// The fingerprints of the SARIF result on `line`.
fn sarif_fingerprints(root: &Path, line: u64) -> Value {
    let json = parse_json(&architecture(root, &["--policy", "--format", "sarif"]));
    json["runs"][0]["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|result| result["locations"][0]["physicalLocation"]["region"]["startLine"] == line)
        .unwrap_or_else(|| panic!("no SARIF result on line {line}: {json}"))["partialFingerprints"]
        .clone()
}

/// The fingerprint of the CodeClimate entry on `line`.
fn codeclimate_fingerprint(root: &Path, line: u64) -> Value {
    let json = parse_json(&architecture(
        root,
        &["--policy", "--format", "codeclimate"],
    ));
    json.as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["location"]["lines"]["begin"] == line)
        .unwrap_or_else(|| panic!("no CodeClimate entry on line {line}: {json}"))["fingerprint"]
        .clone()
}

fn finding_id(root: &Path, matched: &str) -> String {
    let json = policy_json(root);
    json["policy_violations"]
        .as_array()
        .expect("policy_violations")
        .iter()
        .find(|finding| finding["matched"] == matched)
        .unwrap_or_else(|| panic!("no finding `{matched}`: {json}"))["finding_id"]
        .as_str()
        .expect("finding_id")
        .to_owned()
}

/// `new Date()` above `Date()` must not move any identity of the `Date()`
/// finding. The line number is not part of an identity.
const CALL_ONLY: &str = "export const c = Date();\n";
const NEW_ABOVE_CALL: &str = "export const b = new Date();\nexport const c = Date();\n";

#[test]
fn new_above_call_keeps_the_call_finding_id_and_fingerprints() {
    let before = project(CALL_ONLY, CLOCK_RULE);
    let after = project(NEW_ABOVE_CALL, CLOCK_RULE);

    let call_id = finding_id(before.path(), "Date");
    assert_eq!(finding_id(after.path(), "Date"), call_id);
    assert!(
        !call_id.contains('~'),
        "the call finding must keep the base id: {call_id}"
    );
    assert_ne!(finding_id(after.path(), "new Date"), call_id);

    assert_eq!(
        sarif_fingerprints(after.path(), 2),
        sarif_fingerprints(before.path(), 1),
    );
    assert_eq!(
        codeclimate_fingerprint(after.path(), 2),
        codeclimate_fingerprint(before.path(), 1),
    );
}

#[test]
fn saved_baseline_keeps_covering_the_call_and_does_not_cover_the_new_form() {
    let dir = project(CALL_ONLY, CLOCK_RULE);
    let root = dir.path();
    let baseline = root.join("baseline.json");
    let baseline = baseline.to_str().expect("baseline path");
    let saved = architecture(
        root,
        &["--policy", "--format", "json", "--save-baseline", baseline],
    );
    assert!(
        saved.code == 0 || saved.code == 1,
        "stderr: {}",
        saved.stderr
    );

    write(root, "src/tick.js", NEW_ABOVE_CALL);
    let output = architecture(
        root,
        &["--policy", "--format", "json", "--baseline", baseline],
    );

    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert_eq!(
        policy_lines(&parse_json(&output)),
        vec![pair(1, "new Date")],
        "the baseline entry must cover the call line, not the new line"
    );
}

#[test]
fn audit_attributes_the_call_finding_to_the_base_and_only_new_to_the_head() {
    let dir = project(CALL_ONLY, CLOCK_RULE);
    let root = dir.path();
    git(root, &["init", "--initial-branch=main", "--quiet"]);
    commit_all(root, "base");
    write(root, "src/tick.js", NEW_ABOVE_CALL);
    commit_all(root, "head");

    let output = run_fallow_in_root(
        "audit",
        root,
        &[
            "--base",
            "HEAD~1",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    let json = parse_json(&output);
    let findings = json["dead_code"]["policy_violations"]
        .as_array()
        .unwrap_or_else(|| panic!("no policy_violations: {json}"));
    let introduced: Vec<(String, bool)> = findings
        .iter()
        .map(|f| {
            (
                f["matched"].as_str().expect("matched").to_owned(),
                f["introduced"].as_bool().expect("introduced"),
            )
        })
        .collect();
    assert_eq!(
        introduced,
        vec![("new Date".to_owned(), true), ("Date".to_owned(), false)],
    );
}

/// A project with one zone `d` that forbids the callee pattern `Date`.
fn boundary_project(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "repro", "private": true, "type": "module", "main": "src/a.js" }"#,
    );
    write(
        root,
        ".fallowrc.json",
        r#"{
  "rules": { "boundary-violation": "error" },
  "boundaries": {
    "zones": [{ "name": "d", "patterns": ["src/**"] }],
    "rules": [],
    "calls": { "forbidden": [{ "from": "d", "callee": "Date" }] }
  }
}"#,
    );
    write(root, "src/a.js", source);
    dir
}

#[test]
fn forbidden_call_reports_new_with_a_separate_callee_string() {
    let dir = boundary_project(NEW_ABOVE_CALL);
    let output = architecture(dir.path(), &["--boundaries", "--format", "json"]);
    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let found: Vec<(u64, &str, &str)> = json["boundary_call_violations"]
        .as_array()
        .expect("boundary_call_violations")
        .iter()
        .map(|f| {
            (
                f["line"].as_u64().expect("line"),
                f["callee"].as_str().expect("callee"),
                f["pattern"].as_str().expect("pattern"),
            )
        })
        .collect();
    assert_eq!(found, vec![(1, "new Date", "Date"), (2, "Date", "Date")]);

    let human = run_fallow_in_root("architecture", dir.path(), &["--no-cache", "--boundaries"]);
    assert!(
        human
            .stdout
            .contains("src/a.js:1 new Date matches forbidden `Date` in zone 'd'"),
        "stdout: {}",
        human.stdout
    );
}

#[test]
fn forbidden_call_finding_id_does_not_move_when_new_appears_above() {
    // Find the call finding by its line. A find by callee takes the first
    // finding, so a shared subject string would hide a moved id.
    let id = |source: &str, line: u64| -> String {
        let dir = boundary_project(source);
        let json = parse_json(&architecture(
            dir.path(),
            &["--boundaries", "--format", "json"],
        ));
        json["boundary_call_violations"]
            .as_array()
            .expect("boundary_call_violations")
            .iter()
            .find(|f| f["line"] == line)
            .unwrap_or_else(|| panic!("no finding on line {line}: {json}"))["finding_id"]
            .as_str()
            .expect("finding_id")
            .to_owned()
    };
    assert_eq!(id(NEW_ABOVE_CALL, 2), id(CALL_ONLY, 1));
}

#[test]
fn rule_pack_test_previews_the_new_finding() {
    let dir = project(NEW_ABOVE_CALL, CLOCK_RULE);
    let output = run_fallow_in_root(
        "rule-pack",
        dir.path(),
        &["test", "pack.json", "--format", "json"],
    );
    let json = parse_json(&output);
    let matched: Vec<&str> = json["findings"]
        .as_array()
        .unwrap_or_else(|| panic!("no findings: {json}"))
        .iter()
        .map(|f| f["matched"].as_str().expect("matched"))
        .collect();
    assert_eq!(matched, vec!["new Date", "Date"]);
}
