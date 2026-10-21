//! Audit-mode attribution classes of complexity findings, and the measured
//! metrics that grew against the matched base finding.

/// Audit-mode class of a complexity finding against the base snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum AuditFindingAttribution {
    /// No base finding matches, or a metric that the finding exceeds grew. A
    /// CRAP score from estimated coverage does not count as growth.
    Introduced,
    /// A base finding matches and a measured metric grew, but no metric that
    /// the finding exceeds grew.
    Worsened,
    /// A base finding matches and no measured metric grew.
    Inherited,
}

/// A measured metric of a complexity finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WorsenedMetricKind {
    /// Cyclomatic complexity.
    Cyclomatic,
    /// Cognitive complexity.
    Cognitive,
    /// CRAP score. Compared only when both sides have coverage data.
    Crap,
    /// Lines of code in the function body.
    LineCount,
}

impl WorsenedMetricKind {
    /// Short label for human output.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cyclomatic => "cyclomatic",
            Self::Cognitive => "cognitive",
            Self::Crap => "CRAP",
            Self::LineCount => "lines",
        }
    }
}

/// One measured metric that is higher at head than in the matched base
/// finding.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorsenedMetric {
    /// The metric.
    pub metric: WorsenedMetricKind,
    /// Value in the matched base finding.
    #[serde(serialize_with = "serialize_metric_value")]
    pub base: f64,
    /// Value in the head finding. Always higher than `base`.
    #[serde(serialize_with = "serialize_metric_value")]
    pub head: f64,
}

impl WorsenedMetric {
    /// Human form, for example `cyclomatic 15 -> 40`.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "{} {} -> {}",
            self.metric.label(),
            format_metric_value(self.base),
            format_metric_value(self.head)
        )
    }
}

/// The human form of a list of grown metrics, joined with `, `.
#[must_use]
pub fn describe_worsened_metrics(metrics: &[WorsenedMetric]) -> String {
    metrics
        .iter()
        .map(WorsenedMetric::describe)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Largest magnitude that an `f64` holds as an exact whole number.
const MAX_EXACT_WHOLE: f64 = 9_007_199_254_740_992.0;

fn is_whole(value: f64) -> bool {
    value.fract() == 0.0 && value.abs() < MAX_EXACT_WHOLE
}

/// Format a metric value: whole numbers without a fraction, CRAP scores with
/// one decimal.
#[must_use]
pub fn format_metric_value(value: f64) -> String {
    if is_whole(value) {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

/// Write whole metric values as JSON integers, so `15` does not read `15.0`.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde serialize_with passes the field by reference"
)]
fn serialize_metric_value<S: serde::Serializer>(
    value: &f64,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if is_whole(*value) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "is_whole keeps the value whole and inside the exact i64 range"
        )]
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_values_serialize_as_integers() {
        let metric = WorsenedMetric {
            metric: WorsenedMetricKind::LineCount,
            base: 80.0,
            head: 300.0,
        };
        let json = serde_json::to_value(metric).expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({"metric": "line_count", "base": 80, "head": 300})
        );
    }

    #[test]
    fn crap_values_keep_their_fraction() {
        let metric = WorsenedMetric {
            metric: WorsenedMetricKind::Crap,
            base: 31.5,
            head: 42.25,
        };
        let json = serde_json::to_value(metric).expect("serialize");
        assert_eq!(json["base"], 31.5);
        assert_eq!(metric.describe(), "CRAP 31.5 -> 42.2");
    }

    #[test]
    fn describe_joins_metrics() {
        let metrics = [
            WorsenedMetric {
                metric: WorsenedMetricKind::Cyclomatic,
                base: 15.0,
                head: 40.0,
            },
            WorsenedMetric {
                metric: WorsenedMetricKind::LineCount,
                base: 80.0,
                head: 300.0,
            },
        ];
        assert_eq!(
            describe_worsened_metrics(&metrics),
            "cyclomatic 15 -> 40, lines 80 -> 300"
        );
    }
}
