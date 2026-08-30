use fallow_output::{DirectCallerEvidence, DirectCallerSymbolEvidence, FileHealthScore};

use crate::module_graph::StaticTestCoverage;

use super::coverage_gaps::compute_coverage_gaps;
pub(super) use super::coverage_gaps::{CoverageGapData, build_coverage_summary};
use super::threshold_overrides::ThresholdOverrideResolver;

/// Output from `compute_file_scores`, including auxiliary data for refactoring targets.
pub struct FileScoreOutput {
    pub(crate) scores: Vec<FileHealthScore>,
    /// Static coverage gaps derived from runtime-vs-test reachability.
    pub(crate) coverage: CoverageGapData,
    /// Files participating in circular dependencies (absolute paths).
    pub(crate) circular_files: rustc_hash::FxHashSet<std::path::PathBuf>,
    /// Top 3 functions by cognitive complexity per file (name, line, cognitive score).
    pub(crate) top_complex_fns: rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>>,
    /// Files that are configured entry points.
    pub(crate) entry_points: rustc_hash::FxHashSet<std::path::PathBuf>,
    /// Total number of value exports per file (for dead code gate: total_value_exports >= 3).
    pub(crate) value_export_counts: rustc_hash::FxHashMap<std::path::PathBuf, usize>,
    /// Unused export names per file (for evidence linking).
    pub(crate) unused_export_names: rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
    /// Cycle members per file: maps each file to the other files in its cycle.
    pub(crate) cycle_members: rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
    /// Direct importers per file, with the symbols imported by each caller.
    pub(crate) direct_callers: rustc_hash::FxHashMap<std::path::PathBuf, Vec<DirectCallerEvidence>>,
    /// Aggregate counts from AnalysisResults for vital signs (project-wide).
    pub(crate) analysis_counts: crate::vital_signs::AnalysisCounts,
    /// Located prop-drilling chains from the analysis results (empty when the
    /// opt-in `prop-drilling` rule is off, since the detector populates no chains
    /// then). Drives the small capped health penalty, the hotspot surface, and
    /// the `health --format json` `prop_drilling_chains` array.
    pub(crate) prop_drilling_chains: Vec<fallow_types::output_dead_code::PropDrillingChainFinding>,
    /// Per-component render fan-in (JSX render SITES + distinct parents) plus the
    /// precomputed concentration aggregates, cloned from the analysis results.
    /// `None` on non-React projects. Descriptive blast-radius signal: feeds the
    /// `VitalSigns` render-fan-in aggregates and the hotspot/react drill-down
    /// `rendered in N places` line (keyed back to file paths).
    pub(crate) render_fan_in: Option<fallow_types::results::RenderFanInMetric>,
    /// Per-path snapshot of analysis findings, used to recompute
    /// [`crate::vital_signs::AnalysisCounts`] for an arbitrary subset of files
    /// (workspace scoping, `--group-by` partitioning).
    pub(crate) analysis_snapshot: AnalysisCountsSnapshot,
    /// Istanbul match stats: functions matched / total (only meaningful with Istanbul model).
    pub(crate) istanbul_matched: usize,
    pub(crate) istanbul_total: usize,
    /// Analyzed files the coverage map carried an entry for. Read against the
    /// map's own file count, this separates a map that did not join from code
    /// the map says nothing ran in.
    pub(crate) istanbul_files_joined: usize,
    /// Files the coverage map describes, joined or not. Zero without a map.
    pub(crate) istanbul_files_total: usize,
    /// Per-file, per-function CRAP data used to emit `--max-crap` findings.
    /// Absolute paths match `FileHealthScore.path`. Absent entries indicate the
    /// file had zero functions.
    pub(crate) per_function_crap: rustc_hash::FxHashMap<std::path::PathBuf, Vec<PerFunctionCrap>>,
    /// Provenance map for synthetic Angular `<template>` findings whose CRAP
    /// was inherited from the owning `.component.ts` via the inverse
    /// `templateUrl` edge. Keys are the template `.html` absolute paths,
    /// values are the owner `.ts` absolute paths (the path used for the
    /// `inherited from foo.component.ts` human-output suffix). Absent for
    /// non-template files and for templates with no `.ts` owner.
    pub(crate) template_inherit_provenance:
        rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>,
}

struct FileScoreOutputParts<'a> {
    graph: &'a fallow_graph::graph::ModuleGraph,
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    results: &'a crate::results::AnalysisResults,
    scores: Vec<FileHealthScore>,
    coverage: CoverageGapData,
    circular_files: rustc_hash::FxHashSet<std::path::PathBuf>,
    top_complex_fns: rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>>,
    entry_points: rustc_hash::FxHashSet<std::path::PathBuf>,
    value_export_counts: rustc_hash::FxHashMap<std::path::PathBuf, usize>,
    unused_export_names: rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
    cycle_members: rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
    direct_callers: rustc_hash::FxHashMap<std::path::PathBuf, Vec<DirectCallerEvidence>>,
    istanbul_matched: usize,
    istanbul_total: usize,
    istanbul_files_joined: usize,
    istanbul_files_total: usize,
    per_function_crap: rustc_hash::FxHashMap<std::path::PathBuf, Vec<PerFunctionCrap>>,
    template_inherit: rustc_hash::FxHashMap<crate::discover::FileId, TemplateInheritContext>,
}

/// Per-path snapshot of analysis-pipeline findings, retained alongside the
/// pre-aggregated `analysis_counts` so that workspace- or group-scoped runs
/// can recompute counts without re-running the full pipeline.
///
/// All paths are absolute (matching `AnalysisResults` and `FileHealthScore`).
#[derive(Clone, Default)]
pub struct AnalysisCountsSnapshot {
    /// One entry per unused file.
    unused_file_paths: Vec<std::path::PathBuf>,
    /// One entry per unused value or type export, keyed by the file containing
    /// the export.
    unused_export_paths: Vec<std::path::PathBuf>,
    /// One entry per unused dependency across `dependencies`,
    /// `devDependencies`, and `optionalDependencies`, keyed by the
    /// `package.json` path that declared it.
    unused_dep_package_paths: Vec<std::path::PathBuf>,
    /// Each cycle as the set of file paths it contains. Used to count cycles
    /// that touch any file inside a workspace.
    circular_dep_groups: Vec<Vec<std::path::PathBuf>>,
    /// Total exports per module (`module.exports.len()` in the graph), used
    /// as the denominator for `dead_export_pct`.
    module_export_counts: rustc_hash::FxHashMap<std::path::PathBuf, usize>,
}

impl AnalysisCountsSnapshot {
    /// Compute analysis counts for the file subset selected by `subset`.
    ///
    /// Returns `*defaults` when `subset.is_full()`. Otherwise recomputes
    /// every count by retaining paths the subset accepts. Cycles are counted
    /// when any cycle member is in the subset.
    ///
    /// Unused-dep counting is special-cased: dep entries are keyed by their
    /// `package.json` path, which is never a source file and therefore never
    /// matches the source-file membership of a `Paths` subset. For
    /// `SubsetFilter::Paths`, a `package.json` is considered
    /// in scope when at least one source file in the subset sits inside its
    /// directory (the dep's owning workspace).
    ///
    /// `total_deps` is propagated unchanged from `defaults`; it is not
    /// available per-subset today (mirrors the project-wide behaviour).
    pub(crate) fn counts_for(
        &self,
        subset: &crate::health::SubsetFilter<'_>,
        defaults: &crate::vital_signs::AnalysisCounts,
    ) -> crate::vital_signs::AnalysisCounts {
        if subset.is_full() {
            return *defaults;
        }
        let dead_files = self
            .unused_file_paths
            .iter()
            .filter(|p| subset.matches(p))
            .count();
        let dead_exports = self
            .unused_export_paths
            .iter()
            .filter(|p| subset.matches(p))
            .count();
        let unused_deps = self
            .unused_dep_package_paths
            .iter()
            .filter(|dep_path| dep_in_subset(subset, dep_path))
            .count();
        let circular_deps = self
            .circular_dep_groups
            .iter()
            .filter(|cycle| cycle.iter().any(|p| subset.matches(p)))
            .count();
        let total_exports = self
            .module_export_counts
            .iter()
            .filter(|(p, _)| subset.matches(p))
            .map(|(_, n)| *n)
            .sum();
        crate::vital_signs::AnalysisCounts {
            total_exports,
            dead_files,
            dead_exports,
            unused_deps,
            circular_deps,
            total_deps: defaults.total_deps,
        }
    }
}

/// Return true when an unused dependency's `package.json` path belongs to
/// the subset.
///
/// For [`crate::health::SubsetFilter::Paths`] the dep's containing workspace
/// (its `package.json` parent directory) is considered in scope when at
/// least one source file in the subset lives under that directory.
fn dep_in_subset(subset: &crate::health::SubsetFilter<'_>, dep_path: &std::path::Path) -> bool {
    match subset {
        crate::health::SubsetFilter::Full => true,
        crate::health::SubsetFilter::Paths(set) => {
            let Some(workspace_root) = dep_path.parent() else {
                return false;
            };
            set.iter().any(|p| p.starts_with(workspace_root))
        }
    }
}

/// Aggregate complexity totals from a parsed module.
///
/// Returns `(total_cyclomatic, total_cognitive, function_count, lines)`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "line count is bounded by source file size"
)]
fn aggregate_complexity(module: &crate::source::ModuleInfo) -> (u32, u32, usize, u32) {
    let cyc: u32 = module
        .complexity
        .iter()
        .map(|f| u32::from(f.cyclomatic))
        .sum();
    let cog: u32 = module
        .complexity
        .iter()
        .map(|f| u32::from(f.cognitive))
        .sum();
    let funcs = module.complexity.len();
    let lines = module.line_offsets.len() as u32;
    (cyc, cog, funcs, lines)
}

/// Compute the dead code ratio for a single file.
///
/// Returns the fraction of VALUE exports with zero references (0.0-1.0).
/// Type-only exports (interfaces, type aliases) are excluded from both
/// numerator and denominator to avoid inflating the ratio for well-typed
/// codebases. Returns 1.0 if the entire file is unused, 0.0 if it has no
/// value exports.
fn compute_dead_code_ratio(
    path: &std::path::Path,
    exports: &[fallow_graph::graph::ExportSymbol],
    unused_files: &rustc_hash::FxHashSet<&std::path::Path>,
    unused_exports_by_path: &rustc_hash::FxHashMap<&std::path::Path, usize>,
) -> f64 {
    if unused_files.contains(path) {
        return 1.0;
    }
    let value_exports = exports.iter().filter(|e| !e.is_type_only).count();
    if value_exports == 0 {
        return 0.0;
    }
    let unused = unused_exports_by_path.get(path).copied().unwrap_or(0);
    (unused as f64 / value_exports as f64).min(1.0)
}

/// Compute complexity density: total cyclomatic / lines of code.
///
/// Returns 0.0 when the file has no lines.
fn compute_complexity_density(total_cyclomatic: u32, lines: u32) -> f64 {
    if lines > 0 {
        f64::from(total_cyclomatic) / f64::from(lines)
    } else {
        0.0
    }
}

/// CRAP score threshold (inclusive). CC=5 untested gives exactly 30 (5^2 + 5),
/// matching the canonical CRAP threshold from Savoia & Evans (2007).
pub(super) const CRAP_THRESHOLD: f64 = 30.0;

/// Effective-threshold inputs for the CRAP columns of file scoring: the run
/// resolver plus the `enforce_crap` bit from the health scope.
#[derive(Clone, Copy)]
pub(super) struct CrapScoreThresholds<'a> {
    pub(super) resolver: &'a ThresholdOverrideResolver,
    pub(super) enforce_crap: bool,
}

/// Per-function effective-CRAP-ceiling lookup for one file's scoring loop:
/// the run resolver bound to the file's project-relative path, so the CRAP
/// loops can resolve each function's ceiling where the function name is in
/// hand. `enforce_crap` mirrors `HealthScope::enforce_crap`: a global ceiling
/// of `0` disables CRAP enforcement, so every function counts as exempt at the
/// canonical baseline instead of breaching a degenerate `0` ceiling.
pub(super) struct CrapCeilingLookup<'a> {
    resolver: &'a ThresholdOverrideResolver,
    relative: &'a std::path::Path,
    enforce_crap: bool,
}

/// Threshold-relative CRAP signals accumulated over one file's functions.
///
/// `above` and `exempted` count the ROUNDED per-function CRAP value, the same
/// value the findings pipeline compares against the effective ceiling, so a
/// boundary function cannot produce a finding without being counted here, or
/// be counted here without producing a finding.
#[derive(Debug, Default)]
struct CrapThresholdSignals {
    /// Functions whose rounded CRAP meets or exceeds their effective ceiling.
    above: usize,
    /// Functions at or above the canonical 30.0 baseline whose effective
    /// ceiling exempts them (all baseline-breaching functions when CRAP
    /// enforcement is disabled).
    exempted: usize,
    /// Lowest effective ceiling among the observed functions.
    min_ceiling: Option<f64>,
}

impl<'a> CrapCeilingLookup<'a> {
    pub(super) fn new(thresholds: CrapScoreThresholds<'a>, relative: &'a std::path::Path) -> Self {
        Self {
            resolver: thresholds.resolver,
            relative,
            enforce_crap: thresholds.enforce_crap,
        }
    }

    /// Fold one function's rounded CRAP value into the file's
    /// threshold-relative signals.
    fn observe(&self, function: &str, crap_rounded: f64, signals: &mut CrapThresholdSignals) {
        let ceiling = self.resolver.effective_max_crap(self.relative, function);
        signals.min_ceiling = Some(signals.min_ceiling.map_or(ceiling, |m| m.min(ceiling)));
        if !self.enforce_crap {
            if crap_rounded >= CRAP_THRESHOLD {
                signals.exempted += 1;
            }
        } else if crap_rounded >= ceiling {
            signals.above += 1;
        } else if crap_rounded >= CRAP_THRESHOLD {
            signals.exempted += 1;
        }
    }
}

/// Compute per-function CRAP scores using the static binary model.
///
/// Binary model: test-reachable file -> CRAP = CC, untested -> CRAP = CC^2 + CC.
/// Superseded by `compute_crap_scores_estimated` but retained for test coverage
/// of the binary formula behavior.
///
/// Returns `(max_crap, count_above_threshold)`.
#[cfg(test)]
#[expect(
    clippy::suboptimal_flops,
    reason = "cc * cc + cc matches the CRAP formula specification"
)]
fn compute_crap_scores_binary(
    complexity: &[fallow_types::extract::FunctionComplexity],
    is_test_reachable: bool,
) -> (f64, usize) {
    if complexity.is_empty() {
        return (0.0, 0);
    }
    let mut max = 0.0_f64;
    let mut above = 0usize;
    for f in complexity {
        let cc = f64::from(f.cyclomatic);
        let crap = if is_test_reachable { cc } else { cc * cc + cc };
        max = max.max(crap);
        if crap >= CRAP_THRESHOLD {
            above += 1;
        }
    }
    ((max * 10.0).round() / 10.0, above)
}

/// Per-function CRAP data used to emit `--max-crap` findings.
#[derive(Debug, Clone, Copy)]
pub struct PerFunctionCrap {
    /// 1-based line number of the function's definition.
    pub(crate) line: u32,
    /// 0-based column of the function's definition. Required alongside `line`
    /// to disambiguate curried arrows that share a start line, e.g.
    /// `(x) => (y) => {...}`. Without `col`, two `PerFunctionCrap` entries
    /// would collide in the (path, line) finding index and one function's
    /// CRAP score could be attached to another function's identity.
    pub(crate) col: u32,
    /// Computed CRAP score, rounded to one decimal place.
    pub(crate) crap: f64,
    /// Coverage percentage used to compute `crap`, when Istanbul matched the
    /// function. `None` for estimated coverage or unmatched functions.
    pub(crate) coverage_pct: Option<f64>,
    /// Bucketed coverage tier used to drive action selection in JSON output.
    /// Populated for both Istanbul-matched and estimated CRAP rows so the
    /// action builder does not need to recompute reachability state.
    pub(crate) coverage_tier: fallow_output::CoverageTier,
    /// Provenance of `coverage_tier` and `crap`. `Istanbul` for direct fnMap
    /// matches, `Estimated` for graph-based fallbacks against the finding's
    /// own file, `EstimatedComponentInherited` for the template-inherit path
    /// that reaches the owning Angular `.component.ts` through the inverse
    /// `templateUrl` edge. Threaded into `ComplexityViolation.coverage_source` by
    /// `merge_crap_findings`.
    pub(crate) coverage_source: fallow_output::CoverageSource,
}

/// Istanbul CRAP result: CRAP scores plus match statistics.
#[derive(Debug)]
struct IstanbulCrapResult {
    pub max_crap: f64,
    /// Threshold-relative counts and the lowest effective ceiling.
    pub signals: CrapThresholdSignals,
    /// Functions that found a match in Istanbul data.
    pub matched: usize,
    /// Total functions evaluated.
    pub total: usize,
    /// Per-function CRAP data indexed by function position within `complexity`.
    pub per_function: Vec<PerFunctionCrap>,
}

/// Compute per-function CRAP scores using Istanbul coverage data.
///
/// For each function, looks up its per-function statement coverage percentage
/// from the Istanbul data and applies the canonical CRAP formula:
/// `CRAP = CC^2 * (1 - cov/100)^3 + CC`
///
/// Functions not found in the coverage data fall back to the estimated model
/// using the file's test-reachability status.
///
/// Returns CRAP scores and match statistics for reporting.
fn compute_crap_scores_istanbul(
    complexity: &[fallow_types::extract::FunctionComplexity],
    file_coverage: Option<&IstanbulFileCoverage>,
    is_test_reachable: bool,
    ceilings: &CrapCeilingLookup<'_>,
) -> IstanbulCrapResult {
    if complexity.is_empty() {
        return IstanbulCrapResult {
            max_crap: 0.0,
            signals: CrapThresholdSignals::default(),
            matched: 0,
            total: 0,
            per_function: Vec::new(),
        };
    }
    let mut max = 0.0_f64;
    let mut signals = CrapThresholdSignals::default();
    let mut matched = 0usize;
    let mut total = 0usize;
    let mut per_function = Vec::with_capacity(complexity.len());
    for f in complexity {
        // Synthetic template-family units carry no measurable coverage (an
        // Istanbul fnMap can never contain them), so they are excluded from
        // the CRAP dimension entirely: no per-function entry, no max /
        // above-threshold contribution, no match-statistics slot.
        if fallow_types::extract::is_synthetic_template_unit(&f.name) {
            continue;
        }
        total += 1;
        let (crap, coverage_pct, tier, source) =
            crap_for_function(f, file_coverage, is_test_reachable, &mut matched);
        let crap_rounded = (crap * 10.0).round() / 10.0;
        max = max.max(crap);
        ceilings.observe(f.name.as_str(), crap_rounded, &mut signals);
        per_function.push(PerFunctionCrap {
            line: f.line,
            col: f.col,
            crap: crap_rounded,
            coverage_pct,
            coverage_tier: tier,
            coverage_source: source,
        });
    }
    IstanbulCrapResult {
        max_crap: (max * 10.0).round() / 10.0,
        signals,
        matched,
        total,
        per_function,
    }
}

/// Resolve one function's `(crap, coverage_pct, tier, source)` from Istanbul
/// coverage, falling back to the test-reachability estimate model. Increments
/// `matched` when a real coverage value is found.
#[expect(
    clippy::suboptimal_flops,
    reason = "cc * cc + cc matches the CRAP formula specification"
)]
fn crap_for_function(
    f: &fallow_types::extract::FunctionComplexity,
    file_coverage: Option<&IstanbulFileCoverage>,
    is_test_reachable: bool,
    matched: &mut usize,
) -> (
    f64,
    Option<f64>,
    fallow_output::CoverageTier,
    fallow_output::CoverageSource,
) {
    let cc = f64::from(f.cyclomatic);
    let lookup = file_coverage.and_then(|fc| fc.lookup_function(f));
    if let Some(cov_pct) = lookup {
        *matched += 1;
        return (
            crap_formula(cc, cov_pct),
            Some(cov_pct),
            fallow_output::CoverageTier::from_pct(cov_pct),
            fallow_output::CoverageSource::Istanbul,
        );
    }
    // The same static estimate the run without a coverage map would have
    // used. A map that says nothing about this function is not evidence that
    // it ran, so passing one must never lower its CRAP below the estimate.
    if is_test_reachable {
        return (
            crap_formula(cc, INDIRECT_TEST_COVERAGE_ESTIMATE),
            None,
            fallow_output::CoverageTier::from_pct(INDIRECT_TEST_COVERAGE_ESTIMATE),
            fallow_output::CoverageSource::Estimated,
        );
    }
    (
        cc * cc + cc,
        None,
        fallow_output::CoverageTier::None,
        fallow_output::CoverageSource::Estimated,
    )
}

/// Estimated coverage for functions directly referenced by test-reachable modules.
/// An export imported in a test file likely exercises most of the function body.
const DIRECT_TEST_COVERAGE_ESTIMATE: f64 = 85.0;

/// Estimated coverage for functions in test-reachable files but not directly
/// referenced by tests. The file is imported by tests, so the function may
/// be exercised indirectly, but with lower confidence.
const INDIRECT_TEST_COVERAGE_ESTIMATE: f64 = 40.0;
const MAX_DIRECT_CALLER_EVIDENCE: usize = 5;

/// Compute per-function CRAP scores using graph-based coverage estimation.
///
/// For each function, estimates coverage from the module graph:
/// - Function name matches an export with test-reachable references: 85%
/// - File is test-reachable but function not directly referenced: 40%
/// - File is not test-reachable at all: 0%
///
/// Applies the canonical CRAP formula with these estimates.
/// Returns `(max_crap, count_above_threshold)`.
/// Estimated CRAP result: score aggregates plus per-function data.
#[derive(Debug)]
struct EstimatedCrapResult {
    pub max_crap: f64,
    /// Threshold-relative counts and the lowest effective ceiling.
    pub signals: CrapThresholdSignals,
    pub per_function: Vec<PerFunctionCrap>,
}

fn compute_crap_scores_estimated(
    complexity: &[fallow_types::extract::FunctionComplexity],
    test_referenced_exports: &rustc_hash::FxHashSet<String>,
    is_test_reachable: bool,
    coverage_source: fallow_output::CoverageSource,
    ceilings: &CrapCeilingLookup<'_>,
) -> EstimatedCrapResult {
    if complexity.is_empty() {
        return EstimatedCrapResult {
            max_crap: 0.0,
            signals: CrapThresholdSignals::default(),
            per_function: Vec::new(),
        };
    }
    let mut max = 0.0_f64;
    let mut signals = CrapThresholdSignals::default();
    let mut per_function = Vec::with_capacity(complexity.len());
    for f in complexity {
        // Template-family units leave the CRAP dimension: their name never
        // appears in `test_referenced_exports`, so the estimate could only
        // ever restate the file's reachability as a disguised cyclomatic gate.
        if fallow_types::extract::is_synthetic_template_unit(&f.name) {
            continue;
        }
        let cc = f64::from(f.cyclomatic);
        let estimated_coverage = if test_referenced_exports.contains(f.name.as_str()) {
            DIRECT_TEST_COVERAGE_ESTIMATE
        } else if is_test_reachable {
            INDIRECT_TEST_COVERAGE_ESTIMATE
        } else {
            0.0
        };
        let crap = crap_formula(cc, estimated_coverage);
        let crap_rounded = (crap * 10.0).round() / 10.0;
        max = max.max(crap);
        ceilings.observe(f.name.as_str(), crap_rounded, &mut signals);
        per_function.push(PerFunctionCrap {
            line: f.line,
            col: f.col,
            crap: crap_rounded,
            coverage_pct: None,
            coverage_tier: fallow_output::CoverageTier::from_pct(estimated_coverage),
            coverage_source,
        });
    }
    EstimatedCrapResult {
        max_crap: (max * 10.0).round() / 10.0,
        signals,
        per_function,
    }
}

/// Inherited CRAP context for a synthetic `<template>` finding on an Angular
/// `.html` template. Populated by `build_template_inherit_contexts` for every
/// `.html` module that has a `<template>` `FunctionComplexity` entry AND is
/// reached by at least one non-test `.ts` importer via the `templateUrl`
/// `SideEffect` edge.
///
/// The reachability bit is the OR across all non-test `.ts` owners (any
/// tested owner makes the template tested); the `test_referenced_exports`
/// set is the union of each owner's directly-test-referenced export names;
/// the provenance path points at the chosen owner for human output. When
/// multiple owners exist, prefer the first test-reachable one so the
/// "inherited from" suffix points at a meaningful owner rather than an
/// arbitrary first match.
#[derive(Debug, Clone)]
pub(super) struct TemplateInheritContext {
    pub is_test_reachable: bool,
    pub test_referenced_exports: rustc_hash::FxHashSet<String>,
    /// The owning `.ts` file path used for human-output provenance
    /// (`coverage: partial (inherited from foo.component.ts)`). Set to the
    /// first test-reachable owner when one exists, otherwise the first
    /// non-test owner. Absolute path; the human formatter strips it.
    pub provenance_owner: std::path::PathBuf,
}

/// Build the inverse `templateUrl` redirect map: for every `.html` module
/// carrying a synthetic `<template>` `FunctionComplexity` entry, walk
/// `reverse_deps` to find every `.ts` (or `.component.ts`) importer that is
/// NOT a test entry point, and compute an aggregate `TemplateInheritContext`
/// that the CRAP scoring loop can use to redirect reachability + test refs
/// to the owning component file.
///
/// Test-file owners are excluded because Angular spec files do not declare
/// `templateUrl`; if a `.spec.ts` is the only importer of a `.html`, the
/// template is genuinely orphaned and the existing fallback (estimated
/// against the `.html`'s own reachability) is the right answer.
///
/// The `.ts` / `.tsx` / `.mts` / `.cts` extension gate intentionally lets
/// `.d.ts` ambient declarations through, but Angular component classes are
/// not emitted into `.d.ts` files (which model APIs, not runtime behaviour)
/// and `templateUrl` SideEffect edges flow only from concrete `@Component`
/// decorators. A `.d.ts` importer of a `.html` would be a structural
/// anomaly upstream, not a meaningful owner, so the gate stays simple.
///
/// Templates with zero non-test `.ts` owners receive no entry, so the
/// scoring loop falls through to the existing path unchanged.
fn build_template_inherit_contexts(
    graph: &fallow_graph::graph::ModuleGraph,
    test_coverage: StaticTestCoverage<'_>,
    module_by_id: &rustc_hash::FxHashMap<crate::discover::FileId, &crate::source::ModuleInfo>,
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
) -> rustc_hash::FxHashMap<crate::discover::FileId, TemplateInheritContext> {
    let mut out = rustc_hash::FxHashMap::default();
    for node in &graph.modules {
        if let Some(context) =
            template_inherit_context_for_node(node, graph, test_coverage, module_by_id, file_paths)
        {
            out.insert(node.file_id, context);
        }
    }
    out
}

