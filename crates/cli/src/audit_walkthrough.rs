//! Agent-contract loop (the codiff pattern, graph-extended).
//!
//! Closes the steer-the-agent loop. The tool owns the digest + prompt + schema;
//! the agent owns judgment; fallow post-validates the agent's judgment against
//! the LIVE graph + diff. The reentry is the `--walkthrough-file` path, exactly
//! as codiff re-resolves hunk ids against the live diff at validation time, but
//! fallow's verifier is the deterministic module graph, not a second model.
//!
//! ## LEAD PRINCIPLE: the verifier is the graph, not a second model
//!
//! Trust comes from deterministic, reproducible, graph-adjudicated post-validation
//! that cannot hallucinate. Two mechanisms enforce it:
//!
//! 1. **Anti-hallucination** (anchoring): every agent judgment MUST cite a
//!    `signal_id` fallow emitted.
//!    [`DecisionSurface::accept_signal_id`](crate::audit_decision_surface::DecisionSurface::accept_signal_id)
//!    is the allowlist; a judgment whose id was never emitted is REJECTED. The
//!    agent proposes; the graph disposes.
//! 2. **Staleness refusal** (snapshot pin): the digest (the `WalkthroughGuide`)
//!    carries a deterministic `graph_snapshot_hash`, a stable hash of the
//!    relevant graph + diff state. The agent echoes it back in its JSON; if the
//!    tree moved between the guide emission and the reentry, the current hash
//!    differs and the WHOLE payload is REFUSED as stale.
//!
//! ## Injection-resistance by construction
//!
//! The digest is built ONLY from the deterministic graph
//! ([`crate::audit_brief::build_brief_output`], pure over the tree). PR prose
//! NEVER enters the digest. On reentry, the agent's free-text framing is FENCED
//! (marked non-deterministic) onto the validation output; it never gates, never
//! auto-posts, and never folds back into the digest. Treat any PR prose fed to an
//! agent as untrusted: this loop is injection-resistant because the trusted
//! surface is the graph, and the untrusted surface is fenced.

pub use fallow_output::{
    AcceptedJudgment, AgentJudgment, AgentWalkthrough, ChangeAnchor, DirectionUnit, INJECTION_NOTE,
    RejectedJudgment, ReviewDirection, WalkthroughValidation, agent_schema,
};
use fallow_output::{FocusMap, RoutingFacts};
use rustc_hash::{FxHashMap, FxHashSet};
use xxhash_rust::xxh3::xxh3_64;

use crate::audit_brief::{ReviewBriefOutput, ReviewBriefSchemaVersion, build_brief_output};
use crate::audit_decision_surface::DecisionSurface;
use crate::report::ci::diff_filter::parse_new_hunk_start;

/// The standing reason a judgment is rejected for citing a `signal_id` fallow
/// never emitted (the anti-hallucination gate).
const UNANCHORED_REASON: &str = "unanchored-signal-id";

/// The reason a judgment is rejected for citing a `change_anchor` (a `chg:` id)
/// that fallow did not emit for this changed set (the anti-hallucination gate
/// for the weaker, region-level anchor).
const UNKNOWN_CHANGE_ANCHOR_REASON: &str = "unknown-change-anchor";

pub type WalkthroughGuide = fallow_output::StandardWalkthroughGuide;

