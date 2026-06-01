#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use std::process::Command;

use common::{fallow_bin, parse_json};

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