fn template_inherit_context_for_node(
    node: &fallow_graph::graph::ModuleNode,
    graph: &fallow_graph::graph::ModuleGraph,
    test_coverage: StaticTestCoverage<'_>,
    module_by_id: &rustc_hash::FxHashMap<crate::discover::FileId, &crate::source::ModuleInfo>,
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
) -> Option<TemplateInheritContext> {
    if !is_template_inherit_candidate(node, module_by_id, file_paths) {
        return None;
    }
    let importers = graph.reverse_deps.get(node.file_id.0 as usize)?;
    template_inherit_context_from_importers(
        importers,
        graph,
        test_coverage,
        module_by_id,
        file_paths,
    )
}

fn is_template_inherit_candidate(
    node: &fallow_graph::graph::ModuleNode,
    module_by_id: &rustc_hash::FxHashMap<crate::discover::FileId, &crate::source::ModuleInfo>,
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
) -> bool {
    let Some(path) = file_paths.get(&node.file_id) else {
        return false;
    };
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
    {
        return false;
    }
    module_by_id.get(&node.file_id).is_some_and(|module| {
        module
            .complexity
            .iter()
            .any(|finding| finding.name.as_str() == "<template>")
    })
}

fn template_inherit_context_from_importers(
    importers: &[crate::discover::FileId],
    graph: &fallow_graph::graph::ModuleGraph,
    test_coverage: StaticTestCoverage<'_>,
    module_by_id: &rustc_hash::FxHashMap<crate::discover::FileId, &crate::source::ModuleInfo>,
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
) -> Option<TemplateInheritContext> {
    let mut any_reachable = false;
    let mut combined_refs = rustc_hash::FxHashSet::default();
    let mut provenance: Option<std::path::PathBuf> = None;
    let mut first_owner: Option<std::path::PathBuf> = None;

    for &importer_id in importers {
        let Some((owner_node, owner_path)) =
            template_owner(importer_id, graph, module_by_id, file_paths)
        else {
            continue;
        };
        if first_owner.is_none() {
            first_owner = Some((*owner_path).clone());
        }
        if test_coverage.covers_file(owner_node.file_id) {
            any_reachable = true;
            provenance.get_or_insert_with(|| (*owner_path).clone());
            let refs = build_test_referenced_exports(&owner_node.exports, test_coverage);
            combined_refs.extend(refs);
        }
    }

    let provenance_owner = provenance.or(first_owner)?;
    Some(TemplateInheritContext {
        is_test_reachable: any_reachable,
        test_referenced_exports: combined_refs,
        provenance_owner,
    })
}

fn template_owner<'a>(
    importer_id: crate::discover::FileId,
    graph: &'a fallow_graph::graph::ModuleGraph,
    module_by_id: &rustc_hash::FxHashMap<crate::discover::FileId, &crate::source::ModuleInfo>,
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
) -> Option<(&'a fallow_graph::graph::ModuleNode, &'a std::path::PathBuf)> {
    let owner_node = graph.modules.get(importer_id.0 as usize)?;
    let owner_path = *file_paths.get(&importer_id)?;
    if !is_template_owner_path(owner_path) || graph.test_entry_points.contains(&importer_id) {
        return None;
    }
    let owner_has_component = module_by_id
        .get(&importer_id)
        .is_some_and(|module| module.has_angular_component_template_url);
    owner_has_component.then_some((owner_node, owner_path))
}

fn is_template_owner_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "ts" | "tsx" | "mts" | "cts"
            )
        })
}

/// Build the set of export names that have at least one test-reachable reference.
///
/// This is the per-function signal: if an export named "foo" has a reference from
/// a test-reachable module, the function "foo" is considered directly tested.
fn build_test_referenced_exports(
    exports: &[fallow_graph::graph::ExportSymbol],
    test_coverage: StaticTestCoverage<'_>,
) -> rustc_hash::FxHashSet<String> {
    let mut set = rustc_hash::FxHashSet::default();
    for export in exports {
        if export.is_type_only {
            continue;
        }
        let has_test_ref = test_coverage.covers_any_reference(export);
        if has_test_ref {
            set.insert(export.name.to_string());
        }
    }
    set
}

fn collect_direct_callers(
    graph: &fallow_graph::graph::ModuleGraph,
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
) -> rustc_hash::FxHashMap<std::path::PathBuf, Vec<DirectCallerEvidence>> {
    let mut callers_by_target = rustc_hash::FxHashMap::default();
    for node in &graph.modules {
        let Some(target_path) = file_paths.get(&node.file_id) else {
            continue;
        };
        let mut callers = graph
            .direct_importer_summaries(node.file_id)
            .into_iter()
            .filter_map(|summary| {
                file_paths
                    .get(&summary.source)
                    .map(|caller_path| DirectCallerEvidence {
                        path: (*caller_path).clone(),
                        symbols: summary
                            .symbols
                            .into_iter()
                            .map(|symbol| DirectCallerSymbolEvidence {
                                imported: symbol.imported,
                                local: symbol.local,
                                type_only: symbol.type_only,
                            })
                            .collect(),
                    })
            })
            .collect::<Vec<_>>();
        callers.sort_by(|a, b| a.path.cmp(&b.path));
        callers.truncate(MAX_DIRECT_CALLER_EVIDENCE);
        if !callers.is_empty() {
            callers_by_target.insert((*target_path).clone(), callers);
        }
    }
    callers_by_target
}

/// Canonical CRAP formula: `CC^2 * (1 - cov/100)^3 + CC`.
/// At 100% coverage: CRAP = CC. At 0% coverage: CRAP = CC^2 + CC.
#[expect(
    clippy::suboptimal_flops,
    reason = "explicit multiplication matches the CRAP formula specification"
)]
fn crap_formula(cc: f64, coverage_pct: f64) -> f64 {
    let uncovered = 1.0 - coverage_pct / 100.0;
    cc * cc * uncovered * uncovered * uncovered + cc
}

/// Maximum column drift tolerated when the anonymous-by-position fallback
/// matches a candidate on a nearby line. Wide enough to accept curried arrows
/// and chained callbacks that share a leading indent, tight enough to reject
/// `function foo()` at column 0 when the only candidate is a multiline-arrow
/// declaration alias at the typical `const x = async (` column.
const ANONYMOUS_FALLBACK_MAX_COLUMN_DRIFT: u32 = 16;

/// Maximum line drift tolerated by the name-fuzzy and anonymous fallbacks.
/// Both reject an alias further than this from the target, which is what lets
/// a lookup binary-search a window of `IstanbulFileCoverage::alias_lines`
/// instead of scanning every record in the file.
const ALIAS_FUZZ_MAX_LINE_DRIFT: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct IstanbulPosition {
    line: u32,
    col: u32,
}

impl IstanbulPosition {
    const fn new(line: u32, col: u32) -> Self {
        Self { line, col }
    }

    const fn distance_from(self, target: Self) -> (u32, u32) {
        (
            self.line.abs_diff(target.line),
            self.col.abs_diff(target.col),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IstanbulSpan {
    start: IstanbulPosition,
    end: IstanbulPosition,
}

impl IstanbulSpan {
    fn from_entry(
        fn_entry: &oxc_coverage_instrument::FnEntry,
        source_index: Option<&IstanbulSourceIndex<'_>>,
    ) -> Option<Self> {
        let start = normalized_istanbul_position(
            fn_entry.loc.start.line,
            fn_entry.loc.start.column,
            source_index,
        )?;
        let end = normalized_istanbul_position(
            fn_entry.loc.end.line,
            fn_entry.loc.end.column,
            source_index,
        )?;
        (start.line > 0 && end.line > 0 && start < end).then_some(Self { start, end })
    }

    fn header_from_entry(
        fn_entry: &oxc_coverage_instrument::FnEntry,
        source_index: Option<&IstanbulSourceIndex<'_>>,
    ) -> Option<Self> {
        let start = normalized_istanbul_position(
            fn_entry.decl.start.line,
            fn_entry.decl.start.column,
            source_index,
        )?;
        let end = normalized_istanbul_position(
            fn_entry.loc.start.line,
            fn_entry.loc.start.column,
            source_index,
        )?;
        (start.line > 0 && end.line > 0 && start < end).then_some(Self { start, end })
    }

    /// Half-open containment: `end` is the position just past the body.
    fn contains(self, position: IstanbulPosition) -> bool {
        self.start <= position && position < self.end
    }

    fn strictly_contains(self, other: Self) -> bool {
        self != other && self.start <= other.start && other.end <= self.end
    }
}

/// A position under which an Istanbul function record can be found.
///
/// The producer's effective position and `decl.start` are primary: they
/// identify the function itself. The body start (`loc.start`) is secondary.
/// istanbul-lib-instrument records an expression-bodied arrow's `loc` as the
/// body expression, so for curried arrows the outer entry's body start is the
/// inner entry's declaration start. A secondary alias therefore yields to a
/// primary alias at the same position instead of making it ambiguous.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IstanbulAlias {
    position: IstanbulPosition,
    primary: bool,
}

/// How many primary and secondary aliases share one position within a
/// collision namespace (per name, or across all anonymous records).
#[derive(Clone, Copy, Debug, Default)]
struct IstanbulAliasCounts {
    primary: usize,
    secondary: usize,
}

impl IstanbulAliasCounts {
    fn add(&mut self, alias: IstanbulAlias) {
        if alias.primary {
            self.primary += 1;
        } else {
            self.secondary += 1;
        }
    }

    /// A primary alias is unique when no other primary shares its position;
    /// a secondary alias is unique only when nothing else does.
    const fn is_unique(self, alias: IstanbulAlias) -> bool {
        if alias.primary {
            self.primary == 1
        } else {
            self.primary == 0 && self.secondary == 1
        }
    }

    const fn has_secondary_only_collision(self) -> bool {
        self.primary == 0 && self.secondary > 1
    }
}

fn is_anonymous_istanbul_name(name: &str) -> bool {
    name.starts_with("(anonymous_")
}

struct IstanbulFunctionCoverage {
    name: String,
    coverage_pct: f64,
    aliases: Vec<IstanbulAlias>,
    /// Where the producer says the function is written. Unlike the alias set
    /// this is never a synthesized position, so it answers "is another
    /// function declared here" without false positives.
    decl_start: IstanbulPosition,
    /// Whether another function is declared inside this record's header span.
    /// Computed once per file, because the header fallback needs it on every
    /// unmatched lookup and recomputing it there is quadratic.
    header_holds_other_fn: bool,
    header_span: Option<IstanbulSpan>,
    body_span: Option<IstanbulSpan>,
}

impl IstanbulFunctionCoverage {
    fn nearest_alias(
        &self,
        target: IstanbulPosition,
        max_column_drift: Option<u32>,
    ) -> Option<(u32, u32)> {
        self.aliases
            .iter()
            .filter_map(|alias| {
                let distance = alias.position.distance_from(target);
                if distance.0 > ALIAS_FUZZ_MAX_LINE_DRIFT {
                    return None;
                }
                if distance.0 > 0 && max_column_drift.is_some_and(|maximum| distance.1 > maximum) {
                    return None;
                }
                Some(distance)
            })
            .min()
    }
}

/// Pre-processed per-function coverage data for a single file,
/// derived from Istanbul `coverage-final.json`.
pub struct IstanbulFileCoverage {
    /// One record per Istanbul `fnMap` identity. Each record owns its
    /// deduplicated producer, declaration, and valid body-start aliases, so an
    /// entry cannot tie with itself during anonymous positional matching.
    functions: Vec<IstanbulFunctionCoverage>,
    /// Exact aliases that belong to one function identity. Primary aliases
    /// that collide are removed from this index and recorded in
    /// `ambiguous_aliases`. A secondary alias yields to a primary collision,
    /// while secondary-only collisions are also recorded as ambiguous.
    alias_index: rustc_hash::FxHashMap<(String, u32, u32), usize>,
    /// Aliases shared by multiple function identities without a unique
    /// primary owner. Exact queries at these positions abstain instead of
    /// falling through to a fuzzy attribution.
    ambiguous_aliases: rustc_hash::FxHashSet<(String, u32, u32)>,
    /// Positions shared by multiple anonymous identities without a unique
    /// primary owner. Anonymous fallback abstains at these targets even when
    /// each identity has other aliases.
    ambiguous_anonymous_aliases: rustc_hash::FxHashSet<IstanbulPosition>,
    /// `(alias line, function index)` for every retained alias, sorted.
    /// The fuzzy and anonymous fallbacks reject any alias further than
    /// [`ALIAS_FUZZ_MAX_LINE_DRIFT`] lines from the target, so they can search
    /// a window of this index instead of every record in the file.
    alias_lines: Vec<(u32, usize)>,
    /// `(header span start line, function index)` for every record that has a
    /// header span, sorted.
    header_starts: Vec<(u32, usize)>,
    /// Height in lines of the tallest header span in this file. A span reaches
    /// no further than this below its own start, which bounds the window
    /// searched in `header_starts`.
    max_header_height: u32,
    /// The coverage map was recorded against a different checkout of the
    /// project, so line numbers may have drifted beyond the bounded fuzz.
    /// Enables the distance-free unambiguous-name fallback in [`Self::lookup`].
    relocated: bool,
}

/// Records whether each header span has a second function declared inside it,
/// which makes the span say nothing about a position it contains.
///
/// Computed once per file because the header fallback needs the answer on
/// every unmatched lookup and recomputing it there would be quadratic.
fn mark_headers_holding_other_fns(functions: &mut [IstanbulFunctionCoverage]) {
    let mut declarations: Vec<IstanbulPosition> = functions
        .iter()
        .map(|function| function.decl_start)
        .collect();
    declarations.sort_unstable();
    for function in functions {
        let Some(span) = function.header_span else {
            continue;
        };
        // The record's own declaration opens its header span, so it is always
        // in range: a second hit is what marks a foreign function.
        let first = declarations.partition_point(|position| *position < span.start);
        let past = declarations.partition_point(|position| *position < span.end);
        function.header_holds_other_fn = past - first > 1;
    }
}

/// Line-keyed indexes over one file's records, built once per file so the
/// fuzzy, anonymous, and header fallbacks can binary-search a bounded window
/// instead of rescanning every record on every unmatched lookup.
struct IstanbulLineIndexes {
    alias_lines: Vec<(u32, usize)>,
    header_starts: Vec<(u32, usize)>,
    max_header_height: u32,
}

impl IstanbulLineIndexes {
    fn build(functions: &[IstanbulFunctionCoverage]) -> Self {
        let mut alias_lines: Vec<(u32, usize)> = functions
            .iter()
            .enumerate()
            .flat_map(|(function_index, function)| {
                function
                    .aliases
                    .iter()
                    .map(move |alias| (alias.position.line, function_index))
            })
            .collect();
        alias_lines.sort_unstable();

        let mut header_starts: Vec<(u32, usize)> = Vec::new();
        let mut max_header_height = 0;
        for (function_index, function) in functions.iter().enumerate() {
            if let Some(span) = function.header_span {
                header_starts.push((span.start.line, function_index));
                // `IstanbulSpan` construction requires `start < end`, so the
                // end line is never above the start line.
                max_header_height = max_header_height.max(span.end.line - span.start.line);
            }
        }
        header_starts.sort_unstable();

        Self {
            alias_lines,
            header_starts,
            max_header_height,
        }
    }
}

impl IstanbulFileCoverage {
    /// Coverage for an extracted unit.
    ///
    /// The entry point callers outside this module get, because the
    /// private-member rule has to hold for every consumer: gating it at one
    /// call site leaves the next one free to reintroduce the bug.
    pub fn lookup_function(
        &self,
        function: &fallow_types::extract::FunctionComplexity,
    ) -> Option<f64> {
        if function.is_private_member {
            return None;
        }
        self.lookup(function.name.as_str(), function.line, function.col)
    }

    fn new(mut functions: Vec<IstanbulFunctionCoverage>, relocated: bool) -> Self {
        let mut named_alias_counts: rustc_hash::FxHashMap<
            (String, IstanbulPosition),
            IstanbulAliasCounts,
        > = rustc_hash::FxHashMap::default();
        let mut anonymous_alias_counts: rustc_hash::FxHashMap<
            IstanbulPosition,
            IstanbulAliasCounts,
        > = rustc_hash::FxHashMap::default();
        for function in &functions {
            let is_anonymous = is_anonymous_istanbul_name(&function.name);
            for alias in &function.aliases {
                named_alias_counts
                    .entry((function.name.clone(), alias.position))
                    .or_default()
                    .add(*alias);
                if is_anonymous {
                    anonymous_alias_counts
                        .entry(alias.position)
                        .or_default()
                        .add(*alias);
                }
            }
        }

        // A secondary alias that collides with a primary is dropped, which
        // leaves the primary owner of that position unique. Primary/primary
        // and secondary-only collisions remain ambiguous so fuzzy fallback
        // cannot attribute the shared position to an arbitrary record.
        let mut ambiguous_aliases = rustc_hash::FxHashSet::default();
        let mut ambiguous_anonymous_aliases = rustc_hash::FxHashSet::default();
        for function in &mut functions {
            let name = function.name.clone();
            let is_anonymous = is_anonymous_istanbul_name(&name);
            function.aliases.retain(|alias| {
                let named = named_alias_counts
                    .get(&(name.clone(), alias.position))
                    .copied()
                    .unwrap_or_default();
                let anonymous = is_anonymous
                    .then(|| anonymous_alias_counts.get(&alias.position).copied())
                    .flatten();
                let unique = named.is_unique(*alias)
                    && anonymous.is_none_or(|counts| counts.is_unique(*alias));
                if unique {
                    return true;
                }
                if alias.primary {
                    ambiguous_aliases.insert((
                        name.clone(),
                        alias.position.line,
                        alias.position.col,
                    ));
                    if anonymous.is_some_and(|counts| counts.primary > 1) {
                        ambiguous_anonymous_aliases.insert(alias.position);
                    }
                } else {
                    if named.has_secondary_only_collision() {
                        ambiguous_aliases.insert((
                            name.clone(),
                            alias.position.line,
                            alias.position.col,
                        ));
                    }
                    if anonymous.is_some_and(IstanbulAliasCounts::has_secondary_only_collision) {
                        ambiguous_anonymous_aliases.insert(alias.position);
                    }
                }
                false
            });
        }

        let mut alias_index = rustc_hash::FxHashMap::default();
        for (function_index, function) in functions.iter().enumerate() {
            for alias in &function.aliases {
                alias_index.insert(
                    (
                        function.name.clone(),
                        alias.position.line,
                        alias.position.col,
                    ),
                    function_index,
                );
            }
        }

        mark_headers_holding_other_fns(&mut functions);
        let indexes = IstanbulLineIndexes::build(&functions);

        Self {
            functions,
            alias_index,
            ambiguous_aliases,
            ambiguous_anonymous_aliases,
            alias_lines: indexes.alias_lines,
            header_starts: indexes.header_starts,
            max_header_height: indexes.max_header_height,
            relocated,
        }
    }

    /// Indices of the records with an alias within
    /// [`ALIAS_FUZZ_MAX_LINE_DRIFT`] lines of `target`, ascending and
    /// deduplicated.
    ///
    /// This is exactly the candidate set of the fuzzy and anonymous fallbacks:
    /// `nearest_alias` yields `None` for a record whose every alias is further
    /// away, so a record outside the window cannot produce a distance and
    /// cannot win either fallback. Ascending order reproduces the order of a
    /// full `self.functions` scan, which both fallbacks' tie-breaks depend on:
    /// `min_by_key` keeps the first minimum, and the anonymous scan appends
    /// equal-distance records in encounter order.
    fn fuzz_window(&self, target: IstanbulPosition) -> Vec<usize> {
        let low = target.line.saturating_sub(ALIAS_FUZZ_MAX_LINE_DRIFT);
        let high = target.line.saturating_add(ALIAS_FUZZ_MAX_LINE_DRIFT);
        let first = self.alias_lines.partition_point(|(line, _)| *line < low);
        let past = self.alias_lines.partition_point(|(line, _)| *line <= high);
        let mut window: Vec<usize> = self.alias_lines[first..past]
            .iter()
            .map(|(_, function_index)| *function_index)
            .collect();
        window.sort_unstable();
        window.dedup();
        window
    }

    /// Look up coverage for a function by name, start line, and start column.
    ///
    /// Resolution order:
    /// 1. Exact `(name, line, col)` match.
    /// 2. Name-only fuzzy match within ±2 lines (tolerates formatter drift),
    ///    tie-broken by smallest `(line, col)` distance from the target.
    /// 3. Anonymous fallback: among Istanbul `(anonymous_N)` records with an
    ///    alias within ±2 lines, pick the record whose closest alias has the
    ///    smallest `(line, col)` distance from the target. A distance tie is
    ///    resolved only when one valid body span contains the target and is
    ///    strictly inside every other tied span.
    /// 4. Unique anonymous header-span fallback between `decl.start` and
    ///    `loc.start`.
    ///
    /// Step 3 covers arrow-function exports where fallow extracts the binding
    /// identifier
    /// (`const myHandler = () => {...}` yields `myHandler`) while Istanbul
    /// records the function as anonymous. `load_istanbul_coverage` indexes
    /// declaration aliases so standard Istanbul producers still participate
    /// in this fallback. Step 4 covers multi-line callback arguments whose
    /// extracted start falls between Istanbul's declaration and body anchors,
    /// but only after the established anonymous resolution cannot decide. See
    /// issues #155, #166, #181, and #370.
    ///
    /// When the map is `relocated` (recorded against a different checkout of
    /// the project, as in the audit base-worktree pass), a distance-free
    /// name match runs between steps 2 and 3: if every entry named `name`
    /// carries the same coverage value, that value is returned regardless of
    /// line drift. Unrelated edits can shift a function arbitrarily far
    /// between the two checkouts, and when all same-named candidates agree
    /// the choice among them cannot matter (#2347). Same-checkout lookups
    /// keep the bounded fuzz, which protects against stale coverage data.
    pub(crate) fn lookup(&self, name: &str, line: u32, col: u32) -> Option<f64> {
        let exact_key = (name.to_string(), line, col);
        if self.ambiguous_aliases.contains(&exact_key) {
            return None;
        }
        if let Some(&function_index) = self.alias_index.get(&exact_key) {
            return Some(self.functions[function_index].coverage_pct);
        }

        let target = IstanbulPosition::new(line, col);
        let window = self.fuzz_window(target);
        // A function written inside another function's signature is a
        // different function from the one the signature belongs to, and both
        // sit within a line or two of the same position. Proximity cannot
        // tell them apart, so the records whose signature covers this target
        // decide which candidates below are eligible at all.
        let signature_owners = self.header_spans_containing(target);
        if let Some(function) = window
            .iter()
            .copied()
            .filter(|function_index| self.functions[*function_index].name == name)
            .filter_map(|function_index| {
                let function = &self.functions[function_index];
                function
                    .nearest_alias(target, None)
                    .map(|distance| (distance, function_index))
            })
            .filter(|(distance, function_index)| {
                *distance == (0, 0)
                    || !self.foreign_signature_blocks(&signature_owners, *function_index, target)
            })
            .min_by_key(|(distance, _)| *distance)
            .map(|(_, function_index)| &self.functions[function_index])
        {
            return Some(function.coverage_pct);
        }
        if self.relocated
            && let Some(pct) = self.unambiguous_named_pct(name)
        {
            return Some(pct);
        }
        if self.ambiguous_anonymous_aliases.contains(&target) {
            return self.unique_anonymous_header_match(&signature_owners);
        }

        let mut nearest_distance: Option<(u32, u32)> = None;
        let mut nearest_functions = Vec::new();
        for function_index in window {
            let function = &self.functions[function_index];
            if !is_anonymous_istanbul_name(&function.name) {
                continue;
            }
            let Some(distance) =
                function.nearest_alias(target, Some(ANONYMOUS_FALLBACK_MAX_COLUMN_DRIFT))
            else {
                continue;
            };
            // Inside a signature, proximity alone cannot tell a member from
            // the function written in its parameter list. A candidate that
            // neither sits on the target nor contains it is the wrong one.
            if distance != (0, 0)
                && self.foreign_signature_blocks(&signature_owners, function_index, target)
            {
                continue;
            }
            match nearest_distance {
                None => {
                    nearest_distance = Some(distance);
                    nearest_functions.push(function_index);
                }
                Some(previous) if distance < previous => {
                    nearest_distance = Some(distance);
                    nearest_functions.clear();
                    nearest_functions.push(function_index);
                }
                Some(previous) if distance == previous => {
                    nearest_functions.push(function_index);
                }
                Some(_) => {}
            }
        }
        let established_match = match nearest_functions.as_slice() {
            [] => None,
            [function_index] => Some(self.functions[*function_index].coverage_pct),
            tied => self.innermost_anonymous_match(tied, target),
        };
        if established_match.is_some() {
            return established_match;
        }

        self.unique_anonymous_header_match(&signature_owners)
    }

    /// Indices of the records whose header span contains `target`, ascending.
    ///
    /// A span reaches `target` from no further than `max_header_height` lines
    /// above it, so every containing span starts inside that window.
    fn header_spans_containing(&self, target: IstanbulPosition) -> Vec<usize> {
        let low = target.line.saturating_sub(self.max_header_height);
        let first = self.header_starts.partition_point(|(line, _)| *line < low);
        let past = self
            .header_starts
            .partition_point(|(line, _)| *line <= target.line);
        self.header_starts[first..past]
            .iter()
            .filter(|(_, function_index)| {
                self.functions[*function_index]
                    .header_span
                    .is_some_and(|span| span.contains(target))
            })
            .map(|(_, function_index)| *function_index)
            .collect()
    }

    /// Whether `candidate` is the wrong record for a target inside a
    /// signature it does not own.
    ///
    /// A default parameter value, a decorator argument, and a class
    /// expression in a signature are functions of their own, written a line
    /// or two from the member whose signature holds them. Crediting one by
    /// proximity reports its coverage for a function that never shared its
    /// fate. A candidate that contains the target in its own signature or
    /// body is not that case: the target is its code, however the enclosing
    /// signature encloses both.
    fn foreign_signature_blocks(
        &self,
        owners: &[usize],
        candidate: usize,
        target: IstanbulPosition,
    ) -> bool {
        let function = &self.functions[candidate];
        if function
            .header_span
            .is_some_and(|span| span.contains(target))
            || function.body_span.is_some_and(|span| span.contains(target))
        {
            return false;
        }
        owners.iter().any(|&owner| {
            owner != candidate
                && self.functions[owner]
                    .header_span
                    .is_some_and(|span| span.contains(function.decl_start))
        })
    }

    /// Coverage of the single anonymous record whose signature owns the
    /// target, given every record whose header span covers it.
    ///
    /// A header span runs from `decl.start` to `loc.start`, so it covers the
    /// signature: the parameter list, its default values, and any decorators.
    /// A function literal written there is a different function from the one
    /// that owns the span, and crediting it with the owner's coverage reports
    /// never-executed code as covered. Two containing spans therefore abstain,
    /// as does a span with a second function declared inside it. Only an
    /// anonymous record can win, because a named record is already reachable
    /// through the exact and fuzzy name paths.
    fn unique_anonymous_header_match(&self, signature_owners: &[usize]) -> Option<f64> {
        let [function_index] = signature_owners else {
            return None;
        };
        let function = &self.functions[*function_index];
        if !is_anonymous_istanbul_name(&function.name) || function.header_holds_other_fn {
            return None;
        }
        Some(function.coverage_pct)
    }

    fn innermost_anonymous_match(&self, tied: &[usize], target: IstanbulPosition) -> Option<f64> {
        let containing: Option<Vec<_>> = tied
            .iter()
            .map(|&function_index| {
                self.functions[function_index]
                    .body_span
                    .filter(|span| span.contains(target))
                    .map(|span| (function_index, span))
            })
            .collect();
        let containing = containing?;

        let mut winner = None;
        for &(function_index, candidate_span) in &containing {
            let is_strictly_innermost = containing.iter().all(|&(other_index, other_span)| {
                other_index == function_index || other_span.strictly_contains(candidate_span)
            });
            if !is_strictly_innermost {
                continue;
            }
            if winner.replace(function_index).is_some() {
                return None;
            }
        }
        winner.map(|function_index| self.functions[function_index].coverage_pct)
    }

    /// The single coverage value shared by every entry named `name`, or
    /// `None` when the name is absent or its records disagree. Bit-exact
    /// comparison: distinct functions whose values coincide cannot change the
    /// result regardless of which record matched.
    fn unambiguous_named_pct(&self, name: &str) -> Option<f64> {
        let mut found: Option<f64> = None;
        for function in &self.functions {
            if function.name != name {
                continue;
            }
            match found {
                None => found = Some(function.coverage_pct),
                Some(previous) if previous.to_bits() == function.coverage_pct.to_bits() => {}
                Some(_) => return None,
            }
        }
        found
    }
}

/// Loaded Istanbul coverage data, keyed by canonical file path.
pub struct IstanbulCoverage {
    files: rustc_hash::FxHashMap<std::path::PathBuf, IstanbulFileCoverage>,
}

impl IstanbulCoverage {
    /// Get coverage data for a file path.
    pub fn get(&self, path: &std::path::Path) -> Option<&IstanbulFileCoverage> {
        self.files.get(path)
    }

    /// How many files the coverage map describes, joined or not.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

/// Precedence decision for per-function CRAP coverage inputs.
///
/// Template inheritance wins first so Angular `.html` template findings can
/// use the owning `.component.ts` reachability context. Istanbul wins next,
/// even when the current file is missing from the coverage map, because that
/// path still records unmatched functions in the run-level match counters.
/// Plain graph-estimated coverage is the final fallback.
enum CrapCoverageResolution<'a> {
    TemplateInherited(&'a TemplateInheritContext),
    Istanbul {
        file_coverage: Option<&'a IstanbulFileCoverage>,
    },
    StaticEstimated,
}

fn resolve_crap_coverage<'a>(
    template_inherit: Option<&'a TemplateInheritContext>,
    istanbul_coverage: Option<&'a IstanbulCoverage>,
    path: &std::path::Path,
) -> CrapCoverageResolution<'a> {
    if let Some(inherit_ctx) = template_inherit {
        CrapCoverageResolution::TemplateInherited(inherit_ctx)
    } else if let Some(istanbul) = istanbul_coverage {
        let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        CrapCoverageResolution::Istanbul {
            file_coverage: istanbul.get(&canonical),
        }
    } else {
        CrapCoverageResolution::StaticEstimated
    }
}

/// Auto-detect a `coverage-final.json` file in common locations relative to the project root.
///
/// Checks (in order): `coverage/coverage-final.json`, `.nyc_output/coverage-final.json`.
/// Returns the first path found, or `None` if no coverage file exists.
/// The audit base-worktree pass uses the same detection against the head
/// project root, so auto-detected coverage scores both attribution sides
/// (#2347).
pub fn auto_detect_coverage(root: &std::path::Path) -> Option<std::path::PathBuf> {
    let candidates = [
        root.join("coverage/coverage-final.json"),
        root.join(".nyc_output/coverage-final.json"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// Resolve a relative path against the fallow project root. Returns `path`
/// unchanged when it is absolute or `project_root` is `None`. Matches the
/// convention every other path-shaped CLI input uses, so a monorepo CI run
/// invoked from the workspace root with `--root sub-project` finds
/// `sub-project/relative/path.json` instead of `cwd/relative/path.json`.
pub fn resolve_relative_to_root(
    path: &std::path::Path,
    project_root: Option<&std::path::Path>,
) -> std::path::PathBuf {
    if fallow_types::path_util::is_absolute_path_any_platform(path) {
        return path.to_path_buf();
    }
    match project_root {
        Some(root) => root.join(path),
        None => path.to_path_buf(),
    }
}

/// If `path` is a directory, looks for `coverage-final.json` inside it.
/// Parses the Istanbul JSON format and pre-computes per-function statement
/// coverage percentages for efficient lookup during CRAP scoring.
///
/// When `coverage_root` is provided, file paths in the Istanbul data are rebased:
/// the `coverage_root` prefix is stripped and `project_root` is prepended, enabling
/// cross-environment matching (e.g., coverage from CI used on a local checkout).
///
/// `path` itself is resolved against `project_root` when relative, so callers
/// can pass `--coverage coverage/foo.json` from a parent directory and have it
/// land under the `--root` they configured.
///
/// `relocated` marks a map recorded against a different checkout of the
/// project; see [`IstanbulFileCoverage::lookup`].
#[cfg(test)]
fn load_istanbul_coverage(
    path: &std::path::Path,
    coverage_root: Option<&std::path::Path>,
    project_root: Option<&std::path::Path>,
    relocated: bool,
) -> Result<IstanbulCoverage, String> {
    load_istanbul_coverage_for_sources(path, coverage_root, project_root, None, relocated)
}

pub(super) fn load_istanbul_coverage_for_sources(
    path: &std::path::Path,
    coverage_root: Option<&std::path::Path>,
    project_root: Option<&std::path::Path>,
    discovered_sources: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    relocated: bool,
) -> Result<IstanbulCoverage, String> {
    super::validate_coverage_root_absolute(coverage_root)?;
    let resolved = resolve_relative_to_root(path, project_root);
    let file_path = if resolved.is_dir() {
        let candidate = resolved.join("coverage-final.json");
        if candidate.is_file() {
            candidate
        } else {
            return Err(format!(
                "no coverage-final.json found in {}",
                resolved.display()
            ));
        }
    } else {
        resolved
    };

    let json = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("failed to read coverage file {}: {e}", file_path.display()))?;

    let raw: std::collections::BTreeMap<String, oxc_coverage_instrument::FileCoverage> =
        oxc_coverage_instrument::parse_coverage_map(&json).map_err(|e| {
            format!(
                "failed to parse coverage data from {}: {e}",
                file_path.display()
            )
        })?;

    let mut files = rustc_hash::FxHashMap::default();
    for file_cov in raw.values() {
        // A producer may record project-relative keys. They are relative to
        // the project, not to wherever the process happens to run, and
        // resolving them against the current directory fails the whole map at
        // once with every unit silently falling back to its estimate.
        let raw_path = resolve_relative_to_root(std::path::Path::new(&file_cov.path), project_root);
        let file_path = if let (Some(cov_root), Some(proj_root)) = (coverage_root, project_root) {
            rebase_coverage_path(raw_path, cov_root, proj_root)
        } else {
            raw_path
        };
        let canonical = dunce::canonicalize(&file_path).unwrap_or(file_path);
        let source = read_discovered_source(&canonical, discovered_sources, relocated);
        let source_index = source
            .as_deref()
            .map(|source| IstanbulSourceIndex::new(source, &canonical));

        let mut functions = Vec::with_capacity(file_cov.fn_map.len());
        for (fn_id, fn_entry) in &file_cov.fn_map {
            let coverage_pct = compute_function_statement_coverage(file_cov, fn_id, fn_entry);
            if let Some(function) =
                istanbul_function_coverage(fn_entry, coverage_pct, source_index.as_ref())
            {
                functions.push(function);
            }
        }

        files.insert(canonical, IstanbulFileCoverage::new(functions, relocated));
    }

    Ok(IstanbulCoverage { files })
}

#[expect(
    clippy::filetype_is_file,
    reason = "coverage provenance must admit regular files and reject every special file type"
)]
fn read_discovered_source(
    path: &std::path::Path,
    discovered_sources: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    relocated: bool,
) -> Option<String> {
    if relocated || !discovered_sources.is_some_and(|sources| sources.contains(path)) {
        return None;
    }
    if std::fs::symlink_metadata(path).ok()?.file_type().is_file() {
        std::fs::read_to_string(path).ok()
    } else {
        None
    }
}

/// Rebase one Istanbul file path from `coverage_root` onto `project_root`.
///
/// When the recorded path does not start with `coverage_root` verbatim, retry
/// with its canonicalized form: coverage generated inside a symlinked
/// directory (macOS `/var` vs `/private/var`) records the symlinked spelling
/// while the rebase prefix is typically canonical. Paths under neither
/// spelling are kept as-is, matching the previous behavior.
fn rebase_coverage_path(
    raw_path: std::path::PathBuf,
    coverage_root: &std::path::Path,
    project_root: &std::path::Path,
) -> std::path::PathBuf {
    if let Ok(rel) = raw_path.strip_prefix(coverage_root) {
        return project_root.join(rel);
    }
    if let Ok(canonical) = dunce::canonicalize(&raw_path)
        && let Ok(rel) = canonical.strip_prefix(coverage_root)
    {
        return project_root.join(rel);
    }
    raw_path
}

fn istanbul_function_coverage(
    fn_entry: &oxc_coverage_instrument::FnEntry,
    coverage_pct: f64,
    source_index: Option<&IstanbulSourceIndex<'_>>,
) -> Option<IstanbulFunctionCoverage> {
    let body_span = IstanbulSpan::from_entry(fn_entry, source_index);
    let header_span = IstanbulSpan::header_from_entry(fn_entry, source_index);
    let decl_start = normalized_istanbul_position(
        fn_entry.decl.start.line,
        fn_entry.decl.start.column,
        source_index,
    )?;
    let effective_position = normalized_istanbul_position(
        effective_istanbul_fn_line(fn_entry),
        fn_entry.decl.start.column,
        source_index,
    );
    let candidates = [
        effective_position.map(|position| IstanbulAlias {
            position,
            primary: true,
        }),
        Some(IstanbulAlias {
            position: decl_start,
            primary: true,
        }),
        body_span.map(|span| IstanbulAlias {
            position: span.start,
            primary: false,
        }),
        named_function_syntax_alias(fn_entry, source_index),
    ];
    let mut aliases: Vec<IstanbulAlias> = Vec::with_capacity(candidates.len());
    for candidate in candidates.into_iter().flatten() {
        if !aliases
            .iter()
            .any(|alias| alias.position == candidate.position)
        {
            aliases.push(candidate);
        }
    }

    Some(IstanbulFunctionCoverage {
        name: fn_entry.name.clone(),
        coverage_pct,
        aliases,
        decl_start,
        header_holds_other_fn: false,
        header_span,
        body_span,
    })
}

/// Source index used to reconcile Istanbul's UTF-16 columns with Fallow's
/// UTF-8 byte columns and recover exact named-function syntax starts.
struct IstanbulSourceIndex<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
    non_ascii_lines: rustc_hash::FxHashMap<usize, Utf16LineIndex>,
    named_function_starts: rustc_hash::FxHashMap<usize, usize>,
}

