use super::common::{create_config, fixture_path};

#[test]
fn graphql_hash_imports_keep_documents_reachable() {
    let root = fixture_path("graphql-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        !unused_files.contains(&"content.graphql".to_string()),
        "GraphQL #import target should be reachable, got unused files: {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"leaf.gql".to_string()),
        "GraphQL .gql #import target should be reachable, got unused files: {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"orphan.graphql".to_string()),
        "Unreferenced GraphQL documents should still be reported, got unused files: {unused_files:?}"
    );
}
