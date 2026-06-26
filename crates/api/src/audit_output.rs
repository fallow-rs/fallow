//! Shared audit JSON payload contracts for programmatic consumers.

use fallow_config::AuditGate;
use fallow_output::{AuditCommand, RootEnvelopeMode};
use fallow_types::envelope::{ElapsedMs, SchemaVersion, ToolVersion};
use fallow_types::output::NextStep;
use serde::Serialize;

/// Verdict for the audit command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AuditVerdict {
    /// No issues in changed files.
    Pass,
    /// Issues found, but all are warn-severity.
    Warn,
    /// Error-severity issues found in changed files.
    Fail,
}

/// Per-category summary counts for the audit result.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuditSummary {
    pub dead_code_issues: usize,
    pub dead_code_has_errors: bool,
    pub complexity_findings: usize,
    pub max_cyclomatic: Option<u16>,
    pub duplication_clone_groups: usize,
}

/// New-vs-inherited issue counts for audit.
#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuditAttribution {
    pub gate: AuditGate,
    pub dead_code_introduced: usize,
    pub dead_code_inherited: usize,
    pub complexity_introduced: usize,
    pub complexity_inherited: usize,
    pub duplication_introduced: usize,
    pub duplication_inherited: usize,
}

/// Header fields shared by audit JSON and review-brief subtract sections.
pub struct AuditJsonHeaderInput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub verdict: AuditVerdict,
    pub changed_files_count: u32,
    pub base_ref: String,
    pub base_description: Option<String>,
    pub head_sha: Option<String>,
    pub elapsed_ms: ElapsedMs,
    pub base_snapshot_skipped: Option<bool>,
    pub summary: AuditSummary,
    pub attribution: AuditAttribution,
}

/// Typed audit JSON assembly input.
pub struct AuditJsonOutputInput<DeadCode, Duplication, Complexity> {
    pub header: AuditJsonHeaderInput,
    pub dead_code: Option<DeadCode>,
    pub duplication: Option<Duplication>,
    pub complexity: Option<Complexity>,
    pub next_steps: Vec<NextStep>,
}

#[derive(Serialize)]
struct AuditHeaderOutput {
    schema_version: SchemaVersion,
    version: ToolVersion,
    command: AuditCommand,
    verdict: AuditVerdict,
    changed_files_count: u32,
    base_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    head_sha: Option<String>,
    elapsed_ms: ElapsedMs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_snapshot_skipped: Option<bool>,
    summary: AuditSummary,
    attribution: AuditAttribution,
}

fn audit_header_output(input: AuditJsonHeaderInput) -> AuditHeaderOutput {
    AuditHeaderOutput {
        schema_version: input.schema_version,
        version: input.version,
        command: AuditCommand::Audit,
        verdict: input.verdict,
        changed_files_count: input.changed_files_count,
        base_ref: input.base_ref,
        base_description: input.base_description,
        head_sha: input.head_sha,
        elapsed_ms: input.elapsed_ms,
        base_snapshot_skipped: input.base_snapshot_skipped,
        summary: input.summary,
        attribution: input.attribution,
    }
}

/// Build the audit header JSON object used by review brief output.
///
/// # Errors
///
/// Returns a serde error if one of the typed header fields cannot be converted
/// to JSON.
pub fn build_audit_header_json(
    input: AuditJsonHeaderInput,
) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::to_value(audit_header_output(input))
}

/// Serialize a typed audit JSON output envelope.
///
/// # Errors
///
/// Returns a serde error if the envelope or one of its nested payload sections
/// cannot be converted to JSON.
pub fn serialize_audit_json<DeadCode, Duplication, Complexity>(
    input: AuditJsonOutputInput<DeadCode, Duplication, Complexity>,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error>
where
    DeadCode: Serialize,
    Duplication: Serialize,
    Complexity: Serialize,
{
    let header = audit_header_output(input.header);
    let output = fallow_output::AuditOutput {
        schema_version: header.schema_version,
        version: header.version,
        command: header.command,
        verdict: header.verdict,
        changed_files_count: header.changed_files_count,
        base_ref: header.base_ref,
        base_description: header.base_description,
        head_sha: header.head_sha,
        elapsed_ms: header.elapsed_ms,
        base_snapshot_skipped: header.base_snapshot_skipped,
        summary: header.summary,
        attribution: header.attribution,
        meta: None,
        dead_code: input.dead_code,
        duplication: input.duplication,
        complexity: input.complexity,
        next_steps: input.next_steps,
    };
    fallow_output::serialize_audit_json_output(output, mode, analysis_run_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_verdict_uses_snake_case_wire_names() {
        let value = serde_json::to_value(AuditVerdict::Pass).expect("serialize verdict");
        assert_eq!(value, serde_json::json!("pass"));
    }

    fn header_input() -> AuditJsonHeaderInput {
        AuditJsonHeaderInput {
            schema_version: SchemaVersion(7),
            version: ToolVersion("0.0.0-test".to_string()),
            verdict: AuditVerdict::Pass,
            changed_files_count: 5,
            base_ref: "abc123".to_string(),
            base_description: Some("merge-base with origin/main".to_string()),
            head_sha: Some("def456".to_string()),
            elapsed_ms: ElapsedMs(12),
            base_snapshot_skipped: Some(true),
            summary: AuditSummary {
                dead_code_issues: 0,
                dead_code_has_errors: false,
                complexity_findings: 0,
                max_cyclomatic: None,
                duplication_clone_groups: 0,
            },
            attribution: AuditAttribution {
                gate: AuditGate::NewOnly,
                ..AuditAttribution::default()
            },
        }
    }

    #[test]
    fn audit_header_json_uses_typed_contract_fields() {
        let value = build_audit_header_json(header_input()).expect("serialize audit header");

        assert_eq!(value["schema_version"], 7);
        assert_eq!(value["command"], "audit");
        assert_eq!(value["base_description"], "merge-base with origin/main");
        assert_eq!(value["head_sha"], "def456");
        assert_eq!(value["base_snapshot_skipped"], true);
    }

    #[test]
    fn audit_json_serializer_applies_root_kind_and_sections() {
        let value = serialize_audit_json(
            AuditJsonOutputInput {
                header: header_input(),
                dead_code: Some(serde_json::json!({"total_issues": 0})),
                duplication: None::<serde_json::Value>,
                complexity: None::<serde_json::Value>,
                next_steps: Vec::new(),
            },
            RootEnvelopeMode::Tagged,
            Some("run-1"),
        )
        .expect("serialize audit output");

        assert_eq!(value["kind"], "audit");
        assert_eq!(value["dead_code"]["total_issues"], 0);
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-1");
    }
}
