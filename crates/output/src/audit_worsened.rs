//! Audit-mode attribution classes of complexity findings, and the measured
//! metrics that grew against the matched base finding.

/// Audit-mode class of a complexity finding against the base snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum AuditFindingAttribution {
    /// No base finding matches, or a grown metric now exceeds a threshold
    /// that the base finding did not exceed, or the growth of an exceeded
    /// metric moved the finding to a higher severity tier. A CRAP score from
    /// estimated coverage does not count as growth.
    Introduced,
    /// A base finding matches and a measured metric grew, but the finding is
    /// not introduced. Growth of an exceeded metric in the same severity tier
    /// is worsened.
    Worsened,
    /// A base finding matches and no measured metric grew.
    Inherited,
}

/// Audit-mode reason why a complexity finding does not gate the verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum AuditGateExemption {
    /// The finding is in a test file: a spec, a test directory, an end-to-end
    /// suite, a mock or a fixture. Its gate severity is `warn`.
    TestFile,
}

impl AuditGateExemption {
    /// The JSON value, also used in compact output and SARIF properties.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::TestFile => "test-file",
        }
    }
}

/// Audit facts of one finding that an inline review comment shows.
///
/// The CodeClimate builder sets the attribution and the gate exemption. The
/// audit render sets `gate` from the gate policy of the run. The CodeClimate
/// wire shape does not carry these facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditReviewContext {
    /// Class of the finding against the base snapshot.
    pub attribution: AuditFindingAttribution,
    /// Reason why the finding never gates, when it has one.
    pub gate_exemption: Option<AuditGateExemption>,
    /// Effect of the finding on the gate. `None` when the gate policy of the
    /// run is not known: the comment then shows only the attribution.
    pub gate: Option<AuditReviewGate>,
}

/// Effect of one audit finding on the gate of the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditReviewGate {
    /// The finding counts in the gate.
    Gated,
    /// `--gate new-only` does not count an inherited finding.
    NotGatedNewOnly,
    /// `--worsened warn` gives a warning for a worsened finding, not a failure.
    NotGatedWorsenedWarn,
    /// `--worsened off` does not count a worsened finding.
    NotGatedWorsenedOff,
    /// A finding in a test file does not gate.
    NotGatedTestFile,
}

impl AuditReviewContext {
    /// The label text after the severity, for example
    /// `inherited · not gated (`--gate new-only`)`.
    #[must_use]
    pub fn label(&self) -> String {
        let class = match self.attribution {
            AuditFindingAttribution::Introduced => "introduced",
            AuditFindingAttribution::Worsened => "worsened",
            AuditFindingAttribution::Inherited => "inherited",
        };
        let reason = match self.gate {
            None | Some(AuditReviewGate::Gated) => None,
            Some(AuditReviewGate::NotGatedNewOnly) => Some("`--gate new-only`"),
            Some(AuditReviewGate::NotGatedWorsenedWarn) => Some("`--worsened warn`"),
            Some(AuditReviewGate::NotGatedWorsenedOff) => Some("`--worsened off`"),
            Some(AuditReviewGate::NotGatedTestFile) => Some("test file"),
        };
        reason.map_or_else(
            || class.to_owned(),
            |reason| format!("{class} \u{00b7} not gated ({reason})"),
        )
    }
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

/// Metric values of the base finding that an inherited complexity finding
/// matches. The JSON output does not carry these values, so only a live audit
/// run has them. A `report --from` re-render of a saved audit has no base
/// values.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AuditBaseMetrics {
    /// Cyclomatic complexity of the base finding.
    pub cyclomatic: u16,
    /// Cognitive complexity of the base finding.
    pub cognitive: u16,
    /// CRAP score of the base finding, when coverage data exists.
    pub crap: Option<f64>,
    /// Lines of code of the base finding.
    pub line_count: u32,
}

impl AuditBaseMetrics {
    /// The human form of each metric that `head` exceeds, with the base value
    /// and the head value, for example `cognitive 21 -> 21`. A CRAP score
    /// shows only when both sides have one. `None` when no exceeded metric
    /// has a base value.
    #[must_use]
    pub fn describe_exceeded(&self, head: &crate::ComplexityViolation) -> Option<String> {
        let exceeded = head.exceeded;
        let mut parts = Vec::new();
        let mut push = |kind: WorsenedMetricKind, base: f64, head: f64| {
            parts.push(format!(
                "{} {} -> {}",
                kind.label(),
                format_metric_value(base),
                format_metric_value(head)
            ));
        };
        if exceeded.includes_cyclomatic() {
            push(
                WorsenedMetricKind::Cyclomatic,
                f64::from(self.cyclomatic),
                f64::from(head.cyclomatic),
            );
        }
        if exceeded.includes_cognitive() {
            push(
                WorsenedMetricKind::Cognitive,
                f64::from(self.cognitive),
                f64::from(head.cognitive),
            );
        }
        if exceeded.includes_crap()
            && let (Some(base), Some(head)) = (self.crap, head.crap)
        {
            push(WorsenedMetricKind::Crap, base, head);
        }
        if exceeded.is_file_lines() {
            push(
                WorsenedMetricKind::LineCount,
                f64::from(self.line_count),
                f64::from(head.line_count),
            );
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
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

    fn violation(
        exceeded: crate::ExceededThreshold,
        crap: Option<f64>,
    ) -> crate::ComplexityViolation {
        crate::ComplexityViolation {
            path: std::path::PathBuf::from("src/a.ts"),
            name: "work".to_owned(),
            line: 1,
            col: 0,
            cyclomatic: 19,
            standard_cyclomatic: None,
            cognitive: 18,
            line_count: 40,
            param_count: 1,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded,
            effective_severity: None,
            severity: crate::FindingSeverity::High,
            mutation: None,
            coverage_pct: None,
            crap,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
            max_file_lines: None,
        }
    }

    const BASE: AuditBaseMetrics = AuditBaseMetrics {
        cyclomatic: 23,
        cognitive: 18,
        crap: Some(31.5),
        line_count: 60,
    };

    #[test]
    fn base_metrics_name_only_the_exceeded_metrics() {
        let head = violation(crate::ExceededThreshold::Cyclomatic, None);
        assert_eq!(
            BASE.describe_exceeded(&head).as_deref(),
            Some("cyclomatic 23 -> 19")
        );
        let head = violation(crate::ExceededThreshold::All, Some(30.0));
        assert_eq!(
            BASE.describe_exceeded(&head).as_deref(),
            Some("cyclomatic 23 -> 19, cognitive 18 -> 18, CRAP 31.5 -> 30")
        );
        let head = violation(crate::ExceededThreshold::FileLines, None);
        assert_eq!(
            BASE.describe_exceeded(&head).as_deref(),
            Some("lines 60 -> 40")
        );
    }

    #[test]
    fn base_metrics_skip_a_crap_score_that_one_side_lacks() {
        let head = violation(crate::ExceededThreshold::Crap, None);
        assert_eq!(BASE.describe_exceeded(&head), None);
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
