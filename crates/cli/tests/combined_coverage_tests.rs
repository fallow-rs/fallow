#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use std::fs;
use std::path::Path;

use common::{parse_json, run_fallow_raw, run_fallow_raw_with_env};
use tempfile::{TempDir, tempdir};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    fs::write(path, contents).expect("write file");
}

fn create_branchy_project(name: &str) -> TempDir {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("package.json"),
        &format!(r#"{{"name":"{name}","type":"module"}}"#),
    );
    write_file(
        &dir.path().join("src/index.ts"),
        "export function branchy(n: number): number {
  if (n < 0) return -1;
  if (n === 0) return 0;
  if (n < 10) return 1;
  if (n < 100) return 2;
  if (n < 1000) return 3;
  if (n < 10000) return 4;
  return 5;
}
",
    );
    dir
}

fn write_config(root: &Path, body: &str) {
    write_file(&root.join(".fallowrc.json"), body);
}

fn write_branchy_istanbul_coverage(coverage_path: &Path, coverage_source_path: &str) {
    fs::create_dir_all(coverage_path.parent().unwrap()).unwrap();
    let mut coverage = serde_json::Map::new();
    coverage.insert(
        coverage_source_path.to_owned(),
        serde_json::json!({
            "path": coverage_source_path,
            "statementMap": {},
            "fnMap": {
                "0": {
                    "name": "branchy",
                    "line": 1,
                    "decl": {
                        "start": { "line": 1, "column": 16 },
                        "end": { "line": 1, "column": 23 }
                    },
                    "loc": {
                        "start": { "line": 1, "column": 43 },
                        "end": { "line": 9, "column": 1 }
                    }
                }
            },
            "branchMap": {},
            "s": {},
            "f": { "0": 1 },
            "b": {}
        }),
    );
    fs::write(coverage_path, serde_json::to_string(&coverage).unwrap()).unwrap();
}

fn combined_health_model(json: &serde_json::Value) -> Option<&str> {
    json.pointer("/health/summary/coverage_model")
        .and_then(serde_json::Value::as_str)
}

fn standalone_health_model(json: &serde_json::Value) -> Option<&str> {
    json.pointer("/summary/coverage_model")
        .and_then(serde_json::Value::as_str)
}

fn combined_health_args(root: &Path) -> Vec<&str> {
    vec![
        "--root",
        root.to_str().unwrap(),
        "--only",
        "health",
        "--format",
        "json",
        "--quiet",
    ]
}

