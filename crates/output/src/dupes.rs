//! Shared output contracts for duplication action arrays.
//!
//! The duplication report body still lives close to the CLI renderer because
//! it wraps clone types owned by `fallow-core`. These action DTOs are
//! core-independent and are shared by CLI schema emission, JSON output, and
//! future API/LSP consumers.

use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, ToolVersion};
use fallow_types::output::NextStep;
use serde::Serialize;

use crate::{GroupByMode, WorkspaceDiagnosticOutput};

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
    /// is passed.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnosticOutput>,
    /// Read-only follow-up commands computed from this run's findings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<NextStep>,
}

/// Inline suppression comment emitted for code duplication findings.
pub const DUPES_SUPPRESS_COMMENT: &str = "// fallow-ignore-next-line code-duplication";

/// Shared description for the suppression action emitted on duplication findings.
pub const DUPES_SUPPRESS_DESCRIPTION: &str =
    "Suppress with an inline comment above the duplicated code";

/// Per-action wire shape attached to each clone group finding.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneGroupAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: CloneGroupActionType,
    /// Whether `fallow fix` can auto-apply this action.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// Inline comment to insert for suppression actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Discriminant for [`CloneGroupAction::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CloneGroupActionType {
    /// Extract the duplicated code into a shared function.
    ExtractShared,
    /// Suppress the finding with an inline comment above the duplicated code.
    SuppressLine,
}

/// Per-action wire shape attached to each clone family finding.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneFamilyAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: CloneFamilyActionType,
    /// Whether `fallow fix` can auto-apply this action.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// Additional context for the action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Inline comment to insert for suppression actions.
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
