use std::path::Path;

use fallow_api::{
    EditorAnalysisResults as AnalysisResults, EditorDuplicationReport as DuplicationReport,
};
use fallow_types::issue_meta::{IssueKindMeta, diagnostic_issue_metas};
use ls_types::notification;
use serde::{Deserialize, Serialize};

/// Custom notification sent to the client after every analysis completes.
/// Carries summary stats so the extension can update the status bar, context
/// keys, and other UI without running a separate CLI process.
pub enum AnalysisComplete {}

impl notification::Notification for AnalysisComplete {
    type Params = AnalysisCompleteParams;
    const METHOD: &'static str = "fallow/analysisComplete";
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisCompleteParams {
    pub total_issues: usize,
    pub unused_files: usize,
    pub unused_exports: usize,
    pub unused_types: usize,
    pub private_type_leaks: usize,
    pub unused_dependencies: usize,
    pub unused_dev_dependencies: usize,
    pub unused_optional_dependencies: usize,
    pub unused_enum_members: usize,
    pub unused_class_members: usize,
    pub unused_store_members: usize,
    pub unprovided_injects: usize,
    pub unrendered_components: usize,
    pub unused_component_props: usize,
    pub unused_component_emits: usize,
    pub unused_component_inputs: usize,
    pub unused_component_outputs: usize,
    pub unused_svelte_events: usize,
    pub unused_server_actions: usize,
    pub unused_load_data_keys: usize,
    pub unresolved_imports: usize,
    pub unlisted_dependencies: usize,
    pub duplicate_exports: usize,
    pub type_only_dependencies: usize,
    pub test_only_dependencies: usize,
    pub circular_dependencies: usize,
    pub re_export_cycles: usize,
    pub boundary_violations: usize,
    pub stale_suppressions: usize,
    pub unused_catalog_entries: usize,
    pub empty_catalog_groups: usize,
    pub unresolved_catalog_references: usize,
    pub unused_dependency_overrides: usize,
    pub misconfigured_dependency_overrides: usize,
    pub duplication_percentage: f64,
    pub clone_groups: usize,
}

#[derive(Clone, Copy)]
pub struct AnalysisCompleteInput<'a> {
    results: &'a AnalysisResults,
    duplication: &'a DuplicationReport,
}

impl<'a> AnalysisCompleteInput<'a> {
    pub const fn new(results: &'a AnalysisResults, duplication: &'a DuplicationReport) -> Self {
        Self {
            results,
            duplication,
        }
    }
}

pub fn analysis_complete_params(input: AnalysisCompleteInput<'_>) -> AnalysisCompleteParams {
    let AnalysisCompleteInput {
        results,
        duplication,
    } = input;
    let boundary_violations = results.boundary_violations.len()
        + results.boundary_coverage_violations.len()
        + results.boundary_call_violations.len();
    AnalysisCompleteParams {
        total_issues: results.total_issues(),
        unused_files: results.unused_files.len(),
        unused_exports: results.unused_exports.len(),
        unused_types: results.unused_types.len(),
        private_type_leaks: results.private_type_leaks.len(),
        unused_dependencies: results.unused_dependencies.len(),
        unused_dev_dependencies: results.unused_dev_dependencies.len(),
        unused_optional_dependencies: results.unused_optional_dependencies.len(),
        unused_enum_members: results.unused_enum_members.len(),
        unused_class_members: results.unused_class_members.len(),
        unused_store_members: results.unused_store_members.len(),
        unprovided_injects: results.unprovided_injects.len(),
        unrendered_components: results.unrendered_components.len(),
        unused_component_props: results.unused_component_props.len(),
        unused_component_emits: results.unused_component_emits.len(),
        unused_component_inputs: results.unused_component_inputs.len(),
        unused_component_outputs: results.unused_component_outputs.len(),
        unused_svelte_events: results.unused_svelte_events.len(),
        unused_server_actions: results.unused_server_actions.len(),
        unused_load_data_keys: results.unused_load_data_keys.len(),
        unresolved_imports: results.unresolved_imports.len(),
        unlisted_dependencies: results.unlisted_dependencies.len(),
        duplicate_exports: results.duplicate_exports.len(),
        type_only_dependencies: results.type_only_dependencies.len(),
        test_only_dependencies: results.test_only_dependencies.len(),
        circular_dependencies: results.circular_dependencies.len(),
        re_export_cycles: results.re_export_cycles.len(),
        boundary_violations,
        stale_suppressions: results.stale_suppressions.len(),
        unused_catalog_entries: results.unused_catalog_entries.len(),
        empty_catalog_groups: results.empty_catalog_groups.len(),
        unresolved_catalog_references: results.unresolved_catalog_references.len(),
        unused_dependency_overrides: results.unused_dependency_overrides.len(),
        misconfigured_dependency_overrides: results.misconfigured_dependency_overrides.len(),
        duplication_percentage: duplication.stats.duplication_percentage,
        clone_groups: duplication.stats.clone_groups,
    }
}

#[cfg(test)]
pub fn analysis_complete_params_for_test(
    results: &AnalysisResults,
    duplication: &DuplicationReport,
) -> AnalysisCompleteParams {
    analysis_complete_params(AnalysisCompleteInput::new(results, duplication))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IssueTypeInfo {
    pub code: String,
    pub label: String,
}

pub fn diagnostic_issue_types() -> Vec<IssueTypeInfo> {
    diagnostic_issue_metas()
        .map(|meta| IssueTypeInfo {
            code: meta.code.to_string(),
            label: meta.label.to_string(),
        })
        .collect()
}

pub fn diagnostic_issue_type_metas() -> impl Iterator<Item = &'static IssueKindMeta> {
    diagnostic_issue_metas()
}

pub fn config_load_error_detail(
    project_root: &Path,
    explicit_config_path: Option<&Path>,
    err: impl std::fmt::Display,
) -> String {
    match explicit_config_path {
        Some(path) => format!(
            "fallow.configPath '{}' failed to load for {}: {err} (no diagnostics will be produced)",
            path.display(),
            project_root.display()
        ),
        None => format!("config error for {}: {err}", project_root.display()),
    }
}
