//! Score types, grade boundaries, file health metrics, and findings.

/// Hotspot score threshold for counting a file as a hotspot in vital signs.
pub const HOTSPOT_SCORE_THRESHOLD: f64 = 50.0;

/// Cognitive complexity threshold above which a function is flagged for extraction.
pub const COGNITIVE_EXTRACTION_THRESHOLD: u16 = 30;

/// Default cognitive complexity threshold for "high" severity (warning tier).
pub const DEFAULT_COGNITIVE_HIGH: u16 = 25;

/// Default cognitive complexity threshold for "critical" severity.
pub const DEFAULT_COGNITIVE_CRITICAL: u16 = 40;

/// Default cyclomatic complexity threshold for "high" severity (warning tier).
pub const DEFAULT_CYCLOMATIC_HIGH: u16 = 30;

/// Default cyclomatic complexity threshold for "critical" severity.
pub const DEFAULT_CYCLOMATIC_CRITICAL: u16 = 50;

/// Minimum lines of code for full complexity density weight in the MI formula.
/// Files smaller than this get a proportional dampening factor to prevent
/// density from dominating the score on trivially small files.
pub const MI_DENSITY_MIN_LINES: f64 = 50.0;

/// Project-level health score: a single 0–100 number with letter grade.
///
/// ## Score Formula
///
/// ```text
/// score = 100
///   - min(dead_file_pct × 0.2, 15)
///   - min(dead_export_pct × 0.2, 15)
///   - min(critical_complexity_pct × 4, 20)
///   - 0 when critical_complexity_pct is available; otherwise min(max(0, p90_cyclomatic − 10), 10)
///   - min(maintainability_low_pct × 1.5, 15)
///   - min(hotspot_top_pct_count / ceil(total_files × 0.01) × 10, 10)
///   - min(unused_deps_per_k_files × 0.5, 25)
///   - min(circular_deps_per_k_files × 0.5, 25)
///   - min(functions_over_60_loc_per_k × 0.5, 10)        [unit size]
///   - min(coupling_high_pct × 0.5, 5)                   [coupling]
///   - min(max(0, duplication_pct − 5) × 1.0, 10)        [duplication]
/// ```
///
/// Older snapshots that lack the scale-invariant fields fall back to the
/// previous average/p90/count aggregators.
///
/// Missing metrics (from pipelines that didn't run) don't penalize. `--score`
/// computes the score and duplication penalty, but churn-backed hotspot
/// penalties are only available when hotspot analysis runs (`--hotspots`, or
/// target analysis that needs hotspot data).
///
/// ## Letter Grades
///
/// A: score ≥ 85, B: 70–84, C: 55–69, D: 40–54, F: below 40.
pub const HEALTH_SCORE_FORMULA_VERSION: u32 = 2;

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
/// Project-level health score. Score = 100 minus available penalties from dead
/// code, complexity, maintainability, hotspots, unused deps, circular deps,
/// unit size, coupling, and duplication. Missing metrics do not penalize;
/// --score computes the score and duplication penalty, while churn-backed
/// hotspot penalties require hotspot analysis (--hotspots, or --targets with
/// --score).
pub struct HealthScore {
    /// Health score formula version. Version 2 uses scale-invariant
    /// density/tail metrics for monorepo-safe scoring.
    pub formula_version: u32,
    /// Overall score (0-100, higher is better). Reproducible: 100 -
    /// sum(penalties) == score.
    pub score: f64,
    /// Letter grade. A: score >= 85, B: 70-84, C: 55-69, D: 40-54, F: below 40.
    pub grade: &'static str,
    /// Per-component penalty breakdown. Shows what drove the score down.
    pub penalties: HealthScorePenalties,
}

