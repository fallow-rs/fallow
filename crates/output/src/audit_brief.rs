//! Audit brief output contracts.

use serde::Serialize;

/// Wire version for the `fallow audit --brief --format json` envelope.
pub const REVIEW_BRIEF_SCHEMA_VERSION: u32 = 5;

/// Independently-versioned wire-version newtype for the brief envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewBriefSchemaVersion(pub u32);

impl Default for ReviewBriefSchemaVersion {
    fn default() -> Self {
        Self(REVIEW_BRIEF_SCHEMA_VERSION)
    }
}

/// Coarse risk classification for a changeset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    /// Small, contained change.
    Low,
    /// Moderately sized change.
    Medium,
    /// Large change spanning many files or lines.
    High,
}

/// Suggested reviewer effort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReviewEffort {
    /// A quick scan is enough.
    Glance,
    /// A normal line-by-line review.
    Review,
    /// A careful, deep review is warranted.
    DeepDive,
}

/// Stage 0 of the brief: triage facts derived from diff size.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffTriage {
    pub files: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunks: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_lines: Option<i64>,
    pub risk_class: RiskClass,
    pub review_effort: ReviewEffort,
}

/// Stage 1 of the brief: graph-derived orientation facts.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GraphFacts {
    pub exports_added: usize,
    pub api_width_delta: i64,
    pub reachable_from: Vec<String>,
    pub boundaries_touched: Vec<String>,
}

/// Stage 3 of the brief: the impact closure.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImpactClosureFacts {
    pub affected_not_shown: Vec<String>,
    pub coordination_gap: Vec<CoordinationGapFact>,
}

/// One coordination-gap entry.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoordinationGapFact {
    pub changed_file: String,
    pub consumer_file: String,
    pub consumed_symbols: Vec<String>,
    pub note: String,
}

/// Stage 2 of the brief: the partition and review order.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PartitionFacts {
    pub units: Vec<ReviewUnitFact>,
    pub order: Vec<String>,
}

/// One review unit: a coherent by-module cluster of the changed set.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewUnitFact {
    pub module_dir: String,
    pub files: Vec<String>,
}

/// Diff-aware deterministic deltas, framed new-vs-pre-existing.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewDeltas {
    pub boundary_introduced: Vec<String>,
    pub cycle_introduced: Vec<String>,
    pub public_api_added: Vec<String>,
}

/// The full `fallow audit --brief --format json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow audit --brief --format json")
)]
pub struct ReviewBriefOutput<Focus, Weakening, Routing, Decisions> {
    pub schema_version: ReviewBriefSchemaVersion,
    pub version: String,
    pub command: String,
    pub triage: DiffTriage,
    pub graph_facts: GraphFacts,
    pub partition: PartitionFacts,
    pub impact_closure: ImpactClosureFacts,
    pub focus: Focus,
    pub deltas: ReviewDeltas,
    pub weakening: Vec<Weakening>,
    pub routing: Routing,
    pub decisions: Decisions,
}
