#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! Parity guard for the multicall binary.
//!
//! The packaged `fallow` binary is built from `fallow-multicall`, which
//! delegates every non-server invocation to `fallow_cli::run` verbatim. These
//! tests pin that: `fallow-multicall <args>` must match the standalone
//! `fallow` CLI binary byte-for-byte on stdout, stderr, and exit code, so a
//! future edit cannot silently fork one entry point from the other. They also
//! assert the version line keeps the exact `fallow X.Y.Z` shape the VS Code
//! binary-skew probe parses.

use std::path::PathBuf;
use std::process::Command;

/// Resolve the standalone `fallow` CLI binary that builds alongside the
/// multicall binary in the same cargo target directory. A `-p fallow-multicall`
/// only run does not build the `fallow` bin (it lives in another package), so
/// build it on demand with the same profile when it is missing.
fn fallow_cli_binary() -> PathBuf {
    let multicall = PathBuf::from(env!("CARGO_BIN_EXE_fallow-multicall"));
    let target_dir = multicall
        .parent()
        .expect("multicall binary has a parent directory");
    let exe = if cfg!(windows) {
        "fallow.exe"
    } else {
        "fallow"
    };
    let fallow = target_dir.join(exe);
    if !fallow.exists() {
        let mut build = Command::new(env!("CARGO"));
        build.args(["build", "-p", "fallow-cli", "--bin", "fallow"]);
        if target_dir.file_name().and_then(|name| name.to_str()) == Some("release") {
            build.arg("--release");
        }
        let status = build
            .status()
            .expect("spawn cargo build for the fallow bin");
        assert!(status.success(), "building the fallow CLI bin failed");
    }
    fallow
}

fn is_semver(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

#[test]
fn version_matches_cli_and_keeps_fallow_shape() {
    let multicall = env!("CARGO_BIN_EXE_fallow-multicall");
    let cli = fallow_cli_binary();

    let multicall_out = Command::new(multicall)
        .arg("--version")
        .output()
        .expect("run fallow-multicall --version");
    let cli_out = Command::new(&cli)
        .arg("--version")
        .output()
        .expect("run fallow --version");

    assert!(
        multicall_out.status.success(),
        "multicall --version exited non-zero"
    );
    assert_eq!(
        multicall_out.status.code(),
        cli_out.status.code(),
        "--version exit code parity"
    );
    assert_eq!(
        multicall_out.stdout, cli_out.stdout,
        "--version stdout parity between multicall and CLI"
    );

    let stdout = String::from_utf8(multicall_out.stdout).expect("utf8 version output");
    let first_line = stdout.lines().next().unwrap_or_default();
    // The VS Code binary-skew probe parses exactly `fallow X.Y.Z`.
    let mut tokens = first_line.split(' ');
    assert_eq!(
        tokens.next(),
        Some("fallow"),
        "version line must start with `fallow`: {first_line:?}"
    );
    assert!(
        tokens.next().is_some_and(is_semver),
        "version line must be `fallow X.Y.Z`: {first_line:?}"
    );
}

#[test]
fn real_command_matches_cli() {
    let multicall = env!("CARGO_BIN_EXE_fallow-multicall");
    let cli = fallow_cli_binary();
    // Human-format `explain` is fully deterministic; the JSON envelope carries a
    // per-run `analysis_run_id` that would differ between any two invocations.
    let args = ["explain", "unused-files"];

    let multicall_out = Command::new(multicall)
        .args(args)
        .output()
        .expect("run multicall explain");
    let cli_out = Command::new(&cli)
        .args(args)
        .output()
        .expect("run cli explain");

    assert_eq!(
        multicall_out.status.code(),
        cli_out.status.code(),
        "explain exit code parity"
    );
    assert_eq!(
        multicall_out.stdout, cli_out.stdout,
        "explain stdout parity"
    );
    assert_eq!(
        multicall_out.stderr, cli_out.stderr,
        "explain stderr parity"
    );
}
