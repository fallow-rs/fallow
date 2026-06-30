use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use fallow_types::envelope::{
    BaselineDeltas, BaselineMatch, CheckSummary, ElapsedMs, EntryPoints, Meta, RegressionResult,
    SchemaVersion, ToolVersion,
};
use fallow_types::output::{IssueAction, NextStep};
use fallow_types::output_health::{HealthFindingAction, HealthFindingActionType};
use fallow_types::results::AnalysisResults;
use fallow_types::workspace::WorkspaceDiagnostic;
use serde::Serialize;

use crate::HealthReport;
use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};

/// Current schema version for the dead-code/check JSON envelope.
pub const CHECK_SCHEMA_VERSION: u32 = 7;

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
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
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
    pub meta: Option<Meta>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub next_steps: Vec<NextStep>,
}

/// Build the typed dead-code JSON envelope from engine results.
#[must_use]
pub fn build_check_output(input: CheckOutputInput) -> CheckOutput {
    let mut results = input.results;
    apply_config_fixable_to_duplicate_exports(&mut results, input.config_fixable);
    harmonize_multi_kind_suppress_line_actions(&mut results);
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
        meta: input.meta,
        workspace_diagnostics: input.workspace_diagnostics,
        next_steps: input.next_steps,
    }
}

fn serialize_check_family_json_output<T: Serialize>(
    output: T,
    kind: &'static str,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, kind, mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Serialize `fallow dead-code --format json`.
///
/// # Errors
///
/// Returns a serde error when the dead-code output cannot be converted to JSON.
pub fn serialize_check_json_output(
    output: CheckOutput,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_check_family_json_output(output, "dead-code", mode, analysis_run_id)
}

/// Serialize `fallow dead-code --group-by ... --format json`.
///
/// # Errors
///
/// Returns a serde error when the grouped dead-code output cannot be converted
/// to JSON.
pub fn serialize_check_grouped_json_output(
    output: CheckGroupedOutput,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_check_family_json_output(output, "dead-code-grouped", mode, analysis_run_id)
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

type SuppressAnchor = (String, u32);

macro_rules! visit_suppress_line_findings {
    ($results:expr, $visit:expr) => {{
        let results = $results;
        for finding in &results.unused_exports {
            $visit(&finding.export.path, finding.export.line, &finding.actions);
        }
        for finding in &results.unused_types {
            $visit(&finding.export.path, finding.export.line, &finding.actions);
        }
        for finding in &results.private_type_leaks {
            $visit(&finding.leak.path, finding.leak.line, &finding.actions);
        }
        for finding in &results.unused_enum_members {
            $visit(&finding.member.path, finding.member.line, &finding.actions);
        }
        for finding in &results.unused_class_members {
            $visit(&finding.member.path, finding.member.line, &finding.actions);
        }
        for finding in &results.unused_store_members {
            $visit(&finding.member.path, finding.member.line, &finding.actions);
        }
        for finding in &results.unresolved_imports {
            $visit(&finding.import.path, finding.import.line, &finding.actions);
        }
        for finding in &results.unused_dependencies {
            $visit(&finding.dep.path, finding.dep.line, &finding.actions);
        }
        for finding in &results.unused_dev_dependencies {
            $visit(&finding.dep.path, finding.dep.line, &finding.actions);
        }
        for finding in &results.unused_optional_dependencies {
            $visit(&finding.dep.path, finding.dep.line, &finding.actions);
        }
        for finding in &results.type_only_dependencies {
            $visit(&finding.dep.path, finding.dep.line, &finding.actions);
        }
        for finding in &results.test_only_dependencies {
            $visit(&finding.dep.path, finding.dep.line, &finding.actions);
        }
        for finding in &results.circular_dependencies {
            if let Some(path) = finding.cycle.files.first() {
                $visit(path, finding.cycle.line, &finding.actions);
            }
        }
        for finding in &results.boundary_violations {
            $visit(
                &finding.violation.from_path,
                finding.violation.line,
                &finding.actions,
            );
        }
        for finding in &results.boundary_coverage_violations {
            $visit(
                &finding.violation.path,
                finding.violation.line,
                &finding.actions,
            );
        }
        for finding in &results.boundary_call_violations {
            $visit(
                &finding.violation.path,
                finding.violation.line,
                &finding.actions,
            );
        }
        for finding in &results.policy_violations {
            $visit(
                &finding.violation.path,
                finding.violation.line,
                &finding.actions,
            );
        }
        for finding in &results.unused_catalog_entries {
            $visit(&finding.entry.path, finding.entry.line, &finding.actions);
        }
        for finding in &results.empty_catalog_groups {
            $visit(&finding.group.path, finding.group.line, &finding.actions);
        }
        for finding in &results.unresolved_catalog_references {
            $visit(
                &finding.reference.path,
                finding.reference.line,
                &finding.actions,
            );
        }
        for finding in &results.unused_dependency_overrides {
            $visit(&finding.entry.path, finding.entry.line, &finding.actions);
        }
        for finding in &results.misconfigured_dependency_overrides {
            $visit(&finding.entry.path, finding.entry.line, &finding.actions);
        }
        for finding in &results.invalid_client_exports {
            $visit(&finding.export.path, finding.export.line, &finding.actions);
        }
        for finding in &results.mixed_client_server_barrels {
            $visit(&finding.barrel.path, finding.barrel.line, &finding.actions);
        }
        for finding in &results.misplaced_directives {
            $visit(
                &finding.directive_site.path,
                finding.directive_site.line,
                &finding.actions,
            );
        }
        for finding in &results.unprovided_injects {
            $visit(&finding.inject.path, finding.inject.line, &finding.actions);
        }
        for finding in &results.unrendered_components {
            $visit(
                &finding.component.path,
                finding.component.line,
                &finding.actions,
            );
        }
        for finding in &results.route_collisions {
            $visit(
                &finding.collision.path,
                finding.collision.line,
                &finding.actions,
            );
        }
        for finding in &results.dynamic_segment_name_conflicts {
            $visit(
                &finding.conflict.path,
                finding.conflict.line,
                &finding.actions,
            );
        }
        for finding in &results.unused_component_props {
            $visit(&finding.prop.path, finding.prop.line, &finding.actions);
        }
        for finding in &results.unused_component_emits {
            $visit(&finding.emit.path, finding.emit.line, &finding.actions);
        }
        for finding in &results.unused_component_inputs {
            $visit(&finding.input.path, finding.input.line, &finding.actions);
        }
        for finding in &results.unused_component_outputs {
            $visit(&finding.output.path, finding.output.line, &finding.actions);
        }
        for finding in &results.unused_svelte_events {
            $visit(&finding.event.path, finding.event.line, &finding.actions);
        }
        for finding in &results.unused_server_actions {
            $visit(&finding.action.path, finding.action.line, &finding.actions);
        }
        for finding in &results.unused_load_data_keys {
            $visit(&finding.key.path, finding.key.line, &finding.actions);
        }
        for finding in &results.prop_drilling_chains {
            if let Some(hop) = finding.chain.hops.first() {
                $visit(&hop.file, hop.line, &finding.actions);
            }
        }
        for finding in &results.thin_wrappers {
            $visit(
                &finding.wrapper.file,
                finding.wrapper.line,
                &finding.actions,
            );
        }
        for finding in &results.duplicate_prop_shapes {
            $visit(&finding.shape.file, finding.shape.line, &finding.actions);
        }
    }};
}

macro_rules! visit_suppress_line_findings_mut {
    ($results:expr, $visit:expr) => {{
        let results = $results;
        for finding in &mut results.unused_exports {
            $visit(
                &finding.export.path,
                finding.export.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_types {
            $visit(
                &finding.export.path,
                finding.export.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.private_type_leaks {
            $visit(&finding.leak.path, finding.leak.line, &mut finding.actions);
        }
        for finding in &mut results.unused_enum_members {
            $visit(
                &finding.member.path,
                finding.member.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_class_members {
            $visit(
                &finding.member.path,
                finding.member.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_store_members {
            $visit(
                &finding.member.path,
                finding.member.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unresolved_imports {
            $visit(
                &finding.import.path,
                finding.import.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_dependencies {
            $visit(&finding.dep.path, finding.dep.line, &mut finding.actions);
        }
        for finding in &mut results.unused_dev_dependencies {
            $visit(&finding.dep.path, finding.dep.line, &mut finding.actions);
        }
        for finding in &mut results.unused_optional_dependencies {
            $visit(&finding.dep.path, finding.dep.line, &mut finding.actions);
        }
        for finding in &mut results.type_only_dependencies {
            $visit(&finding.dep.path, finding.dep.line, &mut finding.actions);
        }
        for finding in &mut results.test_only_dependencies {
            $visit(&finding.dep.path, finding.dep.line, &mut finding.actions);
        }
        for finding in &mut results.circular_dependencies {
            if let Some(path) = finding.cycle.files.first() {
                $visit(path, finding.cycle.line, &mut finding.actions);
            }
        }
        for finding in &mut results.boundary_violations {
            $visit(
                &finding.violation.from_path,
                finding.violation.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.boundary_coverage_violations {
            $visit(
                &finding.violation.path,
                finding.violation.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.boundary_call_violations {
            $visit(
                &finding.violation.path,
                finding.violation.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.policy_violations {
            $visit(
                &finding.violation.path,
                finding.violation.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_catalog_entries {
            $visit(
                &finding.entry.path,
                finding.entry.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.empty_catalog_groups {
            $visit(
                &finding.group.path,
                finding.group.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unresolved_catalog_references {
            $visit(
                &finding.reference.path,
                finding.reference.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_dependency_overrides {
            $visit(
                &finding.entry.path,
                finding.entry.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.misconfigured_dependency_overrides {
            $visit(
                &finding.entry.path,
                finding.entry.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.invalid_client_exports {
            $visit(
                &finding.export.path,
                finding.export.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.mixed_client_server_barrels {
            $visit(
                &finding.barrel.path,
                finding.barrel.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.misplaced_directives {
            $visit(
                &finding.directive_site.path,
                finding.directive_site.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unprovided_injects {
            $visit(
                &finding.inject.path,
                finding.inject.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unrendered_components {
            $visit(
                &finding.component.path,
                finding.component.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.route_collisions {
            $visit(
                &finding.collision.path,
                finding.collision.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.dynamic_segment_name_conflicts {
            $visit(
                &finding.conflict.path,
                finding.conflict.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_component_props {
            $visit(&finding.prop.path, finding.prop.line, &mut finding.actions);
        }
        for finding in &mut results.unused_component_emits {
            $visit(&finding.emit.path, finding.emit.line, &mut finding.actions);
        }
        for finding in &mut results.unused_component_inputs {
            $visit(
                &finding.input.path,
                finding.input.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_component_outputs {
            $visit(
                &finding.output.path,
                finding.output.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_svelte_events {
            $visit(
                &finding.event.path,
                finding.event.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_server_actions {
            $visit(
                &finding.action.path,
                finding.action.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.unused_load_data_keys {
            $visit(&finding.key.path, finding.key.line, &mut finding.actions);
        }
        for finding in &mut results.prop_drilling_chains {
            if let Some(hop) = finding.chain.hops.first() {
                $visit(&hop.file, hop.line, &mut finding.actions);
            }
        }
        for finding in &mut results.thin_wrappers {
            $visit(
                &finding.wrapper.file,
                finding.wrapper.line,
                &mut finding.actions,
            );
        }
        for finding in &mut results.duplicate_prop_shapes {
            $visit(
                &finding.shape.file,
                finding.shape.line,
                &mut finding.actions,
            );
        }
    }};
}

/// Merge same-line suppress actions so multi-kind findings share one comment.
///
/// This runs on typed `AnalysisResults` before serialization. It replaces the
/// older JSON-object walk for normal check output and keeps the action contract
/// owned by the output builders.
pub fn harmonize_multi_kind_suppress_line_actions(results: &mut AnalysisResults) {
    let mut anchors: BTreeMap<SuppressAnchor, Vec<String>> = BTreeMap::new();
    collect_dead_code_suppress_line_anchors(results, &mut anchors);
    retain_multi_kind_anchors(&mut anchors);
    if anchors.is_empty() {
        return;
    }
    rewrite_dead_code_suppress_line_actions(results, &anchors);
}

/// Merge same-line suppress actions across dead-code and health sections.
///
/// Combined and audit output can surface both dead-code and complexity findings
/// anchored to the same source line. This keeps the single-line suppress hint
/// typed until the final JSON serialization step.
pub fn harmonize_dead_code_health_suppress_line_actions(
    dead_code: Option<&mut AnalysisResults>,
    health: Option<&mut HealthReport>,
) {
    let mut anchors: BTreeMap<SuppressAnchor, Vec<String>> = BTreeMap::new();
    if let Some(results) = dead_code.as_deref() {
        collect_dead_code_suppress_line_anchors(results, &mut anchors);
    }
    if let Some(report) = health.as_deref() {
        collect_health_suppress_line_anchors(report, &mut anchors);
    }

    retain_multi_kind_anchors(&mut anchors);
    if anchors.is_empty() {
        return;
    }

    if let Some(results) = dead_code {
        rewrite_dead_code_suppress_line_actions(results, &anchors);
    }
    if let Some(report) = health {
        rewrite_health_suppress_line_actions(report, &anchors);
    }
}

fn retain_multi_kind_anchors(anchors: &mut BTreeMap<SuppressAnchor, Vec<String>>) {
    anchors.retain(|_, kinds| {
        sort_suppression_kinds(kinds);
        kinds.dedup();
        kinds.len() > 1
    });
}

fn collect_dead_code_suppress_line_anchors(
    results: &AnalysisResults,
    anchors: &mut BTreeMap<SuppressAnchor, Vec<String>>,
) {
    visit_suppress_line_findings!(results, |path: &Path, line, actions: &[IssueAction]| {
        collect_action_kinds(path, line, actions, anchors);
    });
}

fn rewrite_dead_code_suppress_line_actions(
    results: &mut AnalysisResults,
    anchors: &BTreeMap<SuppressAnchor, Vec<String>>,
) {
    visit_suppress_line_findings_mut!(
        results,
        |path: &Path, line, actions: &mut Vec<IssueAction>| {
            let anchor = suppress_anchor(path, line);
            if let Some(kinds) = anchors.get(&anchor) {
                let comment = format!("// fallow-ignore-next-line {}", kinds.join(", "));
                rewrite_action_comments(actions, &comment);
            }
        }
    );
}

fn collect_health_suppress_line_anchors(
    report: &HealthReport,
    anchors: &mut BTreeMap<SuppressAnchor, Vec<String>>,
) {
    for finding in &report.findings {
        collect_health_action_kinds(
            &finding.violation.path,
            finding.violation.line,
            &finding.actions,
            anchors,
        );
    }
    for finding in &report.prop_drilling_chains {
        if let Some(hop) = finding.chain.hops.first() {
            collect_action_kinds(&hop.file, hop.line, &finding.actions, anchors);
        }
    }
}

fn rewrite_health_suppress_line_actions(
    report: &mut HealthReport,
    anchors: &BTreeMap<SuppressAnchor, Vec<String>>,
) {
    for finding in &mut report.findings {
        let anchor = suppress_anchor(&finding.violation.path, finding.violation.line);
        if let Some(kinds) = anchors.get(&anchor) {
            let comment = format!("// fallow-ignore-next-line {}", kinds.join(", "));
            rewrite_health_action_comments(&mut finding.actions, &comment);
        }
    }
    for finding in &mut report.prop_drilling_chains {
        if let Some(hop) = finding.chain.hops.first() {
            let anchor = suppress_anchor(&hop.file, hop.line);
            if let Some(kinds) = anchors.get(&anchor) {
                let comment = format!("// fallow-ignore-next-line {}", kinds.join(", "));
                rewrite_action_comments(&mut finding.actions, &comment);
            }
        }
    }
}

fn collect_action_kinds(
    path: &Path,
    line: u32,
    actions: &[IssueAction],
    anchors: &mut BTreeMap<SuppressAnchor, Vec<String>>,
) {
    for action in actions {
        if let Some(comment) = suppress_line_comment(action) {
            let kinds = anchors.entry(suppress_anchor(path, line)).or_default();
            for kind in parse_suppress_line_comment(comment) {
                if !kinds.iter().any(|existing| existing == &kind) {
                    kinds.push(kind);
                }
            }
        }
    }
}

fn collect_health_action_kinds(
    path: &Path,
    line: u32,
    actions: &[HealthFindingAction],
    anchors: &mut BTreeMap<SuppressAnchor, Vec<String>>,
) {
    for action in actions {
        if let Some(comment) = health_suppress_line_comment(action) {
            let kinds = anchors.entry(suppress_anchor(path, line)).or_default();
            for kind in parse_suppress_line_comment(comment) {
                if !kinds.iter().any(|existing| existing == &kind) {
                    kinds.push(kind);
                }
            }
        }
    }
}

fn rewrite_action_comments(actions: &mut [IssueAction], comment: &str) {
    for action in actions {
        if let IssueAction::SuppressLine(suppress) = action {
            suppress.comment = comment.to_string();
        }
    }
}

fn rewrite_health_action_comments(actions: &mut [HealthFindingAction], comment: &str) {
    for action in actions {
        if matches!(action.kind, HealthFindingActionType::SuppressLine) {
            action.comment = Some(comment.to_string());
        }
    }
}

fn suppress_anchor(path: &Path, line: u32) -> SuppressAnchor {
    (path.display().to_string(), line)
}

fn suppress_line_comment(action: &IssueAction) -> Option<&str> {
    match action {
        IssueAction::SuppressLine(action) => Some(&action.comment),
        _ => None,
    }
}

fn health_suppress_line_comment(action: &HealthFindingAction) -> Option<&str> {
    matches!(action.kind, HealthFindingActionType::SuppressLine)
        .then_some(())
        .and(action.comment.as_deref())
}

fn parse_suppress_line_comment(comment: &str) -> Vec<String> {
    comment
        .strip_prefix("// fallow-ignore-next-line ")
        .map(|rest| {
            rest.split(|c: char| c == ',' || c.is_whitespace())
                .filter(|token| !token.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn sort_suppression_kinds(kinds: &mut [String]) {
    kinds.sort_by_key(|kind| suppression_kind_rank(kind));
}

fn suppression_kind_rank(kind: &str) -> usize {
    match kind {
        "unused-file" => 0,
        "unused-export" => 1,
        "unused-type" => 2,
        "private-type-leak" => 3,
        "unused-enum-member" => 4,
        "unused-class-member" => 5,
        "unused-store-member" => 6,
        "unresolved-import" => 7,
        "unlisted-dependency" => 8,
        "duplicate-export" => 9,
        "circular-dependency" => 10,
        "re-export-cycle" => 11,
        "boundary-violation" => 12,
        "code-duplication" => 13,
        "complexity" => 14,
        "unprovided-inject" => 15,
        "unrendered-component" => 16,
        "unused-server-action" => 17,
        _ => usize::MAX,
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
    use crate::{ComplexityViolation, ExceededThreshold, FindingSeverity, HealthFinding};
    use fallow_types::output_dead_code::{
        UnusedExportFinding, UnusedFileFinding, UnusedTypeFinding,
    };
    use fallow_types::results::{UnusedExport, UnusedFile};
    use fallow_types::workspace::WorkspaceDiagnosticKind;

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
            meta: None,
            workspace_diagnostics: Vec::new(),
            next_steps: Vec::new(),
        });

        assert_eq!(output.schema_version.0, 7);
        assert_eq!(output.total_issues, 1);
        assert_eq!(output.summary.unused_files, 1);
        assert_eq!(output.elapsed_ms.0, 42);
    }

    #[test]
    fn build_check_output_harmonizes_multi_kind_suppress_actions_typed() {
        let mut results = AnalysisResults::default();
        let path = std::path::PathBuf::from("/project/src/shared.ts");
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "value".to_string(),
                is_type_only: false,
                line: 7,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path,
                export_name: "TypeOnly".to_string(),
                is_type_only: true,
                line: 7,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let output = build_check_output(CheckOutputInput {
            schema_version: 7,
            version: "0.0.0".to_string(),
            elapsed: Duration::from_millis(42),
            results,
            config_fixable: false,
            meta: None,
            workspace_diagnostics: Vec::new(),
            next_steps: Vec::new(),
        });

        let export_comment = suppress_comment(&output.results.unused_exports[0].actions);
        let type_comment = suppress_comment(&output.results.unused_types[0].actions);
        assert_eq!(
            export_comment,
            Some("// fallow-ignore-next-line unused-export, unused-type")
        );
        assert_eq!(type_comment, export_comment);
    }

    #[test]
    fn harmonize_dead_code_health_suppress_actions_typed() {
        let mut results = AnalysisResults::default();
        let path = std::path::PathBuf::from("/project/src/shared.ts");
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "value".to_string(),
                is_type_only: false,
                line: 7,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let mut health = HealthReport {
            findings: vec![HealthFinding::new(
                ComplexityViolation {
                    path,
                    name: "expensive".to_string(),
                    line: 7,
                    col: 0,
                    cyclomatic: 22,
                    cognitive: 18,
                    line_count: 40,
                    param_count: 1,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
                    exceeded: ExceededThreshold::Both,
                    severity: FindingSeverity::High,
                    crap: None,
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                },
                vec![HealthFindingAction {
                    kind: HealthFindingActionType::SuppressLine,
                    auto_fixable: false,
                    description: "Suppress with an inline comment above the function declaration"
                        .to_string(),
                    note: None,
                    comment: Some("// fallow-ignore-next-line complexity".to_string()),
                    placement: Some("above-function-declaration".to_string()),
                    target_path: None,
                }],
                None,
            )],
            ..HealthReport::default()
        };

        harmonize_dead_code_health_suppress_line_actions(Some(&mut results), Some(&mut health));

        assert_eq!(
            suppress_comment(&results.unused_exports[0].actions),
            Some("// fallow-ignore-next-line unused-export, complexity")
        );
        assert_eq!(
            health.findings[0].actions[0].comment.as_deref(),
            Some("// fallow-ignore-next-line unused-export, complexity")
        );
    }

    #[test]
    fn check_json_output_uses_output_owned_root_contract() {
        let output = build_check_output(CheckOutputInput {
            schema_version: 7,
            version: "0.0.0".to_string(),
            elapsed: Duration::from_millis(42),
            results: AnalysisResults::default(),
            config_fixable: false,
            meta: None,
            workspace_diagnostics: Vec::new(),
            next_steps: Vec::new(),
        });

        let value =
            serialize_check_json_output(output, RootEnvelopeMode::Tagged, Some("run-check"))
                .expect("check output should serialize");

        assert_eq!(value["kind"], "dead-code");
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-check");
    }

    #[test]
    fn grouped_check_json_output_uses_output_owned_root_contract() {
        let output = CheckGroupedOutput {
            schema_version: SchemaVersion(7),
            version: ToolVersion("0.0.0".to_string()),
            elapsed_ms: ElapsedMs(1),
            grouped_by: GroupByMode::Directory,
            total_issues: 0,
            groups: Vec::new(),
            meta: None,
            next_steps: Vec::new(),
        };

        let value = serialize_check_grouped_json_output(
            output,
            RootEnvelopeMode::Tagged,
            Some("run-group"),
        )
        .expect("grouped check output should serialize");

        assert_eq!(value["kind"], "dead-code-grouped");
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-group");
    }

    #[test]
    fn workspace_diagnostics_serialize_typed_kind_path_message() {
        let root = std::path::Path::new("/project");
        let output = build_check_output(CheckOutputInput {
            schema_version: 7,
            version: "0.0.0".to_string(),
            elapsed: Duration::from_millis(1),
            results: AnalysisResults::default(),
            config_fixable: false,
            meta: None,
            workspace_diagnostics: vec![WorkspaceDiagnostic::new(
                root,
                root.join("packages/legacy"),
                WorkspaceDiagnosticKind::UndeclaredWorkspace,
            )],
            next_steps: Vec::new(),
        });

        let value = serde_json::to_value(&output).expect("check output serializes");
        let diag = &value["workspace_diagnostics"][0];
        assert_eq!(diag["kind"], "undeclared-workspace");
        assert!(
            diag["path"]
                .as_str()
                .is_some_and(|path| path.contains("packages/legacy")),
            "path field is carried verbatim: {diag}"
        );
        assert!(
            diag["message"]
                .as_str()
                .is_some_and(|message| message.contains("packages/legacy")),
            "message is rendered from kind + path: {diag}"
        );
    }

    fn suppress_comment(actions: &[IssueAction]) -> Option<&str> {
        actions.iter().find_map(|action| match action {
            IssueAction::SuppressLine(action) => Some(action.comment.as_str()),
            _ => None,
        })
    }
}
