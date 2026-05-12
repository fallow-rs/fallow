use super::common::{create_config, fixture_path};
use fallow_config::{FallowConfig, OutputFormat, RulesConfig};

// ── Rules "off" disables detection ─────────────────────────────

#[test]
fn rules_off_disables_unused_files() {
    let root = fixture_path("detect-config");
    let mut config = create_config(root);
    config.rules.unused_files = fallow_config::Severity::Off;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_files.is_empty(),
        "unused files should be empty when rule is off"
    );
}

#[test]
fn rules_off_disables_unused_exports() {
    let root = fixture_path("detect-config");
    let mut config = create_config(root);
    config.rules.unused_exports = fallow_config::Severity::Off;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_exports.is_empty(),
        "unused exports should be empty when rule is off"
    );
}

#[test]
fn rules_off_disables_unused_types() {
    let root = fixture_path("detect-config");
    let mut config = create_config(root);
    config.rules.unused_types = fallow_config::Severity::Off;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_types.is_empty(),
        "unused types should be empty when rule is off"
    );
}

#[test]
fn rules_off_disables_unused_dependencies() {
    let root = fixture_path("detect-config");
    let mut config = create_config(root);
    config.rules.unused_dependencies = fallow_config::Severity::Off;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_dependencies.is_empty(),
        "unused dependencies should be empty when rule is off"
    );
}

#[test]
fn rules_off_disables_duplicate_exports() {
    let root = fixture_path("duplicate-exports");
    let mut config = create_config(root);
    config.rules.duplicate_exports = fallow_config::Severity::Off;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.duplicate_exports.is_empty(),
        "duplicate exports should be empty when rule is off"
    );
}

// ── Ignore exports ─────────────────────────────────────────────

#[test]
fn ignore_exports_wildcard() {
    let root = fixture_path("ignore-exports");
    let config = FallowConfig {
        ignore_catalog_references: vec![],
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_exports: vec![fallow_config::IgnoreExportRule {
            file: "src/utils.ts".to_string(),
            exports: vec!["*".to_string()],
        }],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        duplicates: fallow_config::DuplicatesConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: false.into(),
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
    .resolve(root, OutputFormat::Human, 4, true, true);

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"ignored"),
        "ignored should not appear when wildcard ignore is set"
    );
    assert!(
        !unused_export_names.contains(&"notIgnored"),
        "notIgnored should also be ignored by wildcard"
    );
}

#[test]
fn ignore_exports_specific() {
    let root = fixture_path("ignore-exports");
    let config = FallowConfig {
        ignore_catalog_references: vec![],
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_exports: vec![fallow_config::IgnoreExportRule {
            file: "src/utils.ts".to_string(),
            exports: vec!["ignored".to_string()],
        }],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        duplicates: fallow_config::DuplicatesConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: false.into(),
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
    .resolve(root, OutputFormat::Human, 4, true, true);

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"ignored"),
        "ignored should not appear when specifically ignored"
    );
    assert!(
        unused_export_names.contains(&"notIgnored"),
        "notIgnored should still be reported, found: {unused_export_names:?}"
    );
}

#[test]
fn exports_used_only_in_file_are_reported_by_default() {
    let root = fixture_path("ignore-exports-used-in-file");
    let config = create_config(root);

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results
            .unused_exports
            .iter()
            .any(|e| e.export_name == "usedOnlyHere"),
        "same-file references should not suppress unused exports by default"
    );
    assert!(
        results
            .unused_types
            .iter()
            .any(|e| e.export_name == "LocallyUsedType"),
        "same-file references should not suppress unused types by default"
    );
}

#[test]
fn ignore_exports_used_in_file_boolean_suppresses_local_references() {
    let root = fixture_path("ignore-exports-used-in-file");
    let mut config = create_config(root);
    config.ignore_exports_used_in_file = true.into();

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        !unused_export_names.contains(&"usedOnlyHere"),
        "usedOnlyHere is referenced by publicApi and should be suppressed"
    );
    assert!(
        unused_export_names.contains(&"completelyUnused"),
        "completelyUnused has no references and should still be reported"
    );

    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        !unused_type_names.contains(&"LocallyUsedType"),
        "LocallyUsedType is referenced by LocalConsumer and should be suppressed"
    );
    assert!(
        unused_type_names.contains(&"DeadType"),
        "DeadType has no references and should still be reported"
    );
}

#[test]
fn ignore_exports_used_in_file_kind_form_can_target_types_only() {
    let root = fixture_path("ignore-exports-used-in-file");
    let mut config = create_config(root);
    config.ignore_exports_used_in_file = fallow_config::IgnoreExportsUsedInFileByKind {
        type_: true,
        interface: false,
    }
    .into();

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results
            .unused_exports
            .iter()
            .any(|e| e.export_name == "usedOnlyHere"),
        "kind form should not suppress value exports"
    );
    assert!(
        !results
            .unused_types
            .iter()
            .any(|e| e.export_name == "LocallyUsedType"),
        "kind form should suppress type exports referenced in the same file"
    );
}

#[test]
fn ignore_exports_used_in_file_does_not_suppress_export_specifier_self_references() {
    // Regression: `function foo() {}; export { foo };` and
    // `export default foo;` reference the binding only at the export site.
    // The export specifier identifier is not a same-file *use*, so these
    // exports must still be reported when ignoreExportsUsedInFile is on.
    let root = fixture_path("ignore-exports-used-in-file");
    let mut config = create_config(root);
    config.ignore_exports_used_in_file = true.into();

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"specifierOnlyExport"),
        "export {{ specifierOnlyExport }} must still be flagged \
         when no real same-file use exists, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"aliasedSpecifierExportAlias"),
        "export {{ x as y }} must still be flagged when no real same-file \
         use exists, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"default"),
        "export default <identifier> must still be flagged when no real \
         same-file use exists, found: {unused_export_names:?}"
    );
}

// ── Ignore dependencies ────────────────────────────────────────

#[test]
fn ignore_dependencies_config() {
    let root = fixture_path("basic-project");
    let config = FallowConfig {
        ignore_catalog_references: vec![],
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec!["unused-dep".to_string()],
        ignore_exports: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        duplicates: fallow_config::DuplicatesConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: false.into(),
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
    .resolve(root, OutputFormat::Human, 4, true, true);

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .unused_dependencies
            .iter()
            .any(|d| d.package_name == "unused-dep"),
        "unused-dep should be ignored"
    );
}

// ── JSON serialization ─────────────────────────────────────────

#[test]
fn results_serializable_to_json() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let json = serde_json::to_string(&results).unwrap();
    assert!(!json.is_empty());
    // Verify it round-trips
    let _: serde_json::Value = serde_json::from_str(&json).unwrap();
}