/// Per-component penalty breakdown for the health score.
///
/// Each field shows how many points were subtracted for that component.
/// `None` means the metric was not available (pipeline didn't run).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthScorePenalties {
    /// Points lost from dead files (max 15). Null if dead code pipeline not
    /// run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_files: Option<f64>,
    /// Points lost from dead exports (max 15). Null if dead code pipeline not
    /// run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_exports: Option<f64>,
    /// Points lost from critical-complexity density (max 20). Older snapshots
    /// without density fields fall back to average cyclomatic complexity above
    /// 1.5.
    pub complexity: f64,
    /// Points lost from legacy p90 cyclomatic complexity above 10. Current
    /// scale-invariant runs report 0 because tail complexity is folded into
    /// complexity.
    pub p90_complexity: f64,
    /// Points lost from low maintainability index density (max 15).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintainability: Option<f64>,
    /// Points lost from top-percentile hotspot density (max 10). Null if
    /// hotspots not computed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotspots: Option<f64>,
    /// Points lost from unused dependency density (max 25). Null if dead code
    /// pipeline not run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unused_deps: Option<f64>,
    /// Points lost from circular dependency density (max 25). Null if dead code
    /// pipeline not run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circular_deps: Option<f64>,
    /// Points lost from oversized-function density (max 10). Null if no
    /// functions analyzed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_size: Option<f64>,
    /// Points lost from coupling concentration density (max 5). Null if file
    /// scores not computed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coupling: Option<f64>,
    /// Points lost from code duplication (max 10). Penalty = min(max(0,
    /// duplication_pct - 5) * 1, 10). Null if duplication pipeline not run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplication: Option<f64>,
}

/// Map a numeric score (0–100) to a letter grade.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "score is 0-100, fits in u32"
)]
pub const fn letter_grade(score: f64) -> &'static str {
    // Truncate to u32 so that 84.9 maps to B and 85.0 maps to A —
    // fractional digits don't affect the grade bucket.
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
///
/// Bucketed coverage signal that lets action consumers (AI agents, IDE
/// extensions, CI integrations) pick the right remediation without knowing
/// the underlying coverage values:
/// - `None`: file has no test reachability (estimated model 0% band) or
///   Istanbul data shows 0% statement coverage. The right action is
///   "add tests from scratch."
/// - `Partial`: some coverage exists (estimated model 40% band, or
///   Istanbul shows >0% but below the high watermark). The right
///   action is "increase coverage on uncovered branches."
/// - `High`: coverage is at or above the high watermark (estimated model
///   85% band, or Istanbul shows >= 70%). Action selection still checks
///   the CRAP formula before deciding whether coverage or refactoring is
///   the better remediation.
///
/// The high watermark default is 70 (matches Istanbul `lines: 70`).
/// Partial is anything in `(0, 70)`. None is `<= 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageTier {
    /// 0% coverage: file is not test-reachable, or Istanbul reports 0%.
    None,
    /// Some coverage: estimated 40% band, or Istanbul reports `(0, 70)`.
    Partial,
    /// High coverage: estimated 85% band, or Istanbul reports `>= 70`.
    High,
}

/// Coverage percentage at or above which a function is classified as `High`.
/// Matches Istanbul's default `lines: 70` watermark.
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
///
/// Discriminates whether the `coverage_tier` and `crap` score were derived
/// from real Istanbul data, the graph-based estimated model evaluated against
/// the finding's own file, or the graph-based estimated model evaluated
/// against a different file (today: an Angular component `.ts` reached via
/// the inverse `templateUrl` edge from a synthetic `<template>` finding on
/// the component's `.html` template).
///
/// Consumers reading this field:
/// - AI agents picking remediation actions ("the score is inherited, the fix
///   may need to land on the component file, not the template").
/// - Dashboards plotting CRAP trends ("the discriminator changed shape;
///   absorb the rollout rather than flagging a step change").
/// - Future tier 2 (AOT source-map back-mapping) will introduce
///   `MeasuredAotSourceMap` so consumers can distinguish measured-AOT from
///   inherited-JIT without parsing the score itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageSource {
    /// Direct match against Istanbul `fnMap` data for this function.
    Istanbul,
    /// Graph-based estimated model evaluated against the finding's own file
    /// (current default for any CRAP finding that did not match Istanbul).
    Estimated,
    /// Graph-based estimated model evaluated against the owning component's
    /// `.ts` file (reached via the inverse `templateUrl` edge from a
    /// synthetic `<template>` finding on an Angular `.html` template). Emitted
    /// because JIT-compiled Angular tests do not produce Istanbul entries for
    /// `.html` files; tier 2 will replace this with measured coverage for
    /// AOT-compiled tests.
    EstimatedComponentInherited,
}

