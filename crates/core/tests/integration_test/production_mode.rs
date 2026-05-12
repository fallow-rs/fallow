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
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
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

#[test]
fn production_mode_still_parses_vite_config_aliases() {
    let root = fixture_path("vite-alias-project");
    let config = create_production_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.specifier.as_str())
        .collect();

    assert!(
        unresolved_specs.is_empty(),
        "vite.config.ts aliases should still resolve in production mode: {unresolved_specs:?}"
    );
}

#[test]
fn production_mode_resolves_solution_style_tsconfig_paths() {
    let root = fixture_path("vite-solution-tsconfig-paths");
    let config = create_production_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.specifier.as_str())
        .collect();

    assert!(
        unresolved_specs.is_empty(),
        "solution-style tsconfig references should still provide path aliases in production mode: {unresolved_specs:?}"
    );

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(
        !unused_file_names.contains(&"messages.ts".to_string()),
        "messages.ts should be reachable via tsconfig.app.json paths alias: {unused_file_names:?}"
    );
}

#[test]
fn analyze_project_honors_per_analysis_dead_code_production() {
    // analyze_project (used by the LSP) routes through default_config which
    // calls config.resolve() directly. When the loaded config uses the
    // per-analysis production form, default_config must flatten the
    // production flag for dead-code analysis. Without that flatten,
    // ResolvedConfig.production silently stays false and test files leak in.
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"per-analysis-prod","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/index.ts"), "export const ok = 1;\n").unwrap();
    std::fs::write(root.join("src/utils.test.ts"), "export const dead = 1;\n").unwrap();
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"production":{"deadCode":true,"health":false,"dupes":false}}"#,
    )
    .unwrap();

    let results = fallow_core::analyze_project(root).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(
        !unused_file_names.contains(&"utils.test.ts".to_string()),
        "per-analysis production.deadCode=true should exclude *.test.ts from analyze_project, found: {unused_file_names:?}"
    );
}
