//! Schema-side aliases for fallow's top-level JSON output contract.

#[cfg(any(test, feature = "schema-emit"))]
use fallow_api::{
    AuditAttribution, AuditSummary, AuditVerdict, DupesReportPayload, DuplicationGroup,
};
use fallow_output::{
    CheckGroupedOutput, CheckOutput, CoverageAnalyzeOutput, CoverageSetupOutput, DupesOutput,
    ExplainOutput, HealthGroup, HealthOutput, HealthReport, ImpactReport, InspectOutput,
    ReviewEnvelopeOutput, ReviewReconcileOutput,
};
#[cfg(test)]
use fallow_output::{CombinedMeta, RootEnvelopeMode};

#[cfg(test)]
fn serialize_root_output_with_mode(
    output: FallowOutput,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = fallow_output::serialize_json_root_output(output, mode)?;
    fallow_output::attach_telemetry_meta(
        &mut value,
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    );
    Ok(value)
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

#[cfg(any(test, feature = "schema-emit"))]
pub type WorkspacesOutput = fallow_output::WorkspacesOutput<fallow_config::WorkspaceDiagnostic>;

#[cfg(any(test, feature = "schema-emit"))]
pub type SecurityGate = fallow_output::SecurityGate<fallow_api::SecurityGateMode>;

#[cfg(any(test, feature = "schema-emit"))]
pub type SecurityOutputConfig = fallow_output::SecurityOutputConfig<fallow_config::Severity>;

#[cfg(any(test, feature = "schema-emit"))]
pub type SecuritySummaryOutput =
    fallow_output::SecuritySummaryOutput<SecurityOutputConfig, SecurityGate>;

#[cfg(any(test, feature = "schema-emit"))]
pub type SecurityOutput = fallow_output::SecurityOutput<SecurityOutputConfig, SecurityGate>;

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
    ImpactReport,
    fallow_output::CrossRepoImpactReport,
    SecuritySummaryOutput,
    SecurityOutput,
    fallow_output::SecuritySurvivorsOutput,
    fallow_output::SecurityBlindSpotsOutput,
    CheckOutput,
    CombinedOutput,
    fallow_output::StandardReviewBriefOutput,
    fallow_output::DecisionSurfaceOutput,
    fallow_output::StandardWalkthroughGuide,
    fallow_output::WalkthroughValidation,
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
            crate::output_runtime::set_telemetry_analysis_run_id(run_id.map(str::to_owned));
            Self { _lock: lock }
        }
    }

    impl Drop for TelemetryRunIdGuard {
        fn drop(&mut self) {
            crate::output_runtime::set_telemetry_analysis_run_id(None);
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
            RootEnvelopeMode::Tagged,
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
            RootEnvelopeMode::Legacy,
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
            RootEnvelopeMode::Tagged,
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
        let value = serialize_root_output_with_mode(
            FallowOutput::Combined(output),
            RootEnvelopeMode::Tagged,
        )
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
