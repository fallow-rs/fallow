use super::build_diff_risk_markdown;
use fallow_output::{
    DiffRiskFinding, DiffRiskReport, DiffRiskRule, DiffRiskSeverity, WeakenedTestsKind,
};

fn finding(
    rule: DiffRiskRule,
    kind: Option<WeakenedTestsKind>,
    location: (&str, u32),
    evidence: &str,
    severity: DiffRiskSeverity,
) -> DiffRiskFinding {
    DiffRiskFinding {
        rule,
        kind,
        path: location.0.to_owned(),
        line: Some(location.1),
        message: "m".to_owned(),
        evidence: evidence.to_owned(),
        severity,
        introduced: true,
        related: Vec::new(),
        actions: Vec::new(),
        suppression: None,
    }
}

#[test]
fn diff_risk_markdown_is_empty_without_findings() {
    assert_eq!(build_diff_risk_markdown(&DiffRiskReport::default()), "");
}

#[test]
fn diff_risk_markdown_lists_each_finding_in_a_table() {
    let report = DiffRiskReport {
        findings: vec![
            finding(
                DiffRiskRule::WeakenedTests,
                Some(WeakenedTestsKind::SkipOnly),
                ("src/a.test.ts", 4),
                "it.skip",
                DiffRiskSeverity::Warn,
            ),
            finding(
                DiffRiskRule::AddedSuppression,
                None,
                ("src/b.ts", 9),
                "eslint-disable | x",
                DiffRiskSeverity::Error,
            ),
        ],
    };
    assert_eq!(
        build_diff_risk_markdown(&report),
        "## Fallow: 2 diff-risk findings\n\n\
         | Location | Rule | Severity | Evidence |\n\
         |:---------|:-----|:---------|:---------|\n\
         | `src/a.test.ts:4` | `weakened-tests` / `skip-only` | warn | `it.skip` |\n\
         | `src/b.ts:9` | `added-suppression` | error | `eslint-disable \\| x` |\n"
    );
}
