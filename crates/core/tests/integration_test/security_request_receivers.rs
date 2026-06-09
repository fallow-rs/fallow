//! Integration coverage for project-configured security request receivers
//! (issue #1125).

use fallow_config::Severity;
use fallow_core::results::{AnalysisResults, SecurityFinding, TaintConfidence};

use super::common::{create_config_with_rules, fixture_path};

const FIXTURE: &str = "security-request-receivers-1125";
const HANDLERS: &str = "src/handlers.ts";

fn analyze_with_request_receivers(receivers: Vec<&str>) -> AnalysisResults {
    let root = fixture_path(FIXTURE);
    let mut config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    config.security.request_receivers = receivers.into_iter().map(str::to_string).collect();
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn line_of(needle: &str) -> u32 {
    let path = fixture_path(FIXTURE).join(HANDLERS);
    let source = std::fs::read_to_string(&path).expect("fixture file readable");
    let idx = source
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("`{needle}` not found in {HANDLERS}"));
    u32::try_from(idx + 1).expect("line fits u32")
}

fn sink_on_line<'a>(results: &'a AnalysisResults, needle: &str) -> &'a SecurityFinding {
    let line = line_of(needle);
    results
        .security_findings
        .iter()
        .find(|finding| finding.line == line)
        .unwrap_or_else(|| panic!("expected security finding on line {line} for `{needle}`"))
}

#[test]
fn configured_receiver_makes_binding_source_backed() {
    let results = analyze_with_request_receivers(vec!["h"]);
    let finding = sink_on_line(&results, "alias-binding");

    assert!(
        finding.source_backed,
        "h.query should become a source-backed HTTP request input when configured"
    );
    assert_eq!(
        finding.candidate.source_kind.as_deref(),
        Some("http-request-input")
    );
    let reachability = finding
        .reachability
        .as_ref()
        .expect("source-backed finding should have reachability metadata");
    assert!(
        reachability.reachable_from_untrusted_source,
        "configured receiver should seed untrusted-source reachability"
    );
    assert_eq!(
        reachability.taint_confidence,
        Some(TaintConfidence::ArgLevel)
    );
}

#[test]
fn configured_receiver_applies_to_direct_sink_argument_paths() {
    let results = analyze_with_request_receivers(vec!["httpreq"]);
    let finding = sink_on_line(&results, "alias-direct");

    assert!(
        finding.source_backed,
        "httpReq.body inside the sink argument should use configured receivers too"
    );
    assert_eq!(
        finding.candidate.source_kind.as_deref(),
        Some("http-request-input")
    );
    let reachability = finding
        .reachability
        .as_ref()
        .expect("source-backed finding should have reachability metadata");
    assert!(
        reachability.reachable_from_untrusted_source,
        "direct configured source should seed untrusted-source reachability"
    );
    assert_eq!(
        reachability.taint_confidence,
        Some(TaintConfidence::ArgLevel)
    );
}

#[test]
fn absent_config_keeps_custom_receivers_non_source_backed() {
    let results = analyze_with_request_receivers(Vec::new());
    let finding = sink_on_line(&results, "alias-binding");

    assert!(
        !finding.source_backed,
        "h.query should not be source-backed without requestReceivers config"
    );
}

#[test]
fn built_in_receivers_still_work_without_config() {
    let results = analyze_with_request_receivers(Vec::new());
    let finding = sink_on_line(&results, "built-in");

    assert!(
        finding.source_backed,
        "req.params should remain source-backed without requestReceivers config"
    );
}
