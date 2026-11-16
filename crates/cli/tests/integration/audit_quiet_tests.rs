//! `audit --quiet` keeps only the actionable part of the human, compact and
//! markdown reports: introduced and worsened findings, plus configuration,
//! analysis and incomplete-analysis warnings. It drops unchanged inherited
//! findings, aggregate metrics and the `stale` and `active` records of
//! `health.thresholdOverrides`. It keeps the override records that need a
//! configuration change (`insufficient` and `no_match`). The verdict, the
//! exit code and the JSON report do not change.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_raw};

const CONFIG: &str = r#"{
  "entry": ["src/index.js"],
  "health": {
    "maxCyclomatic": 15,
    "maxCognitive": 15,
    "maxCrap": 999999999,
    "thresholdOverrides": [
      { "files": ["src/index.js"], "functions": ["allowed"], "maxCyclomatic": 50, "maxCognitive": 50 },
      { "files": ["src/index.js"], "functions": ["calm"], "maxCyclomatic": 50, "maxCognitive": 50 },
      { "files": ["src/index.js"], "functions": ["fresh"], "maxCyclomatic": 18, "maxCognitive": 18 }
    ]
  }
}"#;

/// A function with `branches` early returns: cyclomatic `branches + 1`.
fn hotspot(name: &str, branches: usize) -> String {
    let mut source = format!("export function {name}(x) {{\n  let total = 0;\n");
    for branch in 0..branches {
        writeln!(source, "  if (x === {branch}) return {branch} + total;").unwrap();
    }
    source.push_str("  return -1;\n}\n");
    source
}

fn simple(name: &str) -> String {
    format!("export function {name}(x) {{\n  return x + 1;\n}}\n")
}

/// A repository where `head` adds the complex function `fresh` and the unused
/// export `unusedNew`. The complex function `legacy` and the unused export
/// `unusedOld` exist on `main` already. Both changed files stay in scope.
fn fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-quiet", "private": true}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), CONFIG).unwrap();
    let base_index = format!(
        "import {{ used }} from './lib.js';\nused();\n{}{}{}",
        hotspot("legacy", 20),
        hotspot("allowed", 20),
        simple("calm"),
    );
    fs::write(dir.join("src/index.js"), &base_index).unwrap();
    fs::write(
        dir.join("src/lib.js"),
        "export const used = () => 1;\nexport const unusedOld = () => 2;\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    fs::write(
        dir.join("src/index.js"),
        format!("{base_index}{}", hotspot("fresh", 20)),
    )
    .unwrap();
    fs::write(
        dir.join("src/lib.js"),
        "export const used = () => 1;\nexport const unusedOld = () => 2;\nexport const unusedNew = () => 3;\n",
    )
    .unwrap();
    commit_all(dir, "head");
    tmp
}

fn audit(dir: &Path, format: &str, quiet: bool) -> CommandOutput {
    let mut args = vec![
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--format",
        format,
        "--no-cache",
        "--no-css",
        "--skip",
        "dupes",
    ];
    if quiet {
        args.push("--quiet");
    }
    run_fallow_raw(&args)
}

#[test]
fn quiet_compact_keeps_only_introduced_findings() {
    let tmp = fixture();
    let full = audit(tmp.path(), "compact", false);
    let quiet = audit(tmp.path(), "compact", true);

    // The full report holds every record that the quiet report drops.
    for record in [":legacy", "unusedOld", "threshold-override:", "vital-signs"] {
        assert!(
            full.stdout.contains(record),
            "full compact report lacks {record}:\n{}",
            full.stdout
        );
    }

    for record in [
        ":fresh",
        "unusedNew",
        "threshold-override:2:complexity:insufficient",
    ] {
        assert!(
            quiet.stdout.contains(record),
            "quiet compact report lacks {record}:\n{}",
            quiet.stdout
        );
    }
    for record in [
        ":legacy",
        ":allowed",
        "unusedOld",
        ":active:",
        ":stale:",
        "vital-signs",
        "cyclomatic-population",
        "health-score",
    ] {
        assert!(
            !quiet.stdout.contains(record),
            "quiet compact report keeps {record}:\n{}",
            quiet.stdout
        );
    }

    assert_eq!(full.code, 1, "introduced findings fail the new-only gate");
    assert_eq!(
        quiet.code, full.code,
        "--quiet must not change the exit code"
    );
}

