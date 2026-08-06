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
    /// Issue-type identifier, e.g. `unused-export`.
    pub id: String,
    /// Human-readable issue-type name.
    pub name: String,
    /// One-line description of what the issue type reports.
    pub summary: String,
    /// Why the issue matters.
    pub rationale: String,
    /// Illustrative code example of the issue.
    pub example: String,
    /// How to resolve findings of this type.
    pub how_to_fix: String,
    /// Public documentation URL for the issue type.
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
    /// What was inspected: a file or a specific exported symbol.
    pub target: InspectTargetDescriptor,
    /// Graph identity facts about the target.
    pub identity: InspectIdentity,
    /// Per-analysis evidence sections for the target.
    pub evidence: InspectEvidence,
    /// Non-fatal problems encountered while gathering evidence.
    pub warnings: Vec<String>,
    /// `_meta` block with type-aware backend info, when applicable.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<fallow_types::envelope::Meta>,
}

/// `target` block of [`InspectOutput`], tagged by `type`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InspectTargetDescriptor {
    /// A whole file was inspected.
    File {
        /// File path relative to the analysed root.
        file: String,
    },
    /// One exported symbol was inspected.
    Symbol {
        /// File path relative to the analysed root.
        file: String,
        /// Name of the inspected export.
        export_name: String,
    },
}

/// `identity` block of [`InspectOutput`]; shape follows the target type.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum InspectIdentity {
    /// Identity facts for a file target.
    File(InspectFileIdentity),
    /// Identity facts for a symbol target.
    Symbol(InspectSymbolIdentity),
}

/// Graph identity facts for a file target. `Value`-typed fields carry the
/// graph's verdict when analysis ran and are `null` when it did not.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InspectFileIdentity {
    /// File path relative to the analysed root.
    pub file: String,
    /// Whether the module graph can reach the file from an entry point.
    pub is_reachable: Option<serde_json::Value>,
    /// Whether the file is itself an entry point.
    pub is_entry_point: Option<serde_json::Value>,
    /// Number of exports the file declares.
    pub export_count: Option<usize>,
    /// Number of modules the file imports.
    pub import_count: Option<usize>,
    /// Number of modules that import the file.
    pub imported_by_count: Option<usize>,
}

/// Graph identity facts for a symbol target. `Value`-typed fields carry the
/// graph's verdict when analysis ran and are `null` when it did not.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InspectSymbolIdentity {
    /// File path relative to the analysed root.
    pub file: String,
    /// Name of the inspected export.
    pub export_name: String,
    /// Whether the containing file is reachable from an entry point.
    pub file_reachable: Option<serde_json::Value>,
    /// Whether the containing file is itself an entry point.
    pub is_entry_point: Option<serde_json::Value>,
    /// Whether the export has any recorded consumer.
    pub is_used: Option<serde_json::Value>,
    /// Explanation of the usage verdict, when the graph recorded one.
    pub reason: Option<serde_json::Value>,
}

/// `evidence` block of [`InspectOutput`]: one section per analysis.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InspectEvidence {
    /// File-level reachability trace.
    pub trace_file: InspectEvidenceSection,
    /// Export-level usage trace; present only for symbol targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_export: Option<InspectEvidenceSection>,
    /// Dead-code findings for the target.
    pub dead_code: InspectEvidenceSection,
    /// Duplication findings involving the target file.
    pub duplication: InspectEvidenceSection,
    /// Complexity findings for the target file.
    pub complexity: InspectEvidenceSection,
    /// Security findings for the target file.
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
    /// Checker-backed declaration, alias, re-export, and use-site evidence.
    /// Present only for a symbol target in type-aware mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_trace: Option<InspectEvidenceSection>,
    /// Package-public TypeScript surface and private-type reachability.
    /// Present only for a symbol target in type-aware mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_surface: Option<InspectEvidenceSection>,
    /// Exact-symbol consumers and transitive affected files.
    /// Present only for a symbol target in type-aware mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_impact: Option<InspectEvidenceSection>,
    /// Test entry points reachable from the exact symbol, with provenance.
    /// Present only for a symbol target in type-aware mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targeted_tests: Option<InspectEvidenceSection>,
}

/// One evidence section: status, the scope the evidence covers, and either a
/// data payload or a message explaining its absence.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InspectEvidenceSection {
    /// Whether the section's analysis produced usable evidence.
    pub status: InspectSectionStatus,
    /// Granularity the evidence covers.
    pub scope: InspectEvidenceScope,
    /// Explanation for `error` / `unavailable` sections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Section payload; present when the analysis ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl InspectEvidenceSection {
    /// Successful section carrying `data`.
    #[must_use]
    pub fn ok(scope: InspectEvidenceScope, data: serde_json::Value) -> Self {
        Self {
            status: InspectSectionStatus::Ok,
            scope,
            message: None,
            data: Some(data),
        }
    }

    /// Section whose status mirrors the semantic backend's completeness
    /// verdict while still carrying `data`.
    #[must_use]
    pub fn semantic(
        scope: InspectEvidenceScope,
        status: fallow_types::semantic::SemanticCompleteness,
        data: serde_json::Value,
    ) -> Self {
        let status = match status {
            fallow_types::semantic::SemanticCompleteness::Complete => InspectSectionStatus::Ok,
            fallow_types::semantic::SemanticCompleteness::Partial => InspectSectionStatus::Partial,
            fallow_types::semantic::SemanticCompleteness::Unavailable => {
                InspectSectionStatus::Unavailable
            }
        };
        Self {
            status,
            scope,
            message: None,
            data: Some(data),
        }
    }

    /// Failed section carrying only an explanation.
    #[must_use]
    pub fn error(scope: InspectEvidenceScope, message: String) -> Self {
        Self {
            status: InspectSectionStatus::Error,
            scope,
            message: Some(message),
            data: None,
        }
    }

    /// Section whose analysis could not run in this environment.
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

/// Status of an [`InspectEvidenceSection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum InspectSectionStatus {
    /// Analysis ran and the payload is complete.
    Ok,
    /// Analysis ran but the semantic backend reported incomplete coverage.
    Partial,
    /// Analysis could not run in this environment.
    Unavailable,
    /// Analysis failed; see `message`.
    Error,
}

/// Granularity an [`InspectEvidenceSection`] payload covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum InspectEvidenceScope {
    /// Evidence about the exact inspected symbol.
    Symbol,
    /// Evidence about the whole target file.
    File,
    /// Project-wide analysis filtered down to entries touching the file.
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
                semantic_trace: None,
                api_surface: None,
                symbol_impact: None,
                targeted_tests: None,
            },
            warnings: Vec::new(),
            meta: Some(fallow_types::envelope::Meta {
                type_aware: Some(fallow_types::envelope::TypeAwareMeta {
                    protocol_version: 6,
                    backend: "typescript-go".to_string(),
                    ..fallow_types::envelope::TypeAwareMeta::default()
                }),
                ..fallow_types::envelope::Meta::default()
            }),
        };

        let value =
            serialize_inspect_json_output(output, RootEnvelopeMode::Tagged, Some("run-inspect"))
                .expect("inspect output should serialize");

        assert_eq!(value["kind"], "inspect_target");
        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"],
            "run-inspect"
        );
        assert_eq!(value["_meta"]["type_aware"]["protocol_version"], 6);
        assert_eq!(value["_meta"]["type_aware"]["backend"], "typescript-go");
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
