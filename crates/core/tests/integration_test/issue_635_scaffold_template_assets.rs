use crate::common::{create_config, fixture_path};

#[test]
fn package_files_template_roots_are_support_entry_points_for_workspace_package() {
    let root = fixture_path("issue-635-scaffold-template-assets");
    let config = create_config(root.clone());

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = results
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
        .collect::<Vec<_>>();

    assert!(
        !unused_files.contains(&"packages/create-vite/template-react/src/App.jsx".to_string()),
        "template files copied at runtime should be treated as support assets: {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"packages/create-vite/template-react-ts/src/App.tsx".to_string()),
        "typed template files copied at runtime should be treated as support assets: {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"packages/create-vite/src/orphan.ts".to_string()),
        "unrelated source files should still be reported unused: {unused_files:?}"
    );
}
