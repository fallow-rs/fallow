#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

//! Prop-drilling, thin-wrapper and duplicate-prop-shape findings never gate
//! the exit code. Their CI level is capped at `warning`, so a CI system that
//! reads the level never shows an error for a run that passed.
//!
//! SARIF carries these findings at level `warning` for an `error` and a `warn`
//! rule, and the SARIF rule default follows. CodeClimate and GitHub
//! annotations do not carry these findings at all.

use crate::common::{CommandOutput, fixture_path, parse_json, run_fallow_raw};
use serde_json::Value;

/// The rule name, the SARIF rule id and the fixture of each type.
const TYPES: [(&str, &str); 3] = [
    ("prop-drilling", "fallow/prop-drilling"),
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
        output.code, 0,
        "`{command}` with {rule}={level} --format {format} must pass, because the type never \
         gates\nstdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
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
fn sarif_caps_non_gating_types_at_warning() {
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
                    levels.iter().all(|found| found == "warning"),
                    "`{command}` {rule}={level}: SARIF levels {levels:?}, expected warning"
                );
                assert_eq!(
                    sarif_default_level(&sarif, rule_id),
                    "warning",
                    "`{command}` {rule}={level}: SARIF rule default level"
                );
            }
        }
    }
}

#[test]
fn codeclimate_and_annotations_carry_no_non_gating_finding() {
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
            assert_eq!(matching, 0, "{rule}={level}: CodeClimate issues");

            let output = run("dead-code", rule, level, "github-annotations");
            assert!(
                !output.stdout.contains("::error"),
                "{rule}={level}: annotations must hold no error line:\n{}",
                output.stdout
            );
        }
    }
}
