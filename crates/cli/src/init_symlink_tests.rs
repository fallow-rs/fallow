//! A cloned repository can commit a project file that `init` or
//! `hooks install --target git` writes as a symlink to a file outside the
//! project. These commands must leave such a target alone.

use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::ExitCode;

use crate::init::{
    GitHooksInstallOptions, GitHooksUninstallOptions, InitOptions, run_git_hooks_install,
    run_git_hooks_uninstall, run_init,
};

fn init_opts(root: &Path, agents: bool) -> InitOptions<'_> {
    InitOptions {
        root,
        use_toml: false,
        agents,
        hooks: false,
        branch: None,
        decline: false,
        quiet: true,
    }
}

fn hooks_install(root: &Path, force: bool) -> ExitCode {
    run_git_hooks_install(&GitHooksInstallOptions {
        root,
        branch: Some("main"),
        dry_run: false,
        force,
        prune_baselines: false,
    })
}

fn hooks_uninstall(root: &Path) -> ExitCode {
    run_git_hooks_uninstall(&GitHooksUninstallOptions {
        root,
        dry_run: false,
        force: true,
    })
}

#[test]
fn init_does_not_append_to_a_gitignore_outside_the_project() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("ignore");
    std::fs::write(&target, "keep\n").unwrap();
    symlink(&target, root.path().join(".gitignore")).unwrap();

    run_init(&init_opts(root.path(), false));

    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep\n");
}

#[test]
fn init_appends_to_a_gitignore_link_inside_the_project() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("config")).unwrap();
    std::fs::write(root.path().join("config/ignore"), "dist/\n").unwrap();
    symlink("config/ignore", root.path().join(".gitignore")).unwrap();

    assert_eq!(run_init(&init_opts(root.path(), false)), ExitCode::SUCCESS);

    assert_eq!(
        std::fs::read_to_string(root.path().join("config/ignore")).unwrap(),
        "dist/\n.fallow/\n"
    );
}

#[test]
fn init_does_not_create_a_config_through_a_dangling_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let planted = outside.path().join("rc.json");
    symlink(&planted, root.path().join(".fallowrc.json")).unwrap();

    assert_ne!(run_init(&init_opts(root.path(), false)), ExitCode::SUCCESS);

    assert!(!planted.exists());
}

#[test]
fn init_agents_does_not_create_a_guide_through_a_dangling_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let planted = outside.path().join("AGENTS.md");
    symlink(&planted, root.path().join("AGENTS.md")).unwrap();

    assert_ne!(run_init(&init_opts(root.path(), true)), ExitCode::SUCCESS);

    assert!(!planted.exists());
}

#[test]
fn hooks_install_does_not_write_into_a_husky_directory_outside_the_project() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join(".husky")).unwrap();

    assert_ne!(hooks_install(root.path(), false), ExitCode::SUCCESS);

    assert!(!outside.path().join("pre-commit").exists());
}

#[test]
fn forced_hooks_install_does_not_overwrite_a_linked_hook_outside_the_project() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("script");
    std::fs::write(&target, "keep\n").unwrap();
    std::fs::create_dir(root.path().join(".husky")).unwrap();
    symlink(&target, root.path().join(".husky/pre-commit")).unwrap();

    assert_ne!(hooks_install(root.path(), true), ExitCode::SUCCESS);

    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep\n");
}

#[test]
fn forced_hooks_uninstall_does_not_delete_a_hook_outside_the_project() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("pre-commit"), "#!/bin/sh\n").unwrap();
    symlink(outside.path(), root.path().join(".husky")).unwrap();

    assert_ne!(hooks_uninstall(root.path()), ExitCode::SUCCESS);

    assert!(outside.path().join("pre-commit").is_file());
}

/// `.git/` is not part of a clone, and users link `.git/hooks` on purpose.
#[test]
fn hooks_install_writes_through_a_linked_git_hooks_directory() {
    let root = tempfile::tempdir().unwrap();
    let shared = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    symlink(shared.path(), root.path().join(".git/hooks")).unwrap();

    assert_eq!(hooks_install(root.path(), false), ExitCode::SUCCESS);
    assert!(shared.path().join("pre-commit").is_file());

    assert_eq!(hooks_uninstall(root.path()), ExitCode::SUCCESS);
    assert!(!shared.path().join("pre-commit").exists());
}
