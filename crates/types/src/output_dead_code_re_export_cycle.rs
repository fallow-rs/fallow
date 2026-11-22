//! Typed envelope wrapper for re-export cycle findings.

use super::*;

/// Wire-shape envelope for a [`ReExportCycle`] finding. Mirrors
/// [`CircularDependencyFinding`]: flattens the bare finding and carries a
/// typed `actions` array (`refactor-re-export-cycle` informational primary
/// plus `suppress-file` secondary; cycles are file-scoped so a single
/// file-level suppression on the alphabetically-first member breaks the
/// cycle, and no `// fallow-ignore-next-line` form makes sense because the
/// diagnostic is anchored at line 1 col 0 of each member).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReExportCycleFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub cycle: ReExportCycle,
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

impl ReExportCycleFinding {
    /// Build the wrapper from a raw [`ReExportCycle`].
    ///
    /// The `SuppressFile` action targets the alphabetically-first member
    /// (`cycle.files[0]`; the `files` Vec is already sorted at graph layer);
    /// for multi-node cycles the description names the other members so
    /// consumers see context for why one file-level suppression suffices.
    #[must_use]
    pub fn with_actions(cycle: ReExportCycle) -> Self {
        // The description is a path-free hint about the suppression's
        // structural effect; the cycle's member list already ships in the
        // sibling `files` field, so consumers can correlate without
        // re-reading the description (and absolute paths cannot leak in
        // here, which the wrapper has no root-prefix context to strip).
        let suppress_description = match cycle.kind {
            ReExportCycleKind::SelfLoop => {
                "Suppress with a file-level comment at the top of this file. \
                 The cycle is a self-loop, so the suppression covers the entire finding."
                    .to_string()
            }
            ReExportCycleKind::MultiNode => {
                "Suppress with a file-level comment at the top of this file. \
                 One suppression on any member breaks the cycle for every member \
                 (see the sibling `files` array)."
                    .to_string()
            }
        };
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RefactorReExportCycle,
                auto_fixable: false,
                description: "Remove one `export * from` (or `export { ... } from`) \
                              statement on any one member to break the cycle"
                    .to_string(),
                note: Some(
                    "Re-export cycles are structurally a no-op: chain propagation through \
                     the loop never reaches a terminating module, so imports from any member \
                     may silently come up empty."
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressFile(SuppressFileAction {
                kind: SuppressFileKind::SuppressFile,
                auto_fixable: false,
                description: suppress_description,
                comment: "// fallow-ignore-file re-export-cycle".to_string(),
            }),
        ];
        Self {
            finding_id: None,
            cycle,
            actions,
            introduced: None,
            effective_severity: None,
        }
    }
}
