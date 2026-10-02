//! `health --group-by` selection, per-group counts, per-group `--top`, and
//! per-group snapshot trends (`--group`, `--save-snapshot`, `--trend-from`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use crate::common::{parse_json, redact_all, run_fallow_in_root};

/// A function with `branches` independent `if` statements.
fn branchy(name: &str, branches: usize) -> String {
    let mut body = format!("export function {name}(x: number): number {{\n  let n = 0;\n");
    for i in 0..branches {
        let _ = writeln!(body, "  if (x > {i}) {{ n += {i}; }}");
    }
    body.push_str("  return n;\n}\n");
    body
}

/// Two CODEOWNERS teams, each owning two complex functions in two files.
fn project() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "group-trend", "private": true, "main": "src/index.ts" }"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(root.join(".github")).expect("create .github");
    std::fs::write(
        root.join(".github/CODEOWNERS"),
        "/src/a/ @team/a\n/src/b/ @team/b\n",
    )
    .expect("write CODEOWNERS");
    for team in ["a", "b"] {
        std::fs::create_dir_all(root.join("src").join(team)).expect("create team dir");
        std::fs::write(
            root.join(format!("src/{team}/one.ts")),
            branchy(&format!("{team}One"), 30),
        )
        .expect("write one");
        std::fs::write(
            root.join(format!("src/{team}/two.ts")),
            branchy(&format!("{team}Two"), 12),
        )
        .expect("write two");
    }
    std::fs::write(
        root.join("src/index.ts"),
        "export * from './a/one';\nexport * from './a/two';\nexport * from './b/one';\nexport * from './b/two';\n",
    )
    .expect("write index");
    dir
}

fn health_json(root: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["--group-by", "owner", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = run_fallow_in_root("health", root, &args);
    assert!(
        matches!(output.code, 0 | 1),
        "unexpected exit {}\nstderr:\n{}",
        output.code,
        output.stderr
    );
    parse_json(&output)
}

/// Replace the run time at the end of a job summary line (`· 4ms`), which
/// changes from run to run.
fn redact_elapsed(line: &str) -> String {
    match line.rsplit_once(" \u{b7} ") {
        Some((head, tail))
            if tail.ends_with('s') && tail.starts_with(|c: char| c.is_ascii_digit()) =>
        {
            format!("{head} \u{b7} [ELAPSED]")
        }
        _ => line.to_owned(),
    }
}

fn group<'a>(envelope: &'a Value, key: &str) -> &'a Value {
    envelope["groups"]
        .as_array()
        .expect("groups array")
        .iter()
        .find(|group| group["key"] == key)
        .unwrap_or_else(|| panic!("group {key} missing: {envelope:#}"))
}

fn group_keys(envelope: &Value) -> Vec<String> {
    envelope["groups"]
        .as_array()
        .expect("groups array")
        .iter()
        .map(|group| group["key"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn group_selector_keeps_only_matching_groups_and_reports_the_request() {
    let dir = project();
    let full = health_json(dir.path(), &[]);
    let selected = health_json(dir.path(), &["--group", "@team/a"]);

    assert_eq!(group_keys(&selected), vec!["@team/a"]);
    assert_eq!(selected["group_filter"], serde_json::json!(["@team/a"]));
    let outcome = &selected["request_outcomes"]["group-filter"];
    assert_eq!(outcome["status"], "applied");
    assert_eq!(outcome["affects"], "scope");
    assert_eq!(outcome["requested"], "@team/a");
    assert_eq!(outcome["scope_size"], 1);
    // The project-level sections stay project-wide.
    assert_eq!(selected["summary"], full["summary"]);
    // The kept group is identical to the same group of an unfiltered run.
    assert_eq!(group(&selected, "@team/a"), group(&full, "@team/a"));
    assert!(full.get("group_filter").is_none());
}

#[test]
fn group_selector_accepts_globs_and_negation() {
    let dir = project();
    let glob = health_json(dir.path(), &["--group", "@team/*,!@team/b"]);
    assert_eq!(group_keys(&glob), vec!["@team/a"]);
}

#[test]
fn group_selector_without_a_match_keeps_the_exit_code_and_warns() {
    let dir = project();
    let full = run_fallow_in_root(
        "health",
        dir.path(),
        &["--group-by", "owner", "--format", "json"],
    );
    let none = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--group-by",
            "owner",
            "--group",
            "@nobody",
            "--format",
            "json",
        ],
    );
    assert_eq!(none.code, full.code, "stderr:\n{}", none.stderr);
    assert!(
        none.stderr
            .contains("--group pattern '@nobody' matched no owner group"),
        "stderr:\n{}",
        none.stderr
    );
    let envelope = parse_json(&none);
    assert_eq!(envelope["groups"], serde_json::json!([]));
    assert_eq!(
        envelope["request_outcomes"]["group-filter"]["scope_size"],
        0
    );
}

#[test]
fn group_selector_is_rejected_outside_health_and_without_group_by() {
    let dir = project();
    let dead_code = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--group-by", "owner", "--group", "@team/a", "--quiet"],
    );
    assert_eq!(dead_code.code, 2, "stderr:\n{}", dead_code.stderr);
    assert!(
        dead_code
            .stderr
            .contains("--group is valid with `fallow health --group-by` only")
    );

    let ungrouped = run_fallow_in_root("health", dir.path(), &["--group", "@team/a", "--quiet"]);
    assert_eq!(ungrouped.code, 2, "stderr:\n{}", ungrouped.stderr);
}

