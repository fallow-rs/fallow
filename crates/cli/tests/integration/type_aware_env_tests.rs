//! The `FALLOW_TYPE_AWARE` and `FALLOW_TYPE_AWARE_REQUIRE` values of the
//! analysis commands: each command that reads type-aware settings rejects a
//! value that it cannot parse with exit code 2, and a command that reads no
//! type-aware setting ignores the variables.

use std::ffi::OsStr;
use std::process::Command;

use super::common::{copy_fixture, fallow_bin, scrub_analysis_env};

const BOOL_MESSAGE: &str =
    "FALLOW_TYPE_AWARE must be one of true, false, 1, 0, yes, no, on, or off";

fn run_with_env(subcommand: &str, args: &[&str], env: &[(&str, &OsStr)]) -> (i32, String) {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    for (name, value) in env {
        cmd.env(name, value);
    }
    // A copy outside the repository, so no config of the repository applies.
    let root = copy_fixture("basic-project");
    cmd.arg(subcommand)
        .arg("--root")
        .arg(root.path())
        .args(args);
    let output = cmd.output().expect("run fallow");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (crate::common::exit_code(&output), text)
}

fn unknown(subcommand: &str, args: &[&str]) -> (i32, String) {
    run_with_env(
        subcommand,
        args,
        &[("FALLOW_TYPE_AWARE", OsStr::new("maybe"))],
    )
}

#[test]
fn dead_code_rejects_an_unknown_type_aware_value() {
    let (code, text) = unknown("dead-code", &["--format", "json", "--quiet"]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains(BOOL_MESSAGE), "{text}");
}

#[test]
fn health_rejects_an_unknown_type_aware_value() {
    let (code, text) = unknown("health", &["--format", "json", "--quiet"]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains(BOOL_MESSAGE), "{text}");
}

/// A flag decides the setting, but the CLI still rejects the variable.
#[test]
fn a_type_aware_flag_does_not_hide_an_unknown_value() {
    let (code, text) = unknown(
        "dead-code",
        &["--no-type-aware", "--format", "json", "--quiet"],
    );
    assert_eq!(code, 2, "{text}");
    assert!(text.contains(BOOL_MESSAGE), "{text}");
}

/// `fallow dupes` reads no type-aware setting, so the value does not stop it.
#[test]
fn dupes_ignores_an_unknown_type_aware_value() {
    let (code, text) = unknown("dupes", &["--format", "json", "--quiet"]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn dead_code_rejects_an_unknown_type_aware_require_value() {
    let (code, text) = run_with_env(
        "dead-code",
        &["--format", "json", "--quiet"],
        &[("FALLOW_TYPE_AWARE_REQUIRE", OsStr::new("sometimes"))],
    );
    assert_eq!(code, 2, "{text}");
    assert!(
        text.contains("FALLOW_TYPE_AWARE_REQUIRE must be best-effort or complete"),
        "{text}"
    );
}

#[cfg(unix)]
fn not_unicode() -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(vec![0x66, 0x80, 0x6f])
}

#[cfg(unix)]
#[test]
fn health_rejects_a_non_utf8_type_aware_value() {
    let value = not_unicode();
    let (code, text) = run_with_env(
        "health",
        &["--format", "json", "--quiet"],
        &[("FALLOW_TYPE_AWARE", value.as_os_str())],
    );
    assert_eq!(code, 2, "{text}");
    assert!(
        text.contains("FALLOW_TYPE_AWARE must be valid UTF-8"),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn dead_code_rejects_a_non_utf8_type_aware_require_value() {
    let value = not_unicode();
    let (code, text) = run_with_env(
        "dead-code",
        &["--format", "json", "--quiet"],
        &[("FALLOW_TYPE_AWARE_REQUIRE", value.as_os_str())],
    );
    assert_eq!(code, 2, "{text}");
    assert!(
        text.contains("FALLOW_TYPE_AWARE_REQUIRE must be valid UTF-8"),
        "{text}"
    );
}
