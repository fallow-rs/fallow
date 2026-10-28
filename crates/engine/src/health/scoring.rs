use fallow_output::{DirectCallerEvidence, DirectCallerSymbolEvidence, FileHealthScore};

use crate::module_graph::StaticTestCoverage;

use super::coverage_gaps::compute_coverage_gaps;
pub(super) use super::coverage_gaps::{CoverageGapData, build_coverage_summary};
use super::test_reach::build_test_referenced_exports;
use super::threshold_overrides::ThresholdOverrideResolver;

const MAX_FUNCTION_EVIDENCE: usize = 3;

/// Output from `compute_file_scores`, including auxiliary data for refactoring targets.
pub struct FileScoreOutput {
    pub(crate) scores: Vec<FileHealthScore>,
    /// Static coverage gaps derived from runtime-vs-test reachability.
    pub(crate) coverage: CoverageGapData,
    /// Files participating in circular dependencies (absolute paths).
    pub(crate) circular_files: rustc_hash::FxHashSet<std::path::PathBuf>,
    /// Top 3 functions by cognitive complexity per file (name, line, cognitive score).
    pub(crate) top_complex_fns: rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>>,
    /// Eligible CRAP breaches, ranked and capped separately from structural evidence.
    pub(crate) crap_target_fns: rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>>,
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
    /// Input format of the coverage map. `None` without a map.
    pub(crate) coverage_input_format: Option<fallow_output::CoverageInputFormat>,
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
    /// Static edges between analyzed files. Change coupling reads it to tell a
    /// hidden co-change pair from a pair that an import already explains.
    /// `None` when the run reads no commit history.
    pub(crate) static_links: Option<crate::change_coupling::StaticLinkIndex>,
}

struct FileScoreOutputParts<'a> {
    graph: &'a fallow_graph::graph::ModuleGraph,
    with_static_links: bool,
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    results: &'a crate::results::AnalysisResults,
    scores: Vec<FileHealthScore>,
    coverage: CoverageGapData,
    circular_files: rustc_hash::FxHashSet<std::path::PathBuf>,
    top_complex_fns: rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>>,
    crap_target_fns: rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>>,
    entry_points: rustc_hash::FxHashSet<std::path::PathBuf>,
    value_export_counts: rustc_hash::FxHashMap<std::path::PathBuf, usize>,
    unused_export_names: rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
    cycle_members: rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
    direct_callers: rustc_hash::FxHashMap<std::path::PathBuf, Vec<DirectCallerEvidence>>,
    istanbul_matched: usize,
    istanbul_total: usize,
    istanbul_files_joined: usize,
    istanbul_files_total: usize,
    coverage_input_format: Option<fallow_output::CoverageInputFormat>,
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
///
/// Every unit counts, including the synthetic `<module>` unit that carries the
/// file's module-scope decision points. `function_count` therefore counts a
/// non-function unit on files that branch outside every function, which is the
/// point: complexity density measured against a total that skipped module
/// scope understated the file.
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
    suppressions: &'a [crate::suppress::Suppression],
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
    suppressed: usize,
    unsuppressed_max: f64,
    unsuppressed_min_ceiling: Option<f64>,
    /// Unsuppressed effective-ceiling breaches, before evidence sorting/capping.
    eligible_functions: Vec<(String, u32, u16)>,
}

impl<'a> CrapCeilingLookup<'a> {
    pub(super) fn new(
        thresholds: CrapScoreThresholds<'a>,
        relative: &'a std::path::Path,
        suppressions: &'a [crate::suppress::Suppression],
    ) -> Self {
        Self {
            resolver: thresholds.resolver,
            relative,
            enforce_crap: thresholds.enforce_crap,
            suppressions,
        }
    }

