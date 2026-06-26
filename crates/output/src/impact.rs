//! Impact report output contracts.

use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};
use fallow_types::envelope::Meta;
use serde::{Deserialize, Serialize};

/// Per-category issue counts captured at a recorded run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImpactCounts {
    pub total_issues: usize,
    pub dead_code: usize,
    pub complexity: usize,
    pub duplication: usize,
}

impl ImpactCounts {
    #[must_use]
    pub fn from_combined(dead_code: usize, complexity: usize, duplication: usize) -> Self {
        Self {
            total_issues: dead_code + complexity + duplication,
            dead_code,
            complexity,
            duplication,
        }
    }
}

/// A commit-gate containment event recorded by `fallow impact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContainmentEvent {
    pub blocked_at: String,
    pub cleared_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    pub blocked_counts: ImpactCounts,
}

/// A resolved or suppressed finding attribution event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ResolutionEvent {
    pub kind: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    pub timestamp: String,
}

/// Why Impact tracking is or is not active for a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum EnabledSource {
    Project,
    User,
    Default,
}

/// Direction of a count trend between two recorded runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ImpactTrendDirection {
    /// Issue count went down.
    Improving,
    /// Issue count went up.
    Declining,
    /// Within tolerance.
    Stable,
}

/// A computed trend between the two most recent records.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TrendSummary {
    pub direction: ImpactTrendDirection,
    /// Signed delta in total issues, current minus previous.
    pub total_delta: i64,
    pub previous_total: usize,
    pub current_total: usize,
}

/// Wire-version discriminator for [`ImpactReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ImpactReportSchemaVersion {
    /// First release of the `fallow impact --format json` shape.
    #[serde(rename = "1")]
    V1,
}

/// The rendered impact report, derived purely from the store.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow impact --format json"))]
pub struct ImpactReport {
    /// Output-shape version for this report.
    pub schema_version: ImpactReportSchemaVersion,
    pub enabled: bool,
    pub enabled_source: EnabledSource,
    pub record_count: usize,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_recorded: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_git_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surfacing: Option<ImpactCounts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<TrendSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_surfacing: Option<ImpactCounts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_trend: Option<TrendSummary>,
    pub containment_count: usize,
    pub recent_containment: Vec<ContainmentEvent>,
    pub resolved_total: usize,
    pub suppressed_total: usize,
    pub recent_resolved: Vec<ResolutionEvent>,
    pub attribution_active: bool,
    pub onboarding_declined: bool,
    pub explicit_decision: bool,
}

/// Independent wire-version for the cross-repo impact report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CrossRepoImpactSchemaVersion {
    /// First release of the `fallow impact --all --format json` shape.
    #[serde(rename = "1")]
    V1,
}

/// Grand totals across every tracked project.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CrossRepoTotals {
    pub resolved_total: usize,
    pub suppressed_total: usize,
    pub containment_count: usize,
    pub project_wide_issues: usize,
    pub projects_with_baseline: usize,
}

/// One project's row in the cross-repo roll-up.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CrossRepoProjectEntry {
    pub project_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_recorded: Option<String>,
    pub report: ImpactReport,
}

/// The cross-repo aggregate report, `fallow impact --all --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow impact --all --format json")
)]
pub struct CrossRepoImpactReport {
    pub schema_version: CrossRepoImpactSchemaVersion,
    pub project_count: usize,
    pub tracked_count: usize,
    pub unreadable_count: usize,
    pub totals: CrossRepoTotals,
    pub projects: Vec<CrossRepoProjectEntry>,
}

/// Serialize the `fallow impact --format json` envelope.
///
/// # Errors
///
/// Returns a serde error when the report cannot be converted to JSON.
pub fn serialize_impact_json_output(
    report: ImpactReport,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(report, "impact", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Serialize the `fallow impact --all --format json` envelope.
///
/// # Errors
///
/// Returns a serde error when the report cannot be converted to JSON.
pub fn serialize_cross_repo_impact_json_output(
    report: CrossRepoImpactReport,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(report, "impact-cross-repo", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impact_report() -> ImpactReport {
        ImpactReport {
            schema_version: ImpactReportSchemaVersion::V1,
            enabled: true,
            enabled_source: EnabledSource::Project,
            record_count: 0,
            meta: None,
            first_recorded: None,
            latest_git_sha: None,
            surfacing: None,
            trend: None,
            project_surfacing: None,
            project_trend: None,
            containment_count: 0,
            recent_containment: Vec::new(),
            resolved_total: 0,
            suppressed_total: 0,
            recent_resolved: Vec::new(),
            attribution_active: false,
            onboarding_declined: false,
            explicit_decision: true,
        }
    }

    #[test]
    fn impact_json_output_uses_named_root_contract() {
        let value =
            serialize_impact_json_output(impact_report(), RootEnvelopeMode::Tagged, Some("run-1"))
                .expect("impact report should serialize");

        assert_eq!(value["kind"], "impact");
        assert_eq!(value["schema_version"], "1");
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-1");
    }

    #[test]
    fn cross_repo_impact_json_output_uses_named_root_contract() {
        let report = CrossRepoImpactReport {
            schema_version: CrossRepoImpactSchemaVersion::V1,
            project_count: 1,
            tracked_count: 1,
            unreadable_count: 0,
            totals: CrossRepoTotals::default(),
            projects: vec![CrossRepoProjectEntry {
                project_key: "demo".to_string(),
                label: None,
                last_recorded: None,
                report: impact_report(),
            }],
        };

        let value = serialize_cross_repo_impact_json_output(
            report,
            RootEnvelopeMode::Tagged,
            Some("run-2"),
        )
        .expect("cross-repo impact report should serialize");

        assert_eq!(value["kind"], "impact-cross-repo");
        assert_eq!(value["schema_version"], "1");
        assert_eq!(value["project_count"], 1);
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-2");
    }
}
