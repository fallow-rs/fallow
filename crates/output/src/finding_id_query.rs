//! The machine-readable answer to `fallow dead-code --finding-id`.
//!
//! A consumer that stores a verdict per finding id asks whether a finding
//! still exists. The report alone cannot answer that: an id that is not in the
//! report can be absent, or it can be hidden by a scope, a baseline or a
//! filter of this run. `finding_id_query` makes the difference explicit.
//! Read `missing` as "resolved" only when `conclusive` is true.

use serde::Serialize;

use crate::ScopeReason;

/// One reason why a missing id does not prove that the finding is gone.
///
/// Serialized as kebab-case inside `inconclusive_reasons`. The set is OPEN: a
/// name this build does not emit means "some reason", not an error, and the
/// query stays inconclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FindingIdQueryReason {
    /// A diff index narrowed the report (`--diff-file`, `--diff-stdin`,
    /// `FALLOW_DIFF_FILE` or a CI format).
    Diff,
    /// A global changed-since ref.
    ChangedSince,
    /// The per-package refs of `workspaces.changedSince` in the config.
    PackageBaselines,
    /// A resolved changed-file set narrowed the report.
    ChangedFiles,
    /// `--workspace`.
    Workspace,
    /// `--changed-workspaces`.
    ChangedWorkspaces,
    /// The positional `[PATH]` scope.
    Scope,
    /// One or more `--file`.
    File,
    /// An issue-type filter such as `--unused-exports`.
    IssueTypeFilter,
    /// Production mode, from the flag or from the project config. It removes
    /// test, story and dev files before the analysis, so a finding in such a
    /// file is never seen. A project that sets production mode in its config
    /// therefore never gets a conclusive answer.
    Production,
    /// `--include-entry-exports` or the `includeEntryExports` config key. It
    /// changes which exports `unused-exports` reports.
    IncludeEntryExports,
    /// `--baseline`: the run hides the findings the baseline lists.
    Baseline,
    /// The rule of a missing id is `off` in `rules` or in one of the
    /// `overrides[].rules`. The analysis does not look for such a finding,
    /// so its absence proves nothing.
    RuleOff,
    /// The analysis found a requested id, and a filter of this run removed it
    /// from the report. The ids are in `filtered`.
    Filtered,
}

impl FindingIdQueryReason {
    /// The kebab-case name this reason serializes as, for prose outside the
    /// JSON envelope.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Diff => "diff",
            Self::ChangedSince => "changed-since",
            Self::PackageBaselines => "package-baselines",
            Self::ChangedFiles => "changed-files",
            Self::Workspace => "workspace",
            Self::ChangedWorkspaces => "changed-workspaces",
            Self::Scope => "scope",
            Self::File => "file",
            Self::IssueTypeFilter => "issue-type-filter",
            Self::Production => "production",
            Self::IncludeEntryExports => "include-entry-exports",
            Self::Baseline => "baseline",
            Self::RuleOff => "rule-off",
            Self::Filtered => "filtered",
        }
    }
}

impl From<ScopeReason> for FindingIdQueryReason {
    fn from(reason: ScopeReason) -> Self {
        match reason {
            ScopeReason::Diff => Self::Diff,
            ScopeReason::ChangedSince => Self::ChangedSince,
            ScopeReason::PackageBaselines => Self::PackageBaselines,
            ScopeReason::ChangedFiles => Self::ChangedFiles,
            ScopeReason::Workspace => Self::Workspace,
            ScopeReason::ChangedWorkspaces => Self::ChangedWorkspaces,
            ScopeReason::Scope => Self::Scope,
            ScopeReason::File => Self::File,
            ScopeReason::IssueTypeFilter => Self::IssueTypeFilter,
            ScopeReason::Production => Self::Production,
            ScopeReason::IncludeEntryExports => Self::IncludeEntryExports,
        }
    }
}

