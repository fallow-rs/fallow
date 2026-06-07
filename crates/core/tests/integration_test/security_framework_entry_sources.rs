//! Integration tests for framework entry-point sources (#879).

use fallow_config::Severity;
use fallow_core::results::{AnalysisResults, SecurityFinding, SecurityFindingKind};

use super::common::{create_config_with_rules, fixture_path};

fn analyze_fixture(name: &str) -> AnalysisResults {
    let root = fixture_path(name);
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn tainted_sink_at_line(results: &AnalysisResults, line: u32) -> &SecurityFinding {
    results
        .security_findings
        .iter()
        .find(|finding| {
            matches!(finding.kind, SecurityFindingKind::TaintedSink) && finding.line == line
        })
        .unwrap_or_else(|| panic!("tainted sink at line {line}"))
}

#[test]
fn express_route_request_param_is_source_backed() {
    let results = analyze_fixture("security-framework-entry-sources-879-express");
    let finding = tainted_sink_at_line(&results, 5);
    assert!(finding.source_backed);
    assert!(
        finding.evidence.contains("framework handler input"),
        "evidence should name the matched framework source: {}",
        finding.evidence
    );

    let accessor_finding = tainted_sink_at_line(&results, 6);
    assert!(accessor_finding.source_backed);
    assert!(
        accessor_finding.evidence.contains("http request input"),
        "evidence should prefer the specific request accessor source: {}",
        accessor_finding.evidence
    );
}

#[test]
fn generic_route_callback_param_is_not_source_backed_without_enabler() {
    let results = analyze_fixture("security-framework-entry-sources-879-plain");
    let finding = tainted_sink_at_line(&results, 5);
    assert!(!finding.source_backed);
}

#[test]
fn bullmq_worker_job_param_is_source_backed() {
    let results = analyze_fixture("security-framework-entry-sources-879-bullmq");
    let finding = tainted_sink_at_line(&results, 4);
    assert!(finding.source_backed);
    assert!(
        finding.evidence.contains("queue job input"),
        "evidence should name the matched queue source: {}",
        finding.evidence
    );
}

#[test]
fn mcp_tool_input_param_is_source_backed() {
    let results = analyze_fixture("security-framework-entry-sources-879-mcp");
    let finding = tainted_sink_at_line(&results, 6);
    assert!(finding.source_backed);
    assert!(
        finding.evidence.contains("mcp tool input"),
        "evidence should name the matched MCP source: {}",
        finding.evidence
    );
}
