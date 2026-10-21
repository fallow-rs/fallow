//! Integration tests for the Sanity plugin.

use super::common::{create_config, fixture_path};

fn relative(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn analyze(include_entry_exports: bool) -> (Vec<String>, Vec<(String, String)>) {
    let root = fixture_path("sanity-config-tsx");
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
fn sanity_config_tsx_is_an_entry_point() {
    // Sanity Studio loads sanity.config.{ts,tsx,js,jsx}. The .tsx config
    // file and the modules it imports are used.
    let (unused_files, unused_exports) = analyze(false);

    assert!(
        unused_files.is_empty(),
        "sanity.config.tsx and its imports are used, unused files: {unused_files:?}"
    );
    assert!(
        unused_exports.is_empty(),
        "fallow skips the exports of an entry point, unused exports: {unused_exports:?}"
    );
}

#[test]
fn sanity_config_tsx_default_export_is_used_with_include_entry_exports() {
    // Sanity Studio reads the default export of sanity.config.tsx. A named
    // export that Sanity does not read stays reportable.
    let (unused_files, unused_exports) = analyze(true);

    assert!(
        unused_files.is_empty(),
        "sanity.config.tsx and its imports are used, unused files: {unused_files:?}"
    );
    assert_eq!(
        unused_exports,
        vec![
            ("sanity.config.tsx".to_owned(), "unusedHelper".to_owned()),
            ("src/index.ts".to_owned(), "main".to_owned()),
        ],
        "only the default export of sanity.config.tsx gets the credit"
    );
}