#[test]
fn groups_carry_severity_and_hotspot_counts() {
    let dir = project();
    let envelope = health_json(dir.path(), &[]);
    for key in ["@team/a", "@team/b"] {
        let group = group(&envelope, key);
        let findings = group["findings"].as_array().map_or(0, Vec::len);
        let counted = group["severity_critical_count"].as_u64().unwrap()
            + group["severity_high_count"].as_u64().unwrap()
            + group["severity_moderate_count"].as_u64().unwrap();
        assert_eq!(counted as usize, findings, "{group:#}");
        assert!(findings > 0, "fixture must give findings: {group:#}");
        assert!(group["hotspot_count"].is_u64(), "{group:#}");
    }
}

/// A section that the project omits is also absent from each group. The
/// counts stay, as they do in the project `summary`.
#[test]
fn score_only_groups_omit_the_lists_that_the_project_omits() {
    let dir = project();
    let envelope = health_json(dir.path(), &["--score"]);
    for list in [
        "findings",
        "file_scores",
        "hotspots",
        "large_functions",
        "targets",
    ] {
        assert!(
            envelope[list].as_array().is_none_or(Vec::is_empty),
            "project {list} must be empty on a score-only run"
        );
    }
    for key in ["@team/a", "@team/b"] {
        let group = group(&envelope, key);
        for list in [
            "findings",
            "file_scores",
            "hotspots",
            "large_functions",
            "targets",
            "coverage_source_consistency",
        ] {
            assert!(group.get(list).is_none(), "{key} has {list}: {group:#}");
        }
        assert_eq!(group["severity_critical_count"], 2, "{group:#}");
        assert!(group["health_score"]["score"].is_number(), "{group:#}");
    }
    assert_eq!(envelope["summary"]["functions_above_threshold"], 4);
}

#[test]
fn complexity_only_groups_keep_findings_and_omit_file_scores() {
    let dir = project();
    let envelope = health_json(dir.path(), &["--complexity"]);
    assert!(envelope["file_scores"].as_array().is_none_or(Vec::is_empty));
    for key in ["@team/a", "@team/b"] {
        let group = group(&envelope, key);
        assert_eq!(group["findings"].as_array().map_or(0, Vec::len), 2);
        assert!(group.get("file_scores").is_none(), "{group:#}");
    }
}

#[test]
fn top_applies_to_each_group_and_keeps_the_counts() {
    let dir = project();
    let full = health_json(dir.path(), &[]);
    let top = health_json(dir.path(), &["--top", "1"]);
    for key in ["@team/a", "@team/b"] {
        let limited = group(&top, key);
        assert_eq!(
            limited["findings"].as_array().map_or(0, Vec::len),
            1,
            "each group keeps its own top finding: {limited:#}"
        );
        let unlimited = group(&full, key);
        for count in [
            "severity_critical_count",
            "severity_high_count",
            "severity_moderate_count",
            "hotspot_count",
        ] {
            assert_eq!(limited[count], unlimited[count], "{count} for {key}");
        }
        assert_eq!(limited["health_score"], unlimited["health_score"]);
    }
    // The project list stays globally truncated.
    assert_eq!(top["findings"].as_array().map_or(0, Vec::len), 1);
}

