//! A name-only security row (`merge`, `request`) must not report a call of a
//! function that a project module exports with an inert body. The analysis
//! follows a direct import one hop. A callee from an external package, a
//! re-export chain, a namespace import and a body with an effect stay
//! reported.

use fallow_config::Severity;
use fallow_core::results::AnalysisResults;

use super::common::{create_config_with_rules, fixture_path};

fn analyze() -> AnalysisResults {
    let root = fixture_path("security-imported-callees");
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn categories_on(results: &AnalysisResults, suffix: &str) -> Vec<String> {
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
        .filter_map(|finding| finding.category.clone())
        .collect()
}

fn assert_cleared(results: &AnalysisResults, suffix: &str) {
    let categories = categories_on(results, suffix);
    assert!(
        categories.is_empty(),
        "{suffix} calls an inert project function, observed {categories:?}"
    );
}

fn assert_reported(results: &AnalysisResults, suffix: &str, category: &str) {
    let categories = categories_on(results, suffix);
    assert!(
        categories.iter().any(|observed| observed == category),
        "{suffix} should keep a {category} candidate, observed {categories:?}"
    );
}

#[test]
fn imported_inert_callees_are_not_sinks() {
    let results = analyze();
    assert_cleared(&results, "src/use/merge-named.ts");
    assert_cleared(&results, "src/use/merge-default.ts");
    assert_cleared(&results, "src/use/merge-alias.ts");
    assert_cleared(&results, "src/use/request-inert.ts");
}

#[test]
fn imported_callees_with_effects_stay_sinks() {
    let results = analyze();
    assert_reported(&results, "src/use/merge-deep.ts", "prototype-pollution");
    assert_reported(&results, "src/use/merge-mutate.ts", "prototype-pollution");
    assert_reported(&results, "src/use/request-fetch.ts", "ssrf");
}

#[test]
fn callees_beyond_one_direct_hop_stay_sinks() {
    let results = analyze();
    assert_reported(&results, "src/use/merge-external.ts", "prototype-pollution");
    assert_reported(&results, "src/use/merge-barrel.ts", "prototype-pollution");
    assert_reported(
        &results,
        "src/use/merge-namespace.ts",
        "prototype-pollution",
    );
}

fn pollution_lines_on(results: &AnalysisResults, suffix: &str) -> Vec<u32> {
    let mut lines: Vec<u32> = results
        .security_findings
        .iter()
        .filter(|finding| {
            finding
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(suffix)
                && finding.category.as_deref() == Some("prototype-pollution")
        })
        .map(|finding| finding.line)
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

#[test]
fn a_local_binding_that_shadows_the_import_stays_a_sink() {
    let results = analyze();
    assert_eq!(
        pollution_lines_on(&results, "src/use/merge-shadow-local.ts"),
        vec![9],
        "only the call of the shadowing `const merge` is a candidate"
    );
    assert_eq!(
        pollution_lines_on(&results, "src/use/merge-shadow-param.ts"),
        vec![10],
        "only the call of the shadowing parameter is a candidate"
    );
}
