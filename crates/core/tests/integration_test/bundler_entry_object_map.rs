use super::common::{create_config, fixture_path};

fn unused_file_paths(fixture: &str) -> Vec<String> {
    let root = fixture_path(fixture);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn assert_object_map_values_are_entries(fixture: &str) {
    let unused_files = unused_file_paths(fixture);

    for used_path in ["src/main.ts", "src/worker.ts", "src/helper.ts"] {
        assert!(
            !unused_files.iter().any(|unused| unused == used_path),
            "{used_path} should be reachable through the configured entry, unused files: {unused_files:?}"
        );
    }

    assert!(
        unused_files.iter().any(|unused| unused == "src/orphan.ts"),
        "unrelated source files should remain reportable, unused files: {unused_files:?}"
    );
}

#[test]
fn tsup_entry_object_map_values_are_entry_points() {
    assert_object_map_values_are_entries("tsup-entry-object-map");
}

#[test]
fn tsdown_entry_object_map_values_are_entry_points() {
    assert_object_map_values_are_entries("tsdown-entry-object-map");
}

#[test]
fn tsup_config_array_entries_are_entry_points() {
    assert_object_map_values_are_entries("tsup-config-array");
}

#[test]
fn tsdown_config_array_entries_are_entry_points() {
    assert_object_map_values_are_entries("tsdown-config-array");
}
