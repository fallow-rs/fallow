//! The review brief reads renames from the diff of the run, with no
//! `--diff-file`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

use crate::common::{commit_all, fallow_bin, git, git_capture};

#[test]
fn a_moved_manifest_adds_no_dependency_to_the_brief() {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("packages/a/src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("packages/a/package.json"),
        r#"{"name":"pkg","main":"src/index.ts","dependencies":{"left-pad":"^1.3.0"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("packages/a/src/index.ts"),
        "import leftPad from 'left-pad';\nexport const a = () => leftPad('x', 2);\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "base");
    git(dir, &["checkout", "-b", "head"]);
    git(dir, &["mv", "packages/a", "packages/b"]);
    commit_all(dir, "move package");

    let renames = git_capture(dir, &["diff", "--name-status", "main"]);
    assert!(
        renames.contains("packages/a/package.json\tpackages/b/package.json"),
        "fixture: git must report the manifest as a rename, got:\n{renames}"
    );

    let output = Command::new(fallow_bin())
        .args(["review", "--brief", "--base", "main", "--format", "json"])
        .current_dir(dir)
        .output()
        .expect("review runs");
    let brief: Value = serde_json::from_slice(&output.stdout).expect("review brief JSON");
    assert!(brief["deltas"].is_object(), "fixture: no deltas in {brief}");
    let added = brief["deltas"]["dependency_added"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !added
            .iter()
            .any(|key| key == "packages/b/package.json::left-pad"),
        "a moved manifest adds no dependency, got {added:?}"
    );
}
