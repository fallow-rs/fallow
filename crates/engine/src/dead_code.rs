//! Dead-code result helpers exposed through the engine boundary.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use fallow_config::ResolvedConfig;

pub use crate::results::{
    AnalysisResults, DeadCodeAnalysis, DeadCodeAnalysisArtifacts, DeadCodeAnalysisOutput,
    DeadCodeAnalysisWithHashes, derive_security_severity, security_catalogue_title,
};

use crate::{
    EngineResult, session::analyze_dead_code_with_parse_result_from_config, source::ModuleInfo,
};

/// Run dead-code analysis from pre-parsed modules.
///
/// # Errors
///
/// Returns an error if discovery, graph construction, or analysis fails.
pub(crate) fn analyze_with_parse_result(
    config: &ResolvedConfig,
    modules: &[ModuleInfo],
) -> EngineResult<DeadCodeAnalysisArtifacts> {
    analyze_dead_code_with_parse_result_from_config(config, modules)
}

/// Scope dead-code results to the union of the given workspace roots.
///
/// The full cross-workspace graph is still built before this helper runs, so
/// cross-package imports are resolved. Only reported findings are narrowed.
pub fn filter_to_workspaces(results: &mut AnalysisResults, ws_roots: &[PathBuf]) {
    let any_under = |path: &Path| ws_roots.iter().any(|root| path.starts_with(root));
    let pkg_jsons = ws_roots
        .iter()
        .map(|root| root.join("package.json"))
        .collect::<Vec<_>>();
    let in_pkg_jsons = |path: &Path| pkg_jsons.iter().any(|pkg| path == pkg);

    filter_workspace_source_findings(results, &any_under);
    filter_workspace_dependency_findings(results, &any_under, &in_pkg_jsons);
    filter_workspace_graph_findings(results, &any_under);
    filter_workspace_policy_findings(results, &any_under);
}

/// Scope dead-code results to findings affected by changed files.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn filter_by_changed_files(results: &mut AnalysisResults, changed_files: &FxHashSet<PathBuf>) {
    crate::changed_files::filter_results_by_changed_files(results, changed_files);
}

fn filter_workspace_source_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    results
        .unused_files
        .retain(|finding| any_under(&finding.file.path));
    results
        .unused_exports
        .retain(|finding| any_under(&finding.export.path));
    results
        .unused_types
        .retain(|finding| any_under(&finding.export.path));
    results
        .private_type_leaks
        .retain(|finding| any_under(&finding.leak.path));
    results
        .unused_enum_members
        .retain(|finding| any_under(&finding.member.path));
    results
        .unused_class_members
        .retain(|finding| any_under(&finding.member.path));
    results
        .unused_store_members
        .retain(|finding| any_under(&finding.member.path));
    results
        .unprovided_injects
        .retain(|finding| any_under(&finding.inject.path));
    results
        .unrendered_components
        .retain(|finding| any_under(&finding.component.path));
    results
        .unused_component_props
        .retain(|finding| any_under(&finding.prop.path));
    results
        .unused_component_emits
        .retain(|finding| any_under(&finding.emit.path));
    results
        .unused_component_inputs
        .retain(|finding| any_under(&finding.input.path));
    results
        .unused_component_outputs
        .retain(|finding| any_under(&finding.output.path));
    results
        .unused_svelte_events
        .retain(|finding| any_under(&finding.event.path));
    results
        .unused_server_actions
        .retain(|finding| any_under(&finding.action.path));
    results
        .unused_load_data_keys
        .retain(|finding| any_under(&finding.key.path));
    results
        .unresolved_imports
        .retain(|finding| any_under(&finding.import.path));
}

fn filter_workspace_dependency_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
    in_pkg_jsons: &dyn Fn(&Path) -> bool,
) {
    results
        .unused_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .unused_dev_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .unused_optional_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .type_only_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .test_only_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .dev_dependencies_in_production
        .retain(|finding| in_pkg_jsons(&finding.dep.path));

    results.unlisted_dependencies.retain(|finding| {
        finding
            .dep
            .imported_from
            .iter()
            .any(|source| any_under(&source.path))
    });
    results.unused_dependency_overrides.clear();
    results.misconfigured_dependency_overrides.clear();
}

fn filter_workspace_graph_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    for duplicate in &mut results.duplicate_exports {
        duplicate
            .export
            .locations
            .retain(|location| any_under(&location.path));
    }
    results
        .duplicate_exports
        .retain(|duplicate| duplicate.export.locations.len() >= 2);

    results
        .circular_dependencies
        .retain(|cycle| cycle.cycle.files.iter().any(|path| any_under(path)));

    results
        .re_export_cycles
        .retain(|cycle| cycle.cycle.files.iter().any(|path| any_under(path)));
}

fn filter_workspace_policy_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    results
        .boundary_violations
        .retain(|finding| any_under(&finding.violation.from_path));
    results
        .boundary_coverage_violations
        .retain(|finding| any_under(&finding.violation.path));
    results
        .boundary_call_violations
        .retain(|finding| any_under(&finding.violation.path));
    results
        .policy_violations
        .retain(|finding| any_under(&finding.violation.path));

    results
        .stale_suppressions
        .retain(|finding| any_under(&finding.path));

    results
        .security_findings
        .retain(|finding| any_under(&finding.path));
    results
        .security_unresolved_callee_diagnostics
        .retain(|finding| any_under(&finding.path));

    results.unused_catalog_entries.clear();
    results.empty_catalog_groups.clear();
    results
        .unresolved_catalog_references
        .retain(|finding| any_under(&finding.reference.path));

    results
        .invalid_client_exports
        .retain(|finding| any_under(&finding.export.path));

    results
        .mixed_client_server_barrels
        .retain(|finding| any_under(&finding.barrel.path));

    results
        .misplaced_directives
        .retain(|finding| any_under(&finding.directive_site.path));

    results
        .route_collisions
        .retain(|finding| any_under(&finding.collision.path));

    results
        .dynamic_segment_name_conflicts
        .retain(|finding| any_under(&finding.conflict.path));
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use fallow_types::output_dead_code::UnusedFileFinding;
    use fallow_types::results::UnusedFile;

    #[test]
    fn workspace_filter_keeps_findings_under_workspace_root() {
        let root = PathBuf::from("/repo/packages/app");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/unused.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/repo/packages/docs/src/unused.ts"),
            }));

        filter_to_workspaces(&mut results, std::slice::from_ref(&root));

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].file.path,
            root.join("src/unused.ts")
        );
    }
}
