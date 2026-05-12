use std::path::PathBuf;

use fallow_config::{FallowConfig, OutputFormat, RulesConfig};

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn create_config(root: PathBuf) -> fallow_config::ResolvedConfig {
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
    .resolve(root, OutputFormat::Human, 4, true, true)
}

pub fn create_config_with_cache(
    root: PathBuf,
    cache_dir: std::path::PathBuf,
) -> fallow_config::ResolvedConfig {
    let mut config = FallowConfig {
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
    .resolve(root, OutputFormat::Human, 4, false, true); // no_cache = false to enable caching
    config.cache_dir = cache_dir;
    config
}
