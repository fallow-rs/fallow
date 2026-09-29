//! Two health analyses in one process must not inherit each other's degraded
//! inputs.
//!
//! The health envelope's `workspace_diagnostics` is assembled from a snapshot
//! captured before the analysis starts plus a registry read taken at finalize
//! time. The capture happens before the health-stage clear on every route, so
//! without a filter the second analysis in a long-lived process reports the
//! first one's entries as well. That matters because six of the seven
//! health-stage kinds answer `degrades_analysis: true`, which is what the MCP
//! server's degraded-run sentence and both shipped CI integrations select on:
//! an input that loaded fine would be reported as degraded for the life of the
//! process (issue #2689).
//!
//! This is the shipped route, not a stand-in: `fallow_api::run_health` is what
//! the MCP `check_health` tool calls.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap/expect to keep fixture setup concise"
)]

use fallow_api::{AnalysisOptions, ComplexityOptions, run_health};

/// A project whose coverage file makes health record `coverage-auto-detected`,
/// the one health-stage kind that is provenance rather than a failure and is
/// therefore trivial to stop applying: delete the file.
fn project_with_auto_detected_coverage() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    std::fs::create_dir(root.join("src")).expect("src dir");
    std::fs::create_dir(root.join("coverage")).expect("coverage dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"health-rerun-fixture","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "export const entry = (): number => 1;\nconsole.log(entry());\n",
    )
    .expect("entry");
    std::fs::write(root.join("coverage/coverage-final.json"), "{}").expect("coverage");
    project
}

fn health_kinds(root: &std::path::Path) -> Vec<String> {
    let run = run_health(&ComplexityOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        complexity: true,
        score: true,
        // Asked for so the fixture also carries a health-stage entry that keeps
        // applying (no git repository at the root), which is the half of the
        // filter that must not drop anything.
        hotspots: true,
        ..ComplexityOptions::default()
    })
    .expect("health runs");
    run.workspace_diagnostics
        .iter()
        .map(|diagnostic| diagnostic.kind.id().to_owned())
        .collect()
}

#[test]
fn a_second_health_analysis_reports_only_what_still_applies() {
    let project = project_with_auto_detected_coverage();
    let root = project.path();

    let first = health_kinds(root);
    assert!(
        first.iter().any(|kind| kind == "coverage-auto-detected"),
        "the first analysis auto-detects the coverage file: {first:?}"
    );

    std::fs::remove_file(root.join("coverage/coverage-final.json")).expect("remove coverage");

    let second = health_kinds(root);
    assert!(
        !second.iter().any(|kind| kind == "coverage-auto-detected"),
        "the second analysis in the same process must not inherit the first's \
         health-stage entries: {second:?}"
    );
    assert!(
        second.iter().any(|kind| kind == "hotspots-skipped"),
        "a health-stage entry that still applies is still reported: {second:?}"
    );
}
