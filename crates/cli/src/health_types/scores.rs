//! Score types, grade boundaries, file health metrics, and findings.

pub const HOTSPOT_SCORE_THRESHOLD: f64 = 50.0;

pub const COGNITIVE_EXTRACTION_THRESHOLD: u16 = 30;

pub const DEFAULT_COGNITIVE_HIGH: u16 = 25;

pub const DEFAULT_COGNITIVE_CRITICAL: u16 = 40;

pub const DEFAULT_CYCLOMATIC_HIGH: u16 = 30;

pub const DEFAULT_CYCLOMATIC_CRITICAL: u16 = 50;

/// Minimum lines of code for full complexity density weight in the MI formula.
pub const MI_DENSITY_MIN_LINES: f64 = 50.0;

pub const HEALTH_SCORE_FORMULA_VERSION: u32 = 2;

/// `skip_serializing_if` predicate: drop a `u16` field from JSON when zero, so
/// the React descriptive counts never bloat non-React complexity findings.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires a by-reference predicate"
)]
fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthScore {
    pub formula_version: u32,
    pub score: f64,
    pub grade: &'static str,
    pub penalties: HealthScorePenalties,
}

/// Per-component penalty breakdown for the health score.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthScorePenalties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_files: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_exports: Option<f64>,
    pub complexity: f64,
    pub p90_complexity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintainability: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotspots: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_deps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circular_deps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coupling: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplication: Option<f64>,
    /// Small capped penalty for prop-drilling chains. `None` unless the opt-in
    /// `prop-drilling` rule is enabled; sized like the coupling penalty (~5pt cap).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prop_drilling: Option<f64>,
}

/// Map a numeric score (0–100) to a letter grade.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "score is 0-100, fits in u32"
)]
pub const fn letter_grade(score: f64) -> &'static str {
    let s = score as u32;
    if s >= 85 {
        "A"
    } else if s >= 70 {
        "B"
    } else if s >= 55 {
        "C"
    } else if s >= 40 {
        "D"
    } else {
        "F"
    }
}

/// Coverage tier classification for CRAP findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageTier {
    None,
    Partial,
    High,
}

/// Coverage percentage at or above which a function is classified as `High`.
const HIGH_COVERAGE_WATERMARK: f64 = 70.0;

impl CoverageTier {
    /// Bucket a numeric coverage percentage `[0, 100]` into a tier.
    #[must_use]
    pub fn from_pct(pct: f64) -> Self {
        if pct <= 0.0 {
            Self::None
        } else if pct >= HIGH_COVERAGE_WATERMARK {
            Self::High
        } else {
            Self::Partial
        }
    }
}

/// Provenance of a CRAP finding's coverage signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageSource {
    Istanbul,
    Estimated,
    EstimatedComponentInherited,
}

/// Whether CRAP findings in the report used one coverage-source kind or a mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageSourceConsistency {
    Uniform,
    Mixed,
}

/// Summarise the coverage-source provenance attached to CRAP findings.
#[must_use]
pub fn summarize_coverage_source_consistency(
    sources: impl IntoIterator<Item = CoverageSource>,
) -> Option<CoverageSourceConsistency> {
    let mut first = None;
    for source in sources {
        match first {
            None => first = Some(source),
            Some(existing) if existing != source => {
                return Some(CoverageSourceConsistency::Mixed);
            }
            Some(_) => {}
        }
    }
    first.map(|_| CoverageSourceConsistency::Uniform)
}

/// Per-component React hook profile derived from the cached `hook_uses` IR at
/// the health layer. Descriptive context that refines the bare
/// [`ComplexityViolation::react_hook_count`] headline with a per-kind breakdown
/// and the maximum `useEffect` dependency-array arity.
///
/// Attached only when at least one component-scope hook was attributed to the
/// function, so non-React findings stay byte-identical on the wire. The
/// per-kind counts cover hooks recorded by the React visitor (calls inside an
/// identified component); a `use*` call inside a plain helper function is
/// counted in `react_hook_count` but NOT here, so the breakdown can sum to LESS
/// than `react_hook_count`. `react_hook_count` remains the headline total; this
/// is an additive refinement.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReactHookProfile {
    /// `useState` call count attributed to this component.
    pub state: u16,
    /// `useEffect` call count attributed to this component.
    pub effect: u16,
    /// `useMemo` call count attributed to this component.
    pub memo: u16,
    /// `useCallback` call count attributed to this component.
    pub callback: u16,
    /// Custom `use*` hook call count attributed to this component.
    pub custom: u16,
    /// Largest `useEffect` dependency-array arity over the attributed effects
    /// that carry a literal deps array. `None` when no attributed `useEffect`
    /// had a literal array (absent or non-literal deps; ADR-001 syntactic-only,
    /// so absence does NOT mean "no coupling").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_effect_dep_arity: Option<u32>,
}

