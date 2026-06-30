//! Decision-surface extractor (stage 6 / 6.G): THE product.
//!
//! The apex of the review brief. A change embeds many decisions; almost all are
//! mechanical and a few are consequential enough to need human taste. This
//! extractor lifts the consequential STRUCTURAL decisions out of the scattered
//! diff, frames each as a judgment question, ranks by consequence (blast x
//! reversibility), caps the surface to a working-memory-sized handful (4 plus or
//! minus 1), collapses the mechanical remainder, and pairs each decision with the
//! routed expert ("who to ask").
//!
//! ## The SOLID-3 (the ONLY categories that ship)
//!
//! Per the verdict (`.plans/agentic-review-e0-verdict.md`) the decision
//! categories are NOT uniformly reliable on a syntactic engine (ADR-001). Exactly
//! three are validated and shippable, each backed by a deterministic signal
//! fallow already emits:
//!
//! 1. **coupling/boundary** (`boundary_introduced`): a new cross-zone edge.
//! 2. **public-API/contract** (`public_api_added` + coordination gaps): a
//!    new exports-aware public surface, or a changed contract consumed by modules
//!    outside the diff.
//! 3. **dependency**: a new `package.json` dependency entry (the arm is present;
//!    its candidate source is a dependency delta not yet threaded on the brief
//!    path, so it produces decisions only once that delta lands, never a
//!    fabricated signal).
//!
//! The four CUT categories (abstraction-with-1-implementor, deletion-still-
//! reachable, convention-divergence, irreversibility/migration) are CONFIRMED
//! NOISE and MUST NOT ship. `DecisionCategory` has exactly three discriminants,
//! so a cut category is not even representable: the type system is the guarantee.
//!
//! ## The trust mechanism (anti-hallucination)
//!
//! Post-validation closes on EXTRACTION, not on framing. Every decision carries a
//! `signal_id` deterministically derived from the fallow-emitted candidate key it
//! frames (a delta key or a coordination-gap key). The deterministic layer keeps
//! the SET of signal_ids it emitted; `DecisionSurface::accept_signal_id` returns
//! true iff an id is in that set. An agent-proposed decision whose `signal_id` was
//! never emitted is REJECTED. The agent proposes; the graph disposes.

pub use fallow_output::{
    Decision, DecisionCategory, DecisionSurface, TruncationNote, build_decision_surface_output,
};
use xxhash_rust::xxh3::xxh3_64;

use crate::audit_brief::ReviewDeltas;
use fallow_output::RoutingFacts;

/// Default decision-surface cap (the working-memory limit). The surface holds at
/// most this many ranked decisions; the rest collapse into a truncation note.
pub const DEFAULT_DECISION_CAP: usize = 4;
/// Lower bound on the configurable cap (4 minus 1).
pub const MIN_DECISION_CAP: usize = 3;
/// Upper bound on the configurable cap (4 plus 1).
pub const MAX_DECISION_CAP: usize = 5;

