//! Refactoring target types, recommendations, effort estimates, and evidence.

/// Adaptive thresholds used for refactoring target scoring.
///
/// Derived from the project's metric distribution (percentile-based with floors).
/// Exposed in JSON output so consumers can interpret scores in context.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    clippy::struct_field_names,
    reason = "triggered in bin but not lib — #[expect] would be unfulfilled in lib"
)]
pub struct TargetThresholds {
    /// Fan-in saturation point for priority formula (p95, floor 5).
    pub fan_in_p95: f64,
    /// Fan-in moderate threshold for contributing factors (p75, floor 3).
    pub fan_in_p75: f64,
    /// Fan-out saturation point for priority formula (p95, floor 8).
    pub fan_out_p95: f64,
    /// Fan-out high threshold for rules and contributing factors (p90, floor 5).
    pub fan_out_p90: usize,
}

/// Category of refactoring recommendation.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RecommendationCategory {
    /// Actively-changing file with growing complexity — highest urgency.
    UrgentChurnComplexity,
    /// File participates in an import cycle with significant blast radius.
    BreakCircularDependency,
    /// High fan-in + high complexity — changes here ripple widely.
    SplitHighImpact,
    /// Majority of exports are unused — reduce surface area.
    RemoveDeadCode,
    /// Contains functions with very high cognitive complexity.
    ExtractComplexFunctions,
    /// Excessive imports reduce testability and increase coupling.
    ExtractDependencies,
    /// Multiple complex functions lack test dependency path.
    AddTestCoverage,
}

impl RecommendationCategory {
    /// Human-readable label for terminal output.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::UrgentChurnComplexity => "churn+complexity",
            Self::BreakCircularDependency => "circular dependency",
            Self::SplitHighImpact => "high impact",
            Self::RemoveDeadCode => "dead code",
            Self::ExtractComplexFunctions => "complexity",
            Self::ExtractDependencies => "coupling",
            Self::AddTestCoverage => "untested risk",
        }
    }

    /// Machine-parseable label for compact output (no spaces).
    #[must_use]
    pub const fn compact_label(&self) -> &'static str {
        match self {
            Self::UrgentChurnComplexity => "churn_complexity",
            Self::BreakCircularDependency => "circular_dep",
            Self::SplitHighImpact => "high_impact",
            Self::RemoveDeadCode => "dead_code",
            Self::ExtractComplexFunctions => "complexity",
            Self::ExtractDependencies => "coupling",
            Self::AddTestCoverage => "untested_risk",
        }
    }
}

/// A contributing factor that triggered or strengthened a recommendation.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContributingFactor {
    /// Metric name (matches JSON field names: `"fan_in"`, `"dead_code_ratio"`, etc.).
    pub metric: &'static str,
    /// Raw metric value for programmatic use.
    pub value: f64,
    /// Threshold that was exceeded.
    pub threshold: f64,
    /// Human-readable explanation.
    pub detail: String,
}

/// A ranked refactoring recommendation for a file.
///
/// ## Priority Formula
///
/// ```text
/// priority = min(density, 1) × 30 + hotspot_boost × 25 + dead_code × 20 + fan_in_norm × 15 + fan_out_norm × 10
/// ```
///
/// Fan-in and fan-out normalization uses adaptive percentile-based thresholds
/// (p95 of the project distribution, with floors) instead of fixed constants.
///
/// ## Efficiency (default sort)
///
/// ```text
/// efficiency = priority / effort_numeric   (Low=1, Medium=2, High=3)
/// ```
///
/// Surfaces quick wins: high-priority, low-effort targets rank first.
/// Effort estimate for a refactoring target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EffortEstimate {
    /// Small file, few functions, low fan-in — quick to address.
    Low,
    /// Moderate size or coupling — needs planning.
    Medium,
    /// Large file, many functions, or high fan-in — significant effort.
    High,
}

impl EffortEstimate {
    /// Human-readable label for terminal output.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Numeric value for arithmetic (efficiency = priority / effort).
    #[must_use]
    pub const fn numeric(&self) -> f64 {
        match self {
            Self::Low => 1.0,
            Self::Medium => 2.0,
            Self::High => 3.0,
        }
    }
}

/// Confidence level for a refactoring recommendation.
///
/// Based on the data source reliability:
/// - **High**: deterministic graph/AST analysis (dead code, circular deps, complexity)
/// - **Medium**: heuristic thresholds (fan-in/fan-out coupling)
/// - **Low**: depends on git history quality (churn-based recommendations)
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Recommendation based on deterministic analysis (graph, AST).
    High,
    /// Recommendation based on heuristic thresholds.
    Medium,
    /// Recommendation depends on external data quality (git history).
    Low,
}

