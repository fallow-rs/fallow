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

#[test]
fn quiet_without_introduced_findings_prints_no_findings() {
    let tmp = fixture();
    let dir = tmp.path();
    // Make the head equal to the base for the health findings, and drop the
    // new export, so every remaining finding is inherited.
    git(dir, &["checkout", "main", "--", "src/lib.js"]);
    let base_index = fs::read_to_string(dir.join("src/index.js")).unwrap();
    let without_fresh = base_index
        .split("export function fresh")
        .next()
        .unwrap()
        .to_owned();
    fs::write(
        dir.join("src/index.js"),
        format!("{without_fresh}// note\n"),
    )
    .unwrap();
    fs::write(
        dir.join("src/lib.js"),
        "export const used = () => 1;\nexport const unusedOld = () => 2;\n// note\n",
    )
    .unwrap();
    commit_all(dir, "only inherited");

    let full = audit(dir, "compact", false);
    let quiet = audit(dir, "compact", true);
    assert!(full.stdout.contains(":legacy"), "{}", full.stdout);
    assert_eq!(
        quiet.stdout.trim(),
        "",
        "the quiet report of inherited findings only must be empty"
    );
    assert_eq!(quiet.code, full.code);
}
