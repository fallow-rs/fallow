//! Tests for the install lookup of the generated gate script.
//!
//! Each test runs the rendered script in bash with a reduced `PATH` and stub
//! launchers. A stub below the version floor makes the lookup visible: the
//! block message names the install that the script selected.

use std::path::{Path, PathBuf};

use tempfile::tempdir;

use super::{render_gate_script, rendered_gate_script};

/// Runs the rendered project-scope gate from inside `project`, installed at
/// `project/.claude/hooks/fallow-gate.sh`, with `PATH` reduced to
/// `extra_bin` plus the system directories, so no globally installed fallow
/// can satisfy the resolution the test is probing. A fake fallow below the
/// version floor makes resolution observable: reaching the floor check at
/// all proves the binary was found.
fn probe_gate_resolution(project: &Path, extra_bin: &Path) -> std::process::Output {
    let script_path = project.join(".claude/hooks/fallow-gate.sh");
    probe_gate(
        &rendered_gate_script(),
        &script_path,
        project,
        extra_bin,
        br#"{"tool_input":{"command":"git commit -m test"}}"#,
    )
}

/// Writes `script` to `script_path` and runs it from `cwd` with `input` on
/// stdin. See [`probe_gate_resolution`] for the reduced `PATH`.
fn probe_gate(
    script: &str,
    script_path: &Path,
    cwd: &Path,
    extra_bin: &Path,
    input: &[u8],
) -> std::process::Output {
    probe_gate_with_path_prefix(script, script_path, cwd, extra_bin, input, "")
}

/// Same as [`probe_gate`], with `path_prefix` put before the `PATH` entries.
fn probe_gate_with_path_prefix(
    script: &str,
    script_path: &Path,
    cwd: &Path,
    extra_bin: &Path,
    input: &[u8],
    path_prefix: &str,
) -> std::process::Output {
    // jq is linked INTO the probe directory rather than having its own
    // directory added to PATH: on a developer machine that directory also
    // holds the globally installed fallow, which would satisfy the very
    // resolution step under test.
    if let Some(jq) = std::process::Command::new("sh")
        .args(["-c", "command -v jq"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|path| !path.is_empty())
    {
        let _ = std::os::unix::fs::symlink(jq, extra_bin.join("jq"));
    }
    let path = format!("{path_prefix}{}:/usr/bin:/bin", extra_bin.display());
    probe_gate_with_path(script, script_path, cwd, &path, input, &[])
}

/// Same as [`probe_gate`], with `path` as the whole `PATH` and `envs` added
/// to the environment.
fn probe_gate_with_path(
    script: &str,
    script_path: &Path,
    cwd: &Path,
    path: &str,
    input: &[u8],
    envs: &[(&str, &Path)],
) -> std::process::Output {
    use std::io::Write;

    std::fs::create_dir_all(script_path.parent().unwrap()).unwrap();
    std::fs::write(script_path, script).unwrap();

    let mut child = std::process::Command::new("bash")
        .arg(script_path)
        .current_dir(cwd)
        .env("PATH", path)
        .envs(envs.iter().copied())
        // An unreachable floor makes resolution observable: the block
        // message names the runner the script chose.
        .env("FALLOW_GATE_MIN_VERSION", "999.0.0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn bash");
    child.stdin.as_mut().unwrap().write_all(input).unwrap();
    child.wait_with_output().expect("wait")
}

fn write_stale_fallow(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

/// A real installed Git hook keeps its caller's PATH, so a project-local
/// install is invisible to `command -v`. Reaching the version floor proves
/// the launcher was resolved rather than the audit silently skipped.
#[test]
fn gate_resolves_a_project_local_launcher_absent_from_path() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    write_stale_fallow(
        &project.join("node_modules/.bin/fallow"),
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'fallow 2.30.0'; exit 0; fi\nexit 0\n",
    );
    std::fs::create_dir_all(project.join("empty-bin")).unwrap();

    let output = probe_gate_resolution(project, &project.join("empty-bin"));
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("/node_modules/.bin/fallow is fallow 2.30.0"),
        "the project-local launcher must be the runner the script chose: {stderr}"
    );
}

/// Yarn Plug'n'Play has no `node_modules/.bin` at all, so neither the
/// launcher check nor `npx --no-install` can see the install.
#[test]
fn gate_resolves_a_yarn_plug_n_play_install() {
    let tmp = tempdir().unwrap();
    let project = &tmp.path().join("project");
    std::fs::create_dir_all(project).unwrap();
    let bin = tmp.path().join("bin");
    write_stale_fallow(
        &bin.join("yarn"),
        "#!/bin/sh\ncase \"$1 $2\" in\n  'bin fallow') echo \"$PWD/.yarn/fallow\"; exit 0 ;;\nesac\nif [ \"$1\" = exec ]; then\n  shift 3\n  if [ \"$1\" = \"--version\" ]; then echo 'fallow 2.30.0'; exit 0; fi\n  exit 0\nfi\nexit 1\n",
    );
    assert!(!project.join("node_modules").exists());

    let output = probe_gate_resolution(project, &bin);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("yarn exec fallow is fallow 2.30.0"),
        "the Plug'n'Play install must be the runner the script chose: {stderr}"
    );
}

