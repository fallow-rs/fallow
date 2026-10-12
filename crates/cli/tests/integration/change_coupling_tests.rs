//! Change coupling and change dispersion in `fallow health`.
//!
//! The fixtures feed history through `--churn-file` with `commit` ids, so the
//! commit sets are exact and no git repository is needed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;

use serde_json::Value;
use tempfile::tempdir;

use crate::common::{commit_all, git, parse_json, run_fallow_raw, run_fallow_raw_with_env};

const CLOCK: u64 = 1_788_782_400;
const DAY: u64 = 86_400;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directories");
    }
    std::fs::write(path, contents).expect("write file");
}

/// A function with enough branches to give every file a complexity density.
fn branchy(name: &str) -> String {
    format!(
        "export function {name}(n: number): number {{
  if (n > 10) {{ return n > 20 ? 2 : 1; }}
  if (n < 0) {{ return n < -10 ? -2 : -1; }}
  return 0;
}}
"
    )
}

/// `a.ts` and `b.ts` change together with no import between them.
/// `c.ts` imports `d.ts` and they also change together.
fn project(commit_count: usize) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"change-coupling","type":"module","main":"src/index.ts"}"#,
    );
    write_file(
        &root.join("src/index.ts"),
        "import { a } from './a';\nimport { b } from './b';\nimport { c } from './c';\nconsole.log(a(1), b(1), c(1));\n",
    );
    write_file(&root.join("src/a.ts"), &branchy("a"));
    write_file(&root.join("src/b.ts"), &branchy("b"));
    write_file(
        &root.join("src/c.ts"),
        &format!("import {{ d }} from './d';\n{}", branchy("c"))
            .replace("return 0;", "return d(n);"),
    );
    write_file(&root.join("src/d.ts"), &branchy("d"));

    let mut events = Vec::new();
    for index in 0..commit_count {
        let timestamp = CLOCK - (index as u64 + 1) * DAY;
        for (commit, path) in [
            (format!("ab{index}"), "src/a.ts"),
            (format!("ab{index}"), "src/b.ts"),
            (format!("cd{index}"), "src/c.ts"),
            (format!("cd{index}"), "src/d.ts"),
        ] {
            events.push(serde_json::json!({
                "commit": commit,
                "path": path,
                "timestamp": timestamp,
                "author": "dev@example.com",
                "added": 3,
                "deleted": 1,
            }));
        }
    }
    let churn = serde_json::json!({ "schema": "fallow-churn/v1", "events": events });
    write_file(
        &root.join("churn.json"),
        &serde_json::to_string(&churn).unwrap(),
    );
    dir
}

