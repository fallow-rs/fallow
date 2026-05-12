use super::common::fixture_path;
use fallow_config::{FallowConfig, OutputFormat, RulesConfig};

fn create_production_config(root: std::path::PathBuf) -> fallow_config::ResolvedConfig {
    FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        duplicates: fallow_config::DuplicatesConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: true.into(),
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: fallow_config::FlagsConfig::default(),
        resolve: fallow_config::ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
    }
    .resolve(root, OutputFormat::Human, 4, true, true)
}

#[test]
fn type_only_import_detected_in_production_mode() {
    let root = fixture_path("type-only-deps");
    let config = create_production_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let type_only_names: Vec<&str> = results
        .type_only_dependencies
        .iter()
        .map(|d| d.package_name.as_str())
        .collect();

    // zod is only imported via `import type`, so it should be type-only
    assert!(
        type_only_names.contains(&"zod"),
        "zod should be detected as type-only dependency, found: {type_only_names:?}"
    );

    // express has a runtime import, should NOT be type-only
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

    // type_only_dependencies is only populated in production mode
    assert!(
        results.type_only_dependencies.is_empty(),
        "type_only_dependencies should be empty outside production mode, found: {:?}",
        results
            .type_only_dependencies
            .iter()
            .map(|d| d.package_name.as_str())
            .collect::<Vec<_>>()
    );
}
