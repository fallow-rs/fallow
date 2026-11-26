#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! The telemetry kill switches and the hidden `fallow telemetry record`
//! command that the MCP server runs after an in-process tool call.

use std::path::Path;
use std::process::Command;

use crate::common::{CommandOutput, fallow_bin, parse_json};

/// Run fallow with a clean telemetry environment, a temporary home and
/// `extra_env`.
fn run_fallow(home: &Path, args: &[&str], extra_env: &[(&str, &str)]) -> CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    for name in [
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "DO_NOT_TRACK",
        "FALLOW_TELEMETRY",
        "FALLOW_TELEMETRY_DEBUG",
        "FALLOW_TELEMETRY_DISABLED",
        "FALLOW_AGENT_SOURCE",
        "FALLOW_INTEGRATION_SURFACE",
        "FALLOW_MCP_TOOL",
        "CLAUDECODE",
        "CLAUDE_CODE_CHILD_SESSION",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("APPDATA", home.join("AppData"))
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .args(args);
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    CommandOutput::from_process(cmd.output().expect("failed to run fallow binary"))
}

fn status_state(extra_env: &[(&str, &str)]) -> String {
    let home = tempfile::tempdir().expect("temp home");
    let output = run_fallow(
        home.path(),
        &["--format", "json", "--quiet", "telemetry", "status"],
        extra_env,
    );
    assert_eq!(output.code, 0, "status should exit 0: {}", output.stderr);
    parse_json(&output)["telemetry"]["state"]
        .as_str()
        .expect("state string")
        .to_owned()
}

#[test]
fn do_not_track_with_any_set_value_disables_telemetry() {
    for value in ["1", "true", "yes", "enabled", "y"] {
        assert_eq!(
            status_state(&[("DO_NOT_TRACK", value), ("FALLOW_TELEMETRY", "on")]),
            "disabled_by_admin",
            "DO_NOT_TRACK={value} must disable telemetry"
        );
    }
}

#[test]
fn do_not_track_with_an_off_value_leaves_telemetry_alone() {
    for value in ["0", "false", ""] {
        assert_eq!(
            status_state(&[("DO_NOT_TRACK", value), ("FALLOW_TELEMETRY", "on")]),
            "on",
            "DO_NOT_TRACK={value:?} must not disable telemetry"
        );
    }
}

#[test]
fn admin_kill_switch_with_any_set_value_disables_telemetry() {
    assert_eq!(
        status_state(&[
            ("FALLOW_TELEMETRY_DISABLED", "enabled"),
            ("FALLOW_TELEMETRY", "on"),
        ]),
        "disabled_by_admin"
    );
}

const MCP_RECORD: &[&str] = &["telemetry", "record", "--duration-ms", "750"];

