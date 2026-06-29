//! Human terminal renderer for `fallow review --walkthrough`.
//!
//! Renders the EXISTING [`StandardWalkthroughGuide`] (already built by
//! `crate::audit_walkthrough::build_guide_from_result`) as a staged, codiff-style
//! review tour. This module is a PURE line-builder over the in-memory guide; it
//! reads no JSON and performs no IO, so it is unit-testable and the entry point
//! in `audit_brief.rs` owns the stdout/stderr split and the viewed-state IO.
//!
//! ## Stages
//!
//! The guide has no literal stage array, so two ordered stages are synthesized
//! from `direction.units` partitioned by `concern_lens`, preserving
//! `direction.order` within each:
//!   - **Stage 1, load-bearing** (`concern_lens == "contract-break"`): units with
//!     out-of-diff consumers.
//!   - **Stage 2, mechanical** (the rest): orientation-only units.
//!
//! ## Badges
//!
//! Badges are SYNTHESIZED at render time from the guide's nested data (the guide
//! pre-computes none): COUPLING / PUBLIC-API / DEPENDENCY from
//! `digest.decisions`, OUT-OF-DIFF from the contract-break lens, OWNER / BUS-FACTOR-1
//! from the unit's routed expert, WEAKENED from `digest.weakening`, INTRODUCED from
//! `digest.deltas`, and VIEWED from the local viewed-state. They are render-only,
//! never a wire field.

use colored::Colorize;
use fallow_output::{
    DecisionCategory, DirectionUnit, ReviewEffort, RiskClass, StandardWalkthroughGuide,
};

use crate::walkthrough_state::ViewedState;

use super::{MAX_FLAT_ITEMS, format_path};
use crate::report::plural;

/// Max out-of-diff consumers named on a per-file fact line before truncating.
const MAX_NAMED_CONSUMERS: usize = 3;

/// Max characters of a per-file "why" fact line before truncating with an ellipsis.
const FACT_LINE_MAX: usize = 120;

/// Inputs to the human tour builder, bundled so the per-file row helpers do not
/// each take a long parameter list.
pub(in crate::report) struct WalkthroughHumanInput<'a> {
    pub(in crate::report) guide: &'a StandardWalkthroughGuide,
    pub(in crate::report) viewed: &'a ViewedState,
    /// Expand the Cleared panel (de-prioritized + viewed) instead of collapsing.
    pub(in crate::report) show_cleared: bool,
}

/// Build the staged human walkthrough tour as a vector of (already colored)
/// lines. The Review Focus header and final status are NOT included here; the
/// entry point emits those to stderr. This returns the tour BODY (stdout).
#[must_use]
pub(in crate::report) fn build_walkthrough_human_lines(
    input: &WalkthroughHumanInput<'_>,
) -> Vec<String> {
    let guide = input.guide;
    let mut lines = Vec::new();

    if guide.direction.order.is_empty() {
        lines.push(
            "No reviewable units in this change (orientation only)."
                .dimmed()
                .to_string(),
        );
        return lines;
    }

    let (stage1, stage2) = partition_stages(guide);
    push_stage(
        &mut lines,
        "Stage 1: Load-bearing (contract-break)",
        &stage1,
        input,
        true,
    );
    push_stage(
        &mut lines,
        "Stage 2: Mechanical (orientation)",
        &stage2,
        input,
        false,
    );
    push_cleared_panel(&mut lines, input);

    lines
}

/// Partition the guide's units into (contract-break, orientation), each in
/// `direction.order`. Files in `order` with no matching unit are skipped.
fn partition_stages(
    guide: &StandardWalkthroughGuide,
) -> (Vec<&DirectionUnit>, Vec<&DirectionUnit>) {
    let mut load_bearing = Vec::new();
    let mut mechanical = Vec::new();
    for file in &guide.direction.order {
        let Some(unit) = guide.direction.units.iter().find(|u| &u.file == file) else {
            continue;
        };
        if unit.concern_lens == "contract-break" {
            load_bearing.push(unit);
        } else {
            mechanical.push(unit);
        }
    }
    (load_bearing, mechanical)
}

