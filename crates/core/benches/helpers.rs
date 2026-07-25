#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::path::PathBuf;

use fallow_config::{BoundaryConfig, FallowConfig, OutputFormat};
use tempfile::TempDir;

#[must_use]
#[allow(
    dead_code,
    reason = "shared helper is used by analysis but not every bench target"
)]
pub fn create_test_config(root: PathBuf) -> fallow_config::ResolvedConfig {
    make_config(root, true)
}

#[must_use]
pub fn make_config(root: PathBuf, no_cache: bool) -> fallow_config::ResolvedConfig {
    FallowConfig {
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
        rules: fallow_config::RulesConfig::default(),
        boundaries: BoundaryConfig::default(),
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
    .resolve(root, OutputFormat::Human, 4, no_cache, true, None)
}

/// Generate a synthetic project with `file_count` source files.
/// Half of the exports are consumed by the entry point, the other half are "dead".
#[must_use]
#[allow(
    dead_code,
    reason = "shared helper is used by analysis but not every bench target"
)]
pub fn create_synthetic_project(
    name: &str,
    file_count: usize,
) -> (TempDir, fallow_config::ResolvedConfig) {
    create_synthetic_project_with_cache(name, file_count, true)
}

/// # Panics
///
/// Panics if temporary directory creation or file writes fail.
#[must_use]
pub fn create_synthetic_project_with_cache(
    name: &str,
    file_count: usize,
    no_cache: bool,
) -> (TempDir, fallow_config::ResolvedConfig) {
    let temp_dir = tempfile::Builder::new()
        .prefix(&format!("fallow-bench-{name}-"))
        .tempdir()
        .unwrap();
    let root = temp_dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();

    std::fs::write(
        root.join("package.json"),
        r#"{"name": "bench-project", "main": "src/index.ts", "dependencies": {"react": "^18"}}"#,
    )
    .unwrap();

    for i in 0..file_count {
        let content = format!(
            r"
export const value{i} = {i};
export function fn{i}() {{ return {i}; }}
export type Type{i} = {{ value: number }};
export const helper{i} = () => value{i} + 1;
"
        );
        std::fs::write(root.join(format!("src/module{i}.ts")), content).unwrap();
    }

    let used_count = file_count / 2;
    let imports: Vec<String> = (0..used_count)
        .map(|i| format!("import {{ value{i} }} from './module{i}';"))
        .collect();
    let uses: Vec<String> = (0..used_count)
        .map(|i| format!("console.log(value{i});"))
        .collect();
    std::fs::write(
        root.join("src/index.ts"),
        format!("{}\n{}\n", imports.join("\n"), uses.join("\n")),
    )
    .unwrap();

    let config = make_config(root, no_cache);
    (temp_dir, config)
}
