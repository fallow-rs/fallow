//! Integration coverage for opt-in hardcoded-secret candidates.

use fallow_config::{SecurityCategories, SecurityConfig, Severity};
use fallow_core::results::{AnalysisResults, SecurityFindingKind};

use super::common::{create_config_with_rules, fixture_path};

const FIXTURE: &str = "security-hardcoded-secret-892";
const CATEGORY: &str = "hardcoded-secret";

fn analyze_with_category(include: bool, exclude: bool) -> AnalysisResults {
    let root = fixture_path(FIXTURE);
    let mut config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    config.security = SecurityConfig {
        categories: Some(SecurityCategories {
            include: include.then(|| vec![CATEGORY.to_string()]),
            exclude: exclude.then(|| vec![CATEGORY.to_string()]),
        }),
    };
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn category_findings(results: &AnalysisResults) -> Vec<&fallow_core::results::SecurityFinding> {
    results
        .security_findings
        .iter()
        .filter(|finding| finding.category.as_deref() == Some(CATEGORY))
        .collect()
}

fn anchored_on(results: &AnalysisResults, suffix: &str) -> bool {
    results.security_findings.iter().any(|finding| {
        finding
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with(suffix)
    })
}

#[test]
fn hardcoded_secret_category_requires_explicit_include() {
    let results = analyze_with_category(false, false);
    assert!(category_findings(&results).is_empty());
}

#[test]
fn hardcoded_secret_category_exclude_wins_over_include() {
    let results = analyze_with_category(true, true);
    assert!(category_findings(&results).is_empty());
}

#[test]
fn hardcoded_secret_category_reports_conservative_candidates() {
    let results = analyze_with_category(true, false);
    let findings = category_findings(&results);

    assert!(
        findings
            .iter()
            .all(|finding| matches!(finding.kind, SecurityFindingKind::TaintedSink))
    );
    assert!(anchored_on(&results, "src/provider.ts"));
    assert!(anchored_on(&results, "src/entropy.ts"));
    assert!(anchored_on(&results, "src/object.ts"));
    assert!(anchored_on(&results, "src/template.ts"));
    assert!(anchored_on(&results, "src/assignment.ts"));
    assert!(!anchored_on(&results, "src/safe.ts"));
    assert!(!anchored_on(&results, "tests/provider.test.ts"));
    assert!(
        findings
            .iter()
            .all(|finding| finding.cwe == Some(798) && !finding.actions.is_empty())
    );
}

#[test]
fn hardcoded_secret_evidence_redacts_literal_values() {
    let results = analyze_with_category(true, false);
    let joined = category_findings(&results)
        .iter()
        .map(|finding| finding.evidence.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!joined.contains("AKIA1234567890ABCDEF"));
    assert!(!joined.contains("mF9a7Qp2Lx8Nz4Rv6Ts0"));
    assert!(!joined.contains("n7Pq4Zx9Lm2Qa8Rt5Vb3"));
}
