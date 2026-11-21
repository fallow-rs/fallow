//! Integration tests for the `react-router-open-redirect` catalogue row (#4951).
//!
//! React Router exports `redirect`, `redirectDocument` and `replace`. Each one
//! builds a redirect `Response` and accepts an absolute URL, so a non-literal
//! target in a `loader` or an `action` is an open-redirect candidate. The row
//! fires only when the project declares `react-router` or `@react-router/dev`,
//! and only when the callee is the binding of an import from `react-router`.

use fallow_config::Severity;
use fallow_core::results::{AnalysisResults, SecurityFinding};

use super::common::{create_config_with_rules, fixture_path};

const CATEGORY: &str = "react-router-open-redirect";

fn analyze_fixture(name: &str) -> AnalysisResults {
    let root = fixture_path(name);
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn findings_on<'a>(results: &'a AnalysisResults, suffix: &str) -> Vec<&'a SecurityFinding> {
    results
        .security_findings
        .iter()
        .filter(|finding| {
            finding
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(suffix)
        })
        .collect()
}

fn assert_one_candidate(results: &AnalysisResults, suffix: &str, callee: &str) {
    let findings = findings_on(results, suffix);
    assert_eq!(findings.len(), 1, "{suffix}: {findings:#?}");
    let finding = findings[0];
    assert_eq!(finding.category.as_deref(), Some(CATEGORY), "{suffix}");
    assert_eq!(finding.cwe, Some(601), "{suffix}");
    assert!(
        finding
            .evidence
            .contains(&format!("React Router {callee}()")),
        "{suffix}: {}",
        finding.evidence
    );
}

#[test]
fn react_router_redirect_helpers_fire_with_a_non_literal_target() {
    let results = analyze_fixture("security-react-router-open-redirect");
    assert_one_candidate(&results, "app/routes/login.tsx", "redirect");
    assert_one_candidate(&results, "app/routes/logout.tsx", "redirectDocument");
    assert_one_candidate(&results, "app/routes/moved.tsx", "replace");
}

#[test]
fn react_router_redirect_with_a_literal_target_does_not_fire() {
    let results = analyze_fixture("security-react-router-open-redirect");
    assert!(findings_on(&results, "app/routes/home.tsx").is_empty());
}

#[test]
fn local_redirect_and_replace_functions_do_not_fire() {
    // The module imports `Link` from react-router, but `redirect` and
    // `replace` are local functions. The row traces the import binding to the
    // callee, so a module-level import of the package is not enough.
    let results = analyze_fixture("security-react-router-open-redirect");
    let findings = findings_on(&results, "app/routes/local.tsx");
    assert!(
        findings
            .iter()
            .all(|finding| finding.category.as_deref() != Some(CATEGORY)),
        "{findings:#?}"
    );
}

#[test]
fn react_router_dev_dependency_enables_the_row() {
    let results = analyze_fixture("security-react-router-open-redirect-dev");
    assert_one_candidate(&results, "app/routes/login.tsx", "redirect");
}

#[test]
fn row_does_not_fire_without_a_react_router_dependency() {
    let results = analyze_fixture("security-react-router-open-redirect-plain");
    assert!(
        results
            .security_findings
            .iter()
            .all(|finding| finding.category.as_deref() != Some(CATEGORY)),
        "{:#?}",
        results.security_findings
    );
}
