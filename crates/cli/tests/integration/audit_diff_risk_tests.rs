//! End-to-end tests of the diff-risk rules of `fallow audit`: weakened tests,
//! added suppressions, test-only branches, diverged clone siblings and added
//! dependencies.

use std::fs;
use std::path::Path;

use crate::common::{commit_all, git, parse_json, run_fallow_raw};
use tempfile::TempDir;

/// A small project with one source file, one test file and a committed base.
fn fixture(config: Option<&str>) -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "diff-risk", "main": "src/index.ts", "dependencies": {}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "export const price = (n: number): number => n * 2;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.test.ts"),
        "import { price } from './index';\n\nit('doubles', () => {\n  expect(price(1)).toBe(2);\n});\n\nit('keeps zero', () => {\n  expect(price(0)).toBe(0);\n});\n",
    )
    .unwrap();
    if let Some(config) = config {
        fs::write(dir.join(".fallowrc.json"), config).unwrap();
    }
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "base");
    tmp
}

fn audit_json(dir: &Path, extra: &[&str]) -> (i32, serde_json::Value) {
    let root = dir.to_str().unwrap();
    let mut args = vec!["audit", "--root", root, "--base", "main", "--quiet"];
    args.extend_from_slice(extra);
    if !extra.contains(&"--format") {
        args.extend_from_slice(&["--format", "json"]);
    }
    let output = run_fallow_raw(&args);
    let json = parse_json(&output);
    (output.code, json)
}

fn findings(json: &serde_json::Value) -> Vec<serde_json::Value> {
    json.pointer("/diff_risk/findings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn rule_lines(json: &serde_json::Value) -> Vec<(String, Option<String>, String, Option<u64>)> {
    findings(json)
        .iter()
        .map(|finding| {
            (
                finding["rule"].as_str().unwrap().to_owned(),
                finding["kind"].as_str().map(str::to_owned),
                finding["path"].as_str().unwrap().to_owned(),
                finding["line"].as_u64(),
            )
        })
        .collect()
}

fn rule(
    rule: &str,
    kind: Option<&str>,
    path: &str,
    line: Option<u64>,
) -> (String, Option<String>, String, Option<u64>) {
    (
        rule.to_owned(),
        kind.map(str::to_owned),
        path.to_owned(),
        line,
    )
}

#[test]
fn skip_marker_suppression_and_test_branch_are_introduced_warnings() {
    let tmp = fixture(None);
    let dir = tmp.path();
    fs::write(
        dir.join("src/index.ts"),
        "// @ts-ignore\nexport const price = (n: number): number => {\n  if (process.env.NODE_ENV === 'test') return 0;\n  return n * 2;\n};\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.test.ts"),
        "import { price } from './index';\n\nit.skip('doubles', () => {\n  expect(price(1)).toBe(2);\n});\n\nit('keeps zero', () => {\n  expect(price(0)).toBe(0);\n});\n",
    )
    .unwrap();

    let (code, json) = audit_json(dir, &[]);
    assert_eq!(code, 0, "warn findings keep exit 0: {json}");
    assert_eq!(json["verdict"], "warn");
    assert_eq!(
        rule_lines(&json),
        vec![
            rule(
                "weakened-tests",
                Some("skip-only"),
                "src/index.test.ts",
                Some(3)
            ),
            rule("added-suppression", None, "src/index.ts", Some(1)),
            rule("test-only-branch", None, "src/index.ts", Some(3)),
        ]
    );
    for finding in findings(&json) {
        assert_eq!(finding["introduced"], true);
        assert_eq!(finding["severity"], "warn");
        let action_types: Vec<&str> = finding["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|action| action["type"].as_str().unwrap())
            .collect();
        assert_eq!(action_types, vec!["suppress-line", "suppress-file"]);
    }
    assert_eq!(json["attribution"]["diff_risk_introduced"], 3);
}

#[test]
fn error_severity_fails_and_off_hides() {
    let tmp = fixture(Some(
        r#"{"rules": {"weakened-tests": "error", "test-only-branch": "off"}}"#,
    ));
    let dir = tmp.path();
    fs::write(
        dir.join("src/index.ts"),
        "export const price = (n: number): number => (process.env.VITEST ? 0 : n * 2);\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.test.ts"),
        "import { price } from './index';\n\nit.only('doubles', () => {\n  expect(price(1)).toBe(2);\n});\n\nit('keeps zero', () => {\n  expect(price(0)).toBe(0);\n});\n",
    )
    .unwrap();

    let (code, json) = audit_json(dir, &[]);
    assert_eq!(code, 1, "an error-severity diff-risk finding fails: {json}");
    assert_eq!(json["verdict"], "fail");
    assert_eq!(
        rule_lines(&json),
        vec![rule(
            "weakened-tests",
            Some("skip-only"),
            "src/index.test.ts",
            Some(3)
        )]
    );
    assert_eq!(findings(&json)[0]["severity"], "error");
}

