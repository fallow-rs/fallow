use super::common::{create_config, fixture_path};

#[test]
fn public_tag_prevents_unused_export_detection() {
    let root = fixture_path("visibility-tags");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // @public tagged export should NOT be reported as unused
    assert!(
        !unused_export_names.contains(&"publicExport"),
        "publicExport should be exempt via @public tag, unused exports: {unused_export_names:?}"
    );

    // @internal tagged export should NOT be reported as unused
    assert!(
        !unused_export_names.contains(&"internalExport"),
        "internalExport should be exempt via @internal tag, unused exports: {unused_export_names:?}"
    );

    // @beta tagged export should NOT be reported as unused
    assert!(
        !unused_export_names.contains(&"betaExport"),
        "betaExport should be exempt via @beta tag, unused exports: {unused_export_names:?}"
    );

    // @alpha tagged export should NOT be reported as unused
    assert!(
        !unused_export_names.contains(&"alphaExport"),
        "alphaExport should be exempt via @alpha tag, unused exports: {unused_export_names:?}"
    );
}

#[test]
fn untagged_unused_export_still_detected() {
    let root = fixture_path("visibility-tags");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // trulyUnused has no visibility tag and is never imported
    assert!(
        unused_export_names.contains(&"trulyUnused"),
        "trulyUnused should be reported as unused export, found: {unused_export_names:?}"
    );

    // usedExport is actually imported by index.ts, so it should NOT be unused
    assert!(
        !unused_export_names.contains(&"usedExport"),
        "usedExport should not be unused (it is imported), found: {unused_export_names:?}"
    );
}
