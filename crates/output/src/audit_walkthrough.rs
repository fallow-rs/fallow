//! Review walkthrough output contracts.

use serde::{Deserialize, Serialize};

use crate::ReviewBriefSchemaVersion;

/// The standing injection-resistance note stamped on every guide.
pub const INJECTION_NOTE: &str = "The digest is built from the deterministic module graph only; PR prose is untrusted and never enters the digest. Your free-text framing is fenced as non-deterministic and never gates or auto-posts.";

/// One stable per-hunk CHANGE ANCHOR: a changed region the agent may cite as a
/// judgment anchor IN ADDITION to a `signal_id`. Where a `signal_id` anchors a
/// graph FINDING ("fallow emitted this exact finding"), a change_anchor anchors
/// only a changed REGION ("fallow confirms this region changed") , a strictly
/// weaker guarantee, surfaced as `anchor_kind` on the accepted judgment so a
/// consumer can tell the two apart. Graph/diff-derived; NEVER from prose.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    clippy::struct_field_names,
    reason = "change_anchor / previous_change_anchor are load-bearing wire keys"
)]
pub struct ChangeAnchor {
    /// Stable, CONTENT-addressed id: `chg:<16-hex>` over the file path + the
    /// normalized added text (line numbers are NOT hashed, so an edit above the
    /// hunk or a whitespace-only change does not move the id).
    pub change_anchor: String,
    /// Root-relative path of the changed file.
    pub file: String,
    /// 1-based first line of the hunk in the head file (display/deep-link only;
    /// NOT part of the id).
    pub start_line: u32,
    /// Number of added lines in the hunk (display only; NOT part of the id).
    pub line_count: u32,
    /// Rename-durable anchor: the id this same hunk would have had under the
    /// pre-rename path. `None` unless the file was renamed in this change, so an
    /// agent that cited the anchor before a `git mv` still resolves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_change_anchor: Option<String>,
}

/// One directed review unit projected from the graph: a file the change touches,
/// the concern to check, the out-of-diff consumers it must account for, and the
/// routed expert. Graph-derived only (routing + impact closure), NEVER from prose.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DirectionUnit {
    /// Root-relative path of the unit to review.
    pub file: String,
    /// The concern lens the agent should check for this unit, derived from the
    /// unit's risk signals (impact-closure consumers vs a plain touched file).
    pub concern_lens: String,
    /// Per-unit review-effort budget: the weighted-focus composite score for
    /// this file. A cloud fan-out spends AI passes/verifiers PROPORTIONAL to this
    /// (higher = review harder); a local single-agent loop can ignore it.
    pub scoring_budget: u32,
    /// Root-relative paths of modules affected by this unit but NOT in the diff
    /// (the out-of-diff context the agent must reason about).
    pub out_of_diff: Vec<String>,
    /// Routed expert(s), when ownership signals are available.
    pub expert: Vec<String>,
}

/// The review direction artifact: the order to review in, the coherent units,
/// and per-unit concern lens + out-of-diff + expert. A minimal projection of the
/// EXISTING graph facts (routing units + impact closure); the full weighted-focus
/// engine is a later epic. Graph-derived only (injection-resistant).
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewDirection {
    /// The dependency-sensible review order: unit file paths, units carrying
    /// out-of-diff consumers first (review the load-bearing definitions before
    /// the mechanical units).
    pub order: Vec<String>,
    /// Coherent review units, in `order`.
    pub units: Vec<DirectionUnit>,
}

/// The shape the agent must return, embedded in the guide so a thin skill needs
/// no frozen copy. Documents the anchoring + staleness contract in the wire.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AgentSchema {
    /// How the agent must structure each judgment: cite an emitted `signal_id`,
    /// add free-text `framing` (non-deterministic, fenced), an optional `concern`.
    pub judgment_shape: &'static str,
    /// The agent MUST echo this `graph_snapshot_hash` back in its JSON; a
    /// mismatch on reentry REFUSES the payload as stale.
    pub echo_field: &'static str,
    /// The anchoring rule name.
    pub anchoring_rule: &'static str,
}

/// The default agent schema descriptor.
#[must_use]
pub const fn agent_schema() -> AgentSchema {
    AgentSchema {
        judgment_shape: "Return { \"graph_snapshot_hash\": <echoed>, \"judgments\": [ { \"signal_id\": <one fallow emitted, OR omit and use change_anchor>, \"change_anchor\": <one fallow emitted chg: id, for a changed region with no finding>, \"framing\": <free text>, \"concern\": <optional> } ] }.",
        echo_field: "graph_snapshot_hash",
        anchoring_rule: "Every judgment must cite an emitted signal_id OR an emitted change_anchor; an unanchored id is rejected (anti-hallucination). A change_anchor proves only that the region changed (anchor_kind=change), a weaker guarantee than a signal_id finding (anchor_kind=signal).",
    }
}

