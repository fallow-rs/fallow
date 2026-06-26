//! Typed envelope structs for the JSON output contract.
//!
//! This module is the schema-side source of truth for fallow's top-level JSON
//! envelopes.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(any(test, feature = "schema-emit"))]
use crate::audit::{AuditAttribution, AuditSummary, AuditVerdict};
#[cfg(any(test, feature = "schema-emit"))]
use crate::health_types::{HealthGroup, HealthReport};
#[cfg(any(test, feature = "schema-emit"))]
use crate::output_dupes::DupesReportPayload;
#[cfg(any(test, feature = "schema-emit"))]
use crate::report::dupes_grouping::DuplicationGroup;
#[allow(
    unused_imports,
    reason = "compatibility re-export while CLI output contracts move to fallow-output"
)]
pub use fallow_output::{
    AuditCommand, BoundariesListRule, BoundariesListZone, CheckGroupedEntry, CheckGroupedOutput,
    CheckOutput, CheckOutputInput, CodeClimateIssue, CodeClimateIssueKind, CodeClimateLines,
    CodeClimateLocation, CodeClimateOutput, CodeClimateSeverity, CombinedMeta,
    CoverageAnalyzeOutput, CoverageAnalyzeSchemaVersion, CoverageSetupFileToEdit,
    CoverageSetupFramework, CoverageSetupMember, CoverageSetupOutput, CoverageSetupPackageManager,
    CoverageSetupRuntimeTarget, CoverageSetupSchemaVersion, CoverageSetupSnippet, DupesOutput,
    DupesOutputInput, ExplainOutput, GitHubReviewComment, GitHubReviewSide, GitLabReviewComment,
    GitLabReviewPosition, GitLabReviewPositionType, GroupByMode, HealthOutput, HealthOutputInput,
    InspectEvidence, InspectEvidenceScope, InspectEvidenceSection, InspectFileIdentity,
    InspectIdentity, InspectOutput, InspectSectionStatus, InspectSymbolIdentity,
    InspectTargetDescriptor, MARKER_REGEX_FLAGS_V2, MARKER_REGEX_V2, ReviewCheckConclusion,
    ReviewComment, ReviewEnvelopeEvent, ReviewEnvelopeMeta, ReviewEnvelopeOutput,
    ReviewEnvelopeSchema, ReviewEnvelopeSummary, ReviewProvider, ReviewReconcileOutput,
    ReviewReconcileSchema, WorkspaceDiagnosticOutput, WorkspaceInfo,
    apply_config_fixable_to_duplicate_exports, build_check_output, build_check_summary,
    build_dupes_output, build_health_output, default_marker_regex, default_marker_regex_flags,
    is_false,
};
use serde::Serialize;

static LEGACY_ENVELOPE: AtomicBool = AtomicBool::new(false);
static TELEMETRY_ANALYSIS_RUN_ID: Mutex<Option<String>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeMode {
    Tagged,
    Legacy,
}

impl EnvelopeMode {
    #[must_use]
    pub fn current() -> Self {
        if LEGACY_ENVELOPE.load(Ordering::Relaxed) {
            Self::Legacy
        } else {
            Self::Tagged
        }
    }
}

pub fn set_legacy_envelope(enabled: bool) {
    LEGACY_ENVELOPE.store(enabled, Ordering::Relaxed);
}

pub fn set_telemetry_analysis_run_id(run_id: Option<String>) {
    if let Ok(mut current) = TELEMETRY_ANALYSIS_RUN_ID.lock() {
        *current = run_id;
    }
}

pub fn telemetry_analysis_run_id() -> Option<String> {
    TELEMETRY_ANALYSIS_RUN_ID
        .lock()
        .ok()
        .and_then(|id| id.clone())
}

pub fn serialize_named_root_output<T: Serialize>(
    output: T,
    kind: &'static str,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value =
        fallow_output::serialize_named_json_output(output, kind, EnvelopeMode::current().into())?;
    attach_telemetry_meta(&mut value);
    Ok(value)
}

pub fn serialize_named_root_output_without_telemetry<T: Serialize>(
    output: T,
    kind: &'static str,
) -> Result<serde_json::Value, serde_json::Error> {
    fallow_output::serialize_named_json_output(output, kind, EnvelopeMode::current().into())
}

#[cfg(test)]
fn serialize_root_output_with_mode(
    output: FallowOutput,
    mode: EnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = fallow_output::serialize_json_root_output(output, mode.into())?;
    attach_telemetry_meta(&mut value);
    Ok(value)
}

pub fn attach_telemetry_meta(value: &mut serde_json::Value) {
    fallow_output::attach_telemetry_meta(value, telemetry_analysis_run_id().as_deref());
}

impl From<EnvelopeMode> for fallow_output::RootEnvelopeMode {
    fn from(mode: EnvelopeMode) -> Self {
        match mode {
            EnvelopeMode::Tagged => Self::Tagged,
            EnvelopeMode::Legacy => Self::Legacy,
        }
    }
}
#[cfg(any(test, feature = "schema-emit"))]
pub type AuditOutput = fallow_output::AuditOutput<
    AuditVerdict,
    AuditSummary,
    AuditAttribution,
    CheckOutput,
    DupesReportPayload,
    HealthReport,
>;

#[cfg(any(test, feature = "schema-emit"))]
pub type CombinedOutput =
    fallow_output::CombinedOutput<CheckOutput, DupesReportPayload, HealthReport>;