#[test]
fn snapshot_stores_groups_and_trend_from_compares_each_group() {
    let dir = project();
    let root = dir.path();
    let snapshot = root.join("baseline.json");
    let snapshot_arg = snapshot.display().to_string();
    let saved = health_json(root, &["--score", "--save-snapshot", &snapshot_arg]);
    assert!(saved.get("health_trend").is_none());

    let stored: Value =
        serde_json::from_str(&std::fs::read_to_string(&snapshot).expect("snapshot written"))
            .expect("snapshot JSON");
    assert_eq!(stored["snapshot_schema_version"], 11);
    assert_eq!(stored["groups"]["grouped_by"], "owner");
    let stored_keys: Vec<&str> = stored["groups"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["key"].as_str().unwrap())
        .collect();
    assert!(stored_keys.contains(&"@team/a") && stored_keys.contains(&"@team/b"));

    // Make team b worse, then compare against the stored file.
    std::fs::write(root.join("src/b/three.ts"), branchy("bThree", 40)).expect("write three");
    std::fs::write(
        root.join("src/index.ts"),
        "export * from './a/one';\nexport * from './a/two';\nexport * from './b/one';\nexport * from './b/two';\nexport * from './b/three';\n",
    )
    .expect("rewrite index");
    let trended = health_json(root, &["--trend-from", &snapshot_arg]);
    assert_eq!(trended["health_trend"]["snapshots_loaded"], 1);
    for key in ["@team/a", "@team/b"] {
        let group = group(&trended, key);
        assert_eq!(group["trend_status"], "compared", "{group:#}");
        assert!(group["trend"]["metrics"].is_array(), "{group:#}");
    }
    let b_score = group(&trended, "@team/b")["trend"]["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|metric| metric["name"] == "score")
        .cloned()
        .expect("score metric for team b");
    assert!(b_score["delta"].as_f64().unwrap() <= 0.0, "{b_score:#}");
}

#[test]
fn trend_from_an_ungrouped_snapshot_reports_no_group_baseline() {
    let dir = project();
    let root = dir.path();
    let snapshot = root.join("ungrouped.json");
    let snapshot_arg = snapshot.display().to_string();
    let saved = run_fallow_in_root(
        "health",
        root,
        &["--score", "--save-snapshot", &snapshot_arg, "--quiet"],
    );
    assert!(matches!(saved.code, 0 | 1), "stderr:\n{}", saved.stderr);

    let trended = health_json(root, &["--trend-from", &snapshot_arg]);
    assert!(
        trended["health_trend"].is_object(),
        "project trend still works"
    );
    for key in ["@team/a", "@team/b"] {
        let group = group(&trended, key);
        assert_eq!(group["trend_status"], "no_group_baseline");
        assert!(group.get("trend").is_none());
    }
    let diagnostic = trended["workspace_diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "trend-group-baseline-unavailable")
        .expect("diagnostic recorded");
    assert_eq!(diagnostic["cause"], "snapshot-has-no-groups");
    // `degrades_analysis` is omitted when false.
    assert!(
        diagnostic.get("degrades_analysis").is_none(),
        "{diagnostic:#}"
    );
}

#[test]
fn trend_from_a_filtered_snapshot_does_not_call_an_excluded_group_new() {
    let dir = project();
    let root = dir.path();
    let snapshot = root.join("filtered.json");
    let snapshot_arg = snapshot.display().to_string();
    health_json(
        root,
        &[
            "--score",
            "--group",
            "@team/a",
            "--save-snapshot",
            &snapshot_arg,
        ],
    );

    let trended = health_json(root, &["--trend-from", &snapshot_arg]);
    assert_eq!(group(&trended, "@team/a")["trend_status"], "compared");
    let excluded = group(&trended, "@team/b");
    assert_eq!(
        excluded["trend_status"], "no_group_baseline",
        "{excluded:#}"
    );
    assert!(excluded.get("trend").is_none(), "{excluded:#}");
}

#[test]
fn trend_from_a_filtered_snapshot_still_reports_a_new_matching_group() {
    let dir = project();
    let root = dir.path();
    let snapshot = root.join("glob.json");
    let snapshot_arg = snapshot.display().to_string();
    health_json(
        root,
        &[
            "--score",
            "--group",
            "@team/*",
            "--save-snapshot",
            &snapshot_arg,
        ],
    );

    // Add one group that the stored selector keeps and one that it drops.
    std::fs::write(
        root.join(".github/CODEOWNERS"),
        "/src/a/ @team/a\n/src/b/ @team/b\n/src/c/ @team/c\n/src/d/ @other/d\n",
    )
    .expect("rewrite CODEOWNERS");
    for team in ["c", "d"] {
        std::fs::create_dir_all(root.join("src").join(team)).expect("create team dir");
        std::fs::write(
            root.join(format!("src/{team}/one.ts")),
            branchy(&format!("{team}One"), 20),
        )
        .expect("write one");
    }
    std::fs::write(
        root.join("src/index.ts"),
        "export * from './a/one';\nexport * from './a/two';\nexport * from './b/one';\nexport * from './b/two';\nexport * from './c/one';\nexport * from './d/one';\n",
    )
    .expect("rewrite index");

    let trended = health_json(root, &["--trend-from", &snapshot_arg]);
    assert_eq!(group(&trended, "@team/a")["trend_status"], "compared");
    assert_eq!(group(&trended, "@team/c")["trend_status"], "new_group");
    assert_eq!(
        group(&trended, "@other/d")["trend_status"],
        "no_group_baseline"
    );
}

#[test]
fn human_group_table_shows_critical_and_trend_columns() {
    let dir = project();
    let root = dir.path();
    let snapshot = root.join("human-baseline.json");
    let snapshot_arg = snapshot.display().to_string();
    health_json(root, &["--score", "--save-snapshot", &snapshot_arg]);
    let output = run_fallow_in_root(
        "health",
        root,
        &["--group-by", "owner", "--trend-from", &snapshot_arg],
    );
    assert!(matches!(output.code, 0 | 1), "stderr:\n{}", output.stderr);
    let header = output
        .stdout
        .lines()
        .find(|line| line.contains("files") && line.contains("crit"))
        .unwrap_or_else(|| panic!("no group header:\n{}", output.stdout));
    for column in ["score", "grade", "trend", "files", "crit", "hot", "p90"] {
        assert!(header.contains(column), "missing {column}: {header}");
    }
    let row = output
        .stdout
        .lines()
        .find(|line| line.trim_start().starts_with("@team/a"))
        .expect("row for @team/a");
    assert!(
        row.contains("+0.0"),
        "unchanged code gives a zero delta: {row}"
    );
    // The grade column is padded, so `files` lines up with its header.
    let files_end = header[..header.find("files").unwrap() + "files".len()]
        .chars()
        .count();
    assert_eq!(
        row.chars().nth(files_end - 1),
        Some('2'),
        "files column misaligned:\n{header}\n{row}"
    );
}

#[test]
fn trend_from_a_missing_file_is_invalid_input() {
    let dir = project();
    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &["--trend-from", "does-not-exist.json", "--quiet"],
    );
    assert_eq!(output.code, 2, "stderr:\n{}", output.stderr);
    assert!(
        output
            .stderr
            .contains("failed to read --trend-from snapshot")
            || output
                .stdout
                .contains("failed to read --trend-from snapshot")
    );
}

#[test]
fn grouped_markdown_renders_the_group_table() {
    let dir = project();
    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &["--group-by", "owner", "--score", "--format", "markdown"],
    );
    assert!(matches!(output.code, 0 | 1), "stderr:\n{}", output.stderr);
    assert!(!output.stderr.contains("not supported for markdown output"));
    // The run shows only the score, so the project section must not claim
    // that no function exceeds a threshold.
    assert!(
        !output.stdout.contains("no functions exceed"),
        "{}",
        output.stdout
    );
    assert!(!output.stdout.contains("<details>"), "{}", output.stdout);
    insta::assert_snapshot!(
        "markdown_health_grouped",
        redact_all(&output.stdout, dir.path())
    );
}

