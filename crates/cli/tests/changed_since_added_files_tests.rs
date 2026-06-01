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

const ORIGINAL_BLOCK: &str = r"export function original() {
  const alpha = 'alpha';
  const beta = 'beta';
  const gamma = 'gamma';
  const delta = 'delta';
  const epsilon = 'epsilon';
  const zeta = 'zeta';
  const eta = 'eta';
  const theta = 'theta';
  const iota = 'iota';
  const kappa = 'kappa';
  return [alpha, beta, gamma, delta, epsilon, zeta, eta, theta, iota, kappa].join(',');
}
";

const ADDED_DUPLICATE_BLOCK: &str = r"export function dupe() {
  const alpha = 'alpha';
  const beta = 'beta';
  const gamma = 'gamma';
  const delta = 'delta';
  const epsilon = 'epsilon';
  const zeta = 'zeta';
  const eta = 'eta';
  const theta = 'theta';
  const iota = 'iota';
  const kappa = 'kappa';
  return [alpha, beta, gamma, delta, epsilon, zeta, eta, theta, iota, kappa].join(',');
}
";

fn git(dir: &Path, args: &[&str]) {
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

fn build_fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"changed-since-added-files","private":true,"type":"module"}"#,
    )
    .unwrap();
    fs::write(dir.join("src/base.ts"), "export const used = 1;\n").unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './base';\nconsole.log(used);\n",
    )
    .unwrap();
    fs::write(dir.join("src/original.ts"), ORIGINAL_BLOCK).unwrap();

    git(dir, &["init", "-b", "main"]);
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );

    fs::write(dir.join("src/new.ts"), "export const unused = 42;\n").unwrap();
    fs::write(dir.join("src/dupe.ts"), ADDED_DUPLICATE_BLOCK).unwrap();

    git(dir, &["add", "."]);
    git(
        dir,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "add changed files",
        ],
    );

    tmp
}

fn build_dirty_modified_fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"changed-since-dirty-modified","private":true,"type":"module"}"#,
    )
    .unwrap();
    fs::write(dir.join("src/base.ts"), "export const used = 1;\n").unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './base';\nconsole.log(used);\n",
    )
    .unwrap();

    git(dir, &["init", "-b", "main"]);
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );

    fs::write(
        dir.join("src/base.ts"),
        "export const used = 1;\nexport type DirtyUnused = { value: number };\n",
    )
    .unwrap();

    tmp
}

fn build_staged_added_export_fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"changed-since-staged-added","private":true,"type":"module"}"#,
    )
    .unwrap();
    fs::write(dir.join("src/base.ts"), "export const used = 1;\n").unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './base';\nconsole.log(used);\n",
    )
    .unwrap();

    git(dir, &["init", "-b", "main"]);
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );

    fs::write(
        dir.join("src/extra.ts"),
        "export const extra = 2;\nexport type AddedUnused = { value: number };\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './base';\nimport { extra } from './extra';\nconsole.log(used, extra);\n",
    )
    .unwrap();

    git(dir, &["add", "src/index.ts", "src/extra.ts"]);

    tmp
}

fn build_untracked_added_export_fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"changed-since-untracked-added","private":true,"type":"module"}"#,
    )
    .unwrap();
    fs::write(dir.join("src/base.ts"), "export const used = 1;\n").unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './base';\nconsole.log(used);\n",
    )
    .unwrap();

    git(dir, &["init", "-b", "main"]);
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );

    fs::write(
        dir.join("src/extra.ts"),
        "export const extra = 2;\nexport type UntrackedUnused = { value: number };\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './base';\nimport { extra } from './extra';\nconsole.log(used, extra);\n",
    )
    .unwrap();

    tmp
}

fn collect_paths(items: &serde_json::Value) -> Vec<String> {
    items
        .as_array()
        .expect("expected array")
        .iter()
        .filter_map(|item| {
            item.get("path")
                .or_else(|| item.get("file"))
                .and_then(serde_json::Value::as_str)
        })
        .map(ToOwned::to_owned)
        .collect()
}

