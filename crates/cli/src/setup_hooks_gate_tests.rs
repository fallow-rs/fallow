//! Tests for the audit root of the generated gate script.
//!
//! The hook process can start in a directory that is not the session
//! directory. The gate reads the session directory from the `cwd` field of
//! the hook input and runs the audit from the audit root of that directory.
//! Each test runs the rendered script in bash with a stub `fallow` on `PATH`.
//! The stub adds the directory of each `audit` call to a log file. It returns
//! a fail verdict in a directory that holds `STUB_FAIL_MARKER`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::{TempDir, tempdir};

use super::rendered_gate_script;

const STUB_FALLOW: &str = "#!/bin/sh\n\
if [ \"$1\" = \"--version\" ]; then echo 'fallow 9.0.0'; exit 0; fi\n\
pwd -P >> \"$FALLOW_STUB_LOG\"\n\
if [ -f .stub-error ]; then echo '{\"error\":true,\"message\":\"stub\"}'; exit 2; fi\n\
if [ -f .stub-fail ]; then echo '{\"verdict\":\"fail\"}'; else echo '{\"verdict\":\"pass\"}'; fi\n";

/// A file that makes the stub return a fail verdict in its directory.
const STUB_FAIL_MARKER: &str = ".stub-fail";

/// A file that makes the stub report a runtime error in its directory.
const STUB_ERROR_MARKER: &str = ".stub-error";

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
    /// The directory of each `audit` call, in call order.
    audit_dirs: Vec<PathBuf>,
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
    let audit_dirs = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(PathBuf::from)
        .collect();
    GateRun { output, audit_dirs }
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
        run.audit_dirs,
        [expected],
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
    assert!(
        run.audit_dirs.is_empty(),
        "a skipped command must not audit"
    );

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

fn command_payload(cwd: &Path, command: &str) -> serde_json::Value {
    serde_json::json!({
        "cwd": cwd,
        "tool_input": { "command": command },
    })
}

/// A main checkout with the gate and two linked worktrees beside it. The
/// session works in worktree `a`; the hook process starts in the main
/// checkout. Worktree `b` has a space in its name. Worktree `c` is a third,
/// clean tree for the redirection cases.
struct TwoWorktrees {
    _tmp: TempDir,
    root: PathBuf,
    gate: PathBuf,
    main: PathBuf,
    a: PathBuf,
    b: PathBuf,
    c: PathBuf,
}

fn two_worktrees() -> TwoWorktrees {
    let (tmp, root) = spaced_root();
    let main = root.join("main");
    init_repo(&main);
    let gate = install_gate(&main, ".claude");
    let a = root.join("wt-a");
    let b = root.join("wt b");
    let c = root.join("wt-c");
    for worktree in [&a, &b, &c] {
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                worktree.to_str().unwrap(),
            ],
        );
    }
    TwoWorktrees {
        _tmp: tmp,
        root,
        gate,
        main,
        a,
        b,
        c,
    }
}

/// Runs each `(case, command, expected audit roots)` from a session in
/// worktree `a/src` and checks the audit roots in order.
fn assert_cases(trees: &TwoWorktrees, cases: &[(&str, String, Vec<&Path>)]) {
    let session = trees.a.join("src");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(trees.c.join("src")).unwrap();
    for (case, command, expected) in cases {
        let run = run_gate(
            &trees.gate,
            &trees.main,
            &trees.root.join("home"),
            &command_payload(&session, command),
        );
        assert_eq!(
            run.output.status.code(),
            Some(0),
            "{case}: the gate must pass on pass verdicts; stderr={}",
            String::from_utf8_lossy(&run.output.stderr)
        );
        assert_eq!(
            &run.audit_dirs,
            expected,
            "{case}: wrong audit roots for {command:?}; stderr={}",
            String::from_utf8_lossy(&run.output.stderr)
        );
    }
}

