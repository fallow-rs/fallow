//! `duplicates.enabled: false` turns the duplication analysis off in the bare
//! `fallow` command. `--only dupes` and the `fallow dupes` command name the
//! analysis, so they still run it.

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

#[test]
fn bare_run_names_a_dupes_baseline_that_has_no_effect() {
    let tmp = project(DISABLED);
    let baseline = tmp.path().join("dupes-baseline.json");
    let output = run(
        tmp.path(),
        &["--dupes-baseline", baseline.to_str().unwrap()],
    );

    assert!(output.code == 0 || output.code == 1, "{}", output.stdout);
    assert!(
        output
            .stderr
            .contains("--dupes-baseline has no effect: duplicates.enabled is false"),
        "stderr: {}",
        output.stderr
    );
}