    /// Fold one function's rounded CRAP value into the file's
    /// threshold-relative signals.
    fn observe(
        &self,
        function: &fallow_types::extract::FunctionComplexity,
        crap_rounded: f64,
        signals: &mut CrapThresholdSignals,
    ) {
        let ceiling = self
            .resolver
            .effective_max_crap(self.relative, &function.name);
        signals.min_ceiling = Some(signals.min_ceiling.map_or(ceiling, |m| m.min(ceiling)));
        if (!self.enforce_crap || crap_rounded < ceiling) && crap_rounded >= CRAP_THRESHOLD {
            signals.exempted += 1;
        }
        if crate::suppress::is_suppressed(
            self.suppressions,
            function.line,
            crate::suppress::IssueKind::Complexity,
        ) {
            signals.suppressed += 1;
            return;
        }
        signals.unsuppressed_max = signals.unsuppressed_max.max(crap_rounded);
        signals.unsuppressed_min_ceiling = Some(
            signals
                .unsuppressed_min_ceiling
                .map_or(ceiling, |minimum| minimum.min(ceiling)),
        );
        if self.enforce_crap && crap_rounded >= ceiling {
            signals.above += 1;
            signals.eligible_functions.push((
                function.name.clone(),
                function.line,
                function.cognitive,
            ));
        }
    }
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
        // above-threshold contribution, no match-statistics slot. The
        // module-scope unit leaves for the same reason plus one more: it has no
        // fnMap identity to match, so counting it would score every branching
        // file as one more uncovered high-CRAP unit and drag the reported
        // Istanbul match rate down.
        if fallow_types::extract::is_synthetic_template_unit(&f.name)
            || fallow_types::extract::is_synthetic_module_unit(&f.name)
        {
            continue;
        }
        total += 1;
        let cc = f64::from(ceilings.resolver.gate_cyclomatic(f));
        let (crap, coverage_pct, tier, source) =
            crap_for_function(f, cc, file_coverage, is_test_reachable, &mut matched);
        let crap_rounded = (crap * 10.0).round() / 10.0;
        max = max.max(crap);
        ceilings.observe(f, crap_rounded, &mut signals);
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
    cc: f64,
    file_coverage: Option<&IstanbulFileCoverage>,
    is_test_reachable: bool,
    matched: &mut usize,
) -> (
    f64,
    Option<f64>,
    fallow_output::CoverageTier,
    fallow_output::CoverageSource,
) {
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
        // The module-scope unit is not an export either, so it leaves too.
        if fallow_types::extract::is_synthetic_template_unit(&f.name)
            || fallow_types::extract::is_synthetic_module_unit(&f.name)
        {
            continue;
        }
        let cc = f64::from(ceilings.resolver.gate_cyclomatic(f));
        // A context name is not an export name, also when it repeats the name
        // of the export that returns the function (`const load = () =>
        // Effect.gen(...)`), so the lookup uses the identity name.
        let estimated_coverage = if test_referenced_exports.contains(f.identity_name()) {
            DIRECT_TEST_COVERAGE_ESTIMATE
        } else if is_test_reachable {
            INDIRECT_TEST_COVERAGE_ESTIMATE
        } else {
            0.0
        };
        let crap = crap_formula(cc, estimated_coverage);
        let crap_rounded = (crap * 10.0).round() / 10.0;
        max = max.max(crap);
        ceilings.observe(f, crap_rounded, &mut signals);
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
            let refs = build_test_referenced_exports(
                owner_node.file_id,
                &owner_node.exports,
                test_coverage,
            );
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
    /// Whether this record answers to `name`, either as recorded or as the
    /// property behind an accessor keyword the producer kept.
    fn answers_to(&self, name: &str) -> bool {
        if self.name == name {
            return true;
        }
        accessor_property_name(&self.name) == Some(name)
    }

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
            return self.lookup_private(&function.name, function.line, function.col);
        }
        // A context name (`Command.make("setup")`, the name of the function
        // that returns `Effect.gen(...)`) is not the name that a coverage
        // producer records. It could match the record of the wrapper function
        // on the same line, so the lookup uses the placeholder.
        self.lookup(function.identity_name(), function.line, function.col)
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
                // An accessor answers to its property name as well, without
                // giving up the spelling the producer recorded.
                if let Some(property) = accessor_property_name(&function.name) {
                    alias_index
                        .entry((
                            property.to_string(),
                            alias.position.line,
                            alias.position.col,
                        ))
                        .or_insert(function_index);
                }
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

    /// Coverage for a private class member, from a record with its own
    /// `#name` only.
    ///
    /// The anonymous, header-span, and relocated fallbacks of [`Self::lookup`]
    /// match on proximity, and the nearest record to a private member is often
    /// the enclosing function or a public twin. The named step stays, but it
    /// must find exactly one record: two `#name` records near the member (for
    /// example a private getter and setter) give no coverage (#3816).
    fn lookup_private(&self, name: &str, line: u32, col: u32) -> Option<f64> {
        let exact_key = (name.to_string(), line, col);
        if self.ambiguous_aliases.contains(&exact_key) {
            return None;
        }
        if let Some(&function_index) = self.alias_index.get(&exact_key) {
            return Some(self.functions[function_index].coverage_pct);
        }
        let target = IstanbulPosition::new(line, col);
        let window = self.fuzz_window(target);
        let signature_owners = self.header_spans_containing(target);
        let mut candidates = self.named_candidates(name, target, &window, &signature_owners);
        let (_, function_index) = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        Some(self.functions[function_index].coverage_pct)
    }

    /// Records in `window` named `name`, with their alias distance from
    /// `target`.
    ///
    /// A function written inside another function's signature is a
    /// different function from the one the signature belongs to, and both
    /// sit within a line or two of the same position. Proximity cannot
    /// tell them apart, so `signature_owners` (the records whose signature
    /// covers `target`) decide which candidates are eligible at all.
    fn named_candidates<'a>(
        &'a self,
        name: &'a str,
        target: IstanbulPosition,
        window: &'a [usize],
        signature_owners: &'a [usize],
    ) -> impl Iterator<Item = ((u32, u32), usize)> + 'a {
        window
            .iter()
            .copied()
            .filter(move |function_index| self.functions[*function_index].answers_to(name))
            .filter_map(move |function_index| {
                self.functions[function_index]
                    .nearest_alias(target, None)
                    .map(|distance| (distance, function_index))
            })
            .filter(move |(distance, function_index)| {
                *distance == (0, 0)
                    || !self.foreign_signature_blocks(signature_owners, *function_index, target)
            })
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
        let signature_owners = self.header_spans_containing(target);
        if let Some(function) = self
            .named_candidates(name, target, &window, &signature_owners)
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
            if !function.answers_to(name) {
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
    format: fallow_output::CoverageInputFormat,
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

    /// The input format the coverage was read from.
    pub const fn format(&self) -> fallow_output::CoverageInputFormat {
        self.format
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
    match read_coverage_input(&resolved)? {
        CoverageInput::Istanbul { file_path, json } => {
            let raw = parse_coverage_map_tolerantly(&json).map_err(|e| {
                format!(
                    "failed to parse coverage data from {}: {e}",
                    file_path.display()
                )
            })?;
            let sources = CoverageMapSources {
                coverage_root,
                project_root,
                discovered_sources,
                relocated,
            };
            Ok(build_istanbul_coverage(
                &raw,
                &sources,
                fallow_output::CoverageInputFormat::Istanbul,
            ))
        }
        CoverageInput::V8 { dump_files } => {
            let scope = super::coverage_v8::V8ScriptScope {
                coverage_root,
                project_root,
                discovered_sources,
            };
            let raw = super::coverage_v8::load_v8_coverage_map(&dump_files, &scope)?;
            // The V8 records already carry rebased canonical paths.
            let sources = CoverageMapSources {
                coverage_root: None,
                project_root,
                discovered_sources,
                relocated,
            };
            Ok(build_istanbul_coverage(
                &raw,
                &sources,
                fallow_output::CoverageInputFormat::V8,
            ))
        }
    }
}

/// A `--coverage` input after format detection.
enum CoverageInput {
    Istanbul {
        file_path: std::path::PathBuf,
        json: String,
    },
    V8 {
        dump_files: Vec<std::path::PathBuf>,
    },
}

/// Detect the format of a `--coverage` path. A directory holds either an
/// Istanbul `coverage-final.json` (preferred) or `NODE_V8_COVERAGE` dumps; a
/// file is an Istanbul map or one V8 dump.
fn read_coverage_input(resolved: &std::path::Path) -> Result<CoverageInput, String> {
    if resolved.is_dir() {
        let candidate = resolved.join("coverage-final.json");
        if candidate.is_file() {
            return read_coverage_file(candidate);
        }
        let dump_files = super::coverage_v8::dump_files_in(resolved)?;
        if dump_files.is_empty() {
            return Err(format!(
                "no coverage-final.json or V8 coverage files found in {}",
                resolved.display()
            ));
        }
        return Ok(CoverageInput::V8 { dump_files });
    }
    read_coverage_file(resolved.to_path_buf())
}

fn read_coverage_file(file_path: std::path::PathBuf) -> Result<CoverageInput, String> {
    let json = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("failed to read coverage file {}: {e}", file_path.display()))?;
    if super::coverage_v8::is_v8_dump(&json) {
        return Ok(CoverageInput::V8 {
            dump_files: vec![file_path],
        });
    }
    Ok(CoverageInput::Istanbul { file_path, json })
}

