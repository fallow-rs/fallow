use std::time::Duration;

use fallow_types::envelope::{
    BaselineDeltas, BaselineMatch, CheckSummary, ElapsedMs, EntryPoints, Meta, RegressionResult,
    SchemaVersion, ToolVersion,
};
use fallow_types::output::NextStep;
use fallow_types::results::AnalysisResults;
use serde::Serialize;

/// Serialized workspace-discovery diagnostic surfaced on check output.
///
/// The runtime diagnostic type currently lives in `fallow-config`, which is
/// upstream of `fallow-output`. This transparent DTO keeps the wire contract in
/// the output layer without introducing a crate cycle.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct WorkspaceDiagnosticOutput(pub serde_json::Value);

/// Envelope emitted by `fallow dead-code --format json` (plus the `check`
/// block inside the combined and audit envelopes).
///
/// The body is the full `AnalysisResults` flattened into the envelope so
/// every issue array (`unused_files`, `unused_exports`, ...) lives at the
/// top level, matching the existing wire shape. `entry_points` lifts the
/// otherwise `#[serde(skip)]`'d `AnalysisResults::entry_point_summary` back
/// into the JSON output. `summary` carries the per-category counts the
/// JSON layer always emits.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow dead-code --format json"))]
pub struct CheckOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub total_issues: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_points: Option<EntryPoints>,
    pub summary: CheckSummary,
    #[serde(flatten)]
    pub results: AnalysisResults,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_deltas: Option<BaselineDeltas>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regression: Option<RegressionResult>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnosticOutput>,
    /// Read-only follow-up commands computed from this run's findings, emitted
    /// at the JSON root so an agent acting on the output is pointed at fallow's
    /// adjacent verification capabilities (trace, complexity breakdown, audit,
    /// workspace scoping). Each command is runnable as-is and never mutating;
    /// see [`NextStep`] for both contracts. Omitted when empty or when
    /// `FALLOW_SUGGESTIONS=off`; does NOT contribute to `total_issues`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Envelope emitted by `fallow dead-code --group-by ... --format json`.
///
/// Issues are partitioned into resolver buckets (CODEOWNERS team, directory
/// prefix, workspace package, or GitLab CODEOWNERS section) instead of flat
/// arrays. Each bucket carries the same issue-array shape as the ungrouped
/// `CheckOutput` body, plus per-group `key` / `owners` / `total_issues`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(
        title = "fallow dead-code --group-by <owner|directory|package|section> --format json"
    )
)]
pub struct CheckGroupedOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub grouped_by: GroupByMode,
    pub total_issues: usize,
    pub groups: Vec<CheckGroupedEntry>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Read-only follow-up commands computed from the full (ungrouped) findings.
    /// See [`CheckOutput::next_steps`] for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Single resolver bucket inside `CheckGroupedOutput`. Carries the group's
/// identifier, optional section owners, and a per-group flattened
/// `AnalysisResults`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckGroupedEntry {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    pub total_issues: usize,
    #[serde(flatten)]
    pub results: AnalysisResults,
}

/// Resolver mode label for grouped envelopes (dead-code, dupes, health).
///
/// `owner` groups by CODEOWNERS team, `directory` groups by top-level
/// directory prefix, `package` groups by workspace package name, `section`
/// groups by GitLab CODEOWNERS `[Section]` header name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum GroupByMode {
    Owner,
    Directory,
    Package,
    Section,
}

/// Inputs for building the dead-code JSON envelope.
pub struct CheckOutputInput {
    pub schema_version: u32,
    pub version: String,
    pub elapsed: Duration,
    pub results: AnalysisResults,
    pub config_fixable: bool,
    pub workspace_diagnostics: Vec<WorkspaceDiagnosticOutput>,
    pub next_steps: Vec<NextStep>,
}

