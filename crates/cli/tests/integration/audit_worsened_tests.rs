//! `audit --gate new-only` classes of complexity findings: introduced,
//! worsened and inherited.
//!
//! A worsened finding matches its base finding, is not introduced, and has a
//! strictly higher measured value (cyclomatic, cognitive, CRAP or line count).
//! `--worsened warn` (the default) gives a `warn` verdict for it. `--worsened
//! error` and `audit.worsened: "error"` count it like an introduced finding.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{commit_all, git, parse_json, run_fallow_raw, run_fallow_raw_with_env};

const CONFIG: &str = r#"{"entry": ["src/*.js"], "health": {"maxCyclomatic": 15, "maxCognitive": 15, "maxCrap": 999999999, "maxUnitSize": 999999}}"#;

/// A function with `branches` early returns and `padding` plain statements:
/// cyclomatic `branches + 1`, cognitive `branches`.
fn hotspot(name: &str, branches: usize, padding: usize) -> String {
    let mut source = format!("export function {name}(x) {{\n  let total = 0;\n");
    for line in 0..padding {
        writeln!(source, "  total += {line};").unwrap();
    }
    for branch in 0..branches {
        writeln!(source, "  if (x === {branch}) return {branch} + total;").unwrap();
    }
    source.push_str("  return -1;\n}\n");
    source
}

/// A simple function that never reaches a threshold.
fn simple(name: &str) -> String {
    format!("export function {name}(x) {{\n  return x + 1;\n}}\n")
}

/// A repository with `src/index.js` holding `base` on `main`, and a `head`
/// branch that holds `head`, committed.
fn fixture(base: &str, head: &str, config: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-worsened", "private": true}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), config).unwrap();
    fs::write(dir.join("src/index.js"), base).unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    fs::write(dir.join("src/index.js"), head).unwrap();
    commit_all(dir, "head");
    tmp
}

struct Run {
    code: i32,
    json: serde_json::Value,
}

impl Run {
    fn verdict(&self) -> &str {
        self.json["verdict"].as_str().unwrap()
    }

    fn finding(&self, name: &str) -> &serde_json::Value {
        self.json["complexity"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == name)
            .unwrap_or_else(|| panic!("no finding named {name}: {:#}", self.json))
    }

    fn worsened_count(&self) -> u64 {
        self.json["attribution"]["complexity_worsened"]
            .as_u64()
            .unwrap()
    }
}

fn audit(dir: &Path, extra: &[&str]) -> Run {
    let mut args = vec![
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--no-css",
        "--skip",
        "dupes",
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_raw(&args);
    assert!(
        output.code == 0 || output.code == 1,
        "audit failed. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    Run {
        code: output.code,
        json: parse_json(&output),
    }
}

#[test]
fn grown_function_is_worsened_and_warns_by_default() {
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 20, 40), CONFIG);
    let run = audit(tmp.path(), &[]);
    let finding = run.finding("work");
    assert_eq!(finding["attribution"], "worsened", "{:#}", run.json);
    assert_eq!(finding["introduced"], false);
    let metrics = finding["worsened_metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 1, "{:#}", run.json);
    assert_eq!(metrics[0]["metric"], "line_count");
    assert!(metrics[0]["head"].as_u64().unwrap() > metrics[0]["base"].as_u64().unwrap());
    assert_eq!(run.worsened_count(), 1);
    assert_eq!(run.json["attribution"]["complexity_inherited"], 1);
    assert_eq!(run.verdict(), "warn");
    assert_eq!(run.code, 0, "a worsened finding must not fail by default");
}

#[test]
fn grown_function_fails_with_worsened_error() {
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 20, 40), CONFIG);
    let run = audit(tmp.path(), &["--worsened", "error"]);
    assert_eq!(run.finding("work")["attribution"], "worsened");
    assert_eq!(run.verdict(), "fail", "{:#}", run.json);
    assert_eq!(run.code, 1);
}

#[test]
fn worsened_off_keeps_the_grown_function_inherited_and_the_verdict_pass() {
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 20, 40), CONFIG);
    let run = audit(tmp.path(), &["--worsened", "off"]);
    let finding = run.finding("work");
    assert_eq!(finding["attribution"], "inherited", "{:#}", run.json);
    assert!(
        finding
            .get("worsened_metrics")
            .is_none_or(|metrics| metrics.as_array().unwrap().is_empty())
    );
    assert_eq!(run.json["attribution"]["worsened"], "off");
    assert_eq!(
        run.json["attribution"]["complexity_worsened"]
            .as_u64()
            .unwrap_or(0),
        0
    );
    assert_eq!(run.verdict(), "pass");
    assert_eq!(run.code, 0);
}

