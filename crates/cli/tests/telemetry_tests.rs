#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{fallow_bin, parse_json};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", message],
    );
}

fn telemetry_command(args: &[&str]) -> common::CommandOutput {
    let home = tempfile::tempdir().expect("temp home");
    let mut cmd = Command::new(fallow_bin());
    cmd.env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITLAB_CI")
        .env_remove("DO_NOT_TRACK")
        .env_remove("FALLOW_TELEMETRY")
        .env_remove("FALLOW_TELEMETRY_DEBUG")
        .env_remove("FALLOW_TELEMETRY_DISABLED")
        .env_remove("FALLOW_AGENT_SOURCE")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("APPDATA", home.path().join("AppData"))
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1");
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to run fallow binary");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

#[test]
fn telemetry_status_json_is_parseable() {
    let output = telemetry_command(&["--format", "json", "--quiet", "telemetry", "status"]);
    assert_eq!(
        output.code, 0,
        "telemetry status should exit 0: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["telemetry"]["state"].as_str(), Some("off"));
    assert_eq!(json["telemetry"]["source"].as_str(), Some("default"));
}

#[test]
fn non_telemetry_subcommands_still_dispatch_normally() {
    let output = telemetry_command(&["--format", "json", "--quiet", "explain", "unused-exports"]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["id"].as_str(), Some("fallow/unused-export"));
}

#[test]
fn telemetry_inspect_preserves_command_stdout_json() {
    let mut cmd = Command::new(fallow_bin());
    let home = tempfile::tempdir().expect("temp home");
    cmd.env("FALLOW_TELEMETRY", "inspect")
        .env("FALLOW_AGENT_SOURCE", "claude-code")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("APPDATA", home.path().join("AppData"))
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .arg("--parent-run")
        .arg("../repo/main")
        .args(["--format", "json", "--quiet"]);
    let raw = cmd.output().expect("failed to run fallow binary");
    let output = common::CommandOutput {
        stdout: String::from_utf8_lossy(&raw.stdout).to_string(),
        stderr: String::from_utf8_lossy(&raw.stderr).to_string(),
        code: raw.status.code().unwrap_or(-1),
    };

    assert_eq!(output.code, 0, "analysis should exit 0: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["kind"].as_str(), Some("combined"));
    assert_eq!(json["schema_version"].as_u64(), Some(7));

    let event_start = output
        .stderr
        .find('{')
        .expect("inspect stderr should contain telemetry JSON");
    let event: serde_json::Value = serde_json::from_str(&output.stderr[event_start..])
        .expect("inspect stderr should contain valid telemetry JSON");
    assert_eq!(event["invocation_context"].as_str(), Some("agent"));
    assert_eq!(event["agent_source"].as_str(), Some("claude_code"));
    assert_eq!(event.get("parent_run"), None);
    assert_eq!(event["has_parent_run"].as_bool(), Some(false));
    assert_eq!(event["run_role"].as_str(), Some("unknown"));
    assert_eq!(event["followup_kind"].as_str(), Some("unknown"));
}

#[test]
fn telemetry_inspect_reports_safe_followup_fields() {
    let mut cmd = Command::new(fallow_bin());
    let home = tempfile::tempdir().expect("temp home");
    cmd.env("FALLOW_TELEMETRY", "inspect")
        .env("FALLOW_AGENT_SOURCE", "codex")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("APPDATA", home.path().join("AppData"))
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .arg("--parent-run")
        .arg("tmp_8x7p4k")
        .args(["--format", "json", "--quiet", "explain", "unused-exports"]);
    let raw = cmd.output().expect("failed to run fallow binary");
    let output = common::CommandOutput {
        stdout: String::from_utf8_lossy(&raw.stdout).to_string(),
        stderr: String::from_utf8_lossy(&raw.stderr).to_string(),
        code: raw.status.code().unwrap_or(-1),
    };

    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    let event_start = output
        .stderr
        .find('{')
        .expect("inspect stderr should contain telemetry JSON");
    let event: serde_json::Value = serde_json::from_str(&output.stderr[event_start..])
        .expect("inspect stderr should contain valid telemetry JSON");
    assert_eq!(event.get("parent_run"), None);
    assert_eq!(event["has_parent_run"].as_bool(), Some(true));
    assert_eq!(event["run_role"].as_str(), Some("followup"));
    assert_eq!(event["followup_kind"].as_str(), Some("explain"));
}

#[test]
fn invalid_explicit_agent_source_is_ignored() {
    let mut cmd = Command::new(fallow_bin());
    let home = tempfile::tempdir().expect("temp home");
    cmd.env("FALLOW_TELEMETRY", "inspect")
        .env("FALLOW_AGENT_SOURCE", "private-agent-x")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("APPDATA", home.path().join("AppData"))
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .args(["--format", "json", "--quiet"]);
    let raw = cmd.output().expect("failed to run fallow binary");
    let output = common::CommandOutput {
        stdout: String::from_utf8_lossy(&raw.stdout).to_string(),
        stderr: String::from_utf8_lossy(&raw.stderr).to_string(),
        code: raw.status.code().unwrap_or(-1),
    };

    assert_eq!(output.code, 0, "analysis should exit 0: {}", output.stderr);
    let event_start = output
        .stderr
        .find('{')
        .expect("inspect stderr should contain telemetry JSON");
    let event: serde_json::Value = serde_json::from_str(&output.stderr[event_start..])
        .expect("inspect stderr should contain valid telemetry JSON");
    assert_ne!(event["agent_source"].as_str(), Some("private-agent-x"));
    assert!(event.get("agent_source").is_none());
}

/// Run a fallow command in telemetry inspect mode against `root`, applying
/// `extra_env`, and return the parsed telemetry event emitted to stderr.
fn inspect_event_output(
    root: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> (serde_json::Value, common::CommandOutput) {
    let home = tempfile::tempdir().expect("temp home");
    let mut cmd = Command::new(fallow_bin());
    cmd.env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITLAB_CI")
        .env_remove("DO_NOT_TRACK")
        .env_remove("FALLOW_TELEMETRY_DISABLED")
        .env_remove("FALLOW_AGENT_SOURCE")
        .env_remove("FALLOW_INTEGRATION_SURFACE")
        .env_remove("FALLOW_MCP_TOOL")
        .env("FALLOW_TELEMETRY", "inspect")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("APPDATA", home.path().join("AppData"))
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .args(["--root", &root.to_string_lossy()])
        .args(args);
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    let raw = cmd.output().expect("failed to run fallow binary");
    let output = common::CommandOutput {
        stdout: String::from_utf8_lossy(&raw.stdout).to_string(),
        stderr: String::from_utf8_lossy(&raw.stderr).to_string(),
        code: raw.status.code().unwrap_or(-1),
    };
    let event_start = output.stderr.find('{').unwrap_or_else(|| {
        panic!(
            "inspect stderr should contain telemetry JSON; stderr was: {}",
            output.stderr
        )
    });
    let event = serde_json::from_str(&output.stderr[event_start..])
        .expect("inspect stderr should contain valid telemetry JSON");
    (event, output)
}

fn inspect_event(root: &Path, args: &[&str], extra_env: &[(&str, &str)]) -> serde_json::Value {
    inspect_event_output(root, args, extra_env).0
}

/// Minimal project with a function duplicated across two files, sized to clear
/// the default duplication thresholds (min_tokens 50, min_lines 5,
/// min_occurrences 2).
fn write_duplicated_project(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        dir.join("package.json"),
        "{\n  \"name\": \"dup-fixture\"\n}\n",
    )
    .expect("write package.json");
    let block = "  let total = 0;\n  let count = 0;\n  for (const item of items) {\n    total = total + item * 2;\n    count = count + 1;\n  }\n  const average = total / Math.max(count, 1);\n  const doubled = average * 2;\n  const adjusted = doubled + count - 1;\n  return adjusted + total + count;\n";
    for name in ["a", "b"] {
        fs::write(
            src.join(format!("{name}.ts")),
            format!("export function compute_{name}(items: number[]): number {{\n{block}}}\n"),
        )
        .expect("write source file");
    }
}

fn write_clean_project(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(dir.join("package.json"), "{\n  \"name\": \"clean\"\n}\n")
        .expect("write package.json");
    fs::write(src.join("index.ts"), "export const value = 41 + 1;\n").expect("write source");
}

fn write_cache_project(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        dir.join("package.json"),
        "{\n  \"name\": \"cache-fixture\",\n  \"main\": \"src/index.ts\"\n}\n",
    )
    .expect("write package.json");
    fs::write(
        src.join("index.ts"),
        "import { used } from './used';\nconsole.log(used());\n",
    )
    .expect("write index");
    fs::write(
        src.join("used.ts"),
        "export const used = (): number => 42;\n",
    )
    .expect("write used");
}

fn write_many_unused_project(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        dir.join("package.json"),
        "{\n  \"name\": \"many-unused\",\n  \"main\": \"src/index.ts\"\n}\n",
    )
    .expect("write package.json");
    fs::write(src.join("index.ts"), "console.log('entry');\n").expect("write index");
    for index in 0..12 {
        fs::write(
            src.join(format!("unused-{index}.ts")),
            format!("export const unused{index} = {index};\n"),
        )
        .expect("write unused source");
    }
}

fn write_audit_base_project(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        dir.join("package.json"),
        "{\n  \"name\": \"audit-fixture\",\n  \"main\": \"src/index.ts\"\n}\n",
    )
    .expect("write package.json");
    fs::write(
        src.join("index.ts"),
        "import { used } from './used';\nconsole.log(used());\n",
    )
    .expect("write index");
    fs::write(
        src.join("used.ts"),
        "export const used = (): number => 42;\n",
    )
    .expect("write used");
}