/// Derive a deterministic, content-addressed `signal_id` from a category tag plus
/// the fallow-emitted candidate key. The tag namespaces the key so a boundary key
/// and a public-API key sharing text never collide. Pure: same inputs always
/// yield the same id (byte-identical across runs).
#[must_use]
pub fn derive_signal_id(category: DecisionCategory, candidate_key: &str) -> String {
    let mut bytes = Vec::with_capacity(category.tag().len() + 1 + candidate_key.len());
    bytes.extend_from_slice(category.tag().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(candidate_key.as_bytes());
    format!("sig:{:016x}", xxh3_64(&bytes))
}

/// A representative boundary violation used to anchor a coupling/boundary
/// decision to a file + line. Decoupled from the `fallow_types` finding type so
/// the extractor unit-tests without constructing full findings.
#[derive(Debug, Clone)]
pub struct BoundaryAnchor {
    /// The R2 zone-pair key (`"<from_zone>->-<to_zone>"`), matching
    /// `ReviewDeltas::boundary_introduced`.
    pub zone_pair_key: String,
    /// Root-relative path of the importing file (the decision anchor).
    pub from_file: String,
    /// The `from_zone` of the edge (for the framed question).
    pub from_zone: String,
    /// The `to_zone` of the edge (for the framed question).
    pub to_zone: String,
    /// 1-based line of the offending import (the suppression anchor).
    pub line: u32,
}

/// A coordination gap projected onto the public-API/contract decision shape: a
/// changed contract consumed by a module outside the diff.
#[derive(Debug, Clone)]
pub struct CoordinationAnchor {
    /// Root-relative path of the changed file whose contract is consumed elsewhere.
    pub changed_file: String,
    /// The consumed symbol names (the contract).
    pub consumed_symbols: Vec<String>,
    /// Count of distinct non-diff consumers of this changed file's contract.
    pub consumer_count: u64,
    /// 1-based line of the contract symbol's declaration in `changed_file`, so the
    /// decision deep-links / inline-anchors to the exact export. `0` when the line
    /// could not be resolved (graph not retained or file unreadable).
    pub line: u32,
}

/// All inputs the extractor needs, gathered from the assembled brief data.
pub struct DecisionInputs<'a> {
    /// Diff-aware deltas (boundary + public-API). The candidate source.
    pub deltas: &'a ReviewDeltas,
    /// Boundary anchors keyed by zone-pair, one representative per introduced edge.
    pub boundary_anchors: &'a [BoundaryAnchor],
    /// Coordination gaps projected to the contract decision shape.
    pub coordination: &'a [CoordinationAnchor],
    /// 1-based line of the first widened public-API export's declaration, so the
    /// public-API-surface decision anchors to a real line. `0` when unresolved.
    pub public_api_anchor_line: u32,
    /// Project-wide fan-in beyond the diff (impact-closure `affected_not_shown`).
    /// Used as the blast magnitude for boundary + public-API-surface decisions.
    pub affected_not_shown: u64,
    /// Ownership routing (routed expert per file).
    pub routing: &'a RoutingFacts,
    /// Per-anchor-file head source, for suppression checks. `None` for a file
    /// whose head content could not be read (the decision is then not suppressed).
    pub head_source: &'a dyn Fn(&str) -> Option<String>,
    /// Resolve a head (post-rename) root-relative path to its pre-rename path, from
    /// the diff's rename pairs. `None` when the file was not renamed. Lets each
    /// decision carry a `previous_signal_id` so review memory survives a `git mv`.
    pub rename_old_path: &'a dyn Fn(&str) -> Option<String>,
    /// Honest per-anchor in-repo out-of-diff consumer count, precomputed from the
    /// retained graph's reverse-deps before it was dropped. `0` for an anchor with
    /// no recorded importers (a genuinely new file). The display number; distinct
    /// from `affected_not_shown` (the project-wide ranking proxy).
    pub internal_consumers: &'a dyn Fn(&str) -> u64,
    /// The decision cap (default 4, clamped to [3, 5] by the caller).
    pub cap: usize,
}

/// Resolve the routed expert(s) + bus-factor flag for a decision's anchor file.
fn route_for(routing: &RoutingFacts, anchor_file: &str) -> (Vec<String>, bool) {
    routing
        .units
        .iter()
        .find(|unit| unit.file == anchor_file)
        .map_or((Vec::new(), false), |unit| {
            (unit.expert.clone(), unit.bus_factor_one)
        })
}

/// Whether the head source of `anchor_file` suppresses a decision of `category`
/// at (1-based) `line`. Honors a file-level `fallow-ignore-file` and a
/// line-level `fallow-ignore-next-line` immediately above the anchor line, in
/// both the category-scoped (`decision-surface` / category tag) and bare forms.
fn is_decision_suppressed(
    head_source: Option<&str>,
    category: DecisionCategory,
    line: u32,
) -> bool {
    let Some(source) = head_source else {
        return false;
    };
    let lines: Vec<&str> = source.lines().collect();
    let token_matches = |comment: &str| {
        if !comment.contains("fallow-ignore") {
            return false;
        }
        // A bare ignore (no kind) suppresses; a kinded ignore must name the
        // decision-surface family or this decision's category tag.
        let after = comment
            .split_once("fallow-ignore-file")
            .or_else(|| comment.split_once("fallow-ignore-next-line"))
            .map(|(_, rest)| rest.trim());
        match after {
            None => false,
            Some("") => true,
            Some(rest) => {
                rest.contains("decision-surface")
                    || rest.contains("decision-surfaces")
                    || rest.contains(category.tag())
            }
        }
    };

    // File-level: any line carrying a file-level ignore.
    if lines
        .iter()
        .any(|l| l.contains("fallow-ignore-file") && token_matches(l))
    {
        return true;
    }
    // Line-level: the comment sits immediately above the 1-based anchor line.
    if line >= 2
        && let Some(prev) = lines.get((line - 2) as usize)
        && prev.contains("fallow-ignore-next-line")
        && token_matches(prev)
    {
        return true;
    }
    false
}

