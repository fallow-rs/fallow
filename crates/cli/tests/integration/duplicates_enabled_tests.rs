//! `duplicates.enabled: false` turns the duplication analysis off in the bare
//! `fallow` command. `--only dupes` and the `fallow dupes` command name the
//! analysis, so they still run it. The health score then has no duplication
//! measure and no duplication penalty.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use crate::common::{CommandOutput, parse_json, run_fallow_raw};

/// A function body long enough to form a clone group with a second copy.
fn cloned(name: &str) -> String {
    let mut source = format!("export function {name}(items) {{\n  const out = [];\n");
    for step in 0..12 {
        writeln!(
            source,
            "  out.push(items.map((item) => item.value * {step}).filter((value) => value > {step}));"
        )
        .unwrap();
    }
    source.push_str("  return out;\n}\n");
    source
}

/// A project with one clone group. `config` is the content of
/// `.fallowrc.json`.
fn project(config: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "duplicates-enabled", "private": true, "type": "module", "main": "src/index.js"}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), config).unwrap();
    fs::write(
        dir.join("src/index.js"),
        "import { first } from './first.js';\nimport { second } from './second.js';\nconsole.log(first, second);\n",
    )
    .unwrap();
    fs::write(dir.join("src/first.js"), cloned("first")).unwrap();
    fs::write(dir.join("src/second.js"), cloned("second")).unwrap();
    tmp
}

const DISABLED: &str = r#"{"duplicates": {"enabled": false}}"#;

fn run(dir: &Path, args: &[&str]) -> CommandOutput {
    let mut all = args.to_vec();
    all.extend_from_slice(&["--root", dir.to_str().unwrap(), "--format", "json"]);
    run_fallow_raw(&all)
}

fn clone_groups(section: &Value) -> usize {
    section["clone_groups"].as_array().map_or(0, Vec::len)
}

#[test]
fn bare_run_reports_duplication_by_default() {
    let tmp = project("{}");
    let json = parse_json(&run(tmp.path(), &["--quiet"]));

    assert_eq!(clone_groups(&json["dupes"]), 1, "{json:#}");
}

#[test]
fn bare_run_skips_duplication_with_duplicates_enabled_false() {
    let tmp = project(DISABLED);
    let output = run(tmp.path(), &["--quiet"]);
    let json = parse_json(&output);

    assert!(json.get("dupes").is_none(), "{json:#}");
    assert!(json.get("check").is_some(), "{json:#}");
    assert!(json.get("health").is_some(), "{json:#}");
}

#[test]
fn an_explicit_selection_runs_duplication_with_duplicates_enabled_false() {
    let tmp = project(DISABLED);

    let only = parse_json(&run(tmp.path(), &["--only", "dupes", "--quiet"]));
    assert_eq!(clone_groups(&only["dupes"]), 1, "{only:#}");

    let command = parse_json(&run(tmp.path(), &["dupes", "--quiet"]));
    assert_eq!(clone_groups(&command), 1, "{command:#}");
}

#[test]
fn a_selection_that_leaves_only_disabled_duplication_is_an_error() {
    let tmp = project(DISABLED);
    let output = run(tmp.path(), &["--skip", "dead-code,health", "--quiet"]);

    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(
        output.stdout.contains("duplicates.enabled"),
        "the error names the config key: {}",
        output.stdout
    );
}

/// The `audit` config keys apply to `fallow audit` only. The bare run ignores
/// `audit.dupes: false`.
#[test]
fn bare_run_ignores_the_audit_dupes_key() {
    let tmp = project(r#"{"audit": {"dupes": false}}"#);
    let output = run(tmp.path(), &["--skip", "dead-code,health", "--quiet"]);
    let json = parse_json(&output);

    assert_eq!(clone_groups(&json["dupes"]), 1, "{json:#}");
}

#[test]
fn bare_run_names_a_dupes_baseline_that_has_no_effect() {
    let tmp = project(DISABLED);
    let baseline = tmp.path().join("dupes-baseline.json");
    let output = run(
        tmp.path(),
        &["--dupes-baseline", baseline.to_str().unwrap()],
    );

    assert!(output.code == 0 || output.code == 1, "{}", output.stdout);
    let json = parse_json(&output);
    let inert = json["workspace_diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["kind"] == "inert-flag" && entry["flag"] == "--dupes-baseline")
        .count();
    assert_eq!(inert, 1, "{json:#}");
}

#[test]
fn health_score_measures_duplication_by_default() {
    let tmp = project("{}");
    let json = parse_json(&run(tmp.path(), &["health", "--score", "--quiet"]));

    assert!(
        json["health_score"]["penalties"]["duplication"].is_number(),
        "{json:#}"
    );
}

#[test]
fn health_score_omits_duplication_with_duplicates_enabled_false() {
    let tmp = project(DISABLED);
    let json = parse_json(&run(tmp.path(), &["health", "--score", "--quiet"]));

    assert!(json["health_score"]["score"].is_number(), "{json:#}");
    assert!(
        json["health_score"]["penalties"]
            .get("duplication")
            .is_none(),
        "{json:#}"
    );
}

#[test]
fn bare_run_score_omits_duplication_with_duplicates_enabled_false() {
    let tmp = project(DISABLED);
    let json = parse_json(&run(tmp.path(), &["--score", "--quiet"]));
    let health = &json["health"];

    assert!(health["health_score"]["score"].is_number(), "{json:#}");
    assert!(
        health["health_score"]["penalties"]
            .get("duplication")
            .is_none(),
        "{json:#}"
    );
}

#[test]
fn include_dupes_runs_the_cross_reference_with_duplicates_enabled_false() {
    let tmp = project(DISABLED);
    // `second.js` is then both a clone and an unused file.
    fs::write(
        tmp.path().join("src/index.js"),
        "import { first } from './first.js';\nconsole.log(first);\n",
    )
    .unwrap();
    let output = run_fallow_raw(&[
        "dead-code",
        "--include-dupes",
        "--root",
        tmp.path().to_str().unwrap(),
    ]);

    assert!(
        output.stdout.contains("Duplicated + Unused"),
        "the flag names the cross-reference: {}",
        output.stdout
    );
}

/// The score of a project does not depend on the analyses that one run
/// selects, so a run that names duplication gives the same score.
#[test]
fn a_named_duplication_run_does_not_change_the_health_score() {
    let tmp = project(DISABLED);
    let json = parse_json(&run(
        tmp.path(),
        &["--only", "dupes,health", "--score", "--quiet"],
    ));

    assert_eq!(clone_groups(&json["dupes"]), 1, "{json:#}");
    assert!(
        json["health"]["health_score"]["penalties"]
            .get("duplication")
            .is_none(),
        "{json:#}"
    );
}