/// How the file paths of a coverage map relate to the analyzed project.
#[derive(Clone, Copy)]
struct CoverageMapSources<'a> {
    coverage_root: Option<&'a std::path::Path>,
    project_root: Option<&'a std::path::Path>,
    discovered_sources: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    relocated: bool,
}

fn build_istanbul_coverage(
    raw: &std::collections::BTreeMap<String, oxc_coverage_instrument::FileCoverage>,
    sources: &CoverageMapSources<'_>,
    format: fallow_output::CoverageInputFormat,
) -> IstanbulCoverage {
    let CoverageMapSources {
        coverage_root,
        project_root,
        discovered_sources,
        relocated,
    } = *sources;
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

        let statement_owners = tally_statements_by_owner(file_cov);
        let mut functions = Vec::with_capacity(file_cov.fn_map.len());
        for (fn_id, fn_entry) in &file_cov.fn_map {
            let coverage_pct = compute_function_statement_coverage(
                file_cov,
                fn_id,
                statement_owners.get(fn_id.as_str()),
            );
            if let Some(function) =
                istanbul_function_coverage(fn_entry, coverage_pct, source_index.as_ref())
            {
                functions.push(function);
            }
        }

        files.insert(canonical, IstanbulFileCoverage::new(functions, relocated));
    }

    IstanbulCoverage { files, format }
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