/// Build the typed dead-code JSON envelope from engine results.
#[must_use]
pub fn build_check_output(input: CheckOutputInput) -> CheckOutput {
    let mut results = input.results;
    apply_config_fixable_to_duplicate_exports(&mut results, input.config_fixable);
    CheckOutput {
        schema_version: SchemaVersion(input.schema_version),
        version: ToolVersion(input.version),
        elapsed_ms: ElapsedMs(input.elapsed.as_millis() as u64),
        total_issues: results.total_issues(),
        entry_points: results
            .entry_point_summary
            .as_ref()
            .map(|entry_points| EntryPoints {
                total: entry_points.total,
                sources: entry_points
                    .by_source
                    .iter()
                    .map(|(key, value)| (key.replace(' ', "_"), *value))
                    .collect(),
            }),
        summary: build_check_summary(&results),
        results,
        baseline_deltas: None,
        baseline: None,
        regression: None,
        meta: None,
        workspace_diagnostics: input.workspace_diagnostics,
        next_steps: input.next_steps,
    }
}

pub fn apply_config_fixable_to_duplicate_exports(
    results: &mut AnalysisResults,
    config_fixable: bool,
) {
    if !config_fixable {
        return;
    }
    for finding in &mut results.duplicate_exports {
        finding.set_config_fixable(true);
    }
}

/// Compute the per-category `CheckSummary` from analysis results.
#[must_use]
pub fn build_check_summary(results: &AnalysisResults) -> CheckSummary {
    CheckSummary {
        total_issues: results.total_issues(),
        unused_files: results.unused_files.len(),
        unused_exports: results.unused_exports.len(),
        unused_types: results.unused_types.len(),
        private_type_leaks: results.private_type_leaks.len(),
        unused_dependencies: results.unused_dependencies.len()
            + results.unused_dev_dependencies.len()
            + results.unused_optional_dependencies.len(),
        unused_enum_members: results.unused_enum_members.len(),
        unused_class_members: results.unused_class_members.len(),
        unused_store_members: results.unused_store_members.len(),
        unresolved_imports: results.unresolved_imports.len(),
        unlisted_dependencies: results.unlisted_dependencies.len(),
        duplicate_exports: results.duplicate_exports.len(),
        type_only_dependencies: results.type_only_dependencies.len(),
        test_only_dependencies: results.test_only_dependencies.len(),
        circular_dependencies: results.circular_dependencies.len(),
        re_export_cycles: results.re_export_cycles.len(),
        boundary_violations: results.boundary_violations.len(),
        boundary_coverage_violations: results.boundary_coverage_violations.len(),
        boundary_call_violations: results.boundary_call_violations.len(),
        policy_violations: results.policy_violations.len(),
        stale_suppressions: results.stale_suppressions.len(),
        unused_catalog_entries: results.unused_catalog_entries.len(),
        empty_catalog_groups: results.empty_catalog_groups.len(),
        unresolved_catalog_references: results.unresolved_catalog_references.len(),
        unused_dependency_overrides: results.unused_dependency_overrides.len(),
        misconfigured_dependency_overrides: results.misconfigured_dependency_overrides.len(),
        invalid_client_exports: results.invalid_client_exports.len(),
        mixed_client_server_barrels: results.mixed_client_server_barrels.len(),
        misplaced_directives: results.misplaced_directives.len(),
        unprovided_injects: results.unprovided_injects.len(),
        unrendered_components: results.unrendered_components.len(),
        unused_component_props: results.unused_component_props.len(),
        unused_component_emits: results.unused_component_emits.len(),
        unused_component_inputs: results.unused_component_inputs.len(),
        unused_component_outputs: results.unused_component_outputs.len(),
        unused_svelte_events: results.unused_svelte_events.len(),
        unused_server_actions: results.unused_server_actions.len(),
        unused_load_data_keys: results.unused_load_data_keys.len(),
        route_collisions: results.route_collisions.len(),
        dynamic_segment_name_conflicts: results.dynamic_segment_name_conflicts.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::output_dead_code::UnusedFileFinding;
    use fallow_types::results::UnusedFile;

    #[test]
    fn build_check_output_counts_issues_and_entry_points() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: "src/unused.ts".into(),
            }));

        let output = build_check_output(CheckOutputInput {
            schema_version: 7,
            version: "0.0.0".to_string(),
            elapsed: Duration::from_millis(42),
            results,
            config_fixable: false,
            workspace_diagnostics: Vec::new(),
            next_steps: Vec::new(),
        });

        assert_eq!(output.schema_version.0, 7);
        assert_eq!(output.total_issues, 1);
        assert_eq!(output.summary.unused_files, 1);
        assert_eq!(output.elapsed_ms.0, 42);
    }
}