/// Frame a coupling/boundary decision as a judgment question.
fn boundary_question(from_zone: &str, to_zone: &str) -> String {
    format!(
        "`{from_zone}` now imports `{to_zone}` for the first time. Intended coupling, or should this edge not exist?"
    )
}

/// Frame the (batch-consolidated, R1) public-API-surface decision.
fn public_api_question(count: usize) -> String {
    format!(
        "This change adds {count} export{} to the public API surface. Intended as maintained contracts, or should they stay internal?",
        if count == 1 { "" } else { "s" }
    )
}

/// Frame a coordination-gap (contract consumed outside the diff) decision.
fn coordination_question(changed_file: &str, symbols: &[String], consumers: u64) -> String {
    format!(
        "`{changed_file}` changes {} ({}) imported by {consumers} {} outside this PR. Does this change break or alter what those callers expect?",
        if symbols.len() == 1 {
            "export"
        } else {
            "exports"
        },
        symbols.join(", "),
        if consumers == 1 { "file" } else { "files" }
    )
}

/// Pluralize "module" against a count.
fn modules_word(n: u64) -> &'static str {
    if n == 1 { "module" } else { "modules" }
}

/// Subject-verb agreement for the per-clause count: a singular subject takes the
/// "-s" verb form ("1 module depends"), plural drops it ("2 modules depend").
fn agrees(verb_plural: &str, n: u64) -> String {
    if n == 1 {
        format!("{verb_plural}s")
    } else {
        verb_plural.to_string()
    }
}

/// The named structural sacrifice for a coupling/boundary decision, as a FACT.
/// `consumers` is the honest in-repo out-of-diff count for the anchor.
fn boundary_tradeoff(from_zone: &str, to_zone: &str, consumers: u64) -> String {
    format!(
        "Couples `{from_zone}` to `{to_zone}`; {consumers} in-repo {} already {} on this anchor.",
        modules_word(consumers),
        agrees("depend", consumers)
    )
}

/// The named structural sacrifice for the public-API-surface decision, as a FACT.
/// The internal count is internal-only, so the clause also names the external
/// contract risk in prose (it cannot count a published library's downstream).
fn public_api_tradeoff(count: usize, consumers: u64) -> String {
    format!(
        "Adds {count} maintained contract{}; {consumers} in-repo {} already {} this surface, and any external consumers become a contract you cannot remove without a breaking change.",
        if count == 1 { "" } else { "s" },
        modules_word(consumers),
        agrees("consume", consumers)
    )
}

/// The named structural sacrifice for a coordination-gap decision, as a FACT.
fn coordination_tradeoff(consumers: u64) -> String {
    format!(
        "{consumers} {} outside the diff {} this contract; changing its shape requires coordinating them.",
        modules_word(consumers),
        agrees("consume", consumers)
    )
}

/// The per-decision fields for [`build_decision`], distinct from the shared
/// run context carried in [`DecisionInputs`].
struct DecisionSpec {
    category: DecisionCategory,
    candidate_key: String,
    question: String,
    anchor_file: String,
    anchor_line: u32,
    blast: u64,
    /// Honest per-decision in-repo out-of-diff consumer count (display number).
    internal_consumer_count: u64,
    /// The named-sacrifice clause, stated as a fact.
    tradeoff: String,
}

/// Build one decision, resolving its routed expert and suppression state.
fn build_decision(spec: DecisionSpec, inputs: &DecisionInputs<'_>) -> Decision {
    let DecisionSpec {
        category,
        candidate_key,
        question,
        anchor_file,
        anchor_line,
        blast,
        internal_consumer_count,
        tradeoff,
    } = spec;
    let signal_id = derive_signal_id(category, &candidate_key);
    // Rename-durable review memory: if any path embedded in the candidate key was
    // renamed, derive the signal_id this decision WOULD have had under the old
    // path so the cloud can carry a prior dismissal across the move.
    let previous_signal_id = remap_key_paths(&candidate_key, inputs.rename_old_path)
        .map(|old_key| derive_signal_id(category, &old_key));
    let (expert, bus_factor_one) = route_for(inputs.routing, &anchor_file);
    let consequence = blast.saturating_mul(category.reversibility_weight());
    Decision {
        signal_id,
        category,
        question,
        anchor_file,
        anchor_line,
        signal_key: candidate_key,
        previous_signal_id,
        blast,
        consequence,
        expert,
        bus_factor_one,
        internal_consumer_count,
        tradeoff,
    }
}

