#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Guards the crates.io publish list in `.github/workflows/release.yml` against
//! the set of publishable workspace crates.
//!
//! The `publish-crates` job iterates a HARDCODED `for crate in ...; do` list.
//! When a new publishable crate joins the workspace (or one is retired / marked
//! `publish = false`) without updating that list, a release silently stops
//! publishing at the first crate whose dependency is missing from the index.
//! v2.103.0 hit exactly this: the new `fallow-output` / `fallow-engine` /
//! `fallow-api` crates were absent from the list, so the chain broke right
//! after `fallow-types` and the rest of the workspace never reached crates.io.
//!
//! This test fails loudly the moment the list and the publishable set diverge.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Repo root: `crates/cli` is two directories below it.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cli sits two levels below the repo root")
        .to_path_buf()
}

/// Package name from a `Cargo.toml`'s first `name = "..."` line (the `[package]`
/// table precedes any `[[bin]]`/`[lib]` table in every workspace manifest).
fn package_name(manifest: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("name")?.trim_start();
        let value = rest.strip_prefix('=')?.trim();
        Some(value.trim_matches('"').to_string())
    })
}

/// `true` when a manifest sets `publish = false` (the key only ever appears in
/// the `[package]` table).
fn is_publish_false(manifest: &str) -> bool {
    manifest.lines().any(|line| {
        let t = line.trim();
        t.starts_with("publish") && t.contains("false")
    })
}

/// Workspace crates that crates.io would accept (`publish` not set to `false`).
/// `members = ["crates/*"]`, so every workspace crate is a `crates/<name>` dir.
fn publishable_crates(root: &Path) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for entry in fs::read_dir(root.join("crates")).expect("read crates/ dir") {
        let manifest_path = entry.expect("crates/ dir entry").path().join("Cargo.toml");
        let Ok(manifest) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        if is_publish_false(&manifest) {
            continue;
        }
        set.insert(package_name(&manifest).unwrap_or_else(|| {
            panic!("no package name in {}", manifest_path.display());
        }));
    }
    set
}

/// Crate names from the `for crate in ...; do` publish loop in `release.yml`.
fn release_publish_list(root: &Path) -> BTreeSet<String> {
    release_publish_sequence(root).into_iter().collect()
}

fn release_publish_sequence(root: &Path) -> Vec<String> {
    let yml = fs::read_to_string(root.join(".github/workflows/release.yml"))
        .expect("read .github/workflows/release.yml");
    yml.lines()
        .find_map(|line| {
            let spec = line
                .trim()
                .strip_prefix("for crate in ")?
                .split(';')
                .next()?;
            Some(spec.split_whitespace().map(String::from).collect())
        })
        .unwrap_or_default()
}

fn local_publishable_dependencies(
    manifest: &str,
    publishable: &BTreeSet<String>,
) -> BTreeSet<String> {
    manifest
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("fallow-") {
                return None;
            }
            let (name, _) = trimmed.split_once('=')?;
            let name = name.trim();
            publishable.contains(name).then(|| name.to_string())
        })
        .collect()
}

#[test]
fn release_publish_list_matches_publishable_workspace_crates() {
    let root = repo_root();
    let listed = release_publish_list(&root);
    let publishable = publishable_crates(&root);

    assert!(
        !listed.is_empty(),
        "could not parse the `for crate in ...; do` publish loop out of \
         .github/workflows/release.yml; the loop format may have changed"
    );

    let missing_from_list: Vec<&String> = publishable.difference(&listed).collect();
    let extra_in_list: Vec<&String> = listed.difference(&publishable).collect();
    assert!(
        missing_from_list.is_empty() && extra_in_list.is_empty(),
        "release.yml crates.io publish list has drifted from the workspace's \
         publishable crates.\n  publishable but NOT in the list (add them in \
         dependency order): {missing_from_list:?}\n  listed but NOT publishable \
         (drop them, or remove `publish = false`): {extra_in_list:?}"
    );
}

#[test]
fn release_publish_list_orders_workspace_dependencies_first() {
    let root = repo_root();
    let publishable = publishable_crates(&root);
    let sequence = release_publish_sequence(&root);
    assert!(
        !sequence.is_empty(),
        "could not parse the `for crate in ...; do` publish loop out of \
         .github/workflows/release.yml; the loop format may have changed"
    );

    for entry in fs::read_dir(root.join("crates")).expect("read crates/ dir") {
        let manifest_path = entry.expect("crates/ dir entry").path().join("Cargo.toml");
        let Ok(manifest) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        if is_publish_false(&manifest) {
            continue;
        }
        let crate_name = package_name(&manifest).unwrap_or_else(|| {
            panic!("no package name in {}", manifest_path.display());
        });
        let Some(crate_index) = sequence.iter().position(|name| name == &crate_name) else {
            continue;
        };
        for dependency in local_publishable_dependencies(&manifest, &publishable) {
            let dependency_index = sequence
                .iter()
                .position(|name| name == &dependency)
                .unwrap_or_else(|| {
                    panic!("{crate_name} depends on publishable crate {dependency}, but it is missing from release.yml")
                });
            assert!(
                dependency_index < crate_index,
                "{crate_name} depends on {dependency}, so release.yml must publish {dependency} first"
            );
        }
    }
}
