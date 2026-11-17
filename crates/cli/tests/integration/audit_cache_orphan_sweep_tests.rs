//! The test harness removes the audit base worktrees that earlier test runs
//! left in the temp directory (#4671).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::remove_orphaned_audit_caches;

/// Write a cache entry named `name` in `temp`, with its three sidecar files.
/// The `.last-used` file names `owner` as the owner repository.
fn write_entry(temp: &Path, name: &str, owner: &Path) {
    let entry = temp.join(name);
    fs::create_dir_all(entry.join("src")).unwrap();
    fs::write(entry.join("src/index.js"), "export const one = 1;\n").unwrap();
    fs::write(
        temp.join(format!("{name}.last-used")),
        format!("{}\n", owner.display()),
    )
    .unwrap();
    fs::write(temp.join(format!("{name}.sha")), "0123\n").unwrap();
    fs::write(temp.join(format!("{name}.lock")), "").unwrap();
}

fn entry_files(temp: &Path, name: &str) -> Vec<bool> {
    ["", ".last-used", ".sha", ".lock"]
        .iter()
        .map(|suffix| temp.join(format!("{name}{suffix}")).exists())
        .collect()
}

#[test]
fn removes_an_entry_whose_owner_below_the_temp_dir_is_gone() {
    let temp = TempDir::new().unwrap();
    let name = "fallow-audit-base-cache-1111111111111111-root-1111111111111111";
    write_entry(temp.path(), name, &temp.path().join("removed-repo"));

    assert_eq!(remove_orphaned_audit_caches(temp.path()), 1);
    assert_eq!(entry_files(temp.path(), name), [false; 4]);
}

#[test]
fn keeps_an_entry_whose_owner_exists() {
    let temp = TempDir::new().unwrap();
    let owner = temp.path().join("live-repo");
    fs::create_dir_all(&owner).unwrap();
    let name = "fallow-audit-base-cache-2222222222222222-root-2222222222222222";
    write_entry(temp.path(), name, &owner);

    assert_eq!(remove_orphaned_audit_caches(temp.path()), 0);
    assert_eq!(entry_files(temp.path(), name), [true; 4]);
}

#[test]
fn keeps_an_entry_whose_missing_owner_is_outside_the_temp_dir() {
    let temp = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    let name = "fallow-audit-base-cache-3333333333333333-root-3333333333333333";
    write_entry(temp.path(), name, &elsewhere.path().join("removed-repo"));

    assert_eq!(remove_orphaned_audit_caches(temp.path()), 0);
    assert_eq!(entry_files(temp.path(), name), [true; 4]);
}

#[test]
fn keeps_other_files_of_the_temp_dir() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("notes.last-used"),
        temp.path().join("removed-repo").display().to_string(),
    )
    .unwrap();

    assert_eq!(remove_orphaned_audit_caches(temp.path()), 0);
    assert!(temp.path().join("notes.last-used").exists());
}