#[test]
fn grouped_markdown_lists_group_findings_when_the_run_lists_findings() {
    let dir = project();
    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--group-by",
            "owner",
            "--complexity",
            "--format",
            "markdown",
        ],
    );
    assert!(matches!(output.code, 0 | 1), "stderr:\n{}", output.stderr);
    for key in ["@team/a", "@team/b"] {
        assert!(
            output.stdout.contains(&format!(
                "<summary><code>{key}</code>: 2 findings</summary>"
            )),
            "{}",
            output.stdout
        );
    }
}

#[test]
fn grouped_github_summary_renders_the_group_table() {
    let dir = project();
    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--group-by",
            "owner",
            "--group",
            "@team/b",
            "--score",
            "--format",
            "github-summary",
        ],
    );
    assert!(matches!(output.code, 0 | 1), "stderr:\n{}", output.stderr);
    assert!(
        !output
            .stderr
            .contains("not supported for github-summary output")
    );
    assert!(
        output.stdout.contains("\n\n## Health by owner"),
        "the group section needs a blank line before it:\n{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("No functions exceed"),
        "{}",
        output.stdout
    );
    let rendered: Vec<String> = redact_all(&output.stdout, dir.path())
        .lines()
        .map(redact_elapsed)
        .collect();
    insta::assert_snapshot!("github_summary_health_grouped", rendered.join("\n"));
}