#[test]
fn check_changed_since_keeps_added_file_findings() {
    let tmp = build_fixture();
    let output = run_fallow_raw(&[
        "check",
        "--root",
        tmp.path().to_str().unwrap(),
        "--changed-since",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 1, "check should fail on added-file issues");

    let json = parse_json(&output);
    let unused_files = collect_paths(&json["unused_files"]);

    assert!(
        unused_files.iter().any(|path| path.ends_with("src/new.ts")),
        "added unused file must survive --changed-since filtering: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path.ends_with("src/dupe.ts")),
        "added duplicate file must survive --changed-since filtering: {unused_files:?}"
    );
}

#[test]
fn check_changed_since_keeps_dirty_modified_file_type_findings() {
    let tmp = build_dirty_modified_fixture();
    let output = run_fallow_raw(&[
        "check",
        "--root",
        tmp.path().to_str().unwrap(),
        "--changed-since",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "check should fail on dirty changed-file issues"
    );

    let json = parse_json(&output);
    let unused_types = json["unused_types"]
        .as_array()
        .expect("unused_types should be an array");

    assert!(
        unused_types.iter().any(|item| {
            item["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("src/base.ts"))
                && item["export_name"].as_str() == Some("DirtyUnused")
        }),
        "dirty modified file must survive --changed-since filtering: {unused_types:?}"
    );
}

#[test]
fn check_changed_since_keeps_staged_added_file_type_findings() {
    let tmp = build_staged_added_export_fixture();
    let output = run_fallow_raw(&[
        "check",
        "--root",
        tmp.path().to_str().unwrap(),
        "--changed-since",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "check should fail on staged added-file issues"
    );

    let json = parse_json(&output);
    let unused_types = json["unused_types"]
        .as_array()
        .expect("unused_types should be an array");

    assert!(
        unused_types.iter().any(|item| {
            item["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("src/extra.ts"))
                && item["export_name"].as_str() == Some("AddedUnused")
        }),
        "staged added file must survive --changed-since filtering: {unused_types:?}"
    );
}

#[test]
fn check_changed_since_keeps_untracked_added_file_type_findings() {
    let tmp = build_untracked_added_export_fixture();
    let output = run_fallow_raw(&[
        "check",
        "--root",
        tmp.path().to_str().unwrap(),
        "--changed-since",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "check should fail on untracked added-file issues"
    );

    let json = parse_json(&output);
    let unused_types = json["unused_types"]
        .as_array()
        .expect("unused_types should be an array");

    assert!(
        unused_types.iter().any(|item| {
            item["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("src/extra.ts"))
                && item["export_name"].as_str() == Some("UntrackedUnused")
        }),
        "untracked added file must survive --changed-since filtering: {unused_types:?}"
    );
}

#[test]
fn dupes_changed_since_keeps_groups_with_added_file_instances() {
    let tmp = build_fixture();
    let output = run_fallow_raw(&[
        "dupes",
        "--root",
        tmp.path().to_str().unwrap(),
        "--changed-since",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 0, "dupes should exit 0 without a threshold");

    let json = parse_json(&output);
    let clone_groups = json["clone_groups"]
        .as_array()
        .expect("clone_groups should be an array");
    assert!(
        !clone_groups.is_empty(),
        "added duplicate file should produce at least one clone group"
    );

    let files: Vec<String> = clone_groups
        .iter()
        .flat_map(|group| {
            group["instances"]
                .as_array()
                .expect("instances should be an array")
                .iter()
                .filter_map(|instance| instance["file"].as_str())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();

    assert!(
        files.iter().any(|path| path.ends_with("src/dupe.ts")),
        "added duplicate instance must survive --changed-since filtering: {files:?}"
    );
}

#[test]
fn combined_changed_since_keeps_added_file_findings() {
    let tmp = build_fixture();
    let output = run_fallow_raw(&[
        "--root",
        tmp.path().to_str().unwrap(),
        "--changed-since",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "combined mode should not crash, got exit code {}",
        output.code
    );

    let json = parse_json(&output);
    let unused_files = collect_paths(&json["check"]["unused_files"]);
    let clone_groups = json["dupes"]["clone_groups"]
        .as_array()
        .expect("combined dupes.clone_groups should be an array");

    assert!(
        unused_files.iter().any(|path| path.ends_with("src/new.ts")),
        "combined check output must retain added unused files: {unused_files:?}"
    );
    assert!(
        !clone_groups.is_empty(),
        "combined dupes output must retain groups touching added files"
    );
}
