#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

//! Component health findings stay advisory at warning severity and enforce errors.

use crate::common::{CommandOutput, fixture_path, parse_json, run_fallow_raw};
use serde_json::Value;

/// The rule name, the SARIF rule id and the fixture of each type.
const TYPES: [(&str, &str); 2] = [
    ("thin-wrapper", "fallow/thin-wrapper"),
    ("duplicate-prop-shape", "fallow/duplicate-prop-shape"),
];

const LEVELS: [&str; 2] = ["error", "warn"];

/// Run `command` on the fixture of `rule`, with `rule` at `level`. The
/// fixtures have no installed packages, so the dependency rules are off.
fn run(command: &str, rule: &str, level: &str, format: &str) -> CommandOutput {
    let config_dir = tempfile::tempdir().expect("config tempdir");
    let config = config_dir.path().join(".fallowrc.json");
    std::fs::write(
        &config,
        format!(r#"{{"rules":{{"{rule}":"{level}","unused-dependencies":"off"}}}}"#),
    )
    .expect("write config");
    let root = fixture_path(rule);
    let mut args = Vec::new();
    if !command.is_empty() {
        args.push(command);
    }
    args.extend_from_slice(&[
        "--root",
        root.to_str().expect("utf-8 path"),
        "--config",
        config.to_str().expect("utf-8 path"),
        "--quiet",
        "--format",
        format,
    ]);
    let output = run_fallow_raw(&args);
    assert_eq!(
        output.code,
        i32::from(command == "dead-code" && level == "error"),
        "`{command}` with {rule}={level} --format {format} must match the enforced exit policy\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
    output
}

fn sarif_levels(sarif: &Value, rule_id: &str) -> Vec<String> {
    sarif["runs"]
        .as_array()
        .expect("SARIF runs")
        .iter()
        .flat_map(|run| run["results"].as_array().into_iter().flatten())
        .filter(|result| result["ruleId"] == rule_id)
        .map(|result| result["level"].as_str().expect("level").to_owned())
        .collect()
}

fn sarif_default_level(sarif: &Value, rule_id: &str) -> String {
    sarif["runs"]
        .as_array()
        .expect("SARIF runs")
        .iter()
        .flat_map(|run| run["tool"]["driver"]["rules"].as_array().into_iter().flatten())
        .find(|rule| rule["id"] == rule_id)
        .unwrap_or_else(|| panic!("no SARIF rule {rule_id} in {sarif}"))["defaultConfiguration"]
        ["level"]
        .as_str()
        .expect("default level")
        .to_owned()
}

#[test]
fn sarif_component_levels_follow_configured_enforcement() {
    for command in ["dead-code", ""] {
        for (rule, rule_id) in TYPES {
            for level in LEVELS {
                let output = run(command, rule, level, "sarif");
                let sarif = parse_json(&output);
                let levels = sarif_levels(&sarif, rule_id);
                assert!(
                    !levels.is_empty(),
                    "`{command}` {rule}={level}: the fixture must produce a finding"
                );
                assert!(
                    levels
                        .iter()
                        .all(|found| found == if level == "error" { "error" } else { "warning" }),
                    "`{command}` {rule}={level}: SARIF levels {levels:?}, expected configured severity"
                );
                assert_eq!(
                    sarif_default_level(&sarif, rule_id),
                    if level == "error" { "error" } else { "warning" },
                    "`{command}` {rule}={level}: SARIF rule default level"
                );
            }
        }
    }
}

#[test]
fn codeclimate_and_annotations_carry_only_enforced_component_findings() {
    for (rule, rule_id) in TYPES {
        for level in LEVELS {
            let output = run("dead-code", rule, level, "codeclimate");
            let issues = parse_json(&output);
            let matching = issues
                .as_array()
                .expect("CodeClimate array")
                .iter()
                .filter(|issue| issue["check_name"] == rule_id)
                .count();
            assert_eq!(
                matching,
                if level == "warn" {
                    0
                } else if rule == "thin-wrapper" {
                    1
                } else {
                    3
                },
                "{rule}={level}: CodeClimate issues"
            );

            let output = run("dead-code", rule, level, "github-annotations");
            assert!(
                output.stdout.contains("::error") == (level == "error"),
                "{rule}={level}: annotations must match the error policy:\n{}",
                output.stdout
            );
        }
    }
}