fn init_audit_repo(dir: &Path) {
    git(dir, &["init", "-b", "main"]);
    write_audit_base_project(dir);
    commit_all(dir, "base");
}

fn write_security_project(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        dir.join("package.json"),
        "{\n  \"name\": \"security-fixture\",\n  \"dependencies\": {\"express\": \"4.18.0\"}\n}\n",
    )
    .expect("write package.json");
    fs::write(
        src.join("app.ts"),
        "import express from 'express';\nconst app = express();\napp.post('/run', (req, res) => {\n  eval(req.body.code);\n  res.send('ok');\n});\n",
    )
    .expect("write security source");
}

#[test]
fn dupes_with_duplication_sets_findings_present_despite_success_outcome() {
    let dir = tempfile::tempdir().expect("temp project");
    write_duplicated_project(dir.path());
    let event = inspect_event(dir.path(), &["dupes", "--format", "json", "--quiet"], &[]);
    assert_eq!(event["workflow"].as_str(), Some("dupes"));
    // The default duplication threshold is 0.0 ("never gate"), so the run exits
    // 0 / outcome=success even with 100% duplication. findings_present is the
    // signal that decouples "found something" from the gate.
    assert_eq!(event["outcome"].as_str(), Some("success"));
    assert_eq!(
        event["findings_present"].as_bool(),
        Some(true),
        "dupes with real duplication must report findings_present=true"
    );
    assert_eq!(event["result_count_bucket"].as_str(), Some("1-9"));
}

