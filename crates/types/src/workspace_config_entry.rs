//! The config entries that had no effect in a run: the accessors and the
//! message text of the redundant config entry kinds (issue #3824).

use super::WorkspaceDiagnosticKind;

impl WorkspaceDiagnosticKind {
    /// The config setting and the entry of a config entry that had no effect
    /// in the run: an unmatched pattern (`ignoreDependencies`,
    /// `ignoreFindings`) or a redundant entry (`entry`,
    /// `ignoreDependencies`, `ignorePatterns`). `None` for every other kind.
    ///
    /// The human note, the SARIF configuration notifications, the Markdown
    /// section and the editor all read these entries through this one
    /// accessor.
    #[must_use]
    pub fn unmatched_config_pattern(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::IgnoreDependenciesGlobUnmatched { pattern }
            | Self::IgnoreDependenciesEntryRedundant { pattern } => {
                Some(("ignoreDependencies", pattern.as_str()))
            }
            Self::IgnoreFindingsPatternUnmatched { pattern } => {
                Some(("ignoreFindings", pattern.as_str()))
            }
            Self::EntryPatternRedundant { pattern } => Some(("entry", pattern.as_str())),
            Self::IgnorePatternsEntryRedundant { pattern } => {
                Some(("ignorePatterns", pattern.as_str()))
            }
            _ => None,
        }
    }

    /// Whether this is a config entry that matched something but changed
    /// nothing, as opposed to one that matched nothing (issue #3824).
    #[must_use]
    pub const fn is_redundant_config_entry(&self) -> bool {
        matches!(
            self,
            Self::EntryPatternRedundant { .. }
                | Self::IgnoreDependenciesEntryRedundant { .. }
                | Self::IgnorePatternsEntryRedundant { .. }
        )
    }
}

/// The message of a redundant config entry kind, or an empty string for any
/// other kind.
pub(super) fn redundant_entry_message(kind: &WorkspaceDiagnosticKind) -> String {
    match kind {
        WorkspaceDiagnosticKind::EntryPatternRedundant { pattern } => format!(
            "entry pattern '{pattern}' matched only files that fallow already detects as entry \
             points in this run, so it has no effect. Remove it from the config."
        ),
        WorkspaceDiagnosticKind::IgnoreDependenciesEntryRedundant { pattern } => format!(
            "ignoreDependencies entry '{pattern}' hid no dependency finding in this run, so it \
             has no effect. Remove it from the config."
        ),
        WorkspaceDiagnosticKind::IgnorePatternsEntryRedundant { pattern } => format!(
            "ignorePatterns entry '{pattern}' excluded no file that fallow would otherwise \
             analyze in this run, so it has no effect. Remove it from the config, unless it is \
             for files that exist only after a build."
        ),
        _ => String::new(),
    }
}
