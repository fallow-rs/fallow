//! Audit brief output contracts.

use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};
use serde::Serialize;
use serde_json::{Map, Value};

/// Wire version for the `fallow audit --brief --format json` envelope.
pub const REVIEW_BRIEF_SCHEMA_VERSION: u32 = 5;

/// Independently-versioned wire-version newtype for the brief envelope.
/// Serializes as the integer `REVIEW_BRIEF_SCHEMA_VERSION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewBriefSchemaVersion(pub u32);

impl Default for ReviewBriefSchemaVersion {
    fn default() -> Self {
        Self(REVIEW_BRIEF_SCHEMA_VERSION)
    }
}

/// Coarse risk classification for a changeset, a pure function of the change
/// size (file count plus, once threaded, net lines).
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

/// Suggested reviewer effort, a pure function of [`RiskClass`].
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

/// Stage 0 of the brief: triage facts derived purely from the diff size.
///
/// `hunks` and `net_lines` are `None` in v1: the file-level audit does not yet
/// thread a `DiffIndex` (from `report/ci/diff_filter.rs`). They populate later,
/// on `--diff-file` / `--diff-stdin`, without a schema bump.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffTriage {
    /// Number of changed files in the audit scope.
    pub files: usize,
    /// Number of diff hunks. `None` in v1 (no diff index threaded yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunks: Option<usize>,
    /// Net added-minus-removed lines. `None` in v1 (no diff index threaded yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_lines: Option<i64>,
    /// Coarse risk class derived from the change size.
    pub risk_class: RiskClass,
    /// Suggested reviewer effort derived from `risk_class`.
    pub review_effort: ReviewEffort,
}

/// Stage 1 of the brief: graph-derived orientation facts.
///
/// `boundaries_touched` is derived from the run's boundary-violation zones;
/// `reachable_from` is populated by the impact closure (the affected-not-shown
/// set: modules the changed code is reachable from / affects, none in the diff).
/// `exports_added` / `api_width_delta` stay honestly stubbed (`0`) until the
/// export-surface delta lands. The fields are present and correctly typed so
/// values fill in later without a schema bump.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GraphFacts {
    /// Number of exports added by the changeset. Stubbed to `0` in v1.
    pub exports_added: usize,
    /// Change in public API width (added minus removed exports). Stubbed to `0`
    /// in v1.
    pub api_width_delta: i64,
    /// Root-relative paths of modules the changed code is reachable from / affects
    /// (the impact closure's affected-but-not-in-diff set), deduped and sorted.
    /// Empty when no graph was retained or nothing depends on the changed files.
    pub reachable_from: Vec<String>,
    /// Architecture boundary zones touched by the changeset, deduped and sorted.
    /// Derived from the run's boundary-violation findings.
    pub boundaries_touched: Vec<String>,
}

/// Stage 3 of the brief: the impact closure. The transitive
/// affected-but-not-in-diff set plus the coordination gap. The differentiator a
/// diff tool fundamentally cannot do, because it has no graph.
///
/// Honest scope (ADR-001, syntactic): the coordination gap is an attention
/// pointer at the exact inter-module failure mode, NOT a correctness proof.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImpactClosureFacts {
    /// Root-relative paths transitively affected by the changeset (reverse-deps +
    /// re-export chains) that are NOT in the diff, deduped and sorted.
    pub affected_not_shown: Vec<String>,
    /// Coordination gaps: a changed file exports a contract consumed by a module
    /// absent from the diff. One entry per (changed file, consumer) pair.
    pub coordination_gap: Vec<CoordinationGapFact>,
}

/// One coordination-gap entry: a changed file exports symbols consumed by a
/// `consumer_file` that is NOT in the diff. Deduped per (changed, consumer) pair
/// (firing-precision rule R2).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoordinationGapFact {
    /// Root-relative path of the changed file whose contract is consumed elsewhere.
    pub changed_file: String,
    /// Root-relative path of the consumer module that is NOT in the diff.
    pub consumer_file: String,
    /// The exported symbol names the consumer references, sorted.
    pub consumed_symbols: Vec<String>,
    /// Honest scope note: this is a syntactic attention pointer, not a proof.
    pub note: String,
}