/// Inner complexity-violation payload, wrapped by
/// [`HealthFinding`](crate::health_types::HealthFinding).
///
/// Carries the raw measured signals for a single function or synthetic
/// template entry that crossed a complexity threshold. The wrapper adds
/// the typed `actions` list and the audit-mode `introduced` flag using
/// `#[serde(flatten)]` so `findings[]` items expose these fields at the
/// top level for wire continuity with the pre-wrapper shape.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComplexityViolation {
    /// Absolute file path.
    pub path: std::path::PathBuf,
    /// Function name, `"<anonymous>"` for unnamed functions/arrows, or
    /// `"<template>"` for synthetic Angular template findings.
    pub name: String,
    /// 1-based line number.
    pub line: u32,
    /// 0-based column.
    pub col: u32,
    /// Cyclomatic complexity.
    pub cyclomatic: u16,
    /// SonarSource cognitive complexity (structural + nesting penalty).
    pub cognitive: u16,
    /// Number of lines in the function.
    pub line_count: u32,
    /// Number of parameters (excluding TypeScript's this parameter).
    pub param_count: u8,
    /// Which threshold(s) this finding exceeds. `crap` and its combinations are
    /// emitted when `max_crap_threshold` is crossed.
    pub exceeded: ExceededThreshold,
    /// How far above the threshold: moderate (just above), high (recommended
    /// for extraction), or critical (immediate extraction candidate). Defaults:
    /// cognitive 25/40, cyclomatic 30/50.
    pub severity: FindingSeverity,
    /// CRAP score (`CC^2 * (1 - cov/100)^3 + CC`), rounded to one decimal.
    /// Present when the function also exceeded `--max-crap`, otherwise absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crap: Option<f64>,
    /// Per-function statement coverage percentage (0.0 to 100.0) used to
    /// derive `crap`. Present when Istanbul data matched the function,
    /// otherwise absent (estimated model or unmatched functions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_pct: Option<f64>,
    /// Bucketed coverage tier used to drive action selection. Present whenever
    /// CRAP triggered the finding (Istanbul or estimated), absent otherwise.
    /// `none` = coverage is at most 0% (file not test-reachable, or Istanbul
    /// reports 0); `partial` = coverage is in `(0, 70)`; `high` = coverage is
    /// at or above the high watermark (default `>= 70`, or the estimated 85%
    /// band).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_tier: Option<CoverageTier>,
    /// Provenance of the coverage signal. Present whenever CRAP triggered the
    /// finding. `istanbul` = direct fnMap match; `estimated` = graph-based
    /// estimate against the finding's own file; `estimated_component_inherited`
    /// = graph-based estimate inherited from an Angular component `.ts`
    /// reached via the inverse `templateUrl` edge (synthetic `<template>`
    /// findings on `.html` files only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_source: Option<CoverageSource>,
    /// Owning component file that contributed reachability when
    /// `coverage_source == "estimated_component_inherited"`. Always paired
    /// with that variant of `coverage_source` and absent otherwise. The
    /// value is the `.ts` file fallow walked to via the inverse `templateUrl`
    /// edge (e.g. `permissions.component.ts`); the JSON serializer strips it
    /// to project-relative form just like other path fields. Lets human and
    /// AI consumers explain "the template scored partial because the
    /// component it belongs to is tested" without re-deriving the link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_from: Option<std::path::PathBuf>,
    /// Breakdown of a synthetic `<component>` rollup finding into its
    /// worst-class-function and template contributions. Present only on
    /// findings whose [`name`](Self::name) is the literal string
    /// `"<component>"` (Angular components whose class AND template both
    /// contributed to a per-component complexity rollup); absent on every
    /// other finding kind.
    ///
    /// The owning [`HealthFinding`](crate::health_types::HealthFinding)'s
    /// [`cyclomatic`](Self::cyclomatic) / [`cognitive`](Self::cognitive)
    /// totals are `class_worst_function + template`, so consumers ranking
    /// by complexity see the component as one unit. The breakdown carries
    /// the pre-summation numbers plus the worst class function's name so
    /// consumers can explain "this component ranked high because the
    /// template added 6 cyclomatic on top of the worst class function's 3".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_rollup: Option<ComponentRollup>,
}

