use super::common::{create_production_config, fixture_path};

#[test]
fn type_only_import_detected_in_production_mode() {
    let root = fixture_path("type-only-deps");
    let config = create_production_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let type_only_names: Vec<&str> = results
        .type_only_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        type_only_names.contains(&"zod"),
        "zod should be detected as type-only dependency, found: {type_only_names:?}"
    );

    assert!(
        !type_only_names.contains(&"express"),
        "express should NOT be type-only (has runtime import), found: {type_only_names:?}"
    );
}

#[test]
fn type_only_deps_not_reported_outside_production_mode() {
    let root = fixture_path("type-only-deps");
    let config = super::common::create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.type_only_dependencies.is_empty(),
        "type_only_dependencies should be empty outside production mode, found: {:?}",
        results
            .type_only_dependencies
            .iter()
            .map(|d| d.dep.package_name.as_str())
            .collect::<Vec<_>>()
    );
}
