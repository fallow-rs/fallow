#![expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]

use std::os::unix::fs::symlink;
use std::path::Path;

use crate::setup_hooks::{
    AGENTS_BLOCK_END, AGENTS_BLOCK_START, HOOK_SCRIPT_MARKER, build_hooks_status,
};

const OUTSIDE_LINE: &str = "OUTSIDE-TARGET-LINE";

fn link_out(root: &Path, outside: &Path, relative: &str, text: &str) {
    let target = outside.join(relative.replace('/', "_"));
    std::fs::write(&target, text).unwrap();
    let link = root.join(relative);
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    symlink(&target, &link).unwrap();
}

#[test]
fn a_git_hook_link_out_of_the_root_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    link_out(
        root.path(),
        outside.path(),
        ".husky/pre-commit",
        &format!("#!/bin/sh\n{}\n", crate::init::GIT_HOOK_MARKER),
    );

    let status = build_hooks_status(root.path());

    assert!(!status.git.installed);
    assert!(!status.git.user_edited);
}

#[test]
fn a_gate_script_link_out_of_the_root_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    link_out(
        root.path(),
        outside.path(),
        ".claude/hooks/fallow-gate.sh",
        &format!("{HOOK_SCRIPT_MARKER}\n# Installer version: {OUTSIDE_LINE}\n"),
    );

    let status = build_hooks_status(root.path());

    assert_eq!(status.claude.script_version, None);
    assert!(!status.claude.user_edited);
}

#[test]
fn a_gate_script_link_inside_the_root_is_read() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("shared")).unwrap();
    std::fs::write(
        root.path().join("shared/gate.sh"),
        format!("{HOOK_SCRIPT_MARKER}\n# Installer version: 1.2.3\n"),
    )
    .unwrap();
    std::fs::create_dir_all(root.path().join(".claude/hooks")).unwrap();
    symlink(
        "../../shared/gate.sh",
        root.path().join(".claude/hooks/fallow-gate.sh"),
    )
    .unwrap();

    let status = build_hooks_status(root.path());

    assert_eq!(status.claude.script_version.as_deref(), Some("1.2.3"));
}

#[test]
fn an_agents_file_link_out_of_the_root_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    link_out(
        root.path(),
        outside.path(),
        "AGENTS.md",
        &format!("# Guide\n\n{AGENTS_BLOCK_START}\n{OUTSIDE_LINE}\n{AGENTS_BLOCK_END}\n"),
    );

    let status = build_hooks_status(root.path());

    assert!(!status.codex.installed);
    assert!(!status.codex.managed_block_present);
}
