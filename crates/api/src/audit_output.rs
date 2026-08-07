//! Shared audit JSON payload contracts for programmatic consumers.

use fallow_config::AuditGate;
use fallow_output::{
    AuditCommand, CodeClimateIssue, RootEnvelopeMode, codeclimate_issues_to_value,
};
use fallow_types::duplicates::DuplicationReport;
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
    /// Total dead-code issues reported for the changed files.
    pub dead_code_issues: usize,
    /// Whether any reported dead-code issue has error severity.
    pub dead_code_has_errors: bool,
    /// Total complexity findings reported for the changed files.
    pub complexity_findings: usize,
    /// Highest cyclomatic complexity among the findings; `None` when there
    /// are no complexity findings.
    pub max_cyclomatic: Option<u16>,
    /// Clone groups touching the changed files.
    pub duplication_clone_groups: usize,
}

/// New-vs-inherited issue counts for audit.
#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuditAttribution {
    /// Configured gate: `new-only` fails only on introduced findings,
    /// `all` also fails on inherited ones.
    pub gate: AuditGate,
    /// Dead-code findings absent from the base snapshot.
    pub dead_code_introduced: usize,
    /// Dead-code findings already present in the base snapshot.
    pub dead_code_inherited: usize,
    /// Complexity findings absent from the base snapshot.
    pub complexity_introduced: usize,
    /// Complexity findings already present in the base snapshot.
    pub complexity_inherited: usize,
    /// Clone groups absent from the base snapshot.
    pub duplication_introduced: usize,
    /// Clone groups already present in the base snapshot.
    pub duplication_inherited: usize,
}

/// Header fields shared by audit JSON and review-brief subtract sections.
pub struct AuditJsonHeaderInput {
    /// Output schema version of the envelope.
    pub schema_version: SchemaVersion,
    /// Fallow version that produced the output.
    pub version: ToolVersion,
    /// Overall audit verdict.
    pub verdict: AuditVerdict,
    /// Number of changed files the audit analyzed.
    pub changed_files_count: u32,
    /// Git revision the audit compared against.
    pub base_ref: String,
    /// Human-readable description of how the base was chosen, such as
    /// `merge-base with origin/main`; omitted from JSON when `None`.
    pub base_description: Option<String>,
    /// Commit SHA of the analyzed head; omitted from JSON when `None`.
    pub head_sha: Option<String>,
    /// Audit wall time in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// `Some(true)` when the base snapshot was skipped, so no attribution
    /// ran; omitted from JSON when `None`.
    pub base_snapshot_skipped: Option<bool>,
    /// Per-category issue counts.
    pub summary: AuditSummary,
    /// New-vs-inherited counts and the configured gate.
    pub attribution: AuditAttribution,
}

/// Typed audit JSON assembly input.
pub struct AuditJsonOutputInput<DeadCode, Duplication, Complexity> {
    /// Envelope header fields.
    pub header: AuditJsonHeaderInput,
    /// Optional explain metadata block.
    pub meta: Option<fallow_types::envelope::Meta>,
    /// Dead-code section payload; `None` omits the section.
    pub dead_code: Option<DeadCode>,
    /// Duplication section payload; `None` omits the section.
    pub duplication: Option<Duplication>,
    /// Complexity (health) section payload; `None` omits the section.
    pub complexity: Option<Complexity>,
    /// Suggested follow-up commands for the consumer.
    pub next_steps: Vec<NextStep>,
}

/// Typed audit SARIF assembly input.
#[derive(Clone, Copy)]
pub struct AuditSarifOutputInput<'a> {
    /// Prebuilt dead-code SARIF document whose runs are merged in.
    pub dead_code: Option<&'a serde_json::Value>,
    /// Duplication report converted into a dedicated SARIF run; skipped when
    /// it has no clone groups.
    pub duplication: Option<&'a DuplicationReport>,
    /// Prebuilt health SARIF document whose runs are merged in.
    pub health: Option<&'a serde_json::Value>,
}

