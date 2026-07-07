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
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
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
    .resolve(root.to_path_buf(), OutputFormat::Human, 4, true, true, None)
}

#[test]
fn external_plugin_entry_points_discovered() {
    let root = fixture_path("external-plugins");
    let config = external_plugin_config(&root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        !unused_file_names.contains(&"home.ts".to_string()),
        "home.ts should be an entry point via external plugin, unused: {unused_file_names:?}"
    );

    assert!(
        !unused_file_names.contains(&"setup.ts".to_string()),
        "setup.ts should be always-used via external plugin, unused: {unused_file_names:?}"
    );

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

    let pkg = fallow_config::PackageJson::load(&root.join("package.json")).unwrap();
    let file_paths: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    let registry = fallow_core::plugins::PluginRegistry::new(
        fallow_config::discover_external_plugins(&root, &[]),
    );
    let plugin_result = registry
        .try_run(&pkg, &root, &file_paths)
        .expect("external plugin registry should run");

    let entries =
        fallow_core::discover::discover_plugin_entry_points(&plugin_result, &config, &files);

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
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"default"),
        "default export should be used via external plugin used_exports"
    );
    assert!(
        !unused_export_names.contains(&"loader"),
        "loader export should be used via external plugin used_exports"
    );

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
        .map(|d| d.dep.package_name.as_str())
        .collect();

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
    let result = registry
        .try_run(&pkg, &root, &file_paths)
        .expect("external plugin registry should run");

    assert!(
        result.active_plugins.contains(&"my-framework".to_string()),
        "my-framework external plugin should be active, found: {:?}",
        result.active_plugins
    );
}

fn manifest_config(root: &std::path::Path) -> fallow_config::ResolvedConfig {
    external_plugin_config(root)
}

fn unused_rel_paths(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn is_unused(unused: &[String], suffix: &str) -> bool {
    unused.iter().any(|p| p.ends_with(suffix))
}

#[test]
fn manifest_entries_seed_plugin_trees_from_kibana_jsonc() {
    let root = fixture_path("manifest-entries-kibana");
    let config = manifest_config(&root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_rel_paths(&results);

    // browser + server + extraPublicDirs seeds are all reachable, plus transitive helper.
    for reachable in [
        "plugins/alpha/public/index.ts",
        "plugins/alpha/public/helper.ts",
        "plugins/alpha/server/index.ts",
        "plugins/alpha/common/index.ts",
        "plugins/beta/public/index.ts",
    ] {
        assert!(
            !is_unused(&unused, reachable),
            "{reachable} should be seeded/reachable via manifestEntries, unused: {unused:?}"
        );
    }

    // beta has server:false, so the per-seed `when` skips its server entry.
    assert!(
        is_unused(&unused, "plugins/beta/server/index.ts"),
        "beta server (server:false) must NOT be seeded and stays unused, unused: {unused:?}"
    );
    // a genuinely-orphan file stays flagged.
    assert!(
        is_unused(&unused, "orphan.ts"),
        "orphan.ts should stay unused, unused: {unused:?}"
    );
}

#[test]
fn manifest_entries_are_load_bearing() {
    // Neuter: with the external plugin removed, the plugin trees have no entry
    // point and must report as unused, proving manifestEntries is what seeds them.
    let root = fixture_path("manifest-entries-kibana");
    let mut config = manifest_config(&root);
    config.external_plugins = vec![];
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_rel_paths(&results);

    for should_be_unused in [
        "plugins/alpha/public/index.ts",
        "plugins/alpha/server/index.ts",
        "plugins/beta/public/index.ts",
    ] {
        assert!(
            is_unused(&unused, should_be_unused),
            "{should_be_unused} should be unused WITHOUT the manifestEntries plugin, unused: {unused:?}"
        );
    }
}

#[test]
fn external_plugin_config_patterns_always_used() {
    let root = fixture_path("external-plugins");
    let config = external_plugin_config(&root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        !unused_file_names.contains(&"my-framework.config.ts".to_string()),
        "my-framework.config.ts should be always-used via config_patterns, unused: {unused_file_names:?}"
    );
}
