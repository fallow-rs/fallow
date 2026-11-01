//! The config entries that had no effect in a run: the accessors and the
//! message text of these kinds (issues #3824 and #4065).

use super::WorkspaceDiagnosticKind;

/// The `override-rule-ineffective` reason of a rule that reads only the
/// top-level severity.
pub const OVERRIDE_REASON_PROJECT_LEVEL: &str = "project-level";
/// The `override-rule-ineffective` reason of a rule whose detector does not
/// run, because the top-level rule is off.
pub const OVERRIDE_REASON_BASE_RULE_OFF: &str = "base-rule-off";

/// The project-level rules for which a run confirms that `ignoreDependencies`
/// clears the finding. It does not clear `type-only-dependencies` in a normal
/// run, nor `unused-catalog-entries` in any run.
const IGNORE_DEPENDENCIES_RULES: [&str; 6] = [
    "unused-dependencies",
    "unused-dev-dependencies",
    "unused-optional-dependencies",
    "unlisted-dependencies",
    "test-only-dependencies",
    "dev-dependencies-in-production",
];

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

    /// The rule, the `files` globs and the reason of an
    /// `override-rule-ineffective` entry. `None` for every other kind.
    ///
    /// This kind has no single pattern, so
    /// [`Self::unmatched_config_pattern`] does not return it. Each surface
    /// reads both accessors.
    #[must_use]
    pub fn ineffective_override(&self) -> Option<(&str, &[String], &str)> {
        match self {
            Self::OverrideRuleIneffective {
                rule,
                files,
                reason,
            } => Some((rule.as_str(), files.as_slice(), reason.as_str())),
            _ => None,
        }
    }

    /// The next step of an `override-rule-ineffective` entry, as one clause
    /// for a one-line note. `None` for every other kind.
    #[must_use]
    pub fn ineffective_override_hint(&self) -> Option<&'static str> {
        let (rule, _, reason) = self.ineffective_override()?;
        if reason == OVERRIDE_REASON_BASE_RULE_OFF {
            return Some(
                "the top-level rule is off, so the detector does not run; set the top-level rule \
                 on, then use an override to set off for the other files",
            );
        }
        Some(match rule {
            "type-only-dependencies" => {
                "project-level rule; move the package to devDependencies, or set the top-level \
                 rule"
            }
            "unused-catalog-entries" => {
                "project-level rule; add `# fallow-ignore-next-line unused-catalog-entry` in \
                 pnpm-workspace.yaml, or set the top-level rule"
            }
            "duplicate-exports" => {
                "project-level rule; add the file and export names to ignoreExports"
            }
            "re-export-cycle" => {
                "project-level rule; add `// fallow-ignore-file re-export-cycle` to one file in \
                 the cycle, or set the top-level rule"
            }
            _ if IGNORE_DEPENDENCIES_RULES.contains(&rule) => {
                "project-level rule; add the package to ignoreDependencies, or set the top-level \
                 rule"
            }
            _ => "project-level rule; set the top-level rule",
        })
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

/// The message of a config entry kind that had no effect, or an empty string
/// for any other kind.
pub(super) fn config_entry_message(kind: &WorkspaceDiagnosticKind) -> String {
    match kind {
        WorkspaceDiagnosticKind::IgnoreDependenciesGlobUnmatched { pattern } => format!(
            "ignoreDependencies glob '{pattern}' matched no declared dependency in this run, so \
             it has no effect. A glob matches package names, such as @scope/*. Fix the glob, or \
             remove it from the config."
        ),
        WorkspaceDiagnosticKind::IgnoreFindingsPatternUnmatched { pattern } => format!(
            "ignoreFindings pattern '{pattern}' matched no finding in this run, so it has no \
             effect. A pattern is a glob relative to the project root. Fix the pattern, or \
             remove it from the config."
        ),
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
        WorkspaceDiagnosticKind::OverrideRuleIneffective {
            rule,
            files,
            reason,
        } => ineffective_override_message(rule, files, reason),
        _ => String::new(),
    }
}

fn ineffective_override_message(rule: &str, files: &[String], reason: &str) -> String {
    let globs = files.join(", ");
    let prefix = format!("overrides.rules.{rule} has no effect for files matching [{globs}]");
    if reason == OVERRIDE_REASON_BASE_RULE_OFF {
        return format!(
            "{prefix}: the top-level rule is off, by default or by config, so the detector does \
             not run. Set `rules.{rule}` to `warn` or `error`. Then add an override that sets \
             `off` for the other files."
        );
    }
    let remedy = match rule {
        "type-only-dependencies" => {
            format!("Move the package to `devDependencies`, or set `rules.{rule}`.")
        }
        "unused-catalog-entries" => format!(
            "In `pnpm-workspace.yaml`, add `# fallow-ignore-next-line unused-catalog-entry` \
             above the entry, or set `rules.{rule}`."
        ),
        "duplicate-exports" => "Add the file and export names to top-level `ignoreExports`. That \
             entry also exempts these exports from unused-export."
            .to_owned(),
        "re-export-cycle" => format!(
            "Add `// fallow-ignore-file re-export-cycle` to one file in the cycle, or set \
             `rules.{rule}`."
        ),
        _ if IGNORE_DEPENDENCIES_RULES.contains(&rule) => format!(
            "To hide one package, add it to `ignoreDependencies`. That entry applies to the \
             whole project. To change the rule for all packages, set `rules.{rule}`."
        ),
        _ => format!("Set `rules.{rule}`."),
    };
    format!(
        "{prefix}: {rule} is a project-level rule, so fallow reads only the top-level severity. \
         {remedy}"
    )
}
