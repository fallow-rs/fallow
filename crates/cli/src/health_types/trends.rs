//! Trend types — comparing current run against a saved snapshot.

/// Trend comparison between the current run and a previous snapshot. Shows
/// per-metric deltas with directional indicators.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthTrend {
    /// The snapshot being compared against.
    pub compared_to: TrendPoint,
    /// Per-metric deltas.
    pub metrics: Vec<TrendMetric>,
    /// Number of snapshots found in the snapshot directory.
    pub snapshots_loaded: usize,
    /// Overall direction across all metrics.
    pub overall_direction: TrendDirection,
}

/// A reference to a snapshot used in trend comparison.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TrendPoint {
    /// ISO 8601 timestamp of the snapshot.
    pub timestamp: String,
    /// Git SHA at time of snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    /// Health score from the snapshot (stored, not re-derived).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Letter grade from the snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    /// Coverage model used for CRAP computation in this snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_model: Option<super::CoverageModel>,
    /// Schema version of the compared snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_schema_version: Option<u32>,
}

/// A single metric's trend between two snapshots.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TrendMetric {
    /// Metric identifier (e.g., `"score"`, `"dead_file_pct"`).
    pub name: &'static str,
    /// Human-readable label (e.g., `"Health Score"`, `"Dead Files"`).
    pub label: &'static str,
    /// Previous value (from snapshot).
    pub previous: f64,
    /// Current value (from this run).
    pub current: f64,
    /// Absolute change (current − previous).
    pub delta: f64,
    /// Direction of change.
    pub direction: TrendDirection,
    /// Unit for display (e.g., `"%"`, `""`, `"pts"`).
    pub unit: &'static str,
    /// Raw count from previous snapshot (for JSON consumers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_count: Option<TrendCount>,
    /// Raw count from current run (for JSON consumers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_count: Option<TrendCount>,
}

/// Raw numerator/denominator for a percentage metric.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TrendCount {
    /// The numerator (e.g., dead files count).
    pub value: usize,
    /// The denominator (e.g., total files).
    pub total: usize,
}

/// Direction of a metric's change, semantically (improving/declining/stable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TrendDirection {
    /// The metric moved in a beneficial direction.
    Improving,
    /// The metric moved in a detrimental direction.
    Declining,
    /// The metric stayed within tolerance.
    Stable,
}

impl TrendDirection {
    /// Arrow symbol for terminal output.
    #[must_use]
    pub const fn arrow(self) -> &'static str {
        match self {
            Self::Improving => "\u{2191}", // ↑
            Self::Declining => "\u{2193}", // ↓
            Self::Stable => "\u{2192}",    // →
        }
    }

    /// Human-readable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Improving => "improving",
            Self::Declining => "declining",
            Self::Stable => "stable",
        }
    }
}
