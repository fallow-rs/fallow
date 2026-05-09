use std::fmt;
use std::path::PathBuf;

/// Top-level verdict for the whole runtime-coverage report. Mirrors
/// `fallow_cov_protocol::ReportVerdict`. The verdict is the SINGLE most
/// actionable finding; for the full set of findings see [`signals`] on
/// [`RuntimeCoverageReport`]. The verdict promotes `hot-path-touched`
/// above `cold-code-detected` in PR-review context (when the CLI was
/// given a change-scope: `--diff-file` or `--changed-since`) because the
/// touched-hot-path is event-tied to the current diff and reviewers need
/// it to be the top-line signal. In standalone analysis (no change
/// scope), `cold-code-detected` remains primary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeCoverageReportVerdict {
    Clean,
    HotPathTouched,
    ColdCodeDetected,
    LicenseExpiredGrace,
    #[default]
    Unknown,
}

/// Discrete signal captured during runtime-coverage post-processing.
/// `verdict` collapses to one summary value; `signals` enumerates ALL
/// findings the report carries so JSON consumers, CI dashboards, and
/// agents can reason about them independently of the headline. Order is
/// stable: severity-descending so the first entry mirrors a sensible
/// non-PR-context verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeCoverageSignal {
    LicenseExpiredGrace,
    ColdCodeDetected,
    HotPathTouched,
}

impl RuntimeCoverageSignal {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LicenseExpiredGrace => "license-expired-grace",
            Self::ColdCodeDetected => "cold-code-detected",
            Self::HotPathTouched => "hot-path-touched",
        }
    }
}

impl fmt::Display for RuntimeCoverageSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl RuntimeCoverageReportVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::HotPathTouched => "hot-path-touched",
            Self::ColdCodeDetected => "cold-code-detected",
            Self::LicenseExpiredGrace => "license-expired-grace",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for RuntimeCoverageReportVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Per-finding verdict. Replaces the 0.1 `state` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCoverageVerdict {
    SafeToDelete,
    ReviewRequired,
    CoverageUnavailable,
    LowTraffic,
    Active,
    Unknown,
}

impl RuntimeCoverageVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SafeToDelete => "safe_to_delete",
            Self::ReviewRequired => "review_required",
            Self::CoverageUnavailable => "coverage_unavailable",
            Self::LowTraffic => "low_traffic",
            Self::Active => "active",
            Self::Unknown => "unknown",
        }
    }

    #[must_use]
    pub const fn human_label(self) -> &'static str {
        match self {
            Self::SafeToDelete => "safe to delete",
            Self::ReviewRequired => "review required",
            Self::CoverageUnavailable => "coverage unavailable",
            Self::LowTraffic => "low traffic",
            Self::Active => "active",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for RuntimeCoverageVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCoverageConfidence {
    VeryHigh,
    High,
    Medium,
    Low,
    None,
    Unknown,
}

impl RuntimeCoverageConfidence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VeryHigh => "very_high",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::None => "none",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for RuntimeCoverageConfidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeCoverageWatermark {
    TrialExpired,
    LicenseExpiredGrace,
    Unknown,
}

impl RuntimeCoverageWatermark {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TrialExpired => "trial-expired",
            Self::LicenseExpiredGrace => "license-expired-grace",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for RuntimeCoverageWatermark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Runtime coverage source used to produce the summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCoverageDataSource {
    #[default]
    Local,
    Cloud,
}

impl RuntimeCoverageDataSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cloud => "cloud",
        }
    }
}

impl fmt::Display for RuntimeCoverageDataSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Summary block mirroring `fallow_cov_protocol::Summary` (0.3 shape).
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RuntimeCoverageSummary {
    pub data_source: RuntimeCoverageDataSource,
    pub last_received_at: Option<String>,
    pub functions_tracked: usize,
    pub functions_hit: usize,
    pub functions_unhit: usize,
    pub functions_untracked: usize,
    pub coverage_percent: f64,
    pub trace_count: u64,
    pub period_days: u32,
    pub deployments_seen: u32,
    /// Capture-quality telemetry. `None` for protocol-0.2 sidecars; protocol-0.3+
    /// sidecars always populate it. Fuels the human-output short-window warning
    /// and the quantified trial CTA, and is passed through to JSON consumers so
    /// agent pipelines can surface the same signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_quality: Option<RuntimeCoverageCaptureQuality>,
}

/// Capture-quality telemetry (mirrors `fallow_cov_protocol::CaptureQuality`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RuntimeCoverageCaptureQuality {
    pub window_seconds: u64,
    pub instances_observed: u32,
    pub lazy_parse_warning: bool,
    pub untracked_ratio_percent: f64,
}

