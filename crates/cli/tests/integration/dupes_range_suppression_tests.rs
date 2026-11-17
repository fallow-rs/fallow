//! `fallow-ignore-start` / `fallow-ignore-end` ranges for clones, end to end
//! (issue #4706).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{commit_all, git, parse_json, run_fallow_in_root, run_fallow_raw_with_env};
use std::path::Path;
use tempfile::tempdir;

const START: &str = "// fallow-ignore-start code-duplication -- kept on purpose";
const END: &str = "// fallow-ignore-end code-duplication";

fn function(name: &str, limit: u32) -> String {
    format!(
        "export function {name}(items: number[], factor: number): number {{\n  let total = 0;\n  for (const item of items) {{\n    if (item > {limit}) {{\n      total += item * factor;\n    }} else {{\n      total -= item;\n    }}\n  }}\n  const scaled = total / items.length;\n  console.log(\"result\", scaled, factor);\n  return Math.round(scaled * 100) / 100;\n}}\n"
    )
}

/// The issue #4706 repro, with a range around `two()` in `a.ts`.
fn write_repro(root: &Path) {
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"r","version":"1.0.0"}"#,
    )
    .unwrap();
    let a = format!(
        "{}{START}\n{}{END}\n{}",
        function("one", 1),
        function("two", 2),
        function("three", 3),
    );
    let b = format!(
        "{}export const sepA = 1;\n{}export const sepB = 2;\n{}\nexport const lonely = 1; // tail\n// fallow-ignore-next-line code-duplication\nexport const alsoLonely = 2;\n",
        function("one", 1),
        function("two", 2),
        function("three", 3),
    );
    std::fs::write(src.join("a.ts"), a).unwrap();
    std::fs::write(src.join("b.ts"), b).unwrap();
}

fn group_lines(json: &serde_json::Value) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = json["clone_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| {
            let mut instances: Vec<String> = group["instances"]
                .as_array()
                .unwrap()
                .iter()
                .map(|instance| {
                    format!(
                        "{}:{}-{}",
                        instance["file"].as_str().unwrap(),
                        instance["start_line"],
                        instance["end_line"]
                    )
                })
                .collect();
            instances.sort();
            instances
        })
        .collect();
    groups.sort();
    groups
}

fn expected_repro_groups() -> Vec<Vec<String>> {
    vec![
        vec!["src/a.ts:1-12".to_string(), "src/b.ts:1-12".to_string()],
        vec!["src/a.ts:29-40".to_string(), "src/b.ts:29-40".to_string()],
        vec![
            "src/b.ts:18-28".to_string(),
            "src/b.ts:32-43".to_string(),
            "src/b.ts:4-14".to_string(),
        ],
    ]
}

const JSON_ARGS: &[&str] = &["--format", "json", "--quiet", "--no-cache"];

#[test]
fn issue_repro_keeps_the_neighbor_clones_and_cuts_them_at_the_range() {
    let dir = tempdir().unwrap();
    write_repro(dir.path());

    let json = parse_json(&run_fallow_in_root("dupes", dir.path(), JSON_ARGS));

    assert_eq!(group_lines(&json), expected_repro_groups());
    assert!(json.get("range_marker_problems").is_none());
}

#[test]
fn rolling_detector_applies_the_range_too() {
    let dir = tempdir().unwrap();
    write_repro(dir.path());
    let root = dir.path().to_str().unwrap();
    let mut args = vec!["dupes", "--root", root];
    args.extend_from_slice(JSON_ARGS);

    let json = parse_json(&run_fallow_raw_with_env(
        &args,
        &[("FALLOW_DUPES_ROLLING", "1")],
    ));

    assert_eq!(group_lines(&json), expected_repro_groups());
}

#[test]
fn clone_groups_offer_a_suppress_range_action() {
    let dir = tempdir().unwrap();
    write_repro(dir.path());

    let json = parse_json(&run_fallow_in_root("dupes", dir.path(), JSON_ARGS));

    let actions = json["clone_groups"][0]["actions"].as_array().unwrap();
    let types: Vec<&str> = actions
        .iter()
        .map(|a| a["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        vec!["extract-shared", "suppress-line", "suppress-range"]
    );
    assert_eq!(
        actions[2]["comment"],
        "// fallow-ignore-start code-duplication -- <reason>"
    );
    assert_eq!(
        actions[2]["end_comment"],
        "// fallow-ignore-end code-duplication"
    );
}

/// A file with every range marker problem: an end with no start, a start
/// with an unsupported kind, and a start that no end closes.
fn write_malformed(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"m","version":"1.0.0"}"#,
    )
    .unwrap();
    let body = function("shared", 1);
    std::fs::write(root.join("a.ts"), &body).unwrap();
    std::fs::write(
        root.join("b.ts"),
        format!(
            "{END}\n// fallow-ignore-start unused-export\n{START}\n{body}const zz = 1; {END}\n"
        ),
    )
    .unwrap();
}

