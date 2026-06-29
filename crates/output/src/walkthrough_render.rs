//! Shared, render-surface-agnostic helpers for the review walkthrough (W2).
//!
//! Both the human terminal renderer (`fallow-cli`) and the markdown renderer
//! (`fallow-api`) project the SAME [`StandardWalkthroughGuide`] into a staged
//! tour. The per-file "why" fact line, the staged/cleared membership split, and
//! the file-accounting math were independently re-derived in each surface and
//! drifted (double-counted files, a path printed twice, mid-word truncation,
//! escaped backticks). This module centralizes that shared logic as pure
//! functions so the two surfaces stay consistent by construction and the wire
//! contracts (`--walkthrough-guide` JSON, audit/brief) are never touched.

use crate::audit_walkthrough::{DirectionUnit, StandardWalkthroughGuide};

/// Max contract members named inline in a coordination fact before collapsing
/// the rest into a "+N more" suffix. Keeps the most load-bearing line readable
/// in a terminal without discarding the trailing guidance.
pub const MAX_CONTRACT_MEMBERS: usize = 6;

/// Honest file accounting for the walkthrough header + status, reconciled so the
/// numbers add up: `staged + cleared + excluded == changed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkthroughAccounting {
    /// Total changed files in the diff (the engine's `triage.files`, the real set).
    pub changed: usize,
    /// Source units shown in a stage (in `direction.order`, not collapsed).
    pub staged: usize,
    /// Source units collapsed into the Cleared panel (de-prioritized + viewed).
    pub cleared: usize,
    /// Non-source files in the diff that carry no contract to review (migrations,
    /// lockfiles, config, docs): counted, never silently dropped.
    pub excluded: usize,
}

impl WalkthroughAccounting {
    /// Reconcile the change into `staged + cleared + excluded`.
    ///
    /// `staged` is the count of direction units that REMAIN in a stage after the
    /// de-prioritized and viewed files collapse out. `cleared` is the
    /// de-prioritized escape hatch plus any staged-then-viewed files. `excluded`
    /// is the remainder of the real changed set that is not a reviewable source
    /// unit. The header renders `staged + cleared + excluded` (which equals
    /// `changed` whenever the engine's changed count is the real set), so the
    /// header, the status line, and reality agree, and no file is counted twice.
    #[must_use]
    pub fn compute(guide: &StandardWalkthroughGuide, viewed: &[String]) -> Self {
        // Each direction unit lands in exactly one rendered bucket: collapsed into
        // Cleared (de-prioritized OR viewed) or visible in a stage.
        let mut staged_visible = 0usize;
        let mut collapsed = 0usize;
        for file in &guide.direction.order {
            if is_deprioritized(guide, file) || is_collapsed_into_cleared(file, viewed) {
                collapsed += 1;
            } else {
                staged_visible += 1;
            }
        }
        // De-prioritized NON-source files (if any ever appear) plus de-prioritized
        // source files all live under Cleared; the loop above already counted the
        // source ones (they are in `direction.order`). Add de-prioritized files NOT
        // in the order so the escape hatch stays fully accounted.
        let deprioritized_off_spine = guide
            .digest
            .focus
            .deprioritized
            .iter()
            .filter(|u| !guide.direction.order.iter().any(|f| f == &u.file))
            .count();
        let cleared = collapsed + deprioritized_off_spine;
        // Source units the engine analyzed (review_here + deprioritized). The diff
        // may also touch non-source files (counted in `changed` but never in the
        // focus map); surface them as the excluded bucket instead of dropping them.
        let source_units = guide.digest.focus.total_units();
        let changed = guide.digest.triage.files;
        let excluded = changed.saturating_sub(source_units);
        WalkthroughAccounting {
            changed,
            staged: staged_visible,
            cleared,
            excluded,
        }
    }

    /// The honest "files in this change" total the header should display:
    /// `staged + cleared + excluded`. Equal to `changed` on a normal change; the
    /// `max` guards the rare case where the engine's count lags the parts.
    #[must_use]
    pub fn header_total(&self) -> usize {
        (self.staged + self.cleared + self.excluded).max(self.changed)
    }
}