#[test]
fn named_suppression_comment_hides_the_finding() {
    let tmp = fixture(None);
    let dir = tmp.path();
    fs::write(
        dir.join("src/index.test.ts"),
        "import { price } from './index';\n\n// fallow-ignore-next-line weakened-tests\nit.skip('doubles', () => {\n  expect(price(1)).toBe(2);\n});\n\nit('keeps zero', () => {\n  expect(price(0)).toBe(0);\n});\n",
    )
    .unwrap();

    let (_, json) = audit_json(dir, &[]);
    assert!(findings(&json).is_empty(), "suppressed: {json}");
    assert!(json.get("diff_risk").is_none(), "empty section is omitted");
    assert_eq!(json["attribution"]["diff_risk_introduced"], 0);
}

#[test]
fn moved_tests_do_not_report_but_a_deleted_test_file_does() {
    let tmp = fixture(None);
    let dir = tmp.path();
    // Move one test case into a new file: the total stays the same.
    fs::write(
        dir.join("src/index.test.ts"),
        "import { price } from './index';\n\nit('doubles', () => {\n  expect(price(1)).toBe(2);\n});\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/zero.test.ts"),
        "import { price } from './index';\n\nit('keeps zero', () => {\n  expect(price(0)).toBe(0);\n});\n",
    )
    .unwrap();
    let (_, json) = audit_json(dir, &[]);
    assert!(findings(&json).is_empty(), "moved tests: {json}");

    fs::remove_file(dir.join("src/zero.test.ts")).unwrap();
    fs::remove_file(dir.join("src/index.test.ts")).unwrap();
    let (_, json) = audit_json(dir, &[]);
    let found = findings(&json);
    assert_eq!(
        rule_lines(&json),
        vec![rule(
            "weakened-tests",
            Some("removed-tests"),
            "src/index.test.ts",
            None
        )]
    );
    assert_eq!(found[0]["evidence"], "test cases: 2 -> 0");
    assert_eq!(found[0]["actions"][0]["type"], "suppress-in-config");
}

#[test]
fn assertion_drop_and_lowered_threshold_report() {
    let tmp = fixture(None);
    let dir = tmp.path();
    fs::write(
        dir.join("vitest.config.ts"),
        "export default { test: { coverage: { thresholds: { lines: 90 } } } };\n",
    )
    .unwrap();
    commit_all(dir, "coverage config");
    fs::write(
        dir.join("vitest.config.ts"),
        "export default { test: { coverage: { thresholds: { lines: 60 } } } };\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.test.ts"),
        "import { price } from './index';\n\nit('doubles', () => {\n  price(1);\n});\n\nit('keeps zero', () => {\n  expect(price(0)).toBe(0);\n});\n",
    )
    .unwrap();

    let (_, json) = audit_json(dir, &[]);
    assert_eq!(
        rule_lines(&json),
        vec![
            rule(
                "weakened-tests",
                Some("assertion-drop"),
                "src/index.test.ts",
                None
            ),
            rule(
                "weakened-tests",
                Some("coverage-threshold"),
                "vitest.config.ts",
                Some(1)
            ),
        ]
    );
    assert_eq!(findings(&json)[1]["evidence"], "lines: 90 -> 60");
}

#[test]
fn added_dependency_reports_only_when_enabled() {
    let tmp = fixture(None);
    let dir = tmp.path();
    fs::write(
        dir.join("package.json"),
        "{\n  \"name\": \"diff-risk\",\n  \"main\": \"src/index.ts\",\n  \"dependencies\": {\n    \"left-pad\": \"^1.3.0\"\n  }\n}\n",
    )
    .unwrap();
    let (_, json) = audit_json(dir, &[]);
    assert!(
        !rule_lines(&json)
            .iter()
            .any(|row| row.0 == "added-dependency"),
        "off by default: {json}"
    );

    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"rules": {"added-dependency": "warn"}}"#,
    )
    .unwrap();
    let (_, json) = audit_json(dir, &[]);
    let found: Vec<serde_json::Value> = findings(&json)
        .into_iter()
        .filter(|finding| finding["rule"] == "added-dependency")
        .collect();
    assert_eq!(found.len(), 1, "{json}");
    assert_eq!(found[0]["path"], "package.json");
    assert_eq!(found[0]["line"], 5);
    assert_eq!(found[0]["actions"][0]["type"], "suppress-in-config");
}

const CLONE: &str = "export function total(items: { price: number; quantity: number }[]): number {\n  let sum = 0;\n  for (const item of items) {\n    if (item.quantity > 0) {\n      sum += item.price * item.quantity;\n    }\n  }\n  const rounded = Math.round(sum * 100) / 100;\n  if (rounded < 0) {\n    throw new Error('negative total');\n  }\n  return rounded;\n}\n";

