//! Typed envelope wrapper for namespace escape findings.

use super::*;
use crate::results::NamespaceEscape;

/// Wire-shape envelope for a [`NamespaceEscape`] finding consumed under the
/// `namespace_escapes` key. Emits only a line-level suppress action: the fix
/// is to read the members by name, which is a code change for the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NamespaceEscapeFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub escape: NamespaceEscape,
    /// Stable id of this finding: `dc1:<rule>:<16 hex digits>`, with a
    /// `~<k>` suffix when several findings of one type share an identity.
    /// Line and column are not inputs, so the id survives line shifts,
    /// reformats and reorders. A rename of the file or the symbol gives a
    /// new id. Absent in output from older versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finding_id: Option<String>,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
    /// Gate severity of this finding after `rules` and `overrides[].rules`
    /// resolve for its path. CI formats read it for the annotation, SARIF
    /// and CodeClimate level. Absent in output from older versions. Not
    /// part of the finding identity, baseline keys or fingerprints.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_effective_severity"
    )]
    pub effective_severity: Option<EffectiveSeverity>,
}

impl NamespaceEscapeFinding {
    /// Build the wrapper from a raw [`NamespaceEscape`]. Emits only a
    /// line-level suppress action.
    #[must_use]
    pub fn with_actions(escape: NamespaceEscape) -> Self {
        let actions = vec![IssueAction::SuppressLine(SuppressLineAction {
            kind: SuppressLineKind::SuppressLine,
            auto_fixable: false,
            description: "Suppress with an inline comment above the import".to_string(),
            comment: "// fallow-ignore-next-line namespace-escape".to_string(),
            scope: None,
        })];
        Self {
            finding_id: None,
            escape,
            actions,
            introduced: None,
            effective_severity: None,
        }
    }
}

impl_gated_finding!(NamespaceEscapeFinding);