struct Utf16LineIndex {
    utf16_len: u32,
    byte_len: usize,
    checkpoints: Vec<Utf16Checkpoint>,
}

struct Utf16Checkpoint {
    utf16_start: u32,
    utf16_end: u32,
    byte_end: usize,
}

impl<'a> IstanbulSourceIndex<'a> {
    fn new(source: &'a str, path: &std::path::Path) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            source
                .bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        let non_ascii_lines = utf16_line_indexes(source, &line_starts);

        let named_function_starts = named_function_starts_from_clean_parse(source, path);

        Self {
            source,
            line_starts,
            non_ascii_lines,
            named_function_starts,
        }
    }

    fn byte_position(&self, line: u32, utf16_column: u32) -> Option<IstanbulPosition> {
        let line_index = usize::try_from(line.checked_sub(1)?).ok()?;
        let line_start = *self.line_starts.get(line_index)?;
        let line_end = self
            .line_starts
            .get(line_index + 1)
            .copied()
            .map_or(self.source.len(), |next_start| next_start - 1);
        let line_source = self.source.get(line_start..line_end)?;
        let byte_column = if let Some(index) = self.non_ascii_lines.get(&line_index) {
            index.byte_column(utf16_column)?
        } else {
            let byte_column = usize::try_from(utf16_column).ok()?;
            (byte_column <= line_source.len()).then_some(byte_column)?
        };
        Some(IstanbulPosition::new(
            line,
            u32::try_from(byte_column).ok()?,
        ))
    }

    fn absolute_offset(&self, line: u32, utf16_column: u32) -> Option<usize> {
        let position = self.byte_position(line, utf16_column)?;
        let line_index = usize::try_from(position.line.checked_sub(1)?).ok()?;
        self.line_starts
            .get(line_index)?
            .checked_add(position.col as usize)
    }

    fn position_at_offset(&self, offset: usize) -> Option<IstanbulPosition> {
        if offset > self.source.len() {
            return None;
        }
        let line_index = self.line_starts.partition_point(|start| *start <= offset) - 1;
        Some(IstanbulPosition::new(
            u32::try_from(line_index + 1).ok()?,
            u32::try_from(offset.checked_sub(self.line_starts[line_index])?).ok()?,
        ))
    }

    fn named_function_start(
        &self,
        fn_entry: &oxc_coverage_instrument::FnEntry,
    ) -> Option<IstanbulPosition> {
        let declaration_offset =
            self.absolute_offset(fn_entry.decl.start.line, fn_entry.decl.start.column)?;
        let syntax_offset = *self.named_function_starts.get(&declaration_offset)?;
        self.position_at_offset(syntax_offset)
    }
}

impl Utf16LineIndex {
    fn byte_column(&self, utf16_column: u32) -> Option<usize> {
        if utf16_column > self.utf16_len {
            return None;
        }
        let completed = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.utf16_end <= utf16_column);
        if let Some(next) = self.checkpoints.get(completed)
            && utf16_column > next.utf16_start
        {
            return None;
        }
        let (utf16_base, byte_base) = completed
            .checked_sub(1)
            .and_then(|index| self.checkpoints.get(index))
            .map_or((0, 0), |checkpoint| {
                (checkpoint.utf16_end, checkpoint.byte_end)
            });
        let ascii_width = usize::try_from(utf16_column.checked_sub(utf16_base)?).ok()?;
        let byte_column = byte_base.checked_add(ascii_width)?;
        (byte_column <= self.byte_len).then_some(byte_column)
    }
}

fn utf16_line_indexes(
    source: &str,
    line_starts: &[usize],
) -> rustc_hash::FxHashMap<usize, Utf16LineIndex> {
    let mut indexes = rustc_hash::FxHashMap::default();
    for (line_index, &line_start) in line_starts.iter().enumerate() {
        let line_end = line_starts
            .get(line_index + 1)
            .copied()
            .map_or(source.len(), |next_start| next_start - 1);
        let Some(line) = source.get(line_start..line_end) else {
            continue;
        };
        if line.is_ascii() {
            continue;
        }
        let mut utf16_column = 0_u32;
        let mut checkpoints = Vec::new();
        for (byte_column, character) in line.char_indices() {
            let utf16_width = character.len_utf16() as u32;
            if !character.is_ascii() {
                checkpoints.push(Utf16Checkpoint {
                    utf16_start: utf16_column,
                    utf16_end: utf16_column.saturating_add(utf16_width),
                    byte_end: byte_column.saturating_add(character.len_utf8()),
                });
            }
            utf16_column = utf16_column.saturating_add(utf16_width);
        }
        indexes.insert(
            line_index,
            Utf16LineIndex {
                utf16_len: utf16_column,
                byte_len: line.len(),
                checkpoints,
            },
        );
    }
    indexes
}

fn named_function_starts_from_clean_parse(
    source: &str,
    path: &std::path::Path,
) -> rustc_hash::FxHashMap<usize, usize> {
    let source_type = match path.extension().and_then(|extension| extension.to_str()) {
        Some("gts") => oxc_span::SourceType::ts(),
        Some("gjs") => oxc_span::SourceType::mjs(),
        _ => oxc_span::SourceType::from_path(path).unwrap_or_default(),
    };
    if let Some(starts) = collect_named_function_starts(source, source_type) {
        return starts;
    }
    if source_type.is_jsx() {
        return rustc_hash::FxHashMap::default();
    }
    let jsx_source_type = if source_type.is_typescript() {
        oxc_span::SourceType::tsx()
    } else {
        oxc_span::SourceType::jsx()
    };
    collect_named_function_starts(source, jsx_source_type).unwrap_or_default()
}

fn collect_named_function_starts(
    source: &str,
    source_type: oxc_span::SourceType,
) -> Option<rustc_hash::FxHashMap<usize, usize>> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return None;
    }
    let mut starts = rustc_hash::FxHashMap::default();
    let mut collector = NamedFunctionSyntaxCollector {
        starts: &mut starts,
    };
    oxc_ast_visit::Visit::visit_program(&mut collector, &parsed.program);
    Some(starts)
}

struct NamedFunctionSyntaxCollector<'a> {
    starts: &'a mut rustc_hash::FxHashMap<usize, usize>,
}

impl<'ast> oxc_ast_visit::Visit<'ast> for NamedFunctionSyntaxCollector<'_> {
    fn visit_function(
        &mut self,
        function: &oxc_ast::ast::Function<'ast>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        if let Some(identifier) = &function.id
            && let (Ok(identifier_start), Ok(syntax_start)) = (
                usize::try_from(identifier.span.start),
                usize::try_from(function.span.start),
            )
        {
            self.starts.insert(identifier_start, syntax_start);
        }
        oxc_ast_visit::walk::walk_function(self, function, flags);
    }
}

fn normalized_istanbul_position(
    line: u32,
    column: u32,
    source_index: Option<&IstanbulSourceIndex<'_>>,
) -> Option<IstanbulPosition> {
    match source_index {
        Some(index) => index.byte_position(line, column),
        None => Some(IstanbulPosition::new(line, column)),
    }
}

/// Exact syntax-backed alias for a named function's source start.
///
/// Istanbul declares a named function at its identifier, while Fallow records
/// the containing `function` or `async` keyword. Parser tokens distinguish
/// that real syntax across legal trivia from unrelated same-width text.
fn named_function_syntax_alias(
    fn_entry: &oxc_coverage_instrument::FnEntry,
    source_index: Option<&IstanbulSourceIndex<'_>>,
) -> Option<IstanbulAlias> {
    if is_anonymous_istanbul_name(&fn_entry.name) {
        return None;
    }
    Some(IstanbulAlias {
        position: source_index?.named_function_start(fn_entry)?,
        primary: true,
    })
}

fn effective_istanbul_fn_line(fn_entry: &oxc_coverage_instrument::FnEntry) -> u32 {
    if fn_entry.line > 0 {
        fn_entry.line
    } else {
        fn_entry.decl.start.line
    }
}

/// Compute statement-level coverage percentage for a single function.
///
/// Maps statements from `statementMap` to the function's body range (`loc`)
/// and computes the fraction with non-zero hit counts. When no statements
/// fall within the function body (e.g., one-liner arrow functions, getters),
/// falls back to the function hit count as a binary signal.
fn compute_function_statement_coverage(
    file_cov: &oxc_coverage_instrument::FileCoverage,
    fn_id: &str,
    fn_entry: &oxc_coverage_instrument::FnEntry,
) -> f64 {
    let fn_start_line = fn_entry.loc.start.line;
    let fn_start_col = fn_entry.loc.start.column;
    let fn_end_line = fn_entry.loc.end.line;
    let fn_end_col = fn_entry.loc.end.column;

    let mut total = 0u32;
    let mut covered = 0u32;

    for (stmt_id, stmt_loc) in &file_cov.statement_map {
        let after_start = stmt_loc.start.line > fn_start_line
            || (stmt_loc.start.line == fn_start_line && stmt_loc.start.column >= fn_start_col);
        let before_end = stmt_loc.end.line < fn_end_line
            || (stmt_loc.end.line == fn_end_line && stmt_loc.end.column <= fn_end_col);

        if after_start && before_end {
            total += 1;
            if file_cov.s.get(stmt_id).copied().unwrap_or(0) > 0 {
                covered += 1;
            }
        }
    }

    if total == 0 {
        let hit = file_cov.f.get(fn_id).copied().unwrap_or(0);
        if hit > 0 { 100.0 } else { 0.0 }
    } else {
        f64::from(covered) / f64::from(total) * 100.0
    }
}

/// Count unused VALUE exports per file path for O(1) lookup.
///
/// Type-only exports (interfaces, type aliases) are intentionally excluded ---
/// they are a different concern than unused functions/components.
fn count_unused_exports_by_path(
    unused_exports: &[crate::results::UnusedExportFinding],
) -> rustc_hash::FxHashMap<&std::path::Path, usize> {
    let mut map: rustc_hash::FxHashMap<&std::path::Path, usize> = rustc_hash::FxHashMap::default();
    for exp in unused_exports {
        *map.entry(exp.export.path.as_path()).or_default() += 1;
    }
    map
}

/// Compute the maintainability index for a single file.
///
/// Formula:
/// ```text
/// dampening = min(lines / 50, 1.0)
/// fan_out_penalty = min(ln(fan_out + 1) * 4, 15)
/// MI = 100 - (complexity_density * 30 * dampening) - (dead_code_ratio * 20) - fan_out_penalty
/// ```
///
/// The dampening factor prevents complexity density from dominating the score
/// on small files. A 5-line utility with CC=2 has density 0.40, but is trivially
/// readable; without dampening it scores worse than a 192-line function with CC=57
/// (density 0.30). Files under 50 lines get proportionally reduced density weight.
///
/// Fan-out uses logarithmic scaling capped at 15 points to reflect diminishing
/// marginal risk (the 30th import is less concerning than the 5th) and prevent
/// composition-root files from being unfairly penalized.
///
/// Clamped to \[0, 100\]. Higher is better.
fn compute_maintainability_index(
    complexity_density: f64,
    dead_code_ratio: f64,
    fan_out: usize,
    lines: u32,
) -> f64 {
    let dampening = (f64::from(lines) / fallow_output::MI_DENSITY_MIN_LINES).min(1.0);
    let fan_out_penalty = ((fan_out as f64).ln_1p() * 4.0).min(15.0);
    #[expect(
        clippy::suboptimal_flops,
        reason = "formula matches documented specification"
    )]
    let score = 100.0
        - (complexity_density * 30.0 * dampening)
        - (dead_code_ratio * 20.0)
        - fan_out_penalty;
    score.clamp(0.0, 100.0)
}

fn file_score_structural_concern(score: &FileHealthScore) -> f64 {
    (100.0 - score.maintainability_index).clamp(0.0, 100.0)
}

/// True when the file's CRAP signal is fully covered by configuration: nothing
/// meets its effective ceiling while something would have been flagged at the
/// canonical 30.0 baseline, or CRAP enforcement is disabled entirely
/// (`max_crap_threshold <= 0`). Mirrors the findings pipeline, which emits no
/// CRAP finding in exactly these states, so the row must not read `risk`.
#[must_use]
pub fn file_score_fully_crap_exempt(score: &FileHealthScore, max_crap_threshold: f64) -> bool {
    max_crap_threshold <= 0.0 || (score.crap_above_threshold == 0 && score.crap_exempted > 0)
}

/// CRAP concern bands generalized over the row's effective ceiling `t`
/// (`crap_effective_threshold`, falling back to the run global). At `t = 30`
/// the breakpoints are the historical (15, 30, 100). A fully exempt file
/// scores `0.0`: band generalization alone cannot drop an exempted file below
/// its structural concern, so the tag would keep reading `risk` against a run
/// with zero findings.
fn file_score_crap_concern(score: &FileHealthScore, max_crap_threshold: f64) -> f64 {
    if file_score_fully_crap_exempt(score, max_crap_threshold) {
        return 0.0;
    }
    let crap_max = score.crap_max;
    let t = score.crap_effective_threshold.unwrap_or(max_crap_threshold);
    let half = t / 2.0;
    let saturation = t * 10.0 / 3.0;
    if crap_max <= 0.0 {
        0.0
    } else if crap_max < half {
        (crap_max / half) * 45.0
    } else if crap_max < t {
        ((crap_max - half) / half).mul_add(30.0, 45.0)
    } else if crap_max < saturation {
        ((crap_max - t) / (saturation - t)).mul_add(25.0, 75.0)
    } else {
        100.0
    }
}

fn file_score_triage_concern(score: &FileHealthScore, max_crap_threshold: f64) -> f64 {
    file_score_structural_concern(score).max(file_score_crap_concern(score, max_crap_threshold))
}

/// Which signal places a file at its triage rank: its structural quality (low
/// maintainability index) or its untested complexity (CRAP risk). Surfaced per
/// row so the human file-scores table can label why a file sits where it does
/// when the two axes disagree (e.g. a low-CRAP file outranking a higher-CRAP
/// one because its MI is the worse signal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileScoreConcern {
    /// Ranked by structural quality: a low maintainability index.
    Structural,
    /// Ranked by untested complexity: a high CRAP score.
    Risk,
}

impl FileScoreConcern {
    /// Short lowercase label for the human file-scores table.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Structural => "structure",
            Self::Risk => "risk",
        }
    }
}