/// Per-component breakdown attached to a synthetic `<component>`
/// [`HealthFinding`](crate::health_types::HealthFinding). See
/// [`ComplexityViolation::component_rollup`] for the owning-finding
/// contract; the wrapper flattens the inner type's
/// [`component_rollup`](ComplexityViolation::component_rollup) field
/// onto its own wire shape.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComponentRollup {
    /// Angular component class name (e.g. `"HostGameComponent"`). Derived
    /// from the worst class function's `ClassName.methodName` identifier.
    pub component: String,
    /// Name of the worst class function/method whose individual cyclomatic
    /// is the largest among the component's class findings (e.g.
    /// `"ngOnInit"`). When two methods tie on cyclomatic the first by
    /// iteration order wins; consumers should treat the choice as
    /// representative, not authoritative.
    pub class_worst_function: String,
    /// Cyclomatic complexity of the worst class function alone (the
    /// `class_worst_function`).
    pub class_cyclomatic: u16,
    /// Cognitive complexity of the worst class function alone.
    pub class_cognitive: u16,
    /// Path of the Angular template that contributed to the rollup.
    /// External-template components use the `.html` template file path;
    /// inline-template components use the owning `.ts` itself (since the
    /// `<template>` finding for inline templates is anchored at the
    /// component's `@Component` decorator on the same file). Stored
    /// absolute internally; the JSON output strips it to project-relative
    /// form via the global `strip_root_prefix` post-pass (as with every
    /// other `PathBuf` field in this crate).
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub template_path: std::path::PathBuf,
    /// Cyclomatic complexity contributed by the template alone (control
    /// flow on `*ngIf` / `*ngFor` / `@if` / `@for` / `@switch` etc.).
    pub template_cyclomatic: u16,
    /// Cognitive complexity contributed by the template alone (nesting +
    /// branching penalty on the same constructs as `template_cyclomatic`).
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
    ///
    /// Exercised by the `exceeded_threshold_includes_helpers` unit test below;
    /// the binary target has no direct caller today, so the lint is allowed
    /// rather than expected (`#[expect]` would be unfulfilled on the lib side
    /// which does reach the tests).
    #[must_use]
    #[allow(
        dead_code,
        reason = "symmetry with includes_cyclomatic/cognitive; consumed by tests and intended for report format extensions"
    )]
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
    /// Absolute file path.
    pub path: std::path::PathBuf,
    /// Function name, or `"<anonymous>"` for unnamed functions/arrows.
    pub name: String,
    /// 1-based line number.
    pub line: u32,
    /// Number of lines in the function.
    pub line_count: u32,
}

