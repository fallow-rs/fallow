//! Integration tests for the n8n plugin.

use super::common::{create_config, fixture_path};

#[test]
fn n8n_nodes_and_credentials_fields_are_entry_points() {
    let root = fixture_path("n8n-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_paths: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(&root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    for path in [
        "nodes/Foo/Foo.node.ts",
        "nodes/Foo/v1/FooV1.node.ts",
        "credentials/FooApi.credentials.ts",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "n8n loads {path} through the package.json n8n field, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"nodes/Unused/Unused.node.ts".to_string()),
        "a node that the n8n field does not list must stay unused: {unused_paths:?}"
    );

    let unused_members: Vec<&str> = results
        .unused_class_members
        .iter()
        .map(|finding| finding.member.member_name.as_str())
        .collect();
    assert!(
        !unused_members.contains(&"webhookMethods"),
        "n8n calls the webhookMethods of a node class: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"unusedHelper"),
        "another member of a node class stays reportable: {unused_members:?}"
    );
}
