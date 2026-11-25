//! Typed envelope wrapper for test-only export findings.

use super::*;
use crate::results::TestOnlyExport;

/// Wire-shape envelope for a [`TestOnlyExport`] finding under the
/// `test_only_exports` key.
///
/// Both fix actions are manual. A removal of the export also breaks the
/// test-side files that import it, so `fallow fix` never applies one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TestOnlyExportFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub export: TestOnlyExport,
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

impl TestOnlyExportFinding {
    /// Build the wrapper with the two manual fixes and a line-level
    /// suppress.
    #[must_use]
    pub fn with_actions(export: TestOnlyExport) -> Self {
        let actions = vec![
            manual_framework_fix(
                FixActionType::RemoveExportAndTests,
                "Remove the export and the tests that use it",
                "Production code does not use this export. When it is dead code that its tests keep alive, remove the export together with those tests. A removal of the export alone breaks the test-side files in test_importers.",
            ),
            manual_framework_fix(
                FixActionType::TestThroughInterface,
                "Test the behavior through the public interface of the module",
                "When the export is a seam for tests only, change the tests to use the public functions of the module, then make the item internal.",
            ),
            suppress_line("// fallow-ignore-next-line test-only-export"),
        ];
        Self {
            export,
            finding_id: None,
            actions,
            introduced: None,
            effective_severity: None,
        }
    }
}

impl_gated_finding!(TestOnlyExportFinding);
