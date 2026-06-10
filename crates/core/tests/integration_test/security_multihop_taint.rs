//! Integration tests for bounded multi-hop taint binding chains (#1146): a
//! source reaching a sink through up to three chained same-module local
//! bindings is arg-level with the trace anchored at the original read, while
//! over-cap chains degrade to module-level (never a false arg-level).

use fallow_config::Severity;
use fallow_core::results::{AnalysisResults, SecurityFinding, TaintConfidence, TraceHopRole};

use super::common::{create_config_with_rules, fixture_path};

const FIXTURE: &str = "security-multihop-taint-1146";

fn analyze_fixture() -> AnalysisResults {
    let root = fixture_path(FIXTURE);
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn finding_on<'a>(results: &'a AnalysisResults, suffix: &str) -> &'a SecurityFinding {
    results
        .security_findings
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(suffix)
        })
        .unwrap_or_else(|| panic!("{suffix} should produce a security finding"))
}

/// 1-based line of the first source line containing `needle` in a fixture file.
fn line_of(rel: &str, needle: &str) -> u32 {
    let path = fixture_path(FIXTURE).join(rel);
    let source = std::fs::read_to_string(&path).expect("fixture file readable");
    let idx = source
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("`{needle}` not found in {rel}"));
    u32::try_from(idx + 1).expect("line fits u32")
}

#[test]
fn two_hop_chain_is_arg_level_and_anchored_at_the_original_read() {
    // The issue headline: `const a = req.query.id; const b = `wrap-${a}`;
    // execSync(`run ${b}`)`. The chained binding carries the original source
    // path and span, so the candidate is arg-level and the trace source node
    // points at the read line, not the chain step or the import line.
    let results = analyze_fixture();
    let finding = finding_on(&results, "src/two-hop.ts");

    assert_eq!(finding.category.as_deref(), Some("command-injection"));
    assert!(finding.source_backed, "a 2-hop chain must be source-backed");
    let reach = finding.reachability.as_ref().expect("reachability present");
    assert_eq!(reach.taint_confidence, Some(TaintConfidence::ArgLevel));

    let source_hop = reach
        .untrusted_source_trace
        .first()
        .expect("trace has a source node");
    assert_eq!(source_hop.role, TraceHopRole::UntrustedSource);
    let read_line = line_of("src/two-hop.ts", "const a = req.query.id");
    assert_eq!(
        source_hop.line, read_line,
        "source node anchors at the ORIGINAL read, not an intermediate binding"
    );
    assert_ne!(source_hop.line, 1, "must not point at the import line");
}

#[test]
fn three_hop_chain_at_the_cap_is_arg_level() {
    let results = analyze_fixture();
    let finding = finding_on(&results, "src/three-hop.ts");

    assert_eq!(finding.category.as_deref(), Some("command-injection"));
    assert!(
        finding.source_backed,
        "a 3-binding chain sits exactly at the cap and is still arg-level"
    );
    let reach = finding.reachability.as_ref().expect("reachability present");
    assert_eq!(reach.taint_confidence, Some(TaintConfidence::ArgLevel));
}

#[test]
fn four_hop_chain_over_the_cap_degrades_to_module_level() {
    // The candidate must still EXIST (command-injection fires on the
    // non-literal argument regardless of source-backing) and must carry an
    // explicit module-level tier: asserting only "not arg-level" would pass
    // vacuously if the candidate were dropped for an unrelated reason.
    let results = analyze_fixture();
    let finding = finding_on(&results, "src/four-hop.ts");

    assert_eq!(finding.category.as_deref(), Some("command-injection"));
    assert!(
        !finding.source_backed,
        "a 4-binding chain exceeds the cap and must not claim arg-level"
    );
    let reach = finding
        .reachability
        .as_ref()
        .expect("the module contains a real source read, so the sink is module-level reachable");
    assert!(reach.reachable_from_untrusted_source);
    assert_eq!(reach.taint_confidence, Some(TaintConfidence::ModuleLevel));
    let source_hop = reach
        .untrusted_source_trace
        .first()
        .expect("trace has a source node");
    assert_eq!(
        source_hop.role,
        TraceHopRole::ModuleSource,
        "an over-cap chain is labeled module-source, never untrusted-source"
    );
}
