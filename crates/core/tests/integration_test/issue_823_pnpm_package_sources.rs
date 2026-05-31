//! Issue #823: pnpm package sources must preserve declared dependency names.

use std::path::PathBuf;

use super::common::{create_config, fixture_path};

#[test]
fn pnpm_package_source_catalog_entries_credit_declared_names() {
    let root = fixture_path("issue-823-pnpm-package-sources");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dependencies: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    for dep in [
        "registry-pkg",
        "jsr-pkg",
        "workspace-pkg",
        "local-dir-pkg",
        "local-tarball-pkg",
        "remote-tarball-pkg",
        "git-url-pkg",
        "git-shorthand-pkg",
        "npm-alias-pkg",
    ] {
        assert!(
            !unused_dependencies.contains(&dep),
            "declared dependency {dep} should be credited, got {unused_dependencies:?}"
        );
    }
    assert!(
        unused_dependencies.contains(&"unused-control"),
        "unused control dependency should still be reported, got {unused_dependencies:?}"
    );

    let unlisted_dependencies: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    for dep in [
        "@jsr/scope__jsr-pkg",
        "actual-npm-package",
        "registry-pkg",
        "jsr-pkg",
        "workspace-pkg",
        "local-dir-pkg",
        "local-tarball-pkg",
        "remote-tarball-pkg",
        "git-url-pkg",
        "git-shorthand-pkg",
        "npm-alias-pkg",
    ] {
        assert!(
            !unlisted_dependencies.contains(&dep),
            "{dep} should not be reported as unlisted, got {unlisted_dependencies:?}"
        );
    }
}

#[test]
fn pnpm_package_source_catalog_entries_are_not_reported_unused() {
    let root = fixture_path("issue-823-pnpm-package-sources");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_catalog_entries: Vec<(&str, &str)> = results
        .unused_catalog_entries
        .iter()
        .map(|finding| {
            (
                finding.entry.catalog_name.as_str(),
                finding.entry.entry_name.as_str(),
            )
        })
        .collect();
    for dep in [
        "registry-pkg",
        "jsr-pkg",
        "workspace-pkg",
        "local-dir-pkg",
        "local-tarball-pkg",
        "remote-tarball-pkg",
        "git-url-pkg",
        "git-shorthand-pkg",
        "npm-alias-pkg",
    ] {
        assert!(
            !unused_catalog_entries.contains(&("default", dep)),
            "catalog entry {dep} is consumed and should not be unused, got {unused_catalog_entries:?}"
        );
    }
}

#[test]
fn fixture_uses_workspace_package_source_without_marking_workspace_unused() {
    let root = fixture_path("issue-823-pnpm-package-sources");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<PathBuf> = results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(&root)
                .unwrap_or(&finding.file.path)
                .to_path_buf()
        })
        .collect();
    assert!(
        !unused_files.contains(&PathBuf::from("packages/workspace-pkg/src/index.ts")),
        "workspace source package should be reachable, got {unused_files:?}"
    );
}
