//! SFC markup asset references (`<img src="./logo.png">`) participate in
//! `unresolved-import`: a genuinely-missing relative asset surfaces, while an
//! existing one resolves to `ExternalFile` (no finding), and dynamic / aliased /
//! custom-component-prop forms abstain.

use super::common::{create_config, fixture_path};

#[test]
fn flags_missing_template_asset_but_not_existing_or_dynamic() {
    let root = fixture_path("sfc-template-asset");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|f| f.import.specifier.as_str())
        .collect();

    // The genuinely-missing asset is flagged.
    assert!(
        specifiers.contains(&"./missing.png"),
        "a missing template asset should surface as unresolved-import: {specifiers:?}"
    );
    // The existing asset (src/hero.png on disk) resolves to ExternalFile.
    assert!(
        !specifiers.contains(&"./hero.png"),
        "an existing template asset must not be flagged: {specifiers:?}"
    );
    // Dynamic `:src` binding and the custom-component `src` prop abstain.
    assert!(
        !specifiers.iter().any(|s| s.contains("whatever.png")),
        "a dynamic :src binding must abstain: {specifiers:?}"
    );
    assert!(
        !specifiers.iter().any(|s| s.contains("also-not-an-asset")),
        "a custom-component src prop must abstain: {specifiers:?}"
    );
}