/// Summary statistics for the health report.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthSummary {
    /// Number of files analyzed.
    pub files_analyzed: usize,
    /// Total number of functions found.
    pub functions_analyzed: usize,
    /// Number of functions exceeding at least one threshold (before --top
    /// truncation).
    pub functions_above_threshold: usize,
    /// Configured cyclomatic threshold.
    pub max_cyclomatic_threshold: u16,
    /// Configured cognitive threshold.
    pub max_cognitive_threshold: u16,
    /// Configured CRAP (Change Risk Anti-Patterns) score threshold. Functions
    /// meeting or exceeding this score appear as findings with the `crap` and
    /// optional `coverage_pct` fields populated.
    pub max_crap_threshold: f64,
    /// Number of files with health scores. Only present when --file-scores is
    /// used. 0 indicates the flag was set but scoring failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_scored: Option<usize>,
    /// Average maintainability index across all scored files (before --top
    /// truncation). Only present when --file-scores is used and at least one
    /// file was scored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_maintainability: Option<f64>,
    /// Coverage model used for CRAP score computation. 'static_estimated'
    /// (default) uses per-function graph-based estimation from export
    /// references: directly test-referenced = 85%, indirectly reachable = 40%,
    /// untested = 0%. 'istanbul' uses real per-function statement coverage from
    /// a coverage-final.json file (--coverage flag or auto-detected).
    /// 'static_binary' is the legacy binary model. Only present when file
    /// scores are computed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_model: Option<CoverageModel>,
    /// Number of functions matched against Istanbul coverage data.
    /// Only present when `coverage_model` is `istanbul`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub istanbul_matched: Option<usize>,
    /// Total functions that could potentially be matched.
    /// Only present when `coverage_model` is `istanbul`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub istanbul_total: Option<usize>,
    /// Number of findings with critical severity (cognitive >= 40 or cyclomatic
    /// >= 50).
    pub severity_critical_count: usize,
    /// Number of findings with high severity (cognitive 25-39 or cyclomatic
    /// 30-49).
    pub severity_high_count: usize,
    /// Number of findings with moderate severity.
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
            istanbul_matched: None,
            istanbul_total: None,
            severity_critical_count: 0,
            severity_high_count: 0,
            severity_moderate_count: 0,
        }
    }
}

/// Per-file health score combining complexity, coupling, and dead code metrics.
///
/// Files with zero functions (barrel files, re-export files) are excluded by default.
///
/// ## Maintainability Index Formula
///
/// ```text
/// dampening = min(lines / 50, 1.0)
/// fan_out_penalty = min(ln(fan_out + 1) × 4, 15)
/// maintainability = 100
///     - (complexity_density × 30 × dampening)
///     - (dead_code_ratio × 20)
///     - fan_out_penalty
/// ```
///
/// Clamped to \[0, 100\]. Higher is better. The dampening factor prevents
/// complexity density from dominating the score on small files (< 50 lines).
///
/// - **complexity_density**: total cyclomatic complexity / lines of code
/// - **dead_code_ratio**: fraction of value exports (excluding type-only exports) with zero references (0.0–1.0)
/// - **fan_out_penalty**: logarithmic scaling with cap at 15 points; reflects diminishing marginal risk of additional imports
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileHealthScore {
    /// File path (absolute; stripped to relative in output).
    pub path: std::path::PathBuf,
    /// Number of files that import this file.
    pub fan_in: usize,
    /// Number of files this file imports.
    pub fan_out: usize,
    /// Fraction of value exports with zero references (0.0–1.0). Files with no value exports get 0.0.
    /// Type-only exports (interfaces, type aliases) are excluded from both numerator and denominator
    /// to avoid inflating the ratio for well-typed codebases that export props types alongside components.
    pub dead_code_ratio: f64,
    /// Total cyclomatic complexity / lines of code.
    pub complexity_density: f64,
    /// Weighted composite score (0–100, higher is better).
    pub maintainability_index: f64,
    /// Sum of cyclomatic complexity across all functions.
    pub total_cyclomatic: u32,
    /// Sum of cognitive complexity across all functions.
    pub total_cognitive: u32,
    /// Number of functions in this file.
    pub function_count: usize,
    /// Total lines of code (from line_offsets).
    pub lines: u32,
    /// Maximum CRAP score among functions in this file. Computed via the active
    /// `coverage_model` per the canonical formula CC^2 * (1 - cov/100)^3 + CC
    /// (Savoia & Evans, 2007). Coverage source: `static_estimated` (default,
    /// graph-based per-function estimate), `istanbul` (real per-function
    /// statement coverage from --coverage), or the legacy `static_binary`
    /// (whole-file 0%/100%, retained for compatibility).
    pub crap_max: f64,
    /// Count of functions with CRAP >= 30 (CC >= 5 without test path).
    pub crap_above_threshold: usize,
}