/// Stage 2 of the brief: the partition + order. The changed files split into
/// coherent BY-MODULE units (the only byte-identical-deterministic clustering
/// definition straight from the graph), plus a dependency-sensible review ORDER
/// over those units (definitions before consumers, mechanical/leaf units last,
/// ties broken by the path sort). Stage 2 sits UNDER the decision surface as a
/// drill-down; it is the backbone the directed-review loop hands the agent.
///
/// Feature-cluster and concern partitioning are deferred (they need scoring
/// heuristics whose tie-breaks are a fresh nondeterminism surface).
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PartitionFacts {
    /// The by-module units, sorted by module directory. Empty when no graph was
    /// retained or no changed file maps to a known module.
    pub units: Vec<ReviewUnitFact>,
    /// The dependency-sensible review order: module-directory strings,
    /// definitions before consumers, mechanical/leaf units last. A permutation of
    /// the `units` module directories.
    pub order: Vec<String>,
}

/// One review unit: a coherent by-module cluster of the changed set.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewUnitFact {
    /// The module directory the unit covers (root-relative, forward-slashed).
    /// The empty string is the repository-root group.
    pub module_dir: String,
    /// The changed files in this unit, path-sorted.
    pub files: Vec<String>,
}

/// Diff-aware deterministic deltas (6.A), framed new-vs-pre-existing against
/// the audit base snapshot. Each entry is a brief summary/verdict line.
///
/// `public_api` is batch-consolidated to ONE decision per change (rule R1):
/// the `added` list carries the introduced public-export keys as evidence, but a
/// reviewer reads "the public surface widened by N", never one decision per
/// symbol.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewDeltas {
    /// Cross-zone boundary EDGES introduced vs base (R2 first-edge-only: one per
    /// `<from_zone>-><to_zone>` pair, never per import). New-vs-pre-existing.
    pub boundary_introduced: Vec<String>,
    /// Circular dependencies introduced vs base (canonical file-set keys).
    pub cycle_introduced: Vec<String>,
    /// Exports-aware public-API surface delta: the public-export keys
    /// (`<rel_path>::<name>`) added vs base, resolved through `package.json`
    /// `exports` + re-export reachability. A symbol re-exported only through an
    /// internal barrel NOT in `exports` is absent here (zero delta); one
    /// reachable through an `exports` path is present (exactly one).
    pub public_api_added: Vec<String>,
}

/// The full `fallow audit --brief --format json` envelope. Carries the
/// informational verdict, the triage and graph-facts orientation stages, plus
/// the reused "subtract" section (the same dead-code / duplication / complexity
/// payload `fallow audit --format json` emits).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow audit --brief --format json")
)]
pub struct ReviewBriefOutput<Focus, Weakening, Routing, Decisions> {
    /// Independently-versioned brief schema version.
    pub schema_version: ReviewBriefSchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: String,
    /// Command discriminator singleton: always `"audit-brief"`.
    pub command: String,
    pub triage: DiffTriage,
    /// Stage 1: graph orientation facts.
    pub graph_facts: GraphFacts,
    /// Stage 2: the partition + order (by-module units + dependency-sensible
    /// review order). The backbone the directed-review loop hands the agent.
    pub partition: PartitionFacts,
    /// Stage 3: the impact closure (affected-not-shown + coordination gap).
    pub impact_closure: ImpactClosureFacts,
    /// Stage 4: the weighted focus map. A composite attention score per
    /// changed-file unit (fan-in/out + security taint + risk zone + change shape),
    /// with `review-here` / `not-prioritized` labels (NEVER `skip` in free mode),
    /// a per-unit confidence flag, and the FULL `deprioritized` escape-hatch list
    /// so every de-prioritized piece is reachable. Stage 4 sits UNDER the decision
    /// surface as drill-down.
    pub focus: Focus,
    /// 6.A: diff-aware deterministic deltas (boundary/cycle introduced +
    /// exports-aware public-API surface delta), new-vs-pre-existing.
    pub deltas: ReviewDeltas,
    /// 6.F, headline: reviewer-private weakening signals (tests
    /// removed/skipped, thresholds lowered, suppressions added, security steps
    /// removed). Advisory, never gates, never auto-posted.
    pub weakening: Vec<Weakening>,
    /// 6.D: ownership-aware reviewer routing (per-file expert + bus-factor).
    pub routing: Routing,
    /// 6.G, the APEX: the decision surface. The ranked, capped,
    /// signal_id-anchored set of consequential structural decisions, each framed
    /// as a judgment question with its routed expert. This is the only thing the
    /// brief visibly leads with; the stages above are its drill-down derivation.
    pub decisions: Decisions,
}

