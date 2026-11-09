//! Integration tests for `unplugin-icons` virtual icon imports.

use super::common::{create_config, fixture_path};

#[test]
fn icon_imports_credit_their_icon_sets_and_are_not_unlisted() {
    let root = fixture_path("unplugin-icons-plugin");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        unlisted.is_empty(),
        "an icon import names a virtual module, not a package: {unlisted:?}"
    );
    assert!(
        results.unresolved_imports.is_empty(),
        "an icon import is not an unresolved import: {:?}",
        results.unresolved_imports
    );

    let unused_dev: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    for used in [
        "@iconify-json/lucide",
        "@iconify-json/mdi",
        "@iconify/json",
        "unplugin-icons",
    ] {
        assert!(
            !unused_dev.contains(&used),
            "an icon import uses {used}, unused dev dependencies: {unused_dev:?}"
        );
    }
    assert!(
        unused_dev.contains(&"@iconify-json/tabler"),
        "an icon set that no import names stays unused: {unused_dev:?}"
    );
}