/// Rebuild a candidate key with every embedded rel path swapped to its pre-rename
/// form via `rename_old_path`. The key embeds paths as `contract:<path>` or as
/// `|`-joined `<path>::<name>` components (boundary zone-pair keys carry no path).
/// Returns the rebuilt, re-sorted key iff at least one path moved, else `None`.
fn remap_key_paths(key: &str, rename_old_path: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut moved = false;
    let mut parts: Vec<String> = key
        .split('|')
        .map(|segment| {
            if let Some(path) = segment.strip_prefix("contract:")
                && let Some(old) = rename_old_path(path)
            {
                moved = true;
                return format!("contract:{old}");
            } else if let Some((path, name)) = segment.split_once("::")
                && let Some(old) = rename_old_path(path)
            {
                moved = true;
                return format!("{old}::{name}");
            }
            segment.to_string()
        })
        .collect();
    if !moved {
        return None;
    }
    // The public-API key is the SORTED added-key set joined; re-sort so the rebuilt
    // key matches what the pre-rename change would have emitted.
    parts.sort();
    Some(parts.join("|"))
}

/// Classify the candidate signals into framed decisions (pre-rank, pre-cap).
fn classify_candidates(inputs: &DecisionInputs<'_>) -> Vec<Decision> {
    let mut decisions: Vec<Decision> = Vec::new();

    // (1) Coupling/boundary: one decision per introduced zone-pair edge (R2).
    for key in &inputs.deltas.boundary_introduced {
        let anchor = inputs
            .boundary_anchors
            .iter()
            .find(|a| &a.zone_pair_key == key);
        let (anchor_file, anchor_line, from_zone, to_zone) = anchor.map_or_else(
            || (String::new(), 0, key.clone(), String::new()),
            |a| {
                (
                    a.from_file.clone(),
                    a.line,
                    a.from_zone.clone(),
                    a.to_zone.clone(),
                )
            },
        );
        let internal_consumer_count = (inputs.internal_consumers)(&anchor_file);
        decisions.push(build_decision(
            DecisionSpec {
                category: DecisionCategory::CouplingBoundary,
                candidate_key: key.clone(),
                question: boundary_question(&from_zone, &to_zone),
                tradeoff: boundary_tradeoff(&from_zone, &to_zone, internal_consumer_count),
                anchor_file,
                anchor_line,
                blast: inputs.affected_not_shown,
                internal_consumer_count,
            },
            inputs,
        ));
    }

    // (2a) Public-API surface: R1 batch-consolidate to ONE decision per change.
    if !inputs.deltas.public_api_added.is_empty() {
        // The candidate key is the full sorted added-key set joined: one stable
        // id per change, never one-per-symbol (kills the 111-export noise).
        let key = inputs.deltas.public_api_added.join("|");
        let anchor_file = inputs
            .deltas
            .public_api_added
            .first()
            .and_then(|k| k.split("::").next())
            .map(str::to_string)
            .unwrap_or_default();
        let internal_consumer_count = (inputs.internal_consumers)(&anchor_file);
        decisions.push(build_decision(
            DecisionSpec {
                category: DecisionCategory::PublicApiContract,
                candidate_key: key,
                question: public_api_question(inputs.deltas.public_api_added.len()),
                tradeoff: public_api_tradeoff(
                    inputs.deltas.public_api_added.len(),
                    internal_consumer_count,
                ),
                anchor_file,
                anchor_line: inputs.public_api_anchor_line,
                blast: inputs.affected_not_shown,
                internal_consumer_count,
            },
            inputs,
        ));
    }

    // (2b) Coordination gaps: a changed contract consumed outside the diff. One
    // decision per (changed file) contract, keyed on the changed file path.
    for gap in inputs.coordination {
        let key = format!("contract:{}", gap.changed_file);
        decisions.push(build_decision(
            DecisionSpec {
                category: DecisionCategory::PublicApiContract,
                candidate_key: key,
                question: coordination_question(
                    &gap.changed_file,
                    &gap.consumed_symbols,
                    gap.consumer_count,
                ),
                tradeoff: coordination_tradeoff(gap.consumer_count),
                anchor_file: gap.changed_file.clone(),
                anchor_line: gap.line,
                blast: gap.consumer_count,
                // The coordination arm already carries the honest per-decision
                // count; no precomputed-map lookup needed.
                internal_consumer_count: gap.consumer_count,
            },
            inputs,
        ));
    }

    decisions
}

