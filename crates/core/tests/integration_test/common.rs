use std::path::PathBuf;

use fallow_config::{ConfigOverride, FallowConfig, OutputFormat, PartialRulesConfig, RulesConfig};

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
        type_aware: fallow_config::TypeAwareConfig::default(),
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
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
    .resolve(root, OutputFormat::Human, 4, true, true, None)
}

/// The default config with production mode enabled, so dependency findings
/// that only exist in production mode (type-only dependencies) are emitted.
pub fn create_production_config(root: PathBuf) -> fallow_config::ResolvedConfig {
    FallowConfig {
        type_aware: fallow_config::TypeAwareConfig::default(),
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
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
        production: true.into(),
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

/// A config with `ignorePatterns` set, resolved through the same path a user's
/// `fallow.toml` takes. Building the `GlobSet` this way keeps the default ignore
/// set (`node_modules`, build output) that overwriting `ResolvedConfig::ignore_patterns`
/// after the fact would discard. Everything else defaults.
pub fn create_config_with_ignore_patterns(
    root: PathBuf,
    ignore_patterns: &[&str],
) -> fallow_config::ResolvedConfig {
    FallowConfig {
        ignore_patterns: ignore_patterns.iter().map(|p| (*p).to_string()).collect(),
        ..Default::default()
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None)
}

/// A config with `unusedComponentProps.ignorePattern` set, used to exercise the
/// opt-in prop-exemption knob. Everything else defaults.
pub fn create_config_with_unused_props_ignore(
    root: PathBuf,
    ignore_pattern: &str,
) -> fallow_config::ResolvedConfig {
    FallowConfig {
        unused_component_props: fallow_config::UnusedComponentPropsConfig {
            ignore_pattern: Some(ignore_pattern.to_string()),
        },
        ..Default::default()
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None)
}

pub fn create_config_with_cache(
    root: PathBuf,
    cache_dir: std::path::PathBuf,
) -> fallow_config::ResolvedConfig {
    let mut config = FallowConfig {
        type_aware: fallow_config::TypeAwareConfig::default(),
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
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
    .resolve(root, OutputFormat::Human, 4, false, true, None); // no_cache = false to enable caching
    config.cache_dir = cache_dir;
    config
}

pub fn create_config_with_rules<F>(root: PathBuf, modify: F) -> fallow_config::ResolvedConfig
where
    F: FnOnce(&mut RulesConfig),
{
    let mut rules = RulesConfig::default();
    modify(&mut rules);
    FallowConfig {
        type_aware: fallow_config::TypeAwareConfig::default(),
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
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
        rules,
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

pub fn create_config_with_overrides(
    root: PathBuf,
    overrides: Vec<(&str, PartialRulesConfig)>,
) -> fallow_config::ResolvedConfig {
    let overrides = overrides
        .into_iter()
        .map(|(glob, rules)| ConfigOverride {
            files: vec![glob.to_string()],
            rules,
        })
        .collect();
    FallowConfig {
        type_aware: fallow_config::TypeAwareConfig::default(),
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
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
        overrides,
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

pub fn create_config_with_ignore_decorators(
    root: PathBuf,
    ignore_decorators: Vec<String>,
) -> fallow_config::ResolvedConfig {
    FallowConfig {
        type_aware: fallow_config::TypeAwareConfig::default(),
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators,
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
