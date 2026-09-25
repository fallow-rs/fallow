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
            // Test binaries live in `<target>/<profile>/deps`, next to the
            // `fallow` binary one level up, so this follows CARGO_TARGET_DIR.
            let exe = std::env::current_exe().expect("test binary path");
            let dir = exe.parent().expect("test binary directory");
            let profile_dir = if dir.ends_with("deps") {
                dir.parent().expect("profile directory")
            } else {
                dir
            };
            let mut path = profile_dir.join("fallow");
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

/// Copy a test fixture into a new temporary directory, so a test can write
/// files (for example a saved baseline) inside the project root.
pub fn copy_fixture(name: &str) -> tempfile::TempDir {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("create fixture copy dir");
        for entry in std::fs::read_dir(from).expect("read fixture dir") {
            let entry = entry.expect("fixture entry");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("fixture entry type").is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).expect("copy fixture file");
            }
        }
    }
    let dir = tempfile::tempdir().expect("create fixture copy");
    copy(&fixture_path(name), dir.path());
    dir
}

/// Drop the Istanbul coverage variables a developer shell may export, so a
/// test that exercises the `health.coverage` / `health.coverageRoot` config
/// fallback cannot pass or fail because of ambient `FALLOW_COVERAGE` /
/// `FALLOW_COVERAGE_ROOT` values. Call before applying a test's own env.
pub fn scrub_coverage_env(cmd: &mut Command) {
    cmd.env_remove("FALLOW_COVERAGE")
        .env_remove("FALLOW_COVERAGE_ROOT");
}

/// Build a fallow command with deterministic output settings.
///
/// Sets `NO_COLOR=1` and `RUST_LOG=""` and removes the ambient coverage
/// variables. A test applies its own env after this setup.
fn fallow_command() -> Command {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_coverage_env(&mut cmd);
    cmd
}

/// Run a prepared command and convert its output to [`CommandOutput`].
fn execute(mut cmd: Command) -> CommandOutput {
    let output = cmd.output().expect("failed to run fallow binary");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// Run an arbitrary fallow command against a fixture, returning structured output.
///
/// Injects `--root <fixture_path>` before the caller's args.
pub fn run_fallow(subcommand: &str, fixture: &str, args: &[&str]) -> CommandOutput {
    let root = fixture_path(fixture);
    run_fallow_in_root(subcommand, &root, args)
}

/// Run an arbitrary fallow command against an explicit project root.
pub fn run_fallow_in_root(subcommand: &str, root: &Path, args: &[&str]) -> CommandOutput {
    let mut cmd = fallow_command();
    cmd.arg(subcommand).arg("--root").arg(root).args(args);
    execute(cmd)
}

/// Run fallow with no subcommand (combined mode) against a fixture.
pub fn run_fallow_combined(fixture: &str, args: &[&str]) -> CommandOutput {
    let mut cmd = fallow_command();
    cmd.arg("--root").arg(fixture_path(fixture)).args(args);
    execute(cmd)
}

/// Run fallow with raw args (no --root injection). Useful for error path tests.
pub fn run_fallow_raw(args: &[&str]) -> CommandOutput {
    run_fallow_raw_with_env(args, &[])
}

/// Run fallow with raw args and string environment variables.
pub fn run_fallow_raw_with_env(args: &[&str], env: &[(&str, &str)]) -> CommandOutput {
    let mut cmd = fallow_command();
    cmd.envs(env.iter().copied()).args(args);
    execute(cmd)
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

/// Fields a report is allowed to change between two runs over the same commit.
///
/// `docs/backwards-compatibility.md` publishes this as the definition of a
/// volatile field: everything else in a report must be byte-identical across
/// reruns of the same analysis. Keep the two in step; a determinism gate that
/// quietly grows its allowlist stops being a gate.
///
/// - `elapsed_ms`: measured duration.
/// - `head_sha`: the base snapshot's own commit, which moves when a fixture
///   commits during the test.
/// - `_meta.telemetry.analysis_run_id`: a per-run identifier by construction.
pub const VOLATILE_REPORT_FIELDS: &[&str] = &["elapsed_ms", "head_sha"];

/// Strip every volatile field from a parsed report, in place and at any depth,
/// so what remains can be compared byte for byte between runs.
pub fn strip_volatile_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for field in VOLATILE_REPORT_FIELDS {
                map.remove(*field);
            }
            if let Some(telemetry) = map
                .get_mut("_meta")
                .and_then(|meta| meta.get_mut("telemetry"))
                .and_then(|telemetry| telemetry.as_object_mut())
            {
                telemetry.remove("analysis_run_id");
            }
            for nested in map.values_mut() {
                strip_volatile_fields(nested);
            }
        }
        serde_json::Value::Array(items) => {
            for nested in items {
                strip_volatile_fields(nested);
            }
        }
        _ => {}
    }
}

/// Parse a report and reduce it to its comparable form: one string that two
/// runs over the same commit must agree on exactly.
pub fn canonical_report(output: &CommandOutput) -> String {
    let mut value = parse_json(output);
    strip_volatile_fields(&mut value);
    serde_json::to_string(&value).expect("re-serialize canonical report")
}

/// [`canonical_report`] with `gate_outcomes` removed, at the root and inside
/// each combined section.
///
/// For the comparisons that ask whether an opt-in gate flag changed the report.
/// `gate_outcomes[g].enforced` records whether a verdict is armed, so it is the
/// one member such a flag is meant to move; everything else must stay put.
pub fn canonical_report_without_gate_outcomes(output: &CommandOutput) -> String {
    let mut value = parse_json(output);
    strip_volatile_fields(&mut value);
    strip_gate_outcomes(&mut value);
    serde_json::to_string(&value).expect("re-serialize canonical report")
}

fn strip_gate_outcomes(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("gate_outcomes");
            for nested in map.values_mut() {
                strip_gate_outcomes(nested);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_gate_outcomes(item);
            }
        }
        _ => {}
    }
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

/// Null device for the git config variables. Git for Windows also accepts it.
const GIT_NULL_CONFIG: &str = "/dev/null";
/// Fixed identity for fixture commits.
const GIT_TEST_NAME: &str = "test";
const GIT_TEST_EMAIL: &str = "test@test.com";

/// Build a `git` command for a test fixture in `dir`.
///
/// The command removes the ambient repository variables (`GIT_DIR`,
/// `GIT_WORK_TREE`, `GIT_INDEX_FILE` and more) that a git hook sets, ignores the
/// global and system config, and uses a fixed author and committer. A developer
/// environment thus cannot change or redirect the fixture repository.
pub fn git_command(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    fallow_engine::changed_files::clear_ambient_git_env(&mut cmd);
    cmd.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", GIT_NULL_CONFIG)
        .env("GIT_CONFIG_SYSTEM", GIT_NULL_CONFIG)
        .env("GIT_AUTHOR_NAME", GIT_TEST_NAME)
        .env("GIT_AUTHOR_EMAIL", GIT_TEST_EMAIL)
        .env("GIT_COMMITTER_NAME", GIT_TEST_NAME)
        .env("GIT_COMMITTER_EMAIL", GIT_TEST_EMAIL);
    cmd
}

/// Run git in `dir` and return trimmed stdout. Panics with stdout and stderr
/// when git fails.
pub fn git_capture(dir: &Path, args: &[&str]) -> String {
    let output = git_command(dir)
        .args(args)
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Run git in `dir`. Panics with stdout and stderr when git fails.
pub fn git(dir: &Path, args: &[&str]) {
    git_capture(dir, args);
}

/// Stage every file in `dir` and commit it without a signature.
pub fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", message],
    );
}