/// Supporting evidence for a finding (mirrors `fallow_cov_protocol::Evidence`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeCoverageEvidence {
    pub static_status: String,
    pub test_coverage: String,
    pub v8_tracking: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub untracked_reason: Option<String>,
    pub observation_days: u32,
    pub deployments_observed: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeCoverageAction {
    /// Stable action identifier. Serialized as `type` in JSON to match the
    /// `actions[].type` contract shared with every other `fallow health` finding.
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
    pub auto_fixable: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeCoverageMessage {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeCoverageFinding {
    /// Stable content-hash ID of the form `fallow:prod:<hash>`.
    pub id: String,
    pub path: PathBuf,
    pub function: String,
    pub line: u32,
    pub verdict: RuntimeCoverageVerdict,
    /// Raw V8 invocation count. `None` when the function was untracked
    /// (lazy-parsed, worker thread, or dynamic code).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invocations: Option<u64>,
    pub confidence: RuntimeCoverageConfidence,
    pub evidence: RuntimeCoverageEvidence,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<RuntimeCoverageAction>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeCoverageHotPath {
    /// Stable content-hash ID of the form `fallow:hot:<hash>`.
    pub id: String,
    pub path: PathBuf,
    pub function: String,
    pub line: u32,
    /// 1-indexed line the function ends on (inclusive). Mirrors
    /// `fallow_cov_protocol::HotPath::end_line` (added in protocol 0.5).
    /// Older 0.4-shape sidecars omit the field on the wire; serde defaults
    /// to `0`, which the line-overlap filter MUST treat as a single-line
    /// range (`line..=line`) rather than a span.
    pub end_line: u32,
    pub invocations: u64,
    /// Percentile rank over this response's hot-path distribution. `100`
    /// means the busiest, `0` means the quietest function that qualified.
    pub percentile: u8,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<RuntimeCoverageAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCoverageRiskBand {
    Low,
    Medium,
    High,
}

impl RuntimeCoverageRiskBand {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl fmt::Display for RuntimeCoverageRiskBand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeCoverageBlastRadiusEntry {
    /// Stable content-hash ID of the form `fallow:blast:<hash>`.
    pub id: String,
    pub file: PathBuf,
    pub function: String,
    pub line: u32,
    pub caller_count: u32,
    pub caller_count_weighted_by_traffic: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploys_touched: Option<u32>,
    pub risk_band: RuntimeCoverageRiskBand,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeCoverageImportanceEntry {
    /// Stable content-hash ID of the form `fallow:importance:<hash>`.
    pub id: String,
    pub file: PathBuf,
    pub function: String,
    pub line: u32,
    pub invocations: u64,
    pub cyclomatic: u32,
    pub owner_count: u32,
    pub importance_score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RuntimeCoverageReport {
    pub verdict: RuntimeCoverageReportVerdict,
    /// All signals captured by post-processing. Independent of `verdict`,
    /// which is the single most actionable signal under the current
    /// context. Empty when the report is `Clean` and not under license
    /// grace. Order is stable severity-descending.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<RuntimeCoverageSignal>,
    pub summary: RuntimeCoverageSummary,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<RuntimeCoverageFinding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hot_paths: Vec<RuntimeCoverageHotPath>,
    pub blast_radius: Vec<RuntimeCoverageBlastRadiusEntry>,
    pub importance: Vec<RuntimeCoverageImportanceEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watermark: Option<RuntimeCoverageWatermark>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<RuntimeCoverageMessage>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_verdict_display_matches_kebab_case_serde() {
        assert_eq!(RuntimeCoverageReportVerdict::Clean.to_string(), "clean");
        assert_eq!(
            RuntimeCoverageReportVerdict::HotPathTouched.to_string(),
            "hot-path-touched",
        );
        assert_eq!(
            RuntimeCoverageReportVerdict::ColdCodeDetected.to_string(),
            "cold-code-detected",
        );
        assert_eq!(
            RuntimeCoverageReportVerdict::LicenseExpiredGrace.to_string(),
            "license-expired-grace",
        );
        assert_eq!(RuntimeCoverageReportVerdict::Unknown.to_string(), "unknown",);
    }

    #[test]
    fn verdict_display_matches_snake_case_serde() {
        assert_eq!(
            RuntimeCoverageVerdict::SafeToDelete.to_string(),
            "safe_to_delete",
        );
        assert_eq!(
            RuntimeCoverageVerdict::ReviewRequired.to_string(),
            "review_required",
        );
        assert_eq!(
            RuntimeCoverageVerdict::CoverageUnavailable.to_string(),
            "coverage_unavailable",
        );
        assert_eq!(
            RuntimeCoverageVerdict::LowTraffic.to_string(),
            "low_traffic",
        );
        assert_eq!(RuntimeCoverageVerdict::Active.to_string(), "active");
    }

    #[test]
    fn confidence_display_matches_snake_case_serde() {
        assert_eq!(RuntimeCoverageConfidence::VeryHigh.to_string(), "very_high",);
        assert_eq!(RuntimeCoverageConfidence::High.to_string(), "high");
        assert_eq!(RuntimeCoverageConfidence::Medium.to_string(), "medium");
        assert_eq!(RuntimeCoverageConfidence::Low.to_string(), "low");
        assert_eq!(RuntimeCoverageConfidence::None.to_string(), "none");
        assert_eq!(RuntimeCoverageConfidence::Unknown.to_string(), "unknown");
    }

    #[test]
    fn watermark_display_matches_kebab_case_serde() {
        assert_eq!(
            RuntimeCoverageWatermark::TrialExpired.to_string(),
            "trial-expired",
        );
        assert_eq!(
            RuntimeCoverageWatermark::LicenseExpiredGrace.to_string(),
            "license-expired-grace",
        );
    }

    #[test]
    fn action_serializes_kind_as_type() {
        let action = RuntimeCoverageAction {
            kind: "review-deletion".to_owned(),
            description: "Remove the function.".to_owned(),
            auto_fixable: false,
        };
        let value = serde_json::to_value(&action).expect("action should serialize");
        assert_eq!(value["type"], "review-deletion");
        assert!(
            value.get("kind").is_none(),
            "kind should be renamed to type"
        );
    }
}
