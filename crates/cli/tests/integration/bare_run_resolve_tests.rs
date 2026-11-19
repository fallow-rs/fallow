//! The bare run resolves its config once before the first analysis, so an
//! invalid config stops the run with no analysis output. The parse share of
//! analyses with the same production mode is a guard: the bare run shared
//! that parse before the run resolved once, and it must keep it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;
use std::process::Command;

use crate::common::{CommandOutput, fallow_bin, git, scrub_analysis_env};

fn project(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"bare-resolve","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").unwrap();
    std::fs::write(root.join(".fallowrc.json"), config).unwrap();
    git(root, &["init", "-q"]);
    dir
}

fn run(root: &Path, args: &[&str]) -> CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    for name in [
        "FALLOW_PRODUCTION",
        "FALLOW_PRODUCTION_DEAD_CODE",
        "FALLOW_PRODUCTION_HEALTH",
        "FALLOW_PRODUCTION_DUPES",
        "FALLOW_FORMAT",
        "FALLOW_QUIET",
    ] {
        cmd.env_remove(name);
    }
    let output = cmd
        .current_dir(root)
        .args(args)
        .arg("--root")
        .arg(root)
        .output()
        .expect("run fallow");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

#[test]
fn bare_run_rejects_an_invalid_config_before_analysis() {
    let dir = project(r#"{"plugins":["plugins/escape.json"]}"#);
    let root = dir.path();
    std::fs::create_dir_all(root.join("plugins")).unwrap();
    std::fs::write(
        root.join("plugins/escape.json"),
        r#"{ "name": "escape", "entryPoints": ["../outside/**"] }"#,
    )
    .unwrap();

    let output = run(root, &["--format", "json"]);
    assert_eq!(output.code, 2, "{}", output.stderr);
    let json: serde_json::Value = serde_json::from_str(&output.stdout).expect("json error");
    assert_eq!(json["error"], serde_json::json!(true), "{}", output.stdout);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("invalid external plugin definition:")),
        "{}",
        output.stdout
    );
    for section in ["check", "dupes", "health"] {
        assert!(json.get(section).is_none(), "{section}: {}", output.stdout);
    }

    // The run stops before it loads the config for an analysis, so the human
    // run prints the error and no `loaded config` line.
    let human = run(root, &[]);
    assert_eq!(human.code, 2, "{}", human.stderr);
    assert_eq!(human.stdout, "");
    assert!(
        human
            .stderr
            .starts_with("Error: invalid external plugin definition:"),
        "{}",
        human.stderr
    );
}

/// The human performance report of the health pass says `(measured above)`
/// when health reuses the parse of the dead-code pass.
fn health_reuses_the_parse(root: &Path) -> bool {
    let output = run(root, &["--performance", "--quiet"]);
    assert!(output.code == 0 || output.code == 1, "{}", output.stderr);
    let report = format!("{}{}", output.stdout, output.stderr);
    let Some((_, health)) = report.split_once("Health Pipeline Performance") else {
        panic!("no health performance report: {report}");
    };
    health
        .lines()
        .find(|line| line.contains("parse/extract:"))
        .is_some_and(|line| line.contains("(measured above)"))
}

/// A guard of the parse share: a per-analysis `production` config with equal
/// modes keeps one parse, and a split config keeps two parses.
#[test]
fn per_analysis_equal_modes_share_one_parse() {
    let equal = project(r#"{"production":{"deadCode":true,"health":true,"dupes":true}}"#);
    assert!(health_reuses_the_parse(equal.path()));

    let split = project(r#"{"production":{"deadCode":true,"health":false,"dupes":true}}"#);
    assert!(!health_reuses_the_parse(split.path()));
}