/// The standard audit brief payload shape used by the CLI, schema emitter,
/// API, and agent-facing review surfaces.
pub type StandardReviewBriefOutput = ReviewBriefOutput<
    crate::audit_focus::FocusMap,
    crate::audit_weakening::WeakeningSignal,
    crate::audit_routing::RoutingFacts,
    crate::audit_decision_surface::DecisionSurface,
>;

/// CLI-built audit subreports that are embedded in the audit brief envelope.
///
/// The brief envelope and field ordering belong to `fallow-output`; the
/// underlying subreport payloads are still supplied by the CLI until their
/// builders are fully command-neutral.
#[derive(Debug, Clone, Default)]
pub struct ReviewBriefSubtractSections {
    pub dead_code: Option<Value>,
    pub duplication: Option<Value>,
    pub complexity: Option<Value>,
}

fn insert_serialized<T: Serialize>(
    obj: &mut Map<String, Value>,
    key: &'static str,
    value: &T,
) -> Result<(), serde_json::Error> {
    obj.insert(key.to_string(), serde_json::to_value(value)?);
    Ok(())
}

/// Build the complete `fallow audit --brief --format json` value.
///
/// `audit_header` carries informational audit scope fields such as verdict,
/// base ref, summary, and attribution. This function restamps the independent
/// brief schema and command after merging that header so the resulting document
/// advertises the brief contract rather than the regular audit JSON contract.
pub fn build_review_brief_json_output<Focus, Weakening, Routing, Decisions>(
    brief: &ReviewBriefOutput<Focus, Weakening, Routing, Decisions>,
    audit_header: Map<String, Value>,
    subtract: ReviewBriefSubtractSections,
) -> Result<Value, serde_json::Error>
where
    Focus: Serialize,
    Weakening: Serialize,
    Routing: Serialize,
    Decisions: Serialize,
{
    let mut obj = Map::new();

    insert_serialized(&mut obj, "schema_version", &brief.schema_version)?;
    obj.insert("version".into(), Value::String(brief.version.clone()));
    obj.insert("command".into(), Value::String(brief.command.clone()));

    for (key, value) in audit_header {
        obj.insert(key, value);
    }

    insert_serialized(&mut obj, "schema_version", &brief.schema_version)?;
    obj.insert("command".into(), Value::String(brief.command.clone()));

    insert_serialized(&mut obj, "decisions", &brief.decisions)?;
    insert_serialized(&mut obj, "triage", &brief.triage)?;
    insert_serialized(&mut obj, "graph_facts", &brief.graph_facts)?;
    insert_serialized(&mut obj, "partition", &brief.partition)?;
    insert_serialized(&mut obj, "impact_closure", &brief.impact_closure)?;
    insert_serialized(&mut obj, "focus", &brief.focus)?;
    insert_serialized(&mut obj, "deltas", &brief.deltas)?;
    insert_serialized(&mut obj, "weakening", &brief.weakening)?;
    insert_serialized(&mut obj, "routing", &brief.routing)?;

    if let Some(value) = subtract.dead_code {
        obj.insert("dead_code".into(), value);
    }
    if let Some(value) = subtract.duplication {
        obj.insert("duplication".into(), value);
    }
    if let Some(value) = subtract.complexity {
        obj.insert("complexity".into(), value);
    }

    Ok(Value::Object(obj))
}