#[test]
fn combined_cli_coverage_and_root_feed_health_crap_scoring() {
    let dir = create_branchy_project("combined-cli-coverage");
    write_config(dir.path(), r#"{"health":{"maxCrap":10}}"#);

    let without_coverage = run_fallow_raw(&combined_health_args(dir.path()));
    assert_eq!(
        without_coverage.code, 0,
        "static health run should still complete before Istanbul coverage is supplied. stderr: {}",
        without_coverage.stderr
    );
    let json = parse_json(&without_coverage);
    assert_eq!(combined_health_model(&json), Some("static_estimated"));

    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_branchy_istanbul_coverage(&coverage_path, "/ci/workspace/src/index.ts");

    let mut args = combined_health_args(dir.path());
    args.extend([
        "--coverage",
        coverage_path.to_str().unwrap(),
        "--coverage-root",
        "/ci/workspace",
    ]);
    let with_coverage = run_fallow_raw(&args);
    assert_eq!(
        with_coverage.code, 0,
        "Istanbul coverage should lower CRAP below the combined health threshold. stderr: {}",
        with_coverage.stderr
    );
    let json = parse_json(&with_coverage);
    assert_eq!(combined_health_model(&json), Some("istanbul"));
}

#[test]
fn combined_env_coverage_fallback_feeds_health_crap_scoring() {
    let dir = create_branchy_project("combined-env-coverage");
    write_config(dir.path(), r#"{"health":{"maxCrap":10}}"#);
    let coverage_path = dir.path().join("artifacts/env-coverage.json");
    let source_path = dir.path().join("src/index.ts");
    write_branchy_istanbul_coverage(&coverage_path, &source_path.to_string_lossy());

    let output = run_fallow_raw_with_env(
        &combined_health_args(dir.path()),
        &[("FALLOW_COVERAGE", coverage_path.to_str().unwrap())],
    );
    assert_eq!(
        output.code, 0,
        "FALLOW_COVERAGE should feed combined health. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(combined_health_model(&json), Some("istanbul"));
}

#[test]
fn combined_config_coverage_fallback_feeds_health_crap_scoring() {
    let dir = create_branchy_project("combined-config-coverage");
    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_branchy_istanbul_coverage(&coverage_path, "/ci/workspace/src/index.ts");
    write_config(
        dir.path(),
        r#"{"health":{"maxCrap":10,"coverage":"artifacts/coverage-final.json","coverageRoot":"/ci/workspace"}}"#,
    );

    let output = run_fallow_raw(&combined_health_args(dir.path()));
    assert_eq!(
        output.code, 0,
        "health.coverage should feed combined health. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(combined_health_model(&json), Some("istanbul"));
}

#[test]
fn mixed_precedence_resolves_coverage_and_root_independently() {
    let dir = create_branchy_project("combined-coverage-precedence");
    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_branchy_istanbul_coverage(&coverage_path, "/ci/workspace/src/index.ts");
    write_config(
        dir.path(),
        r#"{"health":{"maxCrap":10,"coverageRoot":"/wrong/root"}}"#,
    );

    let mut args = combined_health_args(dir.path());
    args.extend(["--coverage", coverage_path.to_str().unwrap()]);
    let cli_coverage_env_root =
        run_fallow_raw_with_env(&args, &[("FALLOW_COVERAGE_ROOT", "/ci/workspace")]);
    assert_eq!(
        cli_coverage_env_root.code, 0,
        "env coverage root should pair with CLI coverage. stderr: {}",
        cli_coverage_env_root.stderr
    );
    let json = parse_json(&cli_coverage_env_root);
    assert_eq!(combined_health_model(&json), Some("istanbul"));

    let env_coverage_config_root = {
        write_config(
            dir.path(),
            r#"{"health":{"maxCrap":10,"coverageRoot":"/ci/workspace"}}"#,
        );
        run_fallow_raw_with_env(
            &combined_health_args(dir.path()),
            &[("FALLOW_COVERAGE", coverage_path.to_str().unwrap())],
        )
    };
    assert_eq!(
        env_coverage_config_root.code, 0,
        "config coverage root should pair with env coverage. stderr: {}",
        env_coverage_config_root.stderr
    );
    let json = parse_json(&env_coverage_config_root);
    assert_eq!(combined_health_model(&json), Some("istanbul"));

    write_config(
        dir.path(),
        r#"{"health":{"maxCrap":10,"coverageRoot":"/wrong/root"}}"#,
    );
    let mut args = combined_health_args(dir.path());
    args.extend([
        "--coverage",
        coverage_path.to_str().unwrap(),
        "--coverage-root",
        "/ci/workspace",
    ]);
    let cli_root_overrides_config = run_fallow_raw(&args);
    assert_eq!(
        cli_root_overrides_config.code, 0,
        "CLI coverage root should override config coverage root. stderr: {}",
        cli_root_overrides_config.stderr
    );
    let json = parse_json(&cli_root_overrides_config);
    assert_eq!(combined_health_model(&json), Some("istanbul"));
}

#[test]
fn health_config_coverage_fallback_feeds_standalone_health() {
    let dir = create_branchy_project("health-config-coverage");
    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_branchy_istanbul_coverage(&coverage_path, "/ci/workspace/src/index.ts");
    write_config(
        dir.path(),
        r#"{"health":{"maxCrap":10,"coverage":"artifacts/coverage-final.json","coverageRoot":"/ci/workspace"}}"#,
    );

    let output = run_fallow_raw(&[
        "health",
        "--root",
        dir.path().to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "health.coverage should feed standalone health. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(standalone_health_model(&json), Some("istanbul"));
}

#[test]
fn relative_config_coverage_root_is_structured_exit_two() {
    let dir = create_branchy_project("relative-config-coverage-root");
    write_config(dir.path(), r#"{"health":{"coverageRoot":"src"}}"#);

    let output = run_fallow_raw(&combined_health_args(dir.path()));
    assert_eq!(
        output.code, 2,
        "relative health.coverageRoot should be rejected before analysis. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::json!(true));
    let message = json["message"].as_str().expect("message should be present");
    assert!(
        message.contains("--coverage-root expects an absolute path")
            && message.contains("got 'src'"),
        "unexpected error message: {message}"
    );
}

#[test]
fn bare_coverage_flags_before_subcommand_are_rejected() {
    let output = run_fallow_raw(&[
        "--coverage",
        "coverage/coverage-final.json",
        "dead-code",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 2,
        "pre-subcommand bare coverage should be rejected. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::json!(true));
    let message = json["message"].as_str().expect("message should be present");
    assert!(
        message.contains("bare combined-mode flags"),
        "unexpected error message: {message}"
    );
}
