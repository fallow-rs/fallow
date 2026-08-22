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

/// Drop the Istanbul coverage variables a developer shell may export, so a
/// test that exercises the `health.coverage` / `health.coverageRoot` config
/// fallback cannot pass or fail because of ambient `FALLOW_COVERAGE` /
/// `FALLOW_COVERAGE_ROOT` values. Call before applying a test's own env.
pub fn scrub_coverage_env(cmd: &mut Command) {
    cmd.env_remove("FALLOW_COVERAGE")
        .env_remove("FALLOW_COVERAGE_ROOT");
}

/// Run an arbitrary fallow command against a fixture, returning structured output.
///
/// Sets `NO_COLOR=1` and `RUST_LOG=""` for deterministic output.
/// Injects `--root <fixture_path>` before the caller's args.
pub fn run_fallow(subcommand: &str, fixture: &str, args: &[&str]) -> CommandOutput {
    let root = fixture_path(fixture);
    run_fallow_in_root(subcommand, &root, args)
}

/// Run an arbitrary fallow command against an explicit project root.
pub fn run_fallow_in_root(subcommand: &str, root: &Path, args: &[&str]) -> CommandOutput {
    let bin = fallow_bin();
    let mut cmd = Command::new(&bin);
    cmd.arg(subcommand)
        .arg("--root")
        .arg(root)
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
    scrub_coverage_env(&mut cmd);
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

/// Run fallow with raw args and string environment variables.
pub fn run_fallow_raw_with_env(args: &[&str], env: &[(&str, &str)]) -> CommandOutput {
    let bin = fallow_bin();
    let mut cmd = Command::new(&bin);
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_coverage_env(&mut cmd);
    for (key, value) in env {
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

/// Configure a command to use the repository's real type-aware sidecar.
///
/// Windows cannot execute the `.mjs` entry point directly, so the harness
/// mirrors the editor integration by launching the script through Node.
pub fn configure_type_aware_sidecar(cmd: &mut Command) {
    let mut sidecar = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    sidecar.pop(); // crates/
    sidecar.pop(); // project root
    sidecar.push("tools/type-aware-sidecar/fallow-type-aware.mjs");

    #[cfg(windows)]
    let sidecar_bin = {
        let path = std::env::var_os("PATH").expect("PATH must contain the Node.js runtime");
        std::env::split_paths(&path)
            .map(|entry| entry.join("node.exe"))
            .find(|candidate| candidate.is_file())
            .expect("Node.js executable must be available for type-aware CLI tests")
    };
    #[cfg(not(windows))]
    let sidecar_bin = sidecar.clone();

    cmd.env("FALLOW_TYPE_AWARE_BIN", sidecar_bin);
    #[cfg(windows)]
    cmd.env("FALLOW_TYPE_AWARE_SCRIPT", sidecar);
}

/// Run fallow with the repository's real type-aware sidecar.
pub fn run_fallow_raw_with_type_aware_sidecar(args: &[&str]) -> CommandOutput {
    let bin = fallow_bin();
    let mut cmd = Command::new(&bin);
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    configure_type_aware_sidecar(&mut cmd);
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
