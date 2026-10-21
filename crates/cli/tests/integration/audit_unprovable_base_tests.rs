//! `audit --gate new-only` attribution of a finding class that the base run
//! cannot prove.
//!
//! The base snapshot is a checkout of committed files only. A workspace member
//! directory that `.gitignore` excludes (a cloned sub-repository) is present
//! in the working tree but missing in the base. With a missing workspace
//! pattern directory, a run does not report unused catalog entries. The head
//! reports them, and they must stay inherited.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{commit_all, git, parse_json, run_fallow_raw};

const CONFIG: &str = r#"{
  "rules": {
    "unused-catalog-entries": "error",
    "unused-dependencies": "off",
    "unresolved-imports": "off",
    "unlisted-dependencies": "off"
  }
}"#;

/// A pnpm workspace with `packages/*` and `vendor/*` members and a catalog
/// with one used and one unused entry. `vendor/` is gitignored. A `head`
/// branch is checked out on top of the base commit.
fn fixture(workspace_yaml: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("packages/app")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "unprovable-base", "private": true}"#,
    )
    .unwrap();
    fs::write(dir.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    fs::write(dir.join(".gitignore"), "vendor/\n").unwrap();
    fs::write(dir.join(".fallowrc.json"), CONFIG).unwrap();
    fs::write(
        dir.join("packages/app/package.json"),
        r#"{"name": "app", "main": "index.js", "dependencies": {"used-dep": "catalog:"}}"#,
    )
    .unwrap();
    fs::write(dir.join("packages/app/index.js"), "export const app = 1;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    tmp
}

/// Add the gitignored workspace member that only the working tree has.
fn clone_vendor_member(dir: &Path) {
    fs::create_dir_all(dir.join("vendor/lib")).unwrap();
    fs::write(
        dir.join("vendor/lib/package.json"),
        r#"{"name": "lib", "main": "index.js"}"#,
    )
    .unwrap();
    fs::write(dir.join("vendor/lib/index.js"), "export const lib = 1;\n").unwrap();
}

/// Commit a change to the app and to `pnpm-workspace.yaml`. The audit reports
/// catalog findings only when the change touches the file that declares them.
fn edit_workspace(dir: &Path) {
    fs::write(dir.join("packages/app/index.js"), "export const app = 2;\n").unwrap();
    let yaml = dir.join("pnpm-workspace.yaml");
    let mut source = fs::read_to_string(&yaml).unwrap();
    source.push_str("# edited\n");
    fs::write(yaml, source).unwrap();
    commit_all(dir, "head");
}

struct Audit {
    code: i32,
    verdict: String,
    introduced: u64,
    inherited: u64,
    /// `(entry_name, introduced)` of each unused catalog entry.
    entries: Vec<(String, bool)>,
}

fn audit(dir: &Path) -> Audit {
    audit_with(dir, &["--no-cache"])
}

fn audit_with(dir: &Path, extra: &[&str]) -> Audit {
    let mut args = vec![
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--gate",
        "new-only",
        "--format",
        "json",
        "--quiet",
        "--no-css",
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_raw(&args);
    assert!(
        output.code == 0 || output.code == 1,
        "audit failed. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let entries = json["dead_code"]["unused_catalog_entries"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    (
                        item["entry_name"].as_str().unwrap().to_string(),
                        item["introduced"].as_bool().expect("introduced flag"),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Audit {
        code: output.code,
        verdict: json["verdict"].as_str().unwrap().to_string(),
        introduced: json["attribution"]["dead_code_introduced"]
            .as_u64()
            .unwrap(),
        inherited: json["attribution"]["dead_code_inherited"].as_u64().unwrap(),
        entries,
    }
}

const WORKSPACE_WITH_VENDOR: &str =
    "packages:\n  - packages/*\n  - vendor/*\ncatalog:\n  used-dep: ^1.0.0\n  stale-dep: ^1.0.0\n";

#[test]
fn findings_of_a_class_the_base_cannot_prove_are_inherited() {
    let tmp = fixture(WORKSPACE_WITH_VENDOR);
    clone_vendor_member(tmp.path());
    edit_workspace(tmp.path());

    let result = audit(tmp.path());

    assert_eq!(result.entries, vec![("stale-dep".to_string(), false)]);
    assert_eq!(result.code, 0, "expected a pass: {:?}", result.entries);
    assert_eq!(result.verdict, "pass");
    assert_eq!(result.introduced, 0);
    assert_eq!(result.inherited, 1);
}

#[test]
fn a_cached_base_snapshot_keeps_the_unprovable_class() {
    let tmp = fixture(WORKSPACE_WITH_VENDOR);
    clone_vendor_member(tmp.path());
    edit_workspace(tmp.path());

    let cold = audit_with(tmp.path(), &[]);
    let warm = audit_with(tmp.path(), &[]);

    for result in [cold, warm] {
        assert_eq!(result.entries, vec![("stale-dep".to_string(), false)]);
        assert_eq!(result.verdict, "pass");
        assert_eq!(result.introduced, 0);
    }
}

#[test]
fn a_class_the_base_proves_still_gates_new_findings() {
    let tmp = fixture("packages:\n  - packages/*\ncatalog:\n  used-dep: ^1.0.0\n");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\ncatalog:\n  used-dep: ^1.0.0\n  stale-dep: ^1.0.0\n",
    )
    .unwrap();
    edit_workspace(tmp.path());

    let result = audit(tmp.path());

    assert_eq!(result.entries, vec![("stale-dep".to_string(), true)]);
    assert_eq!(result.code, 1, "expected a fail: {:?}", result.entries);
    assert_eq!(result.verdict, "fail");
    assert_eq!(result.introduced, 1);
}

#[test]
fn a_head_without_the_member_and_a_base_without_it_compare_normally() {
    let tmp = fixture(WORKSPACE_WITH_VENDOR);
    edit_workspace(tmp.path());

    let result = audit(tmp.path());

    assert!(result.entries.is_empty(), "{:?}", result.entries);
    assert_eq!(result.verdict, "pass");
}
