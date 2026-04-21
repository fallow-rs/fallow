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
        used_class_members: vec![],
        duplicates: fallow_config::DuplicatesConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: false,
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        codeowners: None,
        public_packages: vec![],
        flags: fallow_config::FlagsConfig::default(),
        resolve: fallow_config::ResolveConfig::default(),
        sealed: false,
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
        used_class_members: vec![],
        duplicates: fallow_config::DuplicatesConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: false,
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        codeowners: None,
        public_packages: vec![],
        flags: fallow_config::FlagsConfig::default(),
        resolve: fallow_config::ResolveConfig::default(),
        sealed: false,
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

// ── Ignore dependencies ────────────────────────────────────────

#[test]
fn ignore_dependencies_config() {
    let root = fixture_path("basic-project");
    let config = FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec!["unused-dep".to_string()],
        ignore_exports: vec![],
        used_class_members: vec![],
        duplicates: fallow_config::DuplicatesConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: false,
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        codeowners: None,
        public_packages: vec![],
        flags: fallow_config::FlagsConfig::default(),
        resolve: fallow_config::ResolveConfig::default(),
        sealed: false,
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