/// The clean, surface-agnostic fact text for a coordination decision question.
///
/// The raw wire `question` leads with `` `<anchor_file>` `` (which every render
/// surface ALREADY shows as the row's leading path) and inlines the full,
/// unbounded contract-member list. This strips the redundant leading path and
/// caps the member list to `max_members` names + "+N more", PRESERVING the
/// trailing guidance sentence. The result is plain prose (no backticks), so a
/// markdown surface needs no escaping and a human surface needs no truncation.
#[must_use]
pub fn clean_decision_fact(question: &str, anchor_file: &str, max_members: usize) -> String {
    let stripped = strip_leading_path(question, anchor_file);
    cap_member_list(&stripped, max_members)
}

/// Drop a leading `` `<anchor_file>` `` token (with one trailing space) from the
/// question, so the path is not printed a second time after the row's path.
fn strip_leading_path(question: &str, anchor_file: &str) -> String {
    let prefix = format!("`{anchor_file}` ");
    question
        .strip_prefix(&prefix)
        .map_or_else(|| question.to_string(), str::to_string)
}

/// Cap the FIRST parenthesized comma-list (the contract members) to
/// `max_members` names, replacing the overflow with "+N more". Text outside that
/// first parenthetical (including the trailing question) is preserved verbatim.
fn cap_member_list(text: &str, max_members: usize) -> String {
    let Some(open) = text.find('(') else {
        return text.to_string();
    };
    let Some(rel_close) = text[open..].find(')') else {
        return text.to_string();
    };
    let close = open + rel_close;
    let inner = &text[open + 1..close];
    // Only collapse a genuine member list (comma-separated identifiers), never a
    // prose parenthetical like "(env)" or "(the cache)".
    let members: Vec<&str> = inner.split(", ").collect();
    if members.len() <= max_members {
        return text.to_string();
    }
    let shown = members[..max_members].join(", ");
    let more = members.len() - max_members;
    format!(
        "{}({shown}, +{more} more){}",
        &text[..open],
        &text[close + 1..]
    )
}

/// Cap an arbitrary list of names for inline display: first `max` names, then a
/// "+N more" sentinel. Shared by the out-of-diff consumer fact in both surfaces.
#[must_use]
pub fn cap_names(names: &[String], max: usize) -> (Vec<&str>, usize) {
    let shown: Vec<&str> = names.iter().take(max).map(String::as_str).collect();
    let more = names.len().saturating_sub(shown.len());
    (shown, more)
}

/// Whether a staged file collapses into Cleared instead of showing in its stage.
/// A file is cleared when the local viewed-state marked it seen (the
/// `--mark-viewed` collapse): it must appear ONLY under Cleared, never in both.
#[must_use]
fn is_collapsed_into_cleared(file: &str, viewed: &[String]) -> bool {
    viewed.iter().any(|v| v == file)
}

/// Whether `file` is a de-prioritized focus unit. The `--help` contract is that
/// de-prioritized files collapse INTO the Cleared panel, so they must be removed
/// from their stage row and shown only under Cleared.
#[must_use]
fn is_deprioritized(guide: &StandardWalkthroughGuide, file: &str) -> bool {
    guide
        .digest
        .focus
        .deprioritized
        .iter()
        .any(|u| u.file == file)
}

/// True when a direction unit collapses out of its stage and into Cleared,
/// because it is de-prioritized OR locally viewed. Each file is then in exactly
/// one rendered place (stage XOR cleared).
#[must_use]
fn collapses_into_cleared(guide: &StandardWalkthroughGuide, file: &str, viewed: &[String]) -> bool {
    is_deprioritized(guide, file) || is_collapsed_into_cleared(file, viewed)
}