/// Strip per-line leading/trailing whitespace and join added lines with `\n`, so
/// a reflow or a whitespace-only edit does not move the content-addressed id.
fn normalize_added_text(lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Derive a stable, CONTENT-addressed change-anchor id. Hashes ONLY the file
/// path + the normalized added text + an occurrence ordinal (to disambiguate
/// byte-identical hunks in one file). Line numbers are deliberately excluded so
/// the id survives edits above the hunk and whitespace-only changes. Mirrors
/// [`crate::audit_decision_surface::derive_signal_id`] with a `chg:` namespace.
#[must_use]
pub fn derive_change_anchor_id(path: &str, normalized_added_text: &str, ordinal: u32) -> String {
    let mut bytes =
        Vec::with_capacity(path.len() + 1 + normalized_added_text.len() + 1 + size_of::<u32>());
    bytes.extend_from_slice(path.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(normalized_added_text.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&ordinal.to_le_bytes());
    format!("chg:{:016x}", xxh3_64(&bytes))
}

/// Mutable accumulator threaded through [`parse_change_anchors`] while walking a
/// unified diff. Holds the current file, rename provenance, and the in-progress
/// hunk; [`AnchorParser::flush`] turns the accumulated hunk into one anchor.
#[derive(Default)]
struct AnchorParser {
    anchors: Vec<ChangeAnchor>,
    /// `(file, normalized text) -> next ordinal` for byte-identical hunks.
    seen: FxHashMap<(String, String), u32>,
    current_file: Option<String>,
    rename_from: Option<String>,
    pending_rename_from: Option<String>,
    start_line: u64,
    hunk_lines: Vec<String>,
    in_hunk: bool,
}

impl AnchorParser {
    /// Flush the accumulated hunk into one anchor (computing its occurrence
    /// ordinal for byte-identical normalized text within the same file), then
    /// clear the hunk buffer. No-op when there is no current file or no added
    /// lines.
    fn flush(&mut self) {
        if let Some(file) = self.current_file.clone()
            && !self.hunk_lines.is_empty()
        {
            let normalized = normalize_added_text(&self.hunk_lines);
            let counter = self
                .seen
                .entry((file.clone(), normalized.clone()))
                .or_insert(0);
            let ordinal = *counter;
            *counter += 1;
            let change_anchor = derive_change_anchor_id(&file, &normalized, ordinal);
            let previous_change_anchor = self
                .rename_from
                .as_deref()
                .map(|old| derive_change_anchor_id(old, &normalized, ordinal));
            self.anchors.push(ChangeAnchor {
                change_anchor,
                file,
                start_line: u32::try_from(self.start_line).unwrap_or(u32::MAX),
                line_count: u32::try_from(self.hunk_lines.len()).unwrap_or(u32::MAX),
                previous_change_anchor,
            });
        }
        self.hunk_lines.clear();
    }

    /// Consume one diff line, flushing any pending hunk on a structural boundary
    /// (`diff --git`, `+++ b/`, `+++ /dev/null`, `@@`) and accumulating `+` lines
    /// inside a hunk.
    fn consume(&mut self, line: &str) {
        if line.starts_with("diff --git ") {
            self.flush();
            self.in_hunk = false;
            self.current_file = None;
            self.rename_from = None;
            self.pending_rename_from = None;
            return;
        }
        if let Some(rest) = line.strip_prefix("rename from ") {
            self.pending_rename_from = Some(rest.to_owned());
            return;
        }
        if let Some(rest) = line.strip_prefix("rename to ") {
            if let Some(from) = self.pending_rename_from.take() {
                self.current_file = Some(rest.to_owned());
                self.rename_from = Some(from);
            }
            return;
        }
        if let Some(path) = line.strip_prefix("+++ b/") {
            self.flush();
            self.in_hunk = false;
            self.current_file = Some(path.to_owned());
            return;
        }
        if line.starts_with("+++ /dev/null") {
            self.flush();
            self.in_hunk = false;
            self.current_file = None;
            return;
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            self.flush();
            self.start_line = parse_new_hunk_start(header).unwrap_or(0);
            self.in_hunk = true;
            return;
        }
        if self.in_hunk
            && self.current_file.is_some()
            && line.starts_with('+')
            && !line.starts_with("+++")
        {
            self.hunk_lines.push(line[1..].to_owned());
        }
    }
}

/// Parse a zero-context unified diff (`git diff --unified=0`) into per-hunk
/// [`ChangeAnchor`]s. Each hunk's added (`+`) lines form one anchor. Rename
/// headers make the anchor rename-durable via `previous_change_anchor`. Pure:
/// the same diff text always yields the same anchors.
#[must_use]
pub fn parse_change_anchors(diff: &str) -> Vec<ChangeAnchor> {
    let mut parser = AnchorParser::default();
    for line in diff.lines() {
        parser.consume(line);
    }
    parser.flush();
    parser.anchors
}

/// Build the change-anchor allowlist from the emitted anchors: every current id
/// plus every `previous_change_anchor` (so a judgment that cited an anchor under
/// a pre-rename path still resolves).
#[must_use]
pub fn change_anchor_allowlist(anchors: &[ChangeAnchor]) -> FxHashSet<String> {
    let mut set = FxHashSet::default();
    for anchor in anchors {
        set.insert(anchor.change_anchor.clone());
        if let Some(previous) = &anchor.previous_change_anchor {
            set.insert(previous.clone());
        }
    }
    set
}

/// True when a routing unit names an analyzable source file worth steering a
/// reviewer through. Non-code churn (LICENSE, .gitignore, README.md, JSON/YAML
/// config, lockfiles) is excluded from the direction: it carries no contract to
/// break and only dilutes the order the agent executes.
fn is_reviewable_source_unit(file: &str) -> bool {
    matches!(
        std::path::Path::new(file)
            .extension()
            .and_then(|e| e.to_str()),
        Some(
            "ts" | "tsx"
                | "js"
                | "jsx"
                | "mjs"
                | "cjs"
                | "mts"
                | "cts"
                | "gts"
                | "gjs"
                | "vue"
                | "svelte"
                | "astro"
        )
    )
}

/// Read the displayed fan-in (importer) and fan-out counts back out of a focus
/// unit's reason string. The reason is the canonical rendered "why" both surfaces
/// print verbatim for a Stage-2 row ("high fan-in (N importers), fan-out M, ..."),
/// built in `audit_focus::build_reason` straight from the raw
/// `facts.fan_in` / `facts.fan_out`. Sorting on these parsed counts makes the
/// within-stage order EQUAL the number on the row, instead of the hidden, capped
/// `fan_io` blend the composite feeds on (which undersells a high-fan-in file
/// past the cap). Returns `(0, 0)` when neither phrase is present (the reason has
/// no fan signal), so such a unit sorts last and falls to the path tiebreak.
fn parse_fan_counts(reason: &str) -> (u32, u32) {
    let fan_in = extract_count(reason, "high fan-in (");
    let fan_out = extract_count(reason, "fan-out ");
    (fan_in, fan_out)
}

/// Parse the leading run of decimal digits immediately following `marker` in
/// `text`. Returns `0` when the marker is absent or is not followed by a digit.
fn extract_count(text: &str, marker: &str) -> u32 {
    let Some(rest) = text.find(marker).map(|i| &text[i + marker.len()..]) else {
        return 0;
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

/// Build the review direction. The SPINE is the change itself: every reviewable
/// focus unit (`review_here` + the `deprioritized` escape hatch), so the
/// direction is never empty when there is code to review. Ownership routing is a
/// LEFT-JOINED overlay for the optional `expert` field, NOT the spine: sourcing
/// the work-list from routing made it empty on solo / author's-own-PR changes (no
/// one else to ask), which is exactly the cloud's dominant case. Each unit carries
/// its `scoring_budget` (the focus composite score) so a fan-out spends AI
/// proportional to risk, its per-file `out_of_diff` consumers, and the
/// `concern_lens`. Non-source churn is excluded. Units with out-of-diff consumers
/// sort first (load-bearing definitions before mechanical churn), then by budget.
#[allow(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashMap; fires on the lib target only, so #[expect] is unfulfilled on the bin"
)]
#[must_use]
pub fn build_direction(
    focus: &FocusMap,
    out_of_diff_by_file: &FxHashMap<String, Vec<String>>,
    routing: &RoutingFacts,
) -> ReviewDirection {
    // Optional expert overlay: file -> routed expert(s). Empty on the author's own
    // PR, which is why it is an overlay and not the spine.
    let expert_by_file: FxHashMap<&str, &[String]> = routing
        .units
        .iter()
        .map(|unit| (unit.file.as_str(), unit.expert.as_slice()))
        .collect();

    let mut units: Vec<DirectionUnit> = focus
        .review_here
        .iter()
        .chain(focus.deprioritized.iter())
        .filter(|unit| is_reviewable_source_unit(&unit.file))
        .map(|unit| {
            // Per-unit out-of-diff: the consumers of THIS file outside the diff. A
            // unit that breaks a contract gets the contract-break lens; the rest
            // the plain orientation lens. Graph-derived.
            let out_of_diff = out_of_diff_by_file
                .get(&unit.file)
                .cloned()
                .unwrap_or_default();
            let concern_lens = if out_of_diff.is_empty() {
                "orientation".to_string()
            } else {
                "contract-break".to_string()
            };
            DirectionUnit {
                file: unit.file.clone(),
                concern_lens,
                scoring_budget: unit.score.total,
                out_of_diff,
                expert: expert_by_file
                    .get(unit.file.as_str())
                    .map(|experts| experts.to_vec())
                    .unwrap_or_default(),
            }
        })
        .collect();

    // Review the load-bearing units first: contract-breakers (out-of-diff
    // consumers) ahead of the rest, ordered by their consumer count (the number
    // each Stage-1 row shows as "consumed by N modules"). Within the
    // non-contract-break tier (Stage 2, out-of-diff empty) the order follows the
    // exact importer count each row displays as "high fan-in (N importers)" --
    // descending fan-in, then descending fan-out, then path. We read those counts
    // back out of the focus reason string (the canonical rendered text both
    // surfaces print), so the top-to-bottom order is always the visible number,
    // not the hidden, capped `fan_io` blend (a 60-importer file outranks a
    // 5-importer file even though both cap to the same `fan_io`).
    let fan_counts_by_file: FxHashMap<&str, (u32, u32)> = focus
        .review_here
        .iter()
        .chain(focus.deprioritized.iter())
        .map(|fu| (fu.file.as_str(), parse_fan_counts(&fu.reason)))
        .collect();
    let fan_counts =
        |file: &str| -> (u32, u32) { fan_counts_by_file.get(file).copied().unwrap_or((0, 0)) };
    units.sort_by(|a, b| {
        let (a_in, a_out) = fan_counts(&a.file);
        let (b_in, b_out) = fan_counts(&b.file);
        b.out_of_diff
            .len()
            .cmp(&a.out_of_diff.len())
            .then_with(|| b_in.cmp(&a_in))
            .then_with(|| b_out.cmp(&a_out))
            .then_with(|| a.file.cmp(&b.file))
    });

    let order = units.iter().map(|u| u.file.clone()).collect();
    ReviewDirection { order, units }
}

/// Assemble the walkthrough guide from the assembled brief data. Pure over its
/// inputs: the same digest + hash always produce the same guide.
#[must_use]
pub fn build_walkthrough_guide(
    digest: ReviewBriefOutput,
    graph_snapshot_hash: String,
    direction: ReviewDirection,
    change_anchors: Vec<ChangeAnchor>,
) -> WalkthroughGuide {
    WalkthroughGuide {
        schema_version: ReviewBriefSchemaVersion::default(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        command: "review-walkthrough-guide".to_string(),
        graph_snapshot_hash,
        digest,
        direction,
        change_anchors,
        agent_schema: agent_schema(),
        injection_note: INJECTION_NOTE,
    }
}

/// Post-validate the agent's judgment JSON against the live graph.
///
/// The graph is the verifier:
/// 1. If the agent's echoed `graph_snapshot_hash` != `current_hash`, the tree
///    moved: REFUSE the whole payload as stale (accepted empty, every judgment
///    rejected with `stale-snapshot`).
/// 2. Otherwise, each judgment is ACCEPTED iff its `signal_id` is on the
///    decision surface's emitted allowlist ([`DecisionSurface::accept_signal_id`]);
///    an unanchored id is REJECTED (`unanchored-signal-id`). Accepted judgments
///    carry the agent's framing FENCED as non-deterministic.
#[must_use]
#[allow(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet; the change-anchor allowlist is always built with the fallow hasher"
)]
pub fn validate_walkthrough(
    agent: &AgentWalkthrough,
    surface: &DecisionSurface,
    change_anchor_ids: &FxHashSet<String>,
    current_hash: &str,
) -> WalkthroughValidation {
    let stale = agent.graph_snapshot_hash != current_hash;

    let mut accepted: Vec<AcceptedJudgment> = Vec::new();
    let mut rejected: Vec<RejectedJudgment> = Vec::new();

    if stale {
        // Staleness refusal: the tree moved, so NOTHING the agent said can be
        // trusted against this graph. Refuse the whole payload.
        for judgment in &agent.judgments {
            rejected.push(RejectedJudgment {
                signal_id: judgment.signal_id.clone(),
                change_anchor: judgment.change_anchor.clone(),
                reason: "stale-snapshot".to_string(),
            });
        }
    } else {
        for judgment in &agent.judgments {
            match validate_fresh_judgment(judgment, surface, change_anchor_ids) {
                Ok(judgment) => accepted.push(judgment),
                Err(judgment) => rejected.push(judgment),
            }
        }
    }

    let accepted_count = accepted.len();
    let rejected_count = rejected.len();
    // Every accepted judgment is anchored by construction (accept_signal_id was
    // true), so the unanchored count among accepted is always zero. Surfaced as
    // an explicit field so the done-condition asserts on it directly.
    let unanchored_count = 0;

    WalkthroughValidation {
        schema_version: ReviewBriefSchemaVersion::default(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        command: "review-walkthrough-validation".to_string(),
        graph_snapshot_hash: current_hash.to_string(),
        stale,
        accepted,
        rejected,
        accepted_count,
        rejected_count,
        unanchored_count,
    }
}

fn validate_fresh_judgment(
    judgment: &AgentJudgment,
    surface: &DecisionSurface,
    change_anchor_ids: &FxHashSet<String>,
) -> Result<AcceptedJudgment, RejectedJudgment> {
    // A signal_id (graph finding) is the strong anchor; a change_anchor
    // (changed region) is the weaker fallback. Prefer the signal.
    if !judgment.signal_id.is_empty() && surface.accept_signal_id(&judgment.signal_id) {
        return Ok(AcceptedJudgment {
            signal_id: judgment.signal_id.clone(),
            change_anchor: String::new(),
            anchor_kind: "signal".to_string(),
            agent_framing: judgment.framing.clone(),
            concern: judgment.concern.clone(),
            deterministic: false,
        });
    }
    if !judgment.change_anchor.is_empty() && change_anchor_ids.contains(&judgment.change_anchor) {
        return Ok(AcceptedJudgment {
            signal_id: String::new(),
            change_anchor: judgment.change_anchor.clone(),
            anchor_kind: "change".to_string(),
            agent_framing: judgment.framing.clone(),
            concern: judgment.concern.clone(),
            deterministic: false,
        });
    }

    // Cited a change_anchor (but no valid signal_id) and it did not resolve:
    // the region-level miss. Otherwise this is the signal-id miss.
    let reason = if judgment.signal_id.is_empty() && !judgment.change_anchor.is_empty() {
        UNKNOWN_CHANGE_ANCHOR_REASON
    } else {
        UNANCHORED_REASON
    };
    Err(RejectedJudgment {
        signal_id: judgment.signal_id.clone(),
        change_anchor: judgment.change_anchor.clone(),
        reason: reason.to_string(),
    })
}

/// Parse the agent's judgment JSON from a `--walkthrough-file` path's contents.
/// A malformed payload yields an empty `AgentWalkthrough` whose default hash
/// (`""`) will not match any real snapshot hash, so it is refused as stale (the
/// safe direction: a garbled agent file never accepts).
#[must_use]
pub fn parse_agent_walkthrough(contents: &str) -> AgentWalkthrough {
    serde_json::from_str(contents).unwrap_or_else(|_| AgentWalkthrough {
        graph_snapshot_hash: String::new(),
        judgments: Vec::new(),
    })
}

/// Assemble the walkthrough guide from an [`crate::audit::AuditResult`] on the
/// brief path. Reuses [`build_brief_output`] for the digest (graph-only, pure)
/// and the retained routing + impact closure for the direction.
#[must_use]
pub fn build_guide_from_result(result: &crate::audit::AuditResult) -> WalkthroughGuide {
    let digest = build_brief_output(result);
    let hash = result.graph_snapshot_hash.clone().unwrap_or_default();
    let empty_routing = RoutingFacts::default();
    let routing = result.routing.as_ref().unwrap_or(&empty_routing);
    // Per-file out-of-diff map from the (post-stories-filter) coordination gaps:
    // each changed file -> the consumers outside the diff it actually affects, so
    // every direction unit carries its OWN out-of-diff, not the global set.
    let mut out_of_diff_by_file: FxHashMap<String, Vec<String>> = FxHashMap::default();
    if let Some(closure) = result
        .check
        .as_ref()
        .and_then(|c| c.impact_closure.as_ref())
    {
        for gap in &closure.coordination_gap {
            out_of_diff_by_file
                .entry(gap.changed_file.clone())
                .or_default()
                .push(gap.consumer_file.clone());
        }
        for consumers in out_of_diff_by_file.values_mut() {
            consumers.sort();
            consumers.dedup();
        }
    }
    // Spine the direction on the CHANGE (the focus units), with routing as the
    // optional expert overlay, so the work-list is never empty on the author's own
    // PR. Borrow `digest.focus` before `digest` is moved into the guide.
    let direction = build_direction(&digest.focus, &out_of_diff_by_file, routing);
    build_walkthrough_guide(digest, hash, direction, result.change_anchors.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::routing::RoutingUnit;
    use crate::audit_brief::ReviewDeltas;
    use crate::audit_decision_surface::{
        BoundaryAnchor, DecisionCategory, DecisionInputs, derive_signal_id,
        extract_decision_surface,
    };

    fn no_source(_: &str) -> Option<String> {
        None
    }

    /// Build a synthetic decision surface with one coupling/boundary decision,
    /// returning the surface plus the one real emitted signal id.
    fn surface_with_one_signal() -> (DecisionSurface, String) {
        let deltas = ReviewDeltas {
            boundary_introduced: vec!["ui->-db".to_string()],
            cycle_introduced: Vec::new(),
            public_api_added: Vec::new(),
        };
        let anchors = vec![BoundaryAnchor {
            zone_pair_key: "ui->-db".to_string(),
            from_file: "src/ui/page.ts".to_string(),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            line: 1,
        }];
        let routing = RoutingFacts::default();
        let surface = extract_decision_surface(&DecisionInputs {
            deltas: &deltas,
            boundary_anchors: &anchors,
            coordination: &[],
            public_api_anchor_line: 0,
            affected_not_shown: 3,
            routing: &routing,
            head_source: &no_source,
            rename_old_path: &no_source,
            internal_consumers: &|_: &str| 0u64,
            cap: 4,
        });
        let real_id = derive_signal_id(DecisionCategory::CouplingBoundary, "ui->-db");
        (surface, real_id)
    }

    // Done-condition (a): a valid agent JSON citing only emitted signal_ids with
    // the correct snapshot hash is ACCEPTED with zero unanchored findings.
    #[test]
    fn clean_agent_json_is_accepted_with_zero_unanchored() {
        let (surface, real_id) = surface_with_one_signal();
        let hash = "graph:abc123";
        let agent = AgentWalkthrough {
            graph_snapshot_hash: hash.to_string(),
            judgments: vec![AgentJudgment {
                signal_id: real_id.clone(),
                change_anchor: String::new(),
                framing: "Intended coupling, payments boundary widened on purpose.".to_string(),
                concern: Some("coupling".to_string()),
            }],
        };
        let validation = validate_walkthrough(&agent, &surface, &FxHashSet::default(), hash);
        assert!(!validation.stale, "matching hash is not stale");
        assert_eq!(
            validation.accepted_count, 1,
            "the anchored judgment accepts"
        );
        assert_eq!(validation.rejected_count, 0, "no rejections");
        assert_eq!(validation.unanchored_count, 0, "zero unanchored findings");
        // The framing is fenced as non-deterministic.
        assert!(!validation.accepted[0].deterministic);
        assert_eq!(validation.accepted[0].signal_id, real_id);
    }

    // Done-condition (b): an injected unanchored finding is REJECTED.
    #[test]
    fn injected_unanchored_signal_id_is_rejected() {
        let (surface, real_id) = surface_with_one_signal();
        let hash = "graph:abc123";
        let agent = AgentWalkthrough {
            graph_snapshot_hash: hash.to_string(),
            judgments: vec![
                AgentJudgment {
                    signal_id: real_id.clone(),
                    change_anchor: String::new(),
                    framing: "real".to_string(),
                    concern: None,
                },
                AgentJudgment {
                    // A fabricated id fallow never emitted.
                    signal_id: "sig:deadbeefdeadbeef".to_string(),
                    change_anchor: String::new(),
                    framing: "hallucinated decision with no graph anchor".to_string(),
                    concern: None,
                },
            ],
        };
        let validation = validate_walkthrough(&agent, &surface, &FxHashSet::default(), hash);
        assert_eq!(validation.accepted_count, 1, "only the real one accepts");
        assert_eq!(validation.rejected_count, 1, "the fabricated one rejects");
        assert_eq!(validation.rejected[0].signal_id, "sig:deadbeefdeadbeef");
        assert_eq!(validation.rejected[0].reason, UNANCHORED_REASON);
        // The accepted set never contains the fabricated id.
        assert!(
            validation.accepted.iter().all(|j| j.signal_id == real_id),
            "accepted excludes the unanchored id"
        );
    }

    // Done-condition (c): stale JSON (mutated tree / old snapshot hash) is REFUSED.
    #[test]
    fn stale_snapshot_hash_refuses_the_whole_payload() {
        let (surface, real_id) = surface_with_one_signal();
        let current_hash = "graph:NEW_after_mutation";
        // The agent echoed the OLD hash (the tree moved since the guide).
        let agent = AgentWalkthrough {
            graph_snapshot_hash: "graph:OLD_before_mutation".to_string(),
            judgments: vec![AgentJudgment {
                // Even a real signal id is refused under a stale snapshot.
                signal_id: real_id,
                change_anchor: String::new(),
                framing: "would be valid, but the tree moved".to_string(),
                concern: None,
            }],
        };
        let validation =
            validate_walkthrough(&agent, &surface, &FxHashSet::default(), current_hash);
        assert!(validation.stale, "old hash is stale");
        assert_eq!(validation.accepted_count, 0, "nothing accepts when stale");
        assert_eq!(validation.rejected_count, 1, "the judgment is refused");
        assert_eq!(validation.rejected[0].reason, "stale-snapshot");
    }

    #[test]
    fn malformed_agent_json_parses_to_a_stale_refusal() {
        let agent = parse_agent_walkthrough("{not valid json");
        assert!(agent.graph_snapshot_hash.is_empty());
        assert!(agent.judgments.is_empty());
        let (surface, _) = surface_with_one_signal();
        let validation =
            validate_walkthrough(&agent, &surface, &FxHashSet::default(), "graph:real");
        assert!(
            validation.stale,
            "empty echoed hash never matches a real hash"
        );
        assert_eq!(validation.accepted_count, 0);
    }

    fn focus_unit(file: &str, total: u32) -> crate::audit_focus::FocusUnit {
        crate::audit_focus::FocusUnit {
            file: file.to_string(),
            score: crate::audit_focus::FocusScore {
                total,
                ..Default::default()
            },
            label: crate::audit_focus::FocusLabel::ReviewHere,
            reason: String::new(),
            confidence: Vec::new(),
        }
    }

    /// A focus unit whose `reason` carries a displayed importer count (the exact
    /// "high fan-in (N importers)" phrasing `build_reason` emits) AND a deliberately
    /// disagreeing, capped `fan_io` + composite `total`, so a test can prove the
    /// within-stage order follows the DISPLAYED importer count, not the hidden blend.
    fn focus_unit_fan(
        file: &str,
        importers: u32,
        fan_io: u32,
        total: u32,
    ) -> crate::audit_focus::FocusUnit {
        let s = if importers == 1 { "" } else { "s" };
        crate::audit_focus::FocusUnit {
            file: file.to_string(),
            score: crate::audit_focus::FocusScore {
                fan_io,
                total,
                ..Default::default()
            },
            label: crate::audit_focus::FocusLabel::ReviewHere,
            reason: format!("high fan-in ({importers} importer{s})"),
            confidence: Vec::new(),
        }
    }

    // ORDERING PRINCIPLE: within Stage 2 the order is the DISPLAYED importer count
    // ("high fan-in (N importers)"), NOT the hidden, capped `fan_io` blend. Here
    // a.ts shows 5 importers, b.ts shows 60 -- but both cap to the SAME `fan_io`
    // and a.ts even has the larger composite `total`. Neither has out-of-diff
    // consumers (both Stage 2), so the order must follow the shown importer count:
    // the 60-importer file first.
    #[test]
    fn within_stage_order_follows_visible_fan_io_not_hidden_total() {
        let focus = FocusMap {
            review_here: vec![
                // a.ts: 5 importers shown, fan_io capped to 8, total 99 (hidden high).
                focus_unit_fan("src/a.ts", 5, 8, 99),
                // b.ts: 60 importers shown, fan_io ALSO capped to 8, total 1.
                focus_unit_fan("src/b.ts", 60, 8, 1),
            ],
            deprioritized: vec![],
        };
        let direction = build_direction(&focus, &FxHashMap::default(), &RoutingFacts::default());
        // The displayed importer count decides: b.ts (60 importers) before a.ts (5),
        // even though they share a `fan_io` and a.ts has the larger composite total.
        assert_eq!(direction.order, vec!["src/b.ts", "src/a.ts"]);
    }

    // Reading top-to-bottom, the shown importer count is non-increasing, and
    // fan-out breaks ties among equal-importer files (here zero-importer files).
    #[test]
    fn stage_two_orders_by_importer_then_fanout_then_path() {
        let focus = FocusMap {
            review_here: vec![
                // Two zero-importer files separated only by fan-out, plus one
                // high-importer file that must lead regardless.
                crate::audit_focus::FocusUnit {
                    file: "src/zlo.ts".to_string(),
                    score: crate::audit_focus::FocusScore::default(),
                    label: crate::audit_focus::FocusLabel::ReviewHere,
                    reason: "fan-out 1".to_string(),
                    confidence: Vec::new(),
                },
                crate::audit_focus::FocusUnit {
                    file: "src/zhi.ts".to_string(),
                    score: crate::audit_focus::FocusScore::default(),
                    label: crate::audit_focus::FocusLabel::ReviewHere,
                    reason: "fan-out 9".to_string(),
                    confidence: Vec::new(),
                },
                focus_unit_fan("src/top.ts", 12, 8, 1),
            ],
            deprioritized: vec![],
        };
        let direction = build_direction(&focus, &FxHashMap::default(), &RoutingFacts::default());
        // top.ts (12 importers) leads; then the zero-importer files by fan-out
        // (9 before 1), so the displayed counts are non-increasing top to bottom.
        assert_eq!(
            direction.order,
            vec!["src/top.ts", "src/zhi.ts", "src/zlo.ts"]
        );
    }

    // Contract-breakers (out-of-diff consumers) remain the strict first priority
    // class regardless of importer count: an out-of-diff unit sorts ahead of a
    // higher-fan-in orientation unit.
    #[test]
    fn out_of_diff_units_sort_ahead_of_higher_fan_io_orientation_units() {
        let focus = FocusMap {
            review_here: vec![
                // contract.ts shows just 1 importer; orient.ts shows 9.
                focus_unit_fan("src/contract.ts", 1, 1, 1),
                focus_unit_fan("src/orient.ts", 9, 9, 9),
            ],
            deprioritized: vec![],
        };
        let mut out_of_diff = FxHashMap::default();
        out_of_diff.insert(
            "src/contract.ts".to_string(),
            vec!["src/consumer.ts".to_string()],
        );
        let direction = build_direction(&focus, &out_of_diff, &RoutingFacts::default());
        // contract.ts breaks a contract -> sorts first even with the lower importer
        // count.
        assert_eq!(direction.order, vec!["src/contract.ts", "src/orient.ts"]);
        assert_eq!(direction.units[0].concern_lens, "contract-break");
    }

    #[test]
    fn direction_spines_on_focus_units_with_expert_overlay() {
        // The SPINE is the change (focus units), never the routing. The author's
        // own PR has expert: [] on every routing unit, yet the direction still
        // enumerates the units. b.ts has a real expert overlay; a.ts has none.
        let focus = FocusMap {
            review_here: vec![focus_unit("src/b.ts", 5), focus_unit("src/a.ts", 3)],
            deprioritized: vec![],
        };
        let routing = RoutingFacts {
            units: vec![RoutingUnit {
                file: "src/b.ts".to_string(),
                expert: vec!["@team".to_string()],
                bus_factor_one: false,
            }],
        };
        // Only src/a.ts has an out-of-diff consumer; src/b.ts has none.
        let mut out_of_diff_by_file = FxHashMap::default();
        out_of_diff_by_file.insert("src/a.ts".to_string(), vec!["src/consumer.ts".to_string()]);
        let direction = build_direction(&focus, &out_of_diff_by_file, &routing);
        // a.ts breaks a contract -> sorts first with the contract-break lens,
        // carrying its budget; b.ts has no out-of-diff -> orientation, but the
        // expert overlay still attaches @team.
        assert_eq!(direction.order, vec!["src/a.ts", "src/b.ts"]);
        assert_eq!(direction.units[0].file, "src/a.ts");
        assert_eq!(direction.units[0].concern_lens, "contract-break");
        assert_eq!(direction.units[0].out_of_diff, vec!["src/consumer.ts"]);
        assert_eq!(direction.units[0].scoring_budget, 3);
        assert!(direction.units[0].expert.is_empty());
        assert_eq!(direction.units[1].file, "src/b.ts");
        assert_eq!(direction.units[1].concern_lens, "orientation");
        assert_eq!(direction.units[1].scoring_budget, 5);
        assert_eq!(direction.units[1].expert, vec!["@team".to_string()]);
    }

    #[test]
    fn direction_excludes_non_source_units() {
        let focus = FocusMap {
            review_here: vec![
                focus_unit("LICENSE", 1),
                focus_unit(".gitignore", 1),
                focus_unit("README.md", 1),
                focus_unit("src/app.component.ts", 4),
            ],
            deprioritized: vec![],
        };
        let direction = build_direction(&focus, &FxHashMap::default(), &RoutingFacts::default());
        // Only the source unit survives; docs/config/license churn is dropped.
        assert_eq!(direction.order, vec!["src/app.component.ts"]);
        assert_eq!(direction.units[0].concern_lens, "orientation");
        assert_eq!(direction.units[0].scoring_budget, 4);
    }

    #[test]
    fn guide_carries_the_snapshot_hash_and_injection_note() {
        let digest = ReviewBriefOutput {
            schema_version: ReviewBriefSchemaVersion::default(),
            version: "test".to_string(),
            command: "audit-brief".to_string(),
            triage: crate::audit_brief::DiffTriage {
                files: 0,
                hunks: None,
                net_lines: None,
                risk_class: crate::audit_brief::RiskClass::Low,
                review_effort: crate::audit_brief::ReviewEffort::Glance,
            },
            graph_facts: crate::audit_brief::GraphFacts {
                exports_added: 0,
                api_width_delta: 0,
                reachable_from: Vec::new(),
                boundaries_touched: Vec::new(),
            },
            partition: crate::audit_brief::PartitionFacts::default(),
            impact_closure: crate::audit_brief::ImpactClosureFacts::default(),
            focus: crate::audit_focus::FocusMap::default(),
            deltas: ReviewDeltas::default(),
            weakening: Vec::new(),
            routing: RoutingFacts::default(),
            decisions: DecisionSurface::default(),
        };
        let guide = build_walkthrough_guide(
            digest,
            "graph:pinned".to_string(),
            ReviewDirection::default(),
            Vec::new(),
        );
        assert_eq!(guide.graph_snapshot_hash, "graph:pinned");
        assert!(guide.injection_note.contains("untrusted"));
        assert_eq!(guide.command, "review-walkthrough-guide");
        assert!(guide.agent_schema.anchoring_rule.contains("rejected"));
    }

    // change_anchor: a content-addressed id is stable across line-shifts and
    // whitespace-only edits, and namespaced under `chg:`.
    #[test]
    fn derive_change_anchor_id_is_stable_and_namespaced() {
        let added = vec!["const x = 1;".to_string(), "return x;".to_string()];
        let normalized = normalize_added_text(&added);
        let id = derive_change_anchor_id("src/a.ts", &normalized, 0);
        assert!(id.starts_with("chg:"), "namespaced under chg:");
        // Same content at a DIFFERENT line (the start line is not hashed) -> same id.
        assert_eq!(id, derive_change_anchor_id("src/a.ts", &normalized, 0));
        // A whitespace-only reflow normalizes to the same text -> same id.
        let reflowed = vec!["  const x = 1;  ".to_string(), "\treturn x;".to_string()];
        assert_eq!(
            id,
            derive_change_anchor_id("src/a.ts", &normalize_added_text(&reflowed), 0)
        );
        // Different added text -> different id.
        assert_ne!(
            id,
            derive_change_anchor_id(
                "src/a.ts",
                &normalize_added_text(&["const y = 2;".to_string()]),
                0
            )
        );
        // Same text in a different file -> different id.
        assert_ne!(id, derive_change_anchor_id("src/b.ts", &normalized, 0));
    }

    // change_anchor: parsing a unified diff yields one anchor per hunk; an edit
    // ABOVE a hunk shifts its start_line but NOT its content-addressed id.
    #[test]
    fn parse_change_anchors_is_line_shift_stable() {
        let diff_a = "diff --git a/src/x.ts b/src/x.ts\n--- a/src/x.ts\n+++ b/src/x.ts\n@@ -10,0 +11,1 @@\n+  const added = compute();\n";
        let diff_b = "diff --git a/src/x.ts b/src/x.ts\n--- a/src/x.ts\n+++ b/src/x.ts\n@@ -40,0 +41,1 @@\n+  const added = compute();\n";
        let a = parse_change_anchors(diff_a);
        let b = parse_change_anchors(diff_b);
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(
            a[0].change_anchor, b[0].change_anchor,
            "id is line-shift stable"
        );
        assert_eq!(a[0].start_line, 11);
        assert_eq!(b[0].start_line, 41, "start_line tracks the new position");
    }

    // change_anchor: a judgment citing an emitted change_anchor is ACCEPTED with
    // anchor_kind=change; an unknown change_anchor is REJECTED.
    #[test]
    fn change_anchor_judgment_accepts_and_unknown_rejects() {
        let (surface, _) = surface_with_one_signal();
        let hash = "graph:abc123";
        let diff = "diff --git a/src/x.ts b/src/x.ts\n--- a/src/x.ts\n+++ b/src/x.ts\n@@ -1,0 +2,1 @@\n+  const added = compute();\n";
        let anchors = parse_change_anchors(diff);
        let allow = change_anchor_allowlist(&anchors);
        let real = anchors[0].change_anchor.clone();
        let agent = AgentWalkthrough {
            graph_snapshot_hash: hash.to_string(),
            judgments: vec![
                AgentJudgment {
                    signal_id: String::new(),
                    change_anchor: real.clone(),
                    framing: "this region trades simplicity for a cache".to_string(),
                    concern: None,
                },
                AgentJudgment {
                    signal_id: String::new(),
                    change_anchor: "chg:deadbeefdeadbeef".to_string(),
                    framing: "hallucinated region".to_string(),
                    concern: None,
                },
            ],
        };
        let validation = validate_walkthrough(&agent, &surface, &allow, hash);
        assert_eq!(
            validation.accepted_count, 1,
            "the real change_anchor accepts"
        );
        assert_eq!(validation.accepted[0].anchor_kind, "change");
        assert_eq!(validation.accepted[0].change_anchor, real);
        assert!(validation.accepted[0].signal_id.is_empty());
        assert!(!validation.accepted[0].deterministic);
        assert_eq!(
            validation.rejected_count, 1,
            "the fabricated region rejects"
        );
        assert_eq!(validation.rejected[0].reason, UNKNOWN_CHANGE_ANCHOR_REASON);
        assert_eq!(validation.rejected[0].change_anchor, "chg:deadbeefdeadbeef");
    }

    // change_anchor: a stale snapshot refuses a change_anchor judgment too.
    #[test]
    fn stale_snapshot_refuses_change_anchor_judgment() {
        let (surface, _) = surface_with_one_signal();
        let diff = "diff --git a/src/x.ts b/src/x.ts\n--- a/src/x.ts\n+++ b/src/x.ts\n@@ -1,0 +2,1 @@\n+  const added = compute();\n";
        let anchors = parse_change_anchors(diff);
        let allow = change_anchor_allowlist(&anchors);
        let agent = AgentWalkthrough {
            graph_snapshot_hash: "graph:OLD".to_string(),
            judgments: vec![AgentJudgment {
                signal_id: String::new(),
                change_anchor: anchors[0].change_anchor.clone(),
                framing: "valid region, but the tree moved".to_string(),
                concern: None,
            }],
        };
        let validation = validate_walkthrough(&agent, &surface, &allow, "graph:NEW");
        assert!(validation.stale);
        assert_eq!(validation.accepted_count, 0, "nothing accepts when stale");
        assert_eq!(validation.rejected[0].reason, "stale-snapshot");
    }

    // change_anchor: a renamed file's anchor resolves via previous_change_anchor,
    // so an agent that cited the pre-rename id still anchors.
    #[test]
    fn change_anchor_survives_rename_via_previous_anchor() {
        let renamed = "diff --git a/src/old.ts b/src/new.ts\nrename from src/old.ts\nrename to src/new.ts\n--- a/src/old.ts\n+++ b/src/new.ts\n@@ -1,0 +2,1 @@\n+  const added = compute();\n";
        let anchors = parse_change_anchors(renamed);
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].file, "src/new.ts");
        let previous = anchors[0]
            .previous_change_anchor
            .clone()
            .expect("rename yields a previous anchor");
        // The previous id equals what the same hunk under the OLD path would emit.
        let old_diff = "diff --git a/src/old.ts b/src/old.ts\n--- a/src/old.ts\n+++ b/src/old.ts\n@@ -1,0 +2,1 @@\n+  const added = compute();\n";
        let old_anchors = parse_change_anchors(old_diff);
        assert_eq!(previous, old_anchors[0].change_anchor);
        // The allowlist contains BOTH the new id and the pre-rename id.
        let allow = change_anchor_allowlist(&anchors);
        assert!(
            allow.contains(&previous),
            "pre-rename id is in the allowlist"
        );
        assert!(allow.contains(&anchors[0].change_anchor));
    }
}
