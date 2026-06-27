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

/// Why Impact tracking is (or is not) active for a project. `Project` = an
/// explicit per-repo `enable`; `User` = the user-global default with no per-repo
/// decision; `Default` = off (no per-repo decision and no global default).
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

/// Wire-version discriminator for [`ImpactReport`]. Independent from the global
/// `SchemaVersion` (the impact report versions on its own cadence) and from the
/// on-disk `STORE_SCHEMA_VERSION` (the persisted store shape versions
/// separately). Serializes as a string `const` so JSON consumers can switch on
/// it, matching the other independently-versioned envelopes (e.g.
/// `CoverageAnalyzeSchemaVersion`).
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
    /// Output-shape version for this report, so JSON consumers have a
    /// forward-compat signal independent of the on-disk store version. Always
    /// present; bumped only on a breaking change to this report's wire shape.
    pub schema_version: ImpactReportSchemaVersion,
    pub enabled: bool,
    /// WHY tracking is on or off: `project` (an explicit per-repo enable/disable
    /// decision), `user` (the user-global default with no per-repo decision), or
    /// `default` (off, no per-repo decision and no global default). Combine with
    /// `explicit_decision` to tell a never-asked off-state (`enabled:false`,
    /// `explicit_decision:false`, offer to enable) from a declined-here one
    /// (`enabled:false`, `explicit_decision:true`, do not nag).
    pub enabled_source: EnabledSource,
    pub record_count: usize,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_recorded: Option<String>,
    /// Git SHA of the most recent recorded run, so a consumer can tell which
    /// commit the `surfacing` counts belong to. This is an ABBREVIATED SHA
    /// (`git rev-parse --short`), so it is for display/correlation only and will
    /// not match a full 40-character SHA from `$GITHUB_SHA` or the git API
    /// without expansion. None when the latest run had no SHA (not a git repo)
    /// or there are no records yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_git_sha: Option<String>,
    /// Counts from the most recent recorded run. These are CHANGED-FILE scoped
    /// (each record comes from a `fallow audit` run, whose default `new-only`
    /// gate counts only findings in the changed files of that run), NOT a
    /// whole-project total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surfacing: Option<ImpactCounts>,
    /// Trend between the two most recent records. None until two records exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<TrendSummary>,
    /// Counts from the most recent whole-project `fallow` run. WHOLE-PROJECT
    /// scope (not changed-file), so this is the current issue total across the
    /// whole repo, context next to the actionable changed-file `surfacing`
    /// count. None until a full `fallow` run has been recorded. v1.6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_surfacing: Option<ImpactCounts>,
    /// Trend between the two most recent whole-project records. Comparable over
    /// time (same whole-project denominator every run), unlike the changed-file
    /// `trend`. None until two full `fallow` runs exist. v1.6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_trend: Option<TrendSummary>,
    pub containment_count: usize,
    /// Most recent containment events (newest last), capped for display.
    pub recent_containment: Vec<ContainmentEvent>,
    /// Lifetime count of findings fallow credits as genuinely resolved (code
    /// removed or refactored, never a `fallow-ignore`). v1.5.
    pub resolved_total: usize,
    /// Lifetime count of findings silenced by a newly-added `fallow-ignore`.
    /// Reported as honest context, never as a win. v1.5.
    pub suppressed_total: usize,
    /// Most recent resolution events (newest last), capped for display. v1.5.
    pub recent_resolved: Vec<ResolutionEvent>,
    /// Whether per-finding attribution has a baseline yet. False on a freshly
    /// upgraded v1 store (no frontier captured), which the renderer uses to show
    /// "resolution tracking starts from your next run" instead of a bare zero.
    pub attribution_active: bool,
    /// Whether the local agent onboarding prompt has been explicitly declined.
    /// Stored in the user config dir (per project) so agents avoid cross-session
    /// nags without writing into the repo.
    pub onboarding_declined: bool,
    /// Whether the user ever made an explicit enable/disable decision for
    /// Impact tracking. `enabled: false` with `explicit_decision: false` means
    /// "never asked"; with `true` it means "asked and declined". Agents use
    /// this to offer the impact opt-in exactly once per project.
    pub explicit_decision: bool,
}

/// Independent wire-version for the cross-repo report, on its own cadence (it
/// versions separately from the per-project `ImpactReportSchemaVersion` and the
/// on-disk `STORE_SCHEMA_VERSION`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CrossRepoImpactSchemaVersion {
    /// First release of the `fallow impact --all --format json` shape.
    #[serde(rename = "1")]
    V1,
}

/// Grand totals across every tracked project (including repos whose directory no
/// longer exists on disk: their past wins still count toward lifetime impact).
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CrossRepoTotals {
    pub resolved_total: usize,
    pub suppressed_total: usize,
    pub containment_count: usize,
    /// Sum of whole-project issue totals across projects that have a full-run
    /// baseline, as of EACH project's last full `fallow` run (not a simultaneous
    /// snapshot).
    pub project_wide_issues: usize,
    pub projects_with_baseline: usize,
}

/// One project's row in the cross-repo roll-up.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CrossRepoProjectEntry {
    /// Stable, non-reversible project key (the store filename stem); the
    /// cross-tool/cross-run JOIN key. NEVER a path.
    pub project_key: String,
    /// Repo basename for display (never a full path). Absent on pre-v5 stores
    /// (the row falls back to the short key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Timestamp of the project's most recent recorded run (changed-file or
    /// whole-project), for the LAST RUN column and the default `recent` sort.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_recorded: Option<String>,
    /// The full per-project report (identical shape to `fallow impact --format
    /// json`), reused verbatim so the per-project wire contract is the sub-shape.
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
    /// Per-project stores successfully parsed (add `unreadable_count` for the
    /// total number of store files found in the user config dir).
    pub project_count: usize,
    /// Stores with recorded history (the rows in `projects`); excludes
    /// enabled-but-empty stores, which are still counted in `project_count`.
    pub tracked_count: usize,
    /// Stores that failed to parse and were skipped (corrupt or newer-schema).
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
