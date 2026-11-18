use super::build_walkthrough_markdown;
use fallow_output::{
    AgentSchema, Decision, DecisionCategory, DecisionSurface, DiffTriage, DirectionUnit,
    FocusLabel, FocusMap, FocusScore, FocusUnit, GraphFacts, INJECTION_NOTE, ImpactClosureFacts,
    PartitionFacts, ReviewBriefSchemaVersion, ReviewDeltas, ReviewDirection, ReviewEffort,
    RiskClass, RoutingFacts, StandardReviewBriefOutput, StandardWalkthroughGuide,
};
use std::path::Path;

fn guide_with_question(file: &str, question: &str) -> StandardWalkthroughGuide {
    let unit = DirectionUnit {
        file: file.to_string(),
        concern_lens: "contract-break".to_string(),
        scoring_budget: 3,
        out_of_diff: vec!["src/consumer.ts".to_string()],
        expert: Vec::new(),
        test_adjacency: None,
    };
    // The direction unit comes FROM the focus map's review_here in reality, so
    // mirror that here: review_here has the one source unit and triage.files
    // matches it, keeping the excluded bucket at 0 for this synthetic guide.
    let review_unit = FocusUnit {
        file: file.to_string(),
        score: FocusScore::default(),
        label: FocusLabel::ReviewHere,
        reason: "reason".to_string(),
        confidence: Vec::new(),
    };
    let decision = Decision {
        signal_id: "sig:1".to_string(),
        category: DecisionCategory::CouplingBoundary,
        question: question.to_string(),
        anchor_file: file.to_string(),
        anchor_line: 1,
        signal_key: "k".to_string(),
        previous_signal_id: None,
        blast: 1,
        consequence: 2,
        expert: Vec::new(),
        bus_factor_one: false,
        internal_consumer_count: 0,
        tradeoff: String::new(),
    };
    let digest = StandardReviewBriefOutput {
        branching: None,
        test_reach: None,
        schema_version: ReviewBriefSchemaVersion::default(),
        version: "test".to_string(),
        command: "audit-brief".to_string(),
        triage: DiffTriage {
            files: 1,
            hunks: None,
            net_lines: None,
            risk_class: RiskClass::Low,
            review_effort: ReviewEffort::Glance,
        },
        graph_facts: GraphFacts {
            exports_added: 0,
            api_width_delta: 0,
            boundaries_touched: Vec::new(),
        },
        partition: PartitionFacts::default(),
        impact_closure: ImpactClosureFacts::default(),
        focus: FocusMap {
            review_here: vec![review_unit],
            deprioritized: Vec::new(),
        },
        deltas: ReviewDeltas::default(),
        weakening: Vec::new(),
        routing: RoutingFacts::default(),
        ownership: None,
        decisions: DecisionSurface {
            decisions: vec![decision],
            truncated: None,
            emitted_signal_ids: Vec::new(),
        },
    };
    StandardWalkthroughGuide {
        schema_version: ReviewBriefSchemaVersion::default(),
        version: "test".to_string(),
        command: "review-walkthrough-guide".to_string(),
        graph_snapshot_hash: "graph:abc".to_string(),
        digest,
        direction: ReviewDirection {
            order: vec![file.to_string()],
            units: vec![unit],
        },
        change_anchors: Vec::new(),
        agent_schema: AgentSchema {
            judgment_shape: "",
            echo_field: "graph_snapshot_hash",
            anchoring_rule: "",
            action_vocabulary: &[],
            concern_vocabulary: &[],
        },
        injection_note: INJECTION_NOTE,
    }
}

#[test]
fn renders_header_stage_and_code_span_badges() {
    let guide = guide_with_question("src/page.ts", "Couple ui to db?");
    let md = build_walkthrough_markdown(&guide, Path::new("/project"), &[]);
    assert!(md.starts_with("## Fallow Review"), "got: {md}");
    assert!(md.contains("### Stage 1"), "got: {md}");
    assert!(md.contains("`COUPLING`"), "badges are code spans: {md}");
    assert!(md.contains("`OUT-OF-DIFF`"), "got: {md}");
    assert!(!md.contains('\u{1b}'), "no ANSI in markdown");
    assert!(
        md.contains("- `src/page.ts`: "),
        "list items use a colon separator: {md}"
    );
    assert!(
        !md.contains("- `src/page.ts` \u{2014} "),
        "no em-dash file separator: {md}"
    );
}