/// Render one stage: a colored header plus each file's row. Skipped when empty.
fn push_stage(
    lines: &mut Vec<String>,
    title: &str,
    units: &[&DirectionUnit],
    input: &WalkthroughHumanInput<'_>,
    load_bearing: bool,
) {
    if units.is_empty() {
        return;
    }
    let header = format!("{title} ({})", units.len());
    let bullet = "\u{25cf}";
    let colored = if load_bearing {
        format!("{} {}", bullet.yellow(), header.yellow().bold())
    } else {
        format!("{} {}", bullet.cyan(), header.cyan().bold())
    };
    lines.push(String::new());
    lines.push(colored);
    for unit in units {
        push_file_row(lines, unit, input);
    }
}

/// Render one file's row: the header line (path + score + badges) and the
/// one-line fact beneath it.
fn push_file_row(lines: &mut Vec<String>, unit: &DirectionUnit, input: &WalkthroughHumanInput<'_>) {
    let badges = synthesize_badges(unit, input);
    let score = format!("(score {})", unit.scoring_budget).dimmed();
    let badge_suffix = if badges.is_empty() {
        String::new()
    } else {
        format!(" {}", badges.join(" "))
    };
    lines.push(format!(
        "  {} {score}{badge_suffix}",
        format_path(&unit.file)
    ));
    lines.push(format!("    {}", fact_line(unit, input.guide).dimmed()));
}

/// The one-line "why" for a file: the strongest available signal. Decision
/// question (anchored at this file) > out-of-diff consumers > focus reason >
/// orientation-only.
fn fact_line(unit: &DirectionUnit, guide: &StandardWalkthroughGuide) -> String {
    if let Some(decision) = guide
        .digest
        .decisions
        .decisions
        .iter()
        .find(|d| d.anchor_file == unit.file)
    {
        return truncate(&decision.question, FACT_LINE_MAX);
    }
    if !unit.out_of_diff.is_empty() {
        return out_of_diff_fact(unit);
    }
    if let Some(reason) = focus_reason(unit, guide) {
        return truncate(reason, FACT_LINE_MAX);
    }
    "orientation only".to_string()
}

/// The out-of-diff consumer fact: a count plus the first few consumer paths.
fn out_of_diff_fact(unit: &DirectionUnit) -> String {
    let total = unit.out_of_diff.len();
    let named: Vec<&str> = unit
        .out_of_diff
        .iter()
        .take(MAX_NAMED_CONSUMERS)
        .map(String::as_str)
        .collect();
    let more = if total > named.len() {
        format!(" (+{} more)", total - named.len())
    } else {
        String::new()
    };
    format!(
        "{total} out-of-diff consumer{}: {}{more}",
        plural(total),
        named.join(", ")
    )
}

/// Look up the focus-map reason for a file, searching both the review-here and
/// de-prioritized lists.
fn focus_reason<'a>(unit: &DirectionUnit, guide: &'a StandardWalkthroughGuide) -> Option<&'a str> {
    guide
        .digest
        .focus
        .review_here
        .iter()
        .chain(guide.digest.focus.deprioritized.iter())
        .find(|fu| fu.file == unit.file)
        .map(|fu| fu.reason.as_str())
}

/// Synthesize the colored badge chips that apply to this file.
fn synthesize_badges(unit: &DirectionUnit, input: &WalkthroughHumanInput<'_>) -> Vec<String> {
    let guide = input.guide;
    let mut badges = Vec::new();

    push_decision_badges(&mut badges, &unit.file, guide);
    if introduced_here(&unit.file, guide) {
        badges.push("INTRODUCED".magenta().to_string());
    }
    if unit.concern_lens == "contract-break" {
        badges.push("OUT-OF-DIFF".yellow().to_string());
    }
    if let Some(owner) = unit.expert.first() {
        badges.push(format!("OWNER:{owner}").dimmed().to_string());
    }
    if bus_factor_one(&unit.file, guide) {
        badges.push("BUS-FACTOR-1".red().to_string());
    }
    if weakened_here(&unit.file, guide) {
        badges.push("WEAKENED".yellow().to_string());
    }
    if input
        .viewed
        .is_viewed(&unit.file, &guide.graph_snapshot_hash)
    {
        badges.push("\u{2713} viewed".dimmed().to_string());
    }
    badges
}

