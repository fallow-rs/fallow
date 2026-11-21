//! The base snapshot cache of `fallow audit` keys each snapshot on the inputs
//! of the base pass, so a run never reads a snapshot of other inputs.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{commit_all, git, run_fallow_raw_with_type_aware_sidecar};

/// The number of base snapshot files in the cache of `dir`.
fn snapshot_count(dir: &Path) -> usize {
    let cache = dir.join(".fallow/cache");
    let snapshots = fs::read_dir(&cache)
        .expect("the cache dir should exist")
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("audit-base-v")
        })
        .expect("the base snapshot dir should exist")
        .path();
    fs::read_dir(snapshots)
        .expect("the base snapshot dir should be readable")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "bin"))
        .count()
}

#[test]
fn a_syntactic_audit_does_not_read_the_type_aware_base_snapshot() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-base-identity", "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions": {"strict": true}}"#,
    )
    .unwrap();
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"typeAware": {"enabled": true}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unusedBase = () => 0;\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");
    git(dir, &["checkout", "-b", "feature"]);
    fs::write(
        dir.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unusedBase = () => 0;\nexport const unusedNew = () => 2;\n",
    )
    .unwrap();
    commit_all(dir, "add new unused export to an existing file");

    let root = dir.to_str().unwrap();
    let type_aware = [
        "audit", "--root", root, "--base", "main", "--format", "json", "--quiet",
    ];
    let syntactic = [
        "audit",
        "--root",
        root,
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
        "--no-type-aware",
    ];

    run_fallow_raw_with_type_aware_sidecar(&type_aware);
    assert_eq!(
        snapshot_count(dir),
        1,
        "a type-aware run stores its base snapshot"
    );
    run_fallow_raw_with_type_aware_sidecar(&syntactic);
    assert_eq!(
        snapshot_count(dir),
        2,
        "a syntactic run must store its own base snapshot"
    );
    run_fallow_raw_with_type_aware_sidecar(&type_aware);
    assert_eq!(snapshot_count(dir), 2, "a cache hit adds no snapshot");
}
