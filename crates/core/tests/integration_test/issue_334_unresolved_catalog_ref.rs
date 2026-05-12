//! Integration tests for unresolved-catalog-reference detection (issue #334).
//!
//! The fixture under `tests/fixtures/issue-334-unresolved-catalog-ref/` declares
//! a default catalog (with `is-even` only) and two named catalogs (`react17`
//! with react+react-dom, `react18` with react+react-dom+old-react). The
//! consumer `package.json` files exercise:
//!
//! - **Valid**: `react` -> `catalog:react17`, `is-even` -> `catalog:`,
//!   `react` / `react-dom` -> `catalog:react18`.
//! - **Unresolved with available_in_catalogs**: `old-react` -> `catalog:react17`
//!   (react18 declares it).
//! - **Unresolved with empty available_in_catalogs**: `missing-pkg` ->
//!   `catalog:` (no catalog has it), `future-dep` -> `catalog:upcoming`
//!   (no `upcoming` catalog exists at all).

use std::path::PathBuf;

use fallow_config::{
    FallowConfig, IgnoreCatalogReferenceRule, OutputFormat, RulesConfig, Severity,
};
use rustc_hash::FxHashSet;

use super::common::fixture_path;

fn config_for_fixture(
    root: PathBuf,
    ignore: Vec<IgnoreCatalogReferenceRule>,
) -> fallow_config::ResolvedConfig {
    FallowConfig {
        ignore_catalog_references: ignore,
        ..Default::default()
    }
    .resolve(root, OutputFormat::Human, 4, true, true)
}

#[test]
fn detects_unresolved_named_and_default_catalog_references() {
    let root = fixture_path("issue-334-unresolved-catalog-ref");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Strip the project root so the test asserts on the project-root-relative
    // form regardless of where the fixture lives on disk. The path is stored
    // as absolute internally (matching the convention for path-anchored
    // findings); serde_path::serialize strips the root for JSON output.
    let strip = |p: &std::path::Path| -> String {
        p.strip_prefix(&config.root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let actual: FxHashSet<(&str, &str, String)> = results
        .unresolved_catalog_references
        .iter()
        .map(|r| {
            (
                r.catalog_name.as_str(),
                r.entry_name.as_str(),
                strip(&r.path),
            )
        })
        .collect();

    let expected: FxHashSet<(&str, &str, String)> = [
        ("react17", "old-react", "packages/app/package.json".into()),
        ("default", "missing-pkg", "packages/app/package.json".into()),
        (
            "upcoming",
            "future-dep",
            "packages/placeholder/package.json".into(),
        ),
    ]
    .into_iter()
    .collect();

    assert_eq!(
        actual, expected,
        "unexpected unresolved-catalog-reference findings: {actual:?}",
    );

    // No false positives for the four valid references.
    let valid_references = [
        ("react17", "react"),
        ("default", "is-even"),
        ("react18", "react"),
        ("react18", "react-dom"),
    ];
    for (cat, pkg) in valid_references {
        assert!(
            !results
                .unresolved_catalog_references
                .iter()
                .any(|r| r.catalog_name == cat && r.entry_name == pkg),
            "valid reference (catalog={cat}, pkg={pkg}) must not be flagged",
        );
    }
}

#[test]
fn unresolved_findings_carry_line_numbers_and_available_alternatives() {
    let root = fixture_path("issue-334-unresolved-catalog-ref");
    let config = config_for_fixture(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let old_react = results
        .unresolved_catalog_references
        .iter()
        .find(|r| r.entry_name == "old-react")
        .expect("old-react finding must be present");
    // Line 7 in packages/app/package.json (1-based).
    assert_eq!(old_react.line, 7, "old-react line, got {}", old_react.line);
    // react18 declares old-react, so it should appear in the alternatives.
    assert!(
        old_react.available_in_catalogs.contains(&"react18".into()),
        "old-react.available_in_catalogs missing react18, got {:?}",
        old_react.available_in_catalogs,
    );

    let missing_pkg = results
        .unresolved_catalog_references
        .iter()
        .find(|r| r.entry_name == "missing-pkg")
        .expect("missing-pkg finding must be present");
    assert!(
        missing_pkg.available_in_catalogs.is_empty(),
        "missing-pkg has no other catalog declaring it; available_in_catalogs must be empty",
    );

    let future_dep = results
        .unresolved_catalog_references
        .iter()
        .find(|r| r.entry_name == "future-dep")
        .expect("future-dep finding must be present");
    assert!(
        future_dep.available_in_catalogs.is_empty(),
        "future-dep references a catalog that does not exist; no alternatives expected",
    );
}

#[test]
fn ignore_catalog_references_filters_by_package_and_catalog_and_consumer() {
    let root = fixture_path("issue-334-unresolved-catalog-ref");

    // Suppress only `future-dep` in `catalog:upcoming` for the placeholder
    // workspace. The other two findings (old-react, missing-pkg) must remain.
    let ignore = vec![IgnoreCatalogReferenceRule {
        package: "future-dep".to_string(),
        catalog: Some("upcoming".to_string()),
        consumer: Some("packages/placeholder/package.json".to_string()),
    }];
    let config = config_for_fixture(root.clone(), ignore);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .unresolved_catalog_references
            .iter()
            .any(|r| r.entry_name == "future-dep"),
        "future-dep must be suppressed by ignoreCatalogReferences",
    );
    assert!(
        results
            .unresolved_catalog_references
            .iter()
            .any(|r| r.entry_name == "old-react"),
        "unrelated finding old-react must NOT be suppressed",
    );

    // Consumer glob with a wildcard suppresses everything under that subtree.
    let glob_ignore = vec![IgnoreCatalogReferenceRule {
        package: "old-react".to_string(),
        catalog: None,
        consumer: Some("packages/**/package.json".to_string()),
    }];
    let config = config_for_fixture(root, glob_ignore);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        !results
            .unresolved_catalog_references
            .iter()
            .any(|r| r.entry_name == "old-react"),
        "consumer glob must match against packages/**/package.json",
    );
}

#[test]
fn detector_skipped_when_severity_is_off() {
    let root = fixture_path("issue-334-unresolved-catalog-ref");
    let rules = RulesConfig {
        unresolved_catalog_references: Severity::Off,
        ..RulesConfig::default()
    };
    let config = FallowConfig {
        rules,
        ..Default::default()
    }
    .resolve(root, OutputFormat::Human, 4, true, true);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.unresolved_catalog_references.is_empty(),
        "severity off must short-circuit the detector",
    );
}
