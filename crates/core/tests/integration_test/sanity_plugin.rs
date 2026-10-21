//! Integration tests for the Sanity plugin.

use super::common::{create_config, fixture_path};

/// Fixture, config file and main entry for each config file extension that
/// Sanity Studio loads and that has its own fixture.
const STUDIOS: &[(&str, &str, &str)] = &[
    ("sanity-config-tsx", "sanity.config.tsx", "src/index.ts"),
    ("sanity-config-jsx", "sanity.config.jsx", "src/index.js"),
];

fn relative(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn analyze(fixture: &str, include_entry_exports: bool) -> (Vec<String>, Vec<(String, String)>) {
    let root = fixture_path(fixture);
    let mut config = create_config(root.clone());
    config.include_entry_exports = include_entry_exports;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| relative(&root, &finding.file.path))
        .collect();
    unused_files.sort();
    let mut unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                relative(&root, &finding.export.path),
                finding.export.export_name.clone(),
            )
        })
        .collect();
    unused_exports.sort();
    (unused_files, unused_exports)
}

#[test]
fn sanity_config_tsx_and_jsx_are_entry_points() {
    // Sanity Studio loads sanity.config.{ts,tsx,js,jsx}. The config file and
    // the modules it imports are used.
    for &(fixture, config_file, _) in STUDIOS {
        let (unused_files, unused_exports) = analyze(fixture, false);

        assert!(
            unused_files.is_empty(),
            "{config_file} and its imports are used, unused files: {unused_files:?}"
        );
        assert!(
            unused_exports.is_empty(),
            "fallow skips the exports of {config_file}, unused exports: {unused_exports:?}"
        );
    }
}

#[test]
fn sanity_config_tsx_and_jsx_default_export_is_used_with_include_entry_exports() {
    // Sanity Studio reads the default export of the config file. A named
    // export that Sanity does not read stays reportable.
    for &(fixture, config_file, main) in STUDIOS {
        let (unused_files, unused_exports) = analyze(fixture, true);

        assert!(
            unused_files.is_empty(),
            "{config_file} and its imports are used, unused files: {unused_files:?}"
        );
        assert_eq!(
            unused_exports,
            vec![
                (config_file.to_owned(), "unusedHelper".to_owned()),
                (main.to_owned(), "main".to_owned()),
            ],
            "only the default export of {config_file} gets the credit"
        );
    }
}
