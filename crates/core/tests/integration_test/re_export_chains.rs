use super::common::{create_config, fixture_path};

#[test]
fn three_level_star_chain_used_exports_propagate() {
    let root = fixture_path("re-export-chains");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // alpha and beta are imported through barrel-a -> barrel-b -> barrel-c -> source
    assert!(
        !unused_export_names.contains(&"alpha"),
        "alpha should propagate through 3-level star chain, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"beta"),
        "beta should propagate through 3-level star chain, found: {unused_export_names:?}"
    );
}

#[test]
fn three_level_star_chain_unused_exports_detected() {
    let root = fixture_path("re-export-chains");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // gamma and delta are star-re-exported but never imported by index.ts
    assert!(
        unused_export_names.contains(&"gamma"),
        "gamma should be unused (not imported), found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"delta"),
        "delta should be unused (not imported), found: {unused_export_names:?}"
    );
}

#[test]
fn three_level_star_chain_no_unused_files() {
    let root = fixture_path("re-export-chains");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // All files are part of the chain, none should be unused
    assert!(
        results.unused_files.is_empty(),
        "no files should be unused in re-export chain fixture, found: {:?}",
        results
            .unused_files
            .iter()
            .map(|f| &f.file.path)
            .collect::<Vec<_>>()
    );
}
