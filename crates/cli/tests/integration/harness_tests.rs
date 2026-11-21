//! The strict helpers of the test harness: a signal exit, a missing JSON
//! array and an ambient analysis variable must not pass a test.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::process::Command;

use serde_json::json;

use crate::common::{CommandOutput, fallow_command, required_array, type_aware_sidecar_command};

/// How a command sets one environment variable.
#[derive(Debug, PartialEq, Eq)]
enum EnvState {
    /// The command does not name the variable, so the child inherits it.
    Inherited,
    /// The command removes the variable.
    Removed,
    /// The command sets the variable to this value.
    Set(String),
}

fn env_state(command: &Command, name: &str) -> EnvState {
    match command
        .get_envs()
        .find(|(key, _)| key.to_str() == Some(name))
    {
        None => EnvState::Inherited,
        Some((_, None)) => EnvState::Removed,
        Some((_, Some(value))) => EnvState::Set(value.to_string_lossy().into_owned()),
    }
}

#[test]
fn required_array_returns_an_empty_array() {
    let json = json!({ "comments": [] });
    assert!(required_array(&json, "/comments").is_empty());
}

#[test]
#[should_panic(expected = "no array at")]
fn required_array_rejects_a_missing_section() {
    let json = json!({ "verdict": "pass" });
    let _ = required_array(&json, "/dead_code/unused_files");
}

#[test]
#[should_panic(expected = "no array at")]
fn required_array_rejects_a_value_that_is_not_an_array() {
    let json = json!({ "error": true, "comments": "none" });
    let _ = required_array(&json, "/comments");
}

#[cfg(unix)]
#[test]
#[should_panic(expected = "ended by a signal")]
fn exit_code_rejects_a_signal_exit() {
    let output = Command::new("sh")
        .args(["-c", "kill -ABRT $$"])
        .output()
        .expect("run sh");
    let _ = crate::common::exit_code(&output);
}

#[cfg(unix)]
#[test]
fn exit_code_returns_the_code_of_a_normal_exit() {
    let output = Command::new("sh")
        .args(["-c", "exit 1"])
        .output()
        .expect("run sh");
    assert_eq!(crate::common::exit_code(&output), 1);
}

#[test]
fn from_process_keeps_the_code_and_the_output_of_a_normal_exit() {
    let output = fallow_command().arg("--help").output().expect("run fallow");
    let converted = CommandOutput::from_process(output);
    assert_eq!(converted.code, 0, "stderr: {}", converted.stderr);
    assert!(!converted.stdout.is_empty());
}

#[test]
fn the_fallow_command_removes_the_ambient_diff_inputs() {
    let command = fallow_command();
    for name in ["FALLOW_DIFF_FILE", "FALLOW_DIFF_FILTER"] {
        assert_eq!(env_state(&command, name), EnvState::Removed, "{name}");
    }
}

#[test]
fn an_explicit_diff_input_wins_over_the_scrub() {
    let mut command = fallow_command();
    command.env("FALLOW_DIFF_FILE", "x.diff");
    assert_eq!(
        env_state(&command, "FALLOW_DIFF_FILE"),
        EnvState::Set("x.diff".to_owned())
    );
}

#[test]
fn the_sidecar_command_scrubs_the_analysis_environment() {
    let command = type_aware_sidecar_command(&["dead-code"]);
    assert_eq!(env_state(&command, "FALLOW_COVERAGE"), EnvState::Removed);
    assert_eq!(
        env_state(&command, "FALLOW_CREDENTIAL_STORE"),
        EnvState::Set("none".to_owned())
    );
    assert!(matches!(
        env_state(&command, "FALLOW_TYPE_AWARE_BIN"),
        EnvState::Set(_)
    ));
}