/// Extract the full decision surface from the assembled brief inputs: classify
/// the SOLID-3 candidates, anchor each `signal_id`, rank by consequence, cap to
/// the working-memory limit, collapse the rest, and drop suppressed decisions.
///
/// The emitted-signal-id allowlist is built over EVERY classified decision
/// (before the cap and before suppression drops), so `accept_signal_id` still
/// recognizes a collapsed-or-suppressed decision's anchor as fallow-emitted.
#[must_use]
pub fn extract_decision_surface(inputs: &DecisionInputs<'_>) -> DecisionSurface {
    let cap = inputs.cap.clamp(MIN_DECISION_CAP, MAX_DECISION_CAP);

    let mut classified = classify_candidates(inputs);

    // The allowlist: every signal_id the deterministic layer emitted.
    let emitted_signal_ids: Vec<String> = classified.iter().map(|d| d.signal_id.clone()).collect();

    // Drop suppressed decisions (suppression parity): a `// fallow-ignore` on the
    // anchor hides the decision. Done BEFORE the cap so a suppressed decision does
    // not consume a slot. The signal_id stays on the allowlist (anchor is still a
    // real fallow signal), so an agent re-proposing it is not "hallucinating".
    classified.retain(|d| {
        let source = (inputs.head_source)(&d.anchor_file);
        !is_decision_suppressed(source.as_deref(), d.category, d.anchor_line)
    });

    // Rank by consequence desc; stable, deterministic tiebreak on signal_id.
    classified.sort_by(|a, b| {
        b.consequence
            .cmp(&a.consequence)
            .then_with(|| a.signal_id.cmp(&b.signal_id))
    });

    let total = classified.len();
    let truncated = if total > cap {
        let collapsed = total - cap;
        classified.truncate(cap);
        Some(TruncationNote {
            collapsed,
            reason: format!(
                "{collapsed} more structural decision{} collapsed below the cap of {cap}",
                if collapsed == 1 { "" } else { "s" }
            ),
        })
    } else {
        None
    };

    DecisionSurface {
        decisions: classified,
        truncated,
        emitted_signal_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::routing::RoutingUnit;

    fn deltas(boundary: &[&str], public_api: &[&str]) -> ReviewDeltas {
        ReviewDeltas {
            boundary_introduced: boundary.iter().map(|s| (*s).to_string()).collect(),
            cycle_introduced: Vec::new(),
            public_api_added: public_api.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn no_source(_: &str) -> Option<String> {
        None
    }

    fn no_consumers(_: &str) -> u64 {
        0
    }

    fn inputs<'a>(
        deltas: &'a ReviewDeltas,
        boundary_anchors: &'a [BoundaryAnchor],
        coordination: &'a [CoordinationAnchor],
        routing: &'a RoutingFacts,
        head_source: &'a dyn Fn(&str) -> Option<String>,
        cap: usize,
    ) -> DecisionInputs<'a> {
        DecisionInputs {
            deltas,
            boundary_anchors,
            coordination,
            public_api_anchor_line: 0,
            affected_not_shown: 3,
            routing,
            head_source,
            rename_old_path: &no_source,
            internal_consumers: &no_consumers,
            cap,
        }
    }

    fn empty_routing() -> RoutingFacts {
        RoutingFacts::default()
    }

    // (d) None of the four cut categories can ever appear: the enum has exactly
    // three discriminants, so this is a compile-time + runtime guarantee.
    #[test]
    fn only_three_categories_exist_no_cut_category_representable() {
        let all = [
            DecisionCategory::CouplingBoundary,
            DecisionCategory::PublicApiContract,
            DecisionCategory::Dependency,
        ];
        assert_eq!(all.len(), 3);
        // Serialized tags never include a cut-category name.
        for c in all {
            let tag = c.tag();
            for cut in ["abstraction", "deletion", "convention", "irreversib"] {
                assert!(!tag.contains(cut), "cut category {cut} leaked into {tag}");
            }
        }
    }

    // (a) Every surfaced decision has a signal_id fallow emitted.
    #[test]
    fn every_decision_signal_id_resolves_to_an_emitted_candidate() {
        let d = deltas(&["ui->-db"], &["src/api.ts::Widget"]);
        let anchors = vec![BoundaryAnchor {
            zone_pair_key: "ui->-db".to_string(),
            from_file: "src/ui/page.ts".to_string(),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            line: 4,
        }];
        let routing = empty_routing();
        let surface = extract_decision_surface(&inputs(&d, &anchors, &[], &routing, &no_source, 4));
        assert!(!surface.decisions.is_empty());
        for decision in &surface.decisions {
            assert!(
                surface.accept_signal_id(&decision.signal_id),
                "decision {} has an unanchored signal_id",
                decision.question
            );
        }
    }

    // (b) An injected decision with no signal anchor is REJECTED.
    #[test]
    fn injected_unanchored_signal_id_is_rejected() {
        let d = deltas(&["ui->-db"], &[]);
        let anchors = vec![BoundaryAnchor {
            zone_pair_key: "ui->-db".to_string(),
            from_file: "src/ui/page.ts".to_string(),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            line: 1,
        }];
        let routing = empty_routing();
        let surface = extract_decision_surface(&inputs(&d, &anchors, &[], &routing, &no_source, 4));
        // A fabricated id the deterministic layer never emitted.
        assert!(!surface.accept_signal_id("sig:deadbeefdeadbeef"));
        assert!(!surface.accept_signal_id("sig:0000000000000000"));
        // The real one is accepted.
        let real = derive_signal_id(DecisionCategory::CouplingBoundary, "ui->-db");
        assert!(surface.accept_signal_id(&real));
    }

    // (c) A >cap input is capped to 4 plus/minus 1 with a truncation reason.
    #[test]
    fn over_cap_input_is_capped_with_truncation_reason() {
        // 6 boundary edges; default cap 4.
        let d = deltas(&["a->-x", "b->-x", "c->-x", "d->-x", "e->-x", "f->-x"], &[]);
        let routing = empty_routing();
        let surface = extract_decision_surface(&inputs(&d, &[], &[], &routing, &no_source, 4));
        assert_eq!(surface.decisions.len(), 4, "capped to default 4");
        let note = surface.truncated.expect("truncation note present");
        assert_eq!(note.collapsed, 2);
        assert!(note.reason.contains("collapsed"));
        assert!(note.reason.contains('2'));
    }

    #[test]
    fn cap_is_clamped_to_the_4_plus_minus_1_band() {
        let d = deltas(
            &[
                "a->-x", "b->-x", "c->-x", "d->-x", "e->-x", "f->-x", "g->-x",
            ],
            &[],
        );
        let routing = empty_routing();
        // cap=10 clamps to MAX (5).
        let high = extract_decision_surface(&inputs(&d, &[], &[], &routing, &no_source, 10));
        assert_eq!(high.decisions.len(), MAX_DECISION_CAP);
        // cap=1 clamps to MIN (3).
        let low = extract_decision_surface(&inputs(&d, &[], &[], &routing, &no_source, 1));
        assert_eq!(low.decisions.len(), MIN_DECISION_CAP);
    }

    // (e) A `// fallow-ignore` suppresses a flagged decision.
    #[test]
    fn fallow_ignore_suppresses_a_flagged_decision() {
        let d = deltas(&["ui->-db"], &[]);
        let anchors = vec![BoundaryAnchor {
            zone_pair_key: "ui->-db".to_string(),
            from_file: "src/ui/page.ts".to_string(),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            line: 3,
        }];
        let routing = empty_routing();

        // No suppression: one decision surfaces.
        let unsuppressed =
            extract_decision_surface(&inputs(&d, &anchors, &[], &routing, &no_source, 4));
        assert_eq!(unsuppressed.decisions.len(), 1);

        // File-level suppression hides it.
        let file_src = |f: &str| {
            (f == "src/ui/page.ts").then(|| {
                "// fallow-ignore-file decision-surface\nimport db from 'db';\n".to_string()
            })
        };
        let suppressed =
            extract_decision_surface(&inputs(&d, &anchors, &[], &routing, &file_src, 4));
        assert!(
            suppressed.decisions.is_empty(),
            "file-level ignore hides it"
        );
        // But the signal id stays on the allowlist (the anchor is still real).
        let id = derive_signal_id(DecisionCategory::CouplingBoundary, "ui->-db");
        assert!(suppressed.accept_signal_id(&id));

        // Line-level suppression immediately above the anchor line also hides it.
        let line_src = |f: &str| {
            (f == "src/ui/page.ts").then(|| {
                "line1\n// fallow-ignore-next-line decision-surface\nimport db from 'db';\n"
                    .to_string()
            })
        };
        let line_suppressed =
            extract_decision_surface(&inputs(&d, &anchors, &[], &routing, &line_src, 4));
        assert!(
            line_suppressed.decisions.is_empty(),
            "line-level ignore hides it"
        );
    }

    #[test]
    fn bare_blanket_ignore_suppresses_without_a_kind() {
        let d = deltas(&["ui->-db"], &[]);
        let anchors = vec![BoundaryAnchor {
            zone_pair_key: "ui->-db".to_string(),
            from_file: "src/ui/page.ts".to_string(),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            line: 2,
        }];
        let routing = empty_routing();
        let bare = |f: &str| {
            (f == "src/ui/page.ts")
                .then(|| "// fallow-ignore-next-line\nimport db from 'db';\n".to_string())
        };
        let surface = extract_decision_surface(&inputs(&d, &anchors, &[], &routing, &bare, 4));
        assert!(surface.decisions.is_empty(), "bare blanket ignore hides it");
    }

    #[test]
    fn unrelated_kind_ignore_does_not_suppress() {
        let d = deltas(&["ui->-db"], &[]);
        let anchors = vec![BoundaryAnchor {
            zone_pair_key: "ui->-db".to_string(),
            from_file: "src/ui/page.ts".to_string(),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            line: 2,
        }];
        let routing = empty_routing();
        let other = |f: &str| {
            (f == "src/ui/page.ts").then(|| {
                "// fallow-ignore-next-line unused-export\nimport db from 'db';\n".to_string()
            })
        };
        let surface = extract_decision_surface(&inputs(&d, &anchors, &[], &routing, &other, 4));
        assert_eq!(
            surface.decisions.len(),
            1,
            "an ignore naming a different kind must not suppress a decision"
        );
    }

    #[test]
    fn routed_expert_is_paired_with_a_decision() {
        let d = deltas(&["ui->-db"], &[]);
        let anchors = vec![BoundaryAnchor {
            zone_pair_key: "ui->-db".to_string(),
            from_file: "src/ui/page.ts".to_string(),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            line: 1,
        }];
        let routing = RoutingFacts {
            units: vec![RoutingUnit {
                file: "src/ui/page.ts".to_string(),
                expert: vec!["@team/ui".to_string()],
                bus_factor_one: true,
            }],
        };
        let surface = extract_decision_surface(&inputs(&d, &anchors, &[], &routing, &no_source, 4));
        assert_eq!(surface.decisions.len(), 1);
        assert_eq!(surface.decisions[0].expert, vec!["@team/ui".to_string()]);
        assert!(surface.decisions[0].bus_factor_one);
    }

    #[test]
    fn public_api_is_batch_consolidated_to_one_decision_r1() {
        // 111 added export keys collapse to ONE public-API decision (R1).
        let keys: Vec<String> = (0..111).map(|i| format!("src/ui/index.ts::C{i}")).collect();
        let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let d = deltas(&[], &key_refs);
        let routing = empty_routing();
        let surface = extract_decision_surface(&inputs(&d, &[], &[], &routing, &no_source, 4));
        let public_api_count = surface
            .decisions
            .iter()
            .filter(|dec| dec.category == DecisionCategory::PublicApiContract)
            .count();
        assert_eq!(
            public_api_count, 1,
            "R1: one public-API decision per change"
        );
        assert!(surface.decisions[0].question.contains("111"));
    }

    #[test]
    fn public_api_decision_carries_honest_consumer_count_and_tradeoff() {
        // A public-API delta whose anchor has 7 in-repo out-of-diff consumers must
        // surface that honest number on the decision AND name it as a fact in the
        // trade-off clause, distinct from the project-wide ranking proxy (`blast`).
        let d = deltas(&[], &["src/ui/index.ts::Widget"]);
        let routing = empty_routing();
        let seven = |_: &str| 7u64;
        let surface = extract_decision_surface(&DecisionInputs {
            deltas: &d,
            boundary_anchors: &[],
            coordination: &[],
            public_api_anchor_line: 0,
            // The project-wide proxy must NOT become the display number.
            affected_not_shown: 99,
            routing: &routing,
            head_source: &no_source,
            rename_old_path: &no_source,
            internal_consumers: &seven,
            cap: 4,
        });
        let dec = surface
            .decisions
            .iter()
            .find(|dec| dec.category == DecisionCategory::PublicApiContract)
            .expect("a public-API decision");
        assert_eq!(dec.internal_consumer_count, 7, "honest per-anchor count");
        assert_ne!(
            dec.internal_consumer_count, dec.blast,
            "display number must stay distinct from the ranking proxy"
        );
        assert!(
            dec.tradeoff.contains("7 in-repo"),
            "trade-off clause states the count as a fact: {}",
            dec.tradeoff
        );
        assert!(
            dec.question.ends_with('?'),
            "the decision stays a question (taste ownership)"
        );
    }

    #[test]
    fn coordination_gap_becomes_a_public_api_contract_decision() {
        let d = deltas(&[], &[]);
        let coordination = vec![CoordinationAnchor {
            changed_file: "src/core.ts".to_string(),
            consumed_symbols: vec!["compute".to_string()],
            consumer_count: 4,
            line: 7,
        }];
        let routing = empty_routing();
        let surface =
            extract_decision_surface(&inputs(&d, &[], &coordination, &routing, &no_source, 4));
        assert_eq!(surface.decisions.len(), 1);
        assert_eq!(
            surface.decisions[0].category,
            DecisionCategory::PublicApiContract
        );
        assert_eq!(surface.decisions[0].blast, 4);
        // The contract symbol's declaration line flows onto the decision so a PR
        // review can anchor an inline comment to the exact export.
        assert_eq!(surface.decisions[0].anchor_line, 7);
        // No rename in this change -> no previous_signal_id (the default).
        assert!(surface.decisions[0].previous_signal_id.is_none());
    }

    #[test]
    fn renamed_anchor_carries_a_previous_signal_id_for_review_memory() {
        // A coordination decision on a file renamed src/old.ts -> src/new.ts. The
        // signal_id keys on the NEW path; previous_signal_id keys on the OLD path,
        // so a cloud memory layer carries a prior dismissal across the `git mv`.
        let d = deltas(&[], &[]);
        let coordination = vec![CoordinationAnchor {
            changed_file: "src/new.ts".to_string(),
            consumed_symbols: vec!["compute".to_string()],
            consumer_count: 2,
            line: 0,
        }];
        let routing = empty_routing();
        let rename = |rel: &str| -> Option<String> {
            (rel == "src/new.ts").then(|| "src/old.ts".to_string())
        };
        let surface = extract_decision_surface(&DecisionInputs {
            deltas: &d,
            boundary_anchors: &[],
            coordination: &coordination,
            public_api_anchor_line: 0,
            affected_not_shown: 2,
            routing: &routing,
            head_source: &no_source,
            rename_old_path: &rename,
            internal_consumers: &no_consumers,
            cap: 4,
        });
        assert_eq!(surface.decisions.len(), 1);
        let decision = &surface.decisions[0];
        assert_eq!(
            decision.signal_id,
            derive_signal_id(DecisionCategory::PublicApiContract, "contract:src/new.ts")
        );
        assert_eq!(
            decision.previous_signal_id,
            Some(derive_signal_id(
                DecisionCategory::PublicApiContract,
                "contract:src/old.ts"
            ))
        );
    }

    #[test]
    fn signal_id_is_deterministic_and_namespaced_by_category() {
        let a = derive_signal_id(DecisionCategory::CouplingBoundary, "ui->-db");
        let b = derive_signal_id(DecisionCategory::CouplingBoundary, "ui->-db");
        assert_eq!(a, b, "deterministic");
        let c = derive_signal_id(DecisionCategory::PublicApiContract, "ui->-db");
        assert_ne!(a, c, "category namespaces the hash");
        assert!(a.starts_with("sig:"));
    }

    #[test]
    fn consequence_ranks_less_reversible_categories_higher() {
        // Same blast: dependency > public-api > coupling on reversibility weight.
        let dep = DecisionCategory::Dependency.reversibility_weight();
        let api = DecisionCategory::PublicApiContract.reversibility_weight();
        let coupling = DecisionCategory::CouplingBoundary.reversibility_weight();
        assert!(dep > api && api > coupling);
    }
}
