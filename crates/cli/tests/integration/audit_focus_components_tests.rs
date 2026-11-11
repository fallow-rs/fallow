//! Evidence components of the focus score in `fallow audit --brief`.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::common::{commit_all, git, parse_json, run_fallow_raw};

const PACKAGE: &str = r#"{"name":"focus","private":true,"type":"module","main":"src/main.ts","devDependencies":{"vitest":"^3.0.0","typescript":"^5.6.0"}}"#;

const MAIN: &str =
    "import { label } from './label';\n\nexport const main = (): string => label('x');\n";

const TEST: &str = "import { expect, test } from 'vitest';\nimport { main } from '../src/main';\n\ntest('main', () => {\n  expect(main()).toBe('x!');\n});\n";

const LABEL_BASE: &str =
    "export const label = (value: string): string => {\n  return `${value}!`;\n};\n";

/// The head adds `shout` on lines 5 to 7. No test references it.
const LABEL_WITH_EXPORT: &str = "export const label = (value: string): string => {\n  return `${value}!`;\n};\n\nexport const shout = (value: string): string => {\n  return value.toUpperCase();\n};\n";

/// The head changes line 2 only.
const LABEL_EDITED: &str =
    "export const label = (value: string): string => {\n  return `${value}!!`.slice(0, -1);\n};\n";

/// The score keys that a focus unit had before the evidence components.
const EARLIER_SCORE_KEYS: &[&str] = &["fan_io", "risk_zone", "change_shape", "total"];

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("parent")).expect("create fixture directory");
    fs::write(path, content).expect("write fixture file");
}

fn project(root: &Path, head_label: &str) {
    write(root, "package.json", PACKAGE);
    write(root, "src/main.ts", MAIN);
    write(root, "src/label.ts", LABEL_BASE);
    write(root, "test/main.test.ts", TEST);
    git(root, &["init", "-b", "main"]);
    commit_all(root, "base");
    write(root, "src/label.ts", head_label);
    commit_all(root, "head");
}

fn brief(root: &Path, extra: &[&str]) -> Value {
    let output = brief_raw(root, extra);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    parse_json(&output)
}

fn brief_raw(root: &Path, extra: &[&str]) -> crate::common::CommandOutput {
    let mut args = vec![
        "audit",
        "--root",
        root.to_str().expect("utf-8 root"),
        "--base",
        "main~1",
        "--brief",
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ];
    args.extend_from_slice(extra);
    run_fallow_raw(&args)
}

fn units(brief: &Value) -> Vec<&Value> {
    let focus = &brief["focus"];
    focus["review_here"]
        .as_array()
        .into_iter()
        .chain(focus["deprioritized"].as_array())
        .flatten()
        .collect()
}

fn unit<'a>(brief: &'a Value, file: &str) -> &'a Value {
    units(brief)
        .into_iter()
        .find(|unit| unit["file"] == file)
        .unwrap_or_else(|| panic!("no focus unit for {file}: {brief:#}"))
}

fn mutation_report(root: &Path, line: u32, digest: Option<&str>) -> std::path::PathBuf {
    let source = fs::read(root.join("src/label.ts")).expect("read label");
    let digest = digest.map_or_else(
        || {
            Sha256::digest(&source)
                .iter()
                .fold(String::new(), |mut hex, byte| {
                    let _ = write!(hex, "{byte:02x}");
                    hex
                })
        },
        str::to_string,
    );
    let report = serde_json::json!({
        "kind": "mutation",
        "mutants": [
            {"file": "src/label.ts", "line": line, "status": "survived"},
            {"file": "src/label.ts", "line": line, "status": "killed"},
        ],
        "files": [{"path": "src/label.ts", "source_sha256": digest}],
    });
    let path = root.join("mutation-report.json");
    fs::write(&path, serde_json::to_vec(&report).expect("report json")).expect("write report");
    path
}

