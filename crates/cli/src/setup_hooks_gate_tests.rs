//! Tests for the audit root of the generated gate script.
//!
//! The hook process can start in a directory that is not the session
//! directory. The gate reads the session directory from the `cwd` field of
//! the hook input and runs the audit from the audit root of that directory.
//! Each test runs the rendered script in bash with a stub `fallow` on `PATH`.
//! The stub writes the directory of the `audit` call to a log file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::{TempDir, tempdir};

use super::rendered_gate_script;

const STUB_FALLOW: &str = "#!/bin/sh\n\
if [ \"$1\" = \"--version\" ]; then echo 'fallow 9.0.0'; exit 0; fi\n\
pwd -P > \"$FALLOW_STUB_LOG\"\n\
echo '{\"verdict\":\"pass\"}'\n";

/// Environment variables that point git at a different repository. A git
/// hook that runs the test suite sets some of them.
const GIT_LOCATION_VARS: [&str; 4] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
];

fn tool_missing(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_err()
}

fn skip_without_tools() -> bool {
    for tool in ["jq", "git"] {
        if tool_missing(tool) {
            eprintln!("skipping: {tool} not on PATH");
            return true;
        }
    }
    false
}

/// A canonical temporary root with a space in its name, so each test also
/// covers paths with spaces.
fn spaced_root() -> (TempDir, PathBuf) {
    let tmp = tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join("work space");
    std::fs::create_dir_all(&root).unwrap();
    (tmp, root)
}

fn install_gate(install_root: &Path, harness: &str) -> PathBuf {
    let hooks = install_root.join(harness).join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let script = hooks.join("fallow-gate.sh");
    std::fs::write(&script, rendered_gate_script()).unwrap();
    script
}

fn git(dir: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    for var in GIT_LOCATION_VARS {
        cmd.env_remove(var);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Creates a git repository with one commit, so `git worktree add` works.
fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q"]);
    git(
        dir,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
}

struct GateRun {
    output: Output,
    /// The directory of the `audit` call, or `None` when the audit did not run.
    audit_dir: Option<PathBuf>,
}

/// Runs `script` in bash from `process_dir`, the way a handler starts it.
fn run_gate(
    script: &Path,
    process_dir: &Path,
    home: &Path,
    payload: &serde_json::Value,
) -> GateRun {
    run_gate_with_debug(script, process_dir, home, payload, false)
}

/// Same as `run_gate`. With `debug`, the run sets `FALLOW_GATE_DEBUG=1`.
fn run_gate_with_debug(
    script: &Path,
    process_dir: &Path,
    home: &Path,
    payload: &serde_json::Value,
    debug: bool,
) -> GateRun {
    use std::os::unix::fs::PermissionsExt;

    let stub = tempdir().unwrap();
    let bin = stub.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fallow = bin.join("fallow");
    std::fs::write(&fallow, STUB_FALLOW).unwrap();
    std::fs::set_permissions(&fallow, std::fs::Permissions::from_mode(0o755)).unwrap();
    let log = stub.path().join("audit-dir.log");

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut cmd = Command::new("bash");
    cmd.arg(script)
        .current_dir(process_dir)
        .env("PATH", path)
        .env("HOME", home)
        .env("FALLOW_STUB_LOG", &log)
        .env("FALLOW_GATE_MIN_VERSION", "")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("FALLOW_GATE_DEBUG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for var in GIT_LOCATION_VARS {
        cmd.env_remove(var);
    }
    if debug {
        cmd.env("FALLOW_GATE_DEBUG", "1");
    }
    let mut child = cmd.spawn().expect("spawn bash");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    let audit_dir = std::fs::read_to_string(&log)
        .ok()
        .map(|text| PathBuf::from(text.trim_end()));
    GateRun { output, audit_dir }
}

fn commit_payload(cwd: &Path) -> serde_json::Value {
    serde_json::json!({
        "cwd": cwd,
        "tool_input": { "command": "git commit -m test" },
    })
}

fn assert_audited_in(run: &GateRun, expected: &Path, case: &str) {
    assert_eq!(
        run.output.status.code(),
        Some(0),
        "{case}: the gate must pass on a pass verdict; stderr={}",
        String::from_utf8_lossy(&run.output.stderr)
    );
    assert_eq!(
        run.audit_dir.as_deref(),
        Some(expected),
        "{case}: wrong audit root; stderr={}",
        String::from_utf8_lossy(&run.output.stderr)
    );
}

/// The hook process starts in the main checkout, but the session works in a
/// nested worktree. The audit must run in the worktree, both when the worktree
/// holds its own gate and when only the main checkout holds one.
#[test]
fn gate_audits_the_nested_worktree_of_the_session() {
    if skip_without_tools() {
        return;
    }
    for harness in [".claude", ".codex"] {
        for worktree_has_gate in [true, false] {
            let (_tmp, root) = spaced_root();
            let home = root.join("home");
            let repo = root.join("repo");
            init_repo(&repo);
            let main_gate = install_gate(&repo, harness);
            let worktree = repo.join(".claude/worktrees/feature");
            git(
                &repo,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "--detach",
                    worktree.to_str().unwrap(),
                ],
            );
            if worktree_has_gate {
                install_gate(&worktree, harness);
            }
            let session = worktree.join("src");
            std::fs::create_dir_all(&session).unwrap();

            let run = run_gate(&main_gate, &repo, &home, &commit_payload(&session));
            assert_audited_in(
                &run,
                &worktree.canonicalize().unwrap(),
                &format!("{harness}, worktree gate {worktree_has_gate}"),
            );
        }
    }
}

/// A session in a subdirectory audits the install root, whatever the
/// directory of the hook process is.
#[test]
fn gate_audits_the_install_root_from_a_session_subdirectory() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let repo = root.join("repo");
    init_repo(&repo);
    let gate = install_gate(&repo, ".claude");
    let session = repo.join("src/deep");
    std::fs::create_dir_all(&session).unwrap();
    let elsewhere = root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();

    for process_dir in [&repo, &session, &elsewhere] {
        let run = run_gate(
            &gate,
            process_dir,
            &root.join("home"),
            &commit_payload(&session),
        );
        assert_audited_in(
            &run,
            &repo,
            &format!("process in {}", process_dir.display()),
        );
    }
}

