//! Integration tests for taint confidence tiering + trace anchoring (#1093) and
//! the ORM-source receiver gate (#1092).

use fallow_config::Severity;
use fallow_core::results::{
    AnalysisResults, SecurityFinding, SecurityFindingKind, TaintConfidence, TraceHopRole,
};

use super::common::{create_config_with_rules, fixture_path};

fn analyze_fixture(name: &str) -> AnalysisResults {
    let root = fixture_path(name);
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn tainted_sinks(results: &AnalysisResults) -> Vec<&SecurityFinding> {
    results
        .security_findings
        .iter()
        .filter(|f| matches!(f.kind, SecurityFindingKind::TaintedSink))
        .collect()
}

/// 1-based line of the first source line containing `needle` in a fixture file.
fn line_of(fixture: &str, rel: &str, needle: &str) -> u32 {
    let path = fixture_path(fixture).join(rel);
    let source = std::fs::read_to_string(&path).expect("fixture file readable");
    let idx = source
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("`{needle}` not found in {rel}"));
    u32::try_from(idx + 1).expect("line fits u32")
}

#[test]
fn arg_level_finding_is_tiered_and_anchored_at_the_source_read() {
    // `direct`: `const a = req.query.id` then `execSync(`run ${a}`)`. The sink
    // argument traces to the same-module source read, so the candidate is
    // arg-level and the trace source node points at the read line, NOT line 1.
    let results = analyze_fixture("security-taint-confidence-1093");
    let arg_level = tainted_sinks(&results)
        .into_iter()
        .find(|f| f.source_backed)
        .expect("an arg-level (source-backed) tainted sink");

    let reach = arg_level
        .reachability
        .as_ref()
        .expect("arg-level finding must carry reachability (arg-level implies reachable)");
    // Invariant: a source-backed finding is always reachable_from_untrusted_source
    // with a present tier, so a consumer never sees source_backed without a tier.
    assert!(reach.reachable_from_untrusted_source);
    assert_eq!(reach.taint_confidence, Some(TaintConfidence::ArgLevel));

    let source_hop = reach
        .untrusted_source_trace
        .first()
        .expect("trace has a source node");
    assert_eq!(
        source_hop.role,
        TraceHopRole::UntrustedSource,
        "an arg-level source read is labeled untrusted-source"
    );
    let read_line = line_of(
        "security-taint-confidence-1093",
        "src/handlers.ts",
        "const a = req.query.id",
    );
    assert_eq!(
        source_hop.line, read_line,
        "source node anchors at the real read line, not the module import line 1"
    );
    assert_ne!(source_hop.line, 1, "must not point at the import line");

    // taint_flow.source inherits the same anchored node.
    let flow = arg_level.taint_flow.as_ref().expect("taint flow present");
    assert_eq!(flow.source.line, read_line);
}

#[test]
fn module_level_finding_is_tiered_and_labeled_source_module() {
    // `unrelated`: the sink argument is a plain parameter that does NOT trace to
    // any source, but the module contains `req.query.id` (in `direct`), so the
    // sink is module-level reachable. It must be labeled honestly. (Kept distinct
    // from a template/concat binding, which #1095 now treats as arg-level.)
    let results = analyze_fixture("security-taint-confidence-1093");
    let module_level = tainted_sinks(&results)
        .into_iter()
        .find(|f| {
            !f.source_backed
                && f.reachability
                    .as_ref()
                    .is_some_and(|r| r.reachable_from_untrusted_source)
        })
        .expect("a module-level (not source-backed) reachable tainted sink");

    let reach = module_level.reachability.as_ref().expect("reachability");
    assert_eq!(reach.taint_confidence, Some(TaintConfidence::ModuleLevel));
    let source_hop = reach
        .untrusted_source_trace
        .first()
        .expect("trace has a source node");
    assert_eq!(
        source_hop.role,
        TraceHopRole::ModuleSource,
        "a module-level node is labeled source-module, never untrusted-source"
    );
}

#[test]
fn orm_query_builder_does_not_propagate_source_reachability() {
    // #1092: `db.query` (Drizzle) in ./db must NOT classify its module as an
    // untrusted source, so the sink in ./sink (which imports ./db and reads no
    // request input) is NOT reachable_from_untrusted_source.
    let results = analyze_fixture("security-orm-source-1092");
    let sink = tainted_sinks(&results)
        .into_iter()
        .find(|f| f.path.ends_with("src/sink.ts"))
        .expect("the execSync sink in sink.ts");

    assert!(
        !sink.source_backed,
        "the sink reads no request input directly"
    );
    let reachable_from_source = sink
        .reachability
        .as_ref()
        .is_some_and(|r| r.reachable_from_untrusted_source);
    assert!(
        !reachable_from_source,
        "db.query must not make ./db an untrusted-source module that propagates onto this sink"
    );
}