/// A git write in the strict grammar (`[cd <dir> &&] git [-C <dir>]...
/// commit|push ...`) audits only the tree that it targets. A session in
/// worktree `a` that commits in worktree `b` must not be blocked by findings
/// in `a`.
#[test]
fn gate_audits_only_the_target_of_a_strict_git_write() {
    if skip_without_tools() {
        return;
    }
    let trees = two_worktrees();
    let (a, b, c) = (trees.a.as_path(), trees.b.as_path(), trees.c.as_path());
    let bq = trees.b.display().to_string();
    let cq = trees.c.display().to_string();
    let cases: Vec<(&str, String, Vec<&Path>)> = vec![
        ("plain commit", "git commit -m x".to_owned(), vec![a]),
        (
            "-C with a double-quoted path",
            format!("git -C \"{bq}\" commit -m x"),
            vec![b],
        ),
        (
            "-C with a single-quoted path",
            format!("git -C '{bq}' commit -m x"),
            vec![b],
        ),
        (
            "relative -C",
            "git -C '../../wt b' commit -m 'a b'".to_owned(),
            vec![b],
        ),
        (
            "cumulative -C",
            format!("git -C \"{cq}\" -C '../wt b' commit"),
            vec![b],
        ),
        (
            "cd and &&",
            format!("cd \"{bq}\" && git commit -m \"a b\""),
            vec![b],
        ),
        (
            "relative cd",
            "cd ../../wt-c && git commit -m x".to_owned(),
            vec![c],
        ),
        (
            "message on two lines",
            "git -C ../../wt-c commit -m \"line one\nline two\"".to_owned(),
            vec![c],
        ),
    ];
    assert_cases(&trees, &cases);
}

