//! Shared output contracts for duplication action arrays.
//!
//! The duplication report body is assembled by API/CLI layers while clone
//! contracts live in `fallow-types`. These envelope DTOs stay engine-neutral
//! and are shared by schema emission, JSON output, and programmatic consumers.

use std::time::Duration;

use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, ToolVersion};
use fallow_types::output::NextStep;
use fallow_types::workspace::WorkspaceDiagnostic;
use serde::Serialize;

use crate::GroupByMode;
use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};

/// Envelope emitted by `fallow dupes --format json`.
///
/// `Report` and `Group` are generic so the envelope can live in
/// `fallow-output` while duplication report wrappers and grouped output
/// internals continue to migrate out of CLI/API-specific crates.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow dupes --format json"))]
pub struct DupesOutput<Report, Group> {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    #[serde(flatten)]
    pub report: Report,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouped_by: Option<GroupByMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_issues: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<Group>>,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed (always present in MCP responses).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Workspace-discovery diagnostics surfaced during config load
    /// (issue #473). See `CheckOutput::workspace_diagnostics` for the full
    /// contract; the same list is repeated on each top-level command's
    /// envelope so single-command consumers see it without having to look at
    /// a separate top-level field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// Read-only follow-up commands computed from this run's findings. See
    /// `CheckOutput::next_steps` for the contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Inputs for constructing a [`DupesOutput`] without exposing envelope assembly
/// details to callers.
#[derive(Debug, Clone)]
pub struct DupesOutputInput<Report, Group> {
    pub schema_version: u32,
    pub version: String,
    pub elapsed: Duration,
    pub report: Report,
    pub grouped_by: Option<GroupByMode>,
    pub total_issues: Option<usize>,
    pub groups: Option<Vec<Group>>,
    pub meta: Option<Meta>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub next_steps: Vec<NextStep>,
}

/// Build a duplication JSON envelope from caller-owned report data.
#[must_use]
pub fn build_dupes_output<Report, Group>(
    input: DupesOutputInput<Report, Group>,
) -> DupesOutput<Report, Group> {
    DupesOutput {
        schema_version: SchemaVersion(input.schema_version),
        version: ToolVersion(input.version),
        elapsed_ms: ElapsedMs(input.elapsed.as_millis() as u64),
        report: input.report,
        grouped_by: input.grouped_by,
        total_issues: input.total_issues,
        groups: input.groups,
        meta: input.meta,
        workspace_diagnostics: input.workspace_diagnostics,
        next_steps: input.next_steps,
    }
}

/// Serialize `fallow dupes --format json`.
///
/// # Errors
///
/// Returns a serde error when the duplication output cannot be converted to
/// JSON.
pub fn serialize_dupes_json_output<Report, Group>(
    output: DupesOutput<Report, Group>,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error>
where
    Report: Serialize,
    Group: Serialize,
{
    let mut value = serialize_named_json_output(output, "dupes", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Inline suppression comment emitted for code duplication findings.
pub const DUPES_SUPPRESS_COMMENT: &str = "// fallow-ignore-next-line code-duplication";

/// Shared description for the suppression action emitted on duplication findings.
pub const DUPES_SUPPRESS_DESCRIPTION: &str =
    "Suppress with an inline comment above the duplicated code";

/// Per-action wire shape attached to each `CloneGroupFinding` and
/// `AttributedCloneGroupFinding`. Mirrors the action types previously
/// emitted by `inject_dupes_actions::build_clone_group_actions` in
/// `crates/cli/src/report/json.rs`: `extract-shared` plus `suppress-line`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneGroupAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: CloneGroupActionType,
    /// Whether `fallow fix` can auto-apply this action. Both variants are
    /// manual today; the field is non-singleton so a future auto-applier
    /// does not need a schema change.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// The inline comment to insert (e.g.,
    /// `// fallow-ignore-next-line code-duplication`). Present on
    /// `suppress-line`; absent on `extract-shared`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Discriminant for [`CloneGroupAction::kind`]. Mirrors the action types
/// emitted by the legacy `build_clone_group_actions` walker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CloneGroupActionType {
    /// Extract the duplicated code into a shared function.
    ExtractShared,
    /// Suppress the finding with an inline comment above the duplicated code.
    SuppressLine,
}

/// Per-action wire shape attached to each `CloneFamilyFinding`. Mirrors
/// the action types previously emitted by
/// `build_clone_family_actions`: `extract-shared`, one `apply-suggestion`
/// per `RefactoringSuggestion` on the family, and a trailing
/// `suppress-line`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneFamilyAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: CloneFamilyActionType,
    /// Whether `fallow fix` can auto-apply this action. All three variants
    /// are manual today.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// Additional context. Present on `extract-shared` (explaining that
    /// the family's clone groups share the same files); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The inline comment to insert (e.g.,
    /// `// fallow-ignore-next-line code-duplication`). Present on
    /// `suppress-line` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Discriminant for [`CloneFamilyAction::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CloneFamilyActionType {
    /// Extract the duplicated code blocks into a shared module.
    ExtractShared,
    /// Apply one of the family's refactoring suggestions.
    ApplySuggestion,
    /// Suppress with an inline comment above the duplicated code.
    SuppressLine,
}

/// Build the stable action list for one clone group.
#[must_use]
pub fn clone_group_actions(line_count: usize, instance_count: usize) -> Vec<CloneGroupAction> {
    vec![
        CloneGroupAction {
            kind: CloneGroupActionType::ExtractShared,
            auto_fixable: false,
            description: format!(
                "Extract duplicated code ({line_count} lines, {instance_count} instance{}) into a shared function",
                if instance_count == 1 { "" } else { "s" },
            ),
            comment: None,
        },
        CloneGroupAction {
            kind: CloneGroupActionType::SuppressLine,
            auto_fixable: false,
            description: DUPES_SUPPRESS_DESCRIPTION.to_string(),
            comment: Some(DUPES_SUPPRESS_COMMENT.to_string()),
        },
    ]
}

/// Build the stable action list for a clone family.
#[must_use]
pub fn clone_family_actions<'a>(
    group_count: usize,
    total_duplicated_lines: usize,
    suggestion_descriptions: impl IntoIterator<Item = &'a str>,
) -> Vec<CloneFamilyAction> {
    let suggestions = suggestion_descriptions.into_iter();
    let (lower, _) = suggestions.size_hint();
    let mut actions = Vec::with_capacity(2 + lower);
    actions.push(CloneFamilyAction {
        kind: CloneFamilyActionType::ExtractShared,
        auto_fixable: false,
        description: format!(
            "Extract {group_count} duplicated code block{} ({total_duplicated_lines} lines) into a shared module",
            if group_count == 1 { "" } else { "s" },
        ),
        note: Some(
            "These clone groups share the same files, indicating a structural relationship; refactor together"
                .to_string(),
        ),
        comment: None,
    });
    for description in suggestions {
        actions.push(CloneFamilyAction {
            kind: CloneFamilyActionType::ApplySuggestion,
            auto_fixable: false,
            description: description.to_string(),
            note: None,
            comment: None,
        });
    }
    actions.push(CloneFamilyAction {
        kind: CloneFamilyActionType::SuppressLine,
        auto_fixable: false,
        description: DUPES_SUPPRESS_DESCRIPTION.to_string(),
        note: None,
        comment: Some(DUPES_SUPPRESS_COMMENT.to_string()),
    });
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dupes_json_output_uses_output_owned_root_contract() {
        let output = build_dupes_output(DupesOutputInput::<_, serde_json::Value> {
            schema_version: 7,
            version: "0.0.0".to_string(),
            elapsed: Duration::from_millis(5),
            report: json!({"stats": {"clone_groups": 0}}),
            grouped_by: None,
            total_issues: None,
            groups: None,
            meta: None,
            workspace_diagnostics: Vec::new(),
            next_steps: Vec::new(),
        });

        let value =
            serialize_dupes_json_output(output, RootEnvelopeMode::Tagged, Some("run-dupes"))
                .expect("dupes output should serialize");

        assert_eq!(value["kind"], "dupes");
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-dupes");
    }

    #[test]
    fn clone_group_actions_keep_primary_then_suppression_order() {
        let actions = clone_group_actions(20, 2);
        assert_eq!(actions[0].kind, CloneGroupActionType::ExtractShared);
        assert_eq!(actions[1].kind, CloneGroupActionType::SuppressLine);
        assert_eq!(actions[1].comment.as_deref(), Some(DUPES_SUPPRESS_COMMENT));
    }

    #[test]
    fn clone_family_actions_insert_suggestions_between_primary_and_suppression() {
        let actions = clone_family_actions(2, 40, ["Move to shared parser"]);
        assert_eq!(actions[0].kind, CloneFamilyActionType::ExtractShared);
        assert_eq!(actions[1].kind, CloneFamilyActionType::ApplySuggestion);
        assert_eq!(actions[1].description, "Move to shared parser");
        assert_eq!(actions[2].kind, CloneFamilyActionType::SuppressLine);
    }
}