/// A `fallow` launcher on PATH can fail to start, for example a global npm
/// shim whose package is gone (`MODULE_NOT_FOUND`). The gate must then use
/// the working project-local install. Without the version check the gate
/// selects the broken launcher, reads an empty report and skips the audit.
#[test]
fn gate_skips_a_path_launcher_that_cannot_run() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    let bin = project.join("bin");
    write_stale_fallow(
        &bin.join("fallow"),
        "#!/bin/sh\necho \"Error: Cannot find module 'fallow'\" >&2\nexit 1\n",
    );
    write_stale_fallow(
        &project.join("node_modules/.bin/fallow"),
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'fallow 2.30.0'; exit 0; fi\nexit 0\n",
    );

    let output = probe_gate_resolution(project, &bin);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("/node_modules/.bin/fallow is fallow 2.30.0"),
        "the working local install must be the runner the script chose: {stderr}"
    );
}

/// pnpm often installs only a project-local launcher. When that launcher
/// exits with `MODULE_NOT_FOUND` and no other install works, the gate must
/// block and name the launcher and its error. Before, the gate printed
/// "binary not found", exited 0 and let the commit pass without an audit.
#[test]
fn gate_blocks_when_the_only_launcher_cannot_run() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    write_stale_fallow(
        &project.join("node_modules/.bin/fallow"),
        "#!/bin/sh\necho \"Error: Cannot find module '@fallow-cli/darwin-arm64'\" >&2\necho 'Require stack:' >&2\nexit 1\n",
    );
    std::fs::create_dir_all(project.join("empty-bin")).unwrap();

    let output = probe_gate_resolution(project, &project.join("empty-bin"));
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains(
            "/node_modules/.bin/fallow cannot print its fallow version (Error: Cannot find module '@fallow-cli/darwin-arm64')"
        ),
        "the gate must name the broken launcher and its first error line: {stderr}"
    );
    assert!(!stderr.contains("Require stack"), "{stderr}");
    assert!(!stderr.contains("binary not found"), "{stderr}");
}

/// The gate reports the first launcher that failed, not a later one. The
/// launcher of the install root comes before the one on PATH.
#[test]
fn gate_names_the_first_broken_launcher() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    let bin = project.join("bin");
    write_stale_fallow(
        &bin.join("fallow"),
        "#!/bin/sh\necho 'path shim broken' >&2\nexit 1\n",
    );
    write_stale_fallow(
        &project.join("node_modules/.bin/fallow"),
        "#!/bin/sh\necho 'local launcher broken' >&2\nexit 1\n",
    );

    let output = probe_gate_resolution(project, &bin);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("(local launcher broken)"), "{stderr}");
    assert!(
        stderr.contains("/node_modules/.bin/fallow cannot print"),
        "{stderr}"
    );
}

/// A project pins its fallow version in its dependencies. A global install
/// on PATH can have another version and another packaging, so the gate must
/// run the project-local launcher when both work.
#[test]
fn gate_prefers_the_project_local_launcher_over_path() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    let bin = project.join("bin");
    write_stale_fallow(
        &bin.join("fallow"),
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'fallow 2.10.0'; exit 0; fi\nexit 0\n",
    );
    write_stale_fallow(
        &project.join("node_modules/.bin/fallow"),
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'fallow 2.30.0'; exit 0; fi\nexit 0\n",
    );

    let output = probe_gate_resolution(project, &bin);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("/node_modules/.bin/fallow is fallow 2.30.0"),
        "the project-local launcher must be the runner the script chose: {stderr}"
    );
}

/// With no launcher at all, the gate keeps the commit and names the skip.
#[test]
fn gate_names_the_skip_when_no_launcher_exists() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    std::fs::create_dir_all(project.join("empty-bin")).unwrap();

    let output = probe_gate_resolution(project, &project.join("empty-bin"));
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("fallow binary not found"),
        "the gate must name the skip: {stderr}"
    );
}