/// Typed audit CodeClimate assembly input.
pub struct AuditCodeClimateOutputInput {
    /// Dead-code issues, emitted first in the combined array.
    pub dead_code: Vec<CodeClimateIssue>,
    /// Duplication issues, emitted after dead code.
    pub duplication: Vec<CodeClimateIssue>,
    /// Health issues, emitted last.
    pub health: Vec<CodeClimateIssue>,
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

/// Build the audit header as an object map for composed output contracts such
/// as review briefs.
///
/// # Errors
///
/// Returns a serde error if one of the typed header fields cannot be converted
/// to JSON, or if the typed header unexpectedly does not serialize to an
/// object.
pub fn build_audit_header_map(
    input: AuditJsonHeaderInput,
) -> Result<serde_json::Map<String, serde_json::Value>, serde_json::Error> {
    match build_audit_header_json(input)? {
        serde_json::Value::Object(header) => Ok(header),
        _ => unreachable!("AuditHeaderOutput serializes to an object"),
    }
}

/// Build the typed audit metadata carried by a review brief envelope.
#[must_use]
pub fn build_review_brief_header(
    input: AuditJsonHeaderInput,
) -> fallow_output::ReviewBriefHeader<AuditVerdict, AuditSummary, AuditAttribution> {
    fallow_output::ReviewBriefHeader {
        version: input.version,
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
    let complexity = input.complexity.map(serde_json::to_value).transpose()?;
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
        meta: input.meta,
        dead_code: input.dead_code,
        duplication: input.duplication,
        complexity,
        next_steps: input.next_steps,
    };
    let mut value = fallow_output::serialize_audit_json_output(output, mode, analysis_run_id)?;
    attach_audit_styling_attribution(&mut value);
    Ok(value)
}

/// Add styling attribution totals derived from annotated styling findings.
///
/// This keeps the public Rust attribution and finding structs source-compatible
/// while extending audit-family JSON envelopes with the wire-only fields.
pub fn attach_audit_styling_attribution(value: &mut serde_json::Value) {
    let findings = value
        .get("complexity")
        .and_then(|complexity| complexity.get("styling_findings"))
        .and_then(serde_json::Value::as_array);
    let styling_introduced = findings.map_or(0, |items| {
        items
            .iter()
            .filter(|item| {
                item.get("introduced").and_then(serde_json::Value::as_bool) == Some(true)
            })
            .count()
    });
    let styling_inherited = findings.map_or(0, |items| {
        items
            .iter()
            .filter(|item| {
                item.get("introduced").and_then(serde_json::Value::as_bool) == Some(false)
            })
            .count()
    });
    if let Some(attribution) = value
        .get_mut("attribution")
        .and_then(serde_json::Value::as_object_mut)
    {
        attribution.insert(
            "styling_introduced".to_string(),
            serde_json::json!(styling_introduced),
        );
        attribution.insert(
            "styling_inherited".to_string(),
            serde_json::json!(styling_inherited),
        );
    }
}

/// Build the combined SARIF document for `fallow audit`.
#[must_use]
pub fn build_audit_sarif(input: AuditSarifOutputInput<'_>) -> serde_json::Value {
    let mut all_runs = Vec::new();

    if let Some(sarif) = input.dead_code {
        extend_sarif_runs(&mut all_runs, sarif);
    }

    if let Some(duplication) = input.duplication
        && !duplication.clone_groups.is_empty()
    {
        all_runs.push(build_audit_duplication_sarif_run(duplication));
    }

    if let Some(sarif) = input.health {
        extend_sarif_runs(&mut all_runs, sarif);
    }

    serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": all_runs,
    })
}

fn extend_sarif_runs(all_runs: &mut Vec<serde_json::Value>, sarif: &serde_json::Value) {
    if let Some(runs) = sarif.get("runs").and_then(|runs| runs.as_array()) {
        all_runs.extend(runs.iter().cloned());
    }
}