/// Coverage model used for CRAP score computation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageModel {
    /// Binary model: test-reachable = CC, untested = CC^2 + CC.
    /// Superseded by `StaticEstimated`; retained for serialization compatibility.
    #[allow(
        dead_code,
        reason = "retained for backwards-compatible JSON deserialization"
    )]
    StaticBinary,
    /// Graph-based estimation: per-function coverage derived from export
    /// reference analysis. Directly test-referenced = 85%, indirectly
    /// test-reachable = 40%, untested = 0%. Default model.
    StaticEstimated,
    /// Istanbul-format coverage data: real per-function statement coverage
    /// from Jest, Vitest, c8, nyc, or any Istanbul-compatible tool.
    /// CRAP = CC^2 * (1 - cov/100)^3 + CC.
    Istanbul,
}

/// A hotspot: a file that is both complex and frequently changing.
///
/// ## Score Formula
///
/// ```text
/// normalized_churn = weighted_commits / max_weighted_commits   (0..1)
/// normalized_complexity = complexity_density / max_density      (0..1)
/// score = normalized_churn × normalized_complexity × 100       (0..100)
/// ```
///
/// Score uses within-project max normalization. Higher score = higher risk.
/// Fan-in is shown separately as "blast radius" — not baked into the score.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HotspotEntry {
    /// File path (absolute; stripped to relative in output).
    pub path: std::path::PathBuf,
    /// Hotspot score (0–100). Higher means more risk.
    pub score: f64,
    /// Number of commits in the analysis window.
    pub commits: u32,
    /// Recency-weighted commit count (exponential decay, half-life 90 days).
    pub weighted_commits: f64,
    /// Total lines added across all commits.
    pub lines_added: u32,
    /// Total lines deleted across all commits.
    pub lines_deleted: u32,
    /// Cyclomatic complexity / lines of code.
    pub complexity_density: f64,
    /// Number of files that import this file (blast radius).
    pub fan_in: usize,
    /// Churn trend: accelerating (recent > 1.5× older), stable, or cooling
    /// (recent < 0.67× older).
    pub trend: fallow_core::churn::ChurnTrend,
    /// Ownership signals (bus factor, contributors, declared owner, drift).
    /// Populated only when `--ownership` is requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ownership: Option<OwnershipMetrics>,
    /// True when the file path matches a test/mock convention (e.g.
    /// `**/__tests__/**`, `**/*.test.*`, `**/*.spec.*`, `**/__mocks__/**`).
    /// Test files are intentionally included in hotspot ranking (test
    /// maintenance IS real work), but tagging them lets consumers decide
    /// whether to weight or filter them downstream.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub is_test_path: bool,
}

/// Per-author contribution summary. The identifier is rendered per the
/// configured ownership.emailMode (handle, hash, or raw); the format field
/// discriminates the three so type-aware consumers can branch without
/// re-parsing.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContributorEntry {
    /// Display string per the configured email mode: raw email
    /// (`alice@example.com`), local-part handle (`alice`), or stable hash
    /// pseudonym (`xxh3:<16hex>`). The format depends on `format`.
    ///
    /// Renamed from `email` because in `handle` and `hash` modes the value
    /// is no longer an email address; consumers tempted to use it as one
    /// (e.g. `mailto:`) would be wrong.
    pub identifier: String,
    /// Format of [`identifier`](Self::identifier): `raw`, `handle`, or `hash`.
    /// Lets type-aware consumers branch without re-parsing the string.
    pub format: ContributorIdentifierFormat,
    /// Recency-weighted share of total weighted commits (0..1, three decimals).
    pub share: f64,
    /// Days since this contributor last touched the file.
    pub stale_days: u64,
    /// Total commits by this contributor in the analysis window.
    pub commits: u32,
}

/// Format discriminator for [`ContributorEntry::identifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ContributorIdentifierFormat {
    /// Raw author email as recorded in git history.
    Raw,
    /// Local-part of the author email, with GitHub-style numeric noreply
    /// prefixes unwrapped (`12345+alice@users.noreply.github.com` → `alice`).
    Handle,
    /// Non-cryptographic stable pseudonym (`xxh3:<16hex>`).
    Hash,
}