/// Classify which concern drove `score` to its rank. A file with no CRAP
/// concern (no CRAP risk at all, or every breaching function exempted by its
/// effective ceiling) is always `Structural`; otherwise the larger concern
/// wins, with ties (and the boundary where the two are equal) resolving to
/// `Risk` because untested complexity is the more urgent signal to act on.
///
/// `max_crap_threshold` is the run global (`summary.max_crap_threshold`), the
/// fallback ceiling for rows without a `crap_effective_threshold` of their own.
pub fn file_score_concern_axis(
    score: &FileHealthScore,
    max_crap_threshold: f64,
) -> FileScoreConcern {
    let crap_concern = file_score_crap_concern(score, max_crap_threshold);
    if crap_concern <= 0.0 {
        FileScoreConcern::Structural
    } else if crap_concern >= file_score_structural_concern(score) {
        FileScoreConcern::Risk
    } else {
        FileScoreConcern::Structural
    }
}

fn compare_file_score_triage(
    a: &FileHealthScore,
    b: &FileHealthScore,
    max_crap_threshold: f64,
) -> std::cmp::Ordering {
    file_score_triage_concern(b, max_crap_threshold)
        .total_cmp(&file_score_triage_concern(a, max_crap_threshold))
        .then_with(|| b.crap_max.total_cmp(&a.crap_max))
        .then_with(|| a.maintainability_index.total_cmp(&b.maintainability_index))
        .then_with(|| a.path.cmp(&b.path))
}

/// Inputs for [`compute_file_scores`], bundled so the analysis artifacts stay
/// a separate owned argument.
#[derive(Clone, Copy)]
pub(super) struct FileScoreComputeInput<'a> {
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) istanbul_coverage: Option<&'a IstanbulCoverage>,
    pub(super) root: &'a std::path::Path,
    pub(super) crap_thresholds: CrapScoreThresholds<'a>,
}

/// Compute per-file health scores using a pre-computed analysis output.
///
/// The caller provides an `AnalysisOutput` (with graph and dead code results)
/// so this function does not need to re-run the analysis pipeline. Complexity
/// density is derived from the already-parsed modules.
pub(super) fn compute_file_scores(
    input: FileScoreComputeInput<'_>,
    analysis_output: crate::results::DeadCodeAnalysisArtifacts,
) -> Result<FileScoreOutput, String> {
    let FileScoreComputeInput {
        modules,
        file_paths,
        changed_files,
        istanbul_coverage,
        root,
        crap_thresholds,
    } = input;
    let retained_graph = analysis_output.graph.ok_or("graph not available")?;
    let test_coverage = retained_graph.static_test_coverage();
    let graph = retained_graph.as_graph();
    let results = &analysis_output.results;

    let circular_files = collect_circular_files(results);
    let top_complex_fns = collect_top_complex_fns(modules, file_paths);
    let cycle_members = collect_cycle_members(results);
    let direct_callers = collect_direct_callers(graph, file_paths);
    let unused_export_names = collect_unused_export_names(results);

    let unused_files: rustc_hash::FxHashSet<&std::path::Path> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.as_path())
        .collect();

    let unused_exports_by_path = count_unused_exports_by_path(&results.unused_exports);

    let FileScoreCoverageSetup {
        module_by_id,
        coverage,
    } = prepare_file_score_coverage_setup(modules, file_paths, results, graph, test_coverage, root);

    let template_inherit =
        build_template_inherit_contexts(graph, test_coverage, &module_by_id, file_paths);

    let mut acc = accumulate_file_scores(
        unused_export_names,
        &FileScoreLoopCtx {
            graph,
            test_coverage,
            file_paths,
            module_by_id: &module_by_id,
            unused_files: &unused_files,
            unused_exports_by_path: &unused_exports_by_path,
            template_inherit: &template_inherit,
            istanbul_coverage,
            root,
            crap_thresholds,
        },
    );
    acc.scores = finalize_file_score_list(
        acc.scores,
        changed_files,
        crap_thresholds.resolver.global.crap,
    );

    Ok(build_file_score_output(FileScoreOutputParts {
        graph,
        file_paths,
        results,
        scores: acc.scores,
        coverage,
        circular_files,
        top_complex_fns,
        entry_points: acc.entry_points,
        value_export_counts: acc.value_export_counts,
        unused_export_names: acc.unused_export_names,
        cycle_members,
        direct_callers,
        istanbul_matched: acc.istanbul_matched,
        istanbul_total: acc.istanbul_total,
        istanbul_files_joined: acc.istanbul_files_joined,
        istanbul_files_total: acc.istanbul_files_total,
        per_function_crap: acc.per_function_crap,
        template_inherit,
    }))
}

/// Read-only inputs threaded into the per-node file-score loop.
struct FileScoreLoopCtx<'a> {
    graph: &'a fallow_graph::graph::ModuleGraph,
    test_coverage: StaticTestCoverage<'a>,
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    module_by_id: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a crate::source::ModuleInfo>,
    unused_files: &'a rustc_hash::FxHashSet<&'a std::path::Path>,
    unused_exports_by_path: &'a rustc_hash::FxHashMap<&'a std::path::Path, usize>,
    template_inherit: &'a rustc_hash::FxHashMap<crate::discover::FileId, TemplateInheritContext>,
    istanbul_coverage: Option<&'a IstanbulCoverage>,
    /// Project root used to relativize paths into the override resolver's glob
    /// space, matching the findings pipeline's `strip_prefix` on the same root.
    root: &'a std::path::Path,
    crap_thresholds: CrapScoreThresholds<'a>,
}

/// Mutable accumulators populated by the per-node file-score loop.
struct FileScoreAccumulator {
    scores: Vec<FileHealthScore>,
    entry_points: rustc_hash::FxHashSet<std::path::PathBuf>,
    value_export_counts: rustc_hash::FxHashMap<std::path::PathBuf, usize>,
    unused_export_names: rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
    per_function_crap: rustc_hash::FxHashMap<std::path::PathBuf, Vec<PerFunctionCrap>>,
    istanbul_matched: usize,
    istanbul_files_joined: usize,
    istanbul_files_total: usize,
    istanbul_total: usize,
}

impl FileScoreAccumulator {
    /// Empty accumulator with the score vector pre-sized to the module count.
    fn with_capacity(modules: usize) -> Self {
        FileScoreAccumulator {
            scores: Vec::with_capacity(modules),
            entry_points: rustc_hash::FxHashSet::default(),
            value_export_counts: rustc_hash::FxHashMap::default(),
            unused_export_names: rustc_hash::FxHashMap::default(),
            per_function_crap: rustc_hash::FxHashMap::default(),
            istanbul_matched: 0,
            istanbul_total: 0,
            istanbul_files_joined: 0,
            istanbul_files_total: 0,
        }
    }
}

/// Drive the per-node loop, returning an accumulator with one score per
/// analyzable file. `unused_export_names` seeds the accumulator's same field.
fn accumulate_file_scores(
    unused_export_names: rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
    ctx: &FileScoreLoopCtx<'_>,
) -> FileScoreAccumulator {
    let mut acc = FileScoreAccumulator {
        unused_export_names,
        istanbul_files_total: ctx
            .istanbul_coverage
            .map_or(0, IstanbulCoverage::file_count),
        ..FileScoreAccumulator::with_capacity(ctx.graph.modules.len())
    };
    for node in &ctx.graph.modules {
        let Some(path) = ctx.file_paths.get(&node.file_id) else {
            continue;
        };
        record_entry_point(&mut acc.entry_points, node, path);
        let score = compute_one_file_score(&mut acc, ctx, node, path);
        acc.scores.push(score);
    }
    acc
}

/// Apply the changed-file scope filter, drop zero-function barrels, and sort by
/// risk-aware triage concern.
fn finalize_file_score_list(
    mut scores: Vec<FileHealthScore>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    max_crap_threshold: f64,
) -> Vec<FileHealthScore> {
    if let Some(changed) = changed_files {
        scores.retain(|s| changed.contains(&s.path));
    }
    scores.retain(|s| s.function_count > 0);
    scores.sort_by(|a, b| compare_file_score_triage(a, b, max_crap_threshold));
    scores
}

/// Compute the `FileHealthScore` for one node and fold its side data into `acc`.
fn compute_one_file_score(
    acc: &mut FileScoreAccumulator,
    ctx: &FileScoreLoopCtx<'_>,
    node: &fallow_graph::graph::ModuleNode,
    path: &std::path::Path,
) -> FileHealthScore {
    let fan_in = ctx
        .graph
        .reverse_deps
        .get(node.file_id.0 as usize)
        .map_or(0, Vec::len);
    let fan_out = node.edge_range.len();

    let (total_cyclomatic, total_cognitive, function_count, lines) = ctx
        .module_by_id
        .get(&node.file_id)
        .map_or((0, 0, 0, 0), |module| aggregate_complexity(module));

    let value_exports = node.exports.iter().filter(|e| !e.is_type_only).count();
    let path_owned = path.to_path_buf();
    acc.value_export_counts
        .insert(path_owned.clone(), value_exports);
    record_unused_file_export_names(
        path_owned.as_path(),
        &node.exports,
        ctx.unused_files,
        &mut acc.unused_export_names,
    );

    let (dead_code_ratio_rounded, complexity_density_rounded, maintainability_index_rounded) =
        compute_file_score_metrics(node, &path_owned, ctx, total_cyclomatic, lines, fan_out);

    let relative = path_owned.strip_prefix(ctx.root).unwrap_or(&path_owned);
    let ceilings = CrapCeilingLookup::new(ctx.crap_thresholds, relative);
    let crap = compute_file_score_crap(node, ctx, &path_owned, &ceilings);
    acc.istanbul_matched += crap.istanbul_matched;
    acc.istanbul_total += crap.istanbul_total;
    acc.istanbul_files_joined += usize::from(crap.coverage_file_joined);
    record_per_function_crap(&mut acc.per_function_crap, &path_owned, crap.per_function);

    // `crap_effective_threshold` is the file's lowest effective ceiling, on the
    // wire only when it differs from the run global. Both sides come from the
    // same resolved configuration values, so any difference beyond epsilon is a
    // real override, never rounding noise.
    let global_crap = ctx.crap_thresholds.resolver.global.crap;
    let crap_effective_threshold = crap
        .signals
        .min_ceiling
        .filter(|ceiling| (*ceiling - global_crap).abs() > f64::EPSILON);

    FileHealthScore {
        path: path_owned,
        fan_in,
        fan_out,
        dead_code_ratio: dead_code_ratio_rounded,
        complexity_density: complexity_density_rounded,
        maintainability_index: maintainability_index_rounded,
        total_cyclomatic,
        total_cognitive,
        function_count,
        lines,
        crap_max: crap.max,
        crap_above_threshold: crap.signals.above,
        crap_exempted: crap.signals.exempted,
        crap_effective_threshold,
    }
}

/// Compute the rounded dead-code-ratio, complexity-density, and
/// maintainability-index metrics for one file.
fn compute_file_score_metrics(
    node: &fallow_graph::graph::ModuleNode,
    path: &std::path::Path,
    ctx: &FileScoreLoopCtx<'_>,
    total_cyclomatic: u32,
    lines: u32,
    fan_out: usize,
) -> (f64, f64, f64) {
    let dead_code_ratio = compute_dead_code_ratio(
        path,
        &node.exports,
        ctx.unused_files,
        ctx.unused_exports_by_path,
    );
    let complexity_density = compute_complexity_density(total_cyclomatic, lines);

    let dead_code_ratio_rounded = (dead_code_ratio * 100.0).round() / 100.0;
    let complexity_density_rounded = (complexity_density * 100.0).round() / 100.0;

    let maintainability_index = compute_maintainability_index(
        complexity_density_rounded,
        dead_code_ratio_rounded,
        fan_out,
        lines,
    );
    (
        dead_code_ratio_rounded,
        complexity_density_rounded,
        (maintainability_index * 10.0).round() / 10.0,
    )
}

fn build_file_score_output(parts: FileScoreOutputParts<'_>) -> FileScoreOutput {
    let total_exports: usize = parts.graph.modules.iter().map(|m| m.exports.len()).sum();
    let unused_deps = parts.results.unused_dependencies.len()
        + parts.results.unused_dev_dependencies.len()
        + parts.results.unused_optional_dependencies.len();
    let analysis_snapshot =
        build_analysis_counts_snapshot(parts.graph, parts.file_paths, parts.results, unused_deps);
    let analysis_counts =
        build_file_score_analysis_counts(parts.results, total_exports, unused_deps);
    let template_inherit_provenance =
        build_template_inherit_provenance(parts.template_inherit, parts.file_paths);

    FileScoreOutput {
        scores: parts.scores,
        coverage: parts.coverage,
        circular_files: parts.circular_files,
        top_complex_fns: parts.top_complex_fns,
        entry_points: parts.entry_points,
        value_export_counts: parts.value_export_counts,
        unused_export_names: parts.unused_export_names,
        cycle_members: parts.cycle_members,
        direct_callers: parts.direct_callers,
        analysis_counts,
        prop_drilling_chains: parts.results.prop_drilling_chains.clone(),
        render_fan_in: parts.results.render_fan_in.clone(),
        analysis_snapshot,
        istanbul_matched: parts.istanbul_matched,
        istanbul_total: parts.istanbul_total,
        istanbul_files_joined: parts.istanbul_files_joined,
        istanbul_files_total: parts.istanbul_files_total,
        per_function_crap: parts.per_function_crap,
        template_inherit_provenance,
    }
}

fn build_file_score_analysis_counts(
    results: &crate::results::AnalysisResults,
    total_exports: usize,
    unused_deps: usize,
) -> crate::vital_signs::AnalysisCounts {
    crate::vital_signs::AnalysisCounts {
        total_exports,
        dead_files: results.unused_files.len(),
        dead_exports: results.unused_exports.len() + results.unused_types.len(),
        unused_deps,
        circular_deps: results.circular_dependencies.len(),
        total_deps: 0usize,
    }
}

fn build_template_inherit_provenance(
    template_inherit: rustc_hash::FxHashMap<crate::discover::FileId, TemplateInheritContext>,
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
) -> rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf> {
    template_inherit
        .into_iter()
        .filter_map(|(file_id, ctx)| {
            file_paths
                .get(&file_id)
                .map(|path| ((**path).clone(), ctx.provenance_owner))
        })
        .collect()
}

fn record_entry_point(
    entry_points: &mut rustc_hash::FxHashSet<std::path::PathBuf>,
    node: &fallow_graph::graph::ModuleNode,
    path: &std::path::Path,
) {
    if node.is_entry_point() {
        entry_points.insert(path.to_path_buf());
    }
}

fn record_unused_file_export_names(
    path: &std::path::Path,
    exports: &[fallow_graph::graph::ExportSymbol],
    unused_files: &rustc_hash::FxHashSet<&std::path::Path>,
    unused_export_names: &mut rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
) {
    if !unused_files.contains(path) || unused_export_names.contains_key(path) {
        return;
    }

    let names: Vec<String> = exports
        .iter()
        .filter(|export| !export.is_type_only)
        .map(|export| export.name.to_string())
        .collect();
    if !names.is_empty() {
        unused_export_names.insert(path.to_path_buf(), names);
    }
}

struct FileScoreCrap {
    max: f64,
    signals: CrapThresholdSignals,
    per_function: Vec<PerFunctionCrap>,
    istanbul_matched: usize,
    istanbul_total: usize,
    /// The coverage map carried an entry for this file. Distinguishes a map
    /// that did not join from code the map genuinely says nothing ran in.
    coverage_file_joined: bool,
}

impl FileScoreCrap {
    fn empty() -> Self {
        Self {
            max: 0.0,
            signals: CrapThresholdSignals::default(),
            per_function: Vec::new(),
            istanbul_matched: 0,
            istanbul_total: 0,
            coverage_file_joined: false,
        }
    }

    fn estimated(result: EstimatedCrapResult) -> Self {
        Self {
            max: result.max_crap,
            signals: result.signals,
            per_function: result.per_function,
            istanbul_matched: 0,
            istanbul_total: 0,
            coverage_file_joined: false,
        }
    }

    fn istanbul(result: IstanbulCrapResult, coverage_file_joined: bool) -> Self {
        Self {
            max: result.max_crap,
            signals: result.signals,
            per_function: result.per_function,
            istanbul_matched: result.matched,
            istanbul_total: result.total,
            coverage_file_joined,
        }
    }
}

fn compute_file_score_crap(
    node: &fallow_graph::graph::ModuleNode,
    ctx: &FileScoreLoopCtx<'_>,
    path: &std::path::Path,
    ceilings: &CrapCeilingLookup<'_>,
) -> FileScoreCrap {
    let Some(module) = ctx.module_by_id.get(&node.file_id).copied() else {
        return FileScoreCrap::empty();
    };

    let is_coverage_suppressed = crate::suppress::is_file_suppressed(
        &module.suppressions,
        fallow_types::suppress::IssueKind::CoverageGaps,
    );
    let is_test_reachable = ctx.test_coverage.covers_file(node.file_id) || is_coverage_suppressed;
    let resolution = resolve_crap_coverage(
        ctx.template_inherit.get(&node.file_id),
        ctx.istanbul_coverage,
        path,
    );
    match resolution {
        CrapCoverageResolution::TemplateInherited(inherit_ctx) => {
            compute_template_inherited_crap(module, inherit_ctx, ceilings)
        }
        CrapCoverageResolution::Istanbul { file_coverage } => {
            compute_istanbul_file_crap(module, file_coverage, is_test_reachable, ceilings)
        }
        CrapCoverageResolution::StaticEstimated => compute_static_file_crap(
            module,
            &node.exports,
            ctx.test_coverage,
            is_test_reachable,
            ceilings,
        ),
    }
}

fn compute_template_inherited_crap(
    module: &crate::source::ModuleInfo,
    inherit_ctx: &TemplateInheritContext,
    ceilings: &CrapCeilingLookup<'_>,
) -> FileScoreCrap {
    FileScoreCrap::estimated(compute_crap_scores_estimated(
        &module.complexity,
        &inherit_ctx.test_referenced_exports,
        inherit_ctx.is_test_reachable,
        fallow_output::CoverageSource::EstimatedComponentInherited,
        ceilings,
    ))
}

fn compute_istanbul_file_crap(
    module: &crate::source::ModuleInfo,
    file_coverage: Option<&IstanbulFileCoverage>,
    is_test_reachable: bool,
    ceilings: &CrapCeilingLookup<'_>,
) -> FileScoreCrap {
    FileScoreCrap::istanbul(
        compute_crap_scores_istanbul(
            &module.complexity,
            file_coverage,
            is_test_reachable,
            ceilings,
        ),
        file_coverage.is_some(),
    )
}

fn compute_static_file_crap(
    module: &crate::source::ModuleInfo,
    exports: &[fallow_graph::graph::ExportSymbol],
    test_coverage: StaticTestCoverage<'_>,
    is_test_reachable: bool,
    ceilings: &CrapCeilingLookup<'_>,
) -> FileScoreCrap {
    let test_refs = build_test_referenced_exports(exports, test_coverage);
    FileScoreCrap::estimated(compute_crap_scores_estimated(
        &module.complexity,
        &test_refs,
        is_test_reachable,
        fallow_output::CoverageSource::Estimated,
        ceilings,
    ))
}

fn record_per_function_crap(
    per_function_crap: &mut rustc_hash::FxHashMap<std::path::PathBuf, Vec<PerFunctionCrap>>,
    path: &std::path::Path,
    per_function: Vec<PerFunctionCrap>,
) {
    if !per_function.is_empty() {
        per_function_crap.insert(path.to_path_buf(), per_function);
    }
}

struct FileScoreCoverageSetup<'a> {
    module_by_id: rustc_hash::FxHashMap<crate::discover::FileId, &'a crate::source::ModuleInfo>,
    coverage: CoverageGapData,
}

fn prepare_file_score_coverage_setup<'a>(
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
    results: &crate::results::AnalysisResults,
    graph: &fallow_graph::graph::ModuleGraph,
    test_coverage: StaticTestCoverage<'_>,
    root: &std::path::Path,
) -> FileScoreCoverageSetup<'a> {
    let module_by_id: rustc_hash::FxHashMap<_, _> =
        modules.iter().map(|m| (m.file_id, m)).collect();
    let unused_exports: rustc_hash::FxHashSet<(&std::path::Path, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export.export.path.as_path(),
                export.export.export_name.clone(),
            )
        })
        .collect();
    let coverage = compute_coverage_gaps(
        graph,
        test_coverage,
        file_paths,
        &module_by_id,
        &unused_exports,
        root,
    );
    FileScoreCoverageSetup {
        module_by_id,
        coverage,
    }
}

fn collect_circular_files(
    results: &crate::results::AnalysisResults,
) -> rustc_hash::FxHashSet<std::path::PathBuf> {
    results
        .circular_dependencies
        .iter()
        .flat_map(|c| c.cycle.files.iter().cloned())
        .collect()
}

