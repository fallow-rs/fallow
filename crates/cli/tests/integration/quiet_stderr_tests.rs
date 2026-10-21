//! A quiet run keeps stderr free of analysis-diagnostic warnings.
//!
//! The JSON envelope carries each workspace diagnostic, so `--quiet`,
//! `FALLOW_QUIET` and `--ci` hide the stderr warning that repeats it. An
//! explicit `RUST_LOG` shows it again, and config-load warnings stay visible.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::process::Command;

use crate::common::{CommandOutput, fallow_bin, parse_json, scrub_analysis_env};
use tempfile::TempDir;

const PARSE_WARNING: &str = "Parsed 'src/broken.ts'";

/// A project with one reachable file that does not parse.
fn broken_project(config: &str) -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"quiet-fx","version":"1.0.0","private":true,"main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import { broken } from './broken';\nconsole.log(broken);\n",
    )
    .expect("entry");
    std::fs::write(root.join("src/broken.ts"), "export const broken = = 1;\n").expect("broken");
    std::fs::write(root.join(".fallowrc.json"), config).expect("config");
    dir
}

/// Run `dead-code --format json` with the ambient `RUST_LOG` removed, so the
/// CLI default filter decides what reaches stderr.
fn run(dir: &TempDir, extra: &[&str], env: &[(&str, &str)]) -> CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("NO_COLOR", "1")
        .env_remove("RUST_LOG")
        .env_remove("FALLOW_QUIET");
    scrub_analysis_env(&mut cmd);
    cmd.envs(env.iter().copied())
        .arg("dead-code")
        .arg("--root")
        .arg(dir.path())
        .args(["--format", "json", "--no-cache"])
        .args(extra);
    let output = cmd.output().expect("run fallow");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn diagnostic_kinds(output: &CommandOutput) -> Vec<String> {
    parse_json(output)["workspace_diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .filter_map(|diagnostic| diagnostic["kind"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn quiet_hides_the_parse_warning_and_keeps_the_diagnostic() {
    let project = broken_project("{}");
    let output = run(&project, &["--quiet"], &[]);
    assert!(
        !output.stderr.contains(PARSE_WARNING),
        "stderr: {}",
        output.stderr
    );
    assert!(diagnostic_kinds(&output).contains(&"source-parse-degraded".to_owned()));
}

#[test]
fn a_run_without_quiet_prints_the_parse_warning() {
    let project = broken_project("{}");
    let output = run(&project, &[], &[]);
    assert!(
        output.stderr.contains(PARSE_WARNING),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn an_explicit_rust_log_wins_over_quiet() {
    let project = broken_project("{}");
    let output = run(&project, &["--quiet"], &[("RUST_LOG", "warn")]);
    assert!(
        output.stderr.contains(PARSE_WARNING),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn fallow_quiet_acts_as_the_quiet_flag() {
    let project = broken_project("{}");
    let output = run(&project, &[], &[("FALLOW_QUIET", "1")]);
    assert!(
        !output.stderr.contains(PARSE_WARNING),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn quiet_keeps_config_load_warnings() {
    let project = broken_project(r#"{"rules":{"unused-filez":"warn"}}"#);
    let output = run(&project, &["--quiet"], &[]);
    assert!(
        output.stderr.contains("unknown rule 'unused-filez'"),
        "stderr: {}",
        output.stderr
    );
    assert!(!output.stderr.contains(PARSE_WARNING));
}
