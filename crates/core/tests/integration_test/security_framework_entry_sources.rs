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
    let finding = tainted_sink_at_line(&results, 8);
    assert!(finding.source_backed);
    assert!(
        finding.evidence.contains("framework handler input"),
        "evidence should name the matched framework source: {}",
        finding.evidence
    );

    let accessor_finding = tainted_sink_at_line(&results, 9);
    assert!(accessor_finding.source_backed);
    assert!(
        accessor_finding.evidence.contains("http request input"),
        "evidence should prefer the specific request accessor source: {}",
        accessor_finding.evidence
    );
}

#[test]
fn express_source_backed_finding_carries_candidate_source_kind() {
    // Issue #900: slot 1 (source_kind) is the stable catalogue source id, and
    // the sink slot carries the callee. The framework-handler param at line 8
    // matches the framework source; the specific request accessor at line 9
    // matches the http-request source.
    let results = analyze_fixture("security-framework-entry-sources-879-express");

    let handler = tainted_sink_at_line(&results, 8);
    assert_eq!(
        handler.candidate.source_kind.as_deref(),
        Some("framework-handler-input"),
        "slot 1 should be the stable catalogue source id, not the human title"
    );
    assert!(
        handler.candidate.sink.callee.is_some(),
        "the sink slot should carry the captured callee"
    );
    assert_eq!(handler.candidate.sink.category, handler.category);

    let accessor = tainted_sink_at_line(&results, 9);
    assert_eq!(
        accessor.candidate.source_kind.as_deref(),
        Some("http-request-input")
    );
}

#[test]
fn express_enabled_non_route_get_callback_is_not_source_backed() {
    let results = analyze_fixture("security-framework-entry-sources-879-express");
    assert!(
        results
            .security_findings
            .iter()
            .all(|finding| finding.line != 12),
        "ordinary cache.get callbacks must not be treated as framework request sources"
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

#[test]
fn graphql_resolver_args_param_is_source_backed() {
    let results = analyze_fixture("security-framework-entry-sources-899-graphql");
    let finding = tainted_sink_at_line(&results, 4);
    assert!(finding.source_backed);
    assert!(
        finding.evidence.contains("graphql resolver args"),
        "evidence should name the matched GraphQL source: {}",
        finding.evidence
    );
}

#[test]
fn graphql_resolver_args_requires_enabler() {
    let results = analyze_fixture("security-framework-entry-sources-899-plain-graphql");
    let finding = tainted_sink_at_line(&results, 4);
    assert!(!finding.source_backed);
}

#[test]
fn trpc_procedure_input_is_source_backed() {
    let results = analyze_fixture("security-framework-entry-sources-899-trpc");
    let finding = tainted_sink_at_line(&results, 13);
    assert!(finding.source_backed);
    assert!(
        finding.evidence.contains("trpc procedure input"),
        "evidence should name the matched tRPC source: {}",
        finding.evidence
    );
}