/// Linked worktrees share refs. A strict push from another linked worktree
/// of the session repository can send the branch or tags of the session, so
/// it keeps the session audit and adds the target.
#[test]
fn gate_keeps_the_session_audit_for_a_push_from_a_linked_worktree() {
    if skip_without_tools() {
        return;
    }
    let trees = two_worktrees();
    let (a, b, c) = (trees.a.as_path(), trees.b.as_path(), trees.c.as_path());
    let main = trees.main.as_path();
    let cases: Vec<(&str, String, Vec<&Path>)> = vec![
        (
            "push from the main checkout",
            "git -C ../../main push origin feat-a".to_owned(),
            vec![a, main],
        ),
        (
            "push from a sibling worktree",
            "git -C '../../wt b' push origin feat-a".to_owned(),
            vec![a, b],
        ),
        (
            "push tags",
            "git -C ../../wt-c push --tags".to_owned(),
            vec![a, c],
        ),
        (
            "cd and push",
            "cd ../../wt-c && git push origin main".to_owned(),
            vec![a, c],
        ),
    ];
    assert_cases(&trees, &cases);

    // A clone has its own refs: a push from it audits only the clone.
    let clone = trees.root.join("clone");
    git(
        &trees.root,
        &[
            "clone",
            "-q",
            trees.main.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    let cases: Vec<(&str, String, Vec<&Path>)> = vec![(
        "push from a clone",
        "git -C ../../clone push origin main".to_owned(),
        vec![clone.as_path()],
    )];
    assert_cases(&trees, &cases);
}

/// Every other git write audits the session tree, plus each candidate
/// directory that the scan finds and that exists. A command that the scan
/// reads wrong can only add audits, never move the audit to a clean tree.
#[test]
fn gate_audits_the_session_tree_for_other_git_writes() {
    if skip_without_tools() {
        return;
    }
    let trees = two_worktrees();
    let (a, c) = (trees.a.as_path(), trees.c.as_path());
    let bq = trees.b.display().to_string();
    let gone = trees.root.join("gone").display().to_string();
    let a_file = trees.a.join("README.md");
    std::fs::write(&a_file, "readme\n").unwrap();
    let cases: Vec<(&str, String, Vec<&Path>)> = vec![
        (
            "missing target",
            format!("git -C \"{gone}\" commit -m x"),
            vec![a],
        ),
        (
            "file target",
            format!("git -C \"{}\" commit -m x", a_file.display()),
            vec![a],
        ),
        (
            "escaped space",
            format!("git -C {} commit -m x", bq.replace(' ', "\\ ")),
            vec![a],
        ),
        ("cd and ;", format!("cd \"{bq}\"; git commit -m x"), vec![a]),
        (
            "--git-dir and --work-tree",
            "git --git-dir=../../wt-c/.git --work-tree=../../wt-c commit".to_owned(),
            vec![a, c],
        ),
        (
            "-C of another git command",
            "git -C ../../wt-c status && git commit".to_owned(),
            vec![a],
        ),
        (
            "cd -",
            "cd ../../wt-c && cd - && git commit".to_owned(),
            vec![a],
        ),
        (
            "cd ..",
            "cd ../../wt-c/src; cd ..; git commit".to_owned(),
            vec![a, c],
        ),
        (
            "pushd and popd",
            "pushd ../../wt-c; popd; git commit".to_owned(),
            vec![a],
        ),
        (
            "cd $OLDPWD",
            "cd \"$OLDPWD\" && git commit".to_owned(),
            vec![a],
        ),
        ("cd ~", "cd ~ && git commit".to_owned(), vec![a]),
        ("bare cd", "cd && git commit".to_owned(), vec![a]),
        (
            "a cd that fails",
            format!("cd \"{gone}\"; git commit"),
            vec![a],
        ),
    ];
    assert_cases(&trees, &cases);
}

/// Shell forms that a simple scan can read wrong still audit the session
/// tree.
#[test]
fn gate_audits_the_session_tree_for_shell_forms() {
    if skip_without_tools() {
        return;
    }
    let trees = two_worktrees();
    let (a, c) = (trees.a.as_path(), trees.c.as_path());
    let cases: Vec<(&str, String, Vec<&Path>)> = vec![
        (
            "ANSI-C quoting",
            "git -C $'../../wt-c' commit".to_owned(),
            vec![a],
        ),
        (
            "escaped quotes",
            "git commit -m \"say \\\"hi\\\"\" && git -C ../../wt-c push".to_owned(),
            vec![a, c],
        ),
        (
            "line continuation",
            "git \\\n  -C ../../wt-c \\\n  commit -m x".to_owned(),
            vec![a],
        ),
        (
            "bash -c",
            "bash -c 'git -C ../../wt-c commit -m x'".to_owned(),
            vec![a],
        ),
        ("sh -c", "sh -c \"git commit -m x\"".to_owned(), vec![a]),
        ("eval", "eval 'git commit -m x'".to_owned(), vec![a]),
        ("env", "env git -C ../../wt-c commit".to_owned(), vec![a, c]),
        ("command", "command git commit".to_owned(), vec![a]),
        (
            "-C with a substitution",
            "git -C \"$(pwd)\" commit".to_owned(),
            vec![a],
        ),
        (
            "cd ||",
            "cd ../../wt-c || git commit".to_owned(),
            vec![a, c],
        ),
        (
            "subshell",
            "( cd ../../wt-c && git commit ); git push".to_owned(),
            vec![a, c],
        ),
        (
            "GIT_DIR prefix",
            "GIT_DIR=../../wt-c/.git git commit".to_owned(),
            vec![a, c],
        ),
        (
            "GIT_WORK_TREE prefix",
            "GIT_WORK_TREE=../../wt-c git commit".to_owned(),
            vec![a, c],
        ),
        (
            "here-document",
            "git commit -F - <<'EOF'\nit's done\nEOF\ngit -C ../../wt-c push".to_owned(),
            vec![a, c],
        ),
        (
            "write in a substitution",
            "echo \"$(git -C ../../wt-c commit -m x)\"".to_owned(),
            vec![a, c],
        ),
        ("backquotes", "echo `git commit`".to_owned(), vec![a]),
        (
            "pipeline",
            "cd ../../wt-c | git commit".to_owned(),
            vec![a, c],
        ),
        ("expanded subcommand", "git $SUB".to_owned(), vec![a]),
        ("comment", "ls # git push".to_owned(), vec![a]),
    ];
    assert_cases(&trees, &cases);
}

/// Two git writes into two trees audit both trees. A fail verdict in the
/// second tree blocks the command, also after a runtime error in the first.
#[test]
fn gate_blocks_when_one_of_several_audits_fails() {
    if skip_without_tools() {
        return;
    }
    let trees = two_worktrees();
    let session = trees.a.join("src");
    std::fs::create_dir_all(&session).unwrap();
    let payload = command_payload(&session, "git -C ../../wt-c commit -m x && git push");
    let home = trees.root.join("home");
    let expected = [trees.a.as_path(), trees.c.as_path()];

    let run = run_gate(&trees.gate, &trees.main, &home, &payload);
    assert_eq!(run.output.status.code(), Some(0));
    assert_eq!(run.audit_dirs, expected);

    std::fs::write(trees.c.join(STUB_FAIL_MARKER), "").unwrap();
    let run = run_gate(&trees.gate, &trees.main, &home, &payload);
    assert_eq!(
        run.output.status.code(),
        Some(2),
        "a fail verdict in the second tree must block; stderr={}",
        String::from_utf8_lossy(&run.output.stderr)
    );
    assert_eq!(run.audit_dirs, expected);

    std::fs::write(trees.a.join(STUB_ERROR_MARKER), "").unwrap();
    let run = run_gate(&trees.gate, &trees.main, &home, &payload);
    assert_eq!(
        run.output.status.code(),
        Some(2),
        "a runtime error in the first tree must not hide the fail; stderr={}",
        String::from_utf8_lossy(&run.output.stderr)
    );
    assert_eq!(run.audit_dirs, expected);
}

/// A strict form with an active word is not strict: an expansion in a
/// double-quoted message, a git config override, or an option that runs a
/// command or reads a file. The gate then audits the session tree too.
#[test]
fn gate_leaves_the_strict_form_for_active_words() {
    if skip_without_tools() {
        return;
    }
    let trees = two_worktrees();
    let (a, c) = (trees.a.as_path(), trees.c.as_path());
    let target = "git -C ../../wt-c";
    let cases: Vec<(&str, String, Vec<&Path>)> = [
        ("substitution in a message", "commit -m \"$(git push)\""),
        ("backquotes in a message", "commit -m \"`git push`\""),
        ("parameter in a message", "commit -m \"$X\""),
        ("substitution in an option", "commit -S\"$(git push)\" -m x"),
        ("commit template", "commit --template=/tmp/t"),
        ("commit message file", "commit -F /tmp/m"),
        ("commit reuse message", "commit -c HEAD"),
        ("push receive-pack", "push --receive-pack=x origin"),
        ("push exec", "push --exec=x origin"),
        ("push option", "push -o ci.skip origin"),
        ("push repo", "push --repo=other"),
        ("quoted option", "push origin '--receive-pack=x'"),
        ("separator", "commit -m x -- file"),
        ("carriage return", "commit -m x\rgit push"),
    ]
    .into_iter()
    .map(|(case, rest)| (case, format!("{target} {rest}"), vec![a, c]))
    .chain([
        (
            "config override",
            "git -c core.hooksPath=/tmp/h -C ../../wt-c commit -m x".to_owned(),
            vec![a, c],
        ),
        ("glob", "git -C ../../wt-* commit -m x".to_owned(), vec![a]),
        ("tilde", "git -C ~ commit -m x".to_owned(), vec![a]),
        (
            "brace",
            "git -C {../../wt-c} commit -m x".to_owned(),
            vec![a],
        ),
        (
            "variable",
            "git -C \"$HOME\" commit -m x".to_owned(),
            vec![a],
        ),
    ])
    .collect();
    assert_cases(&trees, &cases);
}

/// The strict form resolves its target with physical paths and requires a
/// git work tree. Tabs separate words as in bash.
#[test]
fn gate_resolves_the_strict_target_physically() {
    if skip_without_tools() {
        return;
    }
    let trees = two_worktrees();
    let (a, c) = (trees.a.as_path(), trees.c.as_path());
    std::fs::create_dir_all(trees.root.join("plain")).unwrap();
    std::os::unix::fs::symlink(&trees.c, trees.root.join("link-c")).unwrap();
    let cases: Vec<(&str, String, Vec<&Path>)> = vec![
        (
            "symlinked target",
            "git -C ../../link-c commit -m x".to_owned(),
            vec![c],
        ),
        (
            "tabs",
            "git\t-C\t../../wt-c\tcommit\t-m\tx".to_owned(),
            vec![c],
        ),
        (
            "inert options",
            "git -C ../../wt-c commit --amend --no-edit -m 'a' -m \"b\" -S".to_owned(),
            vec![c],
        ),
        (
            "push with names",
            "git -C ../../wt-c push -u --force-with-lease origin HEAD:main".to_owned(),
            vec![a, c],
        ),
        (
            "not a work tree",
            "git -C ../../plain commit -m x".to_owned(),
            vec![a],
        ),
    ];
    assert_cases(&trees, &cases);
}

/// A strict command into another part of the same work tree must not move
/// the audit: `git commit` commits the whole index. Only git decides whether
/// a target is another work tree. Here the install root is below the git
/// root, with a sibling install, and the hook process starts in the install
/// root of the session.
#[test]
fn gate_keeps_the_session_audit_inside_the_same_work_tree() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let repo = root.join("repo");
    init_repo(&repo);
    let app = repo.join("packages/app");
    let gate = install_gate(&app, ".claude");
    install_gate(&repo.join("packages/other"), ".claude");
    std::fs::create_dir_all(repo.join("packages/empty/.git")).unwrap();
    let linked = repo.join("packages/linked");
    std::fs::create_dir_all(&linked).unwrap();
    std::fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", repo.join(".git").display()),
    )
    .unwrap();
    let repo_q = repo.display().to_string();
    let cases = [
        ("sibling package", "git -C ../other commit -m x".to_owned()),
        (
            "cd to the repo root",
            format!("cd \"{repo_q}\" && git commit -m x"),
        ),
        ("-C to the repo root", "git -C ../.. commit -m x".to_owned()),
        (
            "empty .git directory",
            "git -C ../empty commit -m x".to_owned(),
        ),
        (
            "gitfile to the session repo",
            "git -C ../linked commit -m x".to_owned(),
        ),
        ("cd -", "cd - && git commit -m x".to_owned()),
        ("cd +1", "cd +1 && git commit -m x".to_owned()),
        (
            "cd through CDPATH form",
            "cd packages && git commit -m x".to_owned(),
        ),
    ];
    for (case, command) in cases {
        let run = run_gate(
            &gate,
            &app,
            &root.join("home"),
            &command_payload(&app, &command),
        );
        assert_audited_in(&run, &app, case);
    }

    // The same commands without the session audit fail as on main.
    std::fs::write(app.join(STUB_FAIL_MARKER), "").unwrap();
    let run = run_gate(
        &gate,
        &app,
        &root.join("home"),
        &command_payload(&app, "git -C ../other commit -m x"),
    );
    assert_eq!(run.output.status.code(), Some(2));
}