/// The version-floor message names the fix for the runner that the gate
/// chose: the devDependency for a project-local launcher, the global install
/// for a fallow on PATH.
#[test]
fn gate_floor_message_names_the_fix_for_the_chosen_runner() {
    let version =
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'fallow 2.30.0'; exit 0; fi\nexit 0\n";

    let tmp = tempdir().unwrap();
    let project = tmp.path();
    write_stale_fallow(&project.join("node_modules/.bin/fallow"), version);
    std::fs::create_dir_all(project.join("empty-bin")).unwrap();
    let output = probe_gate_resolution(project, &project.join("empty-bin"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("update the fallow devDependency of the project"),
        "{stderr}"
    );
    assert!(!stderr.contains("upgrade the fallow on PATH"), "{stderr}");

    let tmp = tempdir().unwrap();
    let project = &tmp.path().join("project");
    std::fs::create_dir_all(project).unwrap();
    write_stale_fallow(&tmp.path().join("bin/fallow"), version);
    let output = probe_gate_resolution(project, &tmp.path().join("bin"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("upgrade the fallow on PATH"), "{stderr}");
    assert!(!stderr.contains("devDependency"), "{stderr}");
}

/// The JSON reader uses the same order as the audit runner: the launcher of
/// the install root before the fallow on PATH. A hook input that the gate reads
/// without help still needs the reader for the audit report.
#[test]
fn gate_json_reader_prefers_the_project_local_launcher_over_path() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    let log = project.join("calls.log");
    let stub = |name: &str| {
        format!(
            "#!/bin/sh\necho \"{name} $*\" >> '{}'\nif [ \"$1 $2 $3\" = 'hooks gate-runtime probe' ]; then echo fallow-gate-json-v1; exit 0; fi\nif [ \"$1\" = \"--version\" ]; then echo 'fallow 2.30.0'; exit 0; fi\nexit 1\n",
            log.display()
        )
    };
    write_stale_fallow(&project.join("bin/fallow"), &stub("path"));
    write_stale_fallow(&project.join("node_modules/.bin/fallow"), &stub("local"));

    let output = probe_gate_resolution(project, &project.join("bin"));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();

    assert!(
        calls.contains("local hooks gate-runtime probe"),
        "{calls}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!calls.contains("path hooks gate-runtime"), "{calls}");
}

/// Writes a stub fallow at `path` that adds `name` to `log` on each call and
/// reports a version below the floor of the probe.
fn logging_stub(path: &Path, name: &str, log: &Path) {
    write_stale_fallow(
        path,
        &format!(
            "#!/bin/sh\necho \"{name} $*\" >> '{}'\nif [ \"$1\" = \"--version\" ]; then echo 'fallow 2.30.0'; exit 0; fi\nexit 1\n",
            log.display()
        ),
    );
}

fn calls(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

fn session_input(cwd: &Path) -> Vec<u8> {
    serde_json::json!({
        "cwd": cwd,
        "tool_input": { "command": "git commit -m test" },
    })
    .to_string()
    .into_bytes()
}

/// A user-scope gate applies in every repository, also in a fresh clone. It
/// must never run a launcher from the session directory or its parents; it
/// runs the fallow on PATH.
#[test]
fn user_scope_gate_never_runs_a_launcher_of_the_session_tree() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let home = base.join("home");
    let clone = base.join("parent/clone");
    let session = clone.join("src");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(clone.join("package.json"), "{}").unwrap();
    let log = base.join("calls.log");
    logging_stub(&session.join("node_modules/.bin/fallow"), "session", &log);
    logging_stub(&clone.join("node_modules/.bin/fallow"), "clone", &log);
    logging_stub(
        &base.join("parent/node_modules/.bin/fallow"),
        "parent",
        &log,
    );
    logging_stub(&base.join("bin/fallow"), "path", &log);

    let output = probe_gate(
        &render_gate_script(true, None),
        &home.join(".claude/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        &session_input(&session),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let calls = calls(&log);

    assert!(calls.contains("path --version"), "{calls}\n{stderr}");
    for name in ["session", "clone", "parent"] {
        assert!(!calls.contains(name), "{name} ran: {calls}\n{stderr}");
    }
    assert_eq!(output.status.code(), Some(2), "{stderr}");
}

/// A user-scope gate runs the fallow that installed it before the fallow on
/// PATH.
#[test]
fn user_scope_gate_runs_the_recorded_fallow_first() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    std::fs::create_dir_all(&session).unwrap();
    let log = base.join("calls.log");
    let recorded: PathBuf = base.join("tools/fallow");
    logging_stub(&recorded, "recorded", &log);
    logging_stub(&base.join("bin/fallow"), "path", &log);
    logging_stub(&session.join("node_modules/.bin/fallow"), "session", &log);

    let output = probe_gate(
        &render_gate_script(true, recorded.to_str()),
        &base.join("home/.codex/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        &session_input(&session),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let calls = calls(&log);

    assert!(
        stderr.contains(&format!("{} is fallow 2.30.0", recorded.display())),
        "{stderr}"
    );
    assert!(!calls.contains("path --version"), "{calls}");
    assert!(!calls.contains("session"), "{calls}");
}

/// A project-scope gate runs only the launcher of its install root. A nested
/// package or repository below the install root does not supply the program.
#[test]
fn project_scope_gate_never_runs_a_launcher_below_the_install_root() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().canonicalize().unwrap();
    let nested = project.join("vendor/pkg");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("package.json"), "{}").unwrap();
    let log = project.join("calls.log");
    logging_stub(&nested.join("node_modules/.bin/fallow"), "nested", &log);
    logging_stub(&project.join("node_modules/.bin/fallow"), "root", &log);
    std::fs::create_dir_all(project.join("empty-bin")).unwrap();

    let output = probe_gate(
        &rendered_gate_script(),
        &project.join(".claude/hooks/fallow-gate.sh"),
        &nested,
        &project.join("empty-bin"),
        &session_input(&nested),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let calls = calls(&log);

    assert!(calls.contains("root --version"), "{calls}\n{stderr}");
    assert!(!calls.contains("nested"), "{calls}\n{stderr}");
}

/// The gate reads the command before it starts any fallow. A hook input whose
/// command is not a git write, but whose other fields name one, runs nothing.
#[test]
fn gate_runs_no_fallow_for_a_command_that_is_not_a_git_write() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().canonicalize().unwrap();
    let log = project.join("calls.log");
    logging_stub(&project.join("bin/fallow"), "path", &log);
    logging_stub(&project.join("node_modules/.bin/fallow"), "root", &log);
    let input = serde_json::json!({
        "cwd": project,
        "tool_input": {
            "command": "ls -la",
            "description": "list files before git commit and git push"
        },
    })
    .to_string();

    let output = probe_gate(
        &rendered_gate_script(),
        &project.join(".claude/hooks/fallow-gate.sh"),
        &project,
        &project.join("bin"),
        input.as_bytes(),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        calls(&log),
        "",
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The small hook input reader decodes JSON string escapes, so a command with
/// escaped quotes or a new line still reaches the exact check.
#[test]
fn gate_reads_an_escaped_command_without_a_json_reader() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let log = base.join("calls.log");
    logging_stub(&base.join("bin/fallow"), "path", &log);
    let input = serde_json::json!({
        "session_id": "x",
        "cwd": project,
        "tool_input": { "command": "echo \"a\\tb\"\ngit commit -m \"fix: x\"", "timeout": 5 },
        "nested": [{ "command": "ls" }, null, true, 1.5e3],
    })
    .to_string();

    let output = probe_gate(
        &rendered_gate_script(),
        &project.join(".claude/hooks/fallow-gate.sh"),
        &project,
        &base.join("bin"),
        input.as_bytes(),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("is fallow 2.30.0, below required"),
        "{stderr}"
    );
}

/// A relative or empty `PATH` entry names a directory below the working
/// directory, which is the session tree. The gate skips such entries.
#[test]
fn gate_skips_relative_and_empty_path_entries() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    std::fs::create_dir_all(&session).unwrap();
    let log = base.join("calls.log");
    logging_stub(&session.join("node_modules/.bin/fallow"), "relative", &log);
    logging_stub(&session.join("fallow"), "empty", &log);
    logging_stub(&base.join("bin/fallow"), "path", &log);

    let output = probe_gate_with_path_prefix(
        &render_gate_script(true, None),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        &session_input(&session),
        "node_modules/.bin::.:",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let calls = calls(&log);

    assert!(calls.contains("path --version"), "{calls}\n{stderr}");
    assert!(!calls.contains("relative"), "{calls}");
    assert!(!calls.contains("empty"), "{calls}");
}

/// A large hook input with many escapes goes to a JSON reader instead of the
/// small reader, so the gate stays fast.
#[test]
fn gate_reads_a_large_escaped_hook_input_in_bounded_time() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let log = base.join("calls.log");
    logging_stub(&base.join("bin/fallow"), "path", &log);
    let message = "a\"b\\".repeat(25_000);
    let input = serde_json::json!({
        "cwd": project,
        "tool_input": { "command": format!("git commit -m '{message}'") },
    })
    .to_string();
    assert!(input.len() > 100_000);

    let started = std::time::Instant::now();
    let output = probe_gate(
        &rendered_gate_script(),
        &project.join(".claude/hooks/fallow-gate.sh"),
        &project,
        &base.join("bin"),
        input.as_bytes(),
    );
    let elapsed = started.elapsed();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(elapsed < std::time::Duration::from_secs(10), "{elapsed:?}");
    if stderr.contains("no native JSON reader or jq found") {
        return;
    }
    assert_eq!(output.status.code(), Some(2), "{stderr}");
}

/// Every program of the gate (git, jq, cat and the others) resolves only
/// through absolute `PATH` entries without a `node_modules` component, in any
/// letter case. A package manager puts the `node_modules/.bin` of the project
/// first on `PATH`.
#[test]
fn gate_runs_no_helper_from_a_package_or_relative_path_entry() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    std::fs::create_dir_all(&session).unwrap();
    let log = base.join("calls.log");
    let helpers = ["git", "jq", "cat", "sed", "mktemp", "dirname", "fallow"];
    for dir in [
        session.join("node_modules/.bin"),
        session.join("NODE_MODULES/.bin"),
        session.join("rel"),
    ] {
        for helper in helpers {
            logging_stub(&dir.join(helper), &format!("stub-{helper}"), &log);
        }
    }
    logging_stub(&base.join("bin/fallow"), "path", &log);
    let prefix = format!(
        "{}:{}:rel:",
        session.join("node_modules/.bin").display(),
        session.join("NODE_MODULES/.bin").display()
    );

    let output = probe_gate_with_path_prefix(
        &render_gate_script(true, None),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        &session_input(&session),
        &prefix,
    );
    let calls = calls(&log);

    assert!(!calls.contains("stub-"), "{calls}");
    assert!(
        calls.contains("path --version"),
        "{calls}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_init(dir: &Path) {
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .status()
        .unwrap();
    assert!(status.success());
}

/// Writes a logging stub for each helper of the gate into `dir`.
fn helper_stubs(dir: &Path, log: &Path) {
    for helper in ["git", "jq", "cat", "sed", "tr", "mktemp", "fallow"] {
        logging_stub(&dir.join(helper), &format!("stub-{helper}"), log);
    }
}

/// A tool such as direnv puts a directory of the project on PATH as an
/// absolute entry. The gate drops each PATH entry below the git top level of
/// the session, also one above the session directory.
#[test]
fn gate_runs_no_helper_from_a_path_entry_below_the_session_git_top() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let clone = base.join("clone");
    let session = clone.join("src");
    std::fs::create_dir_all(&session).unwrap();
    git_init(&clone);
    let log = base.join("calls.log");
    helper_stubs(&clone.join(".venv/bin"), &log);
    helper_stubs(&session.join("bin"), &log);
    logging_stub(&base.join("bin/fallow"), "path", &log);
    let prefix = format!(
        "{}:{}:",
        clone.join(".venv/bin").display(),
        session.join("bin").display()
    );

    let output = probe_gate_with_path_prefix(
        &render_gate_script(true, None),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        &session_input(&session),
        &prefix,
    );
    let calls = calls(&log);

    assert!(!calls.contains("stub-"), "{calls}");
    assert!(
        calls.contains("path --version"),
        "{calls}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A project-scope gate also drops each PATH entry below its install root,
/// when the session directory is in another tree.
#[test]
fn project_scope_gate_runs_no_helper_from_a_path_entry_below_the_install_root() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let project = base.join("project");
    let session = base.join("elsewhere");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&session).unwrap();
    let log = base.join("calls.log");
    helper_stubs(&project.join("tools/bin"), &log);
    logging_stub(&base.join("bin/fallow"), "path", &log);
    let prefix = format!("{}:", project.join("tools/bin").display());

    let output = probe_gate_with_path_prefix(
        &rendered_gate_script(),
        &project.join(".claude/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        &session_input(&session),
        &prefix,
    );
    let calls = calls(&log);

    assert!(!calls.contains("stub-"), "{calls}");
    assert!(
        calls.contains("path --version"),
        "{calls}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A git write can target the tree that holds the only trusted fallow, for
/// example `git -C /opt/homebrew commit`. The audit of that tree cannot use
/// the fallow below it, so the gate blocks and names the target.
#[test]
fn gate_blocks_a_write_to_the_tree_of_the_only_trusted_fallow() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    let tools = base.join("tools");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(base.join("bin")).unwrap();
    let log = base.join("calls.log");
    logging_stub(&tools.join("bin/fallow"), "tools", &log);
    git_init(&session);
    git_init(&tools);
    let input = serde_json::json!({
        "cwd": session,
        "tool_input": { "command": format!("git -C {} commit -m x", tools.display()) },
    })
    .to_string();

    let output = probe_gate_with_path_prefix(
        &render_gate_script(true, None),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        input.as_bytes(),
        &format!("{}:", tools.join("bin").display()),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains(&tools.display().to_string()), "{stderr}");
    assert!(!calls(&log).contains("tools audit"), "{}", calls(&log));
}

/// A git write with two targets audits both trees. A helper below the tree of
/// one target never runs, also not in the audit of the other target.
#[test]
fn gate_runs_no_helper_from_the_tree_of_another_audit_root() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    let first = base.join("first");
    let second = base.join("second");
    let log = base.join("calls.log");
    for dir in [&session, &first, &second] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for helper in ["jq", "cat", "sed", "tr", "mktemp", "head", "sort", "grep"] {
        logging_stub(
            &second.join("bin").join(helper),
            &format!("stub-{helper}"),
            &log,
        );
    }
    logging_stub(&base.join("bin/fallow"), "path", &log);
    for dir in [&session, &first, &second] {
        git_init(dir);
    }
    let command = format!(
        "git -C {} commit -m x; git -C {} commit -m x",
        first.display(),
        second.display()
    );
    let input = serde_json::json!({ "cwd": session, "tool_input": { "command": command } });

    let output = probe_gate_with_path_prefix(
        &render_gate_script(true, None),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        input.to_string().as_bytes(),
        &format!("{}:", second.join("bin").display()),
    );
    let calls = calls(&log);

    assert!(!calls.contains("stub-"), "{calls}");
    assert!(
        calls.contains("path --version"),
        "{calls}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Without jq, a large hook input goes to the JSON reader of fallow before the
/// gate knows the targets. When that reader lies in a target tree, the gate
/// blocks before it runs the reader again or starts an audit.
#[test]
fn gate_blocks_when_the_json_reader_lies_in_a_target_tree() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    let tools = base.join("tools");
    let log = base.join("calls.log");
    std::fs::create_dir_all(&session).unwrap();
    let sys = system_bin_without_jq(&base);
    let command = format!(
        "git -C {} commit -m '{}'",
        tools.display(),
        "x".repeat(9000)
    );
    write_stale_fallow(
        &tools.join("bin/fallow"),
        &format!(
            "#!/bin/sh\necho \"tools $*\" >> '{log}'\ncase \"$*\" in\n  'hooks gate-runtime probe') echo fallow-gate-json-v1 ;;\n  'hooks gate-runtime input') printf '%s\\0%s\\0' \"{command}\" '{session}' ;;\n  --version) echo 'fallow 2.30.0' ;;\nesac\n",
            log = log.display(),
            session = session.display(),
        ),
    );
    logging_stub(&base.join("bin/fallow"), "path", &log);
    git_init(&session);
    git_init(&tools);
    let input = serde_json::json!({ "cwd": session, "tool_input": { "command": command } });

    let output = probe_gate_with_path(
        &render_gate_script(true, tools.join("bin/fallow").to_str()),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &format!("{}:{}", base.join("bin").display(), sys.display()),
        input.to_string().as_bytes(),
        &[],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let calls = calls(&log);

    assert_eq!(output.status.code(), Some(2), "{stderr}\n{calls}");
    assert!(stderr.contains(&tools.display().to_string()), "{stderr}");
    assert!(
        calls.contains("tools hooks gate-runtime input"),
        "{calls}\n{stderr}"
    );
    for line in calls.lines().filter(|line| line.starts_with("tools")) {
        assert!(
            line.ends_with("probe") || line.ends_with("input"),
            "{line}\n{calls}"
        );
    }
}

/// A version-manager shim runs the fallow that the project selects. The gate
/// skips a recorded fallow or a fallow on PATH in a shim directory (in any
/// letter case) or one that links to `volta-shim`, and runs the next one.
#[test]
fn gate_skips_version_manager_shims() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    std::fs::create_dir_all(&session).unwrap();
    let log = base.join("calls.log");
    logging_stub(&base.join(".asdf/shims/fallow"), "asdf", &log);
    logging_stub(&base.join(".volta/bin/fallow"), "volta", &log);
    logging_stub(&base.join("x/MISE/Shims/fallow"), "mise", &log);
    logging_stub(&base.join("volta/volta-shim"), "vshim", &log);
    std::fs::create_dir_all(base.join("links")).unwrap();
    std::os::unix::fs::symlink(base.join("volta/volta-shim"), base.join("links/fallow")).unwrap();
    logging_stub(&base.join("bin/fallow"), "path", &log);
    let prefix = format!(
        "{}:{}:{}:",
        base.join(".volta/bin").display(),
        base.join("x/MISE/Shims").display(),
        base.join("links").display()
    );

    let output = probe_gate_with_path_prefix(
        &render_gate_script(true, base.join(".asdf/shims/fallow").to_str()),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        &session_input(&session),
        &prefix,
    );
    let calls = calls(&log);

    assert!(
        calls.contains("path --version"),
        "{calls}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for name in ["asdf", "volta", "mise", "vshim"] {
        assert!(!calls.contains(name), "{name}: {calls}");
    }
}

/// The wider shim forms: any directory named `shims` (as for
/// `ASDF_DATA_DIR` and `MISE_DATA_DIR`), `volta/bin` without the dot, and
/// the `bin` directory of `VOLTA_HOME`.
#[test]
fn gate_skips_each_shim_directory_form() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    std::fs::create_dir_all(&session).unwrap();
    let log = base.join("calls.log");
    let shims = [
        ("asdf-data", base.join("asdf-data/shims")),
        ("mise-data", base.join("mise-data/Shims")),
        ("windows-volta", base.join("AppData/Volta/bin")),
        ("volta-home", base.join("vh/bin")),
    ];
    for (name, dir) in &shims {
        logging_stub(&dir.join("fallow"), name, &log);
    }
    logging_stub(&base.join("bin/fallow"), "path", &log);
    let mut dirs: Vec<PathBuf> = shims.iter().map(|(_, dir)| dir.clone()).collect();
    dirs.extend([base.join("bin"), "/usr/bin".into(), "/bin".into()]);
    let path = std::env::join_paths(dirs).unwrap();
    let path = path.to_str().unwrap();

    let output = probe_gate_with_path(
        &render_gate_script(true, None),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        path,
        &session_input(&session),
        &[
            ("VOLTA_HOME", &base.join("vh")),
            ("ASDF_DATA_DIR", &base.join("asdf-data")),
            ("MISE_DATA_DIR", &base.join("mise-data")),
        ],
    );
    let calls = calls(&log);

    assert!(
        calls.contains("path --version"),
        "{calls}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (name, _) in &shims {
        assert!(!calls.contains(name), "{name}: {calls}");
    }
}

/// Links the system tools that the gate needs, but not jq, into `base/sys`.
fn system_bin_without_jq(base: &Path) -> PathBuf {
    let sys = base.join("sys");
    std::fs::create_dir_all(&sys).unwrap();
    for tool in [
        "bash", "sh", "git", "cat", "mktemp", "sort", "head", "sed", "tr", "dirname", "basename",
        "grep", "rm", "mkdir", "mv", "readlink", "env",
    ] {
        if let Some(found) = ["/usr/bin", "/bin"]
            .iter()
            .map(|dir| Path::new(dir).join(tool))
            .find(|candidate| candidate.exists())
        {
            std::os::unix::fs::symlink(found, sys.join(tool)).unwrap();
        }
    }
    sys
}

/// Writes a stub fallow at `path` that logs each call as `name`, answers
/// the JSON reader probe, prints `command` and `cwd` for the input request,
/// and reports a version below the floor.
fn reader_stub(path: &Path, name: &str, log: &Path, command: &str, cwd: &Path) {
    write_stale_fallow(
        path,
        &format!(
            "#!/bin/sh\necho \"{name} $*\" >> '{log}'\ncase \"$*\" in\n  'hooks gate-runtime probe') echo fallow-gate-json-v1 ;;\n  'hooks gate-runtime input') printf '%s\\0%s\\0' \"{command}\" '{cwd}' ;;\n  --version) echo 'fallow 2.30.0' ;;\nesac\n",
            log = log.display(),
            cwd = cwd.display(),
        ),
    );
}

/// The small reader decodes `\u` escapes, also a surrogate pair, so such a
/// hook input needs no external reader for the input.
#[test]
fn gate_decodes_unicode_escapes_without_a_json_reader() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    std::fs::create_dir_all(&session).unwrap();
    let sys = system_bin_without_jq(&base);
    let log = base.join("calls.log");
    let cwd = session.display().to_string().replace('/', "\\u002f");
    let input = format!(
        r#"{{"cwd":"{cwd}","tool_input":{{"command":"git commit -m \"caf\u00e9 \ud83d\ude00 \u4e2d\""}}}}"#
    );
    reader_stub(&base.join("bin/fallow"), "path", &log, "ls", &session);

    let output = probe_gate_with_path(
        &render_gate_script(true, None),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &format!("{}:{}", base.join("bin").display(), sys.display()),
        input.as_bytes(),
        &[],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let calls = calls(&log);

    assert!(!calls.contains("gate-runtime input"), "{calls}");
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("is fallow 2.30.0, below required"),
        "{stderr}"
    );
}

/// For a hook input that the small reader cannot decode, the gate tries the
/// fallow that the install recorded before jq.
#[test]
fn gate_reads_a_large_input_with_the_recorded_fallow_before_jq() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    std::fs::create_dir_all(&session).unwrap();
    let log = base.join("calls.log");
    let command = format!("git commit -m '{}'", "x".repeat(9000));
    let recorded = base.join("tools/fallow");
    reader_stub(&recorded, "recorded", &log, &command, &session);
    logging_stub(&base.join("bin/jq"), "stub-jq", &log);
    let input = serde_json::json!({ "cwd": session, "tool_input": { "command": command } });

    let output = probe_gate_with_path(
        &render_gate_script(true, recorded.to_str()),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &format!("{}:/usr/bin:/bin", base.join("bin").display()),
        input.to_string().as_bytes(),
        &[],
    );
    let calls = calls(&log);

    assert!(
        calls.contains("recorded hooks gate-runtime input"),
        "{calls}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!calls.contains("stub-jq"), "{calls}");
}

/// A fallow reader in the session tree never reads the hook input: the gate
/// drops the session tree from PATH before the reader runs. When the gate
/// cannot read `cwd` before the reader runs, it blocks once the reader turns
/// out to lie in the session tree.
#[test]
fn gate_never_trusts_a_hook_input_reader_of_the_session_tree() {
    for escaped_cwd in [false, true] {
        let tmp = tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let session = base.join("clone");
        std::fs::create_dir_all(&session).unwrap();
        git_init(&session);
        let sys = system_bin_without_jq(&base);
        let process = base.join("process");
        std::fs::create_dir_all(&process).unwrap();
        let log = base.join("calls.log");
        let command = format!("git commit -m '{}'", "x".repeat(9000));
        reader_stub(
            &session.join("bin/fallow"),
            "session",
            &log,
            &command,
            &session,
        );
        let cwd = if escaped_cwd {
            session.display().to_string().replace('/', "\\/")
        } else {
            session.display().to_string()
        };
        let input = format!(r#"{{"cwd":"{cwd}","tool_input":{{"command":"{command}"}}}}"#);

        let output = probe_gate_with_path(
            &render_gate_script(true, None),
            &base.join("home/.claude/hooks/fallow-gate.sh"),
            &process,
            &format!("{}:{}", session.join("bin").display(), sys.display()),
            input.as_bytes(),
            &[],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        let calls = calls(&log);

        if escaped_cwd {
            assert_eq!(output.status.code(), Some(2), "{stderr}\n{calls}");
            assert!(!calls.contains("session --version"), "{calls}");
            assert!(!calls.contains("session audit"), "{calls}");
        } else {
            assert!(!calls.contains("session"), "{calls}\n{stderr}");
        }
    }
}

/// yarn and npx never read the hook input. When a hook input needs an
/// external reader and the gate trusts none, a possible git write blocks and
/// any other command passes.
#[test]
fn gate_blocks_a_possible_write_when_no_trusted_reader_can_read_the_input() {
    for (command, code) in [("git commit -m x", 2), ("ls", 0)] {
        let tmp = tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let project = base.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("package.json"), "{}").unwrap();
        let sys = system_bin_without_jq(&base);
        let log = base.join("calls.log");
        logging_stub(&base.join("bin/yarn"), "stub-yarn", &log);
        logging_stub(&base.join("bin/npx"), "stub-npx", &log);
        let input = serde_json::json!({
            "cwd": project,
            "tool_input": { "command": format!("{command} # {}", "x".repeat(9000)) },
        });

        let output = probe_gate_with_path(
            &rendered_gate_script(),
            &project.join(".claude/hooks/fallow-gate.sh"),
            &project,
            &format!("{}:{}", base.join("bin").display(), sys.display()),
            input.to_string().as_bytes(),
            &[],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert_eq!(output.status.code(), Some(code), "{command}: {stderr}");
        assert!(!calls(&log).contains("stub-"), "{}", calls(&log));
    }
}

/// A nested `cwd` key can come before the `cwd` of the session. The gate
/// removes the tree of every `cwd` value from PATH before a reader runs.
#[test]
fn gate_excludes_every_cwd_tree_before_the_input_reader_runs() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    let process = base.join("process");
    let other = base.join("other");
    for dir in [&session, &process, &other] {
        std::fs::create_dir_all(dir).unwrap();
    }
    git_init(&session);
    let sys = system_bin_without_jq(&base);
    let log = base.join("calls.log");
    let command = format!("git commit -m '{}'", "x".repeat(9000));
    reader_stub(
        &session.join("bin/fallow"),
        "session",
        &log,
        &command,
        &session,
    );
    let input = format!(
        r#"{{"tool_input":{{"cwd":"{}","command":"{command}"}},"cwd":"{}"}}"#,
        other.display(),
        session.display()
    );

    let output = probe_gate_with_path(
        &render_gate_script(true, None),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &process,
        &format!("{}:{}", session.join("bin").display(), sys.display()),
        input.as_bytes(),
        &[],
    );

    assert!(
        !calls(&log).contains("session"),
        "{}\n{}",
        calls(&log),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A dotfiles repository at HOME is a target of `git -C ~ commit`. That
/// target excludes only HOME itself from PATH, so the user tools below HOME
/// stay in use.
#[test]
fn gate_keeps_the_user_tools_for_a_write_to_a_dotfiles_repository_at_home() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let home = base.join("home");
    let session = base.join("clone");
    std::fs::create_dir_all(&session).unwrap();
    let log = base.join("calls.log");
    logging_stub(&home.join(".local/bin/fallow"), "path", &log);
    git_init(&home);
    git_init(&session);
    let input = serde_json::json!({
        "cwd": session,
        "tool_input": { "command": format!("git -C {} commit -m x", home.display()) },
    });

    let output = probe_gate_with_path(
        &render_gate_script(true, None),
        &home.join(".claude/hooks/fallow-gate.sh"),
        &session,
        &format!("{}:/usr/bin:/bin", home.join(".local/bin").display()),
        input.to_string().as_bytes(),
        &[("HOME", &home)],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!stderr.contains("git write targets"), "{stderr}");
    assert!(
        calls(&log).contains("path --version"),
        "{}\n{stderr}",
        calls(&log)
    );
}

/// When the fallow that the install recorded lies outside every target tree,
/// the loss of the fallow on PATH does not block: the gate runs the recorded
/// fallow.
#[test]
fn gate_runs_the_recorded_fallow_when_a_target_holds_the_path_fallow() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    let target = base.join("target");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(base.join("bin")).unwrap();
    let log = base.join("calls.log");
    logging_stub(&target.join("bin/fallow"), "target", &log);
    let recorded = base.join("tools/fallow");
    logging_stub(&recorded, "recorded", &log);
    git_init(&session);
    git_init(&target);
    let input = serde_json::json!({
        "cwd": session,
        "tool_input": { "command": format!("git -C {} commit -m x", target.display()) },
    });

    let output = probe_gate_with_path_prefix(
        &render_gate_script(true, recorded.to_str()),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        input.to_string().as_bytes(),
        &format!("{}:", target.join("bin").display()),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let calls = calls(&log);

    assert!(calls.contains("recorded --version"), "{calls}\n{stderr}");
    assert!(!calls.contains("target"), "{calls}");
}

/// A link chain counts as a shim when one hop lies in a shim directory, or
/// when its last file is named `mise`, `asdf` or `volta-shim`. A mise shim
/// links to the `mise` program.
#[test]
fn gate_skips_a_link_chain_through_a_shim() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let session = base.join("clone");
    std::fs::create_dir_all(&session).unwrap();
    let log = base.join("calls.log");
    logging_stub(&base.join("opt/real-fallow"), "hop", &log);
    std::fs::create_dir_all(base.join(".asdf/shims")).unwrap();
    std::os::unix::fs::symlink(
        base.join("opt/real-fallow"),
        base.join(".asdf/shims/fallow"),
    )
    .unwrap();
    logging_stub(&base.join("opt/mise"), "mise", &log);
    for (dir, target) in [
        ("first", base.join(".asdf/shims/fallow")),
        ("second", base.join("opt/mise")),
    ] {
        std::fs::create_dir_all(base.join(dir)).unwrap();
        std::os::unix::fs::symlink(target, base.join(dir).join("fallow")).unwrap();
    }
    logging_stub(&base.join("bin/fallow"), "path", &log);
    let prefix = format!(
        "{}:{}:",
        base.join("first").display(),
        base.join("second").display()
    );

    let output = probe_gate_with_path_prefix(
        &render_gate_script(true, None),
        &base.join("home/.claude/hooks/fallow-gate.sh"),
        &session,
        &base.join("bin"),
        &session_input(&session),
        &prefix,
    );
    let calls = calls(&log);

    assert!(
        calls.contains("path --version"),
        "{calls}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!calls.contains("hop") && !calls.contains("mise"), "{calls}");
}
