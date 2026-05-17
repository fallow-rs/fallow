use super::common::{create_config, fixture_path};

fn analyze_fixture(name: &str) -> fallow_core::results::AnalysisResults {
    let root = fixture_path(name);
    let config = create_config(root);
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn unused_export_names(results: &fallow_core::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect()
}

fn unused_file_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect()
}

#[test]
fn unreachable_mixed_exports_flags_unused_export() {
    let results = analyze_fixture("unreachable-mixed-exports");
    let unused_export_names = unused_export_names(&results);

    // unusedHelper is exported but never imported by anyone — should be flagged
    assert!(
        unused_export_names.contains(&"unusedHelper"),
        "unusedHelper should be detected as unused export, found: {unused_export_names:?}"
    );
}

#[test]
fn unreachable_mixed_exports_flags_export_only_used_by_unreachable() {
    let results = analyze_fixture("unreachable-mixed-exports");
    let unused_export_names = unused_export_names(&results);

    // usedHelper is imported by setup.ts, but setup.ts is also unreachable,
    // so the reference shouldn't count — usedHelper should be flagged
    assert!(
        unused_export_names.contains(&"usedHelper"),
        "usedHelper (only referenced by unreachable module) should be flagged, found: {unused_export_names:?}"
    );
}

#[test]
fn unreachable_mixed_exports_flags_dead_child_file() {
    let results = analyze_fixture("unreachable-mixed-exports");
    let unused_file_names = unused_file_names(&results);

    assert!(
        unused_file_names.contains(&"setup.ts".to_string()),
        "setup.ts should be flagged as the dead root, found: {unused_file_names:?}"
    );
    assert!(
        unused_file_names.contains(&"helpers.ts".to_string()),
        "helpers.ts should be flagged even though only a dead file imports it, found: {unused_file_names:?}"
    );
}

#[test]
fn unreachable_barrel_subtree_flags_barrel_and_leaf_files() {
    let results = analyze_fixture("unreachable-barrel-subtree");
    let unused_file_names = unused_file_names(&results);

    assert!(
        unused_file_names.contains(&"setup.ts".to_string()),
        "setup.ts should be flagged as the dead root, found: {unused_file_names:?}"
    );
    assert!(
        unused_file_names.contains(&"index.ts".to_string()),
        "the barrel index.ts should be flagged inside the dead subtree, found: {unused_file_names:?}"
    );
    assert!(
        unused_file_names.contains(&"helpers.ts".to_string()),
        "helpers.ts should be flagged even though it is re-exported by a dead barrel, found: {unused_file_names:?}"
    );
}

#[test]
fn unreachable_dynamic_subtree_flags_lazy_child_file() {
    let results = analyze_fixture("unreachable-dynamic-subtree");
    let unused_file_names = unused_file_names(&results);

    assert!(
        unused_file_names.contains(&"setup.ts".to_string()),
        "setup.ts should be flagged as the dead root, found: {unused_file_names:?}"
    );
    assert!(
        unused_file_names.contains(&"lazy.ts".to_string()),
        "lazy.ts should be flagged even though a dead file imports it dynamically, found: {unused_file_names:?}"
    );
}

#[test]
fn unreachable_shared_child_stays_alive_when_reachable_parent_imports_it() {
    let results = analyze_fixture("unreachable-shared-child");
    let unused_file_names = unused_file_names(&results);

    assert!(
        unused_file_names.contains(&"setup.ts".to_string()),
        "setup.ts should still be flagged as dead, found: {unused_file_names:?}"
    );
    assert!(
        unused_file_names.contains(&"helpers.ts".to_string()),
        "helpers.ts should be flagged because it is only imported inside the dead subtree, found: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"utils.ts".to_string()),
        "utils.ts should stay alive because a reachable file imports it, found: {unused_file_names:?}"
    );
}