#[test]
fn config_worsened_off_is_accepted() {
    let config = CONFIG.replacen('{', r#"{"audit": {"worsened": "off"}, "#, 1);
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 20, 40), &config);
    let run = audit(tmp.path(), &[]);
    assert_eq!(
        run.finding("work")["attribution"],
        "inherited",
        "{:#}",
        run.json
    );
    assert_eq!(run.verdict(), "pass");
}

#[test]
fn config_worsened_error_gates_and_the_flag_overrides_it() {
    let config = r#"{"entry": ["src/*.js"], "audit": {"worsened": "error"}, "health": {"maxCyclomatic": 15, "maxCognitive": 15, "maxCrap": 999999999, "maxUnitSize": 999999}}"#;
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 20, 40), config);
    assert_eq!(audit(tmp.path(), &[]).code, 1);
    let overridden = audit(tmp.path(), &["--worsened", "warn"]);
    assert_eq!(overridden.verdict(), "warn");
    assert_eq!(overridden.code, 0);
}

#[test]
fn improved_function_stays_inherited() {
    let tmp = fixture(&hotspot("work", 22, 10), &hotspot("work", 18, 4), CONFIG);
    for mode in ["warn", "error"] {
        let run = audit(tmp.path(), &["--worsened", mode]);
        let finding = run.finding("work");
        assert_eq!(finding["attribution"], "inherited", "{:#}", run.json);
        assert!(finding.get("worsened_metrics").is_none(), "{:#}", run.json);
        assert_eq!(run.worsened_count(), 0);
        assert_eq!(run.verdict(), "pass");
        assert_eq!(run.code, 0);
    }
}

#[test]
fn moved_unchanged_function_stays_inherited() {
    let base = hotspot("work", 20, 5);
    let head = format!("{}{}{}", simple("a"), simple("b"), base);
    let tmp = fixture(&base, &head, CONFIG);
    let run = audit(tmp.path(), &["--worsened", "error"]);
    let finding = run.finding("work");
    assert!(finding["line"].as_u64().unwrap() > 1, "the function moved");
    assert_eq!(finding["attribution"], "inherited", "{:#}", run.json);
    assert!(finding.get("worsened_metrics").is_none());
    assert_eq!(run.verdict(), "pass");
}

#[test]
fn new_function_stays_introduced() {
    let base = hotspot("work", 20, 2);
    let head = format!("{base}{}", hotspot("fresh", 20, 40));
    let tmp = fixture(&base, &head, CONFIG);
    let run = audit(tmp.path(), &[]);
    let fresh = run.finding("fresh");
    assert_eq!(fresh["attribution"], "introduced");
    assert_eq!(fresh["introduced"], true);
    assert!(fresh.get("worsened_metrics").is_none(), "no base value");
    assert_eq!(run.finding("work")["attribution"], "inherited");
    assert_eq!(run.worsened_count(), 0);
    assert_eq!(run.verdict(), "fail");
}

#[test]
fn growth_of_an_exceeded_metric_stays_introduced_and_shows_values() {
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 25, 2), CONFIG);
    let run = audit(tmp.path(), &[]);
    let finding = run.finding("work");
    assert_eq!(finding["attribution"], "introduced", "{:#}", run.json);
    assert_eq!(finding["introduced"], true);
    let cyclomatic = finding["worsened_metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|metric| metric["metric"] == "cyclomatic")
        .expect("cyclomatic grew");
    assert_eq!(cyclomatic["base"], 21);
    assert_eq!(cyclomatic["head"], 26);
    assert_eq!(run.worsened_count(), 0);
    assert_eq!(
        run.verdict(),
        "fail",
        "the 3.33 rule still gates in warn mode"
    );
}

#[test]
fn gate_all_has_no_attribution_classes() {
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 20, 40), CONFIG);
    let run = audit(tmp.path(), &["--gate", "all"]);
    assert!(run.finding("work").get("attribution").is_none());
    assert_eq!(run.worsened_count(), 0);
}

