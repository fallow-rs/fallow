/// Detailed timing breakdown for the health pipeline.
///
/// Only populated when `--performance` is passed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthTimings {
    /// Wall-clock time of config loading, in milliseconds.
    pub config_ms: f64,
    /// Wall-clock time of file discovery, in milliseconds.
    pub discover_ms: f64,
    /// Wall-clock time of the parse stage, in milliseconds.
    pub parse_ms: f64,
    /// Summed wall-clock time of the actual AST parses across all rayon
    /// workers (the parse stage's CPU cost). `parse_ms` is the stage's
    /// wall-clock time. Observational and non-deterministic; do not assert
    /// against it. `0.0` when `shared_parse` is true (parse was reused).
    pub parse_cpu_ms: f64,
    /// Wall-clock time of complexity scoring, in milliseconds.
    pub complexity_ms: f64,
    /// Wall-clock time of per-file maintainability scoring, in milliseconds.
    pub file_scores_ms: f64,
    /// Wall-clock time of git churn collection, in milliseconds.
    pub git_churn_ms: f64,
    /// True when churn data came from the on-disk churn cache.
    pub git_churn_cache_hit: bool,
    /// Wall-clock time of hotspot ranking, in milliseconds.
    pub hotspots_ms: f64,
    /// Wall-clock time of duplication analysis, in milliseconds.
    pub duplication_ms: f64,
    /// Wall-clock time of refactoring-target ranking, in milliseconds.
    pub targets_ms: f64,
    /// Wall-clock time of the whole health pipeline, in milliseconds.
    pub total_ms: f64,
    /// True when discover + parse were reused from the upstream dead-code
    /// (check) pass in combined mode, so their timings are `0.0` here and
    /// the cost is attributed to the `Pipeline Performance` table instead.
    /// The renderer shows those two stages as `(measured above)`.
    pub shared_parse: bool,
}

/// Framework-specific health detector coverage surfaced for agent consumers.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FrameworkHealthDiagnostics {
    /// Detected framework IDs, sorted and deduplicated.
    pub detected_frameworks: Vec<String>,
    /// Detector coverage for the detected frameworks.
    pub detectors: Vec<FrameworkHealthDetector>,
}

/// Status for one framework-specific health detector.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FrameworkHealthDetector {
    /// Rule or detector ID, matching fallow's stable rule names where possible.
    pub id: String,
    /// Framework ID that made this detector relevant.
    pub framework: String,
    /// Whether the detector ran, was disabled, abstained, or could not be checked.
    pub status: FrameworkHealthDetectorStatus,
    /// Stable reason code for non-active statuses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Detector status codes for framework health observability.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FrameworkHealthDetectorStatus {
    /// Detector ran on this analysis.
    Active,
    /// Detector was turned off by configuration.
    DisabledByConfig,
    /// Detector ran but declined to report for this project.
    Abstained,
    /// Detector applicability could not be evaluated.
    #[allow(dead_code, reason = "reserved for analysis paths that skip a detector")]
    NotChecked,
}