/// The visible stage members for a guide: direction units in order, MINUS any
/// file collapsed into Cleared (de-prioritized or viewed). Returned as references
/// into the guide so the caller can render rows without cloning.
#[must_use]
pub fn visible_stage_units<'a>(
    guide: &'a StandardWalkthroughGuide,
    viewed: &[String],
) -> Vec<&'a DirectionUnit> {
    guide
        .direction
        .order
        .iter()
        .filter(|file| !collapses_into_cleared(guide, file, viewed))
        .filter_map(|file| guide.direction.units.iter().find(|u| &u.file == file))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit_brief::{
        DiffTriage, GraphFacts, ImpactClosureFacts, PartitionFacts, ReviewBriefSchemaVersion,
        ReviewDeltas, ReviewEffort, RiskClass, StandardReviewBriefOutput,
    };
    use crate::audit_decision_surface::DecisionSurface;
    use crate::audit_focus::{FocusLabel, FocusMap, FocusScore, FocusUnit};
    use crate::audit_routing::RoutingFacts;
    use crate::audit_walkthrough::{
        AgentSchema, DirectionUnit, INJECTION_NOTE, ReviewDirection, StandardWalkthroughGuide,
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

    fn dir_unit(file: &str) -> DirectionUnit {
        DirectionUnit {
            file: file.to_string(),
            concern_lens: "orientation".to_string(),
            scoring_budget: 1,
            out_of_diff: Vec::new(),
            expert: Vec::new(),
        }
    }

    /// A guide whose direction is the review_here + deprioritized source units (as
    /// the engine builds it), with `changed` total files that may exceed the source
    /// unit count (the non-source excluded bucket).
    fn guide_for(
        review_here: &[&str],
        deprioritized: &[&str],
        changed_total: usize,
    ) -> StandardWalkthroughGuide {
        let order: Vec<String> = review_here
            .iter()
            .chain(deprioritized.iter())
            .map(|s| (*s).to_string())
            .collect();
        let units: Vec<DirectionUnit> = order.iter().map(|f| dir_unit(f)).collect();
        let digest = StandardReviewBriefOutput {
            schema_version: ReviewBriefSchemaVersion::default(),
            version: "test".to_string(),
            command: "audit-brief".to_string(),
            triage: DiffTriage {
                files: changed_total,
                hunks: None,
                net_lines: None,
                risk_class: RiskClass::Medium,
                review_effort: ReviewEffort::Review,
            },
            graph_facts: GraphFacts {
                exports_added: 0,
                api_width_delta: 0,
                reachable_from: Vec::new(),
                boundaries_touched: Vec::new(),
            },
            partition: PartitionFacts::default(),
            impact_closure: ImpactClosureFacts::default(),
            focus: FocusMap {
                review_here: review_here
                    .iter()
                    .map(|f| focus_unit(f, FocusLabel::ReviewHere))
                    .collect(),
                deprioritized: deprioritized
                    .iter()
                    .map(|f| focus_unit(f, FocusLabel::NotPrioritized))
                    .collect(),
            },
            deltas: ReviewDeltas::default(),
            weakening: Vec::new(),
            routing: RoutingFacts::default(),
            decisions: DecisionSurface::default(),
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

    #[test]
    fn accounting_reconciles_staged_cleared_excluded() {
        // 16 changed files: 2 review-here source, 1 de-prioritized source, 13
        // non-source (migrations/config/docs). No viewed.
        let guide = guide_for(&["src/a.ts", "src/b.ts"], &["src/c.ts"], 16);
        let acc = WalkthroughAccounting::compute(&guide, &[]);
        assert_eq!(acc.changed, 16);
        assert_eq!(acc.staged, 2, "review-here source units stay in stages");
        assert_eq!(acc.cleared, 1, "de-prioritized collapses into cleared");
        assert_eq!(
            acc.excluded, 13,
            "non-source files are excluded, not dropped"
        );
        // The header total accounts for the whole changed set.
        assert_eq!(acc.header_total(), 16);
        assert_eq!(acc.staged + acc.cleared + acc.excluded, acc.changed);
    }

    #[test]
    fn viewed_file_moves_from_staged_to_cleared() {
        let guide = guide_for(&["src/a.ts", "src/b.ts"], &[], 2);
        let viewed = vec!["src/a.ts".to_string()];
        let acc = WalkthroughAccounting::compute(&guide, &viewed);
        assert_eq!(acc.staged, 1, "the viewed file left the stage");
        assert_eq!(acc.cleared, 1, "the viewed file is counted in cleared");
        assert_eq!(acc.excluded, 0);
        assert_eq!(acc.staged + acc.cleared + acc.excluded, acc.changed);
    }

    #[test]
    fn deprioritized_and_viewed_appear_in_exactly_one_place() {
        let guide = guide_for(&["src/a.ts", "src/b.ts"], &["src/c.ts"], 3);
        let viewed = vec!["src/a.ts".to_string()];
        // a.ts is viewed -> cleared; c.ts is de-prioritized -> cleared; only b.ts
        // remains visible in a stage.
        let visible = visible_stage_units(&guide, &viewed);
        let files: Vec<&str> = visible.iter().map(|u| u.file.as_str()).collect();
        assert_eq!(files, vec!["src/b.ts"]);
        assert!(collapses_into_cleared(&guide, "src/a.ts", &viewed));
        assert!(collapses_into_cleared(&guide, "src/c.ts", &viewed));
        assert!(!collapses_into_cleared(&guide, "src/b.ts", &viewed));
    }

    #[test]
    fn strips_leading_path_and_caps_members() {
        let q = "`src/db/schema.ts` changes a contract (a, b, c, d, e, f, g, h) consumed by 32 modules NOT in this diff. Coordinate the change, or is the contract stable?";
        let out = clean_decision_fact(q, "src/db/schema.ts", 3);
        // The leading path is gone (printed once by the row).
        assert!(
            !out.starts_with("`src/db/schema.ts`"),
            "leading path must be stripped: {out}"
        );
        // The member list is capped with a "+N more".
        assert!(out.contains("(a, b, c, +5 more)"), "got: {out}");
        // The trailing guidance survives in full.
        assert!(
            out.ends_with("Coordinate the change, or is the contract stable?"),
            "trailing question must survive: {out}"
        );
        // No backticks remain to be escaped.
        assert!(!out.contains('`'), "no backticks remain: {out}");
    }

    #[test]
    fn short_member_list_is_untouched() {
        let q = "`src/lib/r2.ts` changes a contract (getR2, getR2Text) consumed by 6 modules NOT in this diff. Coordinate the change, or is the contract stable?";
        let out = clean_decision_fact(q, "src/lib/r2.ts", 6);
        assert_eq!(
            out,
            "changes a contract (getR2, getR2Text) consumed by 6 modules NOT in this diff. Coordinate the change, or is the contract stable?"
        );
    }

    #[test]
    fn single_member_prose_parenthetical_is_untouched() {
        let q = "`src/lib/env.ts` changes a contract (env) consumed by 22 modules NOT in this diff. Coordinate the change, or is the contract stable?";
        let out = clean_decision_fact(q, "src/lib/env.ts", 6);
        assert!(out.contains("(env)"), "single member kept: {out}");
        assert!(out.ends_with("stable?"), "trailing kept: {out}");
    }

    #[test]
    fn non_anchor_question_keeps_its_own_path() {
        // A boundary question names a DIFFERENT path than the anchor; it must not
        // be stripped (the leading token is not the anchor file).
        let q = "`ui` now imports `db` for the first time. Intended coupling, or should this edge not exist?";
        let out = clean_decision_fact(q, "src/ui/page.ts", 6);
        assert_eq!(out, q, "non-matching leading token is preserved");
    }

    #[test]
    fn cap_names_first_k_then_more() {
        let names = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ];
        let (shown, more) = cap_names(&names, 2);
        assert_eq!(shown, vec!["a", "b"]);
        assert_eq!(more, 2);
    }
}
