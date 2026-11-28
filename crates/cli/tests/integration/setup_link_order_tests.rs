//! The setup commands check a project path for a link out of the project
//! before they check whether the path exists. A link out of the project then
//! gets the same answer whether its target exists or not.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{CommandOutput, run_fallow_raw_with_clean_git};

/// A project in a Git repository with a knip config to migrate.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    let git = std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir.path())
        .status()
        .expect("git init should run");
    assert!(git.success());
    std::fs::write(dir.path().join("package.json"), r#"{"name":"app"}"#).unwrap();
    std::fs::write(
        dir.path().join("knip.json"),
        r#"{"entry":["src/index.ts"]}"#,
    )
    .unwrap();
    dir
}

fn run(root: &Path, args: &[&str]) -> CommandOutput {
    let root = root.to_str().unwrap();
    let mut full = vec!["--root", root];
    full.extend_from_slice(args);
    run_fallow_raw_with_clean_git(&full)
}

/// Run `args` in a project where `name` links out of the project, once
/// with an existing target and once with a missing one. Both runs must be
/// refused with the same exit code and the same message, and must not
/// create `created`.
fn assert_same_refusal(name: &str, target_is_dir: bool, args: &[&str], created: &str) {
    let mut outputs = Vec::new();
    for target_exists in [true, false] {
        let dir = project();
        let outside = tempfile::tempdir().expect("create outside dir");
        let target = outside.path().join("target");
        if target_exists && target_is_dir {
            std::fs::create_dir(&target).unwrap();
        } else if target_exists {
            std::fs::write(&target, "{}").unwrap();
        }
        std::os::unix::fs::symlink(&target, dir.path().join(name)).unwrap();

        let output = run(dir.path(), args);

        assert_eq!(output.code, 2, "{name}: stderr: {}", output.stderr);
        assert!(
            dir.path().join(created).symlink_metadata().is_err(),
            "{name}: {created} must not be created"
        );
        outputs.push((output.code, output.stderr));
    }
    assert_eq!(outputs[0], outputs[1], "{name}: {args:?}");
}

#[test]
fn init_refuses_a_config_link_out_of_the_project_before_the_exists_check() {
    assert_same_refusal(".fallowrc.jsonc", false, &["init"], ".fallowrc.json");
}

#[test]
fn init_agents_refuses_a_link_out_of_the_project_before_the_exists_check() {
    assert_same_refusal("AGENTS.md", false, &["init", "--agents"], "AGENTS.md/x");
}

#[test]
fn migrate_refuses_a_config_link_out_of_the_project_before_the_exists_check() {
    assert_same_refusal(".fallowrc.jsonc", false, &["migrate"], ".fallowrc.json");
}

#[test]
fn hooks_install_refuses_a_husky_link_out_of_the_project_before_the_probe() {
    assert_same_refusal(
        ".husky",
        true,
        &["hooks", "install", "--target", "git"],
        ".git/hooks/pre-commit",
    );
}

#[test]
fn hooks_install_refuses_a_lefthook_link_out_of_the_project_before_the_probe() {
    for name in ["lefthook.yml", "lefthook.json"] {
        assert_same_refusal(
            name,
            false,
            &["hooks", "install", "--target", "git"],
            ".git/hooks/pre-commit",
        );
    }
    assert_same_refusal(
        ".lefthook",
        true,
        &["hooks", "install", "--target", "git"],
        ".git/hooks/pre-commit",
    );
}

/// A link inside the project keeps the behavior of a plain file.
#[test]
fn setup_commands_keep_links_inside_the_project() {
    let dir = project();
    let root = dir.path();
    std::fs::create_dir(root.join("shared")).unwrap();
    std::fs::write(root.join("shared/fallowrc.jsonc"), "{}").unwrap();
    std::fs::write(root.join("shared/AGENTS.md"), "# Agents\n").unwrap();
    std::fs::write(root.join("shared/lefthook.yml"), "").unwrap();
    std::os::unix::fs::symlink("shared/fallowrc.jsonc", root.join(".fallowrc.jsonc")).unwrap();
    std::os::unix::fs::symlink("shared/AGENTS.md", root.join("AGENTS.md")).unwrap();
    std::os::unix::fs::symlink("shared/lefthook.yml", root.join("lefthook.yml")).unwrap();

    for args in [&["init"][..], &["init", "--agents"], &["migrate"]] {
        let output = run(root, args);
        assert_eq!(output.code, 2, "{args:?}: {}", output.stderr);
        assert!(
            output.stderr.contains("already exists"),
            "{args:?}: {}",
            output.stderr
        );
    }
    let output = run(root, &["hooks", "install", "--target", "git"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("Lefthook detected"),
        "{}",
        output.stderr
    );

    std::fs::remove_file(root.join("lefthook.yml")).unwrap();
    std::fs::create_dir(root.join("shared/husky")).unwrap();
    std::os::unix::fs::symlink("shared/husky", root.join(".husky")).unwrap();
    let output = run(root, &["hooks", "install", "--target", "git"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(root.join("shared/husky/pre-commit").is_file());
}
