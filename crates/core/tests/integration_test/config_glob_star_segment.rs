//! A single `*` in a config path glob stays within one path segment. Only
//! `**` crosses directories, as in picomatch and the `entry` globs. Fallow
//! matches file paths, not directories, so `src/generated/*` does not ignore
//! a nested file (gitignore does, because it skips a matched directory).

use std::path::Path;

const TOP_FILE: &str = "src/generated/top.js";
const NESTED_FILE: &str = "src/generated/deep/nested.js";

fn project(main_source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    let root = dir.path();
    let files = [
        (
            "package.json",
            r#"{"name":"repro-star","private":true,"type":"module","main":"src/main.js","dependencies":{"gen-lib":"1.0.0"}}"#,
        ),
        ("src/main.js", main_source),
        (TOP_FILE, "export const top = 1;\n"),
        (NESTED_FILE, "export const nested = 1;\n"),
    ];
    for (path, source) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        std::fs::write(path, source).expect("fixture file");
    }
    dir
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("project-relative finding")
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_files(config: &fallow_config::ResolvedConfig) -> Vec<String> {
    let mut files: Vec<String> = fallow_core::analyze(config)
        .expect("analysis")
        .unused_files
        .iter()
        .map(|file| relative(&config.root, &file.file.path))
        .collect();
    files.sort();
    files
}

/// Return the files that report an unused `extra` export.
fn unused_extra_export_files(config: &fallow_config::ResolvedConfig) -> Vec<String> {
    let mut files: Vec<String> = fallow_core::analyze(config)
        .expect("analysis")
        .unused_exports
        .iter()
        .filter(|export| export.export.export_name == "extra")
        .map(|export| relative(&config.root, &export.export.path))
        .collect();
    files.sort();
    files.dedup();
    files
}

#[test]
fn ignore_patterns_from_config_star_does_not_cross_directories() {
    let dir = project("export const ready = true;\n");
    let config = super::common::create_config_with_ignore_patterns(
        dir.path().to_path_buf(),
        &["src/generated/*"],
    );
    assert_eq!(unused_files(&config), vec![NESTED_FILE.to_string()]);
}

#[test]
fn ignore_patterns_double_star_still_crosses_directories() {
    let dir = project("export const ready = true;\n");
    let config = super::common::create_config_with_ignore_patterns(
        dir.path().to_path_buf(),
        &["src/generated/**"],
    );
    let unused = unused_files(&config);
    assert!(unused.is_empty(), "unexpected unused files: {unused:?}");
}

/// Analyze a project whose two generated files each have an `extra` export
/// that no file imports. Return the files with an unused export.
fn unused_exports_with_plugin(plugin: &str) -> Vec<String> {
    let dir = project(
        "import { top } from './generated/top.js';\nimport { nested } from './generated/deep/nested.js';\nexport const ready = [top, nested];\n",
    );
    for path in [TOP_FILE, NESTED_FILE] {
        std::fs::write(
            dir.path().join(path),
            "export const top = 1;\nexport const nested = 1;\nexport const extra = 1;\n",
        )
        .expect("generated file");
    }
    let config_path = dir.path().join(".fallowrc.json");
    std::fs::write(&config_path, format!(r#"{{ "framework": [{plugin}] }}"#)).expect("config file");
    let config = fallow_config::FallowConfig::load(&config_path)
        .expect("config loads")
        .resolve(
            dir.path().to_path_buf(),
            fallow_config::OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
    unused_extra_export_files(&config)
}

#[test]
fn external_plugin_used_exports_star_does_not_cross_directories() {
    let unused = unused_exports_with_plugin(
        r#"{
            "name": "generated-exports",
            "enablers": ["gen-lib"],
            "usedExports": [{ "pattern": "src/generated/*.js", "exports": ["extra"] }]
        }"#,
    );
    assert_eq!(unused, vec![NESTED_FILE.to_string()]);
}

#[test]
fn external_plugin_file_exists_star_does_not_cross_directories() {
    let unused = unused_exports_with_plugin(
        r#"{
            "name": "generated-exports",
            "detection": { "type": "fileExists", "pattern": "src/*/nested.js" },
            "usedExports": [{ "pattern": "src/generated/**/*.js", "exports": ["extra"] }]
        }"#,
    );
    assert_eq!(unused, vec![NESTED_FILE.to_string(), TOP_FILE.to_string()]);
}
