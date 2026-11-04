//! `duplicate-export-alias`: one module exports one binding under two or more
//! names. The fixture holds the reported shapes and the shapes that must stay
//! quiet: two bindings with one value, a value and a type that share a name
//! (`merged-name.ts`), function overloads, `export =` and `export *`. In
//! `type-and-value.ts`, the type `Shape` is exported as `Shape` and as
//! `ShapeType`, so the type group is reported and the value is not.

use super::common::{create_config, fixture_path};

fn findings_by_file(results: &fallow_core::results::AnalysisResults) -> Vec<(String, Vec<String>)> {
    let root = fixture_path("duplicate-export-aliases");
    let mut findings: Vec<(String, Vec<String>)> = results
        .duplicate_export_aliases
        .iter()
        .map(|finding| {
            let path = finding
                .alias
                .path
                .strip_prefix(&root)
                .unwrap_or(&finding.alias.path)
                .to_string_lossy()
                .replace('\\', "/");
            (path, finding.alias.export_names.clone())
        })
        .collect();
    findings.sort();
    findings
}

#[test]
fn reports_one_binding_exported_under_two_names() {
    let config = create_config(fixture_path("duplicate-export-aliases"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let findings = findings_by_file(&results);
    let names = |list: &[&str]| list.iter().map(ToString::to_string).collect::<Vec<_>>();

    assert_eq!(
        findings,
        vec![
            (
                "src/default-alias.ts".to_string(),
                names(&["value", "default"])
            ),
            ("src/forward.ts".to_string(), names(&["r", "r2"])),
            ("src/ignored.ts".to_string(), names(&["pub", "default"])),
            (
                "src/interface-alias.ts".to_string(),
                names(&["Props", "LegacyProps"])
            ),
            (
                "src/named-default-function.ts".to_string(),
                names(&["default", "foo"])
            ),
            (
                "src/re-export-specifiers.ts".to_string(),
                names(&["r", "r3"])
            ),
            ("src/re-exports.ts".to_string(), names(&["q", "q2"])),
            ("src/specifiers.ts".to_string(), names(&["a", "b"])),
            (
                "src/type-and-value.ts".to_string(),
                names(&["Shape", "ShapeType"])
            ),
            ("src/worker.ts".to_string(), names(&["layer", "default"])),
        ],
    );
    // The other rule keeps its meaning: same name across files only.
    assert!(results.duplicate_exports.is_empty());
}

#[test]
fn finding_carries_local_name_and_locations() {
    let config = create_config(fixture_path("duplicate-export-aliases"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let worker = results
        .duplicate_export_aliases
        .iter()
        .find(|finding| finding.alias.path.ends_with("src/worker.ts"))
        .expect("worker.ts should be reported");
    assert_eq!(worker.alias.local_name, "layer");
    assert_eq!((worker.alias.line, worker.alias.col), (1, 13));
    let lines: Vec<u32> = worker.alias.locations.iter().map(|l| l.line).collect();
    assert_eq!(lines, vec![1, 2]);
}

#[test]
fn rule_off_reports_nothing() {
    let mut config = create_config(fixture_path("duplicate-export-aliases"));
    config.rules.duplicate_export_aliases = fallow_config::Severity::Off;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(results.duplicate_export_aliases.is_empty());
}

#[test]
fn line_suppression_is_consumed_and_not_stale() {
    let config = create_config(fixture_path("duplicate-export-aliases"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        !results
            .stale_suppressions
            .iter()
            .any(|stale| stale.path.ends_with("src/suppressed.ts")),
        "the suppression in suppressed.ts hides a real finding"
    );
}
