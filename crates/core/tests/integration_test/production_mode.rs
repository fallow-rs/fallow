use super::common::{create_config, fixture_path};
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
        used_class_members: vec![],
        duplicates: fallow_config::DuplicatesConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: true,
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
    .resolve(root, OutputFormat::Human, 4, true, true)
}

#[test]
fn production_mode_excludes_test_files() {
    let root = fixture_path("production-mode");
    let config = create_production_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let all_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // Test files should not appear at all (not even as unused) since
    // production mode excludes them from discovery.
    assert!(
        !all_file_names.contains(&"utils.test.ts".to_string()),
        "utils.test.ts should not appear in production mode results, found: {all_file_names:?}"
    );
}

#[test]
fn production_mode_disables_dev_dependency_checking() {
    let root = fixture_path("production-mode");
    let config = create_production_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // In production mode, unused_dev_dependencies should be empty
    // because the rule is forced off.
    assert!(
        results.unused_dev_dependencies.is_empty(),
        "unused_dev_dependencies should be empty in production mode, found: {:?}",
        results
            .unused_dev_dependencies
            .iter()
            .map(|d| d.package_name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn production_mode_still_detects_unused_exports() {
    let root = fixture_path("production-mode");
    let config = create_production_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    // testHelper is only used from the test file which is excluded,
    // so in production mode it should be unused.
    assert!(
        unused_export_names.contains(&"testHelper"),
        "testHelper should be unused in production mode (test consumer excluded), found: {unused_export_names:?}"
    );
}

#[test]
fn production_mode_does_not_exclude_nested_config_files() {
    // Regression test for #111: **/*.config.* excluded Angular's src/app/app.config.ts
    let root = fixture_path("angular-production-config");
    let config = create_production_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // app.config.ts is a runtime file imported by main.ts, NOT a tool config.
    // It must be discovered in production mode so app.routes.ts stays reachable.
    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(
        !unused_file_names.contains(&"app.config.ts".to_string()),
        "app.config.ts should not be reported unused in production mode, found: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"app.routes.ts".to_string()),
        "app.routes.ts should be reachable via app.config.ts in production mode, found: {unused_file_names:?}"
    );
}

#[test]
fn non_production_mode_includes_test_files() {
    let root = fixture_path("production-mode");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // In non-production mode, test-only.ts should be detected as unused
    // (it's not imported by anything)
    assert!(
        unused_file_names.contains(&"test-only.ts".to_string()),
        "test-only.ts should be detected as unused in non-production mode, found: {unused_file_names:?}"
    );
}
