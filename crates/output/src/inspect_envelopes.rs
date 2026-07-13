//! Explain and inspect output envelopes.

use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};
use serde::Serialize;

/// Envelope emitted by `fallow explain <issue-type> --format json`.
///
/// Standalone rule explanation. This command does not run project analysis
/// and intentionally returns a compact object without `schema_version` /
/// `version` metadata; consumers that need those should call any other
/// fallow JSON-producing command.
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

/// Serialize the `fallow explain --format json` envelope.
///
/// # Errors
///
/// Returns a serde error when the explain output cannot be converted to JSON.
pub fn serialize_explain_json_output(
    output: ExplainOutput,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, "explain", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
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
    /// Impact closure scoped to the inspected file as the seed: the transitive
    /// affected-but-not-in-diff set + coordination gap.
    pub impact_closure: InspectEvidenceSection,
    /// OPT-IN target-level git churn. Omitted unless historical evidence was
    /// explicitly requested by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub churn: Option<InspectEvidenceSection>,
    /// OPT-IN symbol-level call chain. Present only when `--symbol-chain` was
    /// requested AND the target is a SYMBOL (best-effort, syntactic, OFF the
    /// ranked path). `None` (omitted) by default: symbol-level chains are
    /// best-effort and not part of the trusted ranked evidence.
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

    #[must_use]
    pub fn unavailable(scope: InspectEvidenceScope, message: String) -> Self {
        Self {
            status: InspectSectionStatus::Unavailable,
            scope,
            message: Some(message),
            data: None,
        }
    }
}

/// Serialize the `fallow inspect --format json` envelope.
///
/// # Errors
///
/// Returns a serde error when the inspect output cannot be converted to JSON.
pub fn serialize_inspect_json_output(
    output: InspectOutput,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, "inspect_target", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum InspectSectionStatus {
    Ok,
    Unavailable,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_json_output_uses_output_owned_root_contract() {
        let output = ExplainOutput {
            id: "unused-export".to_string(),
            name: "Unused export".to_string(),
            summary: "summary".to_string(),
            rationale: "rationale".to_string(),
            example: "example".to_string(),
            how_to_fix: "fix".to_string(),
            docs: "https://example.test".to_string(),
        };

        let value =
            serialize_explain_json_output(output, RootEnvelopeMode::Tagged, Some("run-explain"))
                .expect("explain output should serialize");

        assert_eq!(value["kind"], "explain");
        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"],
            "run-explain"
        );
    }

    #[test]
    fn inspect_json_output_uses_output_owned_root_contract() {
        let output = InspectOutput {
            target: InspectTargetDescriptor::File {
                file: "src/app.ts".to_string(),
            },
            identity: InspectIdentity::File(InspectFileIdentity {
                file: "src/app.ts".to_string(),
                is_reachable: None,
                is_entry_point: None,
                export_count: Some(0),
                import_count: Some(0),
                imported_by_count: Some(0),
            }),
            evidence: InspectEvidence {
                trace_file: InspectEvidenceSection::ok(
                    InspectEvidenceScope::File,
                    serde_json::json!({}),
                ),
                trace_export: None,
                dead_code: InspectEvidenceSection::error(
                    InspectEvidenceScope::File,
                    "not run".to_string(),
                ),
                duplication: InspectEvidenceSection::error(
                    InspectEvidenceScope::ProjectFilteredToFile,
                    "not run".to_string(),
                ),
                complexity: InspectEvidenceSection::error(
                    InspectEvidenceScope::ProjectFilteredToFile,
                    "not run".to_string(),
                ),
                security: InspectEvidenceSection::error(
                    InspectEvidenceScope::File,
                    "not run".to_string(),
                ),
                impact_closure: InspectEvidenceSection::error(
                    InspectEvidenceScope::ProjectFilteredToFile,
                    "not run".to_string(),
                ),
                churn: None,
                symbol_chain: None,
            },
            warnings: Vec::new(),
        };

        let value =
            serialize_inspect_json_output(output, RootEnvelopeMode::Tagged, Some("run-inspect"))
                .expect("inspect output should serialize");

        assert_eq!(value["kind"], "inspect_target");
        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"],
            "run-inspect"
        );
        assert!(value["evidence"].get("churn").is_none());
    }

    #[test]
    fn inspect_churn_section_serializes_success_unavailable_and_error_states() {
        let ok = InspectEvidenceSection::ok(
            InspectEvidenceScope::ProjectFilteredToFile,
            serde_json::json!({"file": "src/app.ts", "commits": 4}),
        );
        let unavailable = InspectEvidenceSection::unavailable(
            InspectEvidenceScope::ProjectFilteredToFile,
            "git repository unavailable".to_string(),
        );
        let error = InspectEvidenceSection::error(
            InspectEvidenceScope::ProjectFilteredToFile,
            "git log failed".to_string(),
        );

        assert_eq!(
            serde_json::to_value(ok).expect("ok section should serialize")["status"],
            "ok"
        );
        assert_eq!(
            serde_json::to_value(unavailable).expect("unavailable section should serialize")["status"],
            "unavailable"
        );
        assert_eq!(
            serde_json::to_value(error).expect("error section should serialize")["status"],
            "error"
        );
    }
}