#[test]
fn a_run_without_evidence_keeps_the_earlier_score_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), LABEL_EDITED);
    let brief = brief(dir.path(), &[]);
    let units = units(&brief);
    assert!(!units.is_empty(), "{brief:#}");
    for unit in units {
        let keys: Vec<&str> = unit["score"]
            .as_object()
            .expect("score object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, EARLIER_SCORE_KEYS, "{unit:#}");
    }
}

#[test]
fn a_changed_export_without_a_test_adds_the_test_reach_component() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), LABEL_WITH_EXPORT);
    let brief = brief(dir.path(), &[]);
    let label = unit(&brief, "src/label.ts");
    assert_eq!(label["score"]["test_reach"], 1, "{label:#}");
    assert!(
        label["reason"]
            .as_str()
            .is_some_and(|reason| reason.ends_with("1 changed export with no real test run")),
        "{label:#}"
    );
    let rows = brief["test_reach"]["exports"]
        .as_array()
        .expect("test reach rows");
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert_eq!(rows[0]["export_name"], "shout");
    assert_eq!(rows[0]["reach"], "none");
}

#[test]
fn a_mutation_report_adds_the_survivors_on_added_lines() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), LABEL_EDITED);

    let without = brief(dir.path(), &[]);
    assert!(
        unit(&without, "src/label.ts")["score"]
            .get("mutation")
            .is_none()
    );

    let on_added_line = mutation_report(dir.path(), 2, None);
    let with = brief(
        dir.path(),
        &["--mutation-report", on_added_line.to_str().expect("utf-8")],
    );
    let label = unit(&with, "src/label.ts");
    assert_eq!(label["score"]["mutation"], 1, "{label:#}");
    assert!(
        label["reason"]
            .as_str()
            .is_some_and(|reason| reason.ends_with("1 surviving mutant on changed lines")),
        "{label:#}"
    );

    let on_context_line = mutation_report(dir.path(), 1, None);
    let context = brief(
        dir.path(),
        &[
            "--mutation-report",
            on_context_line.to_str().expect("utf-8"),
        ],
    );
    assert!(
        unit(&context, "src/label.ts")["score"]
            .get("mutation")
            .is_none()
    );

    let stale = mutation_report(dir.path(), 2, Some("0000"));
    let stale = brief(
        dir.path(),
        &["--mutation-report", stale.to_str().expect("utf-8")],
    );
    assert!(
        unit(&stale, "src/label.ts")["score"]
            .get("mutation")
            .is_none()
    );
}

#[test]
fn an_invalid_mutation_report_exits_with_an_input_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), LABEL_EDITED);
    let path = dir.path().join("not-a-report.json");
    fs::write(&path, "{}").expect("write file");
    let output = brief_raw(
        dir.path(),
        &["--mutation-report", path.to_str().expect("utf-8")],
    );
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(
        output.stdout.contains("FALLOW_MUTATION_REPORT_INVALID"),
        "stdout: {}",
        output.stdout
    );
}

#[test]
fn churn_counts_for_a_source_file_and_not_for_a_test_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", PACKAGE);
    write(root, "src/main.ts", MAIN);
    write(root, "src/label.ts", LABEL_BASE);
    write(root, "test/main.test.ts", TEST);
    git(root, &["init", "-b", "main"]);
    commit_all(root, "base");
    for round in 1..=4 {
        write(
            root,
            "src/label.ts",
            &format!("{LABEL_BASE}// round {round}\n"),
        );
        write(
            root,
            "test/main.test.ts",
            &format!("{TEST}// round {round}\n"),
        );
        commit_all(root, &format!("round {round}"));
    }
    let brief = brief(root, &[]);
    let label = unit(&brief, "src/label.ts");
    assert_eq!(label["score"]["churn"], 1, "{label:#}");
    assert!(
        label["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("changed often (5 recent commits)")),
        "{label:#}"
    );
    let test = unit(&brief, "test/main.test.ts");
    assert!(test["score"].get("churn").is_none(), "{test:#}");
}

#[test]
fn the_gating_audit_does_not_read_the_mutation_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), LABEL_EDITED);
    let path = dir.path().join("not-a-report.json");
    fs::write(&path, "{}").expect("write file");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().expect("utf-8 root"),
        "--base",
        "main~1",
        "--no-cache",
        "--format",
        "json",
        "--mutation-report",
        path.to_str().expect("utf-8"),
    ]);
    assert_ne!(output.code, 2, "stdout: {}", output.stdout);
    assert!(
        output
            .stderr
            .contains("--mutation-report has no effect without --brief"),
        "stderr: {}",
        output.stderr
    );
}