impl Confidence {
    /// Human-readable label for terminal output.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// Evidence linking a target back to specific analysis data.
///
/// Provides enough detail for an AI agent to act on a recommendation
/// without a second tool call.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TargetEvidence {
    /// Names of unused exports (populated for `RemoveDeadCode` targets).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_exports: Vec<String>,
    /// Complex functions with line numbers and cognitive scores (populated for `ExtractComplexFunctions`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub complex_functions: Vec<EvidenceFunction>,
    /// Files forming the import cycle (populated for `BreakCircularDependency` targets).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cycle_path: Vec<String>,
}

/// A function referenced in target evidence.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvidenceFunction {
    /// Function name.
    pub name: String,
    /// 1-based line number.
    pub line: u32,
    /// Cognitive complexity score.
    pub cognitive: u16,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RefactoringTarget {
    /// Absolute file path (stripped to relative in output).
    pub path: std::path::PathBuf,
    /// Priority score (0–100, higher = more urgent).
    pub priority: f64,
    /// Efficiency score (priority / effort). Higher = better quick-win value.
    /// Surfaces low-effort, high-priority targets first.
    pub efficiency: f64,
    /// One-line actionable recommendation.
    pub recommendation: String,
    /// Recommendation category for tooling/filtering.
    pub category: RecommendationCategory,
    /// Estimated effort to address this target.
    pub effort: EffortEstimate,
    /// Confidence in this recommendation based on data source reliability.
    pub confidence: Confidence,
    /// Contributing factors that triggered this recommendation. Empty array
    /// omitted from JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub factors: Vec<ContributingFactor>,
    /// Structured evidence linking to specific analysis data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<TargetEvidence>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- RecommendationCategory ---

    #[test]
    fn category_labels_are_non_empty() {
        let categories = [
            RecommendationCategory::UrgentChurnComplexity,
            RecommendationCategory::BreakCircularDependency,
            RecommendationCategory::SplitHighImpact,
            RecommendationCategory::RemoveDeadCode,
            RecommendationCategory::ExtractComplexFunctions,
            RecommendationCategory::ExtractDependencies,
            RecommendationCategory::AddTestCoverage,
        ];
        for cat in &categories {
            assert!(!cat.label().is_empty(), "{cat:?} should have a label");
        }
    }

    #[test]
    fn category_labels_are_unique() {
        let categories = [
            RecommendationCategory::UrgentChurnComplexity,
            RecommendationCategory::BreakCircularDependency,
            RecommendationCategory::SplitHighImpact,
            RecommendationCategory::RemoveDeadCode,
            RecommendationCategory::ExtractComplexFunctions,
            RecommendationCategory::ExtractDependencies,
            RecommendationCategory::AddTestCoverage,
        ];
        let labels: Vec<&str> = categories
            .iter()
            .map(RecommendationCategory::label)
            .collect();
        let unique: rustc_hash::FxHashSet<&&str> = labels.iter().collect();
        assert_eq!(labels.len(), unique.len(), "category labels must be unique");
    }

    // --- Serde serialization ---

    #[test]
    fn category_serializes_as_snake_case() {
        let json = serde_json::to_string(&RecommendationCategory::UrgentChurnComplexity).unwrap();
        assert_eq!(json, r#""urgent_churn_complexity""#);

        let json = serde_json::to_string(&RecommendationCategory::BreakCircularDependency).unwrap();
        assert_eq!(json, r#""break_circular_dependency""#);
    }

    #[test]
    fn refactoring_target_skips_empty_factors() {
        let target = RefactoringTarget {
            path: std::path::PathBuf::from("/src/foo.ts"),
            priority: 75.0,
            efficiency: 75.0,
            recommendation: "Test recommendation".into(),
            category: RecommendationCategory::RemoveDeadCode,
            effort: EffortEstimate::Low,
            confidence: Confidence::High,
            factors: vec![],
            evidence: None,
        };
        let json = serde_json::to_string(&target).unwrap();
        assert!(!json.contains("factors"));
        assert!(!json.contains("evidence"));
    }

    #[test]
    fn effort_numeric_values() {
        assert!((EffortEstimate::Low.numeric() - 1.0).abs() < f64::EPSILON);
        assert!((EffortEstimate::Medium.numeric() - 2.0).abs() < f64::EPSILON);
        assert!((EffortEstimate::High.numeric() - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_labels_are_non_empty() {
        let levels = [Confidence::High, Confidence::Medium, Confidence::Low];
        for level in &levels {
            assert!(!level.label().is_empty(), "{level:?} should have a label");
        }
    }

    #[test]
    fn confidence_serializes_as_snake_case() {
        let json = serde_json::to_string(&Confidence::High).unwrap();
        assert_eq!(json, r#""high""#);
        let json = serde_json::to_string(&Confidence::Medium).unwrap();
        assert_eq!(json, r#""medium""#);
        let json = serde_json::to_string(&Confidence::Low).unwrap();
        assert_eq!(json, r#""low""#);
    }

    #[test]
    fn contributing_factor_serializes_correctly() {
        let factor = ContributingFactor {
            metric: "fan_in",
            value: 15.0,
            threshold: 10.0,
            detail: "15 files depend on this".into(),
        };
        let json = serde_json::to_string(&factor).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["metric"], "fan_in");
        assert_eq!(parsed["value"], 15.0);
        assert_eq!(parsed["threshold"], 10.0);
    }

    // --- RecommendationCategory compact_labels ---

    #[test]
    fn category_compact_labels_are_non_empty() {
        let categories = [
            RecommendationCategory::UrgentChurnComplexity,
            RecommendationCategory::BreakCircularDependency,
            RecommendationCategory::SplitHighImpact,
            RecommendationCategory::RemoveDeadCode,
            RecommendationCategory::ExtractComplexFunctions,
            RecommendationCategory::ExtractDependencies,
            RecommendationCategory::AddTestCoverage,
        ];
        for cat in &categories {
            assert!(
                !cat.compact_label().is_empty(),
                "{cat:?} should have a compact_label"
            );
        }
    }

    #[test]
    fn category_compact_labels_are_unique() {
        let categories = [
            RecommendationCategory::UrgentChurnComplexity,
            RecommendationCategory::BreakCircularDependency,
            RecommendationCategory::SplitHighImpact,
            RecommendationCategory::RemoveDeadCode,
            RecommendationCategory::ExtractComplexFunctions,
            RecommendationCategory::ExtractDependencies,
            RecommendationCategory::AddTestCoverage,
        ];
        let labels: Vec<&str> = categories
            .iter()
            .map(RecommendationCategory::compact_label)
            .collect();
        let unique: rustc_hash::FxHashSet<&&str> = labels.iter().collect();
        assert_eq!(labels.len(), unique.len(), "compact labels must be unique");
    }

    #[test]
    fn category_compact_labels_have_no_spaces() {
        let categories = [
            RecommendationCategory::UrgentChurnComplexity,
            RecommendationCategory::BreakCircularDependency,
            RecommendationCategory::SplitHighImpact,
            RecommendationCategory::RemoveDeadCode,
            RecommendationCategory::ExtractComplexFunctions,
            RecommendationCategory::ExtractDependencies,
            RecommendationCategory::AddTestCoverage,
        ];
        for cat in &categories {
            assert!(
                !cat.compact_label().contains(' '),
                "compact_label for {:?} should not contain spaces: '{}'",
                cat,
                cat.compact_label()
            );
        }
    }

    // --- EffortEstimate ---

    #[test]
    fn effort_labels_are_non_empty() {
        let efforts = [
            EffortEstimate::Low,
            EffortEstimate::Medium,
            EffortEstimate::High,
        ];
        for effort in &efforts {
            assert!(!effort.label().is_empty(), "{effort:?} should have a label");
        }
    }

    #[test]
    fn effort_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&EffortEstimate::Low).unwrap(),
            r#""low""#
        );
        assert_eq!(
            serde_json::to_string(&EffortEstimate::Medium).unwrap(),
            r#""medium""#
        );
        assert_eq!(
            serde_json::to_string(&EffortEstimate::High).unwrap(),
            r#""high""#
        );
    }

    // --- TargetEvidence ---

    #[test]
    fn target_evidence_skips_empty_fields() {
        let evidence = TargetEvidence {
            unused_exports: vec![],
            complex_functions: vec![],
            cycle_path: vec![],
        };
        let json = serde_json::to_string(&evidence).unwrap();
        assert!(!json.contains("unused_exports"));
        assert!(!json.contains("complex_functions"));
        assert!(!json.contains("cycle_path"));
    }

    #[test]
    fn target_evidence_with_data() {
        let evidence = TargetEvidence {
            unused_exports: vec!["foo".to_string(), "bar".to_string()],
            complex_functions: vec![EvidenceFunction {
                name: "processData".into(),
                line: 42,
                cognitive: 30,
            }],
            cycle_path: vec![],
        };
        let json = serde_json::to_string(&evidence).unwrap();
        assert!(json.contains("unused_exports"));
        assert!(json.contains("complex_functions"));
        assert!(json.contains("processData"));
        assert!(!json.contains("cycle_path"));
    }
}
