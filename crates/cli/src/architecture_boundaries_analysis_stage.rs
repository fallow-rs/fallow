//! The analysis-stage diagnostics are recorded only from the dead-code
//! analyze pass.

use super::*;

/// Issue #2366: `WorkspaceDiagnosticKind::is_analysis_stage` may only classify a kind `true`
/// when the dead-code analyze pass re-records it, because that pass is the single site that
/// clears the previous run's analysis-stage entries. The exhaustive match in `fallow-types`
/// forces a NEW kind to be classified, but nothing there notices an EXISTING recorder moving
/// out of the analyze pass; a re-homed recorder would be cleared on every run and its kind
/// would silently vanish from every envelope. Pin the recorder set so re-homing one trips
/// this test instead.
#[test]
fn analysis_stage_diagnostics_are_recorded_only_from_the_dead_code_analyze_pass() {
    // The registry owns the function; this guard names it to pin the set.
    let exempt = [
        "crates/config/src/workspace/diagnostics.rs",
        "crates/cli/src/architecture_boundaries_analysis_stage.rs",
    ];
    let expected = [
        "crates/core/src/analyze/mod.rs",
        "crates/core/src/analyze/public_packages.rs",
        "crates/core/src/analyze/unconfigured_check.rs",
        "crates/core/src/analyze/unused_catalog/mod.rs",
        "crates/core/src/analyze/unused_overrides.rs",
    ];

    let mut callers: Vec<String> = rust_sources_under(["crates"])
        .into_iter()
        .filter(|path| !exempt.contains(&path.as_str()))
        .filter(|path| {
            read_source_without_line_comments(path)
                .expect("read crate source")
                .contains("record_workspace_diagnostics(")
        })
        .collect();
    callers.sort();

    assert_eq!(
        callers, expected,
        "record_workspace_diagnostics must stay inside the detectors the dead-code analyze pass \
         runs; a recorder moved elsewhere loses its kind from every envelope on the next pass"
    );

    let analyze = read_source_without_line_comments("crates/core/src/analyze/mod.rs")
        .expect("read core analyze module");
    assert!(
        analyze.contains("clear_analysis_stage_diagnostics(&config.root)"),
        "find_dead_code_full must clear the previous pass's analysis-stage diagnostics before \
         the detectors re-record this pass's"
    );
    for recorder in [
        "public_packages",
        "unconfigured_check",
        "unused_catalog",
        "unused_overrides",
    ] {
        assert!(
            analyze.contains(recorder),
            "the dead-code analyze pass must reach the {recorder} detector"
        );
    }
}
