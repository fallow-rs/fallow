use fallow_types::results::{AnalysisResults, TOTAL_ISSUE_RESULT_KEYS};
use serde_json::Value;

use super::build_summary_footer;

/// One finding for each array in [`TOTAL_ISSUE_RESULT_KEYS`]. The fixture is
/// JSON so that one test can fill any array from its key.
const ONE_FINDING_PER_KEY: &str = include_str!("check_summary_footer_fixture.json");

fn results_with(key: &str, copies: usize) -> AnalysisResults {
    let fixture: Value = serde_json::from_str(ONE_FINDING_PER_KEY).unwrap();
    let finding = fixture.get(key).unwrap_or_else(|| {
        panic!("add a `{key}` finding to check_summary_footer_fixture.json and a footer label")
    });
    let mut results = serde_json::to_value(AnalysisResults::default()).unwrap();
    results[key] = Value::Array(vec![finding.clone(); copies]);
    serde_json::from_value(results).unwrap()
}

/// A finding type that `total_issues` counts and the footer does not name
/// prints a status line with an empty label, for example `⚠  (0.02s)`.
#[test]
fn every_counted_finding_type_has_a_status_line_label() {
    let unlabeled: Vec<&str> = TOTAL_ISSUE_RESULT_KEYS
        .iter()
        .copied()
        .filter(|key| {
            let results = results_with(key, 1);
            assert_eq!(results.total_issues(), 1, "fixture entry for `{key}`");
            !build_summary_footer(&results, 0, 0).starts_with("1 ")
        })
        .collect();
    assert_eq!(unlabeled, Vec::<&str>::new());
}

#[test]
fn status_line_labels_use_the_singular_and_the_plural() {
    let labels = [
        (
            "unused_catalog_entries",
            "unused catalog entry",
            "unused catalog entries",
        ),
        (
            "empty_catalog_groups",
            "empty catalog group",
            "empty catalog groups",
        ),
        (
            "unresolved_catalog_references",
            "unresolved catalog reference",
            "unresolved catalog references",
        ),
        (
            "unused_dependency_overrides",
            "unused dependency override",
            "unused dependency overrides",
        ),
        (
            "invalid_client_exports",
            "invalid client export",
            "invalid client exports",
        ),
        (
            "mixed_client_server_barrels",
            "mixed client/server barrel",
            "mixed client/server barrels",
        ),
        (
            "misplaced_directives",
            "misplaced directive",
            "misplaced directives",
        ),
        ("route_collisions", "route collision", "route collisions"),
        (
            "dynamic_segment_name_conflicts",
            "dynamic segment conflict",
            "dynamic segment conflicts",
        ),
        (
            "prop_drilling_chains",
            "prop drilling chain",
            "prop drilling chains",
        ),
        ("thin_wrappers", "thin wrapper", "thin wrappers"),
        (
            "duplicate_prop_shapes",
            "duplicate prop shape",
            "duplicate prop shapes",
        ),
    ];
    for (key, singular, plural) in labels {
        assert_eq!(
            build_summary_footer(&results_with(key, 1), 0, 0),
            format!("1 {singular}")
        );
        assert_eq!(
            build_summary_footer(&results_with(key, 2), 0, 0),
            format!("2 {plural}")
        );
    }
}

/// `total_issues` leaves out component health findings without an explicit
/// error severity, so the footer must leave them out too.
#[test]
fn status_line_leaves_out_advisory_component_health_findings() {
    let mut chains = results_with("prop_drilling_chains", 1).prop_drilling_chains;
    chains[0].effective_severity = None;
    let mut wrappers = results_with("thin_wrappers", 1).thin_wrappers;
    wrappers[0].effective_severity = None;
    let mut shapes = results_with("duplicate_prop_shapes", 1).duplicate_prop_shapes;
    shapes[0].effective_severity = None;
    let results = AnalysisResults {
        prop_drilling_chains: chains,
        thin_wrappers: wrappers,
        duplicate_prop_shapes: shapes,
        ..AnalysisResults::default()
    };

    assert_eq!(results.total_issues(), 0);
    assert_eq!(build_summary_footer(&results, 0, 0), "");
}