/// Push the decision-category badge(s) for any decision anchored at `file`.
fn push_decision_badges(badges: &mut Vec<String>, file: &str, guide: &StandardWalkthroughGuide) {
    for decision in &guide.digest.decisions.decisions {
        if decision.anchor_file != file {
            continue;
        }
        let token = match decision.category {
            DecisionCategory::CouplingBoundary => "COUPLING",
            DecisionCategory::PublicApiContract => "PUBLIC-API",
            DecisionCategory::Dependency => "DEPENDENCY",
        };
        let colored = token.cyan().to_string();
        if !badges.contains(&colored) {
            badges.push(colored);
        }
    }
}

/// Whether `file` is named in any "introduced vs base" delta (a new boundary
/// edge, a new cycle, or an added public-API export).
fn introduced_here(file: &str, guide: &StandardWalkthroughGuide) -> bool {
    let deltas = &guide.digest.deltas;
    deltas
        .boundary_introduced
        .iter()
        .chain(deltas.cycle_introduced.iter())
        .chain(deltas.public_api_added.iter())
        .any(|entry| entry.contains(file))
}

/// Whether the routed expert for `file` is a single contributor (bus-factor-1).
fn bus_factor_one(file: &str, guide: &StandardWalkthroughGuide) -> bool {
    guide
        .digest
        .routing
        .units
        .iter()
        .any(|u| u.file == file && u.bus_factor_one)
}

/// Whether any weakening signal was detected in `file`.
fn weakened_here(file: &str, guide: &StandardWalkthroughGuide) -> bool {
    guide.digest.weakening.iter().any(|w| w.file == file)
}

/// Render the Cleared panel: a single collapsed summary line by default, or the
/// full de-prioritized + viewed list under `--show-cleared`.
fn push_cleared_panel(lines: &mut Vec<String>, input: &WalkthroughHumanInput<'_>) {
    let guide = input.guide;
    let deprioritized = &guide.digest.focus.deprioritized;
    let viewed_count = input.viewed.viewed_count(
        guide.direction.order.iter().map(String::as_str),
        &guide.graph_snapshot_hash,
    );

    if deprioritized.is_empty() && viewed_count == 0 {
        return;
    }

    lines.push(String::new());
    if !input.show_cleared {
        lines.push(
            format!(
                "\u{25b8} Cleared ({} de-prioritized, {} viewed) \u{2014} pass --show-cleared to expand",
                deprioritized.len(),
                viewed_count,
            )
            .dimmed()
            .to_string(),
        );
        return;
    }

    lines.push(
        format!(
            "\u{25be} Cleared ({} de-prioritized, {} viewed)",
            deprioritized.len(),
            viewed_count,
        )
        .dimmed()
        .to_string(),
    );
    push_cleared_detail(lines, input);
}

/// Expanded Cleared detail: each de-prioritized file (with its reason) and each
/// viewed file, truncating long lists.
fn push_cleared_detail(lines: &mut Vec<String>, input: &WalkthroughHumanInput<'_>) {
    let guide = input.guide;
    let deprioritized = &guide.digest.focus.deprioritized;
    let shown = deprioritized.len().min(MAX_FLAT_ITEMS);
    for unit in &deprioritized[..shown] {
        lines.push(format!(
            "    {} {}",
            format_path(&unit.file),
            unit.reason.dimmed()
        ));
    }
    if deprioritized.len() > MAX_FLAT_ITEMS {
        lines.push(format!(
            "    {}",
            format!(
                "... and {} more (--format json for full list)",
                deprioritized.len() - MAX_FLAT_ITEMS
            )
            .dimmed()
        ));
    }
    for file in &guide.direction.order {
        if input.viewed.is_viewed(file, &guide.graph_snapshot_hash) {
            lines.push(format!(
                "    {} {}",
                format_path(file),
                "\u{2713} viewed".dimmed()
            ));
        }
    }
}