#[test]
fn dupes_on_clean_project_sets_findings_present_false() {
    let dir = tempfile::tempdir().expect("temp project");
    write_clean_project(dir.path());
    let event = inspect_event(dir.path(), &["dupes", "--format", "json", "--quiet"], &[]);
    assert_eq!(event["workflow"].as_str(), Some("dupes"));
    assert!(
        event.get("failure_reason").is_none(),
        "successful workflows must omit failure_reason"
    );
    assert_eq!(event["run_scope"].as_str(), Some("full_project"));
    assert_eq!(event["config_shape"].as_str(), Some("default"));
    assert_eq!(event["output_destination"].as_str(), Some("stdout"));
    assert_eq!(event["analysis_mode"].as_str(), Some("static"));
    assert_eq!(
        event["findings_present"].as_bool(),
        Some(false),
        "a genuinely clean dupes run must report findings_present=false"
    );
    assert_eq!(event["result_count_bucket"].as_str(), Some("0"));
}

#[test]
fn file_scoped_custom_rules_file_output_are_coarse_context_dimensions() {
    let dir = tempfile::tempdir().expect("temp project");
    write_clean_project(dir.path());
    let config_path = dir.path().join(".fallowrc.json");
    let output_path = dir.path().join("report.json");
    fs::write(
        &config_path,
        "{\n  \"rules\": {\n    \"unused-files\": \"warn\"\n  }\n}\n",
    )
    .expect("write config");
    let config_arg = config_path.to_string_lossy().to_string();
    let output_arg = output_path.to_string_lossy().to_string();

    let (event, output) = inspect_event_output(
        dir.path(),
        &[
            "--config",
            &config_arg,
            "--output-file",
            &output_arg,
            "dead-code",
            "--file",
            "src/index.ts",
            "--format",
            "json",
            "--quiet",
        ],
        &[],
    );

    assert_eq!(
        output.code, 0,
        "file scoped run should pass: {}",
        output.stderr
    );
    assert_eq!(event["workflow"].as_str(), Some("dead_code"));
    assert_eq!(event["run_scope"].as_str(), Some("file_scoped"));
    assert_eq!(event["config_shape"].as_str(), Some("custom_rules"));
    assert_eq!(event["output_destination"].as_str(), Some("file"));
    assert_eq!(event["analysis_mode"].as_str(), Some("static"));
    assert!(
        !event
            .to_string()
            .contains(&dir.path().to_string_lossy().to_string()),
        "telemetry event must not include raw project paths"
    );
}