impl ReactHookProfile {
    /// Total component-scope hooks attributed (state + effect + memo + callback
    /// + custom). Used to gate whether the profile is surfaced at all.
    #[must_use]
    pub fn total(&self) -> u16 {
        self.state
            .saturating_add(self.effect)
            .saturating_add(self.memo)
            .saturating_add(self.callback)
            .saturating_add(self.custom)
    }

    /// `true` when no hook was attributed, so the profile carries no signal.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// Inner complexity-violation payload.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComplexityViolation {
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    pub name: String,
    pub line: u32,
    pub col: u32,
    pub cyclomatic: u16,
    pub cognitive: u16,
    pub line_count: u32,
    pub param_count: u8,
    /// Number of React hook calls in this function's body (`useState` /
    /// `useEffect` / `useMemo` / `useCallback` / custom `use*`). Descriptive
    /// hotspot context for React components; omitted when zero (non-React).
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub react_hook_count: u16,
    /// Deepest JSX element nesting reached in this function's body. Descriptive
    /// hotspot context; omitted when zero (renders no JSX).
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub react_jsx_max_depth: u16,
    /// Number of props destructured from this component's first parameter.
    /// Descriptive hotspot context; omitted when zero.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub react_prop_count: u16,
    /// Per-kind React hook breakdown (state/effect/memo/callback/custom) plus
    /// the max `useEffect` dependency-array arity, derived from the cached
    /// `hook_uses` IR at the health layer. Descriptive refinement of
    /// `react_hook_count`; present only when at least one component-scope hook
    /// was attributed, so non-React findings stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub react_hook_profile: Option<ReactHookProfile>,
    pub exceeded: ExceededThreshold,
    pub severity: FindingSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_tier: Option<CoverageTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_source: Option<CoverageSource>,
    #[serde(
        default,
        serialize_with = "fallow_types::serde_path::serialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub inherited_from: Option<std::path::PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_rollup: Option<ComponentRollup>,
    /// Per-decision-point complexity breakdown explaining WHICH constructs drove
    /// the cyclomatic and cognitive scores. Populated only when the caller opts
    /// in via `health --complexity-breakdown`; empty (and omitted from JSON)
    /// otherwise so default and CI output stay lean.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributions: Vec<fallow_types::extract::ComplexityContribution>,
    /// Resolved thresholds used for this finding when a config override changed
    /// at least one ceiling. Omitted for findings using global thresholds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_thresholds: Option<HealthEffectiveThresholds>,
    /// Source of the effective thresholds. Omitted when thresholds are global.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_source: Option<ThresholdSource>,
}

/// Resolved thresholds used to evaluate a health finding.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    clippy::struct_field_names,
    reason = "target-dependent clippy lint; wire fields mirror max_* config keys"
)]
pub struct HealthEffectiveThresholds {
    pub max_cyclomatic: u16,
    pub max_cognitive: u16,
    pub max_crap: f64,
}

/// Threshold values configured by a single override entry.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    clippy::struct_field_names,
    reason = "target-dependent clippy lint; wire fields mirror max_* config keys"
)]
pub struct HealthConfiguredThresholds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cyclomatic: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cognitive: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_crap: Option<f64>,
}

/// Source for a finding's effective thresholds.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThresholdSource {
    Override,
}

/// Lifecycle state for a configured threshold override.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThresholdOverrideStatus {
    Active,
    Stale,
    NoMatch,
}

/// Current complexity metrics for a matched threshold override entry.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ThresholdOverrideMetrics {
    pub cyclomatic: u16,
    pub cognitive: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crap: Option<f64>,
}

