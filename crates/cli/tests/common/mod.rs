#![allow(dead_code, reason = "shared harness included by multiple test crates")]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Typed return from a CLI binary invocation.
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

/// The exit code of a finished process.
///
/// # Panics
///
/// Panics when a signal ended the process, so an abort cannot pass an
/// assertion such as `code <= 1`.
pub fn exit_code(output: &std::process::Output) -> i32 {
    output.status.code().unwrap_or_else(|| {
        panic!(
            "the process was ended by a signal: {}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

impl CommandOutput {
    /// Convert the output of a finished process. Panics on a signal exit,
    /// see [`exit_code`].
    pub fn from_process(output: std::process::Output) -> Self {
        let code = exit_code(&output);
        let std::process::Output { stdout, stderr, .. } = output;
        Self {
            code,
            stdout: lossy_string(stdout),
            stderr: lossy_string(stderr),
        }
    }
}

/// Convert process output to a string, and replace each invalid UTF-8
/// sequence with U+FFFD.
fn lossy_string(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

/// How long one sweep of [`remove_orphaned_audit_caches`] holds for the later
/// test processes. The scan reads the whole temp directory, so each test
/// process must not repeat it.
const ORPHAN_SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_mins(10);

/// Remove the audit base worktrees that earlier test runs left in the system
/// temp directory, at most once per [`ORPHAN_SWEEP_INTERVAL`].
///
/// `fallow audit` keeps a reusable base worktree in the temp directory, keyed
/// by the repository. Each test audits its own short-lived repository, so
/// each run leaves one worktree that no later run uses. The audit removes
/// such an entry only after its age limit, so the entries collect (#4671).
fn sweep_orphaned_audit_caches() {
    static SWEPT: std::sync::Once = std::sync::Once::new();
    SWEPT.call_once(|| {
        let temp = std::env::temp_dir();
        let marker = temp.join("fallow-cli-tests-orphan-sweep.stamp");
        let recent = std::fs::metadata(&marker)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|stamped| stamped.elapsed().ok())
            .is_some_and(|elapsed| elapsed < ORPHAN_SWEEP_INTERVAL);
        if recent {
            return;
        }
        let _ = std::fs::write(&marker, "");
        for dir in fallow_engine::audit_snapshot_dir::audit_snapshot_scan_dirs() {
            remove_orphaned_audit_caches_in(&dir, &temp);
        }
    });
}

/// Remove each audit base worktree in `temp` whose owner repository was a
/// directory below `temp` and is gone. Returns the number of removed entries.
///
/// The `.last-used` file next to an entry holds the path of the owner
/// repository. Only a test makes a repository below the temp directory. An
/// entry with an owner at another place stays, also when that owner is
/// missing: it can be a repository on a volume that is not mounted now.
pub fn remove_orphaned_audit_caches(temp: &Path) -> usize {
    remove_orphaned_audit_caches_in(temp, temp)
}

/// [`remove_orphaned_audit_caches`] for the entries in `dir`, which can be
/// the snapshot directory of the user inside `temp`.
fn remove_orphaned_audit_caches_in(dir: &Path, temp: &Path) -> usize {
    const PREFIX: &str = "fallow-audit-base-cache-";
    const OWNER_SUFFIX: &str = ".last-used";

    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let canonical_temp = dunce::canonicalize(temp).unwrap_or_else(|_| temp.to_path_buf());
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(entry_name) = name.strip_suffix(OWNER_SUFFIX) else {
            continue;
        };
        if !entry_name.starts_with(PREFIX) {
            continue;
        }
        let Ok(owner) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let owner = Path::new(owner.trim());
        let below_temp = owner.starts_with(&canonical_temp) || owner.starts_with(temp);
        let gone = matches!(
            std::fs::symlink_metadata(owner),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        );
        if !below_temp || !gone {
            continue;
        }
        let _ = std::fs::remove_dir_all(dir.join(entry_name));
        for suffix in [OWNER_SUFFIX, ".sha", ".lock"] {
            let _ = std::fs::remove_file(dir.join(format!("{entry_name}{suffix}")));
        }
        removed += 1;
    }
    removed
}

/// Returns the path to the compiled `fallow` binary for testing.
///
/// The first call in a test process also removes the audit base worktrees
/// that earlier test runs left behind (see [`sweep_orphaned_audit_caches`]).
pub fn fallow_bin() -> PathBuf {
    sweep_orphaned_audit_caches();
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
    let mut path = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    );
    path.pop(); // crates/
    path.pop(); // project root
    path.push("tests/fixtures");
    path.push(name);
    path
}

/// Copy a test fixture into a new temporary directory, so a test can write
/// files (for example a saved baseline) inside the project root.
pub fn copy_fixture(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create fixture copy");
    copy_fixture_tree(&fixture_path(name), dir.path());
    dir
}

/// Copy a fixture tree without its `.fallow` cache directory and without
/// symlinks. Other tests run fallow directly on the shared fixture and write
/// cache files there in parallel, so a cache file can disappear between the
/// listing and the copy. The fixtures track no symlinks, but some `fallow-core`
/// tests link workspace packages into the `node_modules` folder of a shared
/// fixture. Such a link points at a folder, which `fs::copy` cannot copy.
pub fn copy_fixture_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create fixture copy dir");
    for entry in std::fs::read_dir(from).expect("read fixture dir") {
        let entry = entry.expect("fixture entry");
        let file_type = entry.file_type().expect("fixture entry type");
        if entry.file_name() == ".fallow" || file_type.is_symlink() {
            continue;
        }
        let target = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_fixture_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
}

/// Drop the variables a developer shell may export that change what a run
/// analyzes: the Istanbul coverage inputs (`FALLOW_COVERAGE`,
/// `FALLOW_COVERAGE_ROOT`), `FALLOW_PACKAGE_BASELINES`, which turns the
/// package map off, and the diff inputs that a CI shell exports
/// (`FALLOW_DIFF_FILE`, `FALLOW_DIFF_FILTER`). Call before applying a test's
/// own env.
pub fn scrub_analysis_env(cmd: &mut Command) {
    cmd.env_remove("FALLOW_COVERAGE")
        .env_remove("FALLOW_COVERAGE_ROOT")
        .env_remove("FALLOW_PACKAGE_BASELINES")
        .env_remove("FALLOW_DIFF_FILE")
        .env_remove("FALLOW_DIFF_FILTER")
        // A key that `fallow cloud login` stored on the developer machine must
        // not reach a test.
        .env("FALLOW_CREDENTIAL_STORE", "none")
        // A test suite started inside a Claude Code session must not get the
        // plugin hint on stderr.
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_CHILD_SESSION")
        // A test suite that `npx` or `npm exec` started must not get the
        // `npx fallow` form of the human hints.
        .env_remove("npm_command");
}

/// Build a fallow command with deterministic output settings.
///
/// Sets `NO_COLOR=1` and `RUST_LOG=""` and removes the ambient coverage
/// variables. A test applies its own env after this setup.
pub fn fallow_command() -> Command {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    cmd
}

/// Run a prepared command and convert its output to [`CommandOutput`].
fn execute(mut cmd: Command) -> CommandOutput {
    CommandOutput::from_process(cmd.output().expect("failed to run fallow binary"))
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

/// Run fallow with raw args (no --root injection) in the working directory
/// `dir`, so that a relative path argument resolves against `dir`.
pub fn run_fallow_raw_in_dir(dir: &Path, args: &[&str]) -> CommandOutput {
    run_fallow_raw_in_dir_with_env(dir, args, &[])
}

/// Run fallow with raw args in the working directory `dir` and with string
/// environment variables.
pub fn run_fallow_raw_in_dir_with_env(
    dir: &Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> CommandOutput {
    let mut cmd = fallow_command();
    cmd.current_dir(dir).envs(env.iter().copied()).args(args);
    execute(cmd)
}

/// Run fallow with raw args and string environment variables.
/// Run fallow with an empty global and system Git config, for a command
/// that reads Git settings such as `core.hooksPath`. A developer machine and
/// the hostile-home CI job can set such a setting.
pub fn run_fallow_raw_with_clean_git(args: &[&str]) -> CommandOutput {
    let config = tempfile::NamedTempFile::new().expect("create an empty Git config");
    let path = config.path().to_str().expect("UTF-8 temp path");
    run_fallow_raw_with_env(
        args,
        &[("GIT_CONFIG_GLOBAL", path), ("GIT_CONFIG_SYSTEM", path)],
    )
}

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
    let mut sidecar = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    );
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

/// A fallow command with the repository's real type-aware sidecar and the
/// scrubbed analysis environment.
pub fn type_aware_sidecar_command(args: &[&str]) -> Command {
    let mut cmd = fallow_command();
    configure_type_aware_sidecar(&mut cmd);
    cmd.args(args);
    cmd
}

/// Run fallow with the repository's real type-aware sidecar.
pub fn run_fallow_raw_with_type_aware_sidecar(args: &[&str]) -> CommandOutput {
    execute(type_aware_sidecar_command(args))
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

/// The array at the JSON pointer `pointer` (RFC 6901, for example
/// `/dead_code/unused_files`).
///
/// # Panics
///
/// Panics when the value is absent or is not an array, so a report that lost
/// a section cannot pass as an empty list.
pub fn required_array<'a>(json: &'a serde_json::Value, pointer: &str) -> &'a [serde_json::Value] {
    json.pointer(pointer)
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("no array at `{pointer}` in:\n{json:#}"))
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

/// Build a `git` command for a test fixture in `dir`. See
/// [`fallow_engine::git_fixture::fixture_git_command`].
pub fn git_command(dir: &Path) -> Command {
    fallow_engine::git_fixture::fixture_git_command(dir)
}

/// Run git in `dir` and return trimmed stdout. Panics when git fails.
pub fn git_capture(dir: &Path, args: &[&str]) -> String {
    fallow_engine::git_fixture::fixture_git_capture(dir, args)
}

/// Run git in `dir`. Panics when git fails.
pub fn git(dir: &Path, args: &[&str]) {
    fallow_engine::git_fixture::fixture_git(dir, args);
}

/// Stage every file in `dir` and commit it without a signature.
pub fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", message],
    );
}
