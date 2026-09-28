#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{copy_fixture, parse_json, run_fallow, run_fallow_in_root};

const FIXTURE: &str = "package-cycle-workspace";

fn write_rules(root: &std::path::Path, severity: &str) {
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(r#"{{ "rules": {{ "package-cycle": "{severity}" }} }}"#),
    )
    .expect("write config");
}

#[test]
fn reproduction_reports_package_cycle_as_json() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "warn does not fail: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["summary"]["package_cycles"], 1);
    assert_eq!(json["circular_dependencies"].as_array().unwrap().len(), 0);
    let cycles = json["package_cycles"].as_array().unwrap();
    assert_eq!(cycles.len(), 1);
    let cycle = &cycles[0];
    assert_eq!(
        cycle["packages"],
        serde_json::json!(["@repro/a", "@repro/b"])
    );
    assert_eq!(cycle["length"], 2);
    assert_eq!(cycle["edges"][0]["path"], "packages/a/src/x.ts");
    assert_eq!(cycle["edges"][0]["target_path"], "packages/b/src/y.ts");
    assert_eq!(cycle["edges"][1]["path"], "packages/b/src/z.ts");
    assert_eq!(cycle["edges"][1]["type_only"], false);
    assert_eq!(cycle["effective_severity"], "warn");
}

#[test]
fn error_severity_fails_the_run() {
    let dir = copy_fixture(FIXTURE);
    write_rules(dir.path(), "error");
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 1, "error severity fails: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"][0]["effective_severity"], "error");
}

#[test]
fn off_severity_hides_the_finding() {
    let dir = copy_fixture(FIXTURE);
    write_rules(dir.path(), "off");
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"].as_array().map_or(0, Vec::len), 0);
}

#[test]
fn package_cycles_flag_scopes_the_report() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &[
            "--package-cycles",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"].as_array().unwrap().len(), 1);
    assert_eq!(json["total_issues"], 1);
}

#[test]
fn human_output_lists_packages_and_example_imports() {
    let output = run_fallow("dead-code", FIXTURE, &["--no-cache"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output.stdout.contains("Package cycles (1)"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("@repro/a"), "{}", output.stdout);
    assert!(
        output.stdout.contains("packages/b/src/z.ts:1"),
        "{}",
        output.stdout
    );
}

fn write_override(root: &std::path::Path, files: &[&str]) {
    let files = files
        .iter()
        .map(|file| format!("\"{file}\""))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(
            r#"{{ "overrides": [{{ "files": [{files}], "rules": {{ "package-cycle": "off" }} }}] }}"#
        ),
    )
    .expect("write config");
}

/// A per-file `off` on every example import file hides the cycle, the same
/// way a per-file `off` on every file of a circular dependency hides it.
#[test]
fn per_file_off_on_every_example_import_hides_the_cycle() {
    let dir = copy_fixture(FIXTURE);
    write_override(dir.path(), &["packages/**"]);
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"].as_array().map_or(0, Vec::len), 0);
    assert_eq!(json["total_issues"], 0);
}

#[test]
fn per_file_off_on_one_example_import_keeps_the_cycle() {
    let dir = copy_fixture(FIXTURE);
    write_override(dir.path(), &["packages/a/**"]);
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["package_cycles"].as_array().map_or(0, Vec::len), 1);
    assert_eq!(json["package_cycles"][0]["effective_severity"], "warn");
}
