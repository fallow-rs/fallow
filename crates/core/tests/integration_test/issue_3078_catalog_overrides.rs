//! Integration tests for `catalog:` references in pnpm overrides (issue #3078).
//!
//! The fixture under `tests/fixtures/issue-3078-catalog-overrides/` has no
//! dependency that uses the `catalog:` protocol. Every catalog reference is an
//! override value:
//!
//! - `pnpm-workspace.yaml` `overrides`: `is-number` -> `catalog:` (default),
//!   `some-parent>@effect/platform-node-shared` -> `catalog:effect` (named),
//!   `ghost-pkg` -> `catalog:effect` (not declared in `effect`).
//! - root `package.json` `pnpm.overrides`: `legacy-pkg` -> `catalog:legacy`,
//!   `broken-pkg` -> `catalog:missing` (no `missing` catalog).
//!
//! pnpm resolves `catalog:` values in both override locations and fails the
//! install when the catalog does not declare the target package.

use rustc_hash::FxHashSet;

use super::common::{create_config, fixture_path};

#[test]
fn catalog_references_in_overrides_count_as_consumers() {
    let root = fixture_path("issue-3078-catalog-overrides");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: FxHashSet<(&str, &str)> = results
        .unused_catalog_entries
        .iter()
        .map(|e| (e.entry.catalog_name.as_str(), e.entry.entry_name.as_str()))
        .collect();
    let expected: FxHashSet<(&str, &str)> = std::iter::once(("default", "truly-unused")).collect();
    assert_eq!(actual, expected, "unexpected catalog findings: {actual:?}");
}

#[test]
fn named_catalog_reference_in_override_uses_target_package() {
    let root = fixture_path("issue-3078-catalog-overrides");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results.unused_catalog_entries.iter().any(|e| {
            e.entry.catalog_name == "effect" && e.entry.entry_name == "@effect/platform-node-shared"
        }),
        "`catalog:effect` on a parent>child override must consume the child entry",
    );
}

#[test]
fn unresolved_catalog_references_in_overrides_are_reported_with_locations() {
    let root = fixture_path("issue-3078-catalog-overrides");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let strip = |p: &std::path::Path| -> String {
        p.strip_prefix(&config.root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let actual: FxHashSet<(String, String, String, u32)> = results
        .unresolved_catalog_references
        .iter()
        .map(|r| {
            (
                r.reference.catalog_name.clone(),
                r.reference.entry_name.clone(),
                strip(&r.reference.path),
                r.reference.line,
            )
        })
        .collect();
    let expected: FxHashSet<(String, String, String, u32)> = [
        (
            "effect".to_string(),
            "ghost-pkg".to_string(),
            "pnpm-workspace.yaml".to_string(),
            17,
        ),
        (
            "missing".to_string(),
            "broken-pkg".to_string(),
            "package.json".to_string(),
            9,
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        actual, expected,
        "unexpected unresolved-catalog-reference findings: {actual:?}",
    );
}

type UnusedPairs = FxHashSet<(String, String)>;
type UnresolvedTuples = FxHashSet<(String, String, String, u32)>;

/// Catalog findings of a fixture: `(catalog, entry)` pairs for unused entries
/// and `(catalog, entry, file, line)` tuples for unresolved references.
fn catalog_findings(fixture: &str) -> (UnusedPairs, UnresolvedTuples) {
    let config = create_config(fixture_path(fixture));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = results
        .unused_catalog_entries
        .iter()
        .map(|e| (e.entry.catalog_name.clone(), e.entry.entry_name.clone()))
        .collect();
    let unresolved = results
        .unresolved_catalog_references
        .iter()
        .map(|r| {
            let path = r
                .reference
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&r.reference.path)
                .to_string_lossy()
                .replace('\\', "/");
            (
                r.reference.catalog_name.clone(),
                r.reference.entry_name.clone(),
                path,
                r.reference.line,
            )
        })
        .collect();
    (unused, unresolved)
}

fn unused_pairs(items: &[(&str, &str)]) -> UnusedPairs {
    items
        .iter()
        .map(|(catalog, entry)| ((*catalog).to_string(), (*entry).to_string()))
        .collect()
}

fn unresolved_tuples(file: &str, items: &[(&str, &str, u32)]) -> UnresolvedTuples {
    items
        .iter()
        .map(|(catalog, entry, line)| {
            (
                (*catalog).to_string(),
                (*entry).to_string(),
                file.to_string(),
                *line,
            )
        })
        .collect()
}

/// pnpm 11 and later do not read the `pnpm` field or `resolutions` in
/// `package.json`. A stale `catalog:` value there does not fail the install
/// and does not consume a catalog entry. The `pnpm-workspace.yaml` overrides
/// still apply.
#[test]
fn pnpm_11_ignores_package_json_override_sources() {
    let (unused, unresolved) = catalog_findings("issue-3078-catalog-overrides-pnpm11");

    assert_eq!(
        unused,
        unused_pairs(&[("default", "is-number"), ("legacy", "legacy-pkg")]),
        "package.json overrides must not consume catalog entries on pnpm 11",
    );
    assert_eq!(
        unresolved,
        unresolved_tuples("pnpm-workspace.yaml", &[("legacy", "ghost-pkg", 14)]),
        "only the pnpm-workspace.yaml override can be unresolved on pnpm 11",
    );
}

/// pnpm 10 reads `pnpm.overrides` and the top-level `resolutions` of the
/// root `package.json`. When a key is in both, `pnpm.overrides` wins.
#[test]
fn pnpm_10_reads_pnpm_overrides_and_resolutions() {
    let (unused, unresolved) = catalog_findings("issue-3078-catalog-overrides-pnpm10");

    assert_eq!(
        unused,
        unused_pairs(&[("default", "truly-unused")]),
        "`resolutions` and `pnpm.overrides` must consume catalog entries on pnpm 10",
    );
    assert_eq!(
        unresolved,
        unresolved_tuples(
            "package.json",
            &[("legacy", "ghost-pkg", 9), ("missing", "broken-pkg", 15)],
        ),
        "unexpected unresolved-catalog-reference findings on pnpm 10",
    );
}