fn mcp_env(tool: &str) -> [(&'static str, String); 3] {
    [
        ("FALLOW_TELEMETRY", "inspect".to_owned()),
        ("FALLOW_INTEGRATION_SURFACE", "mcp".to_owned()),
        ("FALLOW_MCP_TOOL", tool.to_owned()),
    ]
}

fn record_inspect(tool: &str, extra_args: &[&str]) -> CommandOutput {
    let home = tempfile::tempdir().expect("temp home");
    let env = mcp_env(tool);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let args: Vec<&str> = MCP_RECORD.iter().chain(extra_args).copied().collect();
    run_fallow(home.path(), &args, &env)
}

fn parse_event(output: &CommandOutput) -> serde_json::Value {
    let start = output
        .stderr
        .find('{')
        .unwrap_or_else(|| panic!("stderr should hold an event: {}", output.stderr));
    serde_json::from_str(&output.stderr[start..]).expect("valid event JSON")
}

#[test]
fn record_prints_one_mcp_event_for_an_in_process_tool() {
    let output = record_inspect("check_health", &[]);
    assert_eq!(output.code, 0, "record should exit 0: {}", output.stderr);
    assert!(
        output.stdout.trim().is_empty(),
        "record prints nothing on stdout"
    );
    let event = parse_event(&output);
    assert_eq!(event["event"], "workflow_completed");
    assert_eq!(event["workflow"], "health");
    assert_eq!(event["integration_surface"], "mcp");
    assert_eq!(event["mcp_tool"], "check_health");
    assert_eq!(event["output_format"], "json");
    assert_eq!(event["outcome"], "success");
    assert_eq!(event["duration_bucket_ms"], "500-2000");
    assert!(event.get("findings_present").is_none());
}

#[test]
fn record_marks_a_failed_call_as_failed() {
    let output = record_inspect("audit", &["--failed"]);
    let event = parse_event(&output);
    assert_eq!(event["event"], "workflow_failed");
    assert_eq!(event["workflow"], "audit");
    assert_eq!(event["outcome"], "failed");
    assert_eq!(event["failure_reason"], "unknown");
}

#[test]
fn record_drops_a_tool_off_the_allowlist() {
    let output = record_inspect("private_tool_name", &[]);
    assert_eq!(output.code, 0, "record should exit 0: {}", output.stderr);
    assert!(
        !output.stderr.contains('{'),
        "an off-allowlist tool must record nothing: {}",
        output.stderr
    );
}

#[test]
fn record_without_the_mcp_surface_records_nothing() {
    let home = tempfile::tempdir().expect("temp home");
    let output = run_fallow(
        home.path(),
        MCP_RECORD,
        &[
            ("FALLOW_TELEMETRY", "inspect"),
            ("FALLOW_MCP_TOOL", "audit"),
        ],
    );
    assert_eq!(output.code, 0, "record should exit 0: {}", output.stderr);
    assert!(!output.stderr.contains('{'), "stderr: {}", output.stderr);
}

#[test]
fn record_with_telemetry_off_writes_no_spool() {
    let home = tempfile::tempdir().expect("temp home");
    let output = run_fallow(
        home.path(),
        MCP_RECORD,
        &[
            ("FALLOW_INTEGRATION_SURFACE", "mcp"),
            ("FALLOW_MCP_TOOL", "audit"),
        ],
    );
    assert_eq!(output.code, 0, "record should exit 0: {}", output.stderr);
    assert!(output.stderr.trim().is_empty(), "stderr: {}", output.stderr);
    let spool_files: Vec<_> = walk(home.path())
        .into_iter()
        .filter(|path| path.to_string_lossy().contains("telemetry"))
        .collect();
    assert!(spool_files.is_empty(), "no telemetry file: {spool_files:?}");
}

#[test]
fn record_with_telemetry_on_spools_one_event() {
    let home = tempfile::tempdir().expect("temp home");
    let output = run_fallow(
        home.path(),
        MCP_RECORD,
        &[
            ("FALLOW_TELEMETRY", "on"),
            // An unreachable host keeps the drain from a real upload, so the
            // spooled event stays on disk for the assertion.
            ("FALLOW_API_URL", "http://127.0.0.1:9"),
            ("FALLOW_INTEGRATION_SURFACE", "mcp"),
            ("FALLOW_MCP_TOOL", "find_dupes"),
        ],
    );
    assert_eq!(output.code, 0, "record should exit 0: {}", output.stderr);
    let spool = walk(home.path())
        .into_iter()
        .find(|path| path.ends_with("telemetry-spool.jsonl"))
        .expect("the event is spooled");
    let raw = std::fs::read_to_string(spool).expect("read spool");
    let lines: Vec<&str> = raw.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines.len(), 1, "one spooled event: {raw}");
    assert!(lines[0].contains("\"mcp_tool\":\"find_dupes\""), "{raw}");
    assert!(lines[0].contains("\"workflow\":\"dupes\""), "{raw}");
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn claim(home: &Path, extra_env: &[(&str, &str)]) -> bool {
    let output = run_fallow(home, &["telemetry", "claim-prompt"], extra_env);
    assert_eq!(
        output.code, 0,
        "claim-prompt should exit 0: {}",
        output.stderr
    );
    parse_json(&output)["ask"]
        .as_bool()
        .expect("an ask boolean")
}

#[test]
fn the_first_claim_asks_and_the_next_claim_waits() {
    let home = tempfile::tempdir().expect("temp home");
    assert!(claim(home.path(), &[]), "the first claim may ask");
    assert!(
        !claim(home.path(), &[]),
        "a second claim within 14 days may not"
    );
}

#[test]
fn no_claim_with_a_kill_switch_an_override_or_in_ci() {
    for env in [
        ("DO_NOT_TRACK", "1"),
        ("FALLOW_TELEMETRY_DISABLED", "yes"),
        ("FALLOW_TELEMETRY", "off"),
        ("FALLOW_TELEMETRY_DEBUG", "1"),
        ("CI", "true"),
    ] {
        let home = tempfile::tempdir().expect("temp home");
        assert!(!claim(home.path(), &[env]), "{env:?} must refuse the claim");
    }
}

#[test]
fn no_claim_after_a_recorded_decision() {
    for decision in ["enable", "disable"] {
        let home = tempfile::tempdir().expect("temp home");
        let output = run_fallow(home.path(), &["telemetry", decision], &[]);
        assert_eq!(output.code, 0, "{decision}: {}", output.stderr);
        assert!(
            !claim(home.path(), &[]),
            "a recorded `{decision}` must refuse the claim"
        );
    }
}
