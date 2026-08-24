use crate::common::{create_config, fixture_path};

#[test]
fn convex_json_custom_functions_directory_replaces_the_default_entry_root() {
    let root = fixture_path("issue-2387-convex-custom-functions");
    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");
    let unused_files: Vec<_> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("backend/query.ts")),
        "the configured Convex functions directory is an entry root: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path.ends_with("convex/legacy.ts")),
        "the default Convex directory is replaced by the explicit functions directory: {unused_files:?}"
    );
}