/// The `fallow review --walkthrough-guide` envelope: the current digest + schema
/// the agent fetches. The tool owns this; the skill stays thin (it fetches this
/// rather than embedding a frozen copy). Always emitted with exit 0.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow review --walkthrough-guide --format json")
)]
pub struct WalkthroughGuide<Digest> {
    /// Pinned to the brief schema version (the spec versions the guide by
    /// `review_brief_schema_version`).
    pub schema_version: ReviewBriefSchemaVersion,
    /// Fallow CLI version that produced this guide.
    pub version: String,
    /// Command discriminator singleton: always `"review-walkthrough-guide"`.
    pub command: String,
    /// The deterministic graph-snapshot hash pinned into the digest. The agent
    /// echoes it back; a mismatch on reentry refuses the payload as stale.
    pub graph_snapshot_hash: String,
    /// The graph-derived digest (brief + decision surface). Pure over the tree.
    pub digest: Digest,
    /// The review direction (order/units/concern-lens/out-of-diff/expert).
    pub direction: ReviewDirection,
    /// The per-hunk change anchors: one stable id per changed region. An agent
    /// may cite a `change_anchor` as a judgment anchor in addition to an emitted
    /// `signal_id`, so a trade-off about a changed region with no graph finding
    /// can still anchor (and be post-validated) rather than hallucinate.
    pub change_anchors: Vec<ChangeAnchor>,
    /// The JSON shape the agent must return, embedded so the skill stays thin.
    pub agent_schema: AgentSchema,
    /// The injection-resistance note (digest is graph-only; PR prose untrusted).
    pub injection_note: &'static str,
}

/// The standard walkthrough guide shape emitted by `fallow review`.
pub type StandardWalkthroughGuide = WalkthroughGuide<crate::audit_brief::StandardReviewBriefOutput>;

/// The agent's returned judgment JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentWalkthrough {
    /// Echoed graph-snapshot hash.
    #[serde(default)]
    pub graph_snapshot_hash: String,
    /// The agent's per-signal judgments.
    #[serde(default)]
    pub judgments: Vec<AgentJudgment>,
}

/// One agent judgment.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentJudgment {
    /// The fallow-emitted `signal_id` this judgment frames.
    #[serde(default)]
    pub signal_id: String,
    /// The fallow-emitted `change_anchor` this judgment frames.
    #[serde(default)]
    pub change_anchor: String,
    /// The agent's free-text framing.
    #[serde(default)]
    pub framing: String,
    /// The agent's optional concern category.
    #[serde(default)]
    pub concern: Option<String>,
}

/// One accepted judgment: the real anchored signal passed through with the
/// agent's framing FENCED as non-deterministic.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AcceptedJudgment {
    /// The fallow-emitted `signal_id` (verified against the allowlist). Empty
    /// when this judgment was anchored by a `change_anchor` instead.
    pub signal_id: String,
    /// The fallow-emitted `change_anchor` (verified against the allowlist). Empty
    /// when this judgment was anchored by a `signal_id`.
    pub change_anchor: String,
    /// Which anchor resolved: `"signal"` (a graph FINDING, the strong anchor) or
    /// `"change"` (a changed REGION only, the weaker anchor). Lets a consumer
    /// distinguish a finding-anchored judgment from a region-anchored one rather
    /// than collapsing both into one accepted bucket.
    pub anchor_kind: String,
    /// The agent's fenced free-text framing.
    pub agent_framing: String,
    /// The agent's optional concern category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concern: Option<String>,
    /// Hard fence: always `false`. The framing is agent prose, never a
    /// deterministic fallow result, so it never gates or auto-posts.
    pub deterministic: bool,
}

/// One rejected judgment plus the reason it was rejected.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RejectedJudgment {
    /// The `signal_id` the agent cited (fallow never emitted it). Empty when the
    /// judgment cited a `change_anchor` instead.
    pub signal_id: String,
    /// The `change_anchor` the agent cited (fallow never emitted it). Empty when
    /// the judgment cited a `signal_id` instead.
    pub change_anchor: String,
    /// The rejection reason: `unanchored-signal-id` (cited a signal fallow did
    /// not emit), `unknown-change-anchor` (cited a region fallow did not emit),
    /// or `stale-snapshot` (the tree moved).
    pub reason: String,
}

/// The `fallow review --walkthrough-file` validation envelope: the result of
/// post-validating the agent's judgment against the live graph. Always exit 0.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow review --walkthrough-file --format json")
)]
pub struct WalkthroughValidation {
    /// Pinned to the brief schema version.
    pub schema_version: ReviewBriefSchemaVersion,
    /// Fallow CLI version that produced this validation.
    pub version: String,
    /// Command discriminator singleton: always `"review-walkthrough-validation"`.
    pub command: String,
    /// The current run's deterministic graph-snapshot hash.
    pub graph_snapshot_hash: String,
    /// `true` when the agent's echoed hash != the current hash (the tree moved):
    /// the WHOLE payload is refused, `accepted` is empty.
    pub stale: bool,
    /// Judgments that cite a real fallow-emitted signal, framing fenced.
    pub accepted: Vec<AcceptedJudgment>,
    /// Judgments rejected (unanchored signal id, or all-rejected when stale).
    pub rejected: Vec<RejectedJudgment>,
    /// Count of accepted judgments.
    pub accepted_count: usize,
    /// Count of rejected judgments.
    pub rejected_count: usize,
    /// Count of accepted judgments whose `signal_id` resolved against the live
    /// allowlist. Zero unanchored when this equals `accepted_count` and there are
    /// no rejections (the clean done-condition).
    pub unanchored_count: usize,
}