#[test]
fn validation_failure_sets_failure_reason() {
    let dir = tempfile::tempdir().expect("temp project");
    write_clean_project(dir.path());

    let (event, output) = inspect_event_output(
        dir.path(),
        &[
            "--only",
            "dead-code",
            "--skip",
            "dupes",
            "--format",
            "json",
            "--quiet",
        ],
        &[],
    );

    assert_eq!(output.code, 2, "validation should fail: {}", output.stderr);
    assert_eq!(event["event"].as_str(), Some("workflow_failed"));
    assert_eq!(event["workflow"].as_str(), Some("code_quality_review"));
    assert_eq!(event["failure_reason"].as_str(), Some("validation"));
}

#[test]
fn diff_setup_failure_sets_failure_reason() {
    let dir = tempfile::tempdir().expect("temp project");
    write_clean_project(dir.path());

    let (event, output) = inspect_event_output(
        dir.path(),
        &[
            "--diff-file",
            "changes.diff",
            "--diff-stdin",
            "--format",
            "json",
            "--quiet",
        ],
        &[],
    );

    assert_eq!(output.code, 2, "diff setup should fail: {}", output.stderr);
    assert_eq!(event["event"].as_str(), Some("workflow_failed"));
    assert_eq!(event["workflow"].as_str(), Some("code_quality_review"));
    assert_eq!(event["failure_reason"].as_str(), Some("diff"));
}

#[test]
fn unsupported_format_failure_sets_failure_reason() {
    let dir = tempfile::tempdir().expect("temp project");
    write_clean_project(dir.path());

    let (event, output) =
        inspect_event_output(dir.path(), &["--format", "sarif", "--quiet", "impact"], &[]);

    assert_eq!(
        output.code, 2,
        "unsupported format should fail: {}",
        output.stderr
    );
    assert_eq!(event["event"].as_str(), Some("workflow_failed"));
    assert_eq!(event["workflow"].as_str(), Some("impact"));
    assert_eq!(event["failure_reason"].as_str(), Some("unsupported_format"));
}