/// A function that the clone detector finds again in each copy.
fn pair_clone(name: &str) -> String {
    format!(
        "export function {name}(items: number[]): number {{
  let total = 0;
  for (const item of items) {{
    if (item > 10) {{
      total += item * 2;
    }} else if (item < 0) {{
      total -= item;
    }} else {{
      total += item;
    }}
  }}
  const scaled = total * 3 + items.length;
  const shifted = scaled - Math.floor(scaled / 7);
  return shifted > 100 ? shifted - 100 : shifted;
}}
"
    )
}

/// A second clone with a different structure from [`pair_clone`].
fn trio_clone(name: &str) -> String {
    format!(
        "export function {name}(words: string[]): string {{
  const seen = new Set<string>();
  let out = '';
  while (words.length > 0) {{
    const word = words.pop();
    if (word === undefined || seen.has(word)) {{
      continue;
    }}
    seen.add(word);
    out = out.length > 0 ? `${{out}},${{word}}` : word;
  }}
  return out.toUpperCase().trim();
}}
"
    )
}

/// Two CODEOWNERS teams. One clone has one instance in each team. A second
/// clone has two instances in team a and one instance in team b.
fn cross_team_clone_project() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "cross-team-clone", "private": true, "main": "src/index.ts" }"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(root.join(".github")).expect("create .github");
    std::fs::write(
        root.join(".github/CODEOWNERS"),
        "/src/a/ @team/a\n/src/b/ @team/b\n",
    )
    .expect("write CODEOWNERS");
    let files = [
        ("a/pair", pair_clone("alphaPair")),
        ("b/pair", pair_clone("betaPair")),
        ("a/trio_one", trio_clone("alphaTrioOne")),
        ("a/trio_two", trio_clone("alphaTrioTwo")),
        ("b/trio", trio_clone("betaTrio")),
    ];
    let mut index = String::new();
    for (module, body) in files {
        let path = root.join("src").join(format!("{module}.ts"));
        std::fs::create_dir_all(path.parent().unwrap()).expect("create team dir");
        std::fs::write(path, body).expect("write clone");
        let _ = writeln!(index, "export * from './{module}';");
    }
    std::fs::write(root.join("src/index.ts"), index).expect("write index");
    dir
}

fn duplicated_lines(scope: &Value) -> u64 {
    scope["vital_signs"]["counts"]["duplicated_lines"]
        .as_u64()
        .unwrap_or_else(|| panic!("duplicated_lines missing: {scope:#}"))
}

/// A clone that spans two teams lowers the score of each team. Each team
/// counts only the lines of its own instances, so the group values sum to the
/// project value when the groups partition the files.
#[test]
fn cross_team_clone_counts_for_each_team_and_sums_to_the_project() {
    let dir = cross_team_clone_project();
    let envelope = health_json(
        dir.path(),
        &["--score", "--complexity", "--hotspots", "--report-only"],
    );
    let project_lines = duplicated_lines(&envelope);
    assert!(project_lines > 0, "fixture must give clones: {envelope:#}");
    let sum: u64 = envelope["groups"]
        .as_array()
        .expect("groups array")
        .iter()
        .map(duplicated_lines)
        .sum();
    assert_eq!(sum, project_lines, "{envelope:#}");
    for key in ["@team/a", "@team/b"] {
        let group = group(&envelope, key);
        assert!(duplicated_lines(group) > 0, "{key}: {group:#}");
        let penalty = group["health_score"]["penalties"]["duplication"]
            .as_f64()
            .unwrap();
        assert!(penalty > 0.0, "{key}: {group:#}");
    }
}