/// The result of a `--finding-id` query, present only when the run received
/// one or more `--finding-id` values.
///
/// A requested id that is missing from a conclusive run means "fixed,
/// suppressed, or ignored by config", never "unknown": an inline suppression
/// comment or an `ignoreFindings` entry is a choice a person made to hide the
/// finding, so it counts as absent. A missing id in a run that is not
/// conclusive is unknown, never resolved.
///
/// Every list keeps the order of `requested`. `found` and `missing` partition
/// `requested`. `filtered` is a subset of `missing`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FindingIdQuery {
    /// The requested ids, without duplicates, in the order of the arguments.
    pub requested: Vec<String>,
    /// The requested ids that this report contains.
    pub found: Vec<String>,
    /// The requested ids that this report does not contain. When `conclusive`
    /// is true, a missing id is fixed, suppressed, or ignored by config.
    /// Otherwise its state is unknown.
    pub missing: Vec<String>,
    /// The missing ids that the analysis still found before a filter of this
    /// run (scope, baseline, issue-type filter) removed them. Such a finding
    /// still exists.
    pub filtered: Vec<String>,
    /// True when no option of this run can hide a finding without a fix, and
    /// no requested id was filtered. Only then does a missing id mean that
    /// the analysis no longer reports the finding.
    pub conclusive: bool,
    /// Why the query is not conclusive, sorted. Empty exactly when
    /// `conclusive` is true.
    pub inconclusive_reasons: Vec<FindingIdQueryReason>,
    /// A stable hash (`af1:<16 hex digits>`) of every input other than the
    /// source code that decides which findings the run reports:
    /// - the fallow version;
    /// - the merged config after `extends` (without keys that only shape other
    ///   commands), the loaded external plugins and rule packs;
    /// - production mode, `includeEntryExports`, the effective rules, the
    ///   type-aware mode, requirement and project list, the file size limit;
    /// - the root-relative path and content of each repository `.gitignore`,
    ///   `.ignore` and `.git/info/exclude`, each `package.json`, each
    ///   `tsconfig*.json` and `jsconfig*.json` with the files its `extends`
    ///   names, and each file that matches a built-in or external plugin
    ///   config pattern (for example `vite.config.ts`).
    ///
    /// File content is normalized (CRLF to LF, trailing newlines removed).
    /// Known exclusions: the global git excludes file and other machine
    /// environment outside the `FALLOW_*` variables. Store the fingerprint
    /// with a verdict. A later query with another fingerprint is unknown, even
    /// when `conclusive` is true. An edit to a source file keeps it; an edit
    /// to a manifest or project config changes it, also when the edit fixes a
    /// dependency finding.
    pub analysis_fingerprint: String,
}

impl FindingIdQuery {
    /// Build the query result.
    ///
    /// `requested` must be free of duplicates. `found` and `filtered` are
    /// membership tests over `requested`. `run_reasons` are the options of
    /// the run that can hide a finding; `Filtered` is added when an id was
    /// filtered.
    #[must_use]
    pub fn new(
        requested: Vec<String>,
        is_found: impl Fn(&str) -> bool,
        is_filtered: impl Fn(&str) -> bool,
        run_reasons: impl IntoIterator<Item = FindingIdQueryReason>,
        analysis_fingerprint: String,
    ) -> Self {
        let (found, missing): (Vec<String>, Vec<String>) =
            requested.iter().cloned().partition(|id| is_found(id));
        let filtered: Vec<String> = missing
            .iter()
            .filter(|id| is_filtered(id))
            .cloned()
            .collect();
        let mut reasons: Vec<FindingIdQueryReason> = run_reasons.into_iter().collect();
        if !filtered.is_empty() {
            reasons.push(FindingIdQueryReason::Filtered);
        }
        reasons.sort_unstable();
        reasons.dedup();
        Self {
            requested,
            found,
            missing,
            filtered,
            conclusive: reasons.is_empty(),
            inconclusive_reasons: reasons,
            analysis_fingerprint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn a_missing_id_without_reasons_is_conclusive() {
        let query = FindingIdQuery::new(
            ids(&["a", "b"]),
            |id| id == "a",
            |_| false,
            [],
            String::new(),
        );

        assert_eq!(query.found, ids(&["a"]));
        assert_eq!(query.missing, ids(&["b"]));
        assert!(query.filtered.is_empty());
        assert!(query.conclusive);
        assert!(query.inconclusive_reasons.is_empty());
    }

    #[test]
    fn a_filtered_id_makes_the_query_inconclusive() {
        let query = FindingIdQuery::new(
            ids(&["a", "b"]),
            |_| false,
            |id| id == "b",
            [],
            String::new(),
        );

        assert_eq!(query.filtered, ids(&["b"]));
        assert!(!query.conclusive);
        assert_eq!(
            query.inconclusive_reasons,
            vec![FindingIdQueryReason::Filtered]
        );
    }

    #[test]
    fn run_reasons_are_sorted_and_unique() {
        let query = FindingIdQuery::new(
            ids(&["a"]),
            |_| true,
            |_| false,
            [
                FindingIdQueryReason::Baseline,
                FindingIdQueryReason::Scope,
                FindingIdQueryReason::Baseline,
            ],
            String::new(),
        );

        assert!(!query.conclusive);
        assert_eq!(
            query.inconclusive_reasons,
            vec![FindingIdQueryReason::Scope, FindingIdQueryReason::Baseline]
        );
    }
}
