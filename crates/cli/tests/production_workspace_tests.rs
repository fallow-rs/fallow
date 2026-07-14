#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use std::fmt::Write as _;
use std::process::Command;

use common::{
    CommandOutput, fallow_bin, parse_json, run_fallow, run_fallow_in_root, run_fallow_raw,
};
use tempfile::TempDir;

#[test]
fn production_mode_check_exits_successfully() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--production", "--format", "json", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "--production should not crash, got exit {}",
        output.code
    );
    let json = parse_json(&output);
    assert!(
        json.get("total_issues").is_some(),
        "production mode should still produce results"
    );
}

#[test]
fn production_mode_health_exits_successfully() {
    let output = run_fallow(
        "health",
        "basic-project",
        &["--production", "--format", "json", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "health --production should not crash"
    );
}

#[test]
fn production_mode_dupes_exits_successfully() {
    let output = run_fallow(
        "dupes",
        "basic-project",
        &["--production", "--format", "json", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "dupes --production should not crash"
    );
}

fn create_per_analysis_production_fixture() -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"production-modes-test","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("src/index.ts"), "export const ok = 1;\n").unwrap();

    let mut complex =
        String::from("export function complexTest(n: number): number {\n  let total = 0;\n");
    for i in 1..=25 {
        writeln!(complex, "  if (n > {i}) total++;").unwrap();
    }
    complex.push_str("  return total;\n}\n");
    std::fs::write(dir.path().join("src/complex.test.ts"), complex).unwrap();

    Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .status()
        .unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .status()
        .unwrap();
    Command::new("git")
        .args([
            "-c",
            "user.name=fallow",
            "-c",
            "user.email=fallow@example.com",
            "commit",
            "-m",
            "init",
            "-q",
        ])
        .current_dir(dir.path())
        .status()
        .unwrap();

    dir
}

fn run_combined_raw(root: &std::path::Path, args: &[&str], envs: &[(&str, &str)]) -> CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.arg("--root")
        .arg(root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1");
    for (key, value) in envs {
        cmd.env(key, value);
    }
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to run fallow binary");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

#[test]
fn combined_config_can_enable_production_health_only() {
    let dir = create_per_analysis_production_fixture();
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{"production":{"health":true,"deadCode":false,"dupes":false}}"#,
    )
    .unwrap();

    let output = run_combined_raw(dir.path(), &["--format", "json", "--quiet"], &[]);
    assert!(
        output.code == 0 || output.code == 1,
        "combined run should not crash. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let findings = json["health"]["findings"].as_array().unwrap();
    assert!(
        findings.is_empty(),
        "production health should not reuse non-production dead-code parse data: {findings:?}"
    );

    let output = run_combined_raw(
        dir.path(),
        &["--format", "json", "--quiet", "--only", "health"],
        &[],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "combined run should not crash. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let findings = json["health"]["findings"].as_array().unwrap();
    assert!(
        findings.is_empty(),
        "production health should exclude test-only complexity findings: {findings:?}"
    );

    let output = run_combined_raw(
        dir.path(),
        &["--format", "json", "--quiet", "--only", "health"],
        &[("FALLOW_PRODUCTION_HEALTH", "false")],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "env override run should not crash. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let paths: Vec<_> = json["health"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|finding| finding["path"].as_str())
        .collect();
    assert!(
        paths.contains(&"src/complex.test.ts"),
        "FALLOW_PRODUCTION_HEALTH=false should override config and include test complexity: {paths:?}"
    );

    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{"production":{"health":false,"deadCode":false,"dupes":false}}"#,
    )
    .unwrap();
    let output = run_combined_raw(
        dir.path(),
        &["--format", "json", "--quiet", "--only", "health"],
        &[
            ("FALLOW_PRODUCTION", "false"),
            ("FALLOW_PRODUCTION_HEALTH", "true"),
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "specific env override run should not crash. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let findings = json["health"]["findings"].as_array().unwrap();
    assert!(
        findings.is_empty(),
        "FALLOW_PRODUCTION_HEALTH=true should override global false env and exclude test complexity: {findings:?}"
    );
}

#[test]
fn per_analysis_env_var_beats_global_env_var() {
    let dir = create_per_analysis_production_fixture();

    let output = run_combined_raw(
        dir.path(),
        &["--format", "json", "--quiet", "--only", "health"],
        &[
            ("FALLOW_PRODUCTION", "false"),
            ("FALLOW_PRODUCTION_HEALTH", "true"),
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "env-precedence run should not crash. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let findings = json["health"]["findings"].as_array().unwrap();
    assert!(
        findings.is_empty(),
        "FALLOW_PRODUCTION_HEALTH=true must beat FALLOW_PRODUCTION=false (per-analysis env wins): {findings:?}"
    );

    let output = run_combined_raw(
        dir.path(),
        &["--format", "json", "--quiet", "--only", "health"],
        &[
            ("FALLOW_PRODUCTION", "true"),
            ("FALLOW_PRODUCTION_HEALTH", "false"),
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "env-precedence run should not crash. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let paths: Vec<_> = json["health"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|finding| finding["path"].as_str())
        .collect();
    assert!(
        paths.contains(&"src/complex.test.ts"),
        "FALLOW_PRODUCTION_HEALTH=false must beat FALLOW_PRODUCTION=true: {paths:?}"
    );
}

#[test]
fn audit_accepts_production_health_flag() {
    let dir = create_per_analysis_production_fixture();
    let path = dir.path().join("src/complex.test.ts");
    let mut source = std::fs::read_to_string(&path).unwrap();
    source.push_str("\n// touched for audit\n");
    std::fs::write(path, source).unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD",
        "--production-health",
        "--format",
        "json",
        "--quiet",
    ]);
    assert!(
        output.code == 0 || output.code == 1,
        "audit should accept --production-health. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let findings = json["health"]["findings"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        findings.is_empty(),
        "audit --production-health should exclude test complexity findings: {findings:?}"
    );
}

#[test]
fn workspace_scoping_limits_output_to_package() {
    let output = run_fallow(
        "check",
        "workspace-project",
        &["--workspace", "shared", "--format", "json", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "--workspace should not crash, got exit {}. stderr: {}",
        output.code,
        output.stderr
    );
    let json = parse_json(&output);

    for file in json["unused_files"].as_array().unwrap_or(&Vec::new()) {
        let path = file["path"]
            .as_str()
            .expect("unused_files entry should have 'path' string")
            .replace('\\', "/");
        assert!(
            path.contains("packages/shared/"),
            "workspace-scoped unused file should be in packages/shared/, got: {path}"
        );
    }
    for export in json["unused_exports"].as_array().unwrap_or(&Vec::new()) {
        let path = export["path"]
            .as_str()
            .expect("unused_exports entry should have 'path' string")
            .replace('\\', "/");
        assert!(
            path.contains("packages/shared/"),
            "workspace-scoped unused export should be in packages/shared/, got: {path}"
        );
    }
}

#[test]
fn workspace_scoping_on_nonexistent_package() {
    let output = run_fallow(
        "check",
        "workspace-project",
        &[
            "--workspace",
            "nonexistent-pkg",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(
        output.code == 0 || output.code == 2,
        "nonexistent workspace should exit 0 or 2, got {}",
        output.code
    );
}

#[test]
fn regression_baseline_round_trip() {
    let dir = std::env::temp_dir().join(format!("fallow-regression-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let baseline_path = dir.join("regression.json");

    let output = run_fallow(
        "check",
        "basic-project",
        &[
            "--save-regression-baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "save-regression-baseline should not crash"
    );
    assert!(
        baseline_path.exists(),
        "--save-regression-baseline should create file"
    );

    let output = run_fallow(
        "check",
        "basic-project",
        &[
            "--fail-on-regression",
            "--regression-baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "regression check should not crash, got exit {}. stderr: {}",
        output.code,
        output.stderr
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn regression_baseline_without_path_updates_discovered_config() {
    let dir = TempDir::new().expect("create temp dir");
    std::fs::create_dir(dir.path().join("src")).expect("create source directory");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"baseline-config-test","version":"1.0.0"}"#,
    )
    .expect("write package.json");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source file");
    let config_path = dir.path().join(".fallowrc.json");
    std::fs::write(&config_path, r#"{"entry":["src/index.ts"]}"#).expect("write fallow config");

    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--save-regression-baseline", "--format", "json", "--quiet"],
    );

    assert!(
        output.code == 0 || output.code == 1,
        "config baseline save should succeed, got {}: {}",
        output.code,
        output.stderr
    );
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(config_path).expect("read updated fallow config"),
    )
    .expect("updated fallow config should be valid JSON");
    assert_eq!(config["entry"], serde_json::json!(["src/index.ts"]));
    assert!(config.pointer("/regression/baseline/totalIssues").is_some());
}
