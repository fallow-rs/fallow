//! Audit diff-risk output contracts.
//!
//! A diff-risk finding is a fact about the change itself: the diff weakened a
//! test, added a suppression, added a test-only branch, edited one copy of a
//! clone group, or added a dependency. `fallow audit` computes these from the
//! base and head text of the changed files, so every finding is introduced by
//! the change. The actions offer only suppression, never a fix.

use serde::{Deserialize, Serialize};

/// The rule that owns a diff-risk finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DiffRiskRule {
    /// The diff weakened tests. The `kind` field tells how.
    WeakenedTests,
    /// The diff added a lint, type-check, coverage or Fallow suppression.
    AddedSuppression,
    /// The diff added a test-environment branch to a non-test source file.
    TestOnlyBranch,
    /// The diff edited one copy of a base clone group, and no sibling changed.
    DivergedCloneSibling,
    /// The diff added a dependency to a `package.json`.
    AddedDependency,
}

impl DiffRiskRule {
    /// Every rule, in output order.
    pub const ALL: [Self; 5] = [
        Self::WeakenedTests,
        Self::AddedSuppression,
        Self::TestOnlyBranch,
        Self::DivergedCloneSibling,
        Self::AddedDependency,
    ];

    /// The stable rule code. It is also the `rules` config key and the
    /// suppression token.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::WeakenedTests => "weakened-tests",
            Self::AddedSuppression => "added-suppression",
            Self::TestOnlyBranch => "test-only-branch",
            Self::DivergedCloneSibling => "diverged-clone-sibling",
            Self::AddedDependency => "added-dependency",
        }
    }

    /// The SARIF rule id, `fallow/<code>`.
    #[must_use]
    pub fn sarif_rule_id(self) -> String {
        format!("fallow/{}", self.code())
    }

    /// A short description for SARIF rule metadata and human output.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::WeakenedTests => "The change weakens tests or a coverage threshold",
            Self::AddedSuppression => "The change adds a suppression comment",
            Self::TestOnlyBranch => "The change adds a test-environment branch to source code",
            Self::DivergedCloneSibling => {
                "The change edits one copy of duplicated code but not its siblings"
            }
            Self::AddedDependency => "The change adds a dependency",
        }
    }
}

/// How a `weakened-tests` finding weakened the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum WeakenedTestsKind {
    /// A new skip or focus marker (`it.skip`, `xit`, `it.only`).
    SkipOnly,
    /// Fewer test cases across the changed test files.
    RemovedTests,
    /// Fewer assertions in a test file, with no test case removed.
    AssertionDrop,
    /// A coverage threshold that the change made less strict.
    CoverageThreshold,
}

impl WeakenedTestsKind {
    /// The stable wire value, the same as the serialized form.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::SkipOnly => "skip-only",
            Self::RemovedTests => "removed-tests",
            Self::AssertionDrop => "assertion-drop",
            Self::CoverageThreshold => "coverage-threshold",
        }
    }
}

/// The severity of a diff-risk finding, from its rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DiffRiskSeverity {
    /// The rule is `error`: the finding fails the audit verdict.
    Error,
    /// The rule is `warn`: the finding gives a `warn` verdict.
    Warn,
}

impl DiffRiskSeverity {
    /// The stable wire value, the same as the serialized form.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
        }
    }
}

/// A location that relates to a diff-risk finding, such as an unchanged clone
/// sibling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffRiskLocation {
    /// Root-relative path, with forward slashes.
    pub path: String,
    /// 1-based line.
    pub line: u32,
}

/// The type of a diff-risk action. Every type is a suppression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DiffRiskActionType {
    /// Put the `comment` on the line above the finding.
    SuppressLine,
    /// Put the `comment` at the top of the file.
    SuppressFile,
    /// The file cannot hold a comment. Set the rule to `off` for this path in
    /// an `overrides` entry of the Fallow config.
    SuppressInConfig,
}

/// One action on a diff-risk finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffRiskAction {
    /// Action discriminator.
    #[serde(rename = "type")]
    pub kind: DiffRiskActionType,
    /// Always `false`: Fallow does not apply a suppression for you.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// The suppression comment to insert. Absent on `suppress-in-config`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// One diff-risk finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffRiskFinding {
    /// The rule that owns the finding.
    pub rule: DiffRiskRule,
    /// How the tests were weakened. Present only on `weakened-tests`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<WeakenedTestsKind>,
    /// Root-relative path of the changed file, with forward slashes.
    pub path: String,
    /// 1-based head line. Absent when the finding is about the whole file,
    /// for example removed tests or a deleted file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// One-sentence statement of the fact.
    pub message: String,
    /// The token, the count change, or the value change that supports the
    /// finding.
    pub evidence: String,
    /// The severity from the rule config.
    pub severity: DiffRiskSeverity,
    /// Always `true`: the change introduced the finding.
    pub introduced: bool,
    /// Related locations, such as the unchanged siblings of a clone group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<DiffRiskLocation>,
    /// Suppression actions.
    pub actions: Vec<DiffRiskAction>,
}

/// The diff-risk section of `fallow audit` JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffRiskReport {
    /// Findings, ordered by path, line, rule and kind.
    pub findings: Vec<DiffRiskFinding>,
}

impl DiffRiskReport {
    /// Whether the report has no findings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    /// Whether a finding has `error` severity.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.severity == DiffRiskSeverity::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_the_serialized_values() {
        for kind in [
            WeakenedTestsKind::SkipOnly,
            WeakenedTestsKind::RemovedTests,
            WeakenedTestsKind::AssertionDrop,
            WeakenedTestsKind::CoverageThreshold,
        ] {
            assert_eq!(serde_json::json!(kind), kind.code());
        }
        for severity in [DiffRiskSeverity::Error, DiffRiskSeverity::Warn] {
            assert_eq!(serde_json::json!(severity), severity.code());
        }
    }

    #[test]
    fn finding_serializes_with_kebab_case_and_skips_empty_fields() {
        let finding = DiffRiskFinding {
            rule: DiffRiskRule::WeakenedTests,
            kind: Some(WeakenedTestsKind::SkipOnly),
            path: "src/a.test.ts".to_owned(),
            line: Some(3),
            message: "m".to_owned(),
            evidence: "it.skip".to_owned(),
            severity: DiffRiskSeverity::Warn,
            introduced: true,
            related: Vec::new(),
            actions: vec![DiffRiskAction {
                kind: DiffRiskActionType::SuppressLine,
                auto_fixable: false,
                description: "d".to_owned(),
                comment: Some("// fallow-ignore-next-line weakened-tests".to_owned()),
            }],
        };
        let value = serde_json::to_value(&finding).expect("serialize");
        assert_eq!(value["rule"], "weakened-tests");
        assert_eq!(value["kind"], "skip-only");
        assert_eq!(value["severity"], "warn");
        assert_eq!(value["actions"][0]["type"], "suppress-line");
        assert!(value.get("related").is_none());
    }

    #[test]
    fn rule_codes_match_sarif_ids() {
        for rule in DiffRiskRule::ALL {
            assert_eq!(rule.sarif_rule_id(), format!("fallow/{}", rule.code()));
            let value = serde_json::to_value(rule).expect("serialize");
            assert_eq!(value, rule.code());
        }
    }
}