/// The Review Focus orientation header lines (rendered to stderr by the entry
/// point). Built from the guide's triage + graph facts. Never a verdict.
#[must_use]
pub(in crate::report) fn build_focus_header(guide: &StandardWalkthroughGuide) -> Vec<String> {
    let triage = &guide.digest.triage;
    let mut lines = Vec::new();
    lines.push(format!(
        "{} {}",
        "\u{25cf}".cyan(),
        format!(
            "Review Focus \u{2014} {} risk \u{00b7} {} \u{00b7} {} file{}",
            risk_label(triage.risk_class),
            effort_label(triage.review_effort),
            triage.files,
            plural(triage.files),
        )
        .cyan()
        .bold()
    ));
    if let Some(sub) = focus_subline(guide) {
        lines.push(format!("  {}", sub.dimmed()));
    }
    lines
}

/// The optional dim sub-line: boundaries touched + affected-not-shown counts.
fn focus_subline(guide: &StandardWalkthroughGuide) -> Option<String> {
    let facts = &guide.digest.graph_facts;
    let closure = &guide.digest.impact_closure;
    let mut parts = Vec::new();
    if !facts.boundaries_touched.is_empty() {
        parts.push(format!(
            "{} boundary zone{} touched",
            facts.boundaries_touched.len(),
            plural(facts.boundaries_touched.len())
        ));
    }
    if !closure.affected_not_shown.is_empty() {
        parts.push(format!(
            "{} file{} affected beyond the diff",
            closure.affected_not_shown.len(),
            plural(closure.affected_not_shown.len())
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" \u{00b7} "))
    }
}

/// The final green status line (rendered to stderr by the entry point). Never a
/// failure glyph; the walkthrough always exits 0.
#[must_use]
pub(in crate::report) fn build_status_line(guide: &StandardWalkthroughGuide) -> String {
    let files = guide.direction.order.len();
    format!(
        "{} Walkthrough ready \u{2014} {} file{} across 2 stages",
        "\u{2713}".green(),
        files,
        plural(files),
    )
    .green()
    .to_string()
}

fn risk_label(risk: RiskClass) -> &'static str {
    match risk {
        RiskClass::Low => "low",
        RiskClass::Medium => "medium",
        RiskClass::High => "high",
    }
}

fn effort_label(effort: ReviewEffort) -> &'static str {
    match effort {
        ReviewEffort::Glance => "glance",
        ReviewEffort::Review => "review",
        ReviewEffort::DeepDive => "deep-dive",
    }
}