/// `fallow hooks install --root packages/app` puts the install root below the
/// git root. The walk finds the gate there before it reaches the git root.
#[test]
fn gate_audits_an_install_root_below_the_git_root() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let repo = root.join("repo");
    init_repo(&repo);
    let app = repo.join("packages/app");
    let gate = install_gate(&app, ".claude");
    let session = app.join("src");
    std::fs::create_dir_all(&session).unwrap();

    let run = run_gate(&gate, &repo, &root.join("home"), &commit_payload(&session));
    assert_audited_in(&run, &app, "install root below the git root");
}

/// Without a usable `cwd`, the audit runs in the directory of the hook
/// process, as before.
#[test]
fn gate_keeps_the_process_directory_without_a_usable_cwd() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let repo = root.join("repo");
    init_repo(&repo);
    let gate = install_gate(&repo, ".claude");
    let process_dir = repo.join("src");
    std::fs::create_dir_all(&process_dir).unwrap();
    let a_file = repo.join("README.md");
    std::fs::write(&a_file, "readme\n").unwrap();
    let command = serde_json::json!({ "command": "git commit -m test" });

    let payloads = [
        ("no cwd", serde_json::json!({ "tool_input": command })),
        (
            "null cwd",
            serde_json::json!({ "cwd": null, "tool_input": command }),
        ),
        (
            "empty cwd",
            serde_json::json!({ "cwd": "", "tool_input": command }),
        ),
        (
            "missing directory",
            serde_json::json!({ "cwd": root.join("gone"), "tool_input": command }),
        ),
        (
            "a file",
            serde_json::json!({ "cwd": a_file, "tool_input": command }),
        ),
    ];
    for (case, payload) in payloads {
        let run = run_gate(&gate, &process_dir, &root.join("home"), &payload);
        assert_audited_in(&run, &process_dir, case);
    }
}

/// A command that is not a git commit or push exits before the audit root
/// work and before any fallow call. The run sets `FALLOW_GATE_DEBUG`, so
/// audit root work before the skip would write its own debug line.
#[test]
fn gate_skips_other_commands_before_the_audit_root_work() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let repo = root.join("repo");
    init_repo(&repo);
    let gate = install_gate(&repo, ".claude");
    let session = repo.join("src");
    std::fs::create_dir_all(&session).unwrap();
    let payload = serde_json::json!({
        "cwd": session,
        "tool_input": { "command": "git status" },
    });

    let run = run_gate_with_debug(&gate, &repo, &root.join("home"), &payload, true);
    assert_eq!(run.output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&run.output.stderr),
        "fallow-gate: not a git commit/push, skipping audit.\n",
        "a skipped command must write only the skip line"
    );
    assert_eq!(run.audit_dir, None, "a skipped command must not audit");

    let commit = serde_json::json!({
        "cwd": session,
        "tool_input": { "command": "git commit -m test" },
    });
    let run = run_gate_with_debug(&gate, &repo, &root.join("home"), &commit, true);
    assert_audited_in(&run, &repo, "commit with debug");
    assert!(
        String::from_utf8_lossy(&run.output.stderr).contains(&format!(
            "fallow-gate: auditing {} (session directory {}).",
            repo.display(),
            session.display()
        )),
        "a commit with debug must log the audit root: {}",
        String::from_utf8_lossy(&run.output.stderr)
    );
}

