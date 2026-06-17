use rustc_hash::FxHashSet;

use super::common::{create_config, fixture_path};

#[test]
fn detects_bun_package_json_catalog_unresolved_references() {
    let root = fixture_path("issue-1301-bun-catalog");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: FxHashSet<(&str, &str)> = results
        .unresolved_catalog_references
        .iter()
        .map(|finding| {
            (
                finding.reference.catalog_name.as_str(),
                finding.reference.entry_name.as_str(),
            )
        })
        .collect();
    let expected = std::iter::once(("doesnotexist", "missing")).collect();
    assert_eq!(
        actual, expected,
        "unexpected Bun unresolved catalog references: {actual:?}",
    );

    let missing = results
        .unresolved_catalog_references
        .iter()
        .find(|finding| finding.reference.entry_name == "missing")
        .expect("missing dependency must be reported");
    assert_eq!(missing.reference.line, 7);
    assert!(
        missing.reference.available_in_catalogs.is_empty(),
        "unknown Bun catalog should not suggest alternatives",
    );
}

#[test]
fn detects_bun_package_json_unused_catalog_entries() {
    let root = fixture_path("issue-1301-bun-catalog");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: FxHashSet<(&str, &str, &std::path::Path)> = results
        .unused_catalog_entries
        .iter()
        .map(|finding| {
            (
                finding.entry.catalog_name.as_str(),
                finding.entry.entry_name.as_str(),
                finding.entry.path.as_path(),
            )
        })
        .collect();
    let expected = [
        (
            "default",
            "unused-default",
            std::path::Path::new("package.json"),
        ),
        (
            "testing",
            "unused-testing",
            std::path::Path::new("package.json"),
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        actual, expected,
        "unexpected Bun unused catalog entries: {actual:?}",
    );

    for consumed in [("default", "bun-types"), ("testing", "vitest")] {
        assert!(
            !results.unused_catalog_entries.iter().any(|finding| {
                finding.entry.catalog_name == consumed.0 && finding.entry.entry_name == consumed.1
            }),
            "consumed Bun catalog entry {consumed:?} must not be reported",
        );
    }
}

#[test]
fn detects_bun_package_json_empty_named_catalog_groups() {
    let root = fixture_path("issue-1301-bun-catalog");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let actual: Vec<_> = results
        .empty_catalog_groups
        .iter()
        .map(|finding| {
            (
                finding.group.catalog_name.as_str(),
                finding.group.path.as_path(),
                finding.group.line,
            )
        })
        .collect();
    assert_eq!(
        actual,
        vec![("empty", std::path::Path::new("package.json"), 15)],
    );
}