#[test]
fn code_quality_review_reports_cold_then_partial_cache_state() {
    let dir = tempfile::tempdir().expect("temp project");
    write_cache_project(dir.path());

    let (first_event, first_output) =
        inspect_event_output(dir.path(), &["--format", "json", "--quiet"], &[]);

    assert_eq!(
        first_output.code, 0,
        "first combined run should exit 0: {}",
        first_output.stderr
    );
    assert_eq!(
        first_event["workflow"].as_str(),
        Some("code_quality_review")
    );
    assert_eq!(first_event["cache_state"].as_str(), Some("cold"));

    let (second_event, second_output) =
        inspect_event_output(dir.path(), &["--format", "json", "--quiet"], &[]);

    assert_eq!(
        second_output.code, 0,
        "second combined run should exit 0: {}",
        second_output.stderr
    );
    assert_eq!(
        second_event["workflow"].as_str(),
        Some("code_quality_review")
    );
    assert_eq!(second_event["cache_state"].as_str(), Some("partial"));
}

#[test]
fn code_quality_review_with_no_cache_reports_unknown_cache_state() {
    let dir = tempfile::tempdir().expect("temp project");
    write_cache_project(dir.path());

    let (event, output) = inspect_event_output(
        dir.path(),
        &["--no-cache", "--format", "json", "--quiet"],
        &[],
    );

    assert_eq!(
        output.code, 0,
        "combined run should exit 0: {}",
        output.stderr
    );
    assert_eq!(event["workflow"].as_str(), Some("code_quality_review"));
    assert_eq!(event["cache_state"].as_str(), Some("unknown"));
}

#[test]
fn audit_with_findings_sets_findings_present_true() {
    let dir = tempfile::tempdir().expect("temp project");
    init_audit_repo(dir.path());
    fs::write(
        dir.path().join("src/orphan.ts"),
        "export const orphaned = 'nobody';\n",
    )
    .expect("write orphan");
    commit_all(dir.path(), "add orphan");

    let (event, output) = inspect_event_output(
        dir.path(),
        &["audit", "--base", "HEAD~1", "--format", "json", "--quiet"],
        &[],
    );

    assert_eq!(
        output.code, 1,
        "audit with findings should exit 1: {}",
        output.stderr
    );
    assert_eq!(event["workflow"].as_str(), Some("audit"));
    assert_eq!(event["outcome"].as_str(), Some("issues_found"));
    assert_eq!(event["run_scope"].as_str(), Some("changed_only"));
    assert_eq!(event["output_destination"].as_str(), Some("stdout"));
    assert_eq!(event["analysis_mode"].as_str(), Some("static"));
    assert_eq!(event["findings_present"].as_bool(), Some(true));
    assert_eq!(event["result_count_bucket"].as_str(), Some("1-9"));
}

#[test]
fn audit_on_clean_changed_files_sets_findings_present_false() {
    let dir = tempfile::tempdir().expect("temp project");
    init_audit_repo(dir.path());
    fs::write(dir.path().join("README.md"), "# Audit fixture\n").expect("write readme");
    commit_all(dir.path(), "docs only");

    let (event, output) = inspect_event_output(
        dir.path(),
        &["audit", "--base", "HEAD~1", "--format", "json", "--quiet"],
        &[],
    );

    assert_eq!(
        output.code, 0,
        "clean audit should exit 0: {}",
        output.stderr
    );
    assert_eq!(event["workflow"].as_str(), Some("audit"));
    assert_eq!(event["outcome"].as_str(), Some("success"));
    assert_eq!(event["findings_present"].as_bool(), Some(false));
    assert_eq!(event["result_count_bucket"].as_str(), Some("0"));
}

#[test]
fn audit_with_no_changed_files_sets_findings_present_false() {
    let dir = tempfile::tempdir().expect("temp project");
    init_audit_repo(dir.path());

    let (event, output) = inspect_event_output(
        dir.path(),
        &["audit", "--base", "HEAD", "--format", "json", "--quiet"],
        &[],
    );

    assert_eq!(
        output.code, 0,
        "empty audit should exit 0: {}",
        output.stderr
    );
    assert_eq!(event["workflow"].as_str(), Some("audit"));
    assert_eq!(event["outcome"].as_str(), Some("success"));
    assert_eq!(event["findings_present"].as_bool(), Some(false));
    assert_eq!(event["result_count_bucket"].as_str(), Some("0"));
}

