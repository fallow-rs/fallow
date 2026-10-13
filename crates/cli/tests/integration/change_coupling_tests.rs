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
        output.stdout.contains("  2.0 files/commit"),
        "the hotspot row shows the dispersion: {}",
        output.stdout
    );
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

#[test]
fn out_of_range_thresholds_exit_with_a_config_error() {
    for (config, field) in [
        (r#"{"maxFilesPerCommit":1}"#, "maxFilesPerCommit"),
        (r#"{"maxFilesPerCommit":5000}"#, "maxFilesPerCommit"),
        (r#"{"minDegree":1.5}"#, "minDegree"),
    ] {
        let dir = project(1);
        write_file(
            &dir.path().join(".fallowrc.json"),
            &format!(r#"{{"health":{{"changeCoupling":{config}}}}}"#),
        );
        let output = run_fallow_raw(&["--root", dir.path().to_str().unwrap(), "health", "--quiet"]);
        assert_eq!(output.code, 2, "config {config}: {}", output.stderr);
        assert!(
            output
                .stderr
                .contains(&format!("health.changeCoupling.{field} must be")),
            "stderr: {}",
            output.stderr
        );
    }
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
fn audit_brief_skips_a_gap_whose_partner_is_in_health_ignore() {
    let dir = audit_repo();
    write_file(
        &dir.path().join(".fallowrc.json"),
        r#"{"health":{"ignore":["src/b.ts"]}}"#,
    );
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
    assert!(
        brief["impact_closure"].get("co_change_gap").is_none(),
        "b.ts is ignored: {}",
        brief["impact_closure"]
    );
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

/// Run `fallow health --hotspots` on the fixture in a text format.
fn health_text(root: &Path, format: &str) -> String {
    let root_arg = root.to_str().expect("temp path is UTF-8");
    let clock = CLOCK.to_string();
    let output = run_fallow_raw_with_env(
        &[
            "--root",
            root_arg,
            "health",
            "--hotspots",
            "--churn-file",
            "churn.json",
            "--format",
            format,
            "--quiet",
        ],
        &[("FALLOW_CLOCK_EPOCH", clock.as_str()), ("NO_COLOR", "1")],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    output.stdout
}

/// The change coupling section of a markdown report, up to the next heading
/// or the metric legend.
fn markdown_coupling_section(markdown: &str) -> String {
    let start = markdown
        .find("### Hidden Change Coupling")
        .unwrap_or_else(|| panic!("no coupling section:\n{markdown}"));
    let rest = &markdown[start..];
    let end = ["\n### ", "\n---"]
        .iter()
        .filter_map(|marker| rest[3..].find(marker).map(|index| index + 3))
        .min()
        .unwrap_or(rest.len());
    rest[..end].trim_end().to_owned()
}

#[test]
fn markdown_output_lists_the_hidden_pair() {
    let dir = project(6);
    let markdown = health_text(dir.path(), "markdown");
    insta::assert_snapshot!(
        "change_coupling_markdown_section",
        markdown_coupling_section(&markdown)
    );
}

#[test]
fn report_from_markdown_lists_the_hidden_pair() {
    let dir = project(6);
    let root = dir.path();
    let saved = root.join("health.json");
    std::fs::write(&saved, health_text(root, "json")).expect("write saved report");
    let output = run_fallow_raw(&[
        "report",
        "--from",
        saved.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--format",
        "markdown",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    insta::assert_snapshot!(
        "change_coupling_markdown_section",
        markdown_coupling_section(&output.stdout)
    );
}

#[test]
fn compact_output_has_one_line_per_hidden_pair() {
    let dir = project(6);
    let compact = health_text(dir.path(), "compact");
    let lines: Vec<&str> = compact
        .lines()
        .filter(|line| line.starts_with("change-coupling:"))
        .collect();
    insta::assert_snapshot!("change_coupling_compact_lines", lines.join("\n"));
}

#[test]
fn grouped_run_keeps_change_coupling_at_the_root() {
    let dir = project(6);
    let report = health(dir.path(), &["--hotspots", "--group-by", "directory"]);

    assert_eq!(
        pair_paths(&report),
        vec![("src/a.ts".to_owned(), "src/b.ts".to_owned())]
    );
    let groups = report["groups"]
        .as_array()
        .unwrap_or_else(|| panic!("no groups: {report}"));
    assert!(!groups.is_empty());
    assert!(
        groups
            .iter()
            .all(|group| group.get("change_coupling").is_none()),
        "groups: {groups:?}"
    );
}

#[test]
fn targets_only_run_records_no_skipped_diagnostic() {
    // Too few commits for coupling. The run asks for targets, not for the
    // change coupling section, so no diagnostic must explain an absent section.
    let dir = project(2);
    let report = health(dir.path(), &["--targets", "--min-commits", "1"]);

    assert!(report.get("change_coupling").is_none());
    let diagnostics = report["workspace_diagnostics"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    assert!(
        !diagnostics
            .iter()
            .any(|entry| entry["kind"] == "change-coupling-skipped"),
        "diagnostics: {diagnostics:?}"
    );
}
