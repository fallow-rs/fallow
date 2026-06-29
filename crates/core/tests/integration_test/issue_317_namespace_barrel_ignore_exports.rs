//! Issue #317: ignoreExports must also gate namespace-barrel duplicate exports.

use fallow_config::{
    FallowConfig, IgnoreExportRule, IgnoreExportsUsedInFileConfig, OutputFormat, RulesConfig,
};

use crate::common::fixture_path;

fn make_config(
    root: std::path::PathBuf,
    ignore_exports: Vec<IgnoreExportRule>,
) -> fallow_config::ResolvedConfig {
    FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports,
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        unused_component_props: fallow_config::UnusedComponentPropsConfig::default(),
        duplicates: fallow_config::DuplicatesConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: false.into(),
        plugins: vec![],
        rule_packs: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: fallow_config::FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: fallow_config::ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None)
}

#[test]
fn duplicate_exports_flagged_without_ignore_exports() {
    let root = fixture_path("issue-317-namespace-barrel-ignore-exports");
    let config = make_config(root, vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let dupe_names: Vec<&str> = results
        .duplicate_exports
        .iter()
        .map(|d| d.export.export_name.as_str())
        .collect();
    assert!(
        dupe_names.contains(&"Root"),
        "without ignoreExports, Root should surface as duplicate. got: {dupe_names:?}"
    );
    assert!(
        dupe_names.contains(&"Content"),
        "without ignoreExports, Content should surface as duplicate. got: {dupe_names:?}"
    );
    assert!(
        dupe_names.contains(&"Trigger"),
        "without ignoreExports, Trigger should surface as duplicate. got: {dupe_names:?}"
    );
}

#[test]
fn ignore_exports_wildcard_clears_duplicate_exports() {
    let root = fixture_path("issue-317-namespace-barrel-ignore-exports");
    let config = make_config(
        root,
        vec![IgnoreExportRule {
            file: "**/components/ui/**".to_owned(),
            exports: vec!["*".to_owned()],
        }],
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.duplicate_exports.is_empty(),
        "wildcard ignoreExports on the barrel glob must clear all groups, got: {:?}",
        results
            .duplicate_exports
            .iter()
            .map(|d| &d.export.export_name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn ignore_exports_named_clears_only_listed_names() {
    let root = fixture_path("issue-317-namespace-barrel-ignore-exports");
    let config = make_config(
        root,
        vec![IgnoreExportRule {
            file: "**/components/ui/**".to_owned(),
            exports: vec!["Root".to_owned()],
        }],
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let dupe_names: Vec<&str> = results
        .duplicate_exports
        .iter()
        .map(|d| d.export.export_name.as_str())
        .collect();
    assert!(
        !dupe_names.contains(&"Root"),
        "Root listed in ignoreExports must not surface, got: {dupe_names:?}"
    );
    assert!(
        dupe_names.contains(&"Content"),
        "Content not in the ignore list must still surface, got: {dupe_names:?}"
    );
    assert!(
        dupe_names.contains(&"Trigger"),
        "Trigger not in the ignore list must still surface, got: {dupe_names:?}"
    );
}
