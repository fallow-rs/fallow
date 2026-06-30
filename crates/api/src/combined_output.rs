//! Combined JSON output assembly shared by CLI and programmatic consumers.

use std::path::Path;
use std::time::Duration;

use fallow_output::{
    CHECK_SCHEMA_VERSION, CombinedMeta, CombinedOutput, HealthReport, RootEnvelopeMode, check_meta,
    dupes_meta, harmonize_dead_code_health_suppress_line_actions, health_meta,
    serialize_combined_json_output, strip_root_prefix,
};
use fallow_types::envelope::{ElapsedMs, SchemaVersion, ToolVersion};
use fallow_types::output::NextStep;
use fallow_types::results::AnalysisResults;

use crate::{
    CheckJsonExtraOutputs, CheckJsonPayloadInput, DupesReportPayload, serialize_check_json_payload,
};

/// Dead-code section inputs for a bare combined JSON report.
pub struct CombinedCheckJsonSection<'a> {
    pub results: &'a AnalysisResults,
    pub root: &'a Path,
    pub elapsed: Duration,
    pub config_fixable: bool,
    pub extras: CheckJsonExtraOutputs,
}

/// Inputs for bare `fallow --format json` output assembly.
pub struct CombinedJsonOutputInput<'a> {
    pub check: Option<CombinedCheckJsonSection<'a>>,
    pub dupes: Option<&'a DupesReportPayload>,
    pub health: Option<&'a HealthReport>,
    pub root: &'a Path,
    pub elapsed: Duration,
    pub explain: bool,
    pub next_steps: Vec<NextStep>,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<&'a str>,
}

