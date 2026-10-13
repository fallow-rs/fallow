//! Integration tests for the typescript-content-mapper plugin.

use super::common::{create_config, fixture_path};

#[test]
fn content_mapper_exec_script_is_an_entry_point() {
    // TypeScript runs the `typescript.contentMapper.exec` command, so its
    // script file and the imports of that file are reachable.
    let root = fixture_path("typescript-content-mapper-plugin");
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
    for path in ["lib/transform-server.js", "lib/parse-frontmatter.js"] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable through the content mapper command, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"lib/unused-plugin.js".to_string()),
        "ordinary unused files should still report, unused files: {unused_paths:?}"
    );
}