#[test]
fn quiet_human_keeps_only_introduced_findings() {
    let tmp = fixture();
    let full = audit(tmp.path(), "human", false);
    let quiet = audit(tmp.path(), "human", true);

    assert!(full.stdout.contains("legacy"), "{}", full.stdout);
    assert!(full.stdout.contains("unusedOld"), "{}", full.stdout);
    assert!(quiet.stdout.contains("fresh"), "{}", quiet.stdout);
    assert!(quiet.stdout.contains("unusedNew"), "{}", quiet.stdout);
    assert!(quiet.stdout.contains("insufficient"), "{}", quiet.stdout);
    for text in [
        "legacy",
        "unusedOld",
        " active ",
        " stale ",
        "allowed",
        "Metrics:",
    ] {
        assert!(
            !quiet.stdout.contains(text),
            "quiet human report keeps {text}:\n{}",
            quiet.stdout
        );
    }
    assert_eq!(
        quiet.code, full.code,
        "--quiet must not change the exit code"
    );
}

#[test]
fn quiet_keeps_the_json_report_complete() {
    let tmp = fixture();
    let full = audit(tmp.path(), "json", false);
    let quiet = audit(tmp.path(), "json", true);
    let full_json = parse_json(&full);
    let quiet_json = parse_json(&quiet);
    for section in [
        &["dead_code", "unused_exports"][..],
        &["complexity", "findings"],
        &["complexity", "threshold_overrides"],
        &["complexity", "vital_signs"],
    ] {
        let pick = |json: &serde_json::Value| {
            section
                .iter()
                .fold(json.clone(), |value, key| value[*key].clone())
        };
        assert!(
            !pick(&quiet_json).is_null(),
            "missing {section:?}: {quiet_json:#}"
        );
        assert_eq!(pick(&quiet_json), pick(&full_json), "{section:?} differs");
    }
    assert_eq!(quiet.code, full.code);
}

/// A repository with `base` files committed on `main` and `head` files
/// committed on the `head` branch. A `head` entry replaces or adds a file.
fn repo(config: &str, base: &[(&str, String)], head: &[(&str, String)]) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-quiet", "private": true}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), config).unwrap();
    for (path, content) in base {
        fs::write(dir.join(path), content).unwrap();
    }
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    for (path, content) in head {
        fs::write(dir.join(path), content).unwrap();
    }
    commit_all(dir, "head");
    tmp
}

fn audit_args(dir: &Path, args: &[&str]) -> CommandOutput {
    let mut all = vec![
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--no-cache",
    ];
    all.extend_from_slice(args);
    run_fallow_raw(&all)
}

const LEGACY_CONFIG: &str = r#"{
  "entry": ["src/*.js"],
  "health": { "maxCyclomatic": 15, "maxCognitive": 15, "maxCrap": 999999999 }
}"#;

/// Only a comment changes on `head`, so the complexity finding of `legacy` is
/// inherited.
fn inherited_only(config: &str) -> TempDir {
    let source = hotspot("legacy", 20);
    repo(
        config,
        &[("src/index.js", source.clone())],
        &[("src/index.js", format!("{source}// note\n"))],
    )
}

#[test]
fn quiet_keeps_an_insufficient_override_when_every_finding_is_inherited() {
    let config = r#"{
  "entry": ["src/*.js"],
  "health": {
    "maxCyclomatic": 15,
    "maxCognitive": 15,
    "maxCrap": 999999999,
    "thresholdOverrides": [
      { "files": ["src/index.js"], "functions": ["legacy"], "maxCyclomatic": 18, "maxCognitive": 18 }
    ]
  }
}"#;
    let tmp = inherited_only(config);
    let args = ["--format", "compact", "--no-css", "--skip", "dupes"];
    let full = audit_args(tmp.path(), &args);
    let quiet = audit_args(tmp.path(), &[&args[..], &["--quiet"]].concat());
    assert!(full.stdout.contains(":legacy"), "{}", full.stdout);
    let rows: Vec<&str> = quiet.stdout.lines().collect();
    assert_eq!(rows.len(), 1, "{}", quiet.stdout);
    assert!(
        rows[0].starts_with("threshold-override:0:")
            && rows[0].contains(":insufficient:")
            && rows[0].contains("legacy"),
        "{}",
        quiet.stdout
    );
    assert_eq!(quiet.code, full.code);
}

#[test]
fn quiet_prints_one_line_when_nothing_is_actionable() {
    let tmp = inherited_only(LEGACY_CONFIG);
    let base = ["--no-css", "--skip", "dupes", "--quiet", "--format"];
    let compact = audit_args(tmp.path(), &[&base[..], &["compact"]].concat());
    assert_eq!(compact.stdout.trim(), "", "compact stays empty");
    for format in ["human", "markdown"] {
        let quiet = audit_args(tmp.path(), &[&base[..], &[format]].concat());
        assert_eq!(
            quiet.stdout.trim(),
            "No introduced or worsened findings.",
            "{format}"
        );
        assert_eq!(quiet.code, compact.code, "{format}");
    }
}

