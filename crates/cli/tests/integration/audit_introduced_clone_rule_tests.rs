//! The `introduced-clone-group` rule: its severity decides how a clone group
//! takes part in the `fallow audit` verdict and exit code.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{commit_all, git, parse_json, run_fallow_raw};
use std::fs;
use tempfile::TempDir;

const CLONE: &str = "export function NAME(items, opts) {\n  const out = [];\n  for (const item of items) {\n    if (item.kind === 'alpha' && opts.includeAlpha) {\n      out.push({ id: item.id, label: item.name.toUpperCase(), score: item.score * 2 });\n    } else if (item.kind === 'beta' && opts.includeBeta) {\n      out.push({ id: item.id, label: item.name.toLowerCase(), score: item.score + 10 });\n    } else if (item.kind === 'gamma') {\n      out.push({ id: item.id, label: item.name.trim(), score: Math.max(0, item.score - 5) });\n    } else {\n      out.push({ id: item.id, label: String(item.name), score: 0 });\n    }\n  }\n  const total = out.reduce((sum, row) => sum + row.score, 0);\n  const sorted = out.sort((x, y) => y.score - x.score);\n  return { total, rows: sorted, count: sorted.length, empty: sorted.length === 0 };\n}\n";

const INDEX: &str =
    "import { a } from './a.js';\nimport { b } from './b.js';\nconsole.log(a, b);\n";

/// How the change relates to the clone group.
#[derive(Clone, Copy)]
enum Change {
    /// The base holds `src/a.js`; the change copies the function into `src/b.js`.
    Copy,
    /// The base holds both copies; the change adds an unrelated line to `src/b.js`.
    TouchInheritedCopy,
}

struct AuditRun {
    code: i32,
    json: serde_json::Value,
}

/// Commit a fixture with the given `.fallowrc.json` text, apply `change`, and
/// run `fallow audit --only dupes` against the commit under `gate`.
fn audit(config: Option<&str>, change: Change, gate: &str) -> AuditRun {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-introduced-clone-rule","type":"module","main":"src/index.js"}"#,
    )
    .unwrap();
    if let Some(config) = config {
        fs::write(root.join(".fallowrc.json"), config).unwrap();
    }
    fs::write(root.join("src/index.js"), INDEX).unwrap();
    fs::write(root.join("src/a.js"), CLONE.replace("NAME", "a")).unwrap();
    let copy = CLONE.replace("NAME", "b");
    match change {
        Change::Copy => fs::write(root.join("src/b.js"), "export const b = () => 0;\n").unwrap(),
        Change::TouchInheritedCopy => fs::write(root.join("src/b.js"), &copy).unwrap(),
    }
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "core.autocrlf", "false"]);
    commit_all(root, "initial");
    match change {
        Change::Copy => fs::write(root.join("src/b.js"), &copy).unwrap(),
        Change::TouchInheritedCopy => fs::write(
            root.join("src/b.js"),
            format!("{copy}export const marker = 1;\n"),
        )
        .unwrap(),
    }

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD",
        "--gate",
        gate,
        "--only",
        "dupes",
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    assert!(
        output.code == 0 || output.code == 1,
        "audit must reach a verdict. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    AuditRun {
        code: output.code,
        json: parse_json(&output),
    }
}

fn rule(severity: &str) -> String {
    format!(r#"{{"rules":{{"introduced-clone-group":"{severity}"}}}}"#)
}

fn assert_verdict(run: &AuditRun, verdict: &str, code: i32) {
    let attribution = &run.json["attribution"];
    assert_eq!(
        run.json["verdict"].as_str(),
        Some(verdict),
        "unexpected verdict, attribution: {attribution}"
    );
    assert_eq!(
        run.code, code,
        "unexpected exit code, attribution: {attribution}"
    );
}

fn assert_introduced(run: &AuditRun, introduced: u64) {
    let attribution = &run.json["attribution"];
    assert_eq!(
        attribution["duplication_introduced"].as_u64(),
        Some(introduced),
        "unexpected attribution: {attribution}"
    );
}

#[test]
fn introduced_clone_group_warns_by_default() {
    let run = audit(None, Change::Copy, "new-only");
    assert_introduced(&run, 1);
    assert_verdict(&run, "warn", 0);

    let explicit = audit(Some(&rule("warn")), Change::Copy, "new-only");
    assert_verdict(&explicit, "warn", 0);
}

#[test]
fn introduced_clone_group_error_fails_the_audit() {
    let run = audit(Some(&rule("error")), Change::Copy, "new-only");
    assert_introduced(&run, 1);
    assert_verdict(&run, "fail", 1);
}

#[test]
fn introduced_clone_group_off_drops_it_from_the_verdict() {
    let run = audit(Some(&rule("off")), Change::Copy, "new-only");
    assert_introduced(&run, 1);
    assert_eq!(
        run.json["duplication"]["clone_groups"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "the clone group stays in the report"
    );
    assert_verdict(&run, "pass", 0);
}

#[test]
fn inherited_clone_group_never_gates_under_new_only() {
    let run = audit(Some(&rule("error")), Change::TouchInheritedCopy, "new-only");
    assert_introduced(&run, 0);
    assert_verdict(&run, "pass", 0);
}

#[test]
fn gate_all_applies_the_rule_to_every_clone_group() {
    let default = audit(None, Change::TouchInheritedCopy, "all");
    assert_verdict(&default, "warn", 0);

    let error = audit(Some(&rule("error")), Change::TouchInheritedCopy, "all");
    assert_verdict(&error, "fail", 1);

    let off = audit(Some(&rule("off")), Change::TouchInheritedCopy, "all");
    assert_verdict(&off, "pass", 0);
}

#[test]
fn override_on_any_instance_file_sets_the_group_severity() {
    let config =
        r#"{"overrides":[{"files":["src/b.js"],"rules":{"introduced-clone-group":"error"}}]}"#;
    let run = audit(Some(config), Change::Copy, "new-only");
    assert_verdict(&run, "fail", 1);

    let config = r#"{"rules":{"introduced-clone-group":"error"},"overrides":[{"files":["src/b.js"],"rules":{"introduced-clone-group":"off"}}]}"#;
    let run = audit(Some(config), Change::Copy, "new-only");
    assert_verdict(&run, "fail", 1);

    let config = r#"{"rules":{"introduced-clone-group":"error"},"overrides":[{"files":["src/*.js"],"rules":{"introduced-clone-group":"off"}}]}"#;
    let run = audit(Some(config), Change::Copy, "new-only");
    assert_verdict(&run, "pass", 0);
}

#[test]
fn threshold_still_fails_when_the_rule_is_off() {
    let config = r#"{"rules":{"introduced-clone-group":"off"},"duplicates":{"threshold":1.0}}"#;
    let run = audit(Some(config), Change::Copy, "new-only");
    assert_verdict(&run, "fail", 1);
}
