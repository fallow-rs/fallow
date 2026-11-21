//! `fallow-override-threshold-next-line` comments in `fallow health`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow_in_root};
use std::path::Path;
use tempfile::tempdir;

const MARKER: &str = "fallow-override-threshold-next-line";

/// A body with cyclomatic 7 and cognitive 6.
const BODY: &str = "  let score = 0;
  if (input > 0) score += 1;
  if (input > 1) score += 1;
  if (input > 2) score += 1;
  if (input > 3) score += 1;
  if (input > 4) score += 1;
  if (input > 5) score += 1;
  return score;
";

fn write_project(root: &Path, config: &str, source: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"inline-threshold","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(root.join(".fallowrc.json"), config).unwrap();
    std::fs::write(root.join("src/index.ts"), source).unwrap();
}

fn health(root: &Path, extra: &[&str]) -> (i32, serde_json::Value, String) {
    let mut args = vec![
        "--complexity",
        "--max-cyclomatic",
        "3",
        "--max-cognitive",
        "3",
        "--max-crap",
        "10000",
    ];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["--format", "json", "--quiet"]);
    let output = run_fallow_in_root("health", root, &args);
    let json = parse_json(&output);
    (output.code, json, output.stdout)
}

fn finding_names(json: &serde_json::Value) -> Vec<String> {
    json["findings"]
        .as_array()
        .map(|findings| {
            findings
                .iter()
                .filter_map(|f| f["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn inline_rows(json: &serde_json::Value) -> Vec<serde_json::Value> {
    json["inline_threshold_overrides"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn function_source(header: &str, comment: &str, name: &str) -> String {
    format!("{header}{comment}\nexport function {name}(input: number): number {{\n{BODY}}}\n")
}

#[test]
fn comment_raises_the_ceilings_of_the_next_function() {
    let dir = tempdir().unwrap();
    let source = format!(
        "{}{}",
        function_source(
            "",
            &format!("// {MARKER} maxCyclomatic=10 maxCognitive=10 -- legacy parser"),
            "legacyParse"
        ),
        function_source("", "", "plainParse"),
    );
    write_project(dir.path(), "{}", &source);

    let (code, json, stdout) = health(dir.path(), &[]);
    assert_eq!(code, 1, "plainParse still fails: {stdout}");
    assert_eq!(
        finding_names(&json),
        vec!["plainParse".to_string()],
        "{stdout}"
    );
    let rows = inline_rows(&json);
    let [row] = rows.as_slice() else {
        panic!("one inline row expected: {stdout}");
    };
    assert_eq!(row["status"], "active");
    assert_eq!(row["dimension"], "complexity");
    assert_eq!(row["path"], "src/index.ts");
    assert_eq!(row["function"], "legacyParse");
    assert_eq!(row["comment_line"], 1);
    assert_eq!(row["line"], 2);
    assert_eq!(row["reason"], "legacy parser");
    assert_eq!(row["effective_thresholds"]["max_cognitive"], 10);
    assert_eq!(row["metrics"]["cognitive"], 6);
    assert!(
        json["threshold_overrides"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "comment rows stay out of the config array: {stdout}"
    );
}

#[test]
fn comment_below_the_current_score_is_insufficient() {
    let dir = tempdir().unwrap();
    let source = function_source(
        "",
        &format!("// {MARKER} maxCyclomatic=5 maxCognitive=5 -- cap at the agreed score"),
        "grows",
    );
    write_project(dir.path(), "{}", &source);

    let (code, json, stdout) = health(dir.path(), &[]);
    assert_eq!(code, 1, "{stdout}");
    assert_eq!(finding_names(&json), vec!["grows".to_string()]);
    let finding = &json["findings"][0];
    assert_eq!(finding["threshold_source"], "override", "{stdout}");
    assert_eq!(finding["effective_thresholds"]["max_cognitive"], 5);
    let rows = inline_rows(&json);
    assert_eq!(rows.len(), 1, "{stdout}");
    assert_eq!(rows[0]["status"], "insufficient");
    assert_eq!(rows[0]["outstanding"], serde_json::json!(["complexity"]));
}

#[test]
fn comment_on_a_simple_function_is_stale() {
    let dir = tempdir().unwrap();
    let source = format!(
        "// {MARKER} maxCognitive=40 -- old hotspot\nexport function small(a: number): number {{\n  return a;\n}}\n"
    );
    write_project(dir.path(), "{}", &source);

    let (code, json, stdout) = health(dir.path(), &[]);
    assert_eq!(code, 0, "{stdout}");
    let rows = inline_rows(&json);
    assert_eq!(rows.len(), 1, "{stdout}");
    assert_eq!(rows[0]["status"], "stale");
}

#[test]
fn comment_without_a_function_is_no_match() {
    let dir = tempdir().unwrap();
    let source = format!("// {MARKER} maxCognitive=40 -- moved away\nexport const value = 1;\n");
    write_project(dir.path(), "{}", &source);

    let (_, json, stdout) = health(dir.path(), &[]);
    let rows = inline_rows(&json);
    assert_eq!(rows.len(), 1, "{stdout}");
    assert_eq!(rows[0]["status"], "no_match");
    assert_eq!(rows[0]["comment_line"], 1);
    assert!(rows[0].get("function").is_none());
}

#[test]
fn comment_with_a_problem_has_no_effect() {
    let dir = tempdir().unwrap();
    let source = function_source("", &format!("// {MARKER} maxCognitive=40"), "noReason");
    write_project(dir.path(), "{}", &source);

    let (code, json, stdout) = health(dir.path(), &[]);
    assert_eq!(code, 1, "{stdout}");
    assert_eq!(finding_names(&json), vec!["noReason".to_string()]);
    assert!(inline_rows(&json).is_empty(), "{stdout}");
    let problems = json["inline_threshold_override_problems"]
        .as_array()
        .expect("problem array");
    assert_eq!(problems.len(), 1, "{stdout}");
    assert_eq!(problems[0]["path"], "src/index.ts");
    assert_eq!(problems[0]["line"], 1);
    assert_eq!(problems[0]["problem"], "missing_reason");
}

#[test]
fn config_switch_turns_the_comment_form_off() {
    let dir = tempdir().unwrap();
    let source = function_source(
        "",
        &format!("// {MARKER} maxCyclomatic=10 maxCognitive=10 -- legacy"),
        "legacyParse",
    );
    write_project(
        dir.path(),
        r#"{"health":{"inlineThresholdOverrides":false}}"#,
        &source,
    );

    let (code, json, stdout) = health(dir.path(), &[]);
    assert_eq!(code, 1, "{stdout}");
    assert_eq!(finding_names(&json), vec!["legacyParse".to_string()]);
    assert!(inline_rows(&json).is_empty(), "{stdout}");
    let problems = json["inline_threshold_override_problems"]
        .as_array()
        .expect("problem array");
    assert_eq!(problems[0]["problem"], "disabled", "{stdout}");
}

#[test]
fn comment_wins_over_a_config_entry_for_the_metrics_it_sets() {
    let dir = tempdir().unwrap();
    let source = function_source(
        "",
        &format!("// {MARKER} maxCognitive=4 -- tighter than the config"),
        "capped",
    );
    write_project(
        dir.path(),
        r#"{"health":{"thresholdOverrides":[{"files":["src/index.ts"],"maxCyclomatic":50,"maxCognitive":50}]}}"#,
        &source,
    );

    let (code, json, stdout) = health(dir.path(), &[]);
    assert_eq!(code, 1, "{stdout}");
    let finding = &json["findings"][0];
    assert_eq!(finding["name"], "capped", "{stdout}");
    assert_eq!(finding["effective_thresholds"]["max_cognitive"], 4);
    assert_eq!(finding["effective_thresholds"]["max_cyclomatic"], 50);
    assert_eq!(finding["exceeded"], "cognitive");
}

#[test]
fn comment_and_ignore_comment_share_the_anchor_of_an_export_wrapper() {
    for (wrapper, name) in [
        ("export default function", "wrapped"),
        ("export async function", "wrapped"),
    ] {
        let dir = tempdir().unwrap();
        let source = format!(
            "// {MARKER} maxCyclomatic=10 maxCognitive=10 -- wrapper\n{wrapper} {name}(input: number): Promise<number> | number {{\n{BODY}}}\n"
        );
        write_project(dir.path(), "{}", &source);
        let (code, json, stdout) = health(dir.path(), &[]);
        assert_eq!(code, 0, "{wrapper}: {stdout}");
        let rows = inline_rows(&json);
        assert_eq!(rows.len(), 1, "{wrapper}: {stdout}");
        assert_eq!(rows[0]["status"], "active", "{wrapper}: {stdout}");

        let ignored = source.replace(
            &format!("{MARKER} maxCyclomatic=10 maxCognitive=10 -- wrapper"),
            "fallow-ignore-next-line complexity",
        );
        write_project(dir.path(), "{}", &ignored);
        let (code, _, stdout) = health(dir.path(), &[]);
        assert_eq!(
            code, 0,
            "the ignore comment anchors the same way ({wrapper}): {stdout}"
        );
    }
}

/// A decorated method starts at its name, not at the decorator, so both
/// comment forms go between the decorator and the method name. Above the
/// decorator, neither form applies.
#[test]
fn comment_and_ignore_comment_share_the_anchor_of_a_decorated_method() {
    let override_comment = format!("// {MARKER} maxCyclomatic=10 maxCognitive=10 -- decorated");
    let ignore_comment = "// fallow-ignore-next-line complexity";
    for (comment_above_decorator, expected_code) in [(false, 0), (true, 1)] {
        let source_with = |comment: &str| {
            let (above, below) = if comment_above_decorator {
                (format!("  {comment}\n"), String::new())
            } else {
                (String::new(), format!("  {comment}\n"))
            };
            format!(
                "function log(_t: unknown, _k: string): void {{}}\nexport class Parser {{\n{above}  @log\n{below}  run(input: number): number {{\n{BODY}  }}\n}}\n"
            )
        };
        let dir = tempdir().unwrap();
        write_project(dir.path(), "{}", &source_with(&override_comment));
        let (code_override, json, stdout) = health(dir.path(), &[]);
        write_project(dir.path(), "{}", &source_with(ignore_comment));
        let (code_ignore, _, ignore_stdout) = health(dir.path(), &[]);

        assert_eq!(
            code_override, code_ignore,
            "both comment forms anchor on the same line: {stdout}\n{ignore_stdout}"
        );
        assert_eq!(code_override, expected_code, "{stdout}");
        let expected_status = if comment_above_decorator {
            "no_match"
        } else {
            "active"
        };
        assert_eq!(inline_rows(&json)[0]["status"], expected_status, "{stdout}");
    }
}

#[test]
fn human_output_names_the_comment_as_the_source() {
    let dir = tempdir().unwrap();
    let source = function_source(
        "",
        &format!("// {MARKER} maxCyclomatic=5 maxCognitive=5 -- agreed cap"),
        "grows",
    );
    write_project(dir.path(), "{}", &source);
    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--quiet",
        ],
    );
    assert!(
        output.stdout.contains("comment, line 1"),
        "human output names the comment: {}",
        output.stdout
    );
}

#[test]
fn comment_sets_the_crap_ceiling() {
    let dir = tempdir().unwrap();
    let source = function_source(
        "",
        &format!("// {MARKER} maxCyclomatic=10 maxCognitive=10 maxCrap=100 -- no tests yet"),
        "untested",
    );
    write_project(dir.path(), "{}", &source);
    let args = [
        "--complexity",
        "--max-cyclomatic",
        "3",
        "--max-cognitive",
        "3",
        "--max-crap",
        "30",
        "--format",
        "json",
        "--quiet",
    ];
    let output = run_fallow_in_root("health", dir.path(), &args);
    let json = parse_json(&output);
    assert_eq!(output.code, 0, "{}", output.stdout);
    let rows = inline_rows(&json);
    let crap_row = rows
        .iter()
        .find(|row| row["dimension"] == "crap")
        .expect("crap row");
    assert_eq!(crap_row["status"], "active", "{}", output.stdout);
    assert!(
        crap_row["metrics"]["crap"]
            .as_f64()
            .is_some_and(|crap| crap >= 30.0)
    );

    let lowered = source.replace("maxCrap=100", "maxCrap=40");
    write_project(dir.path(), "{}", &lowered);
    let output = run_fallow_in_root("health", dir.path(), &args);
    let json = parse_json(&output);
    assert_eq!(output.code, 1, "{}", output.stdout);
    assert_eq!(
        json["findings"][0]["override_comment_line"], 1,
        "{}",
        output.stdout
    );
    assert_eq!(json["findings"][0]["exceeded"], "crap");
}