#[test]
fn security_with_findings_sets_findings_present_true() {
    let dir = tempfile::tempdir().expect("temp project");
    write_security_project(dir.path());

    let (event, output) = inspect_event_output(
        dir.path(),
        &["security", "--format", "json", "--quiet"],
        &[],
    );

    assert_eq!(output.code, 0, "security should exit 0: {}", output.stderr);
    assert_eq!(event["workflow"].as_str(), Some("security"));
    assert_eq!(event["outcome"].as_str(), Some("success"));
    assert_eq!(event["analysis_mode"].as_str(), Some("security"));
    assert_eq!(event["findings_present"].as_bool(), Some(true));
    assert_eq!(event["result_count_bucket"].as_str(), Some("1-9"));
}

#[test]
fn fix_dry_run_reports_fix_analysis_mode() {
    let dir = tempfile::tempdir().expect("temp project");
    write_clean_project(dir.path());

    let (event, output) = inspect_event_output(
        dir.path(),
        &["fix", "--dry-run", "--format", "json", "--quiet"],
        &[],
    );

    assert_eq!(output.code, 0, "fix dry-run should pass: {}", output.stderr);
    assert_eq!(event["workflow"].as_str(), Some("fix"));
    assert_eq!(event["analysis_mode"].as_str(), Some("fix"));
}

#[test]
fn health_coverage_flag_reports_runtime_coverage_mode_without_path() {
    let dir = tempfile::tempdir().expect("temp project");
    write_clean_project(dir.path());

    let (event, output) = inspect_event_output(
        dir.path(),
        &[
            "health",
            "--coverage",
            "missing-coverage.json",
            "--format",
            "json",
            "--quiet",
        ],
        &[],
    );

    assert_eq!(
        output.code, 2,
        "missing coverage should fail: {}",
        output.stderr
    );
    assert_eq!(event["workflow"].as_str(), Some("health"));
    assert_eq!(event["analysis_mode"].as_str(), Some("runtime_coverage"));
    assert!(
        !event.to_string().contains("missing-coverage.json"),
        "telemetry event must not include raw coverage paths"
    );
}

#[test]
fn security_on_clean_project_sets_findings_present_false() {
    let dir = tempfile::tempdir().expect("temp project");
    write_clean_project(dir.path());

    let (event, output) = inspect_event_output(
        dir.path(),
        &["security", "--format", "json", "--quiet"],
        &[],
    );

    assert_eq!(output.code, 0, "security should exit 0: {}", output.stderr);
    assert_eq!(event["workflow"].as_str(), Some("security"));
    assert_eq!(event["outcome"].as_str(), Some("success"));
    assert_eq!(event["findings_present"].as_bool(), Some(false));
    assert_eq!(event["result_count_bucket"].as_str(), Some("0"));
}

#[test]
fn review_output_reports_comment_limit_truncation() {
    let dir = tempfile::tempdir().expect("temp project");
    write_many_unused_project(dir.path());

    let (event, output) = inspect_event_output(
        dir.path(),
        &["dead-code", "--format", "review-github", "--quiet"],
        &[("FALLOW_MAX_COMMENTS", "1")],
    );

    assert_eq!(
        output.code, 1,
        "dead-code with findings should exit 1: {}",
        output.stderr
    );
    assert_eq!(event["workflow"].as_str(), Some("dead_code"));
    assert_eq!(event["output_format"].as_str(), Some("review_github"));
    assert_eq!(event["findings_present"].as_bool(), Some(true));
    assert_eq!(event["result_count_bucket"].as_str(), Some("10-99"));
    assert_eq!(event["report_truncated"].as_bool(), Some(true));
    assert_eq!(event["truncation_reason"].as_str(), Some("comment_limit"));
}