/// Per-file ownership signals attached to hotspot entries when the user
/// passes `--ownership`. All fields are derived from git history and the
/// repository's CODEOWNERS file (if any).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OwnershipMetrics {
    /// Avelino truck factor: minimum contributors covering at least 50% of
    /// recency-weighted commits in the analysis window. Lower = higher
    /// knowledge-loss risk.
    pub bus_factor: u32,

    /// Distinct authors in the analysis window after bot filtering.
    pub contributor_count: u32,

    /// The highest-share contributor.
    pub top_contributor: ContributorEntry,

    /// Up to three additional contributors by share, ordered desc.
    /// Useful for "who else could review this file" routing.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub recent_contributors: Vec<ContributorEntry>,

    /// Contributors whose last touch is within 90 days, ordered by share
    /// descending. First-class field so AI agents do not have to
    /// reconstruct it from [`recent_contributors`](Self::recent_contributors)
    /// filtered by [`ContributorEntry::stale_days`]. Excludes the top
    /// contributor (they are the sole author being flagged); consumers
    /// wanting the full list can union with `top_contributor`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub suggested_reviewers: Vec<ContributorEntry>,

    /// CODEOWNERS-resolved owner for this file, if a rule matched.
    /// Only the primary (first) owner of the matched rule is reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_owner: Option<String>,

    /// Tristate: `Some(true)` = CODEOWNERS file exists but no rule matches
    /// this file; `Some(false)` = a CODEOWNERS rule matches; `None` = no
    /// CODEOWNERS file was discovered for the repository (cannot determine).
    pub unowned: Option<bool>,

    /// True when ownership has drifted from the original author to a new
    /// top contributor. Pairs with [`drift_reason`](Self::drift_reason).
    pub drift: bool,

    /// Human-readable explanation of the drift, populated only when
    /// [`drift`](Self::drift) is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift_reason: Option<String>,
}

/// Summary statistics for hotspot analysis.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HotspotSummary {
    /// Analysis window display string (e.g., "6 months").
    pub since: String,
    /// Minimum commits threshold.
    pub min_commits: u32,
    /// Number of files with churn data meeting the threshold.
    pub files_analyzed: usize,
    /// Number of files excluded (below min_commits).
    pub files_excluded: usize,
    /// Whether the repository is a shallow clone.
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
            },
        };
        let json = serde_json::to_string(&score).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["formula_version"], HEALTH_SCORE_FORMULA_VERSION);
        assert_eq!(parsed["score"], 78.5);
        assert_eq!(parsed["grade"], "B");
        assert_eq!(parsed["penalties"]["dead_files"], 3.1);
        // None fields should be absent
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

    // --- FindingSeverity ---

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
        // Cognitive is critical, cyclomatic is moderate -> critical
        let severity = compute_finding_severity(45, 20, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_at_exact_boundaries() {
        // At exactly the high threshold -> high
        let severity = compute_finding_severity(25, 30, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::High);

        // One below high threshold -> moderate
        let severity = compute_finding_severity(24, 29, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Moderate);

        // At exactly the critical threshold -> critical
        let severity = compute_finding_severity(40, 50, None, 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_crap_contributes_high() {
        // Low cyclomatic and cognitive but high CRAP -> high severity
        let severity = compute_finding_severity(10, 10, Some(60.0), 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::High);
    }

    #[test]
    fn compute_severity_crap_contributes_critical() {
        // CRAP at critical tier drives overall severity to critical
        let severity = compute_finding_severity(10, 10, Some(120.0), 25, 40, 30, 50);
        assert_eq!(severity, FindingSeverity::Critical);
    }

    #[test]
    fn compute_severity_crap_moderate_under_high() {
        // CRAP at 30 is moderate; neither cyclomatic nor cognitive trigger
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

        // `includes_crap` distinguishes the three CRAP-containing variants.
        assert!(ExceededThreshold::CyclomaticCrap.includes_crap());
        assert!(ExceededThreshold::CognitiveCrap.includes_crap());
        assert!(!ExceededThreshold::Both.includes_crap());
        assert!(!ExceededThreshold::Cyclomatic.includes_crap());
        assert!(!ExceededThreshold::Cognitive.includes_crap());
    }
}