#[test]
fn ungrouped_walkthrough_paths_use_safe_code_spans() {
    let guide = guide_with_question("src/one`# injected.md", "Review this path?");

    let md = build_walkthrough_markdown(&guide, Path::new("/project"), &[]);

    assert!(
        md.contains("- ``src/one`# injected.md``: "),
        "path remains inside one code span: {md}"
    );
    assert!(!md.contains("\\`"));
}

// The markdown surface honors `--mark-viewed`: a viewed file collapses out of
// its stage into the Cleared panel, and the summary reports the viewed count
// (the same on-disk state the human surface reads).
#[test]
fn viewed_file_collapses_into_cleared_in_markdown() {
    let guide = guide_with_question("src/page.ts", "Couple ui to db?");
    let viewed = vec!["src/page.ts".to_string()];
    let md = build_walkthrough_markdown(&guide, Path::new("/project"), &viewed);
    // The viewed file is not rendered in a stage section.
    assert!(
        !md.contains("### Stage 1"),
        "viewed file left its stage: {md}"
    );
    // The Cleared panel reports the viewed count and lists the viewed file.
    assert!(
        md.contains("Cleared (0 de-prioritized, 1 viewed)"),
        "cleared reports viewed count: {md}"
    );
    assert!(
        md.contains("- `src/page.ts`: \u{2713} viewed"),
        "viewed file listed under cleared: {md}"
    );
}

// F5/F7: a coordination question must NOT re-print the anchor path inside the
// fact text, must NOT emit a backslash-backtick sequence, must cap the
// contract member list, and drops the trailing question in the tour.
#[test]
fn fact_does_not_reprint_path_or_emit_escaped_backticks() {
    let q = "`src/page.ts` changes exports (a, b, c, d, e, f, g, h, i) imported by 9 files outside this PR. Does this change break or alter what those callers expect?";
    let guide = guide_with_question("src/page.ts", q);
    let md = build_walkthrough_markdown(&guide, Path::new("/project"), &[]);
    // No backslash-backtick anywhere (the F5 corruption).
    assert!(
        !md.contains("\\`"),
        "fact must never emit a backslash-backtick sequence: {md}"
    );
    // The path is printed once (the bullet lead), not a second time in the fact.
    assert!(
        !md.contains("`src/page.ts` changes exports"),
        "fact must not re-print the path: {md}"
    );
    // The member list is capped with a "+N more".
    assert!(md.contains("+3 more"), "member list capped: {md}");
    // The trailing decision question is dropped in the tour (it lives in the brief).
    assert!(
        !md.contains("break or alter"),
        "the per-file question must be dropped in the tour: {md}"
    );
    // The raw "(score N)" is gone.
    assert!(!md.contains("(score "), "raw score removed: {md}");
}

#[test]
fn empty_order_renders_orientation_only_note() {
    let mut guide = guide_with_question("src/page.ts", "q");
    guide.direction.order.clear();
    guide.direction.units.clear();
    guide.digest.decisions.decisions.clear();
    let md = build_walkthrough_markdown(&guide, Path::new("/project"), &[]);
    assert!(md.contains("orientation only"), "got: {md}");
}

// A decision whose anchor is not a staged unit (a manifest) still renders,
// so a dependency-only change never reads as "nothing to review".
#[test]
fn decision_outside_staged_units_renders_its_own_section() {
    let mut guide = guide_with_question(
        "package.json",
        "`package.json` moves 1 dependency across a major version (`react` ^18 -> ^19), imported by 8 in-repo modules. Which changelog-listed behavior changes reach those importers?",
    );
    guide.direction.order.clear();
    guide.direction.units.clear();
    let md = build_walkthrough_markdown(&guide, Path::new("/project"), &[]);
    assert!(
        md.contains("### Decisions outside the staged files (1)"),
        "got: {md}"
    );
    assert!(md.contains("`package.json`"), "got: {md}");
    assert!(!md.contains("orientation only"), "got: {md}");
}