fn serialize_agent_contract_json_output<T: Serialize>(
    output: T,
    kind: &'static str,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, kind, mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Serialize the `fallow audit --brief --format json` envelope.
///
/// # Errors
///
/// Returns a serde error when the brief output cannot be converted to JSON.
pub fn serialize_review_brief_json_output<T: Serialize>(
    output: T,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<Value, serde_json::Error> {
    serialize_agent_contract_json_output(output, "audit-brief", mode, analysis_run_id)
}

/// Serialize the standalone decision-surface envelope.
///
/// # Errors
///
/// Returns a serde error when the decision-surface output cannot be converted
/// to JSON.
pub fn serialize_decision_surface_json_output<T: Serialize>(
    output: T,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<Value, serde_json::Error> {
    serialize_agent_contract_json_output(output, "decision-surface", mode, analysis_run_id)
}

/// Serialize the review walkthrough guide envelope.
///
/// # Errors
///
/// Returns a serde error when the walkthrough guide cannot be converted to
/// JSON.
pub fn serialize_walkthrough_guide_json_output<T: Serialize>(
    output: T,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<Value, serde_json::Error> {
    serialize_agent_contract_json_output(output, "review-walkthrough-guide", mode, analysis_run_id)
}

/// Serialize the review walkthrough validation envelope.
///
/// # Errors
///
/// Returns a serde error when the walkthrough validation cannot be converted
/// to JSON.
pub fn serialize_walkthrough_validation_json_output<T: Serialize>(
    output: T,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<Value, serde_json::Error> {
    serialize_agent_contract_json_output(
        output,
        "review-walkthrough-validation",
        mode,
        analysis_run_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn review_brief_json_output_restamps_audit_header_contract() {
        let brief = ReviewBriefOutput {
            schema_version: ReviewBriefSchemaVersion::default(),
            version: "1.2.3".to_string(),
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
                reachable_from: Vec::new(),
                boundaries_touched: Vec::new(),
            },
            partition: PartitionFacts::default(),
            impact_closure: ImpactClosureFacts::default(),
            focus: json!({"units": []}),
            deltas: ReviewDeltas::default(),
            weakening: Vec::<Value>::new(),
            routing: json!({"units": []}),
            decisions: json!({"decisions": []}),
        };
        let mut audit_header = Map::new();
        audit_header.insert("schema_version".into(), json!(999));
        audit_header.insert("command".into(), json!("audit"));
        audit_header.insert("verdict".into(), json!("fail"));

        let value = build_review_brief_json_output(
            &brief,
            audit_header,
            ReviewBriefSubtractSections {
                dead_code: Some(json!({"issues": []})),
                duplication: None,
                complexity: None,
            },
        )
        .expect("brief output should serialize");

        assert_eq!(value["schema_version"], REVIEW_BRIEF_SCHEMA_VERSION);
        assert_eq!(value["command"], "audit-brief");
        assert_eq!(value["verdict"], "fail");
        assert_eq!(value["dead_code"]["issues"], json!([]));
    }

    #[test]
    fn review_brief_serializer_owns_root_contract() {
        let value = serialize_review_brief_json_output(
            json!({"command": "audit-brief"}),
            RootEnvelopeMode::Tagged,
            Some("run-brief"),
        )
        .expect("brief output should serialize");

        assert_eq!(value["kind"], "audit-brief");
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-brief");
    }

    #[test]
    fn decision_surface_serializer_owns_root_contract() {
        let value = serialize_decision_surface_json_output(
            json!({"decisions": []}),
            RootEnvelopeMode::Tagged,
            Some("run-decision"),
        )
        .expect("decision surface should serialize");

        assert_eq!(value["kind"], "decision-surface");
        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"],
            "run-decision"
        );
    }
}
