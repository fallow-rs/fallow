// Shared test harness — not all functions are used by every test file.
#![allow(dead_code, reason = "shared harness included by multiple test crates")]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Typed return from a CLI binary invocation.
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

/// Returns the path to the compiled `fallow` binary for testing.
pub fn fallow_bin() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_fallow").map_or_else(
        || {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.pop(); // crates/
            path.pop(); // project root
            path.push("target/debug/fallow");
            if cfg!(windows) {
                path.set_extension("exe");
            }
            path
        },
        PathBuf::from,
    )
}

/// Returns the absolute path to a test fixture directory.
pub fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/
    path.pop(); // project root
    path.push("tests/fixtures");
    path.push(name);
    path
}

/// Run an arbitrary fallow command against a fixture, returning structured output.
///
/// Sets `NO_COLOR=1` and `RUST_LOG=""` for deterministic output.
/// Injects `--root <fixture_path>` before the caller's args.
pub fn run_fallow(subcommand: &str, fixture: &str, args: &[&str]) -> CommandOutput {
    let bin = fallow_bin();
    let root = fixture_path(fixture);
    let mut cmd = Command::new(&bin);
    cmd.arg(subcommand)
        .arg("--root")
        .arg(&root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1");
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

/// Run fallow with no subcommand (combined mode) against a fixture.
pub fn run_fallow_combined(fixture: &str, args: &[&str]) -> CommandOutput {
    let bin = fallow_bin();
    let root = fixture_path(fixture);
    let mut cmd = Command::new(&bin);
    cmd.arg("--root")
        .arg(&root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1");
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

/// Run fallow with raw args (no --root injection). Useful for error path tests.
pub fn run_fallow_raw(args: &[&str]) -> CommandOutput {
    let bin = fallow_bin();
    let mut cmd = Command::new(&bin);
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
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

/// Parse stdout as JSON, panicking with the raw output on failure.
pub fn parse_json(output: &CommandOutput) -> serde_json::Value {
    serde_json::from_str(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "failed to parse JSON: {e}\nstdout was:\n{}\nstderr was:\n{}",
            output.stdout, output.stderr
        )
    })
}

/// Replace absolute fixture paths with `[ROOT]` and normalize separators.
pub fn redact_paths(s: &str, root: &Path) -> String {
    let root_str = root.to_string_lossy();
    s.replace(root_str.as_ref(), "[ROOT]").replace('\\', "/")
}

/// Replace the crate version with `[VERSION]`.
pub fn redact_version(s: &str) -> String {
    s.replace(env!("CARGO_PKG_VERSION"), "[VERSION]")
}

/// Redact absolute paths and crate version for deterministic snapshots.
pub fn redact_all(s: &str, root: &Path) -> String {
    let s = redact_paths(s, root);
    redact_version(&s)
}
