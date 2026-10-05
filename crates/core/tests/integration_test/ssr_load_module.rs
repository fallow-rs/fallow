use super::common::{create_config, fixture_path};

fn unused_file_paths(
    results: &fallow_types::results::AnalysisResults,
    root: &std::path::Path,
) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| relative(&f.file.path, root))
        .collect()
}

fn relative(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_exports(
    results: &fallow_types::results::AnalysisResults,
    root: &std::path::Path,
) -> Vec<(String, String)> {
    let mut exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| (relative(&e.export.path, root), e.export.export_name.clone()))
        .collect();
    exports.sort();
    exports
}

#[test]
fn ssr_load_module_literal_path_reaches_the_module() {
    let root = fixture_path("ssr-load-module");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results, &root);
    assert!(
        !unused_files.iter().any(|path| path == "src/a.ts"),
        "src/a.ts is loaded through ssrLoadModule, unused files: {unused_files:?}"
    );
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "packages/web/src/view.ts"),
        "packages/web/src/view.ts is loaded through ssrLoadModule, unused files: {unused_files:?}"
    );
}

#[test]
fn ssr_load_module_credits_only_the_members_in_use() {
    let root = fixture_path("ssr-load-module");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unused_exports(&results, &root),
        [
            (
                "packages/web/src/view.ts".to_string(),
                "unusedView".to_string()
            ),
            ("src/a.ts".to_string(), "dead".to_string()),
        ]
    );
}