/// Parse a coverage map, retrying once with negative values clamped.
///
/// A producer can emit a negative coordinate for a position it could not
/// place: `v8-to-istanbul`, which is what c8 and nyc write, records
/// `column: -1` on the implicit else of a bare `if`. Vitest `coverage-v8`
/// (through ast-v8-to-istanbul) records a negative hit count for that same
/// implicit else in `b`. Positions and counts are unsigned, so one such value
/// rejects the entire map. The retry costs a second parse and only runs after
/// the strict one has already failed.
fn parse_coverage_map_tolerantly(
    json: &str,
) -> Result<std::collections::BTreeMap<String, oxc_coverage_instrument::FileCoverage>, String> {
    match oxc_coverage_instrument::parse_coverage_map(json) {
        Ok(raw) => Ok(raw),
        Err(strict_error) => {
            let mut value: serde_json::Value =
                serde_json::from_str(json).map_err(|_| strict_error.to_string())?;
            let positions = clamp_negative_positions(&mut value);
            let counts = clamp_negative_counts(&mut value);
            if !positions && !counts {
                return Err(strict_error.to_string());
            }
            serde_json::from_value(value).map_err(|_| strict_error.to_string())
        }
    }
}

/// Clamp every negative `line` or `column` in the tree to zero, reporting
/// whether anything changed. Zero is what the shared position type already
/// uses for a coordinate a producer left null or absent.
fn clamp_negative_positions(value: &mut serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(entries) => {
            let mut clamped = false;
            for (key, child) in entries.iter_mut() {
                if matches!(key.as_str(), "line" | "column")
                    && child.as_i64().is_some_and(|number| number < 0)
                {
                    *child = serde_json::Value::from(0);
                    clamped = true;
                    continue;
                }
                clamped |= clamp_negative_positions(child);
            }
            clamped
        }
        serde_json::Value::Array(items) => items.iter_mut().fold(false, |clamped, item| {
            clamped | clamp_negative_positions(item)
        }),
        _ => false,
    }
}

/// The hit-count sections of an Istanbul file record.
const COVERAGE_COUNT_KEYS: [&str; 4] = ["s", "f", "b", "b_T"];

/// Clamp every negative hit count in the map to zero, reporting whether
/// anything changed. A negative count is a miss: the coverage test here is
/// `count > 0`, so zero gives the same answer the signed value would.
fn clamp_negative_counts(value: &mut serde_json::Value) -> bool {
    let serde_json::Value::Object(files) = value else {
        return false;
    };
    let mut clamped = false;
    for file in files.values_mut() {
        let serde_json::Value::Object(sections) = file else {
            continue;
        };
        for key in COVERAGE_COUNT_KEYS {
            if let Some(counts) = sections.get_mut(key) {
                clamped |= clamp_negative_numbers(counts);
            }
        }
    }
    clamped
}

