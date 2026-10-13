//! Weakening signals of the advisory review brief.
//!
//! The brief reports the guardrails that a change weakens: tests removed,
//! skipped or focused, coverage thresholds lowered, suppressions added and
//! security steps removed from CI. The signals are attention pointers for a
//! reviewer. They never gate, and the brief always exits 0.
//!
//! The diff-risk rules of `fallow audit` (`fallow_api::diff_risk`) own the
//! test, threshold and suppression facts. This module projects those findings
//! onto the brief signals, so the brief and the `diff_risk` section agree.
//! Only the removed-security-step scan of CI files is local to the brief.

use fallow_output::{DiffRiskReport, DiffRiskRule, WeakenedTestsKind};
pub use fallow_output::{WeakeningKind, WeakeningSignal};

/// Project the findings of a diff-risk report onto weakening signals.
///
/// - `weakened-tests` maps to `test-weakened`, except a less strict coverage
///   threshold, which maps to `threshold-lowered`.
/// - `added-suppression` maps to `suppression-added`.
///
/// The other diff-risk rules are not weakening signals.
#[must_use]
pub fn signals_from_diff_risk(report: &DiffRiskReport) -> Vec<WeakeningSignal> {
    report
        .findings
        .iter()
        .filter_map(|finding| {
            let (kind, evidence) = match (finding.rule, finding.kind) {
                (DiffRiskRule::WeakenedTests, Some(WeakenedTestsKind::CoverageThreshold)) => {
                    (WeakeningKind::ThresholdLowered, finding.evidence.clone())
                }
                (DiffRiskRule::WeakenedTests, Some(WeakenedTestsKind::SkipOnly)) => (
                    WeakeningKind::TestWeakened,
                    format!("{} added", finding.evidence),
                ),
                (DiffRiskRule::WeakenedTests, _) => {
                    (WeakeningKind::TestWeakened, finding.evidence.clone())
                }
                (DiffRiskRule::AddedSuppression, _) => (
                    WeakeningKind::SuppressionAdded,
                    format!("{} added", finding.evidence),
                ),
                (
                    DiffRiskRule::TestOnlyBranch
                    | DiffRiskRule::DivergedCloneSibling
                    | DiffRiskRule::AddedDependency,
                    _,
                ) => return None,
            };
            Some(WeakeningSignal {
                kind,
                file: finding.path.clone(),
                evidence,
            })
        })
        .collect()
}

/// Detect removed security CI steps: a line that invokes a security scanner
/// or an audit step, present in base and gone in head. The caller scopes the
/// scan to CI files.
#[must_use]
pub fn detect_removed_security_steps(base: &str, head: &str) -> Vec<String> {
    const SECURITY_TOKENS: &[&str] = &[
        "npm audit",
        "yarn audit",
        "pnpm audit",
        "fallow security",
        "codeql",
        "snyk",
        "trivy",
        "semgrep",
        "gitleaks",
        "dependency-review",
        "osv-scanner",
    ];
    SECURITY_TOKENS
        .iter()
        .filter(|token| base.matches(**token).count() > head.matches(**token).count())
        .map(|token| format!("{token} step removed"))
        .collect()
}

/// Whether a path looks like a CI workflow file, where a removed security
/// step is meaningful.
#[must_use]
pub fn is_ci_file(rel_path: &str) -> bool {
    rel_path.contains(".github/workflows/")
        || rel_path.ends_with(".gitlab-ci.yml")
        || rel_path.ends_with(".gitlab-ci.yaml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{DiffRiskFinding, DiffRiskSeverity};

    fn finding(
        rule: DiffRiskRule,
        kind: Option<WeakenedTestsKind>,
        path: &str,
        evidence: &str,
    ) -> DiffRiskFinding {
        DiffRiskFinding {
            rule,
            kind,
            path: path.to_owned(),
            line: Some(1),
            message: String::new(),
            evidence: evidence.to_owned(),
            severity: DiffRiskSeverity::Warn,
            introduced: true,
            related: Vec::new(),
            actions: Vec::new(),
        }
    }

    fn project(findings: Vec<DiffRiskFinding>) -> Vec<(WeakeningKind, String, String)> {
        signals_from_diff_risk(&DiffRiskReport { findings })
            .into_iter()
            .map(|signal| (signal.kind, signal.file, signal.evidence))
            .collect()
    }

    fn signal(kind: WeakeningKind, file: &str, evidence: &str) -> (WeakeningKind, String, String) {
        (kind, file.to_owned(), evidence.to_owned())
    }

    #[test]
    fn weakened_tests_map_to_test_and_threshold_signals() {
        let signals = project(vec![
            finding(
                DiffRiskRule::WeakenedTests,
                Some(WeakenedTestsKind::SkipOnly),
                "a.test.ts",
                "it.skip",
            ),
            finding(
                DiffRiskRule::WeakenedTests,
                Some(WeakenedTestsKind::RemovedTests),
                "b.test.ts",
                "test cases: 2 -> 1",
            ),
            finding(
                DiffRiskRule::WeakenedTests,
                Some(WeakenedTestsKind::AssertionDrop),
                "c.test.ts",
                "assertions: 3 -> 1",
            ),
            finding(
                DiffRiskRule::WeakenedTests,
                Some(WeakenedTestsKind::CoverageThreshold),
                "vitest.config.ts",
                "lines: 90 -> 70",
            ),
        ]);
        assert_eq!(
            signals,
            vec![
                signal(WeakeningKind::TestWeakened, "a.test.ts", "it.skip added"),
                signal(
                    WeakeningKind::TestWeakened,
                    "b.test.ts",
                    "test cases: 2 -> 1"
                ),
                signal(
                    WeakeningKind::TestWeakened,
                    "c.test.ts",
                    "assertions: 3 -> 1"
                ),
                signal(
                    WeakeningKind::ThresholdLowered,
                    "vitest.config.ts",
                    "lines: 90 -> 70"
                ),
            ]
        );
    }

    #[test]
    fn added_suppression_maps_and_other_rules_drop() {
        let signals = project(vec![
            finding(
                DiffRiskRule::AddedSuppression,
                None,
                "src/a.ts",
                "eslint-disable",
            ),
            finding(
                DiffRiskRule::TestOnlyBranch,
                None,
                "src/a.ts",
                "process.env.NODE_ENV",
            ),
            finding(DiffRiskRule::DivergedCloneSibling, None, "src/b.ts", ""),
            finding(DiffRiskRule::AddedDependency, None, "package.json", "x@1"),
        ]);
        assert_eq!(
            signals,
            vec![signal(
                WeakeningKind::SuppressionAdded,
                "src/a.ts",
                "eslint-disable added"
            )]
        );
    }

    #[test]
    fn removed_security_step_is_flagged() {
        let base = "      - run: npm audit --audit-level=high\n      - run: build";
        let head = "      - run: build";
        assert_eq!(
            detect_removed_security_steps(base, head),
            vec!["npm audit step removed".to_owned()]
        );
    }

    #[test]
    fn file_classifiers() {
        assert!(is_ci_file(".github/workflows/ci.yml"));
        assert!(is_ci_file(".gitlab-ci.yml"));
        assert!(!is_ci_file("src/app.ts"));
    }
}