/// `git -C` changes the directory with chdir(), so `link/..` is the parent
/// of the link target, not the directory that holds the link. A `cd` resolves
/// `..` logically, so `cd ./link/..` is not strict: the session tree stays in
/// the audit, and the physical candidate only adds an audit.
#[test]
fn gate_resolves_git_c_physically_through_a_symlink() {
    if skip_without_tools() {
        return;
    }
    let trees = two_worktrees();
    let session = trees.a.join("src");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(trees.c.join("src")).unwrap();
    std::os::unix::fs::symlink(trees.c.join("src"), session.join("link")).unwrap();
    let cases: Vec<(&str, String, Vec<&Path>)> = vec![
        (
            "-C link/..",
            "git -C link/.. commit -m x".to_owned(),
            vec![trees.c.as_path()],
        ),
        (
            "cd link/..",
            "cd ./link/.. && git commit -m x".to_owned(),
            vec![trees.a.as_path(), trees.c.as_path()],
        ),
    ];
    assert_cases(&trees, &cases);
}

/// The gate script on main before the target support, for the differential
/// test.
const MAIN_GATE_SCRIPT: &str = include_str!("setup_hooks/fallow-gate.main.sh");

/// Commands for the differential test, run from a session in `wt-a/src`.
const DIFFERENTIAL_COMMANDS: &[&str] = &[
    "git commit -m x",
    "git -C ../../wt-c commit -m x",
    "git -C '../../wt b' push origin main",
    "git -C ../../main push origin feat-a",
    "git -C '../../wt b' push --tags",
    "cd ../../wt-c && git push",
    "git -C .. commit -m x",
    "git -C ../../main commit -m x",
    "cd ../../main && git commit -m x",
    "git -C ../../wt-c -C ../main commit",
    "git -C link/.. commit -m x",
    "git -C ../../wt-c commit -m \"$(git push)\"",
    "git -C ../../wt-c commit -m \"`git push`\"",
    "git -c core.hooksPath=/tmp/h -C ../../wt-c commit -m x",
    "git -C ../../wt-c commit -F /tmp/m",
    "git -C ../../wt-c push --receive-pack=x origin",
    "git --git-dir=../../wt-c/.git --work-tree=../../wt-c commit",
    "GIT_DIR=../../wt-c/.git git commit",
    "git -C ../../wt-c status && git commit",
    "cd ../../wt-c && cd - && git commit",
    "cd ../../wt-c/src; cd ..; git commit",
    "pushd ../../wt-c; popd; git commit",
    "cd ../../wt-c || git commit",
    "( cd ../../wt-c && git commit ); git push",
    "cd ../../wt-c | git commit",
    "git commit -m x; git -C ../../wt-c push",
    "git commit -F - <<'EOF'\nit's done\nEOF\ngit -C ../../wt-c push",
    "echo \"$(git -C ../../wt-c commit -m x)\"",
    "bash -c 'git -C ../../wt-c commit -m x'",
    "eval 'git commit -m x'",
    "env git -C ../../wt-c commit",
    "git -C $'../../wt-c' commit",
    "git \\\n  -C ../../wt-c \\\n  commit -m x",
    "git -C ../../plain commit -m x",
    "git -C ../../gone commit -m x",
    "cd +1 && git commit",
    "git log --oneline",
    "git status",
];

