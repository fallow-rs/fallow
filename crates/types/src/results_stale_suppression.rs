//! Suppression origins and stale suppressions.

use super::*;

/// The origin of a stale suppression: inline comment or JSDoc tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum SuppressionOrigin {
    /// A `// fallow-ignore-next-line` or `// fallow-ignore-file` comment.
    Comment {
        /// The issue kind token from the comment (e.g., "unused-exports"), or None for blanket.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        issue_kind: Option<String>,
        /// Human-authored reason after `--`, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// Whether this was a file-level suppression.
        is_file_level: bool,
        /// Whether this was a `fallow-ignore-start` range marker (then
        /// `is_file_level` is `false`). Omitted from the wire when `false`.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_range: bool,
        /// Whether `issue_kind` parses to a known `IssueKind`. False when the
        /// token is a typo or refers to a kind that was renamed or removed in
        /// a newer fallow release. JSON consumers (CI annotations, MCP agents,
        /// VS Code) branch on this to choose the right next-step text.
        /// Omitted from the wire when `true` so producers that have not yet
        /// adopted the field stay byte-compatible. See issue #449.
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        kind_known: bool,
    },
    /// An `@expected-unused` JSDoc tag on an export.
    JsdocTag {
        /// The name of the export that was tagged.
        export_name: String,
        /// Human-authored reason after `--`, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if takes a reference by contract"
)]
const fn is_true(b: &bool) -> bool {
    *b
}

/// Default for `SuppressionOrigin::Comment.kind_known` when the field is
/// absent from a deserialized payload, paired with `skip_serializing_if = is_true`
/// so schemars marks the field non-required in the generated JSON Schema AND
/// the absent case round-trips to the recognized-kind interpretation.
/// Referenced by the always-emitted `#[serde(default = "default_true")]`
/// attribute. Serde uses it when saved reports deserialize the output back
/// into the typed findings, while schemars uses it to keep `kind_known`
/// optional in the generated schema.
const fn default_true() -> bool {
    true
}

/// A suppression comment or JSDoc tag that no longer matches any issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StaleSuppression {
    /// File containing the stale suppression.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the suppression comment or tag.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// The origin and details of the stale suppression.
    pub origin: SuppressionOrigin,
    /// True when `rules.require-suppression-reason` reported a suppression
    /// comment or tag that has no reason.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing_reason: bool,
    /// Issue kinds that fallow found on the target line of a stale
    /// `fallow-ignore-next-line` marker. The marker names another kind, so it
    /// does not suppress these findings. Absent when the list is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_line_kinds: Vec<String>,
    /// True when the line after a stale `fallow-ignore-next-line` marker is
    /// blank. The marker applies only to the line directly after it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub next_line_blank: bool,
    /// Stable id of this finding: `dc1:<rule>:<16 hex digits>`, with a
    /// `~<k>` suffix when several findings of one type share an identity.
    /// Line and column are not inputs, so the id survives line shifts,
    /// reformats and reorders. A rename of the file or the symbol gives a
    /// new id. Absent in output from older versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finding_id: Option<String>,
    /// Suggested next steps. Always emitted.
    pub actions: Vec<IssueAction>,
    /// Gate severity of this finding after `rules` and `overrides[].rules`
    /// resolve for its path. CI formats read it for the annotation, SARIF
    /// and CodeClimate level. Absent in output from older versions. Not
    /// part of the finding identity, baseline keys or fingerprints.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::output_dead_code::deserialize_effective_severity"
    )]
    pub effective_severity: Option<crate::output_dead_code::EffectiveSeverity>,
}

impl StaleSuppression {
    /// Build the typed action list for this suppression finding.
    #[must_use]
    pub fn actions_for(missing_reason: bool) -> Vec<IssueAction> {
        let (kind, description) = if missing_reason {
            (
                FixActionType::AddSuppressionReason,
                "Add a human-authored reason after `--` on the suppression",
            )
        } else {
            (
                FixActionType::RemoveStaleSuppression,
                "Remove or update the stale suppression",
            )
        };
        let mut actions = vec![IssueAction::Fix(FixAction {
            kind,
            auto_fixable: false,
            description: description.to_string(),
            note: None,
            available_in_catalogs: None,
            suggested_target: None,
        })];
        if !missing_reason {
            actions.push(IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description:
                    "Suppress this stale suppression finding with a comment above the suppression"
                        .to_string(),
                comment: "// fallow-ignore-next-line stale-suppression".to_string(),
                scope: Some(SuppressLineScope::PerLocation),
            }));
        }
        actions
    }
}