#[test]
fn admin_command_emits_no_findings_present_key() {
    let dir = tempfile::tempdir().expect("temp project");
    std::fs::write(dir.path().join("package.json"), "{\n  \"name\": \"x\"\n}\n")
        .expect("write package.json");
    // `explain` runs no analysis, so the findings-present accumulator stays
    // unset and the key is absent (distinguishable from a clean false).
    let event = inspect_event(dir.path(), &["explain", "unused-exports"], &[]);
    assert_eq!(event["workflow"].as_str(), Some("explain"));
    assert!(
        event.get("findings_present").is_none(),
        "commands that run no analysis must omit findings_present"
    );
    assert!(
        event.get("result_count_bucket").is_none(),
        "commands that run no analysis must omit result_count_bucket"
    );
    assert!(
        event.get("cache_state").is_none(),
        "commands that run no analysis must omit cache_state"
    );
}

#[test]
fn project_inventory_command_routes_to_project_inventory_without_findings_present() {
    let dir = tempfile::tempdir().expect("temp project");
    std::fs::write(dir.path().join("package.json"), "{\n  \"name\": \"x\"\n}\n")
        .expect("write package.json");

    let (event, output) = inspect_event_output(dir.path(), &["list"], &[]);

    assert_eq!(output.code, 0, "list should exit 0: {}", output.stderr);
    assert_eq!(event["workflow"].as_str(), Some("project_inventory"));
    assert!(
        event.get("findings_present").is_none(),
        "project-inventory commands must omit findings_present"
    );
}

#[test]
fn setup_command_routes_to_setup_without_findings_present() {
    let dir = tempfile::tempdir().expect("temp project");
    std::fs::write(dir.path().join("package.json"), "{\n  \"name\": \"x\"\n}\n")
        .expect("write package.json");

    let (event, output) = inspect_event_output(dir.path(), &["init"], &[]);

    assert_eq!(output.code, 0, "init should exit 0: {}", output.stderr);
    assert_eq!(event["workflow"].as_str(), Some("setup"));
    assert!(
        event.get("findings_present").is_none(),
        "setup commands must omit findings_present"
    );
}

#[test]
fn license_command_routes_to_license_without_findings_present() {
    let dir = tempfile::tempdir().expect("temp project");
    std::fs::write(dir.path().join("package.json"), "{\n  \"name\": \"x\"\n}\n")
        .expect("write package.json");

    let (event, output) = inspect_event_output(dir.path(), &["license", "status"], &[]);

    assert_eq!(
        output.code, 3,
        "license status should exit 3 when no local license exists: {}",
        output.stderr
    );
    assert_eq!(event["workflow"].as_str(), Some("license"));
    assert_eq!(event["outcome"].as_str(), Some("failed"));
    assert!(
        event.get("findings_present").is_none(),
        "license commands must omit findings_present"
    );
}

#[test]
fn mcp_surface_override_tags_event_with_tool() {
    let dir = tempfile::tempdir().expect("temp project");
    std::fs::write(dir.path().join("package.json"), "{\n  \"name\": \"x\"\n}\n")
        .expect("write package.json");
    let event = inspect_event(
        dir.path(),
        &["dupes", "--format", "json", "--quiet"],
        &[
            ("FALLOW_INTEGRATION_SURFACE", "mcp"),
            ("FALLOW_MCP_TOOL", "find_dupes"),
        ],
    );
    assert_eq!(
        event["integration_surface"].as_str(),
        Some("mcp"),
        "the surface override must re-tag the event as mcp instead of cli_json"
    );
    assert_eq!(event["mcp_tool"].as_str(), Some("find_dupes"));
}

#[test]
fn off_allowlist_mcp_tool_is_dropped() {
    let dir = tempfile::tempdir().expect("temp project");
    std::fs::write(dir.path().join("package.json"), "{\n  \"name\": \"x\"\n}\n")
        .expect("write package.json");
    let event = inspect_event(
        dir.path(),
        &["dupes", "--format", "json", "--quiet"],
        &[
            ("FALLOW_INTEGRATION_SURFACE", "mcp"),
            ("FALLOW_MCP_TOOL", "/etc/passwd"),
        ],
    );
    assert_eq!(event["integration_surface"].as_str(), Some("mcp"));
    assert!(
        event.get("mcp_tool").is_none(),
        "an off-allowlist FALLOW_MCP_TOOL value must be dropped, never echoed"
    );
}