/// The Claude project handler starts the gate with a relative path from the
/// install root (`exec ./.claude/hooks/fallow-gate.sh`). The gate must still
/// find its harness location and audit the nested worktree of the session.
#[test]
fn gate_started_with_a_relative_path_audits_the_session_worktree() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let repo = root.join("repo");
    init_repo(&repo);
    install_gate(&repo, ".claude");
    let worktree = repo.join(".claude/worktrees/feature");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );
    let session = worktree.join("src");
    std::fs::create_dir_all(&session).unwrap();

    let run = run_gate(
        Path::new("./.claude/hooks/fallow-gate.sh"),
        &repo,
        &root.join("home"),
        &commit_payload(&session),
    );
    assert_audited_in(&run, &worktree, "relative script path");
}

/// The walk uses physical paths. A session path through a symlink must not
/// climb the parents of the link into an unrelated project.
#[test]
fn gate_walks_the_physical_parents_of_a_symlinked_session() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let other = root.join("other");
    let gate = install_gate(&other, ".claude");
    let real = root.join("real");
    init_repo(&real);
    let deep = real.join("src/deep");
    std::fs::create_dir_all(&deep).unwrap();
    std::os::unix::fs::symlink(real.join("src"), other.join("link")).unwrap();

    let run = run_gate(
        &gate,
        &real,
        &root.join("home"),
        &commit_payload(&other.join("link/deep")),
    );
    assert_audited_in(&run, &real, "session through a symlink");
}

/// The user-scope gate with the session and the process in a package
/// directory audits that package directory, as before the `cwd` support.
/// With the process outside the work tree of the session (the nested
/// worktree case), it audits the git top level of the session.
#[test]
fn user_scope_gate_keeps_a_process_directory_inside_the_session_work_tree() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let home = root.join("home");
    let gate = install_gate(&home, ".claude");
    let repo = home.join("code/mono");
    init_repo(&repo);
    let app = repo.join("packages/app");
    std::fs::create_dir_all(&app).unwrap();

    let run = run_gate(&gate, &app, &home, &commit_payload(&app));
    assert_audited_in(&run, &app, "process in the package directory");

    let worktree = repo.join(".claude/worktrees/feature");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );
    let session = worktree.join("src");
    std::fs::create_dir_all(&session).unwrap();
    let run = run_gate(&gate, &repo, &home, &commit_payload(&session));
    assert_audited_in(&run, &worktree, "process in the main checkout");

    // The nested worktree is a path under the main checkout, but it is
    // another work tree, so the process directory does not count.
    let main_src = repo.join("src");
    std::fs::create_dir_all(&main_src).unwrap();
    let run = run_gate(&gate, &worktree, &home, &commit_payload(&main_src));
    assert_audited_in(&run, &repo, "process in the nested worktree");
}

/// `$HOME` is never the audit root: not with a trailing slash in `HOME`,
/// and not when `$HOME` is itself a git work tree.
#[test]
fn user_scope_gate_never_audits_home() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let home = root.join("home");
    let gate = install_gate(&home, ".claude");
    let project = home.join("proj");
    std::fs::create_dir_all(&project).unwrap();
    let home_with_slash = PathBuf::from(format!("{}/", home.display()));

    let run = run_gate(&gate, &home, &home_with_slash, &commit_payload(&project));
    assert_audited_in(&run, &project, "HOME with a trailing slash");

    init_repo(&home);
    let run = run_gate(&gate, &home, &home, &commit_payload(&project));
    assert_audited_in(&run, &project, "HOME is a git work tree");
}

/// The user-scope gate lives in `$HOME`, so the walk finds no project gate.
/// The audit root is the git top level of the session directory, and outside
/// a git repository it is the session directory. It is never `$HOME`.
#[test]
fn user_scope_gate_audits_the_git_top_level_of_the_session() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let home = root.join("home");
    let gate = install_gate(&home, ".claude");
    let repo = home.join("code/repo");
    init_repo(&repo);
    let in_repo = repo.join("src");
    std::fs::create_dir_all(&in_repo).unwrap();
    let not_a_repo = home.join("notes");
    std::fs::create_dir_all(&not_a_repo).unwrap();

    let run = run_gate(&gate, &home, &home, &commit_payload(&in_repo));
    assert_audited_in(&run, &repo, "session in a git repository");

    let run = run_gate(&gate, &home, &home, &commit_payload(&not_a_repo));
    assert_audited_in(&run, &not_a_repo, "session outside a git repository");
}