#[test]
fn human_compact_and_markdown_name_the_grown_values() {
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 20, 40), CONFIG);
    let dir = tmp.path().to_str().unwrap();
    let base = [
        "audit",
        "--root",
        dir,
        "--base",
        "main",
        "--no-cache",
        "--no-css",
        "--skip",
        "dupes",
    ];
    let compact = run_fallow_raw(&[&base[..], &["--format", "compact"]].concat());
    assert!(
        compact
            .stdout
            .lines()
            .any(|line| line.starts_with("worse-than-base:src/index.js:")
                && line.contains(":work:attribution=worsened,line_count=")),
        "compact: {}",
        compact.stdout
    );
    let markdown = run_fallow_raw(&[&base[..], &["--format", "markdown"]].concat());
    assert!(
        markdown.stdout.contains("### Worse than base"),
        "{}",
        markdown.stdout
    );
    assert!(markdown.stdout.contains("| `work` | worsened | lines "));
    let human = run_fallow_raw(&[&base[..], &["--format", "human"]].concat());
    let all = format!("{}{}", human.stdout, human.stderr);
    assert!(all.contains("work  lines "), "{all}");
    assert!(
        all.contains("1 inherited complexity finding got worse since main"),
        "{all}"
    );
}

#[test]
fn sarif_and_codeclimate_carry_the_attribution() {
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 20, 40), CONFIG);
    let dir = tmp.path().to_str().unwrap();
    let base = [
        "audit",
        "--root",
        dir,
        "--base",
        "main",
        "--no-cache",
        "--no-css",
        "--skip",
        "dupes",
        "--quiet",
    ];
    let sarif = parse_json(&run_fallow_raw(
        &[&base[..], &["--format", "sarif"]].concat(),
    ));
    let result = sarif["runs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|run| run["results"].as_array().unwrap())
        .find(|result| result["properties"]["attribution"] == "worsened")
        .unwrap_or_else(|| panic!("no worsened SARIF result: {sarif:#}"));
    assert_eq!(
        result["properties"]["worsened_metrics"][0]["metric"],
        "line_count"
    );
    let codeclimate = parse_json(&run_fallow_raw(
        &[&base[..], &["--format", "codeclimate"]].concat(),
    ));
    assert!(
        codeclimate
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["description"]
                .as_str()
                .unwrap()
                .contains("(worsened against the base: lines ")),
        "{codeclimate:#}"
    );
}

/// `hotspot` with `comments` comment lines at the top of the body.
fn commented_hotspot(name: &str, branches: usize, padding: usize, comments: usize) -> String {
    let source = hotspot(name, branches, padding);
    let (head, body) = source.split_once('\n').unwrap();
    let mut out = format!("{head}\n");
    for line in 0..comments {
        writeln!(out, "  // note {line}").unwrap();
    }
    out.push_str(body);
    out
}

#[test]
fn comment_only_edit_stays_inherited() {
    // Comments count in the physical line count. A small edit stays below
    // the line growth minimum, so the finding stays inherited.
    let base = hotspot("work", 20, 40);
    let head = commented_hotspot("work", 20, 40, 3);
    let tmp = fixture(&base, &head, CONFIG);
    let run = audit(tmp.path(), &["--worsened", "error"]);
    let finding = run.finding("work");
    assert!(
        finding["line_count"].as_u64().unwrap() > 60,
        "the comment lines count: {:#}",
        run.json
    );
    assert_eq!(finding["attribution"], "inherited", "{:#}", run.json);
    assert_eq!(run.worsened_count(), 0);
    assert_eq!(run.verdict(), "pass");
    assert_eq!(run.code, 0);
}

#[test]
fn attribution_publishes_the_worsened_mode() {
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 20, 40), CONFIG);
    assert_eq!(
        audit(tmp.path(), &[]).json["attribution"]["worsened"],
        "warn"
    );
    let run = audit(tmp.path(), &["--worsened", "error"]);
    assert_eq!(run.json["attribution"]["worsened"], "error");
    assert_eq!(run.json["attribution"]["complexity_introduced"], 0);
    assert_eq!(run.worsened_count(), 1);
    assert_eq!(run.verdict(), "fail");
}

