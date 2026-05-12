use std::path::PathBuf;

use super::common::fixture_path;
use fallow_config::{FallowConfig, OutputFormat, RulesConfig};

fn external_plugin_config(root: &std::path::Path) -> fallow_config::ResolvedConfig {
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
    .resolve(root.to_path_buf(), OutputFormat::Human, 4, true, true)
}

#[test]
fn external_plugin_entry_points_discovered() {
    let root = fixture_path("external-plugins");
    let config = external_plugin_config(&root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // home.ts is a route file — external plugin marks src/routes/**/*.{ts,tsx} as entry points
    assert!(
        !unused_file_names.contains(&"home.ts".to_string()),
        "home.ts should be an entry point via external plugin, unused: {unused_file_names:?}"
    );

    // setup.ts is always-used via external plugin
    assert!(
        !unused_file_names.contains(&"setup.ts".to_string()),
        "setup.ts should be always-used via external plugin, unused: {unused_file_names:?}"
    );

    // orphan.ts is NOT covered by the plugin, should be unused
    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be unused, found: {unused_file_names:?}"
    );
}

#[test]
fn plugin_entry_points_carry_correct_plugin_name() {
    let root = fixture_path("external-plugins");
    let config = external_plugin_config(&root);

    let files = fallow_core::discover::discover_files(&config);

    // Run plugins to get aggregated result
    let pkg = fallow_config::PackageJson::load(&root.join("package.json")).unwrap();
    let file_paths: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    let registry = fallow_core::plugins::PluginRegistry::new(
        fallow_config::discover_external_plugins(&root, &[]),
    );
    let plugin_result = registry.run(&pkg, &root, &file_paths);

    let entries =
        fallow_core::discover::discover_plugin_entry_points(&plugin_result, &config, &files);

    // External plugin "my-framework" should attribute entry points with its name
    let home_entry = entries
        .iter()
        .find(|ep| ep.path.ends_with("home.ts"))
        .expect("home.ts should be discovered as an entry point");
    assert!(
        matches!(
            &home_entry.source,
            fallow_types::discover::EntryPointSource::Plugin { name } if name == "my-framework"
        ),
        "home.ts should be attributed to 'my-framework' plugin, got: {:?}",
        home_entry.source
    );

    // setup.ts is always-used via the external plugin
    let setup_entry = entries
        .iter()
        .find(|ep| ep.path.ends_with("setup.ts"))
        .expect("setup.ts should be discovered as an entry point");
    assert!(
        matches!(
            &setup_entry.source,
            fallow_types::discover::EntryPointSource::Plugin { name } if name == "my-framework"
        ),
        "setup.ts should be attributed to 'my-framework' plugin, got: {:?}",
        setup_entry.source
    );
}

#[test]
fn external_plugin_used_exports_respected() {
    let root = fixture_path("external-plugins");
    let config = external_plugin_config(&root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    // `default` and `loader` exports are marked as used by the plugin
    assert!(
        !unused_export_names.contains(&"default"),
        "default export should be used via external plugin used_exports"
    );
    assert!(
        !unused_export_names.contains(&"loader"),
        "loader export should be used via external plugin used_exports"
    );

    // `unused` export in utils.ts (not an entry point) should be flagged
    assert!(
        unused_export_names.contains(&"unused"),
        "unused export in utils.ts should be flagged, found: {unused_export_names:?}"
    );
}

#[test]
fn external_plugin_tooling_dependencies_not_flagged() {
    let root = fixture_path("external-plugins");
    let config = external_plugin_config(&root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.package_name.as_str())
        .collect();

    // my-framework-cli is listed as tooling dependency in the external plugin
    assert!(
        !unused_dev_dep_names.contains(&"my-framework-cli"),
        "my-framework-cli should not be flagged (tooling dep), found: {unused_dev_dep_names:?}"
    );
}

#[test]
fn external_plugin_active_in_list() {
    let root = fixture_path("external-plugins");
    let config = external_plugin_config(&root);

    let files = fallow_core::discover::discover_files(&config);
    let file_paths: Vec<std::path::PathBuf> = files.iter().map(|f| f.path.clone()).collect();

    let pkg_path = root.join("package.json");
    let pkg = fallow_config::PackageJson::load(&pkg_path).unwrap();

    let registry = fallow_core::plugins::PluginRegistry::new(config.external_plugins);
    let result = registry.run(&pkg, &root, &file_paths);

    assert!(
        result.active_plugins.contains(&"my-framework".to_string()),
        "my-framework external plugin should be active, found: {:?}",
        result.active_plugins
    );
}

#[test]
fn external_plugin_config_patterns_always_used() {
    let root = fixture_path("external-plugins");
    let config = external_plugin_config(&root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // my-framework.config.ts is matched by config_patterns, should be always-used
    assert!(
        !unused_file_names.contains(&"my-framework.config.ts".to_string()),
        "my-framework.config.ts should be always-used via config_patterns, unused: {unused_file_names:?}"
    );
}
