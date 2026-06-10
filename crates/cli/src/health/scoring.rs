use crate::health_types::{DirectCallerEvidence, DirectCallerSymbolEvidence, FileHealthScore};

use super::coverage_gaps::compute_coverage_gaps;
pub(super) use super::coverage_gaps::{CoverageGapData, build_coverage_summary};

/// Output from `compute_file_scores`, including auxiliary data for refactoring targets.
pub(super) struct FileScoreOutput {
    pub scores: Vec<FileHealthScore>,
    /// Static coverage gaps derived from runtime-vs-test reachability.
    pub coverage: CoverageGapData,
    /// Files participating in circular dependencies (absolute paths).
    pub circular_files: rustc_hash::FxHashSet<std::path::PathBuf>,
    /// Top 3 functions by cognitive complexity per file (name, line, cognitive score).
    pub top_complex_fns: rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>>,
    /// Files that are configured entry points.
    pub entry_points: rustc_hash::FxHashSet<std::path::PathBuf>,
    /// Total number of value exports per file (for dead code gate: total_value_exports >= 3).
    pub value_export_counts: rustc_hash::FxHashMap<std::path::PathBuf, usize>,
    /// Unused export names per file (for evidence linking).
    pub unused_export_names: rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
    /// Cycle members per file: maps each file to the other files in its cycle.
    pub cycle_members: rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
    /// Direct importers per file, with the symbols imported by each caller.
    pub direct_callers: rustc_hash::FxHashMap<std::path::PathBuf, Vec<DirectCallerEvidence>>,
    /// Aggregate counts from AnalysisResults for vital signs (project-wide).
    pub analysis_counts: crate::vital_signs::AnalysisCounts,
    /// Per-path snapshot of analysis findings, used to recompute
    /// [`crate::vital_signs::AnalysisCounts`] for an arbitrary subset of files
    /// (workspace scoping, `--group-by` partitioning).
    pub analysis_snapshot: AnalysisCountsSnapshot,
    /// Istanbul match stats: functions matched / total (only meaningful with Istanbul model).
    pub istanbul_matched: usize,
    pub istanbul_total: usize,
    /// Per-file, per-function CRAP data used to emit `--max-crap` findings.
    /// Absolute paths match `FileHealthScore.path`. Absent entries indicate the
    /// file had zero functions.
    pub per_function_crap: rustc_hash::FxHashMap<std::path::PathBuf, Vec<PerFunctionCrap>>,
    /// Provenance map for synthetic Angular `<template>` findings whose CRAP
    /// was inherited from the owning `.component.ts` via the inverse
    /// `templateUrl` edge. Keys are the template `.html` absolute paths,
    /// values are the owner `.ts` absolute paths (the path used for the
    /// `inherited from foo.component.ts` human-output suffix). Absent for
    /// non-template files and for templates with no `.ts` owner.
    pub template_inherit_provenance: rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>,
}