/// Report entry describing whether a threshold override is active, stale, or
/// no longer matching any analyzed file or function.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ThresholdOverrideState {
    pub status: ThresholdOverrideStatus,
    pub override_index: usize,
    #[serde(
        default,
        serialize_with = "fallow_types::serde_path::serialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub path: Option<std::path::PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    pub configured_thresholds: HealthConfiguredThresholds,
    pub effective_thresholds: HealthEffectiveThresholds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<ThresholdOverrideMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComponentRollup {
    pub component: String,
    pub class_worst_function: String,
    pub class_cyclomatic: u16,
    pub class_cognitive: u16,
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub template_path: std::path::PathBuf,
    pub template_cyclomatic: u16,
    pub template_cognitive: u16,
}

/// Which complexity threshold was exceeded.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExceededThreshold {
    /// Only cyclomatic exceeded.
    Cyclomatic,
    /// Only cognitive exceeded.
    Cognitive,
    /// Both cyclomatic and cognitive exceeded (may or may not also exceed CRAP).
    Both,
    /// Only CRAP exceeded (cyclomatic and cognitive are under threshold).
    Crap,
    /// Cyclomatic and CRAP exceeded.
    CyclomaticCrap,
    /// Cognitive and CRAP exceeded.
    CognitiveCrap,
    /// Cyclomatic, cognitive, and CRAP all exceeded.
    All,
}

impl ExceededThreshold {
    /// Classify a finding from which individual thresholds were exceeded.
    ///
    /// Panics if all three bools are false; callers are expected to only
    /// construct an `ExceededThreshold` for findings that exceeded at least
    /// one threshold.
    #[must_use]
    pub fn from_bools(cyclomatic: bool, cognitive: bool, crap: bool) -> Self {
        match (cyclomatic, cognitive, crap) {
            (true, true, true) => Self::All,
            (true, true, false) => Self::Both,
            (true, false, true) => Self::CyclomaticCrap,
            (false, true, true) => Self::CognitiveCrap,
            (true, false, false) => Self::Cyclomatic,
            (false, true, false) => Self::Cognitive,
            (false, false, true) => Self::Crap,
            (false, false, false) => {
                unreachable!("ExceededThreshold requires at least one threshold exceeded")
            }
        }
    }

    /// True when the cyclomatic threshold contributed to the finding.
    #[must_use]
    pub const fn includes_cyclomatic(self) -> bool {
        matches!(
            self,
            Self::Cyclomatic | Self::Both | Self::CyclomaticCrap | Self::All
        )
    }

    /// True when the cognitive threshold contributed to the finding.
    #[must_use]
    pub const fn includes_cognitive(self) -> bool {
        matches!(
            self,
            Self::Cognitive | Self::Both | Self::CognitiveCrap | Self::All
        )
    }

    /// True when the CRAP threshold contributed to the finding.
    #[must_use]
    pub const fn includes_crap(self) -> bool {
        matches!(
            self,
            Self::Crap | Self::CyclomaticCrap | Self::CognitiveCrap | Self::All
        )
    }
}

/// Severity tier indicating how far a function exceeds complexity thresholds.
///
/// Determined by the highest tier reached across both cognitive and cyclomatic
/// scores. Default thresholds: cognitive 25/40, cyclomatic 30/50.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, clap::ValueEnum)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    /// Above threshold but manageable (cognitive < 25 or cyclomatic < 30).
    Moderate,
    /// Recommended for extraction (cognitive 25-39 or cyclomatic 30-49).
    High,
    /// Immediate extraction candidate (cognitive >= 40 or cyclomatic >= 50).
    Critical,
}

/// CRAP score threshold for "high" severity. CC=7 untested -> 56, CC=10 -> 110.
pub const DEFAULT_CRAP_HIGH: f64 = 50.0;

/// CRAP score threshold for "critical" severity. CC=10 untested gives 110,
/// CC=12 untested gives 156; 100 lands between the two and flags genuinely
/// dangerous combinations of high complexity and low coverage.
pub const DEFAULT_CRAP_CRITICAL: f64 = 100.0;