fn collect_top_complex_fns(
    modules: &[crate::source::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
) -> rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>> {
    let mut top_complex_fns = rustc_hash::FxHashMap::default();
    for module in modules {
        if module.complexity.is_empty() {
            continue;
        }
        let Some(path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let mut funcs: Vec<(String, u32, u16)> = module
            .complexity
            .iter()
            .map(|f| (f.name.clone(), f.line, f.cognitive))
            .collect();
        funcs.sort_by_key(|f| std::cmp::Reverse(f.2));
        funcs.truncate(3);
        if funcs[0].2 > 0 {
            top_complex_fns.insert((*path).clone(), funcs);
        }
    }
    top_complex_fns
}

fn collect_cycle_members(
    results: &crate::results::AnalysisResults,
) -> rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>> {
    let mut cycle_members: rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>> =
        rustc_hash::FxHashMap::default();
    for cycle in &results.circular_dependencies {
        for file in &cycle.cycle.files {
            let others: Vec<std::path::PathBuf> = cycle
                .cycle
                .files
                .iter()
                .filter(|f| *f != file)
                .cloned()
                .collect();
            cycle_members
                .entry(file.clone())
                .or_default()
                .extend(others);
        }
    }
    for members in cycle_members.values_mut() {
        members.sort();
        members.dedup();
    }
    cycle_members
}

fn collect_unused_export_names(
    results: &crate::results::AnalysisResults,
) -> rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>> {
    let mut unused_export_names: rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>> =
        rustc_hash::FxHashMap::default();
    for exp in &results.unused_exports {
        unused_export_names
            .entry(exp.export.path.clone())
            .or_default()
            .push(exp.export.export_name.clone());
    }
    unused_export_names
}

fn build_analysis_counts_snapshot(
    graph: &fallow_graph::graph::ModuleGraph,
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
    results: &crate::results::AnalysisResults,
    unused_deps: usize,
) -> AnalysisCountsSnapshot {
    let mut module_export_counts = rustc_hash::FxHashMap::with_capacity_and_hasher(
        graph.modules.len(),
        rustc_hash::FxBuildHasher,
    );
    for module in &graph.modules {
        if let Some(path) = file_paths.get(&module.file_id) {
            module_export_counts.insert((*path).clone(), module.exports.len());
        }
    }

    let mut unused_export_paths =
        Vec::with_capacity(results.unused_exports.len() + results.unused_types.len());
    unused_export_paths.extend(results.unused_exports.iter().map(|e| e.export.path.clone()));
    unused_export_paths.extend(results.unused_types.iter().map(|e| e.export.path.clone()));

    let mut unused_dep_package_paths = Vec::with_capacity(unused_deps);
    unused_dep_package_paths.extend(
        results
            .unused_dependencies
            .iter()
            .map(|d| d.dep.path.clone()),
    );
    unused_dep_package_paths.extend(
        results
            .unused_dev_dependencies
            .iter()
            .map(|d| d.dep.path.clone()),
    );
    unused_dep_package_paths.extend(
        results
            .unused_optional_dependencies
            .iter()
            .map(|d| d.dep.path.clone()),
    );

    AnalysisCountsSnapshot {
        unused_file_paths: results
            .unused_files
            .iter()
            .map(|f| f.file.path.clone())
            .collect(),
        unused_export_paths,
        unused_dep_package_paths,
        circular_dep_groups: results
            .circular_dependencies
            .iter()
            .map(|c| c.cycle.files.clone())
            .collect(),
        module_export_counts,
    }
}

#[cfg(test)]
mod tests {
    use super::super::threshold_overrides::GlobalHealthThresholds;
    use super::*;

    /// Resolver with no override entries and the given global CRAP ceiling,
    /// the default-configuration shape for scoring tests.
    fn test_crap_resolver(crap: f64) -> ThresholdOverrideResolver {
        ThresholdOverrideResolver::new(
            &[],
            GlobalHealthThresholds {
                cyclomatic: 20,
                cognitive: 15,
                crap,
                unit_size: 120,
            },
        )
    }

    /// Resolver with the given override entries over the default global 30.0.
    fn test_override_resolver(
        overrides: &[fallow_config::HealthThresholdOverride],
    ) -> ThresholdOverrideResolver {
        ThresholdOverrideResolver::new(
            overrides,
            GlobalHealthThresholds {
                cyclomatic: 20,
                cognitive: 15,
                crap: CRAP_THRESHOLD,
                unit_size: 120,
            },
        )
    }

    /// `compute_crap_scores_istanbul` with default-configuration ceilings.
    fn istanbul_crap_default(
        complexity: &[fallow_types::extract::FunctionComplexity],
        file_coverage: Option<&IstanbulFileCoverage>,
        is_test_reachable: bool,
    ) -> IstanbulCrapResult {
        let resolver = test_crap_resolver(CRAP_THRESHOLD);
        let ceilings = CrapCeilingLookup::new(
            CrapScoreThresholds {
                resolver: &resolver,
                enforce_crap: true,
            },
            std::path::Path::new("src/test.ts"),
        );
        compute_crap_scores_istanbul(complexity, file_coverage, is_test_reachable, &ceilings)
    }

    /// A coverage map that says nothing about a function is not evidence that
    /// the function ran, so passing one must not score it lower than the run
    /// without a map would have. Both paths use the same static estimate for a
    /// function whose file tests reach.
    #[test]
    fn an_unmatched_function_scores_the_same_with_and_without_a_coverage_map() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/grade.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "// geometry fixture\n").unwrap();

        // A map that carries the file but records a function fallow never
        // extracted, which is what a stale map or an unresolved producer
        // anchor produces.
        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "unrelated",
                    "line": 40,
                    "decl": { "start": { "line": 40, "column": 0 }, "end": { "line": 40, "column": 9 } },
                    "loc": { "start": { "line": 40, "column": 20 }, "end": { "line": 44, "column": 1 } }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );
        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        let function = make_fn_complexity(10);
        let with_map =
            istanbul_crap_default(std::slice::from_ref(&function), Some(file_coverage), true);
        let estimated = compute_crap_scores_estimated(
            std::slice::from_ref(&function),
            &rustc_hash::FxHashSet::default(),
            true,
            fallow_output::CoverageSource::Estimated,
            &CrapCeilingLookup::new(
                CrapScoreThresholds {
                    resolver: &test_crap_resolver(CRAP_THRESHOLD),
                    enforce_crap: true,
                },
                std::path::Path::new("src/test.ts"),
            ),
        );

        assert_eq!(with_map.matched, 0);
        assert!(
            (with_map.per_function[0].crap - estimated.per_function[0].crap).abs() < f64::EPSILON,
            "a map that attributes nothing must not change the score"
        );
        assert_eq!(with_map.per_function[0].coverage_pct, None);
    }

    fn test_istanbul_file_coverage(
        functions: rustc_hash::FxHashMap<(String, u32, u32), f64>,
        relocated: bool,
    ) -> IstanbulFileCoverage {
        let functions = functions
            .into_iter()
            .map(
                |((name, line, col), coverage_pct)| IstanbulFunctionCoverage {
                    name,
                    coverage_pct,
                    aliases: vec![primary_alias(line, col)],
                    decl_start: IstanbulPosition::new(line, col),
                    header_holds_other_fn: false,
                    header_span: None,
                    body_span: None,
                },
            )
            .collect();
        IstanbulFileCoverage::new(functions, relocated)
    }

    fn primary_alias(line: u32, col: u32) -> IstanbulAlias {
        IstanbulAlias {
            position: IstanbulPosition::new(line, col),
            primary: true,
        }
    }

    fn secondary_alias(line: u32, col: u32) -> IstanbulAlias {
        IstanbulAlias {
            position: IstanbulPosition::new(line, col),
            primary: false,
        }
    }

    fn body_span(start: (u32, u32), end: (u32, u32)) -> IstanbulSpan {
        IstanbulSpan {
            start: IstanbulPosition::new(start.0, start.1),
            end: IstanbulPosition::new(end.0, end.1),
        }
    }

    /// `compute_crap_scores_estimated` with default-configuration ceilings.
    fn estimated_crap_default(
        complexity: &[fallow_types::extract::FunctionComplexity],
        test_referenced_exports: &rustc_hash::FxHashSet<String>,
        is_test_reachable: bool,
        coverage_source: fallow_output::CoverageSource,
    ) -> EstimatedCrapResult {
        let resolver = test_crap_resolver(CRAP_THRESHOLD);
        let ceilings = CrapCeilingLookup::new(
            CrapScoreThresholds {
                resolver: &resolver,
                enforce_crap: true,
            },
            std::path::Path::new("src/test.ts"),
        );
        compute_crap_scores_estimated(
            complexity,
            test_referenced_exports,
            is_test_reachable,
            coverage_source,
            &ceilings,
        )
    }

    /// `compute_file_scores` with default-configuration CRAP thresholds.
    fn compute_file_scores_default(
        modules: &[crate::source::ModuleInfo],
        file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
        changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
        analysis_output: crate::results::DeadCodeAnalysisArtifacts,
        istanbul_coverage: Option<&IstanbulCoverage>,
        root: &std::path::Path,
    ) -> Result<FileScoreOutput, String> {
        let resolver = test_crap_resolver(CRAP_THRESHOLD);
        compute_file_scores(
            FileScoreComputeInput {
                modules,
                file_paths,
                changed_files,
                istanbul_coverage,
                root,
                crap_thresholds: CrapScoreThresholds {
                    resolver: &resolver,
                    enforce_crap: true,
                },
            },
            analysis_output,
        )
    }

    #[test]
    fn maintainability_perfect_score() {
        assert!((compute_maintainability_index(0.0, 0.0, 0, 100) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn crap_resolution_prefers_template_inheritance_over_istanbul() {
        let inherit_ctx = TemplateInheritContext {
            is_test_reachable: true,
            test_referenced_exports: rustc_hash::FxHashSet::default(),
            provenance_owner: std::path::PathBuf::from("/project/src/app.component.ts"),
        };
        let istanbul = IstanbulCoverage {
            files: rustc_hash::FxHashMap::default(),
        };

        let resolution = resolve_crap_coverage(
            Some(&inherit_ctx),
            Some(&istanbul),
            std::path::Path::new("/project/src/app.component.html"),
        );

        assert!(matches!(
            resolution,
            CrapCoverageResolution::TemplateInherited(_)
        ));
    }

    #[test]
    fn crap_resolution_keeps_istanbul_when_file_is_missing() {
        let istanbul = IstanbulCoverage {
            files: rustc_hash::FxHashMap::default(),
        };

        let resolution = resolve_crap_coverage(
            None,
            Some(&istanbul),
            std::path::Path::new("/project/src/missing.ts"),
        );

        assert!(matches!(
            resolution,
            CrapCoverageResolution::Istanbul {
                file_coverage: None
            }
        ));
    }

    #[test]
    fn maintainability_clamped_at_zero() {
        assert!((compute_maintainability_index(10.0, 1.0, 100, 200) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn maintainability_formula_correct() {
        let result = compute_maintainability_index(0.5, 0.3, 10, 100);
        let expected = 11.0_f64.ln().mul_add(-4.0, 100.0 - 15.0 - 6.0);
        assert!((result - expected).abs() < 0.01);
    }

    #[test]
    fn maintainability_dead_file_penalty() {
        let result = compute_maintainability_index(0.0, 1.0, 0, 100);
        assert!((result - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn maintainability_fan_out_is_logarithmic() {
        let result_10 = compute_maintainability_index(0.0, 0.0, 10, 100);
        let result_100 = compute_maintainability_index(0.0, 0.0, 100, 100);
        let result_200 = compute_maintainability_index(0.0, 0.0, 200, 100);

        assert!(result_10 > 90.0); // ~90.4
        assert!(result_100 > 84.0); // 85.0 (capped)
        assert!((result_100 - result_200).abs() < f64::EPSILON);
    }

    #[test]
    fn maintainability_fan_out_capped_at_15() {
        let result = compute_maintainability_index(0.0, 1.0, 1000, 100);
        assert!((result - 65.0).abs() < f64::EPSILON);
    }

    #[test]
    fn maintainability_small_file_dampened() {
        let small = compute_maintainability_index(0.40, 0.0, 0, 5);
        assert!((small - 98.8).abs() < 0.01);
    }

    #[test]
    fn maintainability_large_file_undampened() {
        let large = compute_maintainability_index(0.30, 0.0, 0, 192);
        assert!((large - 91.0).abs() < 0.01);
    }

    #[test]
    fn maintainability_small_file_ranks_better_than_complex_large_file() {
        let trivial = compute_maintainability_index(0.40, 0.0, 0, 5);
        let nightmare = compute_maintainability_index(0.30, 0.0, 0, 192);
        assert!(
            trivial > nightmare,
            "trivial file ({trivial}) should rank better than nightmare ({nightmare})"
        );
    }

    #[test]
    fn maintainability_at_dampening_boundary() {
        let at_boundary = compute_maintainability_index(0.5, 0.0, 0, 50);
        let above_boundary = compute_maintainability_index(0.5, 0.0, 0, 51);
        assert!((at_boundary - above_boundary).abs() < 0.01);
    }

    #[test]
    fn maintainability_zero_lines_zero_density_penalty() {
        let result = compute_maintainability_index(5.0, 0.0, 0, 0);
        assert!((result - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn complexity_density_zero_lines() {
        assert!((compute_complexity_density(10, 0)).abs() < f64::EPSILON);
    }

    #[test]
    fn complexity_density_normal() {
        let result = compute_complexity_density(10, 100);
        assert!((result - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn complexity_density_high() {
        let result = compute_complexity_density(50, 10);
        assert!((result - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_ratio_no_exports() {
        let unused_files = rustc_hash::FxHashSet::default();
        let unused_map = rustc_hash::FxHashMap::default();
        let path = std::path::Path::new("/src/foo.ts");
        let exports: Vec<fallow_graph::graph::ExportSymbol> = vec![];

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
        assert!((ratio).abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_ratio_all_unused_file() {
        let mut unused_files: rustc_hash::FxHashSet<&std::path::Path> =
            rustc_hash::FxHashSet::default();
        let path = std::path::Path::new("/src/foo.ts");
        unused_files.insert(path);
        let unused_map = rustc_hash::FxHashMap::default();
        let exports: Vec<fallow_graph::graph::ExportSymbol> = vec![];

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_ratio_mix() {
        let unused_files = rustc_hash::FxHashSet::default();
        let path = std::path::Path::new("/src/foo.ts");

        let exports = vec![
            fallow_graph::graph::ExportSymbol {
                name: crate::source::ExportName::Named("a".into()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                references: vec![],
                reference_paths: Vec::new(),
                members: vec![],
            },
            fallow_graph::graph::ExportSymbol {
                name: crate::source::ExportName::Named("b".into()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                references: vec![],
                reference_paths: Vec::new(),
                members: vec![],
            },
            fallow_graph::graph::ExportSymbol {
                name: crate::source::ExportName::Named("c".into()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                references: vec![],
                reference_paths: Vec::new(),
                members: vec![],
            },
            fallow_graph::graph::ExportSymbol {
                name: crate::source::ExportName::Named("MyType".into()),
                is_type_only: true,
                is_side_effect_used: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                references: vec![],
                reference_paths: Vec::new(),
                members: vec![],
            },
        ];

        let mut unused_map: rustc_hash::FxHashMap<&std::path::Path, usize> =
            rustc_hash::FxHashMap::default();
        unused_map.insert(path, 2);

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
        assert!((ratio - 2.0 / 3.0).abs() < 1e-10);
    }

    #[test]
    fn dead_code_ratio_all_type_only_exports() {
        let unused_files = rustc_hash::FxHashSet::default();
        let path = std::path::Path::new("/src/types.ts");

        let exports = vec![fallow_graph::graph::ExportSymbol {
            name: crate::source::ExportName::Named("Foo".into()),
            is_type_only: true,
            is_side_effect_used: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
        }];
        let unused_map = rustc_hash::FxHashMap::default();

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
        assert!((ratio).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_complexity_empty_module() {
        let module = crate::source::ModuleInfo::empty(crate::discover::FileId(0));

        let (cyc, cog, funcs, lines) = aggregate_complexity(&module);
        assert_eq!(cyc, 0);
        assert_eq!(cog, 0);
        assert_eq!(funcs, 0);
        assert_eq!(lines, 0);
    }

    #[test]
    fn aggregate_complexity_single_function() {
        let module = crate::source::ModuleInfo {
            line_offsets: vec![0, 10, 20, 30, 40], // 5 lines
            complexity: vec![fallow_types::extract::FunctionComplexity {
                name: "doStuff".into(),
                is_private_member: false,
                line: 1,
                col: 0,
                cyclomatic: 7,
                cognitive: 4,
                line_count: 5,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
            ..crate::source::ModuleInfo::empty(crate::discover::FileId(0))
        };

        let (cyc, cog, funcs, lines) = aggregate_complexity(&module);
        assert_eq!(cyc, 7);
        assert_eq!(cog, 4);
        assert_eq!(funcs, 1);
        assert_eq!(lines, 5);
    }

    #[test]
    fn aggregate_complexity_multiple_functions() {
        let module = crate::source::ModuleInfo {
            line_offsets: vec![0, 10, 20], // 3 lines
            complexity: vec![
                fallow_types::extract::FunctionComplexity {
                    name: "a".into(),
                    is_private_member: false,
                    line: 1,
                    col: 0,
                    cyclomatic: 3,
                    cognitive: 2,
                    line_count: 1,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
                fallow_types::extract::FunctionComplexity {
                    name: "b".into(),
                    is_private_member: false,
                    line: 2,
                    col: 0,
                    cyclomatic: 5,
                    cognitive: 8,
                    line_count: 2,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
            ],
            ..crate::source::ModuleInfo::empty(crate::discover::FileId(0))
        };

        let (cyc, cog, funcs, lines) = aggregate_complexity(&module);
        assert_eq!(cyc, 8);
        assert_eq!(cog, 10);
        assert_eq!(funcs, 2);
        assert_eq!(lines, 3);
    }

    #[test]
    fn count_unused_exports_empty() {
        let exports: Vec<crate::results::UnusedExportFinding> = vec![];
        let map = count_unused_exports_by_path(&exports);
        assert!(map.is_empty());
    }

    #[test]
    fn count_unused_exports_groups_by_path() {
        let exports = vec![
            crate::results::UnusedExportFinding::with_actions(crate::results::UnusedExport {
                path: std::path::PathBuf::from("/src/a.ts"),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }),
            crate::results::UnusedExportFinding::with_actions(crate::results::UnusedExport {
                path: std::path::PathBuf::from("/src/a.ts"),
                export_name: "bar".into(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 40,
                is_re_export: false,
            }),
            crate::results::UnusedExportFinding::with_actions(crate::results::UnusedExport {
                path: std::path::PathBuf::from("/src/b.ts"),
                export_name: "baz".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }),
        ];
        let map = count_unused_exports_by_path(&exports);
        assert_eq!(map.get(std::path::Path::new("/src/a.ts")).copied(), Some(2));
        assert_eq!(map.get(std::path::Path::new("/src/b.ts")).copied(), Some(1));
    }

    #[test]
    fn dead_code_ratio_all_value_exports_unused() {
        let unused_files = rustc_hash::FxHashSet::default();
        let path = std::path::Path::new("/src/foo.ts");

        let exports = vec![
            fallow_graph::graph::ExportSymbol {
                name: crate::source::ExportName::Named("a".into()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                references: vec![],
                reference_paths: Vec::new(),
                members: vec![],
            },
            fallow_graph::graph::ExportSymbol {
                name: crate::source::ExportName::Named("b".into()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                references: vec![],
                reference_paths: Vec::new(),
                members: vec![],
            },
        ];

        let mut unused_map: rustc_hash::FxHashMap<&std::path::Path, usize> =
            rustc_hash::FxHashMap::default();
        unused_map.insert(path, 2);

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_ratio_clamped_when_unused_exceeds_value_exports() {
        let unused_files = rustc_hash::FxHashSet::default();
        let path = std::path::Path::new("/src/foo.ts");

        let exports = vec![fallow_graph::graph::ExportSymbol {
            name: crate::source::ExportName::Named("a".into()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
        }];

        let mut unused_map: rustc_hash::FxHashMap<&std::path::Path, usize> =
            rustc_hash::FxHashMap::default();
        unused_map.insert(path, 5);

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_ratio_no_unused_exports_for_path() {
        let unused_files = rustc_hash::FxHashSet::default();
        let path = std::path::Path::new("/src/clean.ts");

        let exports = vec![fallow_graph::graph::ExportSymbol {
            name: crate::source::ExportName::Named("used".into()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
        }];

        let unused_map = rustc_hash::FxHashMap::default();
        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
        assert!(ratio.abs() < f64::EPSILON);
    }

    #[test]
    fn complexity_density_zero_cyclomatic_with_lines() {
        let result = compute_complexity_density(0, 100);
        assert!(result.abs() < f64::EPSILON);
    }

    #[test]
    fn complexity_density_single_line() {
        let result = compute_complexity_density(1, 1);
        assert!((result - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn maintainability_only_complexity_penalty() {
        let result = compute_maintainability_index(3.0, 0.0, 0, 100);
        assert!((result - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn maintainability_only_dead_code_penalty() {
        let result = compute_maintainability_index(0.0, 0.5, 0, 100);
        assert!((result - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn maintainability_fan_out_one() {
        let result = compute_maintainability_index(0.0, 0.0, 1, 100);
        let expected = 2.0_f64.ln().mul_add(-4.0, 100.0);
        assert!((result - expected).abs() < 0.01);
    }

    #[test]
    fn maintainability_all_penalties_maxed() {
        let result = compute_maintainability_index(10.0, 1.0, 1000, 200);
        assert!(result.abs() < f64::EPSILON);
    }

    #[test]
    fn count_unused_exports_single_file_single_export() {
        let exports = vec![crate::results::UnusedExportFinding::with_actions(
            crate::results::UnusedExport {
                path: std::path::PathBuf::from("/src/only.ts"),
                export_name: "lonely".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            },
        )];
        let map = count_unused_exports_by_path(&exports);
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get(std::path::Path::new("/src/only.ts")).copied(),
            Some(1)
        );
    }

    /// Helper to build a minimal `ModuleGraph` from scratch.
    fn build_test_graph(
        files: &[crate::discover::DiscoveredFile],
        entry_point_paths: &[std::path::PathBuf],
        resolved_modules: &[fallow_graph::resolve::ResolvedModule],
    ) -> fallow_graph::graph::ModuleGraph {
        let entry_points: Vec<crate::discover::EntryPoint> = entry_point_paths
            .iter()
            .map(|p| crate::discover::EntryPoint {
                path: p.clone(),
                source: crate::discover::EntryPointSource::PackageJsonMain,
            })
            .collect();
        fallow_graph::graph::ModuleGraph::build(resolved_modules, &entry_points, files)
    }

    /// Helper to create a `ModuleInfo` with given complexity and line count.
    fn make_module_info(
        file_id: u32,
        line_count: usize,
        functions: Vec<fallow_types::extract::FunctionComplexity>,
    ) -> crate::source::ModuleInfo {
        crate::source::ModuleInfo {
            line_offsets: (0..line_count).map(|i| (i * 10) as u32).collect(),
            complexity: functions,
            ..crate::source::ModuleInfo::empty(crate::discover::FileId(file_id))
        }
    }

    fn make_file_score(path: &str, maintainability_index: f64, crap_max: f64) -> FileHealthScore {
        FileHealthScore {
            path: std::path::PathBuf::from(path),
            fan_in: 0,
            fan_out: 0,
            dead_code_ratio: 0.0,
            complexity_density: 0.0,
            maintainability_index,
            total_cyclomatic: 0,
            total_cognitive: 0,
            function_count: 1,
            lines: 1,
            crap_max,
            crap_above_threshold: usize::from(crap_max >= CRAP_THRESHOLD),
            crap_exempted: 0,
            crap_effective_threshold: None,
        }
    }

    fn crap_concern_at_default(crap_max: f64) -> f64 {
        file_score_crap_concern(
            &make_file_score("/src/concern.ts", 100.0, crap_max),
            CRAP_THRESHOLD,
        )
    }

    #[test]
    fn file_score_crap_concern_tracks_crap_risk_bands() {
        assert!((crap_concern_at_default(0.0) - 0.0).abs() < f64::EPSILON);
        assert!((crap_concern_at_default(15.0) - 45.0).abs() < f64::EPSILON);
        assert!((crap_concern_at_default(CRAP_THRESHOLD) - 75.0).abs() < f64::EPSILON);
        assert!((crap_concern_at_default(100.0) - 100.0).abs() < f64::EPSILON);
        assert!((crap_concern_at_default(552.0) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn file_score_crap_concern_generalizes_bands_over_effective_ceiling() {
        // At t = 500 the band edges scale to (250, 500, 1666.7): a breaching
        // 250 sits at the moderate/high edge and a breaching 500 at high.
        let mut at_edge = make_file_score("/src/edge.ts", 100.0, 250.0);
        at_edge.crap_above_threshold = 1;
        at_edge.crap_effective_threshold = Some(500.0);
        assert!((file_score_crap_concern(&at_edge, CRAP_THRESHOLD) - 45.0).abs() < f64::EPSILON);

        let mut at_ceiling = make_file_score("/src/ceiling.ts", 100.0, 500.0);
        at_ceiling.crap_above_threshold = 1;
        at_ceiling.crap_effective_threshold = Some(500.0);
        assert!((file_score_crap_concern(&at_ceiling, CRAP_THRESHOLD) - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn file_score_crap_concern_zeroes_fully_exempt_file() {
        // The issue's own numbers: crap_max 110 under ceiling 500 with both
        // breaches exempted. Band generalization alone would yield 19.8 and
        // keep the row above a structural concern of 12; the fully-exempt rule
        // must zero it (issue #2228).
        let mut exempt = make_file_score("/src/legacy.ts", 88.0, 110.0);
        exempt.crap_above_threshold = 0;
        exempt.crap_exempted = 2;
        exempt.crap_effective_threshold = Some(500.0);
        assert!((file_score_crap_concern(&exempt, CRAP_THRESHOLD) - 0.0).abs() < f64::EPSILON);
        assert!(file_score_fully_crap_exempt(&exempt, CRAP_THRESHOLD));
        assert_eq!(
            file_score_concern_axis(&exempt, CRAP_THRESHOLD),
            FileScoreConcern::Structural
        );
    }

    #[test]
    fn file_score_crap_concern_zeroes_when_enforcement_disabled() {
        let mut score = make_file_score("/src/any.ts", 88.0, 110.0);
        score.crap_above_threshold = 0;
        score.crap_exempted = 2;
        assert!((file_score_crap_concern(&score, 0.0) - 0.0).abs() < f64::EPSILON);
        assert!(file_score_fully_crap_exempt(&score, 0.0));
        assert_eq!(
            file_score_concern_axis(&score, 0.0),
            FileScoreConcern::Structural
        );
    }

    #[test]
    fn file_score_partial_exemption_keeps_risk_axis() {
        // One breaching function survives its ceiling: the row is NOT fully
        // exempt, so the risk story stays.
        let mut mixed = make_file_score("/src/mixed.ts", 88.0, 110.0);
        mixed.crap_above_threshold = 1;
        mixed.crap_exempted = 1;
        mixed.crap_effective_threshold = Some(30.0);
        assert!(!file_score_fully_crap_exempt(&mixed, CRAP_THRESHOLD));
        assert_eq!(
            file_score_concern_axis(&mixed, CRAP_THRESHOLD),
            FileScoreConcern::Risk
        );
    }

    #[test]
    fn file_score_concern_axis_labels_dominant_signal() {
        let risk_driven = make_file_score("/src/risk.ts", 84.8, 552.0);
        assert_eq!(
            file_score_concern_axis(&risk_driven, CRAP_THRESHOLD),
            FileScoreConcern::Risk
        );
        assert_eq!(
            file_score_concern_axis(&risk_driven, CRAP_THRESHOLD).label(),
            "risk"
        );

        let structure_driven = make_file_score("/src/structure.ts", 30.0, 8.0);
        assert_eq!(
            file_score_concern_axis(&structure_driven, CRAP_THRESHOLD),
            FileScoreConcern::Structural
        );
        assert_eq!(
            file_score_concern_axis(&structure_driven, CRAP_THRESHOLD).label(),
            "structure"
        );

        let no_risk = make_file_score("/src/clean.ts", 100.0, 0.0);
        assert_eq!(
            file_score_concern_axis(&no_risk, CRAP_THRESHOLD),
            FileScoreConcern::Structural
        );
    }

    #[test]
    fn file_score_triage_sort_prioritizes_high_crap_over_slightly_lower_mi() {
        let low_mi_low_risk = make_file_score("/src/low-mi-low-risk.ts", 81.7, 2.0);
        let higher_mi_high_risk = make_file_score("/src/higher-mi-high-risk.ts", 84.8, 552.0);

        let mut scores = [low_mi_low_risk, higher_mi_high_risk];
        scores.sort_by(|a, b| compare_file_score_triage(a, b, CRAP_THRESHOLD));

        assert_eq!(
            scores[0].path,
            std::path::Path::new("/src/higher-mi-high-risk.ts")
        );
        assert_eq!(
            scores[1].path,
            std::path::Path::new("/src/low-mi-low-risk.ts")
        );
    }

    #[test]
    fn file_score_triage_sort_orders_saturated_crap_by_raw_crap_descending() {
        let lower_crap_worse_mi = make_file_score("/src/a.ts", 84.8, 106.0);
        let higher_crap_better_mi = make_file_score("/src/b.ts", 96.7, 552.0);

        let mut scores = [lower_crap_worse_mi, higher_crap_better_mi];
        scores.sort_by(|a, b| compare_file_score_triage(a, b, CRAP_THRESHOLD));

        assert_eq!(scores[0].path, std::path::Path::new("/src/b.ts"));
        assert_eq!(scores[1].path, std::path::Path::new("/src/a.ts"));
    }

    #[test]
    fn file_score_triage_sort_uses_mi_crap_and_path_tie_breakers() {
        let mut scores = [
            make_file_score("/src/b.ts", 70.0, 1.0),
            make_file_score("/src/a.ts", 70.0, 1.0),
            make_file_score("/src/higher-crap.ts", 70.0, 2.0),
            make_file_score("/src/lower-concern.ts", 80.0, 1.0),
        ];

        scores.sort_by(|a, b| compare_file_score_triage(a, b, CRAP_THRESHOLD));

        let paths: Vec<_> = scores.iter().map(|score| score.path.as_path()).collect();
        assert_eq!(
            paths,
            vec![
                std::path::Path::new("/src/higher-crap.ts"),
                std::path::Path::new("/src/a.ts"),
                std::path::Path::new("/src/b.ts"),
                std::path::Path::new("/src/lower-concern.ts"),
            ]
        );
    }

    #[test]
    fn compute_file_scores_empty_graph() {
        let files: Vec<crate::discover::DiscoveredFile> = vec![];
        let graph = build_test_graph(&files, &[], &[]);
        let modules: Vec<crate::source::ModuleInfo> = vec![];
        let file_paths = rustc_hash::FxHashMap::default();

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert!(result.scores.is_empty());
        assert!(result.circular_files.is_empty());
        assert!(result.top_complex_fns.is_empty());
        assert!(result.entry_points.is_empty());
        assert_eq!(result.analysis_counts.total_exports, 0);
        assert_eq!(result.analysis_counts.dead_files, 0);
    }

    #[test]
    fn compute_file_scores_no_graph_returns_error() {
        let modules: Vec<crate::source::ModuleInfo> = vec![];
        let file_paths = rustc_hash::FxHashMap::default();

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: None,
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        );
        assert!(result.is_err());
        match result {
            Err(msg) => assert_eq!(msg, "graph not available"),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn compute_file_scores_single_file_with_function() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            exports: vec![fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("foo".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }]
            .into(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);

        let modules = vec![make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "foo".into(),
                is_private_member: false,
                line: 1,
                col: 0,
                cyclomatic: 5,
                cognitive: 3,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert_eq!(result.scores.len(), 1);

        let score = &result.scores[0];
        assert_eq!(score.path, path_a);
        assert_eq!(score.total_cyclomatic, 5);
        assert_eq!(score.total_cognitive, 3);
        assert_eq!(score.function_count, 1);
        assert_eq!(score.lines, 10);
        assert!((score.complexity_density - 0.5).abs() < f64::EPSILON);
        assert!(score.dead_code_ratio.abs() < f64::EPSILON);
        assert!(result.entry_points.contains(&path_a));
    }

    #[test]
    fn compute_file_scores_excludes_barrel_files() {
        let path_a = std::path::PathBuf::from("/src/index.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 50,
        }];

        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);

        let modules = vec![make_module_info(0, 5, vec![])];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert!(result.scores.is_empty());
    }

    #[test]
    fn compute_file_scores_changed_since_filter() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let path_b = std::path::PathBuf::from("/src/b.ts");
        let files = vec![
            crate::discover::DiscoveredFile {
                id: crate::discover::FileId(0),
                path: path_a.clone(),
                size_bytes: 100,
            },
            crate::discover::DiscoveredFile {
                id: crate::discover::FileId(1),
                path: path_b.clone(),
                size_bytes: 100,
            },
        ];

        let resolved_modules = vec![
            fallow_graph::resolve::ResolvedModule {
                file_id: crate::discover::FileId(0),
                path: path_a,
                ..Default::default()
            },
            fallow_graph::resolve::ResolvedModule {
                file_id: crate::discover::FileId(1),
                path: path_b.clone(),
                ..Default::default()
            },
        ];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![
            make_module_info(
                0,
                10,
                vec![fallow_types::extract::FunctionComplexity {
                    name: "fn_a".into(),
                    is_private_member: false,
                    line: 1,
                    col: 0,
                    cyclomatic: 2,
                    cognitive: 1,
                    line_count: 10,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
            make_module_info(
                1,
                10,
                vec![fallow_types::extract::FunctionComplexity {
                    name: "fn_b".into(),
                    is_private_member: false,
                    line: 1,
                    col: 0,
                    cyclomatic: 3,
                    cognitive: 2,
                    line_count: 10,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
        ];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);
        file_paths.insert(crate::discover::FileId(1), &files[1].path);

        let path_b_check = std::path::PathBuf::from("/src/b.ts");
        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(path_b);

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            Some(&changed),
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert_eq!(result.scores.len(), 1);
        assert_eq!(result.scores[0].path, path_b_check);
    }

    #[test]
    fn compute_file_scores_sorted_by_triage_concern() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let path_b = std::path::PathBuf::from("/src/b.ts");
        let files = vec![
            crate::discover::DiscoveredFile {
                id: crate::discover::FileId(0),
                path: path_a.clone(),
                size_bytes: 100,
            },
            crate::discover::DiscoveredFile {
                id: crate::discover::FileId(1),
                path: path_b.clone(),
                size_bytes: 100,
            },
        ];

        let resolved_modules = vec![
            fallow_graph::resolve::ResolvedModule {
                file_id: crate::discover::FileId(0),
                path: path_a.clone(),
                ..Default::default()
            },
            fallow_graph::resolve::ResolvedModule {
                file_id: crate::discover::FileId(1),
                path: path_b,
                ..Default::default()
            },
        ];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![
            make_module_info(
                0,
                10,
                vec![fallow_types::extract::FunctionComplexity {
                    name: "complex_fn".into(),
                    is_private_member: false,
                    line: 1,
                    col: 0,
                    cyclomatic: 30,
                    cognitive: 20,
                    line_count: 10,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
            make_module_info(
                1,
                100,
                vec![fallow_types::extract::FunctionComplexity {
                    name: "simple_fn".into(),
                    is_private_member: false,
                    line: 1,
                    col: 0,
                    cyclomatic: 1,
                    cognitive: 0,
                    line_count: 100,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
        ];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);
        file_paths.insert(crate::discover::FileId(1), &files[1].path);

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert_eq!(result.scores.len(), 2);
        assert!(result.scores[0].maintainability_index <= result.scores[1].maintainability_index);
        assert_eq!(result.scores[0].path, path_a);
    }

    #[test]
    fn compute_file_scores_with_unused_file_populates_evidence() {
        let path_a = std::path::PathBuf::from("/src/unused.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            exports: vec![fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("orphan".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }]
            .into(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "orphan".into(),
                is_private_member: false,
                line: 1,
                col: 0,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_files.push(
            fallow_types::output_dead_code::UnusedFileFinding::with_actions(
                fallow_types::results::UnusedFile {
                    path: path_a.clone(),
                },
            ),
        );

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results,
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert_eq!(result.scores.len(), 1);
        assert!((result.scores[0].dead_code_ratio - 1.0).abs() < f64::EPSILON);
        assert!(result.unused_export_names.contains_key(&path_a));
        let names = &result.unused_export_names[&path_a];
        assert_eq!(names, &["orphan"]);
        assert_eq!(result.analysis_counts.dead_files, 1);
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
    fn compute_file_scores_tracks_top_complex_functions() {
        let path_a = std::path::PathBuf::from("/src/complex.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 500,
        }];

        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![make_module_info(
            0,
            50,
            vec![
                fallow_types::extract::FunctionComplexity {
                    name: "high".into(),
                    is_private_member: false,
                    line: 1,
                    col: 0,
                    cyclomatic: 10,
                    cognitive: 20,
                    line_count: 10,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
                fallow_types::extract::FunctionComplexity {
                    name: "medium".into(),
                    is_private_member: false,
                    line: 11,
                    col: 0,
                    cyclomatic: 5,
                    cognitive: 10,
                    line_count: 10,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
                fallow_types::extract::FunctionComplexity {
                    name: "low".into(),
                    is_private_member: false,
                    line: 21,
                    col: 0,
                    cyclomatic: 2,
                    cognitive: 5,
                    line_count: 10,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
                fallow_types::extract::FunctionComplexity {
                    name: "trivial".into(),
                    is_private_member: false,
                    line: 31,
                    col: 0,
                    cyclomatic: 1,
                    cognitive: 1,
                    line_count: 10,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
            ],
        )];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert!(result.top_complex_fns.contains_key(&path_a));
        let top = &result.top_complex_fns[&path_a];
        assert_eq!(top.len(), 3);
        assert_eq!(top[0].0, "high");
        assert_eq!(top[0].2, 20);
        assert_eq!(top[1].0, "medium");
        assert_eq!(top[1].2, 10);
        assert_eq!(top[2].0, "low");
        assert_eq!(top[2].2, 5);
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
    fn compute_file_scores_with_circular_deps() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let path_b = std::path::PathBuf::from("/src/b.ts");
        let files = vec![
            crate::discover::DiscoveredFile {
                id: crate::discover::FileId(0),
                path: path_a.clone(),
                size_bytes: 100,
            },
            crate::discover::DiscoveredFile {
                id: crate::discover::FileId(1),
                path: path_b.clone(),
                size_bytes: 100,
            },
        ];

        let resolved_modules = vec![
            fallow_graph::resolve::ResolvedModule {
                file_id: crate::discover::FileId(0),
                path: path_a.clone(),
                ..Default::default()
            },
            fallow_graph::resolve::ResolvedModule {
                file_id: crate::discover::FileId(1),
                path: path_b.clone(),
                ..Default::default()
            },
        ];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![
            make_module_info(
                0,
                10,
                vec![fallow_types::extract::FunctionComplexity {
                    name: "fn_a".into(),
                    is_private_member: false,
                    line: 1,
                    col: 0,
                    cyclomatic: 2,
                    cognitive: 1,
                    line_count: 10,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
            make_module_info(
                1,
                10,
                vec![fallow_types::extract::FunctionComplexity {
                    name: "fn_b".into(),
                    is_private_member: false,
                    line: 1,
                    col: 0,
                    cyclomatic: 3,
                    cognitive: 2,
                    line_count: 10,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
        ];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);
        file_paths.insert(crate::discover::FileId(1), &files[1].path);

        let mut results = fallow_types::results::AnalysisResults::default();
        results.circular_dependencies.push(
            fallow_types::output_dead_code::CircularDependencyFinding::with_actions(
                fallow_types::results::CircularDependency {
                    files: vec![path_a.clone(), path_b.clone()],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ),
        );

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results,
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert!(result.circular_files.contains(&path_a));
        assert!(result.circular_files.contains(&path_b));
        assert!(result.cycle_members.contains_key(&path_a));
        assert_eq!(result.cycle_members[&path_a], vec![path_b.clone()]);
        assert!(result.cycle_members.contains_key(&path_b));
        assert_eq!(result.cycle_members[&path_b], vec![path_a]);
        assert_eq!(result.analysis_counts.circular_deps, 1);
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
    fn compute_file_scores_analysis_counts_unused_exports_and_types() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: crate::source::ExportName::Named("foo".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: crate::source::VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: crate::source::ExportName::Named("bar".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: crate::source::VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ]
            .into(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let mut module = make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_a".into(),
                is_private_member: false,
                line: 1,
                col: 0,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        );
        module.exports = vec![
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("foo".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("bar".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
        ]
        .into();
        let modules = vec![module];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_exports.push(
            fallow_types::output_dead_code::UnusedExportFinding::with_actions(
                fallow_types::results::UnusedExport {
                    path: path_a.clone(),
                    export_name: "foo".into(),
                    is_type_only: false,
                    line: 1,
                    col: 0,
                    span_start: 0,
                    is_re_export: false,
                },
            ),
        );
        results.unused_types.push(
            fallow_types::output_dead_code::UnusedTypeFinding::with_actions(
                fallow_types::results::UnusedExport {
                    path: path_a,
                    export_name: "MyType".into(),
                    is_type_only: true,
                    line: 5,
                    col: 0,
                    span_start: 40,
                    is_re_export: false,
                },
            ),
        );
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                fallow_types::results::UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: std::path::PathBuf::from("/package.json"),
                    line: 1,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results,
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert_eq!(result.analysis_counts.total_exports, 2);
        assert_eq!(result.analysis_counts.dead_exports, 2);
        assert_eq!(result.analysis_counts.unused_deps, 1);
    }

    /// Regression: total_exports must count graph modules, not extraction modules.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
    fn total_exports_counts_graph_modules_not_extraction_modules() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: crate::source::ExportName::Named("foo".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: crate::source::VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: crate::source::ExportName::Named("bar".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: crate::source::VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: crate::source::ExportName::Named("baz".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: crate::source::VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ]
            .into(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let mut module = make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_a".into(),
                is_private_member: false,
                line: 1,
                col: 0,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        );
        module.exports = vec![
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("foo".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("bar".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
        ]
        .into();
        let modules = vec![module];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);

        let mut results = fallow_types::results::AnalysisResults::default();
        for name in ["foo", "bar", "baz"] {
            results.unused_exports.push(
                fallow_types::output_dead_code::UnusedExportFinding::with_actions(
                    fallow_types::results::UnusedExport {
                        path: path_a.clone(),
                        export_name: name.into(),
                        is_type_only: false,
                        line: 1,
                        col: 0,
                        span_start: 0,
                        is_re_export: name == "baz",
                    },
                ),
            );
        }

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results,
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert_eq!(result.analysis_counts.total_exports, 3);
        assert_eq!(result.analysis_counts.dead_exports, 3);
    }

    #[test]
    fn compute_file_scores_module_not_in_file_paths_skipped() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a,
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_a".into(),
                is_private_member: false,
                line: 1,
                col: 0,
                cyclomatic: 2,
                cognitive: 1,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert!(result.scores.is_empty());
    }

    #[test]
    fn compute_file_scores_mi_rounded_to_one_decimal() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);

        let modules = vec![make_module_info(
            0,
            100,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn".into(),
                is_private_member: false,
                line: 1,
                col: 0,
                cyclomatic: 7,
                cognitive: 3,
                line_count: 100,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        let mi = result.scores[0].maintainability_index;
        let rounded = (mi * 10.0).round() / 10.0;
        assert!((mi - rounded).abs() < f64::EPSILON);
    }

    #[test]
    fn compute_file_scores_value_export_counts_tracked() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: crate::source::ExportName::Named("a".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: crate::source::VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: crate::source::ExportName::Named("b".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: crate::source::VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: crate::source::ExportName::Named("T".into()),
                    local_name: None,
                    is_type_only: true,
                    visibility: crate::source::VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ]
            .into(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_a".into(),
                is_private_member: false,
                line: 1,
                col: 0,
                cyclomatic: 2,
                cognitive: 1,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert_eq!(result.value_export_counts[&path_a], 2);
    }

    #[test]
    fn compute_file_scores_top_complex_fns_zero_cognitive_excluded() {
        let path_a = std::path::PathBuf::from("/src/simple.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "trivial".into(),
                is_private_member: false,
                line: 1,
                col: 0,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);

        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores_default(
            &modules,
            &file_paths,
            None,
            output,
            None,
            std::path::Path::new("/project"),
        )
        .unwrap();
        assert!(!result.top_complex_fns.contains_key(&path_a));
    }

    fn make_fn_complexity(cyclomatic: u16) -> fallow_types::extract::FunctionComplexity {
        fallow_types::extract::FunctionComplexity {
            name: "test_fn".into(),
            is_private_member: false,
            line: 1,
            col: 0,
            cyclomatic,
            cognitive: 0,
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }
    }

    fn make_named_fn_complexity(
        name: &str,
        line: u32,
        cyclomatic: u16,
    ) -> fallow_types::extract::FunctionComplexity {
        fallow_types::extract::FunctionComplexity {
            name: name.into(),
            is_private_member: false,
            line,
            col: 0,
            cyclomatic,
            cognitive: 0,
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }
    }

    fn crap_override_entry(
        files: &[&str],
        functions: &[&str],
        max_crap: Option<f64>,
    ) -> fallow_config::HealthThresholdOverride {
        fallow_config::HealthThresholdOverride {
            files: files.iter().map(ToString::to_string).collect(),
            functions: functions.iter().map(ToString::to_string).collect(),
            max_cyclomatic: None,
            max_cognitive: None,
            max_crap,
            max_unit_size: None,
            reason: Some("test override".into()),
        }
    }

    fn estimated_signals_with(
        resolver: &ThresholdOverrideResolver,
        relative: &str,
        enforce_crap: bool,
        complexity: &[fallow_types::extract::FunctionComplexity],
    ) -> CrapThresholdSignals {
        let ceilings = CrapCeilingLookup::new(
            CrapScoreThresholds {
                resolver,
                enforce_crap,
            },
            std::path::Path::new(relative),
        );
        compute_crap_scores_estimated(
            complexity,
            &rustc_hash::FxHashSet::default(),
            false,
            fallow_output::CoverageSource::Estimated,
            &ceilings,
        )
        .signals
    }

    #[test]
    fn crap_counting_exempts_functions_under_override_ceiling() {
        // The issue's repro: two untested cyclomatic-10 functions (CRAP 110)
        // under an override raising maxCrap to 500 on the file.
        let resolver =
            test_override_resolver(&[crap_override_entry(&["src/legacy.ts"], &[], Some(500.0))]);
        let fns = vec![
            make_named_fn_complexity("a", 1, 10),
            make_named_fn_complexity("b", 12, 10),
        ];

        let covered = estimated_signals_with(&resolver, "src/legacy.ts", true, &fns);
        assert_eq!(covered.above, 0);
        assert_eq!(covered.exempted, 2);
        assert_eq!(covered.min_ceiling, Some(500.0));

        let elsewhere = estimated_signals_with(&resolver, "src/other.ts", true, &fns);
        assert_eq!(elsewhere.above, 2);
        assert_eq!(elsewhere.exempted, 0);
        assert_eq!(elsewhere.min_ceiling, Some(CRAP_THRESHOLD));
    }

    #[test]
    fn crap_counting_insufficient_override_keeps_count() {
        let resolver =
            test_override_resolver(&[crap_override_entry(&["src/legacy.ts"], &[], Some(50.0))]);
        let fns = vec![
            make_named_fn_complexity("a", 1, 10),
            make_named_fn_complexity("b", 12, 10),
        ];

        let signals = estimated_signals_with(&resolver, "src/legacy.ts", true, &fns);
        assert_eq!(signals.above, 2);
        assert_eq!(signals.exempted, 0);
        assert_eq!(signals.min_ceiling, Some(50.0));
    }

    #[test]
    fn crap_counting_partial_function_override() {
        // Only `a` is exempted; `b` keeps the global ceiling, which is also
        // the file's lowest effective ceiling.
        let resolver =
            test_override_resolver(&[crap_override_entry(&["src/legacy.ts"], &["a"], Some(500.0))]);
        let fns = vec![
            make_named_fn_complexity("a", 1, 10),
            make_named_fn_complexity("b", 12, 10),
        ];

        let signals = estimated_signals_with(&resolver, "src/legacy.ts", true, &fns);
        assert_eq!(signals.above, 1);
        assert_eq!(signals.exempted, 1);
        assert_eq!(signals.min_ceiling, Some(CRAP_THRESHOLD));
    }

    #[test]
    fn crap_counting_disabled_enforcement_counts_baseline_exemptions() {
        // Global maxCrap 0 disables enforcement: nothing is above threshold
        // and every canonical-baseline breach is disclosed as exempt.
        let resolver = test_crap_resolver(0.0);
        let fns = vec![
            make_named_fn_complexity("a", 1, 10),
            make_named_fn_complexity("b", 12, 10),
            make_named_fn_complexity("tiny", 24, 1),
        ];

        let signals = estimated_signals_with(&resolver, "src/any.ts", false, &fns);
        assert_eq!(signals.above, 0);
        assert_eq!(signals.exempted, 2);
    }

    #[test]
    fn crap_counting_stricter_ceiling_never_counts_exempt() {
        // A ceiling below the canonical baseline flags the band between it and
        // 30 as above-threshold, never as exempt.
        let resolver = test_crap_resolver(10.0);
        let fns = vec![make_named_fn_complexity("a", 1, 4)]; // untested CRAP 20

        let signals = estimated_signals_with(&resolver, "src/any.ts", true, &fns);
        assert_eq!(signals.above, 1);
        assert_eq!(signals.exempted, 0);
    }

    #[test]
    fn crap_counting_uses_rounded_value_at_boundary() {
        // Istanbul coverage tuned so unrounded CRAP is 29.96, which rounds to
        // the stored per-function 30.0. The findings pipeline compares the
        // ROUNDED value against the ceiling and emits a finding; the count
        // must agree at the same boundary (issue #2228).
        let funcs = vec![make_fn_complexity(10)];
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("test_fn".to_string(), 1, 0), 41.56);
        let file_cov = test_istanbul_file_coverage(functions, false);

        let result = istanbul_crap_default(&funcs, Some(&file_cov), false);
        assert!((result.per_function[0].crap - 30.0).abs() < f64::EPSILON);
        assert_eq!(result.signals.above, 1);
        assert_eq!(result.signals.exempted, 0);
    }

    #[test]
    fn compute_file_scores_discloses_override_exemption_on_row() {
        let path_a = std::path::PathBuf::from("/project/src/legacy.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];
        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        }];
        let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);
        let modules = vec![make_module_info(
            0,
            26,
            vec![
                make_named_fn_complexity("a", 1, 10),
                make_named_fn_complexity("b", 12, 10),
            ],
        )];
        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);
        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let resolver =
            test_override_resolver(&[crap_override_entry(&["src/legacy.ts"], &[], Some(500.0))]);
        let result = compute_file_scores(
            FileScoreComputeInput {
                modules: &modules,
                file_paths: &file_paths,
                changed_files: None,
                istanbul_coverage: None,
                root: std::path::Path::new("/project"),
                crap_thresholds: CrapScoreThresholds {
                    resolver: &resolver,
                    enforce_crap: true,
                },
            },
            output,
        )
        .unwrap();

        assert_eq!(result.scores.len(), 1);
        let score = &result.scores[0];
        assert!((score.crap_max - 110.0).abs() < f64::EPSILON);
        assert_eq!(score.crap_above_threshold, 0);
        assert_eq!(score.crap_exempted, 2);
        assert_eq!(score.crap_effective_threshold, Some(500.0));
        assert!(file_score_fully_crap_exempt(score, CRAP_THRESHOLD));
        assert_eq!(
            file_score_concern_axis(score, CRAP_THRESHOLD),
            FileScoreConcern::Structural
        );
    }

    #[test]
    fn compute_file_scores_raised_global_omits_row_threshold() {
        let path_a = std::path::PathBuf::from("/project/src/legacy.ts");
        let files = vec![crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];
        let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        }];
        let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);
        let modules = vec![make_module_info(
            0,
            26,
            vec![
                make_named_fn_complexity("a", 1, 10),
                make_named_fn_complexity("b", 12, 10),
            ],
        )];
        let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();
        file_paths.insert(crate::discover::FileId(0), &files[0].path);
        let output = crate::results::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        // Raised via the global (`--max-crap 5000`), not an override: the row
        // must not repeat the run global as its own effective threshold.
        let resolver = test_crap_resolver(5000.0);
        let result = compute_file_scores(
            FileScoreComputeInput {
                modules: &modules,
                file_paths: &file_paths,
                changed_files: None,
                istanbul_coverage: None,
                root: std::path::Path::new("/project"),
                crap_thresholds: CrapScoreThresholds {
                    resolver: &resolver,
                    enforce_crap: true,
                },
            },
            output,
        )
        .unwrap();

        assert_eq!(result.scores.len(), 1);
        let score = &result.scores[0];
        assert_eq!(score.crap_above_threshold, 0);
        assert_eq!(score.crap_exempted, 2);
        assert_eq!(score.crap_effective_threshold, None);
        assert!(file_score_fully_crap_exempt(score, 5000.0));
        assert_eq!(
            file_score_concern_axis(score, 5000.0),
            FileScoreConcern::Structural
        );
    }

    #[test]
    fn crap_scores_empty_complexity() {
        let (max, above) = compute_crap_scores_binary(&[], true);
        assert!((max).abs() < f64::EPSILON);
        assert_eq!(above, 0);
    }

    #[test]
    fn crap_scores_test_reachable() {
        let funcs = vec![make_fn_complexity(5)];
        let (max, above) = compute_crap_scores_binary(&funcs, true);
        assert!((max - 5.0).abs() < f64::EPSILON);
        assert_eq!(above, 0);
    }

    #[test]
    fn crap_scores_untested_at_threshold() {
        let funcs = vec![make_fn_complexity(5)];
        let (max, above) = compute_crap_scores_binary(&funcs, false);
        assert!((max - 30.0).abs() < f64::EPSILON);
        assert_eq!(above, 1);
    }

    #[test]
    fn crap_scores_untested_above_threshold() {
        let funcs = vec![make_fn_complexity(6)];
        let (max, above) = compute_crap_scores_binary(&funcs, false);
        assert!((max - 42.0).abs() < f64::EPSILON);
        assert_eq!(above, 1);
    }

    #[test]
    fn crap_scores_untested_below_threshold() {
        let funcs = vec![make_fn_complexity(4)];
        let (max, above) = compute_crap_scores_binary(&funcs, false);
        assert!((max - 20.0).abs() < f64::EPSILON);
        assert_eq!(above, 0);
    }

    #[test]
    fn crap_scores_mixed_functions_untested() {
        let funcs = vec![
            make_fn_complexity(2),
            make_fn_complexity(5),
            make_fn_complexity(8),
        ];
        let (max, above) = compute_crap_scores_binary(&funcs, false);
        assert!((max - 72.0).abs() < f64::EPSILON);
        assert_eq!(above, 2);
    }

    #[test]
    fn crap_formula_full_coverage() {
        let result = crap_formula(10.0, 100.0);
        assert!((result - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn crap_formula_zero_coverage() {
        let result = crap_formula(5.0, 0.0);
        assert!((result - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn crap_formula_partial_coverage() {
        let result = crap_formula(10.0, 50.0);
        assert!((result - 22.5).abs() < f64::EPSILON);
    }

    #[test]
    fn crap_formula_high_coverage_low_complexity() {
        let result = crap_formula(2.0, 90.0);
        assert!((result - 2.004).abs() < 0.001);
    }

    /// Pin the exact cyclomatic value at which the default CRAP gate (30.0)
    /// trips for each estimated-coverage tier. These numbers back the
    /// changelog and docs wording: 5 at the 0% tier, 10 at the 40% indirect
    /// tier, 28 at the 85% direct tier.
    #[test]
    fn crap_default_gate_cyclomatic_boundaries_per_estimate_tier() {
        for (coverage_pct, gate_cc) in [(0.0, 5.0), (40.0, 10.0), (85.0, 28.0)] {
            assert!(
                crap_formula(gate_cc, coverage_pct) >= CRAP_THRESHOLD,
                "cyclomatic {gate_cc} at {coverage_pct}% must reach the gate"
            );
            assert!(
                crap_formula(gate_cc - 1.0, coverage_pct) < CRAP_THRESHOLD,
                "cyclomatic {} at {coverage_pct}% must stay under the gate",
                gate_cc - 1.0
            );
        }
    }

    #[test]
    fn istanbul_crap_excludes_synthetic_template_units() {
        let funcs = vec![
            make_named_fn_complexity("<template>", 1, 21),
            make_named_fn_complexity("<snippet:rowBody>", 1, 16),
            make_fn_complexity(6),
        ];
        let result = istanbul_crap_default(&funcs, None, false);
        assert!((result.max_crap - 42.0).abs() < f64::EPSILON, "{result:#?}");
        assert_eq!(result.signals.above, 1);
        assert_eq!(
            result.total, 1,
            "template units must not count as unmatched"
        );
        assert_eq!(result.per_function.len(), 1);
    }

    #[test]
    fn estimated_crap_excludes_synthetic_template_units() {
        let funcs = vec![
            make_named_fn_complexity("<template>", 1, 21),
            make_named_fn_complexity("<snippet:rowBody>", 1, 16),
        ];
        let result = estimated_crap_default(
            &funcs,
            &rustc_hash::FxHashSet::default(),
            false,
            fallow_output::CoverageSource::Estimated,
        );
        assert!(result.max_crap.abs() < f64::EPSILON, "{result:#?}");
        assert_eq!(result.signals.above, 0);
        assert!(result.per_function.is_empty());
    }

    #[test]
    fn istanbul_crap_with_coverage_data() {
        let funcs = vec![make_fn_complexity(10)];
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("test_fn".to_string(), 1, 0), 80.0);
        let file_cov = test_istanbul_file_coverage(functions, false);
        let result = istanbul_crap_default(&funcs, Some(&file_cov), false);
        assert!((result.max_crap - 10.8).abs() < 0.1);
        assert_eq!(result.signals.above, 0);
    }

    #[test]
    fn istanbul_crap_falls_back_to_binary_when_no_match() {
        let funcs = vec![make_fn_complexity(6)];
        let file_cov = test_istanbul_file_coverage(rustc_hash::FxHashMap::default(), false);
        let result = istanbul_crap_default(&funcs, Some(&file_cov), false);
        assert!((result.max_crap - 42.0).abs() < f64::EPSILON);
        assert_eq!(result.signals.above, 1);
    }

    /// A file tests reach, with no coverage data for it at all, keeps the
    /// static estimate rather than being scored as fully covered.
    #[test]
    fn istanbul_crap_uses_the_static_estimate_when_no_file_coverage() {
        let funcs = vec![make_fn_complexity(5)];
        let result = istanbul_crap_default(&funcs, None, true);
        // The reported score is rounded to one decimal, so compare against
        // the estimate rather than the formula's last bit.
        assert!((result.max_crap - 10.4).abs() < 1e-9);
        assert_eq!(result.signals.above, 0);
    }

    #[test]
    fn istanbul_crap_zero_coverage_matches_binary_untested() {
        let funcs = vec![make_fn_complexity(5)];
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("test_fn".to_string(), 1, 0), 0.0);
        let file_cov = test_istanbul_file_coverage(functions, false);
        let result = istanbul_crap_default(&funcs, Some(&file_cov), false);
        assert!((result.max_crap - 30.0).abs() < f64::EPSILON);
        assert_eq!(result.signals.above, 1);
    }

    #[test]
    fn estimated_crap_direct_test_reference() {
        let funcs = vec![make_fn_complexity(10)];
        let mut refs = rustc_hash::FxHashSet::default();
        refs.insert("test_fn".to_string());
        let result = estimated_crap_default(
            &funcs,
            &refs,
            true,
            fallow_output::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.signals.above);
        assert!((max - 10.3).abs() < 0.1);
        assert_eq!(above, 0);
    }

    #[test]
    fn estimated_crap_indirect_test_reachable() {
        let funcs = vec![make_fn_complexity(10)];
        let refs = rustc_hash::FxHashSet::default();
        let result = estimated_crap_default(
            &funcs,
            &refs,
            true,
            fallow_output::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.signals.above);
        assert!((max - 31.6).abs() < 0.1);
        assert_eq!(above, 1);
    }

    #[test]
    fn estimated_crap_untested_file() {
        let funcs = vec![make_fn_complexity(5)];
        let refs = rustc_hash::FxHashSet::default();
        let result = estimated_crap_default(
            &funcs,
            &refs,
            false,
            fallow_output::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.signals.above);
        assert!((max - 30.0).abs() < f64::EPSILON);
        assert_eq!(above, 1);
    }

    #[test]
    fn estimated_crap_low_complexity_direct_ref() {
        let funcs = vec![make_fn_complexity(2)];
        let mut refs = rustc_hash::FxHashSet::default();
        refs.insert("test_fn".to_string());
        let result = estimated_crap_default(
            &funcs,
            &refs,
            true,
            fallow_output::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.signals.above);
        assert!(max < 3.0);
        assert_eq!(above, 0);
    }

    #[test]
    fn estimated_crap_empty() {
        let refs = rustc_hash::FxHashSet::default();
        let result =
            estimated_crap_default(&[], &refs, true, fallow_output::CoverageSource::Estimated);
        let (max, above) = (result.max_crap, result.signals.above);
        assert!((max).abs() < f64::EPSILON);
        assert_eq!(above, 0);
    }

    fn make_export(name: &str, is_type_only: bool) -> fallow_graph::graph::ExportSymbol {
        fallow_graph::graph::ExportSymbol {
            name: fallow_types::extract::ExportName::Named(name.into()),
            is_type_only,
            is_side_effect_used: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::default(),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
        }
    }

    #[test]
    fn dead_code_ratio_type_only_exports_excluded_from_denominator() {
        let path = std::path::Path::new("src/types.ts");
        let exports = vec![
            make_export("MyInterface", true),
            make_export("MyType", true),
            make_export("myFunction", false),
        ];
        let unused_files = rustc_hash::FxHashSet::default();
        let mut unused_by_path = rustc_hash::FxHashMap::default();
        unused_by_path.insert(path, 1_usize); // 1 unused value export

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_by_path);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_ratio_only_type_exports_returns_zero() {
        let path = std::path::Path::new("src/types.ts");
        let exports = vec![
            make_export("MyInterface", true),
            make_export("MyType", true),
        ];
        let unused_files = rustc_hash::FxHashSet::default();
        let unused_by_path = rustc_hash::FxHashMap::default();

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_by_path);
        assert!(ratio.abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_ratio_mixed_exports_counts_only_values() {
        let path = std::path::Path::new("src/component.ts");
        let exports = vec![
            make_export("Props", true),
            make_export("State", true),
            make_export("Component", false),
            make_export("helper", false),
        ];
        let unused_files = rustc_hash::FxHashSet::default();
        let mut unused_by_path = rustc_hash::FxHashMap::default();
        unused_by_path.insert(path, 1_usize);

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_by_path);
        assert!((ratio - 0.5).abs() < f64::EPSILON);
    }

    fn write_single_file_istanbul_fixture(
        coverage_path: &std::path::Path,
        source_path: &std::path::Path,
        fn_map: &serde_json::Value,
        function_hits: &serde_json::Value,
    ) {
        let mut root = serde_json::Map::new();
        root.insert(
            source_path.to_string_lossy().into_owned(),
            serde_json::json!({
                "path": source_path.to_string_lossy().into_owned(),
                "statementMap": {},
                "fnMap": fn_map,
                "branchMap": {},
                "s": {},
                "f": function_hits,
                "b": {}
            }),
        );

        std::fs::write(coverage_path, serde_json::to_string(&root).unwrap()).unwrap();
    }

    #[test]
    fn resolve_relative_to_root_joins_relative_with_project_root() {
        let resolved = resolve_relative_to_root(
            std::path::Path::new("coverage/coverage-final.json"),
            Some(std::path::Path::new("/work/my-app")),
        );
        assert_eq!(
            resolved,
            std::path::PathBuf::from("/work/my-app/coverage/coverage-final.json")
        );
    }

    #[test]
    fn resolve_relative_to_root_returns_absolute_unchanged() {
        let resolved = resolve_relative_to_root(
            std::path::Path::new("/tmp/coverage-final.json"),
            Some(std::path::Path::new("/work/my-app")),
        );
        assert_eq!(
            resolved,
            std::path::PathBuf::from("/tmp/coverage-final.json")
        );
    }

    #[test]
    fn resolve_relative_to_root_returns_windows_absolute_unchanged_on_any_host() {
        let path = std::path::Path::new(r"C:\coverage\coverage-final.json");
        let resolved = resolve_relative_to_root(path, Some(std::path::Path::new("/work/my-app")));
        assert_eq!(resolved, path);
    }

    #[cfg(windows)]
    #[test]
    fn resolve_relative_to_root_returns_posix_rooted_path_unchanged_on_windows() {
        let path = std::path::Path::new(r"/ci/workspace/coverage-final.json");
        let resolved =
            resolve_relative_to_root(path, Some(std::path::Path::new(r"C:\work\my-app")));
        assert_eq!(resolved, path);
    }

    #[test]
    fn resolve_relative_to_root_without_project_root_returns_relative_unchanged() {
        let resolved =
            resolve_relative_to_root(std::path::Path::new("coverage/coverage-final.json"), None);
        assert_eq!(
            resolved,
            std::path::PathBuf::from("coverage/coverage-final.json")
        );
    }

    /// nyc and some Jest setups record project-relative keys. Resolving one
    /// against the process directory misses the file, and a run from anywhere
    /// but the project root loses the whole map at once.
    #[test]
    fn load_istanbul_coverage_resolves_relative_map_keys_against_project_root() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/index.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export function f(){}").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        std::fs::write(
            &coverage_path,
            serde_json::to_string(&serde_json::json!({
                "src/index.ts": {
                    "path": "src/index.ts",
                    "statementMap": {},
                    "fnMap": {
                        "0": {
                            "name": "f",
                            "decl": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 21 } },
                            "loc": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 21 } }
                        }
                    },
                    "branchMap": {},
                    "s": {},
                    "f": { "0": 2 },
                    "b": {}
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let coverage =
            load_istanbul_coverage(&coverage_path, None, Some(temp.path()), false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("f", 1, 0), Some(100.0));
    }

    #[test]
    fn load_istanbul_coverage_resolves_relative_path_against_project_root() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/index.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export function f(){}").unwrap();

        let coverage_path = temp.path().join("coverage/coverage-final.json");
        std::fs::create_dir_all(coverage_path.parent().unwrap()).unwrap();
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "f",
                    "decl": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 21 } },
                    "loc":  { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 21 } }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );

        let coverage = load_istanbul_coverage(
            std::path::Path::new("coverage/coverage-final.json"),
            None,
            Some(temp.path()),
            false,
        )
        .expect("relative path must resolve against project_root");
        assert!(
            !coverage.files.is_empty(),
            "expected coverage to load via project_root resolution, got {} files",
            coverage.files.len()
        );
    }

    #[test]
    fn load_istanbul_coverage_falls_back_to_decl_line_for_missing_fn_line() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/service.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export class DataService {}\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "decl": {
                        "start": { "line": 5, "column": 2 },
                        "end": { "line": 5, "column": 13 }
                    },
                    "loc": {
                        "start": { "line": 5, "column": 14 },
                        "end": { "line": 11, "column": 3 }
                    }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "decl": {
                        "start": { "line": 20, "column": 14 },
                        "end": { "line": 20, "column": 25 }
                    },
                    "loc": {
                        "start": { "line": 20, "column": 28 },
                        "end": { "line": 22, "column": 2 }
                    }
                }
            }),
            &serde_json::json!({
                "0": 1,
                "1": 0
            }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("processData", 5, 0), Some(100.0));
        assert_eq!(file_coverage.lookup("handleSpecial", 20, 0), Some(0.0));
    }

    #[test]
    fn load_istanbul_coverage_indexes_explicit_and_decl_lines() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/handler.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export const handleClick = () => {}\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "handleClick",
                    "line": 40,
                    "decl": {
                        "start": { "line": 22, "column": 13 },
                        "end": { "line": 22, "column": 24 }
                    },
                    "loc": {
                        "start": { "line": 40, "column": 27 },
                        "end": { "line": 42, "column": 1 }
                    }
                }
            }),
            &serde_json::json!({
                "0": 1
            }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("handleClick", 40, 0), Some(100.0));
        assert_eq!(file_coverage.lookup("handleClick", 22, 13), Some(100.0));
    }

    #[test]
    fn load_istanbul_coverage_indexes_valid_body_start_alias() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/handler.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export const handler = () => true;\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 8,
                    "decl": {
                        "start": { "line": 8, "column": 14 },
                        "end": { "line": 8, "column": 25 }
                    },
                    "loc": {
                        "start": { "line": 20, "column": 6 },
                        "end": { "line": 22, "column": 1 }
                    }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("handler", 20, 6), Some(100.0));

        let mut function = make_fn_complexity(4);
        function.name = "handler".to_string();
        function.line = 20;
        function.col = 6;
        let result = istanbul_crap_default(&[function], Some(file_coverage), false);
        assert_eq!(result.matched, 1);
        assert_eq!(result.total, 1);
        assert_eq!(
            result.per_function[0].coverage_source,
            fallow_output::CoverageSource::Istanbul
        );
        assert_eq!(result.per_function[0].coverage_pct, Some(100.0));
    }

    /// Curried arrows written one per line put each record's body start on
    /// the next record's declaration. The declaration is primary and wins the
    /// position, so each arrow keeps a record of its own and the innermost one
    /// resolves through the header span that opens at the arrow above it.
    /// Geometry from istanbul-lib-instrument 6 for the source below, which is
    /// what Prettier produces for a curried arrow.
    #[test]
    fn curried_arrows_one_per_line_each_take_their_own_record() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/adjust.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export const adjust = (base: number) =>\n  (factor: number) =>\n  (offset: number) =>\n    base * factor + offset;\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 2,
                    "decl": { "start": { "line": 1, "column": 22 }, "end": { "line": 1, "column": 23 } },
                    "loc": { "start": { "line": 2, "column": 2 }, "end": { "line": 4, "column": 26 } }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "line": 3,
                    "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                    "loc": { "start": { "line": 3, "column": 2 }, "end": { "line": 4, "column": 26 } }
                },
                "2": {
                    "name": "(anonymous_2)",
                    "line": 4,
                    "decl": { "start": { "line": 3, "column": 2 }, "end": { "line": 3, "column": 3 } },
                    "loc": { "start": { "line": 4, "column": 4 }, "end": { "line": 4, "column": 26 } }
                }
            }),
            &serde_json::json!({ "0": 2, "1": 1, "2": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("adjust", 1, 22), Some(100.0));
        assert_eq!(file_coverage.lookup("<arrow>", 2, 2), Some(100.0));
        // The innermost arrow never ran, and takes neither neighbour's value.
        assert_eq!(file_coverage.lookup("<arrow>", 3, 2), Some(0.0));
    }

    /// A default value in a parameter list is inside the member's signature,
    /// and its own record can be anchored at the parameter rather than at the
    /// function, putting the extracted position tens of columns from the
    /// declaration. The record still owns that position, because its own
    /// signature span covers it. Geometry from an @vitest/coverage-istanbul
    /// map for a constructor whose parameter carries a default arrow.
    #[test]
    fn a_default_value_keeps_its_own_record_inside_the_enclosing_signature() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/filter.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "// geometry fixture\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 173,
                    "decl": { "start": { "line": 169, "column": 2 }, "end": { "line": 170, "column": 3 } },
                    "loc": { "start": { "line": 173, "column": 4 }, "end": { "line": 182, "column": 3 } }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "line": 171,
                    "decl": { "start": { "line": 171, "column": 21 }, "end": { "line": 171, "column": 67 } },
                    "loc": { "start": { "line": 171, "column": 67 }, "end": { "line": 171, "column": 76 } }
                }
            }),
            &serde_json::json!({ "0": 46, "1": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        // Fallow extracts the arrow at its parameter paren, 40 columns right
        // of the record's declaration but inside the record's own span.
        assert_eq!(file_coverage.lookup("<arrow>", 171, 61), Some(0.0));
    }

    /// A named function in a signature is a second function inside the
    /// member's header span, so the span identifies nothing on its own. A unit
    /// with no record of its own, such as a private member or a unit the
    /// producer names differently, must take the estimate rather than the
    /// coverage of whichever record happens to be near. Geometry from
    /// istanbul-lib-instrument 6 for the source below.
    #[test]
    fn header_span_abstains_when_another_function_is_declared_inside_it() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/chart.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export class Chart {\n  @Watch(\"data\")\n  render(\n    rows: number[],\n    project = function scale(\n      row: number\n    ) {\n      return row * 2;\n    }\n  ) {\n    return rows.map(project);\n  }\n}\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 10,
                    "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                    "loc": { "start": { "line": 10, "column": 4 }, "end": { "line": 12, "column": 3 } }
                },
                "1": {
                    "name": "scale",
                    "line": 7,
                    "decl": { "start": { "line": 5, "column": 23 }, "end": { "line": 5, "column": 28 } },
                    "loc": { "start": { "line": 7, "column": 6 }, "end": { "line": 9, "column": 5 } }
                }
            }),
            &serde_json::json!({ "0": 3, "1": 0 }),
        );

        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
        let coverage = load_istanbul_coverage_for_sources(
            &coverage_path,
            None,
            Some(temp.path()),
            Some(&discovered_sources),
            false,
        )
        .unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        // Inside both the member's header span and `scale`'s, so neither says
        // anything about this position.
        assert_eq!(file_coverage.lookup("<anonymous>", 6, 6), None);
        // Both records still resolve.
        assert_eq!(file_coverage.lookup("scale", 5, 14), Some(0.0));
        assert_eq!(file_coverage.lookup("render", 3, 8), Some(100.0));
    }

    /// A named function in a member signature can legally reuse the member's
    /// name. Name fuzz must not return that inner function before established
    /// anonymous resolution preserves the member's valid attribution.
    #[test]
    fn same_named_function_in_signature_does_not_supply_member_coverage() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/chart.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export class Chart {\n  @Watch(\"data\")\n  render(\n    rows: number[],\n    project = function render(\n      row: number\n    ) {\n      return row * 2;\n    }\n  ) {\n    return rows.map(project);\n  }\n}\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 10,
                    "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                    "loc": { "start": { "line": 10, "column": 4 }, "end": { "line": 12, "column": 3 } }
                },
                "1": {
                    "name": "render",
                    "line": 7,
                    "decl": { "start": { "line": 5, "column": 23 }, "end": { "line": 5, "column": 29 } },
                    "loc": { "start": { "line": 7, "column": 6 }, "end": { "line": 9, "column": 5 } }
                }
            }),
            &serde_json::json!({ "0": 3, "1": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("render", 3, 8), Some(100.0));
        assert_eq!(file_coverage.lookup("render", 5, 23), Some(0.0));
    }

    /// A same-line signature can place an unrelated identifier exactly one
    /// function-expression prefix away from the member start. Column distance
    /// alone must not make that nested function the member's coverage source.
    #[test]
    fn same_line_same_named_function_in_signature_does_not_supply_member_coverage() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/chart.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export class Chart {\n  @Watch(\"data\") render(x  = function  render() {}) {}\n}\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 2,
                    "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                    "loc": { "start": { "line": 2, "column": 52 }, "end": { "line": 2, "column": 54 } }
                },
                "1": {
                    "name": "render",
                    "line": 2,
                    "decl": { "start": { "line": 2, "column": 39 }, "end": { "line": 2, "column": 45 } },
                    "loc": { "start": { "line": 2, "column": 48 }, "end": { "line": 2, "column": 50 } }
                }
            }),
            &serde_json::json!({ "0": 3, "1": 0 }),
        );

        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
        let coverage = load_istanbul_coverage_for_sources(
            &coverage_path,
            None,
            Some(temp.path()),
            Some(&discovered_sources),
            false,
        )
        .unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("render", 2, 23), Some(100.0));
        assert_eq!(file_coverage.lookup("render", 2, 29), Some(0.0));
    }

    #[test]
    fn named_generator_alias_handles_trivia_and_utf16_columns() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/chart.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        let source_line =
            "  render(label = \"pi: π, mushroom: 🍄\", x = function/* gap */*render() {}) {}";
        std::fs::write(
            &source_path,
            format!("export class Chart {{\n{source_line}\n}}\n"),
        )
        .unwrap();

        let utf16_column = |byte_column: usize| {
            u32::try_from(source_line[..byte_column].encode_utf16().count()).unwrap()
        };
        let outer_target_column = source_line.find("render(").unwrap() + "render".len();
        let syntax_column = source_line.find("function").unwrap();
        let name_column = source_line.find("*render").unwrap() + 1;
        let inner_body_column = source_line.find("{}").unwrap();
        let outer_body_column = source_line.rfind("{}").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 2,
                    "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                    "loc": {
                        "start": { "line": 2, "column": utf16_column(outer_body_column) },
                        "end": { "line": 2, "column": utf16_column(outer_body_column + 2) }
                    }
                },
                "1": {
                    "name": "render",
                    "line": 2,
                    "decl": {
                        "start": { "line": 2, "column": utf16_column(name_column) },
                        "end": { "line": 2, "column": utf16_column(name_column + "render".len()) }
                    },
                    "loc": {
                        "start": { "line": 2, "column": utf16_column(inner_body_column) },
                        "end": { "line": 2, "column": utf16_column(inner_body_column + 2) }
                    }
                }
            }),
            &serde_json::json!({ "0": 3, "1": 0 }),
        );

        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
        let coverage = load_istanbul_coverage_for_sources(
            &coverage_path,
            None,
            Some(temp.path()),
            Some(&discovered_sources),
            false,
        )
        .unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(
            file_coverage.lookup("render", 2, u32::try_from(outer_target_column).unwrap()),
            Some(100.0)
        );
        assert_eq!(
            file_coverage.lookup("render", 2, u32::try_from(syntax_column).unwrap()),
            Some(0.0)
        );
    }

    #[test]
    fn utf16_index_is_sparse_and_rejects_surrogate_boundaries() {
        let ascii_prefix = "a".repeat(4_096);
        let source = format!("{ascii_prefix}🍄{}", "b".repeat(4_096));
        let index = IstanbulSourceIndex::new(&source, std::path::Path::new("minified.js"));
        let line_index = &index.non_ascii_lines[&0];

        assert_eq!(line_index.checkpoints.len(), 1);
        assert_eq!(
            index.byte_position(1, 4_096),
            Some(IstanbulPosition::new(1, 4_096))
        );
        assert_eq!(index.byte_position(1, 4_097), None);
        assert_eq!(
            index.byte_position(1, 4_098),
            Some(IstanbulPosition::new(1, 4_100))
        );
        assert_eq!(
            index.byte_position(1, 8_194),
            Some(IstanbulPosition::new(1, 8_196))
        );
    }

    #[test]
    fn effective_alias_normalizes_against_its_own_unicode_line() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/render.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "/*🍄*/ const placeholder = 0;\n/*π*/ function render() {}\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "render",
                    "line": 1,
                    "decl": { "start": { "line": 2, "column": 15 }, "end": { "line": 2, "column": 21 } },
                    "loc": { "start": { "line": 2, "column": 24 }, "end": { "line": 2, "column": 26 } }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );

        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
        let coverage = load_istanbul_coverage_for_sources(
            &coverage_path,
            None,
            Some(temp.path()),
            Some(&discovered_sources),
            false,
        )
        .unwrap();
        let function = &coverage.get(&canonical_source).unwrap().functions[0];

        assert!(
            function
                .aliases
                .iter()
                .any(|alias| { alias.position == IstanbulPosition::new(1, 17) && alias.primary })
        );
        assert!(
            !function
                .aliases
                .iter()
                .any(|alias| alias.position == IstanbulPosition::new(1, 16))
        );
    }

    #[test]
    fn stale_utf16_coordinates_are_rejected_with_trusted_source() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/render.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export function render() {}\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "render",
                    "line": 1,
                    "decl": { "start": { "line": 1, "column": 999 }, "end": { "line": 1, "column": 22 } },
                    "loc": { "start": { "line": 1, "column": 25 }, "end": { "line": 1, "column": 27 } }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );

        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
        let coverage = load_istanbul_coverage_for_sources(
            &coverage_path,
            None,
            Some(temp.path()),
            Some(&discovered_sources),
            false,
        )
        .unwrap();

        assert_eq!(
            coverage
                .get(&canonical_source)
                .unwrap()
                .lookup("render", 1, 7),
            None
        );
    }

    #[test]
    fn invalid_optional_coordinates_preserve_valid_declaration_attribution() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/render.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export function render() {}\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "render",
                    "line": 99,
                    "decl": { "start": { "line": 1, "column": 16 }, "end": { "line": 99, "column": 999 } },
                    "loc": { "start": { "line": 1, "column": 999 }, "end": { "line": 99, "column": 999 } }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );

        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
        let coverage = load_istanbul_coverage_for_sources(
            &coverage_path,
            None,
            Some(temp.path()),
            Some(&discovered_sources),
            false,
        )
        .unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();
        let function = &file_coverage.functions[0];

        assert_eq!(file_coverage.lookup("render", 1, 16), Some(100.0));
        assert!(function.body_span.is_none());
        assert!(function.header_span.is_none());
        assert!(
            !function
                .aliases
                .iter()
                .any(|alias| { alias.position.line == 99 || alias.position.col == 999 })
        );
    }

    #[test]
    fn malformed_source_does_not_supply_named_function_provenance() {
        assert!(
            !IstanbulSourceIndex::new(
                "export function render() {}",
                std::path::Path::new("valid.ts"),
            )
            .named_function_starts
            .is_empty()
        );
        let index = IstanbulSourceIndex::new(
            "export function render() {} const broken = ;",
            std::path::Path::new("broken.ts"),
        );

        assert!(index.named_function_starts.is_empty());
    }

    #[test]
    fn javascript_with_jsx_uses_clean_jsx_provenance_parse() {
        let index = IstanbulSourceIndex::new(
            "export function render() { return <div />; }",
            std::path::Path::new("component.js"),
        );

        assert!(!index.named_function_starts.is_empty());
    }

    #[test]
    fn undiscovered_coverage_path_is_not_loaded_for_source_provenance() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/excluded.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export function render() {}\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "render",
                    "line": 1,
                    "decl": { "start": { "line": 1, "column": 16 }, "end": { "line": 1, "column": 22 } },
                    "loc": { "start": { "line": 1, "column": 25 }, "end": { "line": 1, "column": 27 } }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );

        let coverage = load_istanbul_coverage_for_sources(
            &coverage_path,
            None,
            Some(temp.path()),
            Some(&rustc_hash::FxHashSet::default()),
            false,
        )
        .unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let function = &coverage.get(&canonical_source).unwrap().functions[0];

        assert!(
            function
                .aliases
                .iter()
                .all(|alias| alias.position != IstanbulPosition::new(1, 7))
        );
    }

    /// No instrumenter emits an `fnMap` identity for a private class member,
    /// so any candidate one reaches belongs to an enclosing function.
    #[test]
    fn private_class_member_never_takes_enclosing_coverage() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/vault.js");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "// geometry fixture\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 7,
                    "decl": { "start": { "line": 1, "column": 24 }, "end": { "line": 1, "column": 25 } },
                    "loc": { "start": { "line": 7, "column": 5 }, "end": { "line": 7, "column": 20 } }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        // `#wipe` sits in the arrow's header span and the arrow ran, but the
        // private member has no record of its own and never ran.
        let mut private_member = make_fn_complexity(1);
        private_member.name = "#wipe".to_string();
        private_member.is_private_member = true;
        private_member.line = 3;
        private_member.col = 9;
        let result = istanbul_crap_default(&[private_member], Some(file_coverage), false);
        assert_eq!(result.matched, 0);
        assert_eq!(
            result.per_function[0].coverage_source,
            fallow_output::CoverageSource::Estimated
        );
        assert_eq!(file_coverage.lookup("<arrow>", 1, 24), Some(100.0));
    }

    #[test]
    fn quoted_hash_method_keeps_exact_istanbul_coverage() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/vault.js");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "class Vault { '#wipe'() {} }\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "#wipe",
                    "line": 1,
                    "decl": { "start": { "line": 1, "column": 14 }, "end": { "line": 1, "column": 21 } },
                    "loc": { "start": { "line": 1, "column": 24 }, "end": { "line": 1, "column": 26 } }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        let mut quoted_public_method = make_fn_complexity(1);
        quoted_public_method.name = "#wipe".to_string();
        quoted_public_method.line = 1;
        quoted_public_method.col = 14;
        let result = istanbul_crap_default(&[quoted_public_method], Some(file_coverage), false);
        assert_eq!(result.matched, 1);
        assert_eq!(
            result.per_function[0].coverage_source,
            fallow_output::CoverageSource::Istanbul
        );
        assert_eq!(result.per_function[0].coverage_pct, Some(100.0));
    }

    /// A decorated member's `decl` opens at the decorator and its `loc` opens
    /// at the body brace, so the extracted position sits between them with no
    /// alias in reach: the decorator is more than
    /// `ANONYMOUS_FALLBACK_MAX_COLUMN_DRIFT` columns to the left and the body
    /// is more than `ALIAS_FUZZ_MAX_LINE_DRIFT` lines below. The header span
    /// is what identifies the member. Geometry from istanbul-lib-instrument 6
    /// for the source below, which is ordinary NestJS, Angular, and TypeORM
    /// shape rather than a corner case.
    #[test]
    fn decorated_member_matches_its_istanbul_header_span() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/users.controller.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export class UserController {\n  @Get(\":id\")\n  async findOneWithProfile(\n    id: string,\n    include: string[]\n  ) {\n    return this.service.find(id, include);\n  }\n}\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 6,
                    "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                    "loc": { "start": { "line": 6, "column": 4 }, "end": { "line": 8, "column": 3 } }
                }
            }),
            &serde_json::json!({ "0": 3 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        // Fallow extracts the member at the parameter paren, 3:26.
        assert_eq!(
            file_coverage.lookup("findOneWithProfile", 3, 26),
            Some(100.0)
        );
    }

    /// A function written in a signature has a record of its own, and the
    /// signature it sits in belongs to a different function. The record wins:
    /// crediting the enclosing member would report the member's coverage for
    /// a default value that never ran. Geometry from istanbul-lib-instrument 6
    /// for the source below.
    #[test]
    fn established_alias_wins_over_the_signature_that_contains_it() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/chart.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export class Chart {\n  @Watch(\"data\")\n  render(\n    rows: number[],\n    project = (row: number) => row * 2\n  ) {\n    return rows.map(project);\n  }\n}\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 6,
                    "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                    "loc": { "start": { "line": 6, "column": 4 }, "end": { "line": 8, "column": 3 } }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "line": 5,
                    "decl": { "start": { "line": 5, "column": 14 }, "end": { "line": 5, "column": 15 } },
                    "loc": { "start": { "line": 5, "column": 31 }, "end": { "line": 5, "column": 38 } }
                }
            }),
            &serde_json::json!({ "0": 3, "1": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        // The default value takes its own record, not the method's 100.
        assert_eq!(file_coverage.lookup("<arrow>", 5, 14), Some(0.0));
        // The method still resolves, and not to its default value.
        assert_eq!(file_coverage.lookup("render", 3, 8), Some(100.0));
    }

    /// A member whose signature holds a function of its own has two records a
    /// line or two apart, and the member's extracted position is closer to
    /// the inner one. Proximity would report the default value's coverage for
    /// the member, and the header span cannot break the tie because it
    /// contains both. Abstaining leaves the static estimate, which is wrong by
    /// a known amount rather than wrong while claiming to be measured.
    /// Geometry from istanbul-lib-instrument 6 for the source below.
    #[test]
    fn signature_holding_a_function_abstains_instead_of_crediting_it() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/users.controller.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export class UserController {\n  @Get(\":id\")\n  async findOneWithProfile(\n    id: string,\n    transform = (row: string) => row.trim()\n  ) {\n    return transform(id);\n  }\n}\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 6,
                    "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                    "loc": { "start": { "line": 6, "column": 4 }, "end": { "line": 8, "column": 3 } }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "line": 5,
                    "decl": { "start": { "line": 5, "column": 16 }, "end": { "line": 5, "column": 17 } },
                    "loc": { "start": { "line": 5, "column": 33 }, "end": { "line": 5, "column": 43 } }
                }
            }),
            &serde_json::json!({ "0": 3, "1": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        // The member is two lines from the default value's declaration and
        // ten columns off it, well inside the fallback's reach.
        assert_eq!(file_coverage.lookup("findOneWithProfile", 3, 26), None);
        // The default value itself still resolves.
        assert_eq!(file_coverage.lookup("<arrow>", 5, 16), Some(0.0));
    }

    #[test]
    fn anonymous_record_aliases_do_not_tie_with_their_own_identity() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/aliases.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export const handler = () => true;\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 10,
                    "decl": {
                        "start": { "line": 10, "column": 8 },
                        "end": { "line": 10, "column": 9 }
                    },
                    "loc": {
                        "start": { "line": 12, "column": 8 },
                        "end": { "line": 13, "column": 1 }
                    }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("handler", 11, 8), Some(100.0));
    }

    /// istanbul-lib-instrument geometry for
    /// `export const nested = () => () => true;`: the outer arrow's `loc` is
    /// its expression body, which starts exactly where the inner arrow is
    /// declared.
    #[test]
    fn curried_arrow_one_liner_resolves_both_arrows() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/nested.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export const nested = () => () => true;\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 1,
                    "decl": {
                        "start": { "line": 1, "column": 22 },
                        "end": { "line": 1, "column": 23 }
                    },
                    "loc": {
                        "start": { "line": 1, "column": 28 },
                        "end": { "line": 1, "column": 38 }
                    }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "line": 1,
                    "decl": {
                        "start": { "line": 1, "column": 28 },
                        "end": { "line": 1, "column": 29 }
                    },
                    "loc": {
                        "start": { "line": 1, "column": 34 },
                        "end": { "line": 1, "column": 38 }
                    }
                }
            }),
            &serde_json::json!({ "0": 1, "1": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("nested", 1, 22), Some(100.0));
        assert_eq!(file_coverage.lookup("<arrow>", 1, 28), Some(0.0));
    }

    /// istanbul-lib-instrument geometry for a multi-line higher-order
    /// component. The producer's `line` is the body start line, so the outer
    /// record also carries an effective alias on line 2.
    #[test]
    fn curried_arrow_multiline_hoc_resolves_both_arrows() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/with-auth.tsx");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export const withAuth = (Component) =>\n  (props) => {\n    return Component(props);\n  };\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 2,
                    "decl": {
                        "start": { "line": 1, "column": 24 },
                        "end": { "line": 1, "column": 25 }
                    },
                    "loc": {
                        "start": { "line": 2, "column": 2 },
                        "end": { "line": 4, "column": 3 }
                    }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "line": 2,
                    "decl": {
                        "start": { "line": 2, "column": 2 },
                        "end": { "line": 2, "column": 3 }
                    },
                    "loc": {
                        "start": { "line": 2, "column": 13 },
                        "end": { "line": 4, "column": 3 }
                    }
                }
            }),
            &serde_json::json!({ "0": 1, "1": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("withAuth", 1, 24), Some(100.0));
        assert_eq!(file_coverage.lookup("<arrow>", 2, 2), Some(0.0));
    }

    /// istanbul-lib-instrument geometry for a depth-3 redux middleware chain.
    /// Every non-first arrow is declared where the previous body starts.
    #[test]
    fn curried_arrow_depth_three_chain_resolves_every_arrow() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/logger.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export const logger = (store) => (next) => (action) => {\n  return next(action);\n};\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 1,
                    "decl": {
                        "start": { "line": 1, "column": 22 },
                        "end": { "line": 1, "column": 23 }
                    },
                    "loc": {
                        "start": { "line": 1, "column": 33 },
                        "end": { "line": 3, "column": 1 }
                    }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "line": 1,
                    "decl": {
                        "start": { "line": 1, "column": 33 },
                        "end": { "line": 1, "column": 34 }
                    },
                    "loc": {
                        "start": { "line": 1, "column": 43 },
                        "end": { "line": 3, "column": 1 }
                    }
                },
                "2": {
                    "name": "(anonymous_2)",
                    "line": 1,
                    "decl": {
                        "start": { "line": 1, "column": 43 },
                        "end": { "line": 1, "column": 44 }
                    },
                    "loc": {
                        "start": { "line": 1, "column": 55 },
                        "end": { "line": 3, "column": 1 }
                    }
                }
            }),
            &serde_json::json!({ "0": 0, "1": 1, "2": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("logger", 1, 22), Some(0.0));
        assert_eq!(file_coverage.lookup("<arrow>", 1, 33), Some(100.0));
        assert_eq!(file_coverage.lookup("<arrow>", 1, 43), Some(0.0));
    }

    /// istanbul-lib-instrument geometry for a curried class-property arrow.
    #[test]
    fn curried_class_property_arrow_resolves_both_arrows() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/store.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export class Store {\n  handle = (event) => (payload) => {\n    return payload;\n  };\n}\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 2,
                    "decl": {
                        "start": { "line": 2, "column": 11 },
                        "end": { "line": 2, "column": 12 }
                    },
                    "loc": {
                        "start": { "line": 2, "column": 22 },
                        "end": { "line": 4, "column": 3 }
                    }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "line": 2,
                    "decl": {
                        "start": { "line": 2, "column": 22 },
                        "end": { "line": 2, "column": 23 }
                    },
                    "loc": {
                        "start": { "line": 2, "column": 35 },
                        "end": { "line": 4, "column": 3 }
                    }
                }
            }),
            &serde_json::json!({ "0": 1, "1": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("handle", 2, 11), Some(100.0));
        assert_eq!(file_coverage.lookup("<arrow>", 2, 22), Some(0.0));
    }

    /// istanbul-lib-instrument geometry for two sibling arrows in an object
    /// literal. A target one line away from both, at the shared column, ties
    /// on distance and lies in neither body, so the lookup abstains.
    #[test]
    fn anonymous_sibling_tie_outside_every_body_abstains() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/handlers.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export const handlers = {\n  a: () => true,\n\n  b: () => false,\n};\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 2,
                    "decl": {
                        "start": { "line": 2, "column": 5 },
                        "end": { "line": 2, "column": 6 }
                    },
                    "loc": {
                        "start": { "line": 2, "column": 11 },
                        "end": { "line": 2, "column": 15 }
                    }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "line": 4,
                    "decl": {
                        "start": { "line": 4, "column": 5 },
                        "end": { "line": 4, "column": 6 }
                    },
                    "loc": {
                        "start": { "line": 4, "column": 11 },
                        "end": { "line": 4, "column": 16 }
                    }
                }
            }),
            &serde_json::json!({ "0": 1, "1": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("a", 2, 5), Some(100.0));
        assert_eq!(file_coverage.lookup("b", 4, 5), Some(0.0));
        assert!(file_coverage.lookup("<arrow>", 3, 5).is_none());
    }

    /// istanbul-lib-instrument geometry for a function expression whose block
    /// body wraps an arrow, with the closing braces on a second line. A target
    /// on that line, equidistant from the outer `{` and the inner declaration,
    /// ties on distance and lies inside both bodies; the strictly innermost
    /// body wins.
    #[test]
    fn anonymous_tie_selects_unique_strictly_innermost_containing_span() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/nested.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export const o = function () { const f = () => { return 1;\n                                       }; return f; };\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 1,
                    "decl": {
                        "start": { "line": 1, "column": 17 },
                        "end": { "line": 1, "column": 18 }
                    },
                    "loc": {
                        "start": { "line": 1, "column": 29 },
                        "end": { "line": 2, "column": 53 }
                    }
                },
                "1": {
                    "name": "(anonymous_1)",
                    "line": 1,
                    "decl": {
                        "start": { "line": 1, "column": 41 },
                        "end": { "line": 1, "column": 42 }
                    },
                    "loc": {
                        "start": { "line": 1, "column": 47 },
                        "end": { "line": 2, "column": 40 }
                    }
                }
            }),
            &serde_json::json!({ "0": 1, "1": 0 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("<arrow>", 2, 35), Some(0.0));
    }

    /// Defensive: no producer emits partially overlapping bodies, but a tie
    /// between two containing spans that are not nested must not pick either.
    #[test]
    fn anonymous_tie_rejects_incomparable_containing_spans() {
        let file_coverage = IstanbulFileCoverage::new(
            vec![
                IstanbulFunctionCoverage {
                    name: "(anonymous_0)".to_string(),
                    coverage_pct: 100.0,
                    aliases: vec![primary_alias(10, 8), secondary_alias(10, 14)],
                    decl_start: IstanbulPosition::new(10, 8),
                    header_holds_other_fn: false,
                    header_span: None,
                    body_span: Some(body_span((10, 14), (12, 30))),
                },
                IstanbulFunctionCoverage {
                    name: "(anonymous_1)".to_string(),
                    coverage_pct: 0.0,
                    aliases: vec![primary_alias(10, 20), secondary_alias(11, 0)],
                    decl_start: IstanbulPosition::new(10, 20),
                    header_holds_other_fn: false,
                    header_span: None,
                    body_span: Some(body_span((11, 0), (14, 0))),
                },
            ],
            false,
        );

        assert!(file_coverage.lookup("<arrow>", 12, 17).is_none());
    }

    /// Two records whose primary aliases coincide (a multi-line parameter
    /// list whose body-start line and declaration column produce the same
    /// effective position as the inner arrow's declaration) stay ambiguous
    /// even though their bodies nest.
    #[test]
    fn anonymous_shared_primary_alias_rejects_even_nested_spans() {
        let file_coverage = IstanbulFileCoverage::new(
            vec![
                IstanbulFunctionCoverage {
                    name: "(anonymous_0)".to_string(),
                    coverage_pct: 100.0,
                    aliases: vec![primary_alias(4, 11), primary_alias(1, 11)],
                    decl_start: IstanbulPosition::new(4, 11),
                    header_holds_other_fn: false,
                    header_span: None,
                    body_span: Some(body_span((4, 11), (4, 23))),
                },
                IstanbulFunctionCoverage {
                    name: "(anonymous_1)".to_string(),
                    coverage_pct: 0.0,
                    aliases: vec![primary_alias(4, 11), secondary_alias(4, 18)],
                    decl_start: IstanbulPosition::new(4, 11),
                    header_holds_other_fn: false,
                    header_span: None,
                    body_span: Some(body_span((4, 18), (4, 23))),
                },
            ],
            false,
        );

        assert!(file_coverage.lookup("<arrow>", 4, 11).is_none());
        assert_eq!(file_coverage.lookup("aa", 1, 11), Some(100.0));
    }

    #[test]
    fn colliding_anonymous_alias_uses_unique_safe_header_span() {
        let file_coverage = IstanbulFileCoverage::new(
            vec![
                IstanbulFunctionCoverage {
                    name: "(anonymous_0)".to_string(),
                    coverage_pct: 100.0,
                    aliases: vec![primary_alias(1, 0), secondary_alias(3, 4)],
                    decl_start: IstanbulPosition::new(1, 0),
                    header_holds_other_fn: false,
                    header_span: Some(body_span((1, 0), (5, 0))),
                    body_span: Some(body_span((5, 0), (8, 0))),
                },
                IstanbulFunctionCoverage {
                    name: "(anonymous_1)".to_string(),
                    coverage_pct: 0.0,
                    aliases: vec![primary_alias(10, 0), secondary_alias(3, 4)],
                    decl_start: IstanbulPosition::new(10, 0),
                    header_holds_other_fn: false,
                    header_span: None,
                    body_span: Some(body_span((10, 0), (12, 0))),
                },
            ],
            false,
        );

        assert_eq!(file_coverage.lookup("<arrow>", 3, 4), Some(100.0));
    }

    /// A secondary alias that collides with another record's secondary alias
    /// is dropped from both, and the shared position remains ambiguous.
    #[test]
    fn colliding_secondary_aliases_abstain_at_shared_position() {
        let file_coverage = IstanbulFileCoverage::new(
            vec![
                IstanbulFunctionCoverage {
                    name: "(anonymous_0)".to_string(),
                    coverage_pct: 100.0,
                    aliases: vec![primary_alias(10, 0), secondary_alias(12, 4)],
                    decl_start: IstanbulPosition::new(10, 0),
                    header_holds_other_fn: false,
                    header_span: None,
                    body_span: Some(body_span((12, 4), (20, 0))),
                },
                IstanbulFunctionCoverage {
                    name: "(anonymous_1)".to_string(),
                    coverage_pct: 0.0,
                    aliases: vec![primary_alias(11, 0), secondary_alias(12, 4)],
                    decl_start: IstanbulPosition::new(11, 0),
                    header_holds_other_fn: false,
                    header_span: None,
                    body_span: Some(body_span((12, 4), (18, 0))),
                },
            ],
            false,
        );

        assert_eq!(file_coverage.lookup("first", 10, 0), Some(100.0));
        assert_eq!(file_coverage.lookup("second", 11, 0), Some(0.0));
        assert!(file_coverage.lookup("<arrow>", 12, 4).is_none());
    }

    #[test]
    fn colliding_named_secondary_aliases_abstain_at_shared_position() {
        let file_coverage = IstanbulFileCoverage::new(
            vec![
                IstanbulFunctionCoverage {
                    name: "handler".to_string(),
                    coverage_pct: 100.0,
                    aliases: vec![primary_alias(10, 0), secondary_alias(12, 4)],
                    decl_start: IstanbulPosition::new(10, 0),
                    header_holds_other_fn: false,
                    header_span: None,
                    body_span: Some(body_span((12, 4), (20, 0))),
                },
                IstanbulFunctionCoverage {
                    name: "handler".to_string(),
                    coverage_pct: 0.0,
                    aliases: vec![primary_alias(11, 0), secondary_alias(12, 4)],
                    decl_start: IstanbulPosition::new(11, 0),
                    header_holds_other_fn: false,
                    header_span: None,
                    body_span: Some(body_span((12, 4), (18, 0))),
                },
            ],
            false,
        );

        assert_eq!(file_coverage.lookup("handler", 10, 0), Some(100.0));
        assert_eq!(file_coverage.lookup("handler", 11, 0), Some(0.0));
        assert!(file_coverage.lookup("handler", 12, 4).is_none());
    }

    #[test]
    fn invalid_body_location_does_not_create_an_alias() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/invalid-location.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "export const handler = () => true;\n").unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 8,
                    "decl": {
                        "start": { "line": 8, "column": 14 },
                        "end": { "line": 8, "column": 25 }
                    },
                    "loc": {
                        "start": { "line": 22, "column": 1 },
                        "end": { "line": 20, "column": 6 }
                    }
                }
            }),
            &serde_json::json!({ "0": 1 }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert!(file_coverage.lookup("handler", 22, 1).is_none());
    }

    #[test]
    fn load_istanbul_coverage_matches_multiline_async_arrow_decl_alias() {
        let temp = tempfile::TempDir::new().unwrap();
        let source_path = temp.path().join("src/actor.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export const elementsFrom = async (\n  locator: AnyLocator,\n  options?: { missingAsEmpty?: boolean },\n): Promise<HTMLElement[]> => {\n  return [];\n};\n",
        )
        .unwrap();

        let coverage_path = temp.path().join("coverage-final.json");
        write_single_file_istanbul_fixture(
            &coverage_path,
            &source_path,
            &serde_json::json!({
                "0": {
                    "name": "(anonymous_0)",
                    "line": 4,
                    "decl": {
                        "start": { "line": 1, "column": 28 },
                        "end": { "line": 4, "column": 26 }
                    },
                    "loc": {
                        "start": { "line": 4, "column": 27 },
                        "end": { "line": 6, "column": 1 }
                    }
                }
            }),
            &serde_json::json!({
                "0": 642
            }),
        );

        let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("elementsFrom", 1, 28), Some(100.0));
    }

    #[test]
    fn istanbul_lookup_exact_match() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 10, 0), 85.0);
        let fc = test_istanbul_file_coverage(functions, false);
        assert!((fc.lookup("handleClick", 10, 0).unwrap() - 85.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_fuzzy_match_within_offset() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 10, 0), 72.0);
        let fc = test_istanbul_file_coverage(functions, false);
        assert!((fc.lookup("handleClick", 11, 0).unwrap() - 72.0).abs() < f64::EPSILON);
        assert!((fc.lookup("handleClick", 12, 0).unwrap() - 72.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_fuzzy_match_outside_offset() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 10, 0), 72.0);
        let fc = test_istanbul_file_coverage(functions, false);
        assert!(fc.lookup("handleClick", 13, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_relocated_matches_unique_name_at_any_distance() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 29, 0), 72.0);
        let fc = test_istanbul_file_coverage(functions, true);
        assert!((fc.lookup("handleClick", 10, 0).unwrap() - 72.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_relocated_accepts_declaration_alias_pair() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 29, 16), 72.0);
        functions.insert(("handleClick".to_string(), 29, 0), 72.0);
        let fc = test_istanbul_file_coverage(functions, true);
        assert!((fc.lookup("handleClick", 10, 0).unwrap() - 72.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_relocated_bails_on_disagreeing_same_name_entries() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("render".to_string(), 29, 0), 72.0);
        functions.insert(("render".to_string(), 80, 0), 10.0);
        let fc = test_istanbul_file_coverage(functions, true);
        assert!(fc.lookup("render", 10, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_relocated_prefers_bounded_fuzzy_match() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("render".to_string(), 11, 0), 72.0);
        functions.insert(("render".to_string(), 80, 0), 10.0);
        let fc = test_istanbul_file_coverage(functions, true);
        assert!((fc.lookup("render", 10, 0).unwrap() - 72.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_name_mismatch() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 10, 0), 85.0);
        let fc = test_istanbul_file_coverage(functions, false);
        assert!(fc.lookup("handleSubmit", 10, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_empty() {
        let fc = test_istanbul_file_coverage(rustc_hash::FxHashMap::default(), false);
        assert!(fc.lookup("anything", 1, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_fuzzy_picks_closest() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("render".to_string(), 8, 0), 60.0);
        functions.insert(("render".to_string(), 12, 0), 90.0);
        let fc = test_istanbul_file_coverage(functions, false);
        let result = fc.lookup("render", 10, 0);
        assert!(result.is_some());
        let pct = result.unwrap();
        assert!((pct - 60.0).abs() < f64::EPSILON || (pct - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_single_candidate() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 28, 0), 75.0);
        let fc = test_istanbul_file_coverage(functions, false);
        assert!((fc.lookup("myHandler", 28, 0).unwrap() - 75.0).abs() < f64::EPSILON);
        assert!((fc.lookup("myHandler", 30, 0).unwrap() - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_rejects_nearby_far_column() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 4, 28), 75.0);
        let fc = test_istanbul_file_coverage(functions, false);

        assert!(fc.lookup("declaredHelper", 3, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_picks_closest_when_lines_differ() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 28, 0), 75.0);
        functions.insert(("(anonymous_1)".to_string(), 29, 0), 50.0);
        let fc = test_istanbul_file_coverage(functions, false);
        assert!((fc.lookup("myHandler", 28, 0).unwrap() - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_picks_closest_by_col_on_same_line() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 1, 23), 90.0); // outer
        functions.insert(("(anonymous_1)".to_string(), 1, 43), 10.0); // inner
        let fc = test_istanbul_file_coverage(functions, false);
        assert!((fc.lookup("<arrow>", 1, 43).unwrap() - 10.0).abs() < f64::EPSILON);
        assert!((fc.lookup("<arrow>", 1, 23).unwrap() - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_bails_only_on_true_tie() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 27, 0), 75.0);
        functions.insert(("(anonymous_1)".to_string(), 29, 0), 50.0);
        let fc = test_istanbul_file_coverage(functions, false);
        assert!(fc.lookup("myHandler", 28, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_outside_offset() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 28, 0), 75.0);
        let fc = test_istanbul_file_coverage(functions, false);
        assert!(fc.lookup("myHandler", 31, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_named_match_beats_nearby_anonymous() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 10, 0), 90.0);
        functions.insert(("(anonymous_7)".to_string(), 11, 0), 10.0);
        let fc = test_istanbul_file_coverage(functions, false);
        assert!((fc.lookup("handleClick", 10, 0).unwrap() - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn build_test_refs_empty() {
        let exports: Vec<fallow_graph::graph::ExportSymbol> = vec![];
        let graph = fallow_graph::graph::ModuleGraph::build(&[], &[], &[]);
        let refs = build_test_referenced_exports(&exports, StaticTestCoverage::new(&graph));
        assert!(refs.is_empty());
    }

    #[test]
    fn build_test_refs_empty_inputs() {
        let exports: Vec<fallow_graph::graph::ExportSymbol> = vec![];
        let graph = fallow_graph::graph::ModuleGraph::build(&[], &[], &[]);
        let refs = build_test_referenced_exports(&exports, StaticTestCoverage::new(&graph));
        assert!(refs.is_empty());
    }

    #[test]
    fn istanbul_crap_empty_complexity() {
        let result = istanbul_crap_default(&[], None, false);
        assert!((result.max_crap).abs() < f64::EPSILON);
        assert_eq!(result.signals.above, 0);
        assert_eq!(result.matched, 0);
        assert_eq!(result.total, 0);
    }

    #[test]
    fn istanbul_crap_match_statistics() {
        let funcs = vec![make_fn_complexity(5), {
            let mut f = make_fn_complexity(3);
            f.name = "other_fn".into();
            f.line = 10;
            f
        }];
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("test_fn".to_string(), 1, 0), 80.0);
        let file_cov = test_istanbul_file_coverage(functions, false);
        let result = istanbul_crap_default(&funcs, Some(&file_cov), true);
        assert_eq!(result.matched, 1);
        assert_eq!(result.total, 2);
    }

    #[test]
    fn estimated_crap_multiple_functions_mixed_coverage() {
        let funcs = vec![
            make_fn_complexity(10), // name "test_fn" line 1
            {
                let mut f = make_fn_complexity(3);
                f.name = "helper".into();
                f.line = 20;
                f
            },
        ];
        let mut refs = rustc_hash::FxHashSet::default();
        refs.insert("test_fn".to_string());
        let result = estimated_crap_default(
            &funcs,
            &refs,
            true,
            fallow_output::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.signals.above);
        assert!(max > 10.0);
        assert_eq!(above, 0);
    }

    #[test]
    fn binary_crap_test_reachable() {
        let funcs = vec![make_fn_complexity(10)];
        let (max, above) = compute_crap_scores_binary(&funcs, true);
        assert!((max - 10.0).abs() < f64::EPSILON);
        assert_eq!(above, 0);
    }

    #[test]
    fn binary_crap_not_reachable() {
        let funcs = vec![make_fn_complexity(6)];
        let (max, above) = compute_crap_scores_binary(&funcs, false);
        assert!((max - 42.0).abs() < f64::EPSILON);
        assert_eq!(above, 1);
    }

    #[test]
    fn binary_crap_threshold_boundary() {
        let funcs = vec![make_fn_complexity(5)];
        let (max, above) = compute_crap_scores_binary(&funcs, false);
        assert!((max - 30.0).abs() < f64::EPSILON);
        assert_eq!(above, 1);
    }

    #[test]
    fn binary_crap_empty() {
        let (max, above) = compute_crap_scores_binary(&[], true);
        assert!((max).abs() < f64::EPSILON);
        assert_eq!(above, 0);
    }

    #[test]
    fn binary_crap_multiple_functions() {
        let funcs = vec![make_fn_complexity(3), make_fn_complexity(8)];
        let (max, above) = compute_crap_scores_binary(&funcs, false);
        assert!((max - 72.0).abs() < f64::EPSILON);
        assert_eq!(above, 1);
    }
}
