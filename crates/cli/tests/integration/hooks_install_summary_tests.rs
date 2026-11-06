//! Human summary lines of `fallow hooks install --target agent`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{CommandOutput, run_fallow_raw_with_env};

fn install_agent_hooks(root: &Path, args: &[&str]) -> CommandOutput {
    let root_str = root.to_str().expect("UTF-8 tempdir");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("create home");
    let mut full = vec!["hooks", "install", "--target", "agent", "--root", root_str];
    full.extend_from_slice(args);
    run_fallow_raw_with_env(&full, &[("HOME", home.to_str().expect("UTF-8 home"))])
}

fn summary_line<'a>(output: &'a CommandOutput, path: &str) -> &'a str {
    output
        .stderr
        .lines()
        .find(|line| line.trim_start().starts_with(path))
        .unwrap_or_else(|| panic!("no {path} line in stderr: {}", output.stderr))
}

#[test]
fn gitignore_line_reports_no_change_when_entry_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".gitignore"), "node_modules/\n.claude/\n")
        .expect("write .gitignore");

    let output = install_agent_hooks(dir.path(), &["--agent", "claude", "--gitignore-claude"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let line = summary_line(&output, ".gitignore");
    assert!(!line.contains("added"), "line: {line:?}");
    assert!(line.contains("unchanged"), "line: {line:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".gitignore")).expect("read .gitignore"),
        "node_modules/\n.claude/\n"
    );
}

#[test]
fn gitignore_line_reports_added_when_entry_is_new() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".gitignore"), "node_modules/\n").expect("write .gitignore");

    let output = install_agent_hooks(dir.path(), &["--agent", "claude", "--gitignore-claude"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let line = summary_line(&output, ".gitignore");
    assert!(line.contains(".claude/ added"), "line: {line:?}");
}

#[test]
fn codex_dry_run_says_would_create_for_the_hook_config() {
    let dir = tempfile::tempdir().expect("tempdir");

    let output = install_agent_hooks(dir.path(), &["--agent", "codex", "--dry-run"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let line = summary_line(&output, ".codex/hooks.json");
    assert!(line.ends_with("would create"), "line: {line:?}");
    assert!(!dir.path().join(".codex/hooks.json").exists());
}

#[test]
fn claude_dry_run_says_would_create_for_the_settings_file() {
    let dir = tempfile::tempdir().expect("tempdir");

    let output = install_agent_hooks(dir.path(), &["--agent", "claude", "--dry-run"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let line = summary_line(&output, ".claude/settings.json");
    assert!(line.ends_with("would create"), "line: {line:?}");
}

#[test]
fn gitignore_dry_run_says_would_add_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".gitignore"), "node_modules/\n").expect("write .gitignore");

    let output = install_agent_hooks(
        dir.path(),
        &["--agent", "claude", "--gitignore-claude", "--dry-run"],
    );

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let line = summary_line(&output, ".gitignore");
    assert!(line.ends_with("would add .claude/"), "line: {line:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".gitignore")).expect("read .gitignore"),
        "node_modules/\n"
    );
}

#[test]
fn gitignore_dry_run_reports_no_change_when_entry_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".gitignore"), ".claude\n").expect("write .gitignore");

    let output = install_agent_hooks(
        dir.path(),
        &["--agent", "claude", "--gitignore-claude", "--dry-run"],
    );

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let line = summary_line(&output, ".gitignore");
    assert!(
        line.ends_with("unchanged (.claude/ already listed)"),
        "line: {line:?}"
    );
}