/// Compute the severity tier for a complexity finding.
///
/// Uses the highest tier reached across cognitive, cyclomatic, and CRAP
/// scores. Pass `None` for `crap` to skip the CRAP contribution (used when
/// the finding was triggered by complexity thresholds only).
pub fn compute_finding_severity(
    cognitive: u16,
    cyclomatic: u16,
    crap: Option<f64>,
    cognitive_high: u16,
    cognitive_critical: u16,
    cyclomatic_high: u16,
    cyclomatic_critical: u16,
) -> FindingSeverity {
    let cog = if cognitive >= cognitive_critical {
        FindingSeverity::Critical
    } else if cognitive >= cognitive_high {
        FindingSeverity::High
    } else {
        FindingSeverity::Moderate
    };

    let cyc = if cyclomatic >= cyclomatic_critical {
        FindingSeverity::Critical
    } else if cyclomatic >= cyclomatic_high {
        FindingSeverity::High
    } else {
        FindingSeverity::Moderate
    };

    let crap_sev = crap.map_or(FindingSeverity::Moderate, |c| {
        if c >= DEFAULT_CRAP_CRITICAL {
            FindingSeverity::Critical
        } else if c >= DEFAULT_CRAP_HIGH {
            FindingSeverity::High
        } else {
            FindingSeverity::Moderate
        }
    });

    cog.max(cyc).max(crap_sev)
}

/// A function exceeding the very-high-risk size threshold (>60 LOC).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LargeFunctionEntry {
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    pub name: String,
    pub line: u32,
    pub line_count: u32,
}

/// Summary statistics for the health report.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthSummary {
    pub files_analyzed: usize,
    pub functions_analyzed: usize,
    pub functions_above_threshold: usize,
    pub max_cyclomatic_threshold: u16,
    pub max_cognitive_threshold: u16,
    pub max_crap_threshold: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_scored: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_maintainability: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_model: Option<CoverageModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_source_consistency: Option<CoverageSourceConsistency>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub istanbul_matched: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub istanbul_total: Option<usize>,
    pub severity_critical_count: usize,
    pub severity_high_count: usize,
    pub severity_moderate_count: usize,
}

#[cfg(test)]
impl Default for HealthSummary {
    fn default() -> Self {
        Self {
            files_analyzed: 0,
            functions_analyzed: 0,
            functions_above_threshold: 0,
            max_cyclomatic_threshold: 20,
            max_cognitive_threshold: 15,
            max_crap_threshold: 30.0,
            files_scored: None,
            average_maintainability: None,
            coverage_model: None,
            coverage_source_consistency: None,
            istanbul_matched: None,
            istanbul_total: None,
            severity_critical_count: 0,
            severity_high_count: 0,
            severity_moderate_count: 0,
        }
    }
}

/// Per-file health score combining complexity, coupling, and dead code metrics.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileHealthScore {
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    pub fan_in: usize,
    pub fan_out: usize,
    pub dead_code_ratio: f64,
    pub complexity_density: f64,
    pub maintainability_index: f64,
    pub total_cyclomatic: u32,
    pub total_cognitive: u32,
    pub function_count: usize,
    pub lines: u32,
    pub crap_max: f64,
    pub crap_above_threshold: usize,
}

/// Coverage model used for CRAP score computation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageModel {
    #[allow(
        dead_code,
        reason = "retained for backwards-compatible JSON deserialization"
    )]
    StaticBinary,
    StaticEstimated,
    Istanbul,
}