#[test]
fn dupes_reports_malformed_range_markers() {
    let dir = tempdir().unwrap();
    write_malformed(dir.path());

    let json = parse_json(&run_fallow_in_root("dupes", dir.path(), JSON_ARGS));

    assert_eq!(
        json["range_marker_problems"],
        serde_json::json!([
            {"path": "b.ts", "line": 1, "problem": "unmatched_end"},
            {"path": "b.ts", "line": 2, "problem": "unsupported_kind", "token": "unused-export"},
            {"path": "b.ts", "line": 3, "problem": "unclosed_start"},
            {"path": "b.ts", "line": 17, "problem": "not_own_line"},
        ])
    );
    assert_eq!(
        group_lines(&json).len(),
        1,
        "malformed markers mask nothing"
    );

    let human = run_fallow_in_root("dupes", dir.path(), &["--no-cache"]);
    let text = format!("{}{}", human.stdout, human.stderr);
    assert!(
        text.contains("b.ts:3"),
        "human output names the marker: {text}"
    );
    assert!(
        text.contains("fallow-ignore-start has no fallow-ignore-end"),
        "human output explains the problem: {text}"
    );
}

#[test]
fn audit_carries_the_range_marker_problems_of_changed_files() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"m","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("old.ts"),
        format!("{END}\nexport const old = 1;\n"),
    )
    .unwrap();
    git(dir.path(), &["init", "-q"]);
    commit_all(dir.path(), "base");
    write_malformed(dir.path());

    let json = parse_json(&run_fallow_in_root(
        "audit",
        dir.path(),
        &[
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--base",
            "HEAD",
        ],
    ));

    let paths: Vec<&str> = json["duplication"]["range_marker_problems"]
        .as_array()
        .unwrap_or_else(|| panic!("audit lists the problems: {}", json["duplication"]))
        .iter()
        .map(|p| p["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["b.ts"; 4], "only the changed file: {paths:?}");
}

#[test]
fn suppressions_inventory_lists_ranges_and_marks_malformed_markers() {
    let dir = tempdir().unwrap();
    write_malformed(dir.path());
    let body = function("other", 2);
    std::fs::write(
        dir.path().join("c.ts"),
        format!("// fallow-ignore-start code-duplication\n{body}{END}\n"),
    )
    .unwrap();

    let json = parse_json(&run_fallow_in_root(
        "suppressions",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    ));

    let files = json["files"].as_array().unwrap();
    let entries = |path: &str| -> Vec<serde_json::Value> {
        files
            .iter()
            .find(|f| f["path"] == path)
            .map(|f| f["suppressions"].as_array().unwrap().clone())
            .unwrap_or_default()
    };
    assert_eq!(
        entries("c.ts"),
        vec![serde_json::json!({
            "line": 1,
            "kind": "code-duplication",
            "level": "range",
            "end_line": 15,
            "origin": "comment",
            "reason": null,
            "reason_present": false,
        })]
    );
    let problems: Vec<(u64, String)> = entries("b.ts")
        .iter()
        .map(|e| {
            assert_eq!(e["level"], "range");
            (
                e["line"].as_u64().unwrap(),
                e["problem"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        problems,
        vec![
            (1, "unmatched_end".to_string()),
            (2, "unsupported_kind".to_string()),
            (3, "unclosed_start".to_string()),
            (17, "not_own_line".to_string()),
        ]
    );
}

#[test]
fn require_suppression_reason_names_the_range_marker() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"q","version":"1.0.0","main":"a.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{"rules":{"require-suppression-reason":"error"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("a.ts"),
        format!(
            "// fallow-ignore-start code-duplication\n{}{END}\n",
            function("one", 1)
        ),
    )
    .unwrap();

    let json = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    ));

    let entries = json["stale_suppressions"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["line"], 1);
    assert_eq!(entries[0]["missing_reason"], true);
    assert_eq!(entries[0]["origin"]["is_range"], true);
    assert_eq!(entries[0]["origin"]["is_file_level"], false);
}
