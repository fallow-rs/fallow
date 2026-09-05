use std::path::PathBuf;

use rustc_hash::FxHashSet;

use super::common::{create_config, fixture_path};

#[test]
fn detects_unused_default_and_named_catalog_entries() {
    let root = fixture_path("issue-329-pnpm-catalog");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut expected: FxHashSet<(&str, &str)> = FxHashSet::default();
    for entry in [
        ("default", "is-even"),
        ("default", "hardcoded-pkg"),
        ("react17", "react"),
        ("react17", "react-dom"),
    ] {
        expected.insert(entry);
    }
    let actual: FxHashSet<(&str, &str)> = results
        .unused_catalog_entries
        .iter()
        .map(|e| (e.entry.catalog_name.as_str(), e.entry.entry_name.as_str()))
        .collect();
    assert_eq!(actual, expected, "unexpected catalog findings: {actual:?}");

    assert!(
        !results
            .unused_catalog_entries
            .iter()
            .any(|e| e.entry.catalog_name == "default" && e.entry.entry_name == "react"),
        "default catalog 'react' is referenced by both consumers and must not be flagged"
    );

    assert!(
        !results
            .unused_catalog_entries
            .iter()
            .any(|e| e.entry.catalog_name == "legacy"),
        "legacy catalog entries are all referenced and must not appear"
    );
}

#[test]
fn hardcoded_consumers_are_surfaced() {
    let root = fixture_path("issue-329-pnpm-catalog");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let hardcoded = results
        .unused_catalog_entries
        .iter()
        .find(|e| e.entry.entry_name == "hardcoded-pkg")
        .expect("hardcoded-pkg should be reported as unused");

    let consumer_paths: Vec<String> = hardcoded
        .entry
        .hardcoded_consumers
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(consumer_paths, vec!["packages/app/package.json"]);
}

#[test]
fn catalog_entries_are_sorted_default_first() {
    let root = fixture_path("issue-329-pnpm-catalog");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names: Vec<&str> = results
        .unused_catalog_entries
        .iter()
        .map(|e| e.entry.catalog_name.as_str())
        .collect();
    let first_named = names.iter().position(|n| *n != "default");
    if let Some(idx) = first_named {
        assert!(
            names[..idx].iter().all(|n| *n == "default"),
            "default catalog entries should come first, got: {names:?}"
        );
    }
}

#[test]
fn path_is_relative_pnpm_workspace_yaml() {
    let root = fixture_path("issue-329-pnpm-catalog");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    for entry in &results.unused_catalog_entries {
        let entry = &entry.entry;
        assert_eq!(
            entry.path,
            PathBuf::from("pnpm-workspace.yaml"),
            "catalog entry path should be relative pnpm-workspace.yaml, got {:?}",
            entry.path
        );
        assert!(entry.line > 0, "catalog entry line must be 1-based");
    }
}

#[test]
fn yaml_catalog_next_line_suppressions_preserve_unrelated_entries() {
    let root = fixture_path("issue-2548-catalog-suppression");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let actual: FxHashSet<(&str, &str)> = results
        .unused_catalog_entries
        .iter()
        .map(|entry| {
            (
                entry.entry.catalog_name.as_str(),
                entry.entry.entry_name.as_str(),
            )
        })
        .collect();
    let expected = FxHashSet::from_iter([
        ("default", "retained-neighbor"),
        ("default", "wrong-rule"),
        ("default", "separated"),
        ("default", "directive-value"),
        ("default", "after-value"),
        ("default", "block-value"),
        ("default", "after-block"),
        ("default", "file-marker"),
        ("default", "unknown-rule"),
        ("future", "retained-neighbor"),
        ("current", "@sveltejs/adapter-static"),
    ]);
    assert_eq!(
        actual, expected,
        "suppressions must target only their entry"
    );
}