/// Per-path snapshot of analysis-pipeline findings, retained alongside the
/// pre-aggregated `analysis_counts` so that workspace- or group-scoped runs
/// can recompute counts without re-running the full pipeline.
///
/// All paths are absolute (matching `AnalysisResults` and `FileHealthScore`).
#[derive(Clone, Default)]
pub(super) struct AnalysisCountsSnapshot {
    /// One entry per unused file.
    pub unused_file_paths: Vec<std::path::PathBuf>,
    /// One entry per unused value or type export, keyed by the file containing
    /// the export.
    pub unused_export_paths: Vec<std::path::PathBuf>,
    /// One entry per unused dependency across `dependencies`,
    /// `devDependencies`, and `optionalDependencies`, keyed by the
    /// `package.json` path that declared it.
    pub unused_dep_package_paths: Vec<std::path::PathBuf>,
    /// Each cycle as the set of file paths it contains. Used to count cycles
    /// that touch any file inside a workspace.
    pub circular_dep_groups: Vec<Vec<std::path::PathBuf>>,
    /// Total exports per module (`module.exports.len()` in the graph), used
    /// as the denominator for `dead_export_pct`.
    pub module_export_counts: rustc_hash::FxHashMap<std::path::PathBuf, usize>,
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
    /// [`crate::health::SubsetFilter::Paths`], a `package.json` is considered
    /// in scope when at least one source file in the subset sits inside its
    /// directory (the dep's owning workspace).
    ///
    /// `total_deps` is propagated unchanged from `defaults`; it is not
    /// available per-subset today (mirrors the project-wide behaviour).
    pub fn counts_for(
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
pub(super) fn aggregate_complexity(
    module: &fallow_core::extract::ModuleInfo,
) -> (u32, u32, usize, u32) {
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
pub(super) fn compute_dead_code_ratio(
    path: &std::path::Path,
    exports: &[fallow_core::graph::ExportSymbol],
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
pub(super) fn compute_complexity_density(total_cyclomatic: u32, lines: u32) -> f64 {
    if lines > 0 {
        f64::from(total_cyclomatic) / f64::from(lines)
    } else {
        0.0
    }
}

/// CRAP score threshold (inclusive). CC=5 untested gives exactly 30 (5^2 + 5),
/// matching the canonical CRAP threshold from Savoia & Evans (2007).
pub(super) const CRAP_THRESHOLD: f64 = 30.0;

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
pub(super) struct PerFunctionCrap {
    /// 1-based line number of the function's definition.
    pub line: u32,
    /// 0-based column of the function's definition. Required alongside `line`
    /// to disambiguate curried arrows that share a start line, e.g.
    /// `(x) => (y) => {...}`. Without `col`, two `PerFunctionCrap` entries
    /// would collide in the (path, line) finding index and one function's
    /// CRAP score could be attached to another function's identity.
    pub col: u32,
    /// Computed CRAP score, rounded to one decimal place.
    pub crap: f64,
    /// Coverage percentage used to compute `crap`, when Istanbul matched the
    /// function. `None` for estimated coverage or unmatched functions.
    pub coverage_pct: Option<f64>,
    /// Bucketed coverage tier used to drive action selection in JSON output.
    /// Populated for both Istanbul-matched and estimated CRAP rows so the
    /// action builder does not need to recompute reachability state.
    pub coverage_tier: crate::health_types::CoverageTier,
    /// Provenance of `coverage_tier` and `crap`. `Istanbul` for direct fnMap
    /// matches, `Estimated` for graph-based fallbacks against the finding's
    /// own file, `EstimatedComponentInherited` for the template-inherit path
    /// that reaches the owning Angular `.component.ts` through the inverse
    /// `templateUrl` edge. Threaded into `ComplexityViolation.coverage_source` by
    /// `merge_crap_findings`.
    pub coverage_source: crate::health_types::CoverageSource,
}

/// Istanbul CRAP result: CRAP scores plus match statistics.
pub(super) struct IstanbulCrapResult {
    pub max_crap: f64,
    pub above_threshold: usize,
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
#[expect(
    clippy::suboptimal_flops,
    reason = "cc * cc + cc matches the CRAP formula specification"
)]
fn compute_crap_scores_istanbul(
    complexity: &[fallow_types::extract::FunctionComplexity],
    file_coverage: Option<&IstanbulFileCoverage>,
    is_test_reachable: bool,
) -> IstanbulCrapResult {
    if complexity.is_empty() {
        return IstanbulCrapResult {
            max_crap: 0.0,
            above_threshold: 0,
            matched: 0,
            total: 0,
            per_function: Vec::new(),
        };
    }
    let mut max = 0.0_f64;
    let mut above = 0usize;
    let mut matched = 0usize;
    let mut per_function = Vec::with_capacity(complexity.len());
    for f in complexity {
        let cc = f64::from(f.cyclomatic);
        let lookup = file_coverage.and_then(|fc| fc.lookup(f.name.as_str(), f.line, f.col));
        let (crap, coverage_pct, tier, source) = if let Some(cov_pct) = lookup {
            matched += 1;
            (
                crap_formula(cc, cov_pct),
                Some(cov_pct),
                crate::health_types::CoverageTier::from_pct(cov_pct),
                crate::health_types::CoverageSource::Istanbul,
            )
        } else if is_test_reachable {
            (
                cc,
                None,
                crate::health_types::CoverageTier::from_pct(INDIRECT_TEST_COVERAGE_ESTIMATE),
                crate::health_types::CoverageSource::Estimated,
            )
        } else {
            (
                cc * cc + cc,
                None,
                crate::health_types::CoverageTier::None,
                crate::health_types::CoverageSource::Estimated,
            )
        };
        let crap_rounded = (crap * 10.0).round() / 10.0;
        max = max.max(crap);
        if crap >= CRAP_THRESHOLD {
            above += 1;
        }
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
        above_threshold: above,
        matched,
        total: complexity.len(),
        per_function,
    }
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
pub(super) struct EstimatedCrapResult {
    pub max_crap: f64,
    pub above_threshold: usize,
    pub per_function: Vec<PerFunctionCrap>,
}

fn compute_crap_scores_estimated(
    complexity: &[fallow_types::extract::FunctionComplexity],
    test_referenced_exports: &rustc_hash::FxHashSet<String>,
    is_test_reachable: bool,
    coverage_source: crate::health_types::CoverageSource,
) -> EstimatedCrapResult {
    if complexity.is_empty() {
        return EstimatedCrapResult {
            max_crap: 0.0,
            above_threshold: 0,
            per_function: Vec::new(),
        };
    }
    let mut max = 0.0_f64;
    let mut above = 0usize;
    let mut per_function = Vec::with_capacity(complexity.len());
    for f in complexity {
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
        if crap >= CRAP_THRESHOLD {
            above += 1;
        }
        per_function.push(PerFunctionCrap {
            line: f.line,
            col: f.col,
            crap: crap_rounded,
            coverage_pct: None,
            coverage_tier: crate::health_types::CoverageTier::from_pct(estimated_coverage),
            coverage_source,
        });
    }
    EstimatedCrapResult {
        max_crap: (max * 10.0).round() / 10.0,
        above_threshold: above,
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
    graph: &fallow_core::graph::ModuleGraph,
    module_by_id: &rustc_hash::FxHashMap<
        fallow_core::discover::FileId,
        &fallow_core::extract::ModuleInfo,
    >,
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
) -> rustc_hash::FxHashMap<fallow_core::discover::FileId, TemplateInheritContext> {
    let mut out = rustc_hash::FxHashMap::default();
    for node in &graph.modules {
        let Some(path) = file_paths.get(&node.file_id) else {
            continue;
        };
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
        {
            continue;
        }
        let Some(module) = module_by_id.get(&node.file_id) else {
            continue;
        };
        if !module
            .complexity
            .iter()
            .any(|f| f.name.as_str() == "<template>")
        {
            continue;
        }
        let Some(importers) = graph.reverse_deps.get(node.file_id.0 as usize) else {
            continue;
        };

        let mut any_reachable = false;
        let mut combined_refs: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        let mut provenance: Option<std::path::PathBuf> = None;
        let mut first_owner: Option<std::path::PathBuf> = None;
        for &importer_id in importers {
            let Some(owner_node) = graph.modules.get(importer_id.0 as usize) else {
                continue;
            };
            let Some(owner_path) = file_paths.get(&importer_id) else {
                continue;
            };
            if !owner_path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "ts" | "tsx" | "mts" | "cts"
                    )
                })
            {
                continue;
            }
            if graph.test_entry_points.contains(&importer_id) {
                continue;
            }
            let owner_has_component = module_by_id
                .get(&importer_id)
                .is_some_and(|m| m.has_angular_component_template_url);
            if !owner_has_component {
                continue;
            }
            if first_owner.is_none() {
                first_owner = Some((**owner_path).clone());
            }
            let owner_reachable = owner_node.is_test_reachable();
            if owner_reachable {
                any_reachable = true;
                if provenance.is_none() {
                    provenance = Some((**owner_path).clone());
                }
                let refs = build_test_referenced_exports(&owner_node.exports, &graph.modules);
                combined_refs.extend(refs);
            }
        }
        let Some(provenance_owner) = provenance.or(first_owner) else {
            continue;
        };
        out.insert(
            node.file_id,
            TemplateInheritContext {
                is_test_reachable: any_reachable,
                test_referenced_exports: combined_refs,
                provenance_owner,
            },
        );
    }
    out
}

/// Build the set of export names that have at least one test-reachable reference.
///
/// This is the per-function signal: if an export named "foo" has a reference from
/// a test-reachable module, the function "foo" is considered directly tested.
fn build_test_referenced_exports(
    exports: &[fallow_core::graph::ExportSymbol],
    graph_modules: &[fallow_core::graph::ModuleNode],
) -> rustc_hash::FxHashSet<String> {
    let mut set = rustc_hash::FxHashSet::default();
    for export in exports {
        if export.is_type_only {
            continue;
        }
        let has_test_ref = export.references.iter().any(|reference| {
            graph_modules
                .get(reference.from_file.0 as usize)
                .is_some_and(fallow_core::graph::ModuleNode::is_test_reachable)
        });
        if has_test_ref {
            set.insert(export.name.to_string());
        }
    }
    set
}

fn collect_direct_callers(
    graph: &fallow_core::graph::ModuleGraph,
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
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

/// Pre-processed per-function coverage data for a single file,
/// derived from Istanbul `coverage-final.json`.
pub(super) struct IstanbulFileCoverage {
    /// Per-function coverage percentages, keyed by (name, line, col). Lines
    /// are 1-based and columns are 0-based, matching both fallow's
    /// `FunctionComplexity` positions and Istanbul `Position`s.
    ///
    /// Istanbul producers are not consistent about `FnEntry.line`: some use
    /// the declaration line, while others use the body start. The loader
    /// therefore indexes both the producer's effective line and
    /// `decl.start`, so multiline TypeScript signatures still match the
    /// function start that fallow extracts.
    functions: rustc_hash::FxHashMap<(String, u32, u32), f64>,
}

impl IstanbulFileCoverage {
    /// Look up coverage for a function by name, start line, and start column.
    ///
    /// Resolution order:
    /// 1. Exact `(name, line, col)` match.
    /// 2. Name-only fuzzy match within ±2 lines (tolerates formatter drift),
    ///    tie-broken by smallest `(line, col)` distance from the target.
    /// 3. Anonymous fallback: among Istanbul `(anonymous_N)` entries within
    ///    ±2 lines, pick the one closest in `(line, col)` to the target.
    ///    Bail only if two candidates tie on distance, which would be
    ///    genuinely ambiguous.
    ///
    /// Step 3 covers arrow-function exports where fallow extracts the binding
    /// identifier (`const myHandler = () => {...}` yields `myHandler`) while
    /// Istanbul records the function as anonymous. `load_istanbul_coverage`
    /// indexes declaration aliases so standard Istanbul producers still
    /// participate in this fallback. See issues #155, #166, #181, and #370.
    pub(super) fn lookup(&self, name: &str, line: u32, col: u32) -> Option<f64> {
        if let Some(&pct) = self.functions.get(&(name.to_string(), line, col)) {
            return Some(pct);
        }
        if let Some(pct) = self
            .functions
            .iter()
            .filter(|((n, l, _), _)| n == name && l.abs_diff(line) <= 2)
            .min_by_key(|((_, l, c), _)| (l.abs_diff(line), c.abs_diff(col)))
            .map(|(_, &pct)| pct)
        {
            return Some(pct);
        }
        let mut nearest_distance: Option<(u32, u32)> = None;
        let mut nearest_pct: Option<f64> = None;
        let mut tied = false;
        for ((n, l, c), &pct) in &self.functions {
            if !n.starts_with("(anonymous_") {
                continue;
            }
            if l.abs_diff(line) > 2 {
                continue;
            }
            let dist = (l.abs_diff(line), c.abs_diff(col));
            if dist.0 > 0 && dist.1 > ANONYMOUS_FALLBACK_MAX_COLUMN_DRIFT {
                continue;
            }
            match nearest_distance {
                None => {
                    nearest_distance = Some(dist);
                    nearest_pct = Some(pct);
                    tied = false;
                }
                Some(prev) if dist < prev => {
                    nearest_distance = Some(dist);
                    nearest_pct = Some(pct);
                    tied = false;
                }
                Some(prev) if dist == prev => {
                    tied = true;
                }
                Some(_) => {}
            }
        }
        if tied { None } else { nearest_pct }
    }
}

/// Loaded Istanbul coverage data, keyed by canonical file path.
pub(super) struct IstanbulCoverage {
    files: rustc_hash::FxHashMap<std::path::PathBuf, IstanbulFileCoverage>,
}

impl IstanbulCoverage {
    /// Get coverage data for a file path.
    pub fn get(&self, path: &std::path::Path) -> Option<&IstanbulFileCoverage> {
        self.files.get(path)
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

/// Load Istanbul coverage data from a `coverage-final.json` file or directory.
///
/// Auto-detect a `coverage-final.json` file in common locations relative to the project root.
///
/// Checks (in order): `coverage/coverage-final.json`, `.nyc_output/coverage-final.json`.
/// Returns the first path found, or `None` if no coverage file exists.
pub(super) fn auto_detect_coverage(root: &std::path::Path) -> Option<std::path::PathBuf> {
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
    if crate::path_util::is_absolute_path_any_platform(path) {
        return path.to_path_buf();
    }
    match project_root {
        Some(root) => root.join(path),
        None => path.to_path_buf(),
    }
}

pub fn validate_coverage_root_absolute(
    coverage_root: Option<&std::path::Path>,
) -> Result<(), String> {
    if let Some(path) = coverage_root
        && !path.has_root()
    {
        return Err(format!(
            "--coverage-root expects an absolute path prefix from the coverage data, got '{}'. Use the checkout prefix from the machine that generated coverage, for example '/home/runner/work/myapp'.",
            path.display()
        ));
    }
    Ok(())
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
pub(super) fn load_istanbul_coverage(
    path: &std::path::Path,
    coverage_root: Option<&std::path::Path>,
    project_root: Option<&std::path::Path>,
) -> Result<IstanbulCoverage, String> {
    validate_coverage_root_absolute(coverage_root)?;
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
        let raw_path = std::path::PathBuf::from(&file_cov.path);
        let file_path = if let (Some(cov_root), Some(proj_root)) = (coverage_root, project_root) {
            raw_path
                .strip_prefix(cov_root)
                .map(|rel| proj_root.join(rel))
                .unwrap_or(raw_path)
        } else {
            raw_path
        };
        let canonical = dunce::canonicalize(&file_path).unwrap_or(file_path);

        let mut functions = rustc_hash::FxHashMap::default();
        for (fn_id, fn_entry) in &file_cov.fn_map {
            let coverage_pct = compute_function_statement_coverage(file_cov, fn_id, fn_entry);
            insert_istanbul_function_coverage(&mut functions, fn_entry, coverage_pct);
        }

        files.insert(canonical, IstanbulFileCoverage { functions });
    }

    Ok(IstanbulCoverage { files })
}

fn insert_istanbul_function_coverage(
    functions: &mut rustc_hash::FxHashMap<(String, u32, u32), f64>,
    fn_entry: &oxc_coverage_instrument::FnEntry,
    coverage_pct: f64,
) {
    let name = fn_entry.name.clone();
    let primary = (
        name.clone(),
        effective_istanbul_fn_line(fn_entry),
        effective_istanbul_fn_col(fn_entry),
    );
    functions.insert(primary.clone(), coverage_pct);

    let declaration = (name, fn_entry.decl.start.line, fn_entry.decl.start.column);
    if declaration != primary {
        functions.entry(declaration).or_insert(coverage_pct);
    }
}

fn effective_istanbul_fn_line(fn_entry: &oxc_coverage_instrument::FnEntry) -> u32 {
    if fn_entry.line > 0 {
        fn_entry.line
    } else {
        fn_entry.decl.start.line
    }
}

/// Effective 0-based start column for an Istanbul function entry. `FnEntry`
/// has no top-level `column` field, so we always read it off
/// `decl.start.column`. Both fallow's `FunctionComplexity.col` and Istanbul's
/// `Position::column` are 0-based, so they match directly.
fn effective_istanbul_fn_col(fn_entry: &oxc_coverage_instrument::FnEntry) -> u32 {
    fn_entry.decl.start.column
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
pub(super) fn count_unused_exports_by_path(
    unused_exports: &[fallow_core::results::UnusedExportFinding],
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
pub(super) fn compute_maintainability_index(
    complexity_density: f64,
    dead_code_ratio: f64,
    fan_out: usize,
    lines: u32,
) -> f64 {
    let dampening = (f64::from(lines) / crate::health_types::MI_DENSITY_MIN_LINES).min(1.0);
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

fn file_score_crap_concern(crap_max: f64) -> f64 {
    if crap_max <= 0.0 {
        0.0
    } else if crap_max < 15.0 {
        (crap_max / 15.0) * 45.0
    } else if crap_max < CRAP_THRESHOLD {
        ((crap_max - 15.0) / 15.0).mul_add(30.0, 45.0)
    } else if crap_max < 100.0 {
        ((crap_max - CRAP_THRESHOLD) / (100.0 - CRAP_THRESHOLD)).mul_add(25.0, 75.0)
    } else {
        100.0
    }
}

fn file_score_triage_concern(score: &FileHealthScore) -> f64 {
    file_score_structural_concern(score).max(file_score_crap_concern(score.crap_max))
}

/// Which signal places a file at its triage rank: its structural quality (low
/// maintainability index) or its untested complexity (CRAP risk). Surfaced per
/// row so the human file-scores table can label why a file sits where it does
/// when the two axes disagree (e.g. a low-CRAP file outranking a higher-CRAP
/// one because its MI is the worse signal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileScoreConcern {
    Structural,
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

/// Classify which concern drove `score` to its rank. A file with no CRAP risk
/// is always `Structural`; otherwise the larger concern wins, with ties (and
/// the boundary where the two are equal) resolving to `Risk` because untested
/// complexity is the more urgent signal to act on.
pub fn file_score_concern_axis(score: &FileHealthScore) -> FileScoreConcern {
    if score.crap_max <= 0.0 {
        FileScoreConcern::Structural
    } else if file_score_crap_concern(score.crap_max) >= file_score_structural_concern(score) {
        FileScoreConcern::Risk
    } else {
        FileScoreConcern::Structural
    }
}

fn compare_file_score_triage(a: &FileHealthScore, b: &FileHealthScore) -> std::cmp::Ordering {
    file_score_triage_concern(b)
        .total_cmp(&file_score_triage_concern(a))
        .then_with(|| b.crap_max.total_cmp(&a.crap_max))
        .then_with(|| a.maintainability_index.total_cmp(&b.maintainability_index))
        .then_with(|| a.path.cmp(&b.path))
}

/// Compute per-file health scores using a pre-computed analysis output.
///
/// The caller provides an `AnalysisOutput` (with graph and dead code results)
/// so this function does not need to re-run the analysis pipeline. Complexity
/// density is derived from the already-parsed modules.
#[expect(
    clippy::too_many_lines,
    reason = "file scoring still coordinates per-module score assembly after setup extraction"
)]
pub(super) fn compute_file_scores(
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    analysis_output: fallow_core::AnalysisOutput,
    istanbul_coverage: Option<&IstanbulCoverage>,
    root: &std::path::Path,
) -> Result<FileScoreOutput, String> {
    let graph = analysis_output.graph.ok_or("graph not available")?;
    let results = &analysis_output.results;

    let circular_files = collect_circular_files(results);
    let top_complex_fns = collect_top_complex_fns(modules, file_paths);
    let cycle_members = collect_cycle_members(results);
    let direct_callers = collect_direct_callers(&graph, file_paths);
    let mut unused_export_names = collect_unused_export_names(results);

    let mut entry_points: rustc_hash::FxHashSet<std::path::PathBuf> =
        rustc_hash::FxHashSet::default();
    let mut value_export_counts: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
        rustc_hash::FxHashMap::default();

    let unused_files: rustc_hash::FxHashSet<&std::path::Path> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.as_path())
        .collect();

    let unused_exports_by_path = count_unused_exports_by_path(&results.unused_exports);

    let module_by_id: rustc_hash::FxHashMap<
        fallow_core::discover::FileId,
        &fallow_core::extract::ModuleInfo,
    > = modules.iter().map(|m| (m.file_id, m)).collect();
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
    let coverage = compute_coverage_gaps(&graph, file_paths, &module_by_id, &unused_exports, root);

    let mut scores = Vec::with_capacity(graph.modules.len());
    let mut istanbul_matched = 0usize;
    let mut istanbul_total = 0usize;
    let mut per_function_crap: rustc_hash::FxHashMap<std::path::PathBuf, Vec<PerFunctionCrap>> =
        rustc_hash::FxHashMap::default();

    let template_inherit = build_template_inherit_contexts(&graph, &module_by_id, file_paths);

    for node in &graph.modules {
        let Some(path) = file_paths.get(&node.file_id) else {
            continue;
        };

        if node.is_entry_point() {
            entry_points.insert((*path).clone());
        }

        let fan_in = graph
            .reverse_deps
            .get(node.file_id.0 as usize)
            .map_or(0, Vec::len);

        let fan_out = node.edge_range.len();

        let (total_cyclomatic, total_cognitive, function_count, lines) = module_by_id
            .get(&node.file_id)
            .map_or((0, 0, 0, 0), |module| aggregate_complexity(module));

        let value_exports = node.exports.iter().filter(|e| !e.is_type_only).count();
        let path_owned = (*path).clone();
        value_export_counts.insert(path_owned.clone(), value_exports);

        if unused_files.contains(path_owned.as_path())
            && !unused_export_names.contains_key(&path_owned)
        {
            let names: Vec<String> = node
                .exports
                .iter()
                .filter(|e| !e.is_type_only)
                .map(|e| e.name.to_string())
                .collect();
            if !names.is_empty() {
                unused_export_names.insert(path_owned.clone(), names);
            }
        }

        let dead_code_ratio = compute_dead_code_ratio(
            path_owned.as_path(),
            &node.exports,
            &unused_files,
            &unused_exports_by_path,
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

        let module = module_by_id.get(&node.file_id);
        let is_coverage_suppressed = module.is_some_and(|m| {
            fallow_core::suppress::is_file_suppressed(
                &m.suppressions,
                fallow_types::suppress::IssueKind::CoverageGaps,
            )
        });
        let is_test_reachable = node.is_test_reachable() || is_coverage_suppressed;
        let crap_resolution = resolve_crap_coverage(
            template_inherit.get(&node.file_id),
            istanbul_coverage,
            &path_owned,
        );
        let (crap_max, crap_above_threshold, per_function) = match (module, crap_resolution) {
            (None, _) => (0.0, 0, Vec::new()),
            (Some(m), CrapCoverageResolution::TemplateInherited(inherit_ctx)) => {
                let result = compute_crap_scores_estimated(
                    &m.complexity,
                    &inherit_ctx.test_referenced_exports,
                    inherit_ctx.is_test_reachable,
                    crate::health_types::CoverageSource::EstimatedComponentInherited,
                );
                (result.max_crap, result.above_threshold, result.per_function)
            }
            (Some(m), CrapCoverageResolution::Istanbul { file_coverage }) => {
                let result =
                    compute_crap_scores_istanbul(&m.complexity, file_coverage, is_test_reachable);
                istanbul_matched += result.matched;
                istanbul_total += result.total;
                (result.max_crap, result.above_threshold, result.per_function)
            }
            (Some(m), CrapCoverageResolution::StaticEstimated) => {
                let test_refs = build_test_referenced_exports(&node.exports, &graph.modules);
                let result = compute_crap_scores_estimated(
                    &m.complexity,
                    &test_refs,
                    is_test_reachable,
                    crate::health_types::CoverageSource::Estimated,
                );
                (result.max_crap, result.above_threshold, result.per_function)
            }
        };

        if !per_function.is_empty() {
            per_function_crap.insert(path_owned.clone(), per_function);
        }

        scores.push(FileHealthScore {
            path: path_owned,
            fan_in,
            fan_out,
            dead_code_ratio: dead_code_ratio_rounded,
            complexity_density: complexity_density_rounded,
            maintainability_index: (maintainability_index * 10.0).round() / 10.0,
            total_cyclomatic,
            total_cognitive,
            function_count,
            lines,
            crap_max,
            crap_above_threshold,
        });
    }

    if let Some(changed) = changed_files {
        scores.retain(|s| changed.contains(&s.path));
    }

    scores.retain(|s| s.function_count > 0);

    scores.sort_by(compare_file_score_triage);

    let total_exports: usize = graph.modules.iter().map(|m| m.exports.len()).sum();
    let dead_exports = results.unused_exports.len() + results.unused_types.len();
    let unused_deps = results.unused_dependencies.len()
        + results.unused_dev_dependencies.len()
        + results.unused_optional_dependencies.len();
    let total_deps = 0usize;

    let analysis_snapshot =
        build_analysis_counts_snapshot(&graph, file_paths, results, unused_deps);

    Ok(FileScoreOutput {
        scores,
        coverage,
        circular_files,
        top_complex_fns,
        entry_points,
        value_export_counts,
        unused_export_names,
        cycle_members,
        direct_callers,
        analysis_counts: crate::vital_signs::AnalysisCounts {
            total_exports,
            dead_files: results.unused_files.len(),
            dead_exports,
            unused_deps,
            circular_deps: results.circular_dependencies.len(),
            total_deps,
        },
        analysis_snapshot,
        istanbul_matched,
        istanbul_total,
        per_function_crap,
        template_inherit_provenance: template_inherit
            .into_iter()
            .filter_map(|(file_id, ctx)| {
                file_paths
                    .get(&file_id)
                    .map(|p| ((**p).clone(), ctx.provenance_owner))
            })
            .collect(),
    })
}

fn collect_circular_files(
    results: &fallow_core::results::AnalysisResults,
) -> rustc_hash::FxHashSet<std::path::PathBuf> {
    results
        .circular_dependencies
        .iter()
        .flat_map(|c| c.cycle.files.iter().cloned())
        .collect()
}

fn collect_top_complex_fns(
    modules: &[fallow_core::extract::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
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
    results: &fallow_core::results::AnalysisResults,
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
    results: &fallow_core::results::AnalysisResults,
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
    graph: &fallow_core::graph::ModuleGraph,
    file_paths: &rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf>,
    results: &fallow_core::results::AnalysisResults,
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
    use super::*;

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
        let exports: Vec<fallow_core::graph::ExportSymbol> = vec![];

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
        let exports: Vec<fallow_core::graph::ExportSymbol> = vec![];

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_ratio_mix() {
        let unused_files = rustc_hash::FxHashSet::default();
        let path = std::path::Path::new("/src/foo.ts");

        let exports = vec![
            fallow_core::graph::ExportSymbol {
                name: fallow_core::extract::ExportName::Named("a".into()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                references: vec![],
                members: vec![],
            },
            fallow_core::graph::ExportSymbol {
                name: fallow_core::extract::ExportName::Named("b".into()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                references: vec![],
                members: vec![],
            },
            fallow_core::graph::ExportSymbol {
                name: fallow_core::extract::ExportName::Named("c".into()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                references: vec![],
                members: vec![],
            },
            fallow_core::graph::ExportSymbol {
                name: fallow_core::extract::ExportName::Named("MyType".into()),
                is_type_only: true,
                is_side_effect_used: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                references: vec![],
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

        let exports = vec![fallow_core::graph::ExportSymbol {
            name: fallow_core::extract::ExportName::Named("Foo".into()),
            is_type_only: true,
            is_side_effect_used: false,
            visibility: fallow_core::extract::VisibilityTag::None,
            span: oxc_span::Span::empty(0),
            references: vec![],
            members: vec![],
        }];
        let unused_map = rustc_hash::FxHashMap::default();

        let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
        assert!((ratio).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_complexity_empty_module() {
        let module = fallow_core::extract::ModuleInfo {
            file_id: fallow_core::discover::FileId(0),
            exports: vec![],
            imports: vec![],
            re_exports: vec![],
            dynamic_imports: vec![],
            dynamic_import_patterns: vec![],
            require_calls: vec![],
            package_path_references: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: vec![],
            unknown_suppression_kinds: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            line_offsets: vec![],
            complexity: vec![],
            flag_uses: vec![],
            class_heritage: vec![],
            injection_tokens: vec![],
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
        };

        let (cyc, cog, funcs, lines) = aggregate_complexity(&module);
        assert_eq!(cyc, 0);
        assert_eq!(cog, 0);
        assert_eq!(funcs, 0);
        assert_eq!(lines, 0);
    }

    #[test]
    fn aggregate_complexity_single_function() {
        let module = fallow_core::extract::ModuleInfo {
            file_id: fallow_core::discover::FileId(0),
            exports: vec![],
            imports: vec![],
            re_exports: vec![],
            dynamic_imports: vec![],
            dynamic_import_patterns: vec![],
            require_calls: vec![],
            package_path_references: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: vec![],
            unknown_suppression_kinds: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            flag_uses: vec![],
            class_heritage: vec![],
            injection_tokens: vec![],
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
            line_offsets: vec![0, 10, 20, 30, 40], // 5 lines
            complexity: vec![fallow_types::extract::FunctionComplexity {
                name: "doStuff".into(),
                line: 1,
                col: 0,
                cyclomatic: 7,
                cognitive: 4,
                line_count: 5,
                param_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        };

        let (cyc, cog, funcs, lines) = aggregate_complexity(&module);
        assert_eq!(cyc, 7);
        assert_eq!(cog, 4);
        assert_eq!(funcs, 1);
        assert_eq!(lines, 5);
    }

    #[test]
    fn aggregate_complexity_multiple_functions() {
        let module = fallow_core::extract::ModuleInfo {
            file_id: fallow_core::discover::FileId(0),
            exports: vec![],
            imports: vec![],
            re_exports: vec![],
            dynamic_imports: vec![],
            dynamic_import_patterns: vec![],
            require_calls: vec![],
            package_path_references: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: vec![],
            unknown_suppression_kinds: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            flag_uses: vec![],
            class_heritage: vec![],
            injection_tokens: vec![],
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
            line_offsets: vec![0, 10, 20], // 3 lines
            complexity: vec![
                fallow_types::extract::FunctionComplexity {
                    name: "a".into(),
                    line: 1,
                    col: 0,
                    cyclomatic: 3,
                    cognitive: 2,
                    line_count: 1,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
                fallow_types::extract::FunctionComplexity {
                    name: "b".into(),
                    line: 2,
                    col: 0,
                    cyclomatic: 5,
                    cognitive: 8,
                    line_count: 2,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
            ],
        };

        let (cyc, cog, funcs, lines) = aggregate_complexity(&module);
        assert_eq!(cyc, 8);
        assert_eq!(cog, 10);
        assert_eq!(funcs, 2);
        assert_eq!(lines, 3);
    }

    #[test]
    fn count_unused_exports_empty() {
        let exports: Vec<fallow_core::results::UnusedExportFinding> = vec![];
        let map = count_unused_exports_by_path(&exports);
        assert!(map.is_empty());
    }

    #[test]
    fn count_unused_exports_groups_by_path() {
        let exports = vec![
            fallow_core::results::UnusedExportFinding::with_actions(
                fallow_core::results::UnusedExport {
                    path: std::path::PathBuf::from("/src/a.ts"),
                    export_name: "foo".into(),
                    is_type_only: false,
                    line: 1,
                    col: 0,
                    span_start: 0,
                    is_re_export: false,
                },
            ),
            fallow_core::results::UnusedExportFinding::with_actions(
                fallow_core::results::UnusedExport {
                    path: std::path::PathBuf::from("/src/a.ts"),
                    export_name: "bar".into(),
                    is_type_only: false,
                    line: 5,
                    col: 0,
                    span_start: 40,
                    is_re_export: false,
                },
            ),
            fallow_core::results::UnusedExportFinding::with_actions(
                fallow_core::results::UnusedExport {
                    path: std::path::PathBuf::from("/src/b.ts"),
                    export_name: "baz".into(),
                    is_type_only: false,
                    line: 1,
                    col: 0,
                    span_start: 0,
                    is_re_export: false,
                },
            ),
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
            fallow_core::graph::ExportSymbol {
                name: fallow_core::extract::ExportName::Named("a".into()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                references: vec![],
                members: vec![],
            },
            fallow_core::graph::ExportSymbol {
                name: fallow_core::extract::ExportName::Named("b".into()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                references: vec![],
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

        let exports = vec![fallow_core::graph::ExportSymbol {
            name: fallow_core::extract::ExportName::Named("a".into()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: fallow_core::extract::VisibilityTag::None,
            span: oxc_span::Span::empty(0),
            references: vec![],
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

        let exports = vec![fallow_core::graph::ExportSymbol {
            name: fallow_core::extract::ExportName::Named("used".into()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: fallow_core::extract::VisibilityTag::None,
            span: oxc_span::Span::empty(0),
            references: vec![],
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
        let exports = vec![fallow_core::results::UnusedExportFinding::with_actions(
            fallow_core::results::UnusedExport {
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
        files: &[fallow_core::discover::DiscoveredFile],
        entry_point_paths: &[std::path::PathBuf],
        resolved_modules: &[fallow_core::resolve::ResolvedModule],
    ) -> fallow_core::graph::ModuleGraph {
        let entry_points: Vec<fallow_core::discover::EntryPoint> = entry_point_paths
            .iter()
            .map(|p| fallow_core::discover::EntryPoint {
                path: p.clone(),
                source: fallow_core::discover::EntryPointSource::PackageJsonMain,
            })
            .collect();
        fallow_core::graph::ModuleGraph::build(resolved_modules, &entry_points, files)
    }

    /// Helper to create a `ModuleInfo` with given complexity and line count.
    fn make_module_info(
        file_id: u32,
        line_count: usize,
        functions: Vec<fallow_types::extract::FunctionComplexity>,
    ) -> fallow_core::extract::ModuleInfo {
        fallow_core::extract::ModuleInfo {
            file_id: fallow_core::discover::FileId(file_id),
            exports: vec![],
            imports: vec![],
            re_exports: vec![],
            dynamic_imports: vec![],
            dynamic_import_patterns: vec![],
            require_calls: vec![],
            package_path_references: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: vec![],
            unknown_suppression_kinds: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            line_offsets: (0..line_count).map(|i| (i * 10) as u32).collect(),
            complexity: functions,
            flag_uses: vec![],
            class_heritage: vec![],
            injection_tokens: vec![],
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
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
        }
    }

    #[test]
    fn file_score_crap_concern_tracks_crap_risk_bands() {
        assert!((file_score_crap_concern(0.0) - 0.0).abs() < f64::EPSILON);
        assert!((file_score_crap_concern(15.0) - 45.0).abs() < f64::EPSILON);
        assert!((file_score_crap_concern(CRAP_THRESHOLD) - 75.0).abs() < f64::EPSILON);
        assert!((file_score_crap_concern(100.0) - 100.0).abs() < f64::EPSILON);
        assert!((file_score_crap_concern(552.0) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn file_score_concern_axis_labels_dominant_signal() {
        let risk_driven = make_file_score("/src/risk.ts", 84.8, 552.0);
        assert_eq!(
            file_score_concern_axis(&risk_driven),
            FileScoreConcern::Risk
        );
        assert_eq!(file_score_concern_axis(&risk_driven).label(), "risk");

        let structure_driven = make_file_score("/src/structure.ts", 30.0, 8.0);
        assert_eq!(
            file_score_concern_axis(&structure_driven),
            FileScoreConcern::Structural
        );
        assert_eq!(
            file_score_concern_axis(&structure_driven).label(),
            "structure"
        );

        let no_risk = make_file_score("/src/clean.ts", 100.0, 0.0);
        assert_eq!(
            file_score_concern_axis(&no_risk),
            FileScoreConcern::Structural
        );
    }

    #[test]
    fn file_score_triage_sort_prioritizes_high_crap_over_slightly_lower_mi() {
        let low_mi_low_risk = make_file_score("/src/low-mi-low-risk.ts", 81.7, 2.0);
        let higher_mi_high_risk = make_file_score("/src/higher-mi-high-risk.ts", 84.8, 552.0);

        let mut scores = [low_mi_low_risk, higher_mi_high_risk];
        scores.sort_by(compare_file_score_triage);

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
        scores.sort_by(compare_file_score_triage);

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

        scores.sort_by(compare_file_score_triage);

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
        let files: Vec<fallow_core::discover::DiscoveredFile> = vec![];
        let graph = build_test_graph(&files, &[], &[]);
        let modules: Vec<fallow_core::extract::ModuleInfo> = vec![];
        let file_paths = rustc_hash::FxHashMap::default();

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
        let modules: Vec<fallow_core::extract::ModuleInfo> = vec![];
        let file_paths = rustc_hash::FxHashMap::default();

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: None,
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
        let files = vec![fallow_core::discover::DiscoveredFile {
            id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_core::resolve::ResolvedModule {
            file_id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            exports: vec![fallow_types::extract::ExportInfo {
                name: fallow_core::extract::ExportName::Named("foo".into()),
                local_name: None,
                is_type_only: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }],
            ..Default::default()
        }];

        let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);

        let modules = vec![make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "foo".into(),
                line: 1,
                col: 0,
                cyclomatic: 5,
                cognitive: 3,
                line_count: 10,
                param_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
        let files = vec![fallow_core::discover::DiscoveredFile {
            id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 50,
        }];

        let resolved_modules = vec![fallow_core::resolve::ResolvedModule {
            file_id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);

        let modules = vec![make_module_info(0, 5, vec![])];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
            fallow_core::discover::DiscoveredFile {
                id: fallow_core::discover::FileId(0),
                path: path_a.clone(),
                size_bytes: 100,
            },
            fallow_core::discover::DiscoveredFile {
                id: fallow_core::discover::FileId(1),
                path: path_b.clone(),
                size_bytes: 100,
            },
        ];

        let resolved_modules = vec![
            fallow_core::resolve::ResolvedModule {
                file_id: fallow_core::discover::FileId(0),
                path: path_a,
                ..Default::default()
            },
            fallow_core::resolve::ResolvedModule {
                file_id: fallow_core::discover::FileId(1),
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
                    line: 1,
                    col: 0,
                    cyclomatic: 2,
                    cognitive: 1,
                    line_count: 10,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
            make_module_info(
                1,
                10,
                vec![fallow_types::extract::FunctionComplexity {
                    name: "fn_b".into(),
                    line: 1,
                    col: 0,
                    cyclomatic: 3,
                    cognitive: 2,
                    line_count: 10,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
        ];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);
        file_paths.insert(fallow_core::discover::FileId(1), &files[1].path);

        let path_b_check = std::path::PathBuf::from("/src/b.ts");
        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(path_b);

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
            fallow_core::discover::DiscoveredFile {
                id: fallow_core::discover::FileId(0),
                path: path_a.clone(),
                size_bytes: 100,
            },
            fallow_core::discover::DiscoveredFile {
                id: fallow_core::discover::FileId(1),
                path: path_b.clone(),
                size_bytes: 100,
            },
        ];

        let resolved_modules = vec![
            fallow_core::resolve::ResolvedModule {
                file_id: fallow_core::discover::FileId(0),
                path: path_a.clone(),
                ..Default::default()
            },
            fallow_core::resolve::ResolvedModule {
                file_id: fallow_core::discover::FileId(1),
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
                    line: 1,
                    col: 0,
                    cyclomatic: 30,
                    cognitive: 20,
                    line_count: 10,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
            make_module_info(
                1,
                100,
                vec![fallow_types::extract::FunctionComplexity {
                    name: "simple_fn".into(),
                    line: 1,
                    col: 0,
                    cyclomatic: 1,
                    cognitive: 0,
                    line_count: 100,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
        ];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);
        file_paths.insert(fallow_core::discover::FileId(1), &files[1].path);

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
        let files = vec![fallow_core::discover::DiscoveredFile {
            id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_core::resolve::ResolvedModule {
            file_id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            exports: vec![fallow_types::extract::ExportInfo {
                name: fallow_core::extract::ExportName::Named("orphan".into()),
                local_name: None,
                is_type_only: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }],
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "orphan".into(),
                line: 1,
                col: 0,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 10,
                param_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_files.push(
            fallow_types::output_dead_code::UnusedFileFinding::with_actions(
                fallow_types::results::UnusedFile {
                    path: path_a.clone(),
                },
            ),
        );

        let output = fallow_core::AnalysisOutput {
            results,
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
    fn compute_file_scores_tracks_top_complex_functions() {
        let path_a = std::path::PathBuf::from("/src/complex.ts");
        let files = vec![fallow_core::discover::DiscoveredFile {
            id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 500,
        }];

        let resolved_modules = vec![fallow_core::resolve::ResolvedModule {
            file_id: fallow_core::discover::FileId(0),
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
                    line: 1,
                    col: 0,
                    cyclomatic: 10,
                    cognitive: 20,
                    line_count: 10,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
                fallow_types::extract::FunctionComplexity {
                    name: "medium".into(),
                    line: 11,
                    col: 0,
                    cyclomatic: 5,
                    cognitive: 10,
                    line_count: 10,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
                fallow_types::extract::FunctionComplexity {
                    name: "low".into(),
                    line: 21,
                    col: 0,
                    cyclomatic: 2,
                    cognitive: 5,
                    line_count: 10,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
                fallow_types::extract::FunctionComplexity {
                    name: "trivial".into(),
                    line: 31,
                    col: 0,
                    cyclomatic: 1,
                    cognitive: 1,
                    line_count: 10,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                },
            ],
        )];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
    fn compute_file_scores_with_circular_deps() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let path_b = std::path::PathBuf::from("/src/b.ts");
        let files = vec![
            fallow_core::discover::DiscoveredFile {
                id: fallow_core::discover::FileId(0),
                path: path_a.clone(),
                size_bytes: 100,
            },
            fallow_core::discover::DiscoveredFile {
                id: fallow_core::discover::FileId(1),
                path: path_b.clone(),
                size_bytes: 100,
            },
        ];

        let resolved_modules = vec![
            fallow_core::resolve::ResolvedModule {
                file_id: fallow_core::discover::FileId(0),
                path: path_a.clone(),
                ..Default::default()
            },
            fallow_core::resolve::ResolvedModule {
                file_id: fallow_core::discover::FileId(1),
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
                    line: 1,
                    col: 0,
                    cyclomatic: 2,
                    cognitive: 1,
                    line_count: 10,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
            make_module_info(
                1,
                10,
                vec![fallow_types::extract::FunctionComplexity {
                    name: "fn_b".into(),
                    line: 1,
                    col: 0,
                    cyclomatic: 3,
                    cognitive: 2,
                    line_count: 10,
                    param_count: 0,
                    source_hash: None,
                    contributions: Vec::new(),
                }],
            ),
        ];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);
        file_paths.insert(fallow_core::discover::FileId(1), &files[1].path);

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

        let output = fallow_core::AnalysisOutput {
            results,
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
    fn compute_file_scores_analysis_counts_unused_exports_and_types() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let files = vec![fallow_core::discover::DiscoveredFile {
            id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_core::resolve::ResolvedModule {
            file_id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: fallow_core::extract::ExportName::Named("foo".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: fallow_core::extract::VisibilityTag::None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: fallow_core::extract::ExportName::Named("bar".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: fallow_core::extract::VisibilityTag::None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let mut module = make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_a".into(),
                line: 1,
                col: 0,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 10,
                param_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        );
        module.exports = vec![
            fallow_types::extract::ExportInfo {
                name: fallow_core::extract::ExportName::Named("foo".into()),
                local_name: None,
                is_type_only: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
            fallow_types::extract::ExportInfo {
                name: fallow_core::extract::ExportName::Named("bar".into()),
                local_name: None,
                is_type_only: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
        ];
        let modules = vec![module];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);

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

        let output = fallow_core::AnalysisOutput {
            results,
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
    fn total_exports_counts_graph_modules_not_extraction_modules() {
        let path_a = std::path::PathBuf::from("/src/a.ts");
        let files = vec![fallow_core::discover::DiscoveredFile {
            id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_core::resolve::ResolvedModule {
            file_id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: fallow_core::extract::ExportName::Named("foo".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: fallow_core::extract::VisibilityTag::None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: fallow_core::extract::ExportName::Named("bar".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: fallow_core::extract::VisibilityTag::None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: fallow_core::extract::ExportName::Named("baz".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: fallow_core::extract::VisibilityTag::None,
                    span: oxc_span::Span::new(0, 0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let mut module = make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_a".into(),
                line: 1,
                col: 0,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 10,
                param_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        );
        module.exports = vec![
            fallow_types::extract::ExportInfo {
                name: fallow_core::extract::ExportName::Named("foo".into()),
                local_name: None,
                is_type_only: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
            fallow_types::extract::ExportInfo {
                name: fallow_core::extract::ExportName::Named("bar".into()),
                local_name: None,
                is_type_only: false,
                visibility: fallow_core::extract::VisibilityTag::None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
        ];
        let modules = vec![module];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);

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

        let output = fallow_core::AnalysisOutput {
            results,
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
        let files = vec![fallow_core::discover::DiscoveredFile {
            id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_core::resolve::ResolvedModule {
            file_id: fallow_core::discover::FileId(0),
            path: path_a,
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_a".into(),
                line: 1,
                col: 0,
                cyclomatic: 2,
                cognitive: 1,
                line_count: 10,
                param_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let file_paths: rustc_hash::FxHashMap<fallow_core::discover::FileId, &std::path::PathBuf> =
            rustc_hash::FxHashMap::default();

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
        let files = vec![fallow_core::discover::DiscoveredFile {
            id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_core::resolve::ResolvedModule {
            file_id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);

        let modules = vec![make_module_info(
            0,
            100,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn".into(),
                line: 1,
                col: 0,
                cyclomatic: 7,
                cognitive: 3,
                line_count: 100,
                param_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
        let files = vec![fallow_core::discover::DiscoveredFile {
            id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_core::resolve::ResolvedModule {
            file_id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: fallow_core::extract::ExportName::Named("a".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: fallow_core::extract::VisibilityTag::None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: fallow_core::extract::ExportName::Named("b".into()),
                    local_name: None,
                    is_type_only: false,
                    visibility: fallow_core::extract::VisibilityTag::None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: fallow_core::extract::ExportName::Named("T".into()),
                    local_name: None,
                    is_type_only: true,
                    visibility: fallow_core::extract::VisibilityTag::None,
                    span: oxc_span::Span::empty(0),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_a".into(),
                line: 1,
                col: 0,
                cyclomatic: 2,
                cognitive: 1,
                line_count: 10,
                param_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
        let files = vec![fallow_core::discover::DiscoveredFile {
            id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        }];

        let resolved_modules = vec![fallow_core::resolve::ResolvedModule {
            file_id: fallow_core::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        }];

        let graph = build_test_graph(&files, &[], &resolved_modules);

        let modules = vec![make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "trivial".into(),
                line: 1,
                col: 0,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 10,
                param_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];

        let mut file_paths: rustc_hash::FxHashMap<
            fallow_core::discover::FileId,
            &std::path::PathBuf,
        > = rustc_hash::FxHashMap::default();
        file_paths.insert(fallow_core::discover::FileId(0), &files[0].path);

        let output = fallow_core::AnalysisOutput {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: Some(graph),
            modules: None,
            files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
        };

        let result = compute_file_scores(
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
            line: 1,
            col: 0,
            cyclomatic,
            cognitive: 0,
            line_count: 10,
            param_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }
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

    #[test]
    fn istanbul_crap_with_coverage_data() {
        let funcs = vec![make_fn_complexity(10)];
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("test_fn".to_string(), 1, 0), 80.0);
        let file_cov = IstanbulFileCoverage { functions };
        let result = compute_crap_scores_istanbul(&funcs, Some(&file_cov), false);
        assert!((result.max_crap - 10.8).abs() < 0.1);
        assert_eq!(result.above_threshold, 0);
    }

    #[test]
    fn istanbul_crap_falls_back_to_binary_when_no_match() {
        let funcs = vec![make_fn_complexity(6)];
        let file_cov = IstanbulFileCoverage {
            functions: rustc_hash::FxHashMap::default(),
        };
        let result = compute_crap_scores_istanbul(&funcs, Some(&file_cov), false);
        assert!((result.max_crap - 42.0).abs() < f64::EPSILON);
        assert_eq!(result.above_threshold, 1);
    }

    #[test]
    fn istanbul_crap_falls_back_to_binary_when_no_file_coverage() {
        let funcs = vec![make_fn_complexity(5)];
        let result = compute_crap_scores_istanbul(&funcs, None, true);
        assert!((result.max_crap - 5.0).abs() < f64::EPSILON);
        assert_eq!(result.above_threshold, 0);
    }

    #[test]
    fn istanbul_crap_zero_coverage_matches_binary_untested() {
        let funcs = vec![make_fn_complexity(5)];
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("test_fn".to_string(), 1, 0), 0.0);
        let file_cov = IstanbulFileCoverage { functions };
        let result = compute_crap_scores_istanbul(&funcs, Some(&file_cov), false);
        assert!((result.max_crap - 30.0).abs() < f64::EPSILON);
        assert_eq!(result.above_threshold, 1);
    }

    #[test]
    fn estimated_crap_direct_test_reference() {
        let funcs = vec![make_fn_complexity(10)];
        let mut refs = rustc_hash::FxHashSet::default();
        refs.insert("test_fn".to_string());
        let result = compute_crap_scores_estimated(
            &funcs,
            &refs,
            true,
            crate::health_types::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.above_threshold);
        assert!((max - 10.3).abs() < 0.1);
        assert_eq!(above, 0);
    }

    #[test]
    fn estimated_crap_indirect_test_reachable() {
        let funcs = vec![make_fn_complexity(10)];
        let refs = rustc_hash::FxHashSet::default();
        let result = compute_crap_scores_estimated(
            &funcs,
            &refs,
            true,
            crate::health_types::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.above_threshold);
        assert!((max - 31.6).abs() < 0.1);
        assert_eq!(above, 1);
    }

    #[test]
    fn estimated_crap_untested_file() {
        let funcs = vec![make_fn_complexity(5)];
        let refs = rustc_hash::FxHashSet::default();
        let result = compute_crap_scores_estimated(
            &funcs,
            &refs,
            false,
            crate::health_types::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.above_threshold);
        assert!((max - 30.0).abs() < f64::EPSILON);
        assert_eq!(above, 1);
    }

    #[test]
    fn estimated_crap_low_complexity_direct_ref() {
        let funcs = vec![make_fn_complexity(2)];
        let mut refs = rustc_hash::FxHashSet::default();
        refs.insert("test_fn".to_string());
        let result = compute_crap_scores_estimated(
            &funcs,
            &refs,
            true,
            crate::health_types::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.above_threshold);
        assert!(max < 3.0);
        assert_eq!(above, 0);
    }

    #[test]
    fn estimated_crap_empty() {
        let refs = rustc_hash::FxHashSet::default();
        let result = compute_crap_scores_estimated(
            &[],
            &refs,
            true,
            crate::health_types::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.above_threshold);
        assert!((max).abs() < f64::EPSILON);
        assert_eq!(above, 0);
    }

    fn make_export(name: &str, is_type_only: bool) -> fallow_core::graph::ExportSymbol {
        fallow_core::graph::ExportSymbol {
            name: fallow_types::extract::ExportName::Named(name.into()),
            is_type_only,
            is_side_effect_used: false,
            visibility: fallow_core::extract::VisibilityTag::None,
            span: oxc_span::Span::default(),
            references: vec![],
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

        let coverage = load_istanbul_coverage(&coverage_path, None, None).unwrap();
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

        let coverage = load_istanbul_coverage(&coverage_path, None, None).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("handleClick", 40, 0), Some(100.0));
        assert_eq!(file_coverage.lookup("handleClick", 22, 13), Some(100.0));
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

        let coverage = load_istanbul_coverage(&coverage_path, None, None).unwrap();
        let canonical_source = dunce::canonicalize(&source_path).unwrap();
        let file_coverage = coverage.get(&canonical_source).unwrap();

        assert_eq!(file_coverage.lookup("elementsFrom", 1, 28), Some(100.0));
    }

    #[test]
    fn istanbul_lookup_exact_match() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 10, 0), 85.0);
        let fc = IstanbulFileCoverage { functions };
        assert!((fc.lookup("handleClick", 10, 0).unwrap() - 85.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_fuzzy_match_within_offset() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 10, 0), 72.0);
        let fc = IstanbulFileCoverage { functions };
        assert!((fc.lookup("handleClick", 11, 0).unwrap() - 72.0).abs() < f64::EPSILON);
        assert!((fc.lookup("handleClick", 12, 0).unwrap() - 72.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_fuzzy_match_outside_offset() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 10, 0), 72.0);
        let fc = IstanbulFileCoverage { functions };
        assert!(fc.lookup("handleClick", 13, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_name_mismatch() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 10, 0), 85.0);
        let fc = IstanbulFileCoverage { functions };
        assert!(fc.lookup("handleSubmit", 10, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_empty() {
        let fc = IstanbulFileCoverage {
            functions: rustc_hash::FxHashMap::default(),
        };
        assert!(fc.lookup("anything", 1, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_fuzzy_picks_closest() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("render".to_string(), 8, 0), 60.0);
        functions.insert(("render".to_string(), 12, 0), 90.0);
        let fc = IstanbulFileCoverage { functions };
        let result = fc.lookup("render", 10, 0);
        assert!(result.is_some());
        let pct = result.unwrap();
        assert!((pct - 60.0).abs() < f64::EPSILON || (pct - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_single_candidate() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 28, 0), 75.0);
        let fc = IstanbulFileCoverage { functions };
        assert!((fc.lookup("myHandler", 28, 0).unwrap() - 75.0).abs() < f64::EPSILON);
        assert!((fc.lookup("myHandler", 30, 0).unwrap() - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_rejects_nearby_far_column() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 4, 28), 75.0);
        let fc = IstanbulFileCoverage { functions };

        assert!(fc.lookup("declaredHelper", 3, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_picks_closest_when_lines_differ() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 28, 0), 75.0);
        functions.insert(("(anonymous_1)".to_string(), 29, 0), 50.0);
        let fc = IstanbulFileCoverage { functions };
        assert!((fc.lookup("myHandler", 28, 0).unwrap() - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_picks_closest_by_col_on_same_line() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 1, 23), 90.0); // outer
        functions.insert(("(anonymous_1)".to_string(), 1, 43), 10.0); // inner
        let fc = IstanbulFileCoverage { functions };
        assert!((fc.lookup("<arrow>", 1, 43).unwrap() - 10.0).abs() < f64::EPSILON);
        assert!((fc.lookup("<arrow>", 1, 23).unwrap() - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_bails_only_on_true_tie() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 27, 0), 75.0);
        functions.insert(("(anonymous_1)".to_string(), 29, 0), 50.0);
        let fc = IstanbulFileCoverage { functions };
        assert!(fc.lookup("myHandler", 28, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_anonymous_fallback_outside_offset() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("(anonymous_0)".to_string(), 28, 0), 75.0);
        let fc = IstanbulFileCoverage { functions };
        assert!(fc.lookup("myHandler", 31, 0).is_none());
    }

    #[test]
    fn istanbul_lookup_named_match_beats_nearby_anonymous() {
        let mut functions = rustc_hash::FxHashMap::default();
        functions.insert(("handleClick".to_string(), 10, 0), 90.0);
        functions.insert(("(anonymous_7)".to_string(), 11, 0), 10.0);
        let fc = IstanbulFileCoverage { functions };
        assert!((fc.lookup("handleClick", 10, 0).unwrap() - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn build_test_refs_empty() {
        let exports: Vec<fallow_core::graph::ExportSymbol> = vec![];
        let modules: Vec<fallow_core::graph::ModuleNode> = vec![];
        let refs = build_test_referenced_exports(&exports, &modules);
        assert!(refs.is_empty());
    }

    #[test]
    fn build_test_refs_empty_inputs() {
        let exports: Vec<fallow_core::graph::ExportSymbol> = vec![];
        let modules: Vec<fallow_core::graph::ModuleNode> = vec![];
        let refs = build_test_referenced_exports(&exports, &modules);
        assert!(refs.is_empty());
    }

    #[test]
    fn istanbul_crap_empty_complexity() {
        let result = compute_crap_scores_istanbul(&[], None, false);
        assert!((result.max_crap).abs() < f64::EPSILON);
        assert_eq!(result.above_threshold, 0);
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
        let file_cov = IstanbulFileCoverage { functions };
        let result = compute_crap_scores_istanbul(&funcs, Some(&file_cov), true);
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
        let result = compute_crap_scores_estimated(
            &funcs,
            &refs,
            true,
            crate::health_types::CoverageSource::Estimated,
        );
        let (max, above) = (result.max_crap, result.above_threshold);
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

    #[test]
    fn validate_coverage_root_accepts_posix_absolute() {
        assert!(
            validate_coverage_root_absolute(Some(std::path::Path::new("/ci/workspace"))).is_ok()
        );
        assert!(
            validate_coverage_root_absolute(Some(std::path::Path::new("/home/runner/work/myapp")))
                .is_ok()
        );
    }

    #[test]
    fn validate_coverage_root_rejects_truly_relative() {
        assert!(validate_coverage_root_absolute(Some(std::path::Path::new("src"))).is_err());
        assert!(validate_coverage_root_absolute(Some(std::path::Path::new("./coverage"))).is_err());
        assert!(validate_coverage_root_absolute(Some(std::path::Path::new("a/b/c"))).is_err());
    }

    #[test]
    fn validate_coverage_root_accepts_none() {
        assert!(validate_coverage_root_absolute(None).is_ok());
    }

    /// Regression for issue #561: `Path::is_absolute` on Windows requires a
    /// drive letter, so a POSIX-style `--coverage-root /ci/workspace` (the
    /// shape Linux-CI-generated Istanbul data uses) was rejected with exit 2
    /// when fallow ran on Windows. The prefix-strip in `load_istanbul_coverage`
    /// is component-wise and works on root-anchored POSIX paths regardless of
    /// host OS, so the validator must accept `has_root` rather than
    /// `is_absolute`.
    #[cfg(windows)]
    #[test]
    fn validate_coverage_root_accepts_posix_absolute_on_windows() {
        assert!(
            validate_coverage_root_absolute(Some(std::path::Path::new(r"/ci/workspace"))).is_ok(),
            "POSIX-absolute prefix must be accepted on Windows: coverage data from Linux CI uses this exact shape"
        );
    }

    #[cfg(windows)]
    #[test]
    fn validate_coverage_root_accepts_windows_absolute() {
        assert!(
            validate_coverage_root_absolute(Some(std::path::Path::new(r"C:\ci\workspace"))).is_ok()
        );
    }
}
