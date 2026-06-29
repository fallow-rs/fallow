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
    DecisionCategory, DirectionUnit, MAX_CONTRACT_MEMBERS, ReviewEffort, RiskClass,
    StandardWalkthroughGuide, WalkthroughAccounting, cap_names, clean_decision_fact,
    visible_stage_units,
};

use crate::walkthrough_state::ViewedState;

use super::{MAX_FLAT_ITEMS, format_path};
use crate::report::plural;

/// Max out-of-diff consumers named on a per-file fact line before truncating.
const MAX_NAMED_CONSUMERS: usize = 3;

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

    let viewed = viewed_files(input);
    let (stage1, stage2) = partition_stages(guide, &viewed);
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

/// The staged source files the local state has marked viewed (current hash only),
/// so the shared membership helpers can collapse them into Cleared.
fn viewed_files(input: &WalkthroughHumanInput<'_>) -> Vec<String> {
    viewed_files_for(input.guide, input.viewed)
}

/// Same, but from a raw guide + viewed-state pair (used by the header/status
/// accounting, which do not build a full `WalkthroughHumanInput`).
fn viewed_files_for(guide: &StandardWalkthroughGuide, viewed: &ViewedState) -> Vec<String> {
    guide
        .direction
        .order
        .iter()
        .filter(|file| viewed.is_viewed(file, &guide.graph_snapshot_hash))
        .cloned()
        .collect()
}

