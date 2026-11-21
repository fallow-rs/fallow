//! The production rule of the commands that read a config and are not an
//! analysis command. The project config sets `production: true` and each
//! run sets `FALLOW_PRODUCTION=false`, so a command that reads the variable
//! analyzes the test file and a command that reads the config only does not.
//!
//! - `trace --path`, `trace FILE:SYMBOL` and `trace-error` read the config
//!   only.
//! - `trace --dependency`, `baselines prune` and the child processes of
//!   `inspect` read the flags, then the variables, then the config.
//! - `security` and `doctor` reject a production flag.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;
use std::process::Command;

use crate::common::{CommandOutput, fallow_bin, git, scrub_analysis_env};

const TEST_FILE: &str = "src/util.test.ts";

fn project(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"production-rule","main":"src/index.ts","dependencies":{"lodash":"1"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from './util';\nconsole.log(used);\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/util.ts"),
        "export const used = 1;\nexport const testOnly = 2;\n",
    )
    .unwrap();
    std::fs::write(
        root.join(TEST_FILE),
        "import { testOnly } from './util';\n\
         import { map } from 'lodash';\n\
         export function fixture(x: string) { return map([testOnly, x]); }\n",
    )
    .unwrap();
    std::fs::write(root.join(".fallowrc.json"), config).unwrap();
    git(root, &["init", "-q"]);
    dir
}

fn run(root: &Path, args: &[&str]) -> CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    for name in [
        "FALLOW_PRODUCTION_DEAD_CODE",
        "FALLOW_PRODUCTION_HEALTH",
        "FALLOW_PRODUCTION_DUPES",
        "FALLOW_FORMAT",
        "FALLOW_QUIET",
    ] {
        cmd.env_remove(name);
    }
    let output = cmd
        .env("FALLOW_PRODUCTION", "false")
        .current_dir(root)
        .args(args)
        .args(["--format", "json", "--quiet", "--root"])
        .arg(root)
        .output()
        .expect("run fallow");
    CommandOutput::from_process(output)
}

fn json(output: &CommandOutput) -> serde_json::Value {
    serde_json::from_str(&output.stdout)
        .unwrap_or_else(|err| panic!("json output: {err}\n{}\n{}", output.stdout, output.stderr))
}

fn production_project() -> tempfile::TempDir {
    project(r#"{"production": true}"#)
}

#[test]
fn trace_path_production_rule_is_unchanged() {
    let dir = production_project();
    let output = run(dir.path(), &["trace", "--path", TEST_FILE, "src/util.ts"]);
    assert_eq!(output.code, 2, "{}\n{}", output.stdout, output.stderr);
    assert!(
        output.stdout.contains("not found in module graph"),
        "{}",
        output.stdout
    );
}

#[test]
fn trace_chain_production_rule_is_unchanged() {
    let dir = production_project();
    let target = format!("{TEST_FILE}:fixture");
    let output = run(dir.path(), &["trace", &target]);
    assert_eq!(output.code, 2, "{}\n{}", output.stdout, output.stderr);
    assert!(
        output.stdout.contains("not found in module graph"),
        "{}",
        output.stdout
    );
}

#[test]
fn trace_error_production_rule_is_unchanged() {
    let dir = production_project();
    std::fs::write(
        dir.path().join("stack.txt"),
        format!("Error: x\n    at fixture ({TEST_FILE}:3:40)\n"),
    )
    .unwrap();
    let output = run(dir.path(), &["trace-error", "stack.txt"]);
    assert_eq!(output.code, 0, "{}\n{}", output.stdout, output.stderr);
    assert_eq!(json(&output)["frames"][0]["origin"], "out_of_corpus");
}

#[test]
fn trace_dependency_production_rule_is_unchanged() {
    let dir = production_project();
    let output = run(dir.path(), &["trace", "--dependency", "lodash"]);
    assert_eq!(output.code, 0, "{}\n{}", output.stdout, output.stderr);
    assert_eq!(
        json(&output)["imported_by"],
        serde_json::json!([TEST_FILE]),
        "the off variable wins over the config"
    );
}

#[test]
fn inspect_production_rule_is_unchanged() {
    let dir = production_project();
    let output = run(dir.path(), &["inspect", "--file", TEST_FILE]);
    assert_eq!(output.code, 0, "{}\n{}", output.stdout, output.stderr);
    assert_eq!(
        json(&output)["evidence"]["complexity"]["data"]["summary"]["files_analyzed"],
        3,
        "the child processes read the off variable"
    );
}

#[test]
fn baselines_prune_production_rule_is_unchanged() {
    let dir = production_project();
    let root = dir.path();
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"production": true, "audit": {"deadCodeBaseline": "baseline.json"}}"#,
    )
    .unwrap();
    // The save runs without the variable, so the baseline has the production
    // scope of the config.
    let mut save = Command::new(fallow_bin());
    scrub_analysis_env(&mut save);
    let status = save
        .current_dir(root)
        .env_remove("FALLOW_PRODUCTION")
        .args(["dead-code", "--save-baseline", "baseline.json", "--quiet"])
        .arg("--root")
        .arg(root)
        .output()
        .expect("save the baseline");
    assert!(root.join("baseline.json").exists(), "{status:?}");
    let output = run(root, &["baselines", "prune", "--check"]);
    let files = json(&output)["files"].clone();
    assert_eq!(
        files[0]["reason_code"], "scope-mismatch",
        "the off variable wins over the config: {files}"
    );
}

#[test]
fn security_production_flag_still_exits_2() {
    let dir = production_project();
    for flag in ["--production", "--no-production"] {
        let output = run(dir.path(), &["security", flag]);
        assert_eq!(
            output.code, 2,
            "{flag}: {}\n{}",
            output.stdout, output.stderr
        );
    }
}

#[test]
fn doctor_relative_config_joins_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sub = dir.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("package.json"), "{}").unwrap();
    std::fs::write(sub.join("x.json"), r#"{"rules":{"unused-files":"off"}}"#).unwrap();
    let mut cmd = Command::new(fallow_bin());
    scrub_analysis_env(&mut cmd);
    let output = cmd
        .current_dir(dir.path())
        .args([
            "doctor", "--root", "sub", "--config", "x.json", "--format", "json", "--quiet",
        ])
        .output()
        .expect("run fallow");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("doctor json");
    let config = report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["id"] == "config")
        .expect("config check")
        .clone();
    assert_eq!(config["status"], "pass", "{config}");
    assert_eq!(config["message"], "Configuration resolved from x.json.");
}

#[test]
fn doctor_broken_config_is_a_fail_check() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("package.json"), "{}").unwrap();
    std::fs::write(dir.path().join("x.json"), r#"{"rules":"#).unwrap();
    let mut cmd = Command::new(fallow_bin());
    scrub_analysis_env(&mut cmd);
    let output = cmd
        .args([
            "doctor", "--config", "x.json", "--format", "json", "--quiet", "--root",
        ])
        .arg(dir.path())
        .output()
        .expect("run fallow");
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("doctor json");
    assert_eq!(report["status"], "fail");
    let config = report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["id"] == "config")
        .expect("config check")
        .clone();
    assert_eq!(config["status"], "fail", "{config}");
    assert_eq!(
        config["message"],
        "Fallow configuration could not be resolved."
    );
}