#[test]
fn quiet_keeps_a_worsened_complexity_finding() {
    let tmp = repo(
        LEGACY_CONFIG,
        &[(
            "src/index.js",
            format!("{}{}", hotspot("work", 20), hotspot("legacy", 20)),
        )],
        &[(
            "src/index.js",
            format!("{}{}", hotspot("work", 21), hotspot("legacy", 20)),
        )],
    );
    let args = ["--format", "compact", "--no-css", "--skip", "dupes"];
    let full = audit_args(tmp.path(), &args);
    let quiet = audit_args(tmp.path(), &[&args[..], &["--quiet"]].concat());
    assert!(full.stdout.contains(":legacy:"), "{}", full.stdout);
    assert!(
        quiet
            .stdout
            .contains("worse-than-base:src/index.js:1:work:attribution=worsened"),
        "{}",
        quiet.stdout
    );
    assert!(
        quiet
            .stdout
            .contains("high-complexity:src/index.js:1:work:"),
        "{}",
        quiet.stdout
    );
    assert!(!quiet.stdout.contains(":legacy:"), "{}", quiet.stdout);
    assert_eq!(quiet.code, full.code);
}

/// A block of code with enough tokens to form a clone group. `name` makes the
/// blocks of two groups differ.
fn clone_block(name: &str) -> String {
    let mut source = format!("export function {name}(input) {{\n  const out = [];\n");
    for step in 0..12 {
        writeln!(
            source,
            "  if (input.{name}Value{step} > {step}) {{ out.push(input.{name}Label{step} + '{name}'); }}"
        )
        .unwrap();
    }
    source.push_str("  return out;\n}\n");
    source
}

#[test]
fn quiet_keeps_only_introduced_clone_groups() {
    let legacy = clone_block("legacy");
    let fresh = clone_block("fresh");
    let tmp = repo(
        r#"{"entry": ["src/*.js"]}"#,
        &[("src/a.js", legacy.clone()), ("src/b.js", legacy.clone())],
        &[
            ("src/a.js", format!("{legacy}// note\n")),
            ("src/c.js", fresh.clone()),
            ("src/d.js", fresh),
        ],
    );
    let args = ["--format", "compact", "--only", "dupes"];
    let full = audit_args(tmp.path(), &args);
    let quiet = audit_args(tmp.path(), &[&args[..], &["--quiet"]].concat());
    assert!(full.stdout.contains("src/b.js"), "{}", full.stdout);
    assert!(full.stdout.contains("src/c.js"), "{}", full.stdout);
    assert!(quiet.stdout.contains("src/c.js"), "{}", quiet.stdout);
    assert!(quiet.stdout.contains("src/d.js"), "{}", quiet.stdout);
    assert!(!quiet.stdout.contains("src/b.js"), "{}", quiet.stdout);
    assert_eq!(quiet.code, full.code);
}

#[test]
fn quiet_keeps_only_introduced_styling_findings() {
    let base = "#app .legacy .title { color: red; }\n".to_owned();
    let tmp = repo(
        r#"{"entry": ["src/index.js"], "rules": {"css-selector-complexity": "error"}}"#,
        &[
            ("src/index.js", "import './styles.css';\n".to_owned()),
            ("src/styles.css", base.clone()),
        ],
        &[(
            "src/styles.css",
            format!("{base}#nav .menu .item {{ color: green; }}\n"),
        )],
    );
    let args = ["--format", "compact", "--only", "health"];
    let full = audit_args(tmp.path(), &args);
    let quiet = audit_args(tmp.path(), &[&args[..], &["--quiet"]].concat());
    assert!(
        full.stdout
            .contains("src/styles.css:1  css-selector-complexity"),
        "{}",
        full.stdout
    );
    assert!(
        quiet
            .stdout
            .contains("src/styles.css:2  css-selector-complexity"),
        "{}",
        quiet.stdout
    );
    assert!(
        !quiet
            .stdout
            .contains("src/styles.css:1  css-selector-complexity"),
        "{}",
        quiet.stdout
    );
    assert_eq!(quiet.code, full.code);
}

#[test]
fn quiet_under_gate_all_keeps_inherited_findings() {
    let tmp = fixture();
    let args = [
        "--format", "compact", "--no-css", "--skip", "dupes", "--gate", "all",
    ];
    let full = audit_args(tmp.path(), &args);
    let quiet = audit_args(tmp.path(), &[&args[..], &["--quiet"]].concat());
    assert!(full.stdout.contains("vital-signs"), "{}", full.stdout);
    for record in [
        ":legacy:",
        ":fresh:",
        "unusedOld",
        "threshold-override:2:complexity:insufficient",
    ] {
        assert!(
            quiet.stdout.contains(record),
            "lacks {record}:\n{}",
            quiet.stdout
        );
    }
    for record in ["vital-signs", ":active:", ":stale:"] {
        assert!(
            !quiet.stdout.contains(record),
            "keeps {record}:\n{}",
            quiet.stdout
        );
    }
    assert_eq!(full.code, 1);
    assert_eq!(quiet.code, full.code);
}
