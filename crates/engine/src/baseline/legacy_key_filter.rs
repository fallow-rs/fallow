//! Filters new issues against the keys of a legacy baseline file.

use super::*;

pub(super) fn filter_new_issues_by_legacy_keys(
    mut results: crate::results::AnalysisResults,
    baseline: &BaselineData,
    root: &Path,
) -> crate::results::AnalysisResults {
    let baseline_files: FxHashSet<&str> =
        baseline.unused_files.iter().map(String::as_str).collect();
    let baseline_exports: FxHashSet<&str> =
        baseline.unused_exports.iter().map(String::as_str).collect();
    let baseline_types: FxHashSet<&str> =
        baseline.unused_types.iter().map(String::as_str).collect();
    let baseline_deps: FxHashSet<&str> = baseline
        .unused_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    let baseline_dev_deps: FxHashSet<&str> = baseline
        .unused_dev_dependencies
        .iter()
        .map(String::as_str)
        .collect();

    results
        .unused_files
        .retain(|f| !baseline_files.contains(relative_path(&f.file.path, root).as_str()));
    results.unused_exports.retain(|e| {
        let key = format!(
            "{}:{}",
            relative_path(&e.export.path, root),
            e.export.export_name
        );
        !baseline_exports.contains(key.as_str())
    });
    results.unused_types.retain(|e| {
        let key = format!(
            "{}:{}",
            relative_path(&e.export.path, root),
            e.export.export_name
        );
        !baseline_types.contains(key.as_str())
    });
    filter_private_type_leaks(
        &mut results.private_type_leaks,
        &baseline.private_type_leaks,
        root,
    );
    let baseline_deprecated: FxHashSet<&str> = baseline
        .deprecated_exports_in_use
        .iter()
        .map(String::as_str)
        .collect();
    results
        .deprecated_exports_in_use
        .retain(|e| !baseline_deprecated.contains(deprecated_export_key(&e.export, root).as_str()));
    results.unused_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.dep.package_name, &d.dep.path, root);
        !baseline_contains_dependency(&baseline_deps, &d.dep.package_name, key.as_str())
    });
    results.unused_dev_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.dep.package_name, &d.dep.path, root);
        !baseline_contains_dependency(&baseline_dev_deps, &d.dep.package_name, key.as_str())
    });

    let filter = BaselineFilterContext { baseline, root };
    filter.filter_cycles_and_members(&mut results);
    filter.filter_dependency_variants(&mut results);
    filter.filter_boundaries_and_suppressions(&mut results);
    filter.filter_pnpm_entries(&mut results);

    results
}