/// Truncate `s` to at most `max` chars, appending an ellipsis when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}\u{2026}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::human::plain;
    use fallow_output::{
        AgentSchema, Decision, DecisionCategory, DecisionSurface, DiffTriage, DirectionUnit,
        FocusLabel, FocusMap, FocusScore, FocusUnit, INJECTION_NOTE, ImpactClosureFacts,
        PartitionFacts, ReviewBriefSchemaVersion, ReviewDeltas, ReviewDirection, ReviewEffort,
        RiskClass, RoutingFacts, RoutingUnit, StandardReviewBriefOutput, WeakeningKind,
        WeakeningSignal,
    };

    fn focus_unit(file: &str, label: FocusLabel) -> FocusUnit {
        FocusUnit {
            file: file.to_string(),
            score: FocusScore::default(),
            label,
            reason: format!("reason for {file}"),
            confidence: Vec::new(),
        }
    }

    fn unit(file: &str, lens: &str, out_of_diff: Vec<String>) -> DirectionUnit {
        DirectionUnit {
            file: file.to_string(),
            concern_lens: lens.to_string(),
            scoring_budget: 3,
            out_of_diff,
            expert: Vec::new(),
        }
    }

    fn guide_with(
        units: Vec<DirectionUnit>,
        decisions: Vec<Decision>,
        deprioritized: Vec<FocusUnit>,
        routing: RoutingFacts,
        weakening: Vec<WeakeningSignal>,
    ) -> StandardWalkthroughGuide {
        let order = units.iter().map(|u| u.file.clone()).collect();
        let digest = StandardReviewBriefOutput {
            schema_version: ReviewBriefSchemaVersion::default(),
            version: "test".to_string(),
            command: "audit-brief".to_string(),
            triage: DiffTriage {
                files: order_len(&units),
                hunks: None,
                net_lines: None,
                risk_class: RiskClass::Medium,
                review_effort: ReviewEffort::Review,
            },
            graph_facts: fallow_output::GraphFacts {
                exports_added: 0,
                api_width_delta: 0,
                reachable_from: Vec::new(),
                boundaries_touched: Vec::new(),
            },
            partition: PartitionFacts::default(),
            impact_closure: ImpactClosureFacts::default(),
            focus: FocusMap {
                review_here: Vec::new(),
                deprioritized,
            },
            deltas: ReviewDeltas::default(),
            weakening,
            routing,
            decisions: DecisionSurface {
                decisions,
                truncated: None,
                emitted_signal_ids: Vec::new(),
            },
        };
        StandardWalkthroughGuide {
            schema_version: ReviewBriefSchemaVersion::default(),
            version: "test".to_string(),
            command: "review-walkthrough-guide".to_string(),
            graph_snapshot_hash: "hash1".to_string(),
            digest,
            direction: ReviewDirection { order, units },
            change_anchors: Vec::new(),
            agent_schema: AgentSchema {
                judgment_shape: "",
                echo_field: "graph_snapshot_hash",
                anchoring_rule: "",
            },
            injection_note: INJECTION_NOTE,
        }
    }

    fn order_len(units: &[DirectionUnit]) -> usize {
        units.len()
    }

    fn coupling_decision(file: &str) -> Decision {
        Decision {
            signal_id: "sig:1".to_string(),
            category: DecisionCategory::CouplingBoundary,
            question: "Couple ui to db?".to_string(),
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
        }
    }

    #[test]
    fn empty_order_renders_graceful_empty_state() {
        let guide = guide_with(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            RoutingFacts::default(),
            Vec::new(),
        );
        let viewed = ViewedState::default();
        let lines = build_walkthrough_human_lines(&WalkthroughHumanInput {
            guide: &guide,
            viewed: &viewed,
            show_cleared: false,
        });
        let text = plain(&lines);
        assert!(text.contains("No reviewable units"), "got: {text}");
    }

    #[test]
    fn partitions_into_two_stages_in_order() {
        let units = vec![
            unit(
                "src/page.ts",
                "contract-break",
                vec!["src/consumer.ts".to_string()],
            ),
            unit("src/util.ts", "orientation", Vec::new()),
        ];
        let guide = guide_with(
            units,
            vec![coupling_decision("src/page.ts")],
            Vec::new(),
            RoutingFacts::default(),
            Vec::new(),
        );
        let viewed = ViewedState::default();
        let lines = build_walkthrough_human_lines(&WalkthroughHumanInput {
            guide: &guide,
            viewed: &viewed,
            show_cleared: false,
        });
        let text = plain(&lines);
        assert!(text.contains("Stage 1: Load-bearing"), "got: {text}");
        assert!(text.contains("Stage 2: Mechanical"), "got: {text}");
        assert!(text.contains("page.ts"));
        assert!(text.contains("util.ts"));
        // Stage 1 appears before Stage 2.
        let s1 = text.find("Stage 1").unwrap();
        let s2 = text.find("Stage 2").unwrap();
        assert!(s1 < s2);
        // The coupling decision badge renders on the load-bearing file.
        assert!(text.contains("COUPLING"), "got: {text}");
        assert!(text.contains("OUT-OF-DIFF"), "got: {text}");
    }

    #[test]
    fn cleared_panel_collapses_by_default_and_expands() {
        let units = vec![unit("src/page.ts", "orientation", Vec::new())];
        let deprioritized = vec![focus_unit("src/old.ts", FocusLabel::NotPrioritized)];
        let guide = guide_with(
            units,
            Vec::new(),
            deprioritized,
            RoutingFacts::default(),
            Vec::new(),
        );
        let viewed = ViewedState::default();

        let collapsed = plain(&build_walkthrough_human_lines(&WalkthroughHumanInput {
            guide: &guide,
            viewed: &viewed,
            show_cleared: false,
        }));
        assert!(
            collapsed.contains("Cleared (1 de-prioritized"),
            "got: {collapsed}"
        );
        assert!(collapsed.contains("--show-cleared"), "got: {collapsed}");
        assert!(
            !collapsed.contains("old.ts"),
            "collapsed must not list files: {collapsed}"
        );

        let expanded = plain(&build_walkthrough_human_lines(&WalkthroughHumanInput {
            guide: &guide,
            viewed: &viewed,
            show_cleared: true,
        }));
        assert!(
            expanded.contains("old.ts"),
            "expanded must list de-prioritized: {expanded}"
        );
    }

    #[test]
    fn viewed_badge_renders_when_hash_matches() {
        let units = vec![unit("src/page.ts", "orientation", Vec::new())];
        let guide = guide_with(
            units,
            Vec::new(),
            Vec::new(),
            RoutingFacts::default(),
            Vec::new(),
        );
        let mut viewed = ViewedState {
            graph_snapshot_hash: "hash1".to_string(),
            ..Default::default()
        };
        viewed.entries.insert(
            "src/page.ts".to_string(),
            crate::walkthrough_state::ViewedEntry {
                viewed_at: "t".to_string(),
            },
        );
        let lines = build_walkthrough_human_lines(&WalkthroughHumanInput {
            guide: &guide,
            viewed: &viewed,
            show_cleared: false,
        });
        assert!(plain(&lines).contains("viewed"), "got: {}", plain(&lines));
    }

    #[test]
    fn stale_viewed_hash_does_not_render_badge() {
        let units = vec![unit("src/page.ts", "orientation", Vec::new())];
        let guide = guide_with(
            units,
            Vec::new(),
            Vec::new(),
            RoutingFacts::default(),
            Vec::new(),
        );
        let mut viewed = ViewedState {
            graph_snapshot_hash: "STALE".to_string(),
            ..Default::default()
        };
        viewed.entries.insert(
            "src/page.ts".to_string(),
            crate::walkthrough_state::ViewedEntry {
                viewed_at: "t".to_string(),
            },
        );
        let lines = build_walkthrough_human_lines(&WalkthroughHumanInput {
            guide: &guide,
            viewed: &viewed,
            show_cleared: false,
        });
        // The page row has no "viewed" badge (stale state ignored).
        let body = plain(&lines);
        assert!(
            !body.contains("\u{2713} viewed"),
            "stale must not mark viewed: {body}"
        );
    }

    #[test]
    fn weakened_and_bus_factor_badges_render() {
        let mut page = unit("src/page.ts", "orientation", Vec::new());
        page.expert = vec!["alice".to_string()];
        let units = vec![page];
        let routing = RoutingFacts {
            units: vec![RoutingUnit {
                file: "src/page.ts".to_string(),
                expert: vec!["alice".to_string()],
                bus_factor_one: true,
            }],
        };
        let weakening = vec![WeakeningSignal {
            kind: WeakeningKind::TestWeakened,
            file: "src/page.ts".to_string(),
            evidence: "removed test".to_string(),
        }];
        let guide = guide_with(units, Vec::new(), Vec::new(), routing, weakening);
        let viewed = ViewedState::default();
        let text = plain(&build_walkthrough_human_lines(&WalkthroughHumanInput {
            guide: &guide,
            viewed: &viewed,
            show_cleared: false,
        }));
        assert!(text.contains("OWNER:alice"), "got: {text}");
        assert!(text.contains("BUS-FACTOR-1"), "got: {text}");
        assert!(text.contains("WEAKENED"), "got: {text}");
    }

    #[test]
    fn focus_header_and_status_never_fail_glyph() {
        let units = vec![unit("src/page.ts", "orientation", Vec::new())];
        let guide = guide_with(
            units,
            Vec::new(),
            Vec::new(),
            RoutingFacts::default(),
            Vec::new(),
        );
        let header = plain(&build_focus_header(&guide));
        assert!(header.contains("Review Focus"), "got: {header}");
        let status = crate::report::human::strip_ansi(&build_status_line(&guide));
        assert!(status.contains("Walkthrough ready"), "got: {status}");
        assert!(!status.contains('\u{2717}'), "no failure glyph");
    }

    #[test]
    fn truncate_caps_long_strings() {
        let long = "x".repeat(200);
        let cut = truncate(&long, 50);
        assert!(cut.chars().count() <= 50);
        assert!(cut.ends_with('\u{2026}'));
    }
}