#[cfg(any(test, feature = "schema-emit"))]
pub type ListBoundariesOutput = fallow_output::ListBoundariesOutput<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

pub type WorkspacesOutput = fallow_output::WorkspacesOutput<fallow_config::WorkspaceDiagnostic>;

#[allow(
    dead_code,
    reason = "schema compatibility alias for the concrete fallow-config boundary contract"
)]
pub type BoundariesListing = fallow_output::BoundariesListing<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

#[allow(
    dead_code,
    reason = "schema compatibility alias for the concrete fallow-config boundary contract"
)]
pub type BoundariesListLogicalGroup = fallow_output::BoundariesListLogicalGroup<
    fallow_config::LogicalGroupStatus,
    fallow_config::AuthoredRule,
>;

#[cfg(any(test, feature = "schema-emit"))]
#[allow(
    clippy::type_complexity,
    reason = "concrete CLI alias fills every generic payload slot in the shared root output contract"
)]
pub type FallowOutput = fallow_output::FallowOutput<
    AuditOutput,
    ExplainOutput,
    InspectOutput,
    fallow_engine::trace_chain::SymbolChainTrace,
    ReviewEnvelopeOutput,
    ReviewReconcileOutput,
    CoverageSetupOutput,
    CoverageAnalyzeOutput,
    ListBoundariesOutput,
    WorkspacesOutput,
    HealthOutput<HealthReport, HealthGroup>,
    DupesOutput<DupesReportPayload, DuplicationGroup>,
    CheckGroupedOutput,
    crate::impact::ImpactReport,
    crate::impact::CrossRepoImpactReport,
    fallow_output::SecuritySummaryOutput<
        crate::security::SecurityOutputConfig,
        crate::security::SecurityGate,
    >,
    crate::security::SecurityOutput,
    crate::security::SecuritySurvivorsOutput,
    crate::security::SecurityBlindSpotsOutput,
    CheckOutput,
    CombinedOutput,
    crate::audit_brief::ReviewBriefOutput,
    fallow_output::DecisionSurfaceOutput,
    crate::audit_walkthrough::WalkthroughGuide,
    crate::audit_walkthrough::WalkthroughValidation,
>;

#[cfg(test)]
mod tests {
    use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, ToolVersion};

    use super::*;

    static TEST_TELEMETRY_RUN_ID_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct TelemetryRunIdGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl TelemetryRunIdGuard {
        fn set(run_id: Option<&str>) -> Self {
            let lock = TEST_TELEMETRY_RUN_ID_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            set_telemetry_analysis_run_id(run_id.map(str::to_owned));
            Self { _lock: lock }
        }
    }

    impl Drop for TelemetryRunIdGuard {
        fn drop(&mut self) {
            set_telemetry_analysis_run_id(None);
        }
    }

    fn combined_output() -> CombinedOutput {
        CombinedOutput {
            schema_version: SchemaVersion(crate::report::SCHEMA_VERSION),
            version: ToolVersion("test".to_string()),
            elapsed_ms: ElapsedMs(0),
            meta: None,
            check: None,
            dupes: None,
            health: None,
            next_steps: Vec::new(),
        }
    }

    #[test]
    fn root_output_serializes_kind_by_default() {
        let _guard = TelemetryRunIdGuard::set(None);
        let value = serialize_root_output_with_mode(
            FallowOutput::Combined(combined_output()),
            EnvelopeMode::Tagged,
        )
        .expect("combined root should serialize");

        assert_eq!(value["kind"], serde_json::Value::String("combined".into()));
        assert_eq!(value["schema_version"], crate::report::SCHEMA_VERSION);
    }

    #[test]
    fn legacy_mode_removes_only_root_kind() {
        let _guard = TelemetryRunIdGuard::set(None);
        let value = serialize_root_output_with_mode(
            FallowOutput::Combined(combined_output()),
            EnvelopeMode::Legacy,
        )
        .expect("combined root should serialize");

        assert!(value.get("kind").is_none());

        let mut nested = serde_json::json!({
            "kind": "root",
            "action": {
                "kind": "suppress"
            }
        });
        fallow_output::remove_root_kind(&mut nested);
        assert!(nested.get("kind").is_none());
        assert_eq!(nested["action"]["kind"], "suppress");
    }

    #[test]
    fn root_output_attaches_telemetry_meta() {
        let _guard = TelemetryRunIdGuard::set(Some("run_test123"));
        let value = serialize_root_output_with_mode(
            FallowOutput::Combined(combined_output()),
            EnvelopeMode::Tagged,
        )
        .expect("combined root should serialize");

        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"].as_str(),
            Some("run_test123")
        );
    }

    #[test]
    fn telemetry_meta_preserves_existing_meta_sections() {
        let mut output = combined_output();
        output.meta = Some(CombinedMeta {
            check: Some(Meta {
                docs: Some("https://example.com/check".to_string()),
                ..Meta::default()
            }),
            dupes: None,
            health: None,
            telemetry: None,
        });

        let _guard = TelemetryRunIdGuard::set(Some("run_test123"));
        let value =
            serialize_root_output_with_mode(FallowOutput::Combined(output), EnvelopeMode::Tagged)
                .expect("combined root should serialize");

        assert_eq!(
            value["_meta"]["check"]["docs"].as_str(),
            Some("https://example.com/check")
        );
        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"].as_str(),
            Some("run_test123")
        );
    }
}