/// A hotspot: a file that is both complex and frequently changing.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HotspotEntry {
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    pub score: f64,
    pub commits: u32,
    pub weighted_commits: f64,
    pub lines_added: u32,
    pub lines_deleted: u32,
    pub complexity_density: f64,
    pub fan_in: usize,
    pub trend: fallow_core::churn::ChurnTrend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership: Option<OwnershipMetrics>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub is_test_path: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContributorEntry {
    pub identifier: String,
    pub format: ContributorIdentifierFormat,
    pub share: f64,
    pub stale_days: u64,
    pub commits: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ContributorIdentifierFormat {
    Raw,
    Handle,
    Anonymized,
    Hash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OwnershipState {
    Active,
    Unowned,
    DeclaredInactive,
    Drifting,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OwnershipMetrics {
    pub bus_factor: u32,

    pub contributor_count: u32,

    pub top_contributor: ContributorEntry,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub recent_contributors: Vec<ContributorEntry>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub suggested_reviewers: Vec<ContributorEntry>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_owner: Option<String>,

    pub unowned: Option<bool>,

    pub ownership_state: OwnershipState,

    pub drift: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HotspotSummary {
    pub since: String,
    pub min_commits: u32,
    pub files_analyzed: usize,
    pub files_excluded: usize,
    pub shallow_clone: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exceeded_threshold_serializes_as_snake_case() {
        let json = serde_json::to_string(&ExceededThreshold::Both).unwrap();
        assert_eq!(json, r#""both""#);

        let json = serde_json::to_string(&ExceededThreshold::Cyclomatic).unwrap();
        assert_eq!(json, r#""cyclomatic""#);
    }

    #[test]
    fn exceeded_threshold_all_variants_serialize() {
        for (variant, expected) in [
            (ExceededThreshold::Cyclomatic, r#""cyclomatic""#),
            (ExceededThreshold::Cognitive, r#""cognitive""#),
            (ExceededThreshold::Both, r#""both""#),
            (ExceededThreshold::Crap, r#""crap""#),
            (ExceededThreshold::CyclomaticCrap, r#""cyclomatic_crap""#),
            (ExceededThreshold::CognitiveCrap, r#""cognitive_crap""#),
            (ExceededThreshold::All, r#""all""#),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected, "wire form for {variant:?} should be stable");
        }
    }

    #[test]
    fn letter_grade_boundaries() {
        assert_eq!(letter_grade(100.0), "A");
        assert_eq!(letter_grade(85.0), "A");
        assert_eq!(letter_grade(84.9), "B");
        assert_eq!(letter_grade(70.0), "B");
        assert_eq!(letter_grade(69.9), "C");
        assert_eq!(letter_grade(55.0), "C");
        assert_eq!(letter_grade(54.9), "D");
        assert_eq!(letter_grade(40.0), "D");
        assert_eq!(letter_grade(39.9), "F");
        assert_eq!(letter_grade(0.0), "F");
    }

    #[test]
    fn coverage_tier_boundaries() {
        assert_eq!(CoverageTier::from_pct(0.0), CoverageTier::None);
        assert_eq!(CoverageTier::from_pct(0.1), CoverageTier::Partial);
        assert_eq!(CoverageTier::from_pct(69.9), CoverageTier::Partial);
        assert_eq!(CoverageTier::from_pct(70.0), CoverageTier::High);
        assert_eq!(CoverageTier::from_pct(100.0), CoverageTier::High);
    }

    #[test]
    fn hotspot_score_threshold_is_50() {
        assert!((HOTSPOT_SCORE_THRESHOLD - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn health_score_serializes_correctly() {
        let score = HealthScore {
            formula_version: HEALTH_SCORE_FORMULA_VERSION,
            score: 78.5,
            grade: "B",
            penalties: HealthScorePenalties {
                dead_files: Some(3.1),
                dead_exports: Some(6.0),
                complexity: 0.0,
                p90_complexity: 0.0,
                maintainability: None,
                hotspots: None,
                unused_deps: Some(5.0),
                circular_deps: Some(4.0),
                unit_size: None,
                coupling: None,
                duplication: None,
                prop_drilling: None,
            },
        };
        let json = serde_json::to_string(&score).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["formula_version"], HEALTH_SCORE_FORMULA_VERSION);
        assert_eq!(parsed["score"], 78.5);
        assert_eq!(parsed["grade"], "B");
        assert_eq!(parsed["penalties"]["dead_files"], 3.1);
        assert!(!json.contains("maintainability"));
        assert!(!json.contains("hotspots"));
        assert!(!json.contains("duplication"));
    }

    #[test]
    fn coverage_model_serializes_as_snake_case() {
        let json = serde_json::to_string(&CoverageModel::StaticBinary).unwrap();
        assert_eq!(json, r#""static_binary""#);

        let json = serde_json::to_string(&CoverageModel::StaticEstimated).unwrap();
        assert_eq!(json, r#""static_estimated""#);

        let json = serde_json::to_string(&CoverageModel::Istanbul).unwrap();
        assert_eq!(json, r#""istanbul""#);
    }

    #[test]
    fn finding_severity_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&FindingSeverity::Moderate).unwrap(),
            r#""moderate""#,
        );
        assert_eq!(
            serde_json::to_string(&FindingSeverity::High).unwrap(),
            r#""high""#,
        );
        assert_eq!(
            serde_json::to_string(&FindingSeverity::Critical).unwrap(),
            r#""critical""#,
        );
    }

    #[test]
    fn finding_severity_ordering() {
        assert!(FindingSeverity::Moderate < FindingSeverity::High);
        assert!(FindingSeverity::High < FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_moderate_when_below_high_thresholds() {
        let severity = compute_finding_severity(20, 25, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Moderate);
    }

    #[test]
    fn compute_severity_high_from_cognitive() {
        let severity = compute_finding_severity(25, 20, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::High);
    }

    #[test]
    fn compute_severity_high_from_cyclomatic() {
        let severity = compute_finding_severity(20, 30, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::High);
    }

    #[test]
    fn compute_severity_critical_from_cognitive() {
        let severity = compute_finding_severity(40, 20, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_critical_from_cyclomatic() {
        let severity = compute_finding_severity(20, 50, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_uses_highest_across_dimensions() {
        let severity = compute_finding_severity(45, 20, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_at_exact_boundaries() {
        let severity = compute_finding_severity(25, 30, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::High);

        let severity = compute_finding_severity(24, 29, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Moderate);

        let severity = compute_finding_severity(40, 50, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_crap_contributes_high() {
        let severity = compute_finding_severity(10, 10, Some(60.0), 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::High);
    }

    #[test]
    fn compute_severity_crap_contributes_critical() {
        let severity = compute_finding_severity(10, 10, Some(120.0), 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_crap_moderate_under_high() {
        let severity = compute_finding_severity(10, 10, Some(30.0), 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Moderate);
    }

    #[test]
    fn exceeded_threshold_from_bools() {
        assert!(matches!(
            ExceededThreshold::from_bools(true, false, false),
            ExceededThreshold::Cyclomatic
        ));
        assert!(matches!(
            ExceededThreshold::from_bools(true, true, true),
            ExceededThreshold::All
        ));
        assert!(matches!(
            ExceededThreshold::from_bools(false, false, true),
            ExceededThreshold::Crap
        ));
        assert!(matches!(
            ExceededThreshold::from_bools(true, false, true),
            ExceededThreshold::CyclomaticCrap
        ));
    }

    #[test]
    fn exceeded_threshold_includes_helpers() {
        let all = ExceededThreshold::All;
        assert!(all.includes_cyclomatic());
        assert!(all.includes_cognitive());
        assert!(all.includes_crap());

        let crap_only = ExceededThreshold::Crap;
        assert!(!crap_only.includes_cyclomatic());
        assert!(!crap_only.includes_cognitive());
        assert!(crap_only.includes_crap());

        assert!(ExceededThreshold::CyclomaticCrap.includes_crap());
        assert!(ExceededThreshold::CognitiveCrap.includes_crap());
        assert!(!ExceededThreshold::Both.includes_crap());
        assert!(!ExceededThreshold::Cyclomatic.includes_crap());
        assert!(!ExceededThreshold::Cognitive.includes_crap());
    }

    #[test]
    fn coverage_source_consistency_omits_empty_sources() {
        let sources = Vec::new();
        assert_eq!(summarize_coverage_source_consistency(sources), None);
    }

    #[test]
    fn coverage_source_consistency_reports_uniform_sources() {
        assert_eq!(
            summarize_coverage_source_consistency([
                CoverageSource::Estimated,
                CoverageSource::Estimated,
            ]),
            Some(CoverageSourceConsistency::Uniform)
        );
    }

    #[test]
    fn coverage_source_consistency_reports_mixed_sources() {
        assert_eq!(
            summarize_coverage_source_consistency([
                CoverageSource::Istanbul,
                CoverageSource::Estimated,
            ]),
            Some(CoverageSourceConsistency::Mixed)
        );
    }
}
