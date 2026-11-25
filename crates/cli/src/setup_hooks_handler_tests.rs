//! Tests for the project hook handlers that run the gate script.
//!
//! Each test runs a handler command through `sh -c` with a stub gate script
//! that prints its working directory.

use std::path::{Path, PathBuf};

use tempfile::tempdir;

use super::{PROJECT_FALLOW_HANDLER_COMMAND, codex_project_handler_command};

/// Runs `handler` through `sh -c` from `cwd` with a stub gate script at
/// `<install_root>/<dir>/hooks/fallow-gate.sh` that prints its cwd.
fn run_handler_stub(
    handler: &str,
    dir: &str,
    install_root: &Path,
    cwd: &Path,
    project_dir: &Path,
) -> std::process::Output {
    use std::os::unix::fs::PermissionsExt;
    let hooks_dir = install_root.join(dir).join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let script = hooks_dir.join("fallow-gate.sh");
    std::fs::write(&script, "#!/bin/sh\npwd -P\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(cwd).unwrap();
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg(handler)
        .current_dir(cwd)
        .env("CLAUDE_PROJECT_DIR", project_dir)
        .env("HOME", project_dir.join("no-home"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(var);
    }
    let out = command.output().unwrap();
    std::fs::remove_file(&script).unwrap();
    out
}

fn git(dir: &Path, args: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn commit_all(dir: &Path) {
    git(dir, &["add", "-A"]);
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

fn codex(rel: &str) -> String {
    codex_project_handler_command(Path::new(rel))
}

/// Codex and Claude do not block on exit 127, so a handler that cannot
/// find the script lets every commit through. Each project handler must
/// reach the install root from any cwd below it, also when the install
/// root is not the git root.
#[test]
fn project_handlers_run_the_gate_from_the_install_root() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path().canonicalize().unwrap();
    git(&repo, &["init", "-q"]);
    let nested = repo.join("packages/app");
    for dir in [".codex", ".claude"] {
        for (install_root, rel, cwd) in [
            (&repo, ".", repo.clone()),
            (&repo, ".", repo.join("src/deep")),
            (&nested, "packages/app", nested.clone()),
            (&nested, "packages/app", nested.join("src")),
        ] {
            let handler = if dir == ".codex" {
                codex(rel)
            } else {
                PROJECT_FALLOW_HANDLER_COMMAND.to_string()
            };
            let out = run_handler_stub(&handler, dir, install_root, &cwd, install_root);
            assert!(
                out.status.success(),
                "{dir} handler failed from {}",
                cwd.display()
            );
            assert_eq!(
                String::from_utf8_lossy(&out.stdout).trim(),
                install_root.to_string_lossy(),
                "wrong {dir} gate cwd from {}",
                cwd.display()
            );
        }
    }
}

/// The Codex handler is in a committed file, so it holds no absolute path. A
/// clone at another path and a linked worktree of the repository keep the
/// gate.
#[test]
fn codex_handler_runs_the_gate_of_each_checkout() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let main = base.join("main");
    git(&main, &["init", "-q"]);
    std::fs::create_dir_all(main.join("packages/app/.codex/hooks")).unwrap();
    std::fs::write(main.join("packages/app/.codex/hooks/keep"), "").unwrap();
    commit_all(&main);
    let linked = base.join("linked");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    let clone = base.join("elsewhere/clone");
    git(
        &base,
        &[
            "clone",
            "-q",
            main.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );

    let handler = codex("packages/app");
    for checkout in [&main, &linked, &clone] {
        let app: PathBuf = checkout.join("packages/app");
        let out = run_handler_stub(&handler, ".codex", &app, &app.join("src"), &app);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            app.to_string_lossy(),
            "no gate in {}: {}",
            checkout.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// A nested worktree has its own `.git` entry. The walk must not reach the
/// gate of the checkout around it. Codex then has no gate; Claude falls
/// back to the script under `$CLAUDE_PROJECT_DIR`, run from the cwd.
#[test]
fn project_handlers_stop_at_a_nested_worktree() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path().canonicalize().unwrap();
    git(&repo, &["init", "-q"]);
    commit_all(&repo);
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
    let cwd = worktree.join("src");

    let codex = run_handler_stub(&codex("."), ".codex", &repo, &cwd, &repo);
    assert!(
        codex.stdout.is_empty(),
        "the Codex handler ran the outer gate: {}",
        String::from_utf8_lossy(&codex.stdout)
    );
    assert!(codex.status.success(), "a missing Codex gate must exit 0");
    assert!(
        codex.stderr.is_empty(),
        "a missing Codex gate must stay quiet"
    );

    let claude = run_handler_stub(
        PROJECT_FALLOW_HANDLER_COMMAND,
        ".claude",
        &repo,
        &cwd,
        &repo,
    );
    assert!(claude.status.success());
    assert_eq!(
        String::from_utf8_lossy(&claude.stdout).trim(),
        cwd.to_string_lossy(),
        "the Claude fallback must run from the cwd, not from the outer checkout"
    );
}

/// A linked worktree of the same repository inside the main checkout is its
/// own checkout, not a nested repository. The Codex handler runs the gate
/// script of that worktree.
#[test]
fn codex_handler_runs_the_gate_of_a_linked_worktree_inside_the_main_checkout() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path().canonicalize().unwrap();
    git(&repo, &["init", "-q"]);
    commit_all(&repo);
    let worktree = repo.join(".worktrees/feat");
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

    let out = run_handler_stub(
        &codex("."),
        ".codex",
        &worktree,
        &worktree.join("src"),
        &worktree,
    );

    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        worktree.to_string_lossy(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A nested repository, submodule or vendored package below the project
/// can commit its own `.claude/hooks/fallow-gate.sh`. The handler of the
/// project must run only the script of the install root.
#[test]
fn project_handlers_never_run_the_gate_script_of_a_nested_repository() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempdir().unwrap();
    let repo = tmp.path().canonicalize().unwrap();
    git(&repo, &["init", "-q"]);
    let vendored = repo.join("vendor/pkg");
    git(&vendored, &["init", "-q"]);
    let cwd = vendored.join("src");
    for dir in [".claude", ".codex"] {
        let nested_hooks = vendored.join(dir).join("hooks");
        std::fs::create_dir_all(&nested_hooks).unwrap();
        let nested_script = nested_hooks.join("fallow-gate.sh");
        std::fs::write(&nested_script, "#!/bin/sh\necho nested-script\n").unwrap();
        std::fs::set_permissions(&nested_script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let handler = if dir == ".codex" {
            codex(".")
        } else {
            PROJECT_FALLOW_HANDLER_COMMAND.to_string()
        };
        let out = run_handler_stub(&handler, dir, &repo, &cwd, &repo);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{dir}: {stdout}");
        assert!(
            !stdout.contains("nested-script"),
            "the {dir} handler ran the script of the nested repository"
        );
    }
}

/// The Claude handler resolves `$CLAUDE_PROJECT_DIR` physically and runs the
/// gate script of that resolved root, from that root. A `..` after a link
/// then names one directory for both the script and the working directory.
#[test]
fn claude_handler_runs_the_script_of_the_physical_root() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let real = base.join("real");
    std::fs::create_dir_all(real.join("deep")).unwrap();
    std::os::unix::fs::symlink(real.join("deep"), base.join("link")).unwrap();
    let logical_hooks = base.join(".claude/hooks");
    std::fs::create_dir_all(&logical_hooks).unwrap();
    std::fs::write(
        logical_hooks.join("fallow-gate.sh"),
        "#!/bin/sh\necho logical\n",
    )
    .unwrap();
    std::fs::set_permissions(
        logical_hooks.join("fallow-gate.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let out = run_handler_stub(
        PROJECT_FALLOW_HANDLER_COMMAND,
        ".claude",
        &real,
        &real.join("src"),
        &base.join("link/.."),
    );

    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        real.to_string_lossy(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
