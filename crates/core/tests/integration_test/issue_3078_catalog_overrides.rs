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