#[test]
fn edited_clone_copy_with_unchanged_sibling_reports_diverged_clone_when_enabled() {
    let tmp = fixture(None);
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src/a")).unwrap();
    fs::create_dir_all(dir.join("src/b")).unwrap();
    fs::write(dir.join("src/a/total.ts"), CLONE).unwrap();
    fs::write(dir.join("src/b/total.ts"), CLONE).unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { total as a } from './a/total';\nimport { total as b } from './b/total';\nexport const price = (n: number): number => n * 2 + a([]) + b([]);\n",
    )
    .unwrap();
    commit_all(dir, "clones");

    let edited = CLONE.replace(
        "      sum += item.price * item.quantity;\n",
        "      sum += item.price * item.quantity * 0.9;\n      sum -= 1;\n",
    );
    fs::write(dir.join("src/a/total.ts"), edited).unwrap();

    let (_, json) = audit_json(dir, &[]);
    assert!(
        !rule_lines(&json)
            .iter()
            .any(|row| row.0 == "diverged-clone-sibling"),
        "off by default: {json}"
    );

    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"rules": {"diverged-clone-sibling": "warn"}}"#,
    )
    .unwrap();
    let (_, json) = audit_json(dir, &[]);
    let found: Vec<serde_json::Value> = findings(&json)
        .into_iter()
        .filter(|finding| finding["rule"] == "diverged-clone-sibling")
        .collect();
    assert_eq!(found.len(), 1, "{json}");
    assert_eq!(found[0]["path"], "src/a/total.ts");
    assert_eq!(found[0]["related"][0]["path"], "src/b/total.ts");
}

#[test]
fn sarif_carries_diff_risk_rule_ids() {
    let tmp = fixture(None);
    let dir = tmp.path();
    fs::write(
        dir.join("src/index.test.ts"),
        "import { price } from './index';\n\nxit('doubles', () => {\n  expect(price(1)).toBe(2);\n});\n\nit('keeps zero', () => {\n  expect(price(0)).toBe(0);\n});\n",
    )
    .unwrap();
    let (_, sarif) = audit_json(dir, &["--format", "sarif"]);
    let run = sarif["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["automationDetails"]["id"] == "fallow/audit/diff-risk")
        .expect("diff-risk SARIF run");
    assert_eq!(run["results"][0]["ruleId"], "fallow/weakened-tests");
    assert_eq!(run["results"][0]["level"], "warning");
    assert_eq!(
        run["results"][0]["locations"][0]["physicalLocation"]["region"]["startLine"],
        3
    );
    assert_eq!(
        run["tool"]["driver"]["rules"][0]["id"],
        "fallow/weakened-tests"
    );
}

/// A change whose only finding is a diff-risk `it.skip` marker.
fn skip_only_change() -> TempDir {
    let tmp = fixture(None);
    fs::write(
        tmp.path().join("src/index.test.ts"),
        "import { price } from './index';\n\nit.skip('doubles', () => {\n  expect(price(1)).toBe(2);\n});\n\nit('keeps zero', () => {\n  expect(price(0)).toBe(0);\n});\n",
    )
    .unwrap();
    tmp
}

fn audit_text(dir: &Path, format: &str) -> crate::common::CommandOutput {
    let root = dir.to_str().unwrap();
    run_fallow_raw(&[
        "audit", "--root", root, "--base", "main", "--format", format,
    ])
}

#[test]
fn human_status_line_names_diff_risk_when_it_sets_the_verdict() {
    let tmp = skip_only_change();
    let output = audit_text(tmp.path(), "human");
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output
            .stderr
            .contains("diff risk: 1 finding (warn) \u{00b7} 1 changed file"),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn compact_prints_diff_risk_records_in_the_compact_grammar() {
    let tmp = skip_only_change();
    let output = audit_text(tmp.path(), "compact");
    assert_eq!(output.code, 0, "{}", output.stderr);
    let lines: Vec<&str> = output
        .stdout
        .lines()
        .filter(|line| line.starts_with("weakened-tests:"))
        .collect();
    assert_eq!(
        lines,
        vec!["weakened-tests:src/index.test.ts:3:it.skip,severity=warn,kind=skip-only"],
        "stdout: {}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("[warn]"),
        "compact must not print the human line: {}",
        output.stdout
    );
}

#[test]
fn markdown_prints_a_diff_risk_table() {
    let tmp = skip_only_change();
    let output = audit_text(tmp.path(), "markdown");
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output.stdout.contains(
            "## Fallow: 1 diff-risk finding\n\n| Location | Rule | Severity | Evidence |"
        ),
        "stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains(
            "| `src/index.test.ts:3` | `weakened-tests` / `skip-only` | warn | `it.skip` |"
        ),
        "stdout: {}",
        output.stdout
    );
}