/// Build and serialize bare combined JSON through the API output boundary.
///
/// # Errors
///
/// Returns a serde error when any typed section cannot be converted to JSON.
pub fn serialize_combined_json(
    input: CombinedJsonOutputInput<'_>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut check_results = input.check.as_ref().map(|section| section.results.clone());
    let mut health_report = input.health.cloned();
    harmonize_dead_code_health_suppress_line_actions(
        check_results.as_mut(),
        health_report.as_mut(),
    );

    let check = if let Some(section) = input.check {
        if let Some(results) = check_results.as_ref() {
            Some(serialize_combined_check_json(section, results)?)
        } else {
            None
        }
    } else {
        None
    };
    let dupes = serialize_combined_dupes_json(input.dupes, input.root)?;
    let health = serialize_combined_health_json(health_report.as_ref(), input.root)?;

    let output = CombinedOutput {
        schema_version: SchemaVersion(CHECK_SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(elapsed_ms_for_output(input.elapsed)),
        meta: input
            .explain
            .then(|| combined_meta_for_output(check.is_some(), dupes.is_some(), health.is_some())),
        check,
        dupes,
        health,
        next_steps: input.next_steps,
    };

    serialize_combined_json_output(output, input.envelope_mode, input.telemetry_analysis_run_id)
}

fn serialize_combined_check_json(
    section: CombinedCheckJsonSection<'_>,
    results: &AnalysisResults,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_check_json_payload(CheckJsonPayloadInput {
        results,
        root: section.root,
        elapsed: section.elapsed,
        config_fixable: section.config_fixable,
        extras: section.extras,
        workspace_diagnostics: Vec::new(),
    })
}

/// Build a combined duplication section without adding a nested root envelope.
///
/// # Errors
///
/// Returns a serde error when the typed duplication payload cannot be
/// serialized.
pub fn serialize_combined_dupes_json(
    dupes: Option<&DupesReportPayload>,
    root: &Path,
) -> Result<Option<serde_json::Value>, serde_json::Error> {
    let Some(payload) = dupes else {
        return Ok(None);
    };
    let mut json = serde_json::to_value(payload)?;
    let root_prefix = format!("{}/", root.display());
    strip_root_prefix(&mut json, &root_prefix);
    Ok(Some(json))
}

/// Build a combined health section without adding a nested root envelope.
///
/// # Errors
///
/// Returns a serde error when the typed health payload cannot be serialized.
pub fn serialize_combined_health_json(
    health: Option<&HealthReport>,
    root: &Path,
) -> Result<Option<serde_json::Value>, serde_json::Error> {
    let Some(report) = health else {
        return Ok(None);
    };
    let mut json = serde_json::to_value(report)?;
    let root_prefix = format!("{}/", root.display());
    strip_root_prefix(&mut json, &root_prefix);
    Ok(Some(json))
}

fn elapsed_ms_for_output(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

fn combined_meta_for_output(
    include_check: bool,
    include_dupes: bool,
    include_health: bool,
) -> CombinedMeta {
    CombinedMeta {
        check: include_check.then(check_meta),
        dupes: include_dupes.then(dupes_meta),
        health: include_health.then(health_meta),
        telemetry: None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use fallow_output::{
        ComplexityViolation, ExceededThreshold, FindingSeverity, HealthFinding, HealthReport,
        RootEnvelopeMode,
    };
    use fallow_types::output_dead_code::UnusedExportFinding;
    use fallow_types::output_health::{HealthFindingAction, HealthFindingActionType};
    use fallow_types::results::{AnalysisResults, UnusedExport};

    use super::{CombinedCheckJsonSection, CombinedJsonOutputInput, serialize_combined_json};

    #[test]
    fn combined_json_root_contains_stable_envelope_fields() {
        let root = serialize_combined_json(CombinedJsonOutputInput {
            check: None,
            dupes: None,
            health: None,
            root: std::path::Path::new("."),
            elapsed: Duration::from_millis(42),
            explain: false,
            next_steps: Vec::new(),
            envelope_mode: RootEnvelopeMode::Tagged,
            telemetry_analysis_run_id: None,
        })
        .expect("combined JSON root");

        assert_eq!(
            root.get("kind").and_then(serde_json::Value::as_str),
            Some("combined")
        );
        assert_eq!(
            root.get("elapsed_ms").and_then(serde_json::Value::as_u64),
            Some(42)
        );
        assert!(root.get("schema_version").is_some());
        assert!(root.get("version").is_some());
    }

    #[test]
    fn combined_json_harmonizes_dead_code_and_health_suppress_actions_before_serialization() {
        let root = std::path::Path::new("/project");
        let path = root.join("src/shared.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "value".to_string(),
                is_type_only: false,
                line: 7,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let health = HealthReport {
            findings: vec![HealthFinding::new(
                ComplexityViolation {
                    path,
                    name: "expensive".to_string(),
                    line: 7,
                    col: 0,
                    cyclomatic: 22,
                    cognitive: 18,
                    line_count: 40,
                    param_count: 1,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
                    exceeded: ExceededThreshold::Both,
                    severity: FindingSeverity::High,
                    crap: None,
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                },
                vec![HealthFindingAction {
                    kind: HealthFindingActionType::SuppressLine,
                    auto_fixable: false,
                    description: "Suppress with an inline comment above the function declaration"
                        .to_string(),
                    note: None,
                    comment: Some("// fallow-ignore-next-line complexity".to_string()),
                    placement: Some("above-function-declaration".to_string()),
                    target_path: None,
                }],
                None,
            )],
            ..HealthReport::default()
        };

        let output = serialize_combined_json(CombinedJsonOutputInput {
            check: Some(CombinedCheckJsonSection {
                results: &results,
                root,
                elapsed: Duration::ZERO,
                config_fixable: false,
                extras: crate::CheckJsonExtraOutputs::default(),
            }),
            dupes: None,
            health: Some(&health),
            root,
            elapsed: Duration::ZERO,
            explain: false,
            next_steps: Vec::new(),
            envelope_mode: RootEnvelopeMode::Tagged,
            telemetry_analysis_run_id: None,
        })
        .expect("combined JSON");

        assert_eq!(
            output["check"]["unused_exports"][0]["actions"][1]["comment"],
            "// fallow-ignore-next-line unused-export, complexity"
        );
        assert_eq!(
            output["health"]["findings"][0]["actions"][0]["comment"],
            "// fallow-ignore-next-line unused-export, complexity"
        );
    }
}