#[test]
fn report_from_a_saved_run_keeps_the_worsened_values() {
    let tmp = fixture(&hotspot("work", 20, 2), &hotspot("work", 20, 40), CONFIG);
    let dir = tmp.path();
    let run = audit(dir, &[]);
    let saved = dir.join("saved-audit.json");
    fs::write(&saved, serde_json::to_string(&run.json).unwrap()).unwrap();
    let report = |format: &str| {
        parse_json(&run_fallow_raw(&[
            "report",
            "--from",
            saved.to_str().unwrap(),
            "--root",
            dir.to_str().unwrap(),
            "--format",
            format,
            "--quiet",
        ]))
    };
    let sarif = report("sarif");
    let result = sarif["runs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|run| run["results"].as_array().unwrap())
        .find(|result| result["properties"]["attribution"] == "worsened")
        .unwrap_or_else(|| panic!("no worsened SARIF result: {sarif:#}"));
    assert_eq!(
        result["properties"]["worsened_metrics"][0]["metric"],
        "line_count"
    );
    let codeclimate = report("codeclimate");
    assert!(
        codeclimate
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["description"]
                .as_str()
                .unwrap()
                .contains("(worsened against the base: lines ")),
        "{codeclimate:#}"
    );
    let summary = run_fallow_raw(&[
        "report",
        "--from",
        saved.to_str().unwrap(),
        "--root",
        dir.to_str().unwrap(),
        "--format",
        "github-summary",
        "--quiet",
    ])
    .stdout;
    assert!(summary.contains("| `work` | worsened |"), "{summary}");
    assert!(
        summary.contains("1 inherited complexity finding got worse than the base"),
        "{summary}"
    );
}

/// A feature branch merges the default branch, and the default branch grew
/// `work` after the merge base. The merge brings the grown function in, so it
/// is inherited from the merged commit, not worsened.
#[test]
fn growth_that_a_merge_brings_in_from_main_is_not_worsened() {
    let base = hotspot("work", 20, 2);
    let tmp = fixture(&base, &format!("{base}{}", simple("unused")), CONFIG);
    let dir = tmp.path();
    git(dir, &["checkout", "-q", "main"]);
    git(dir, &["branch", "-D", "head"]);
    git(dir, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(
        dir,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(dir, &["checkout", "-q", "-b", "feature"]);
    fs::write(dir.join("src/other.js"), simple("other")).unwrap();
    commit_all(dir, "feature: other work");
    git(dir, &["checkout", "-q", "main"]);
    fs::write(dir.join("src/index.js"), hotspot("work", 20, 40)).unwrap();
    commit_all(dir, "main: work grew");
    git(dir, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(dir, &["checkout", "-q", "feature"]);
    git(dir, &["merge", "--no-commit", "--no-ff", "origin/main"]);

    let output = run_fallow_raw_with_env(
        &[
            "audit",
            "--root",
            dir.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--no-css",
            "--skip",
            "dupes",
            "--worsened",
            "error",
        ],
        &[("FALLOW_AUDIT_BASE", "")],
    );
    let json = parse_json(&output);
    assert_eq!(
        json["base_description"], "merge-base with origin/main",
        "{json:#}"
    );
    let finding = json["complexity"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "work")
        .unwrap_or_else(|| panic!("no work finding: {json:#}"));
    assert_eq!(finding["attribution"], "inherited", "{json:#}");
    assert_eq!(json["attribution"]["complexity_worsened"], 0);
    assert_eq!(json["verdict"], "pass", "{json:#}");
}

/// A path with a pipe must stay inside its Markdown table cell. Windows does
/// not allow `|` in a file name, so this test runs on Unix only.
#[cfg(unix)]
#[test]
fn markdown_row_escapes_a_pipe_in_the_path() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-worsened", "private": true}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), CONFIG).unwrap();
    fs::write(dir.join("src/a|b.js"), hotspot("work", 20, 2)).unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    fs::write(dir.join("src/a|b.js"), hotspot("work", 20, 40)).unwrap();
    commit_all(dir, "grow");
    let root = dir.to_str().unwrap();
    let markdown = run_fallow_raw(&[
        "audit",
        "--root",
        root,
        "--base",
        "main",
        "--no-cache",
        "--no-css",
        "--skip",
        "dupes",
        "--format",
        "markdown",
    ]);
    let row = markdown
        .stdout
        .lines()
        .find(|line| line.contains("worsened | lines"))
        .unwrap_or_else(|| panic!("no worsened row: {}", markdown.stdout));
    assert!(row.contains(r"src/a\|b.js:"), "{row}");
    assert_eq!(row.matches(" | ").count(), 3, "{row}");
}