/// Partition the guide's VISIBLE stage units (de-prioritized + viewed files
/// already collapsed out into Cleared) into (contract-break, orientation), each
/// in `direction.order`. Each file is in exactly one place: a stage XOR Cleared.
fn partition_stages<'a>(
    guide: &'a StandardWalkthroughGuide,
    viewed: &[String],
) -> (Vec<&'a DirectionUnit>, Vec<&'a DirectionUnit>) {
    let mut load_bearing = Vec::new();
    let mut mechanical = Vec::new();
    for unit in visible_stage_units(guide, viewed) {
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

/// Render one file's row: the header line (path + badges) and the one-line fact
/// beneath it. The raw "(score N)" is intentionally NOT shown: it is an internal
/// composite that does not explain the visible within-stage order (Stage 1 is
/// ordered by out-of-diff consumer count, Stage 2 by that same composite), so
/// surfacing the number contradicted the order. The fact line carries the real
/// ordering signal (consumer count / fan-in) instead.
fn push_file_row(lines: &mut Vec<String>, unit: &DirectionUnit, input: &WalkthroughHumanInput<'_>) {
    let badges = synthesize_badges(unit, input);
    let badge_suffix = if badges.is_empty() {
        String::new()
    } else {
        format!(" {}", badges.join(" "))
    };
    lines.push(format!("  {}{badge_suffix}", format_path(&unit.file)));
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
        // Strip the redundant leading path (the row already shows it) and cap the
        // contract-member list, PRESERVING the trailing guidance question instead
        // of truncating it away.
        return clean_decision_fact(&decision.question, &unit.file, MAX_CONTRACT_MEMBERS);
    }
    if !unit.out_of_diff.is_empty() {
        return out_of_diff_fact(unit);
    }
    if let Some(reason) = focus_reason(unit, guide) {
        return reason.to_string();
    }
    "orientation only".to_string()
}

/// The out-of-diff consumer fact: a count plus the first few consumer paths.
fn out_of_diff_fact(unit: &DirectionUnit) -> String {
    let total = unit.out_of_diff.len();
    let (named, more_count) = cap_names(&unit.out_of_diff, MAX_NAMED_CONSUMERS);
    let more = if more_count > 0 {
        format!(" (+{more_count} more)")
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
    // Count viewed files that are NOT already in the de-prioritized bucket, so a
    // de-prioritized-and-viewed file lands in exactly one bucket (no double count).
    let viewed_count = viewed_only_files(input).len();

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
    for file in viewed_only_files(input) {
        lines.push(format!(
            "    {} {}",
            format_path(&file),
            "\u{2713} viewed".dimmed()
        ));
    }
}

/// Files marked viewed (current hash) that are NOT already in the de-prioritized
/// bucket, so the viewed sub-list of Cleared never re-lists a de-prioritized file.
fn viewed_only_files(input: &WalkthroughHumanInput<'_>) -> Vec<String> {
    let guide = input.guide;
    viewed_files(input)
        .into_iter()
        .filter(|file| {
            !guide
                .digest
                .focus
                .deprioritized
                .iter()
                .any(|u| &u.file == file)
        })
        .collect()
}

/// The Review Focus orientation header lines (rendered to stderr by the entry
/// point). Built from the guide's triage + graph facts. Never a verdict. The
/// file count is the reconciled changed set (`staged + cleared + excluded`), so
/// the header, the status line, and reality agree.
#[must_use]
pub(in crate::report) fn build_focus_header(
    guide: &StandardWalkthroughGuide,
    viewed: &ViewedState,
) -> Vec<String> {
    let triage = &guide.digest.triage;
    let viewed_in_order = viewed_files_for(guide, viewed);
    let acc = WalkthroughAccounting::compute(guide, &viewed_in_order);
    let total = acc.header_total();
    let mut lines = Vec::new();
    lines.push(format!(
        "{} {}",
        "\u{25cf}".cyan(),
        format!(
            "Review Focus \u{2014} {} risk \u{00b7} {} \u{00b7} {} file{}",
            risk_label(triage.risk_class),
            effort_label(triage.review_effort),
            total,
            plural(total),
        )
        .cyan()
        .bold()
    ));
    if let Some(breakdown) = accounting_breakdown(&acc) {
        lines.push(format!("  {}", breakdown.dimmed()));
    }
    if let Some(sub) = focus_subline(guide) {
        lines.push(format!("  {}", sub.dimmed()));
    }
    lines
}

/// The honest accounting sub-line: how the changed set splits into staged /
/// cleared / excluded buckets, so non-source files are surfaced, not dropped.
/// Returns `None` when there is nothing beyond the staged files to explain.
fn accounting_breakdown(acc: &WalkthroughAccounting) -> Option<String> {
    let mut parts = Vec::new();
    parts.push(format!("{} in stages", acc.staged));
    if acc.cleared > 0 {
        parts.push(format!("{} cleared", acc.cleared));
    }
    if acc.excluded > 0 {
        parts.push(format!("{} non-source not reviewed", acc.excluded));
    }
    if acc.cleared == 0 && acc.excluded == 0 {
        return None;
    }
    Some(parts.join(" \u{00b7} "))
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
/// failure glyph; the walkthrough always exits 0. The count is the files visible
/// in stages (de-prioritized + viewed already collapsed into Cleared), so it
/// reconciles with the header's `staged` bucket.
#[must_use]
pub(in crate::report) fn build_status_line(
    guide: &StandardWalkthroughGuide,
    viewed: &ViewedState,
) -> String {
    let acc = WalkthroughAccounting::compute(guide, &viewed_files_for(guide, viewed));
    let files = acc.staged;
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
        let order: Vec<String> = units.iter().map(|u| u.file.clone()).collect();
        // Mirror reality: the focus map's review_here is the direction units that
        // are NOT de-prioritized, so review_here + deprioritized == the source set
        // and triage.files (no extra non-source churn) keeps `excluded` at 0 for
        // the synthetic guides that don't deliberately add non-source files.
        let review_here: Vec<FocusUnit> = order
            .iter()
            .filter(|f| !deprioritized.iter().any(|d| &d.file == *f))
            .map(|f| focus_unit(f, FocusLabel::ReviewHere))
            .collect();
        let total_files = review_here.len() + deprioritized.len();
        let digest = StandardReviewBriefOutput {
            schema_version: ReviewBriefSchemaVersion::default(),
            version: "test".to_string(),
            command: "audit-brief".to_string(),
            triage: DiffTriage {
                files: total_files,
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
                review_here,
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
        let viewed = ViewedState::default();
        let header = plain(&build_focus_header(&guide, &viewed));
        assert!(header.contains("Review Focus"), "got: {header}");
        let status = crate::report::human::strip_ansi(&build_status_line(&guide, &viewed));
        assert!(status.contains("Walkthrough ready"), "got: {status}");
        assert!(!status.contains('\u{2717}'), "no failure glyph");
    }

    // F1/F2: the header count matches the staged set and surfaces the cleared +
    // excluded buckets; the status count agrees with the header's staged bucket.
    #[test]
    fn header_and_status_counts_reconcile() {
        // Two review-here source units in stages, one de-prioritized, and the diff
        // also touched 3 non-source files (migrations/config) not in the focus map.
        let units = vec![
            unit("src/a.ts", "orientation", Vec::new()),
            unit("src/b.ts", "orientation", Vec::new()),
        ];
        let deprioritized = vec![focus_unit("src/c.ts", FocusLabel::NotPrioritized)];
        let mut guide = guide_with(
            units,
            Vec::new(),
            deprioritized,
            RoutingFacts::default(),
            Vec::new(),
        );
        // a.ts + b.ts (review-here) + c.ts (deprioritized) = 3 source; +3 non-source.
        guide.digest.triage.files = 6;
        let viewed = ViewedState::default();
        let header = plain(&build_focus_header(&guide, &viewed));
        // The header total is the whole changed set, not the staged subset.
        assert!(header.contains("6 files"), "header total: {header}");
        // The breakdown surfaces the cleared + excluded buckets honestly.
        assert!(header.contains("2 in stages"), "staged: {header}");
        assert!(header.contains("1 cleared"), "cleared: {header}");
        assert!(
            header.contains("3 non-source not reviewed"),
            "excluded: {header}"
        );
        // The status line agrees with the staged bucket (not the total).
        let status = crate::report::human::strip_ansi(&build_status_line(&guide, &viewed));
        assert!(
            status.contains("2 files across 2 stages"),
            "status: {status}"
        );
    }

    // F3: a de-prioritized file appears ONLY under Cleared, never in a stage.
    #[test]
    fn deprioritized_file_is_not_double_listed() {
        // One review-here source unit plus one DE-PRIORITIZED source unit; the
        // engine puts both in direction.order, but the render must collapse the
        // de-prioritized one into Cleared only.
        let units = vec![
            unit("src/keep.ts", "orientation", Vec::new()),
            unit("src/drop.ts", "orientation", Vec::new()),
        ];
        let deprioritized = vec![focus_unit("src/drop.ts", FocusLabel::NotPrioritized)];
        let guide = guide_with(
            units,
            Vec::new(),
            deprioritized,
            RoutingFacts::default(),
            Vec::new(),
        );
        let viewed = ViewedState::default();
        let body = plain(&build_walkthrough_human_lines(&WalkthroughHumanInput {
            guide: &guide,
            viewed: &viewed,
            show_cleared: true,
        }));
        assert!(
            body.contains("keep.ts"),
            "review-here file stays staged: {body}"
        );
        // The de-prioritized file appears only in the Cleared section, never in the
        // stage section above it (each file is in exactly one place).
        let cleared_at = body.find("Cleared").expect("cleared panel present");
        let (stage_section, cleared_section) = body.split_at(cleared_at);
        assert!(
            !stage_section.contains("drop.ts"),
            "de-prioritized file must not be in a stage: {body}"
        );
        assert!(
            cleared_section.contains("drop.ts"),
            "de-prioritized file must be under Cleared: {body}"
        );
    }

    // F3: a --mark-viewed file is removed from its stage and counted ONLY in
    // Cleared (no double listing).
    #[test]
    fn viewed_file_collapses_into_cleared_only() {
        let units = vec![
            unit("src/seen.ts", "orientation", Vec::new()),
            unit("src/fresh.ts", "orientation", Vec::new()),
        ];
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
            "src/seen.ts".to_string(),
            crate::walkthrough_state::ViewedEntry {
                viewed_at: "t".to_string(),
            },
        );
        let body = plain(&build_walkthrough_human_lines(&WalkthroughHumanInput {
            guide: &guide,
            viewed: &viewed,
            show_cleared: true,
        }));
        // seen.ts appears only under Cleared (as viewed), never in a stage row.
        let cleared_at = body.find("Cleared").expect("cleared panel present");
        let (stage_section, cleared_section) = body.split_at(cleared_at);
        assert!(
            !stage_section.contains("seen.ts"),
            "viewed file must not be in a stage: {body}"
        );
        assert!(
            cleared_section.contains("seen.ts"),
            "viewed file must be under Cleared: {body}"
        );
        assert!(
            body.contains("fresh.ts"),
            "the un-viewed file stays staged: {body}"
        );
        // Header staged count drops the viewed file; status agrees.
        let status = crate::report::human::strip_ansi(&build_status_line(&guide, &viewed));
        assert!(
            status.contains("1 file across 2 stages"),
            "status: {status}"
        );
    }

    // F4/F5/F7: the contract member list is capped and the trailing guidance
    // question survives (not truncated away); no raw score is shown.
    #[test]
    fn contract_question_keeps_trailing_guidance_and_caps_members() {
        let members =
            "alertRules, apiKeys, auditLog, budgetAlerts, coverageAlerts, deployments, users, orgs";
        let question = format!(
            "`src/db/schema.ts` changes a contract ({members}) consumed by 32 modules NOT in this diff. Coordinate the change, or is the contract stable?"
        );
        let mut decision = coupling_decision("src/db/schema.ts");
        decision.question = question;
        let units = vec![unit(
            "src/db/schema.ts",
            "contract-break",
            vec!["src/x.ts".to_string()],
        )];
        let guide = guide_with(
            units,
            vec![decision],
            Vec::new(),
            RoutingFacts::default(),
            Vec::new(),
        );
        let viewed = ViewedState::default();
        let body = plain(&build_walkthrough_human_lines(&WalkthroughHumanInput {
            guide: &guide,
            viewed: &viewed,
            show_cleared: false,
        }));
        // The trailing actionable question is NOT truncated away.
        assert!(
            body.contains("Coordinate the change, or is the contract stable?"),
            "trailing guidance must survive: {body}"
        );
        // The member list is capped with a "+N more".
        assert!(body.contains("+2 more"), "member list capped: {body}");
        // The leading path is not re-printed inside the fact text.
        assert!(
            !body.contains("`src/db/schema.ts` changes a contract"),
            "fact must not re-print the path: {body}"
        );
        // The contradictory raw "(score N)" is gone.
        assert!(!body.contains("(score "), "raw score removed: {body}");
    }
}