/// Clamp every negative number in the tree to zero, reporting whether
/// anything changed.
fn clamp_negative_numbers(value: &mut serde_json::Value) -> bool {
    match value {
        serde_json::Value::Number(number) if number.as_f64().is_some_and(|n| n < 0.0) => {
            *value = serde_json::Value::from(0);
            true
        }
        serde_json::Value::Object(entries) => entries.values_mut().fold(false, |clamped, child| {
            clamped | clamp_negative_numbers(child)
        }),
        serde_json::Value::Array(items) => items.iter_mut().fold(false, |clamped, item| {
            clamped | clamp_negative_numbers(item)
        }),
        _ => false,
    }
}

/// The property name behind an accessor record, when a producer prefixed it.
///
/// istanbul-lib-instrument leaves a class accessor anonymous, but raw V8
/// coverage and `oxc-coverage-instrument` record `get area` and `set area`,
/// while fallow extracts the unit as `area`. The prefix is the whole
/// difference, so a record keeps its own spelling and answers to the bare
/// property name as well.
fn accessor_property_name(name: &str) -> Option<&str> {
    let property = name
        .strip_prefix("get ")
        .or_else(|| name.strip_prefix("set "))?;
    (!property.is_empty() && !property.contains(' ')).then_some(property)
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
    fallow_extract::parse_bounded_or_default(source, source_type, |parsed| {
        if parsed.fatal_error || !parsed.diagnostics.is_empty() {
            return None;
        }
        let mut starts = rustc_hash::FxHashMap::default();
        let mut collector = NamedFunctionSyntaxCollector {
            starts: &mut starts,
        };
        oxc_ast_visit::Visit::visit_program(&mut collector, &parsed.program);
        Some(starts)
    })
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

/// Statements owned by a single `fnMap` record.
#[derive(Default)]
struct StatementTally {
    covered: u32,
    total: u32,
}

/// Whether `outer` encloses `inner`, with both bounds inclusive.
///
/// These are the bounds the per-function statement scan has always used, so
/// which statements a function can claim is unchanged; only how a contested
/// statement is awarded moved.
fn istanbul_range_contains(
    outer: &oxc_coverage_instrument::Location,
    inner: &oxc_coverage_instrument::Location,
) -> bool {
    let after_start = inner.start.line > outer.start.line
        || (inner.start.line == outer.start.line && inner.start.column >= outer.start.column);
    let before_end = inner.end.line < outer.end.line
        || (inner.end.line == outer.end.line && inner.end.column <= outer.end.column);
    after_start && before_end
}

/// Assign every statement to the innermost `fnMap` record that encloses it.
///
/// A nested function body lies inside the enclosing function's `loc`, so a
/// bare containment test charges a closure's statements to the function that
/// returns it as well, and an outer function that ran every statement of its
/// own is scored down for code it does not own. Ownership here is exclusive:
/// a statement counts for exactly one record, or for none at all when no
/// record encloses it, which keeps module-scope statements out of every
/// function as before.
///
/// Innermost is the record with the greatest `loc` start. A tie breaks on the
/// smallest `loc` end, and a remaining tie between two byte-identical ranges
/// keeps the first record in `fnMap` key order, which is stable because the
/// map is a `BTreeMap`. Identical ranges are pathological and any winner is
/// arbitrary, so determinism is the property that matters.
///
/// Computed once per file for the same reason as
/// [`mark_headers_holding_other_fns`]: every record needs the answer, and the
/// alternative rescans the whole statement map once per record.
fn tally_statements_by_owner(
    file_cov: &oxc_coverage_instrument::FileCoverage,
) -> rustc_hash::FxHashMap<&str, StatementTally> {
    let mut owners: rustc_hash::FxHashMap<&str, StatementTally> = rustc_hash::FxHashMap::default();

    for (stmt_id, stmt_loc) in &file_cov.statement_map {
        let mut owner: Option<(&str, &oxc_coverage_instrument::Location)> = None;

        for (fn_id, fn_entry) in &file_cov.fn_map {
            if !istanbul_range_contains(&fn_entry.loc, stmt_loc) {
                continue;
            }
            let inner_than_current = owner.is_none_or(|(_, best)| {
                let candidate = (
                    (fn_entry.loc.start.line, fn_entry.loc.start.column),
                    std::cmp::Reverse((fn_entry.loc.end.line, fn_entry.loc.end.column)),
                );
                let incumbent = (
                    (best.start.line, best.start.column),
                    std::cmp::Reverse((best.end.line, best.end.column)),
                );
                candidate > incumbent
            });
            if inner_than_current {
                owner = Some((fn_id.as_str(), &fn_entry.loc));
            }
        }

        let Some((fn_id, _)) = owner else {
            continue;
        };
        let tally = owners.entry(fn_id).or_default();
        tally.total += 1;
        if file_cov.s.get(stmt_id).copied().unwrap_or(0) > 0 {
            tally.covered += 1;
        }
    }

    owners
}

/// Compute statement-level coverage percentage for a single function.
///
/// Takes the fraction of the statements the function owns that have non-zero
/// hit counts, where ownership comes from [`tally_statements_by_owner`] and a
/// statement belongs only to the innermost function that encloses it. When the
/// function owns no statements (a one-liner arrow, a getter, an outer function
/// whose whole body is a nested declaration, or a producer that emits an empty
/// `statementMap`), falls back to the function hit count as a binary signal.
fn compute_function_statement_coverage(
    file_cov: &oxc_coverage_instrument::FileCoverage,
    fn_id: &str,
    owned: Option<&StatementTally>,
) -> f64 {
    match owned {
        Some(tally) if tally.total > 0 => f64::from(tally.covered) / f64::from(tally.total) * 100.0,
        _ => {
            let hit = file_cov.f.get(fn_id).copied().unwrap_or(0);
            if hit > 0 { 100.0 } else { 0.0 }
        }
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
    let signal = file_score_crap_signal(score, max_crap_threshold);
    max_crap_threshold <= 0.0
        || (score.crap_above_threshold == 0
            && if signal.suppression_aware {
                signal.maximum >= CRAP_THRESHOLD
            } else {
                score.crap_exempted > 0
            })
}

/// The complete CRAP metric/ceiling pair used for triage and remediation.
pub struct FileScoreCrapSignal {
    /// Maximum remaining score, or the raw maximum for legacy rows.
    pub maximum: f64,
    /// Effective ceiling belonging to the same population as the maximum.
    pub threshold: f64,
    suppression_aware: bool,
}

impl FileScoreCrapSignal {
    /// JSON metric whose value the signal carries.
    pub const fn metric(&self) -> &'static str {
        if self.suppression_aware {
            "crap_unsuppressed_max"
        } else {
            "crap_max"
        }
    }
}

/// Select the unsuppressed pair only when complete. Absent or partial external
/// metadata uses the complete legacy pair, never a mixture of populations.
pub fn file_score_crap_signal(score: &FileHealthScore, global: f64) -> FileScoreCrapSignal {
    match (
        score.crap_unsuppressed_max,
        score.crap_unsuppressed_effective_threshold,
    ) {
        (Some(maximum), Some(threshold)) => FileScoreCrapSignal {
            maximum,
            threshold,
            suppression_aware: true,
        },
        _ => FileScoreCrapSignal {
            maximum: score.crap_max,
            threshold: score.crap_effective_threshold.unwrap_or(global),
            suppression_aware: false,
        },
    }
}

/// CRAP concern bands generalized over the selected metric pair's ceiling `t`.
/// The complete unsuppressed pair takes precedence over legacy metrics. At `t = 30`
/// the breakpoints are the historical (15, 30, 100). A fully exempt file
/// scores `0.0`: band generalization alone cannot drop an exempted file below
/// its structural concern, so the tag would keep reading `risk` against a run
/// with zero findings.
fn file_score_crap_concern(score: &FileHealthScore, max_crap_threshold: f64) -> f64 {
    if file_score_fully_crap_exempt(score, max_crap_threshold) {
        return 0.0;
    }
    let signal = file_score_crap_signal(score, max_crap_threshold);
    let crap_max = signal.maximum;
    let t = signal.threshold;
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
        .then_with(|| {
            file_score_crap_signal(b, max_crap_threshold)
                .maximum
                .total_cmp(&file_score_crap_signal(a, max_crap_threshold).maximum)
        })
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
    /// Build the static edge index for change coupling. Only a run that reads
    /// the commit history needs it, so other runs skip the graph walk.
    pub(super) with_static_links: bool,
    /// Graph of the full project that supplies test reachability when the
    /// analysis graph has no test files (production mode).
    pub(super) test_reach: Option<&'a super::test_reach::TestReachGraph>,
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
        with_static_links,
        test_reach,
    } = input;
    let retained_graph = analysis_output.graph.ok_or("graph not available")?;
    let test_coverage = test_reach.map_or_else(
        || retained_graph.static_test_coverage(),
        super::test_reach::TestReachGraph::static_test_coverage,
    );
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
        with_static_links,
        file_paths,
        results,
        scores: acc.scores,
        coverage,
        circular_files,
        top_complex_fns,
        crap_target_fns: acc.crap_target_fns,
        entry_points: acc.entry_points,
        value_export_counts: acc.value_export_counts,
        unused_export_names: acc.unused_export_names,
        cycle_members,
        direct_callers,
        istanbul_matched: acc.istanbul_matched,
        istanbul_total: acc.istanbul_total,
        istanbul_files_joined: acc.istanbul_files_joined,
        istanbul_files_total: acc.istanbul_files_total,
        coverage_input_format: istanbul_coverage.map(IstanbulCoverage::format),
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
    crap_target_fns: rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>>,
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
            crap_target_fns: rustc_hash::FxHashMap::default(),
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
    let suppressions = ctx
        .module_by_id
        .get(&node.file_id)
        .map_or(&[][..], |module| module.suppressions.as_slice());
    let ceilings = CrapCeilingLookup::new(ctx.crap_thresholds, relative, suppressions);
    let crap = compute_file_score_crap(node, ctx, &path_owned, &ceilings);
    acc.istanbul_matched += crap.istanbul_matched;
    acc.istanbul_total += crap.istanbul_total;
    acc.istanbul_files_joined += usize::from(crap.coverage_file_joined);
    record_per_function_crap(&mut acc.per_function_crap, &path_owned, crap.per_function);
    let mut eligible_functions = crap.signals.eligible_functions;
    if !eligible_functions.is_empty() {
        eligible_functions.sort_by_key(|function| std::cmp::Reverse(function.2));
        eligible_functions.truncate(MAX_FUNCTION_EVIDENCE);
        acc.crap_target_fns
            .insert(path_owned.clone(), eligible_functions);
    }

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
        crap_suppressed: crap.signals.suppressed,
        crap_unsuppressed_max: (crap.signals.suppressed > 0)
            .then_some(crap.signals.unsuppressed_max),
        crap_unsuppressed_effective_threshold: (crap.signals.suppressed > 0)
            .then_some(crap.signals.unsuppressed_min_ceiling.unwrap_or(global_crap)),
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
        crap_target_fns: parts.crap_target_fns,
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
        coverage_input_format: parts.coverage_input_format,
        per_function_crap: parts.per_function_crap,
        template_inherit_provenance,
        static_links: parts
            .with_static_links
            .then(|| crate::change_coupling::StaticLinkIndex::from_graph(parts.graph)),
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
        CrapCoverageResolution::StaticEstimated => {
            compute_static_file_crap(module, node, ctx.test_coverage, is_test_reachable, ceilings)
        }
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
    node: &fallow_graph::graph::ModuleNode,
    test_coverage: StaticTestCoverage<'_>,
    is_test_reachable: bool,
    ceilings: &CrapCeilingLookup<'_>,
) -> FileScoreCrap {
    let test_refs = build_test_referenced_exports(node.file_id, &node.exports, test_coverage);
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
        // The module-scope unit is excluded: this list feeds the refactoring
        // target's evidence copy, which would otherwise render
        // "<module> has cognitive complexity 12" next to advice about
        // extracting helpers.
        let mut funcs: Vec<(String, u32, u16)> = module
            .complexity
            .iter()
            .filter(|f| !fallow_types::extract::is_synthetic_module_unit(&f.name))
            .map(|f| (f.name.clone(), f.line, f.cognitive))
            .collect();
        funcs.sort_by_key(|f| std::cmp::Reverse(f.2));
        funcs.truncate(MAX_FUNCTION_EVIDENCE);
        if funcs.first().is_some_and(|worst| worst.2 > 0) {
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
#[path = "scoring_private_member_tests.rs"]
mod private_member_tests;

#[cfg(test)]
#[path = "scoring_tests.rs"]
mod tests;