/// The output of `git rev-parse <flag>` in a directory.
fn git_rev_parse(dir: &Path, flag: &str) -> String {
    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "--path-format=absolute", flag])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    for var in GIT_LOCATION_VARS {
        cmd.env_remove(var);
    }
    let out = cmd.output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Whether `dir` is another work tree than `session` for a write: another
/// top level, and for a push also another common git directory, because
/// linked worktrees share refs.
fn other_tree_for_write(dir: &Path, session: &Path, push: bool) -> bool {
    let other_top =
        git_rev_parse(dir, "--show-toplevel") != git_rev_parse(session, "--show-toplevel");
    let other_common =
        git_rev_parse(dir, "--git-common-dir") != git_rev_parse(session, "--git-common-dir");
    other_top && (!push || other_common)
}

/// Runs main's gate and the new gate at `gate` over the same commands. The
/// new gate must audit every root that main audits, unless it audits only a
/// strict target that git reports in another work tree than the session.
fn assert_never_less_than_main(
    gate: &Path,
    process_dir: &Path,
    session: &Path,
    home: &Path,
    commands: &[&str],
) {
    let new_gate = rendered_gate_script();
    for command in commands {
        let payload = command_payload(session, command);
        std::fs::write(gate, MAIN_GATE_SCRIPT).unwrap();
        let main_run = run_gate(gate, process_dir, home, &payload);
        std::fs::write(gate, &new_gate).unwrap();
        let new_run = run_gate(gate, process_dir, home, &payload);
        let missing: Vec<&PathBuf> = main_run
            .audit_dirs
            .iter()
            .filter(|dir| !new_run.audit_dirs.contains(dir))
            .collect();
        if missing.is_empty() {
            continue;
        }
        let target_only = new_run.audit_dirs.len() == 1
            && other_tree_for_write(&new_run.audit_dirs[0], session, command.contains("push"));
        assert!(
            target_only,
            "{command:?}: the new gate skips {missing:?} that main audits; new={:?}",
            new_run.audit_dirs
        );
    }
}