fn health(root: &Path, extra: &[&str]) -> Value {
    let root_arg = root.to_str().expect("temp path is UTF-8");
    let mut args = vec![
        "--root",
        root_arg,
        "health",
        "--churn-file",
        "churn.json",
        "--format",
        "json",
        "--quiet",
    ];
    args.extend_from_slice(extra);
    let clock = CLOCK.to_string();
    let output = run_fallow_raw_with_env(&args, &[("FALLOW_CLOCK_EPOCH", clock.as_str())]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    parse_json(&output)
}

fn pair_paths(report: &Value) -> Vec<(String, String)> {
    report["change_coupling"]["pairs"]
        .as_array()
        .expect("pairs array")
        .iter()
        .map(|pair| {
            (
                pair["path_a"].as_str().unwrap().to_owned(),
                pair["path_b"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn hidden_pair_is_reported_and_an_imported_pair_is_not() {
    let dir = project(6);
    let report = health(dir.path(), &["--hotspots"]);

    assert_eq!(
        pair_paths(&report),
        vec![("src/a.ts".to_owned(), "src/b.ts".to_owned())],
        "c.ts imports d.ts, so only a/b is hidden: {}",
        report["change_coupling"]
    );
    let pair = &report["change_coupling"]["pairs"][0];
    assert_eq!(pair["shared_revisions"], 6);
    assert_eq!(pair["degree"], 1.0);
    assert_eq!(pair["strong"], true);
    assert_eq!(report["change_coupling"]["commits_analyzed"], 12);
    assert_eq!(report["change_coupling"]["commits_skipped"], 0);
    assert_eq!(report["change_coupling"]["thresholds"]["min_revisions"], 5);
    assert!(
        report["sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|section| section == "change-coupling"),
        "sections: {}",
        report["sections"]
    );
}

#[test]
fn human_output_lists_the_hidden_pair() {
    let dir = project(6);
    let root_arg = dir.path().to_str().unwrap();
    let clock = CLOCK.to_string();
    let output = run_fallow_raw_with_env(
        &[
            "--root",
            root_arg,
            "health",
            "--hotspots",
            "--churn-file",
            "churn.json",
            "--quiet",
        ],
        &[("FALLOW_CLOCK_EPOCH", clock.as_str()), ("NO_COLOR", "1")],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        output.stdout.contains("Hidden change coupling (1 pair"),
        "stdout: {}",
        output.stdout
    );
    assert!(output.stdout.contains("src/a.ts  src/b.ts [strong]"));
    assert!(
        output
            .stdout
            .contains("6 shared commits (6 and 6 in total)")
    );
}

#[test]
fn hotspot_entries_carry_change_dispersion() {
    let dir = project(6);
    let report = health(dir.path(), &["--hotspots"]);

    let hotspot = report["hotspots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == "src/a.ts")
        .unwrap_or_else(|| panic!("no hotspot for a.ts: {}", report["hotspots"]));
    assert_eq!(hotspot["median_files_per_commit"], 2.0);
    assert_eq!(hotspot["median_lines_per_commit"], 8.0);
}

#[test]
fn a_strong_hidden_pair_becomes_a_refactoring_target() {
    let dir = project(6);
    let report = health(dir.path(), &["--targets"]);

    let target = report["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["path"] == "src/a.ts")
        .unwrap_or_else(|| panic!("no target for a.ts: {}", report["targets"]));
    assert_eq!(target["category"], "resolve_hidden_coupling");
    assert_eq!(
        target["evidence"]["co_change_partners"][0]["path"],
        "src/b.ts"
    );
    assert!(
        target["factors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|factor| factor["metric"] == "hidden_coupling"),
        "factors: {}",
        target["factors"]
    );
    assert!(
        report.get("change_coupling").is_none(),
        "the section itself comes with --hotspots only"
    );
}

#[test]
fn too_few_commits_leave_the_section_empty_with_a_diagnostic() {
    // 2 a/b commits and 2 c/d commits: 4 commits, below the 5 revisions minimum.
    let dir = project(2);
    let report = health(dir.path(), &["--hotspots", "--min-commits", "1"]);

    assert!(pair_paths(&report).is_empty());
    assert!(
        report["workspace_diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "change-coupling-skipped"),
        "diagnostics: {}",
        report["workspace_diagnostics"]
    );
}

#[test]
fn config_thresholds_apply() {
    let dir = project(3);
    write_file(
        &dir.path().join(".fallowrc.json"),
        r#"{"health":{"changeCoupling":{"minRevisions":3,"minSharedRevisions":3}}}"#,
    );
    let report = health(dir.path(), &["--hotspots", "--min-commits", "1"]);

    assert_eq!(
        pair_paths(&report),
        vec![("src/a.ts".to_owned(), "src/b.ts".to_owned())]
    );
    assert_eq!(report["change_coupling"]["thresholds"]["min_revisions"], 3);
}

/// A repository where `a.ts` and `b.ts` changed together in six commits with
/// no import between them, and a head commit that changes only `a.ts`.
fn audit_repo() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"co-change-audit","type":"module","main":"src/index.ts"}"#,
    );
    write_file(
        &root.join("src/index.ts"),
        "import { a } from './a';\nimport { b } from './b';\nconsole.log(a(1), b(1));\n",
    );
    git(root, &["init", "-b", "main"]);
    for round in 0..6 {
        write_file(
            &root.join("src/a.ts"),
            &format!("export const a = (n: number): number => n + {round};\n"),
        );
        write_file(
            &root.join("src/b.ts"),
            &format!("export const b = (n: number): number => n * {round};\n"),
        );
        commit_all(root, &format!("round {round}"));
    }
    write_file(
        &root.join("src/a.ts"),
        "export const a = (n: number): number => n - 1;\n",
    );
    commit_all(root, "change a only");
    dir
}

#[test]
fn audit_brief_points_at_a_hidden_partner_missing_from_the_diff() {
    let dir = audit_repo();
    let output = run_fallow_raw(&[
        "review",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--brief",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let brief = parse_json(&output);

    let gaps = brief["impact_closure"]["co_change_gap"]
        .as_array()
        .unwrap_or_else(|| panic!("no co_change_gap: {}", brief["impact_closure"]));
    assert_eq!(gaps.len(), 1, "gaps: {gaps:?}");
    assert_eq!(gaps[0]["changed_file"], "src/a.ts");
    assert_eq!(gaps[0]["partner_file"], "src/b.ts");
    assert_eq!(gaps[0]["shared_revisions"], 6);
}

#[test]
fn audit_brief_has_no_gap_when_the_diff_changes_both_sides() {
    let dir = audit_repo();
    // Control: the same repository reports the gap while only one side changed,
    // so the assertion below cannot pass because gaps are never computed.
    let before = run_fallow_raw(&[
        "review",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--brief",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(before.code, 0, "stderr: {}", before.stderr);
    assert!(
        parse_json(&before)["impact_closure"]["co_change_gap"].is_array(),
        "control run has no gap"
    );
    write_file(
        &dir.path().join("src/b.ts"),
        "export const b = (n: number): number => n * 9;\n",
    );
    commit_all(dir.path(), "change b too");
    let output = run_fallow_raw(&[
        "review",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "main~2",
        "--brief",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let brief = parse_json(&output);
    assert!(
        brief["impact_closure"].get("co_change_gap").is_none(),
        "both sides changed: {}",
        brief["impact_closure"]
    );
}
