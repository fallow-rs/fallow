//! `fallow config --resolved` prints each decision of a run with its cause.
//! `fallow config` without the flag keeps its loader path and exit codes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use crate::common::{CommandOutput, parse_json, run_fallow_raw_with_env};

/// A project with the files of a parity case, and the environment of the case.
fn parity_case(name: &str) -> (TempDir, Vec<(String, String)>) {
    let case_path =
        Path::new(&std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/run-parity/cases")
            .join(format!("{name}.json"));
    let case: Value = serde_json::from_str(&fs::read_to_string(case_path).unwrap()).unwrap();
    let tmp = TempDir::new().unwrap();
    for (file, content) in case["files"].as_object().unwrap() {
        fs::write(tmp.path().join(file), content.as_str().unwrap()).unwrap();
    }
    let env = case["env"]
        .as_object()
        .map(|vars| {
            vars.iter()
                .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned()))
                .collect()
        })
        .unwrap_or_default();
    (tmp, env)
}

fn project(files: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("package.json"), r#"{"name":"config-view"}"#).unwrap();
    for (name, content) in files {
        fs::write(tmp.path().join(name), content).unwrap();
    }
    tmp
}

fn config(tmp: &TempDir, args: &[&str], env: &[(&str, &str)]) -> CommandOutput {
    let root = tmp.path().to_str().unwrap().to_owned();
    let mut all = vec!["config", "--root", root.as_str()];
    all.extend_from_slice(args);
    run_fallow_raw_with_env(&all, env)
}

#[test]
fn resolved_view_names_the_source_of_each_production_mode() {
    let (tmp, env) = parity_case("per-analysis-split-env");
    let env: Vec<(&str, &str)> = env
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let output = config(&tmp, &["--resolved", "--for", "audit"], &env);

    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let view = parse_json(&output);
    assert_eq!(view["kind"], "resolved-run", "{view:#}");
    assert_eq!(view["schema_version"], 1, "{view:#}");
    assert_eq!(view["command"], "audit", "{view:#}");
    let analyses = &view["analyses"];
    assert_eq!(
        analyses["dead_code"]["production"]["value"], true,
        "{view:#}"
    );
    assert_eq!(
        analyses["dead_code"]["production"]["source"],
        serde_json::json!({
            "kind": "config-file",
            "file": ".fallowrc.json",
            "key": "production.deadCode",
        }),
        "{view:#}"
    );
    assert_eq!(analyses["duplication"]["production"]["value"], false);
    assert_eq!(
        analyses["duplication"]["production"]["source"]["key"],
        "production.dupes"
    );
    assert_eq!(analyses["health"]["production"]["value"], true);
    assert_eq!(
        analyses["health"]["production"]["source"],
        serde_json::json!({ "kind": "environment", "name": "FALLOW_PRODUCTION_HEALTH" }),
        "{view:#}"
    );
}

#[test]
fn resolved_conflicts_with_path() {
    let tmp = project(&[(".fallowrc.json", "{}")]);
    let output = config(&tmp, &["--resolved", "--path"], &[]);

    assert_eq!(
        output.code, 2,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
}

#[test]
fn plain_config_keeps_exit_3_with_path_and_no_file() {
    let tmp = project(&[]);
    let output = config(&tmp, &["--path"], &[]);

    assert_eq!(
        output.code, 3,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    assert!(output.stdout.is_empty(), "{}", output.stdout);
}

#[test]
fn plain_config_ignores_a_broken_plugin_file() {
    let tmp = project(&[(".fallowrc.json", r#"{"plugins":["missing-plugin.json"]}"#)]);
    let output = config(&tmp, &[], &[]);

    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let printed = parse_json(&output);
    assert!(printed.get("plugins").is_some(), "{printed:#}");
}

#[test]
fn resolved_view_lists_env_names_not_values() {
    let tmp = project(&[]);
    let output = config(
        &tmp,
        &["--resolved", "--for", "dead-code"],
        &[
            ("FALLOW_PRODUCTION", "true"),
            ("FALLOW_COVERAGE", "secret-coverage.json"),
        ],
    );

    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    assert!(
        !output.stdout.contains("secret-coverage.json") && !output.stdout.contains("\"true\""),
        "the view never holds a value of a variable: {}",
        output.stdout
    );
    let view = parse_json(&output);
    let names = |list: &str| -> Vec<String> {
        view["env"][list]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_owned())
            .collect()
    };
    assert!(
        names("read").contains(&"FALLOW_PRODUCTION".to_owned()),
        "{view:#}"
    );
    assert!(
        names("ignored").contains(&"FALLOW_COVERAGE".to_owned()),
        "{view:#}"
    );
}

#[test]
fn resolved_view_of_a_broken_config_is_a_structured_error() {
    let tmp = project(&[(".fallowrc.json", "{ not json")]);
    let output = config(&tmp, &["--resolved"], &[]);

    assert_eq!(
        output.code, 2,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let error = parse_json(&output);
    assert_eq!(error["error"], true, "{error:#}");
}

#[test]
fn resolved_view_lists_a_rejected_value_as_ignored_and_prints_its_note() {
    let tmp = project(&[]);
    let output = config(&tmp, &["--resolved"], &[("FALLOW_PRODUCTION", "maybe")]);

    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let view = parse_json(&output);
    let names = |list: &str| -> Vec<&str> {
        view["env"][list]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect()
    };
    assert!(!names("read").contains(&"FALLOW_PRODUCTION"), "{view:#}");
    assert!(names("ignored").contains(&"FALLOW_PRODUCTION"), "{view:#}");
    assert!(
        output
            .stderr
            .contains("Note: The value of FALLOW_PRODUCTION is not a bool"),
        "{}",
        output.stderr
    );
    assert!(!output.stdout.contains("maybe"), "{}", output.stdout);

    let quiet = config(
        &tmp,
        &["--resolved", "--quiet"],
        &[("FALLOW_PRODUCTION", "maybe")],
    );
    assert!(!quiet.stderr.contains("Note:"), "{}", quiet.stderr);
}

#[test]
fn resolved_view_of_one_analysis_rejects_only_and_skip() {
    let tmp = project(&[]);
    for (for_command, flag) in [
        ("dead-code", "--only"),
        ("dupes", "--only"),
        ("health", "--skip"),
    ] {
        let output = config(
            &tmp,
            &[
                "--resolved",
                "--for",
                for_command,
                flag,
                "health",
                "--format",
                "json",
            ],
            &[],
        );
        assert_eq!(
            output.code, 2,
            "{for_command} {flag}\nstdout: {}\nstderr: {}",
            output.stdout, output.stderr
        );
        let error = parse_json(&output);
        assert_eq!(error["error"], true, "{error:#}");
        assert!(
            error["message"].as_str().unwrap().contains(flag),
            "{error:#}"
        );
    }
    let audit = config(
        &tmp,
        &["--resolved", "--for", "audit", "--only", "health"],
        &[],
    );
    assert_eq!(audit.code, 0, "{}", audit.stderr);
}