#[test]
fn gate_never_audits_less_than_main() {
    if skip_without_tools() {
        return;
    }
    let trees = two_worktrees();
    let session = trees.a.join("src");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(trees.c.join("src")).unwrap();
    std::fs::create_dir_all(trees.root.join("plain")).unwrap();
    std::os::unix::fs::symlink(trees.c.join("src"), session.join("link")).unwrap();
    assert_never_less_than_main(
        &trees.gate,
        &trees.main,
        &session,
        &trees.root.join("home"),
        DIFFERENTIAL_COMMANDS,
    );
}

/// The same comparison with the install root below the git root and the hook
/// process in the install root of the session.
#[test]
fn gate_never_audits_less_than_main_below_the_git_root() {
    if skip_without_tools() {
        return;
    }
    let (_tmp, root) = spaced_root();
    let repo = root.join("repo");
    init_repo(&repo);
    let app = repo.join("packages/app");
    let gate = install_gate(&app, ".claude");
    install_gate(&repo.join("packages/other"), ".claude");
    std::fs::create_dir_all(app.join("src")).unwrap();
    let repo_q = repo.display().to_string();
    let cd_root = format!("cd \"{repo_q}\" && git commit -m x");
    let mut commands = vec![
        "git -C ../other commit -m x",
        "git -C ../.. commit -m x",
        "git -C .. commit -m x",
        "cd ../other && git commit -m x",
        cd_root.as_str(),
    ];
    commands.extend_from_slice(DIFFERENTIAL_COMMANDS);
    assert_never_less_than_main(&gate, &app, &app, &root.join("home"), &commands);
    // The same with the hook process in the repository root.
    assert_never_less_than_main(&gate, &repo, &app, &root.join("home"), &commands);
}
