//! Report rows of `fallow-override-threshold-next-line` comments.

use crate::health_scores::{
    HealthConfiguredThresholds, HealthEffectiveThresholds, ThresholdOverrideDimension,
    ThresholdOverrideMetrics, ThresholdOverrideStatus,
};

pub use fallow_types::inline_threshold::InlineThresholdProblemKind;

/// Report entry for one `fallow-override-threshold-next-line` comment and one
/// threshold dimension. The status values are the values of a
/// `thresholdOverrides` row: `active`, `stale`, `insufficient` and `no_match`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InlineThresholdOverrideState {
    /// Lifecycle state of the comment.
    pub status: ThresholdOverrideStatus,
    /// Threshold dimension this row describes.
    pub dimension: ThresholdOverrideDimension,
    /// Dimensions the function still breaches despite this comment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outstanding: Vec<ThresholdOverrideDimension>,
    /// File that holds the comment.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    /// 1-based line of the comment.
    pub comment_line: u32,
    /// Name of the function on the next line. Absent on `no_match` rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// 1-based line of the function. Absent on `no_match` rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 0-based byte column of the function. Absent on `no_match` rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
    /// Ceilings the comment sets.
    pub configured_thresholds: HealthConfiguredThresholds,
    /// Ceilings in effect after the config and the comment apply.
    pub effective_thresholds: HealthEffectiveThresholds,
    /// Current metrics of the function, when the comment matched one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<ThresholdOverrideMetrics>,
    /// The reason after ` -- ` in the comment.
    pub reason: String,
}

impl InlineThresholdOverrideState {
    /// Render the comment as `path:line`, or the matched function as
    /// `path:line:function`, given the path already formatted for the target
    /// surface.
    #[must_use]
    pub fn target_label(&self, display: &str) -> String {
        match (self.line, self.function.as_deref()) {
            (Some(line), Some(name)) => format!("{display}:{line}:{name}"),
            _ => format!("{display}:{}", self.comment_line),
        }
    }
}

/// One `fallow-override-threshold-next-line` comment with a problem. The
/// comment has no effect.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InlineThresholdOverrideProblem {
    /// File that holds the comment.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    /// 1-based line of the comment.
    pub line: u32,
    /// What is wrong with the comment.
    pub problem: InlineThresholdProblemKind,
    /// The token with the problem, when one token has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}
