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

#[test]
fn issue_1373_merged_namespace_value_import_through_star_barrel_is_used() {
    let root = fixture_path("issue-1373-merged-namespace-star-reexport");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"Merged"),
        "Merged value import should be credited through the star barrel, found: {unused_export_names:?}"
    );
    assert!(
        unused_type_names.contains(&"Merged"),
        "value-only usage should not credit the type-only namespace, found: {unused_type_names:?}"
    );
    assert!(
        unused_export_names.contains(&"unusedControl"),
        "unrelated value exports should remain reportable, found: {unused_export_names:?}"
    );

    let output = fallow_core::analyze_with_trace(&config).expect("trace analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("trace graph should be retained");
    let trace = fallow_core::trace::trace_export(graph, &config.root, "src/merged.ts", "Merged")
        .expect("Merged trace should exist");

    assert!(trace.is_used, "trace should agree the value export is used");
    assert_eq!(
        trace.direct_references.len(),
        1,
        "trace should include the consumer named import"
    );
}