fn build_audit_duplication_sarif_run(duplication: &DuplicationReport) -> serde_json::Value {
    serde_json::json!({
        "tool": {
            "driver": {
                "name": "fallow",
                "version": env!("CARGO_PKG_VERSION"),
                "informationUri": "https://github.com/fallow-rs/fallow",
            }
        },
        "automationDetails": { "id": "fallow/audit/dupes" },
        "results": duplication.clone_groups.iter().enumerate().map(|(i, group)| {
            serde_json::json!({
                "ruleId": "fallow/code-duplication",
                "level": "warning",
                "message": {
                    "text": format!(
                        "Clone group {} ({} lines, {} instances)",
                        i + 1,
                        group.line_count,
                        group.instances.len()
                    ),
                },
            })
        }).collect::<Vec<_>>()
    })
}

/// Build combined CodeClimate issues for `fallow audit`.
#[must_use]
pub fn build_audit_codeclimate_issues(input: AuditCodeClimateOutputInput) -> Vec<CodeClimateIssue> {
    let mut all_issues = input.dead_code;
    all_issues.extend(input.duplication);
    all_issues.extend(input.health);
    all_issues
}

/// Build the combined CodeClimate JSON array for `fallow audit`.
#[must_use]
pub fn build_audit_codeclimate(input: AuditCodeClimateOutputInput) -> serde_json::Value {
    codeclimate_issues_to_value(&build_audit_codeclimate_issues(input))
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
    fn audit_header_map_uses_typed_contract_fields() {
        let header = build_audit_header_map(header_input()).expect("serialize audit header");

        assert_eq!(header["schema_version"], 7);
        assert_eq!(header["command"], "audit");
        assert_eq!(header["base_description"], "merge-base with origin/main");
    }

    #[test]
    fn audit_json_serializer_applies_root_kind_and_sections() {
        let value = serialize_audit_json(
            AuditJsonOutputInput {
                header: header_input(),
                meta: None,
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

    #[test]
    fn audit_sarif_combines_runs_and_duplication_run() {
        let duplication = DuplicationReport {
            clone_groups: vec![fallow_types::duplicates::CloneGroup {
                instances: vec![
                    fallow_types::duplicates::CloneInstance {
                        file: "src/a.ts".into(),
                        start_line: 1,
                        end_line: 12,
                        start_col: 1,
                        end_col: 1,
                        fragment: "duplicated();".to_string(),
                    },
                    fallow_types::duplicates::CloneInstance {
                        file: "src/b.ts".into(),
                        start_line: 1,
                        end_line: 12,
                        start_col: 1,
                        end_col: 1,
                        fragment: "duplicated();".to_string(),
                    },
                ],
                token_count: 40,
                line_count: 12,
                similarity: None,
            }],
            ..DuplicationReport::default()
        };
        let dead_code = serde_json::json!({"runs": [{"automationDetails": {"id": "check"}}]});
        let health = serde_json::json!({"runs": [{"automationDetails": {"id": "health"}}]});

        let value = build_audit_sarif(AuditSarifOutputInput {
            dead_code: Some(&dead_code),
            duplication: Some(&duplication),
            health: Some(&health),
        });

        assert_eq!(value["version"], "2.1.0");
        assert_eq!(value["runs"].as_array().expect("runs").len(), 3);
        assert_eq!(
            value["runs"][1]["automationDetails"]["id"],
            "fallow/audit/dupes"
        );
    }

    #[test]
    fn audit_codeclimate_combines_issue_sections() {
        let issue = CodeClimateIssue {
            kind: fallow_output::CodeClimateIssueKind::Issue,
            check_name: "fallow/test".to_string(),
            description: "test".to_string(),
            severity: fallow_output::CodeClimateSeverity::Minor,
            fingerprint: "abc".to_string(),
            location: fallow_output::CodeClimateLocation {
                path: "src/a.ts".to_string(),
                lines: fallow_output::CodeClimateLines { begin: 1 },
            },
            categories: vec!["Bug Risk".to_string()],
            owner: None,
            group: None,
        };

        let value = build_audit_codeclimate(AuditCodeClimateOutputInput {
            dead_code: vec![issue.clone()],
            duplication: vec![issue.clone()],
            health: vec![issue],
        });

        assert_eq!(value.as_array().expect("issues").len(), 3);
    }
}
