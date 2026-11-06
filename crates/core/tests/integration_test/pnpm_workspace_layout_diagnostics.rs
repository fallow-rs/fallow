use std::path::Path;

use fallow_config::WorkspaceDiagnosticKind;

use super::common::{create_config, fixture_path};

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// A pnpm workspace that declares cloned directories (`vite`,
/// `vite/packages/*`) that are missing, a nested independent project with its
/// own `pnpm-workspace.yaml` (`docs`), and a `bench` manifest that only sets
/// the module type.
#[test]
fn layout_diagnostics_replace_false_workspace_and_catalog_findings() {
    let root = fixture_path("pnpm-workspace-layout-diagnostics");
    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis succeeds");

    assert!(
        results.unused_catalog_entries.is_empty(),
        "a missing pattern directory can hold catalog consumers: {:?}",
        results.unused_catalog_entries
    );
    assert!(
        results.unresolved_catalog_references.is_empty(),
        "the plain `vite` override replaces the `catalog:` reference: {:?}",
        results.unresolved_catalog_references
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| relative(&root, &finding.file.path))
        .collect();
    assert!(
        !unused_files.iter().any(|path| path == "docs/src/index.ts"),
        "the docs manifest owns its entry point: {unused_files:?}"
    );
    assert!(
        results
            .unlisted_dependencies
            .iter()
            .all(|finding| finding.dep.package_name != "vue"),
        "the docs manifest declares vue"
    );

    let mut layout: Vec<(String, String, Option<String>)> =
        fallow_config::workspace_diagnostics_for(&root)
            .into_iter()
            .filter_map(|diagnostic| {
                let pattern = match &diagnostic.kind {
                    WorkspaceDiagnosticKind::WorkspacePatternDirMissing { pattern } => {
                        Some(pattern.clone())
                    }
                    WorkspaceDiagnosticKind::NestedProjectRoot
                    | WorkspaceDiagnosticKind::UndeclaredWorkspace => None,
                    _ => return None,
                };
                Some((
                    diagnostic.kind.id().to_owned(),
                    relative(&root, &diagnostic.path),
                    pattern,
                ))
            })
            .collect();
    layout.sort();
    assert_eq!(
        layout,
        [
            ("nested-project-root".to_owned(), "docs".to_owned(), None),
            (
                "workspace-pattern-dir-missing".to_owned(),
                "pnpm-workspace.yaml".to_owned(),
                Some("vite".to_owned())
            ),
            (
                "workspace-pattern-dir-missing".to_owned(),
                "pnpm-workspace.yaml".to_owned(),
                Some("vite/packages/*".to_owned())
            ),
        ]
    );
}

fn unresolved_catalog_names(fixture: &str) -> Vec<String> {
    let root = fixture_path(fixture);
    let results = fallow_core::analyze(&create_config(root)).expect("analysis succeeds");
    let mut names: Vec<String> = results
        .unresolved_catalog_references
        .iter()
        .map(|finding| finding.reference.entry_name.clone())
        .collect();
    names.sort();
    names
}

/// A plain override key and a `<consumer>>tool` key replace the `catalog:`
/// reference before pnpm resolves it. A key with a version selector does not.
#[test]
fn pnpm_workspace_overrides_take_precedence_over_catalog_references() {
    assert_eq!(
        unresolved_catalog_names("pnpm-catalog-override-precedence"),
        ["rolldown"]
    );
}

/// The members of a nested independent pnpm project are workspaces. Their
/// `catalog:` references resolve against the nearest `pnpm-workspace.yaml`,
/// and the catalog findings of the nested project name its own files. The
/// root catalog checks see only the root workspaces.
#[test]
fn nested_pnpm_project_members_use_the_nested_catalog() {
    let root = fixture_path("pnpm-nested-workspace-members");
    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis succeeds");

    let mut unused: Vec<(String, String, String)> = results
        .unused_catalog_entries
        .iter()
        .map(|finding| {
            (
                relative(&root, &finding.entry.path),
                finding.entry.catalog_name.clone(),
                finding.entry.entry_name.clone(),
            )
        })
        .collect();
    unused.sort();
    assert_eq!(
        unused,
        [
            (
                "pnpm-workspace.yaml".to_owned(),
                "default".to_owned(),
                "vue".to_owned()
            ),
            (
                "site/pnpm-workspace.yaml".to_owned(),
                "default".to_owned(),
                "unused-in-site".to_owned()
            ),
        ]
    );

    let mut unresolved: Vec<(String, String, String)> = results
        .unresolved_catalog_references
        .iter()
        .map(|finding| {
            (
                relative(&root, &finding.reference.path),
                finding.reference.catalog_name.clone(),
                finding.reference.entry_name.clone(),
            )
        })
        .collect();
    unresolved.sort();
    assert_eq!(
        unresolved,
        [
            (
                "site/packages/web/package.json".to_owned(),
                "default".to_owned(),
                "react".to_owned()
            ),
            (
                "site/packages/web/package.json".to_owned(),
                "legacy".to_owned(),
                "pinia".to_owned()
            ),
        ]
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| relative(&root, &finding.file.path))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "site/packages/web/src/index.ts"),
        "the nested member manifest owns its entry point: {unused_files:?}"
    );
}

/// pnpm 10 reads `pnpm.overrides` in `package.json`, with the same rules.
#[test]
fn package_json_overrides_take_precedence_for_pnpm_10() {
    assert_eq!(
        unresolved_catalog_names("pnpm-catalog-override-precedence-pnpm10"),
        ["rolldown"]
    );
}
