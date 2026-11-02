//! Integration tests for the graphql-config plugin.

use super::common::{create_config, fixture_path};

fn unused_file_paths(
    root: &std::path::Path,
    results: &fallow_types::results::AnalysisResults,
) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn graphql_config_schema_and_documents_are_used() {
    // A GraphQL tool reads the `schema` and `documents` files of a
    // graphql-config file, so no import reaches them.
    let root = fixture_path("graphql-config-documents");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths = unused_file_paths(&root, &results);
    for used in [
        "packages/github/graphql.config.ts",
        "packages/github/schema/github.graphql",
        "packages/github/src/server/Viewer.graphql",
        "shared/ViewerFields.graphql",
        "schema/app.graphql",
        "operations/AppName.graphql",
    ] {
        assert!(
            !unused_paths.contains(&used.to_string()),
            "{used} should be used, unused files: {unused_paths:?}"
        );
    }
    for unused in [
        "src/orphan.ts",
        "packages/github/src/unusedHelper.ts",
        "packages/github/legacy/Stray.graphql",
    ] {
        assert!(
            unused_paths.contains(&unused.to_string()),
            "{unused} should still be reported, unused files: {unused_paths:?}"
        );
    }
}
