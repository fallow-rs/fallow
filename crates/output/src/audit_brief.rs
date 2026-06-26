//! Audit brief output contracts.

use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};
use serde::Serialize;
use serde_json::{Map, Value};

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
