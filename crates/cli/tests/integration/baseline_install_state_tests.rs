//! A dead-code baseline records whether `node_modules` existed when it was
//! saved. A run in the other install state reports
//! `baseline-install-state-mismatch`, because findings that need installed
//! packages can differ and the run can report baseline findings as new.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, parse_json, run_fallow_raw};
use serde_json::Value;
use tempfile::TempDir;

/// A devDependency that only a package script uses, through its binary. Without
/// `node_modules`, fallow cannot map the binary `foo` to the package `foo-cli`.
fn project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::create_dir_all(root.join("node_modules/foo-cli")).expect("node_modules");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"install-state-fx","version":"1.0.0","private":true,"main":"src/index.js",
            "scripts":{"lint":"foo src"},"devDependencies":{"foo-cli":"1.0.0"}}"#,
    )
    .expect("package.json");
    std::fs::write(root.join("src/index.js"), "console.log('hi');\n").expect("entry");
    std::fs::write(
        root.join("node_modules/foo-cli/package.json"),
        r#"{"name":"foo-cli","version":"1.0.0","bin":{"foo":"cli.js"}}"#,
    )
    .expect("installed package");
    std::fs::write(root.join("node_modules/foo-cli/cli.js"), "").expect("bin");
    dir
}

fn run(dir: &TempDir, baseline_flag: &str, baseline: &str) -> (CommandOutput, Value) {
    let root = dir.path().to_str().expect("UTF-8 path");
    let path = dir.path().join(baseline);
    let path = path.to_str().expect("UTF-8 path");
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        baseline_flag,
        path,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    let json = parse_json(&output);
    (output, json)
}

fn mismatches(json: &Value) -> Vec<&Value> {
    json["workspace_diagnostics"]
        .as_array()
        .map(|diagnostics| {
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic["kind"] == "baseline-install-state-mismatch")
                .collect()
        })
        .unwrap_or_default()
}

fn saved_state(dir: &TempDir) -> Value {
    let content = std::fs::read_to_string(dir.path().join("baseline.json")).expect("baseline");
    let value: Value = serde_json::from_str(&content).expect("baseline JSON");
    value["analysis_identity"]["node_modules"].clone()
}

#[test]
fn a_run_without_node_modules_names_the_install_state_of_the_baseline() {
    let project = project();
    let (saved, _) = run(&project, "--save-baseline", "baseline.json");
    assert_eq!(saved.code, 0, "stderr: {}", saved.stderr);
    assert_eq!(saved_state(&project), "present");

    std::fs::remove_dir_all(project.path().join("node_modules")).expect("uninstall");
    let (output, json) = run(&project, "--baseline", "baseline.json");
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let found = mismatches(&json);
    assert_eq!(found.len(), 1, "{}", json["workspace_diagnostics"]);
    assert_eq!(found[0]["baseline_node_modules"], "present");
    assert_eq!(found[0]["run_node_modules"], "missing");
    assert_eq!(found[0]["path"], "baseline.json");
    assert_eq!(found[0]["degrades_analysis"], true);
    assert!(
        output
            .stderr
            .contains("Warning: The baseline 'baseline.json' was saved with node_modules present"),
        "stderr: {}",
        output.stderr
    );
    assert_eq!(
        json["unused_dev_dependencies"][0]["package_name"], "foo-cli",
        "the finding itself is unchanged: {json}"
    );
}

#[test]
fn the_same_install_state_gives_no_diagnostic() {
    let project = project();
    run(&project, "--save-baseline", "baseline.json");
    let (output, json) = run(&project, "--baseline", "baseline.json");
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(mismatches(&json).is_empty(), "{json}");
    assert!(!output.stderr.contains("Warning: The baseline"));
}

#[test]
fn a_legacy_baseline_without_an_install_state_gives_no_diagnostic() {
    let project = project();
    run(&project, "--save-baseline", "baseline.json");
    let path = project.path().join("baseline.json");
    let mut value: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("baseline")).expect("JSON");
    value["analysis_identity"]
        .as_object_mut()
        .expect("identity object")
        .remove("node_modules");
    std::fs::write(&path, value.to_string()).expect("legacy baseline");

    std::fs::remove_dir_all(project.path().join("node_modules")).expect("uninstall");
    let (output, json) = run(&project, "--baseline", "baseline.json");
    assert!(mismatches(&json).is_empty(), "{json}");
    assert!(!output.stderr.contains("Warning: The baseline"));
}
