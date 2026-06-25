//! Explain and inspect output envelopes.

use serde::Serialize;

/// Envelope emitted by `fallow explain <issue-type> --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow explain <issue-type> --format json")
)]
pub struct ExplainOutput {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub rationale: String,
    pub example: String,
    pub how_to_fix: String,
    pub docs: String,
}

/// Envelope emitted by `fallow inspect --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow inspect --format json"))]
pub struct InspectOutput {
    pub target: InspectTargetDescriptor,
    pub identity: InspectIdentity,
    pub evidence: InspectEvidence,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InspectTargetDescriptor {
    File { file: String },
    Symbol { file: String, export_name: String },
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum InspectIdentity {
    File(InspectFileIdentity),
    Symbol(InspectSymbolIdentity),
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InspectFileIdentity {
    pub file: String,
    pub is_reachable: Option<serde_json::Value>,
    pub is_entry_point: Option<serde_json::Value>,
    pub export_count: Option<usize>,
    pub import_count: Option<usize>,
    pub imported_by_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InspectSymbolIdentity {
    pub file: String,
    pub export_name: String,
    pub file_reachable: Option<serde_json::Value>,
    pub is_entry_point: Option<serde_json::Value>,
    pub is_used: Option<serde_json::Value>,
    pub reason: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InspectEvidence {
    pub trace_file: InspectEvidenceSection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_export: Option<InspectEvidenceSection>,
    pub dead_code: InspectEvidenceSection,
    pub duplication: InspectEvidenceSection,
    pub complexity: InspectEvidenceSection,
    pub security: InspectEvidenceSection,
    /// Impact closure scoped to the inspected file as the seed.
    pub impact_closure: InspectEvidenceSection,
    /// Optional symbol-level call chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_chain: Option<InspectEvidenceSection>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InspectEvidenceSection {
    pub status: InspectSectionStatus,
    pub scope: InspectEvidenceScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl InspectEvidenceSection {
    #[must_use]
    pub fn ok(scope: InspectEvidenceScope, data: serde_json::Value) -> Self {
        Self {
            status: InspectSectionStatus::Ok,
            scope,
            message: None,
            data: Some(data),
        }
    }

    #[must_use]
    pub fn error(scope: InspectEvidenceScope, message: String) -> Self {
        Self {
            status: InspectSectionStatus::Error,
            scope,
            message: Some(message),
            data: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum InspectSectionStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum InspectEvidenceScope {
    Symbol,
    File,
    ProjectFilteredToFile,
}
