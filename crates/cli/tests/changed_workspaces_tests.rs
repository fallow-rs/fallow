#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Build a minimal two-workspace pnpm monorepo, committed on `main`.
fn create_monorepo_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();

    fs::create_dir_all(dir.join("packages/ui/src")).unwrap();
    fs::create_dir_all(dir.join("packages/api/src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"monorepo","private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();

    fs::write(
        dir.join("packages/ui/package.json"),
        r#"{"name":"@mono/ui","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("packages/ui/src/index.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/ui/src/utils.ts"),
        "export const used = () => 42;\nexport const unused_ui = () => 0;\n",
    )
    .unwrap();

    fs::write(
        dir.join("packages/api/package.json"),
        r#"{"name":"@mono/api","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("packages/api/src/index.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/api/src/utils.ts"),
        "export const used = () => 42;\nexport const unused_api = () => 0;\n",
    )
    .unwrap();

    git_init_and_commit(dir);
    tmp
}

fn run_git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .status()
        .expect("git command failed");
    assert!(status.success(), "git {args:?} failed");
}

fn git_init_and_commit(dir: &Path) {
    run_git(dir, &["init", "-b", "main"]);
    run_git(dir, &["add", "."]);
    run_git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
}

fn touch_and_commit(dir: &Path, rel_path: &str, contents: &str, message: &str) {
    fs::write(dir.join(rel_path), contents).unwrap();
    run_git(dir, &["add", "."]);
    run_git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", message],
    );
}

#[test]
fn changed_workspaces_scopes_to_workspaces_with_changes() {
    let tmp = create_monorepo_fixture();
    let dir = tmp.path();

    touch_and_commit(
        dir,
        "packages/ui/src/extra.ts",
        "export const extra = 1;\n",
        "ui: add extra",
    );

    let output = run_fallow_raw(&[
        "check",
        "--root",
        dir.to_str().unwrap(),
        "--changed-workspaces",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "check should not crash: code={}, stderr={}",
        output.code,
        output.stderr
    );
    let json = parse_json(&output);

    let paths: Vec<String> = json["unused_exports"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e["path"].as_str().map(ToOwned::to_owned))
        .collect();

    assert!(
        paths
            .iter()
            .all(|p| p.contains("packages/ui/") || p.contains("packages\\ui\\")),
        "expected only UI-workspace exports after --changed-workspaces, got {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|p| p.contains("packages/ui/") || p.contains("packages\\ui\\")),
        "expected at least one UI-workspace export (unused_ui): {paths:?}"
    );
}

#[test]
fn changed_workspaces_scopes_to_workspace_with_untracked_file() {
    let tmp = create_monorepo_fixture();
    let dir = tmp.path();

    fs::write(
        dir.join("packages/ui/src/extra.ts"),
        "export const extra = 1;\nexport type UiUntracked = { value: number };\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/ui/src/index.ts"),
        "import { used } from './utils';\nimport { extra } from './extra';\nconsole.log(used(), extra);\n",
    )
    .unwrap();

    let output = run_fallow_raw(&[
        "check",
        "--root",
        dir.to_str().unwrap(),
        "--changed-workspaces",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "check should not crash: code={}, stderr={}",
        output.code,
        output.stderr
    );
    let json = parse_json(&output);

    let unused_types = json["unused_types"]
        .as_array()
        .expect("unused_types should be an array");
    assert!(
        unused_types.iter().any(|item| {
            item["path"].as_str().is_some_and(|path| {
                path.contains("packages/ui/src/extra.ts")
                    || path.contains("packages\\ui\\src\\extra.ts")
            }) && item["export_name"].as_str() == Some("UiUntracked")
        }),
        "expected untracked UI file to keep workspace-scoped findings: {unused_types:?}"
    );

    let export_paths: Vec<String> = json["unused_exports"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e["path"].as_str().map(ToOwned::to_owned))
        .collect();
    assert!(
        export_paths
            .iter()
            .all(|p| p.contains("packages/ui/") || p.contains("packages\\ui\\")),
        "expected only UI-workspace exports after --changed-workspaces, got {export_paths:?}"
    );
}

#[test]
fn workspace_and_changed_workspaces_are_mutually_exclusive() {
    let tmp = create_monorepo_fixture();
    let dir = tmp.path();

    let output = run_fallow_raw(&[
        "check",
        "--root",
        dir.to_str().unwrap(),
        "--workspace",
        "@mono/ui",
        "--changed-workspaces",
        "HEAD",
        "--quiet",
    ]);

    assert_ne!(
        output.code, 0,
        "combining --workspace and --changed-workspaces must fail"
    );
    assert!(
        output.stderr.contains("mutually exclusive"),
        "expected 'mutually exclusive' error, got stderr={}",
        output.stderr
    );
}

#[test]
fn changed_workspaces_bad_ref_is_hard_error() {
    let tmp = create_monorepo_fixture();
    let dir = tmp.path();

    let output = run_fallow_raw(&[
        "check",
        "--root",
        dir.to_str().unwrap(),
        "--changed-workspaces",
        "refs/heads/does-not-exist",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_ne!(
        output.code, 0,
        "unknown ref should cause a non-zero exit so CI notices instead of \
         silently widening analysis back to the full monorepo"
    );
    let combined = format!("{}\n{}", output.stdout, output.stderr);
    assert!(
        combined.contains("--changed-workspaces"),
        "error should mention --changed-workspaces, got:\n{combined}"
    );
}

#[test]
fn changed_workspaces_without_monorepo_errors() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"single","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join("src/index.ts"), "export const a = 1;\n").unwrap();
    git_init_and_commit(dir);

    let output = run_fallow_raw(&[
        "check",
        "--root",
        dir.to_str().unwrap(),
        "--changed-workspaces",
        "HEAD",
        "--quiet",
    ]);

    assert_ne!(output.code, 0);
    let combined = format!("{}\n{}", output.stdout, output.stderr);
    assert!(
        combined.contains("no workspaces found"),
        "expected 'no workspaces found' error, got:\n{combined}"
    );
}

#[test]
fn changed_workspaces_root_only_diff_scopes_to_empty() {
    let tmp = create_monorepo_fixture();
    let dir = tmp.path();

    touch_and_commit(
        dir,
        "package.json",
        r#"{"name":"monorepo","private":true,"workspaces":["packages/*"],"version":"0.0.1"}"#,
        "root: bump",
    );

    let output = run_fallow_raw(&[
        "check",
        "--root",
        dir.to_str().unwrap(),
        "--changed-workspaces",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 0,
        "root-only diff should not flag any workspace-scoped issues: stderr={}",
        output.stderr
    );
    let json = parse_json(&output);
    let exports = json["unused_exports"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        exports.is_empty(),
        "root-only change must map to zero scoped workspaces, got {} exports",
        exports.len()
    );
}
