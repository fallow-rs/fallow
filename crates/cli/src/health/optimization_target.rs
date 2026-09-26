//! Speed-work inputs for runtime hot paths.
//!
//! `importance` ranks the risk of a change. The optimization target ranks where
//! speed work pays off: how often a function runs, multiplied by the work each
//! call does. The per-call work comes from V8 block counts when the dump has
//! them, and from static cognitive complexity when it does not.

use std::path::{Component, Path, PathBuf};

use fallow_output::{
    RuntimeCoverageCostBasis, RuntimeCoverageHotPath, RuntimeCoverageOptimizationTarget,
};
use rustc_hash::FxHashMap;

/// Block totals per function start, keyed by the canonical source path and the
/// 1-based start line.
pub type InnerIterationIndex = FxHashMap<(PathBuf, u32), InnerIterations>;

/// Warning code for hot paths that have no static counterpart to join with.
pub const UNMATCHED_WARNING_CODE: &str = "optimization_target_unmatched";

/// Block execution totals for one function, summed over all coverage dumps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InnerIterations {
    /// Sum of the calls of the function.
    pub calls: u64,
    /// Sum of the peak execution count of one block in the function.
    pub peak_block_executions: u64,
}

impl InnerIterations {
    /// Read the block totals of one V8 function. `None` when the function has
    /// no block counts, was never called, or is the script wrapper.
    #[must_use]
    pub fn from_v8_function(function: &fallow_v8_coverage::FunctionCoverage) -> Option<Self> {
        if !function.is_block_coverage {
            return None;
        }
        let outer = function.ranges.first()?;
        // V8 reports the top-level script body as an unnamed function at
        // offset 0. It has no static counterpart, and it starts on the same
        // line as a function declared on line 1.
        if function.function_name.is_empty() && outer.start_offset == 0 {
            return None;
        }
        let calls = outer.count;
        if calls == 0 {
            return None;
        }
        // The function body itself runs once per call, so the peak never
        // drops below the call count, even when every inner block is a branch
        // that did not run.
        let peak_block_executions = function
            .ranges
            .iter()
            .skip(1)
            .map(|range| range.count)
            .fold(calls, u64::max);
        Some(Self {
            calls,
            peak_block_executions,
        })
    }

    /// Add the totals of the same function from another dump.
    pub const fn add(&mut self, other: Self) {
        self.calls = self.calls.saturating_add(other.calls);
        self.peak_block_executions = self
            .peak_block_executions
            .saturating_add(other.peak_block_executions);
    }
}

/// Static complexity of one function, from the local analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCost {
    pub cognitive: u16,
    pub cyclomatic: u16,
    pub line_count: u32,
}

/// One V8 function start in original source coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionStart {
    /// Canonical source path.
    pub path: PathBuf,
    /// 1-based start line.
    pub line: u32,
    /// 0-based start column.
    pub column: u32,
    pub inner: InnerIterations,
}

/// Add the function starts of one script to the index.
///
/// Static analysis keys a function by its start line only. When several V8
/// functions start on one line, the first one on the line (the outer function)
/// represents the line, so a nested callback does not blend into its parent.
pub fn record_function_starts(index: &mut InnerIterationIndex, mut starts: Vec<FunctionStart>) {
    starts.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line.cmp(&right.line))
            .then(left.column.cmp(&right.column))
    });
    starts.dedup_by(|later, first| later.path == first.path && later.line == first.line);
    for start in starts {
        index
            .entry((start.path, start.line))
            .or_default()
            .add(start.inner);
    }
}

/// Function starts of a script that has no source map, read from the script
/// source on disk. Dependency scripts are skipped because static analysis never
/// reports hot paths for them.
pub fn raw_script_function_starts(
    path: &Path,
    functions: &[fallow_v8_coverage::FunctionCoverage],
) -> Vec<FunctionStart> {
    if path
        .components()
        .any(|component| component == Component::Normal("node_modules".as_ref()))
    {
        return Vec::new();
    }
    let measured = functions
        .iter()
        .filter_map(|function| {
            let start = function.ranges.first()?.start_offset;
            InnerIterations::from_v8_function(function).map(|inner| (start, inner))
        })
        .collect::<Vec<_>>();
    if measured.is_empty() {
        return Vec::new();
    }
    let Ok(source) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let lines = fallow_v8_coverage::LineOffsetTable::from_source(&source);
    measured
        .into_iter()
        .map(|(offset, inner)| {
            let position = lines.position(offset);
            FunctionStart {
                path: canonical.clone(),
                line: position.line,
                column: position.column,
                inner,
            }
        })
        .collect()
}

/// Static facts of one function that a hot path joins with by `stable_id`.
#[derive(Debug, Clone)]
pub struct StaticTarget {
    /// Absolute module path as discovered.
    pub path: PathBuf,
    /// 1-based start line.
    pub line: u32,
    pub cost: StaticCost,
    /// True when another static function starts on the same line. Block counts
    /// are then ambiguous, so the score uses cognitive complexity.
    pub shares_line: bool,
}

/// Fill `optimization_target` on each hot path. Returns the count of hot paths
/// that have no `stable_id` or no static counterpart.
pub fn attach_optimization_targets(
    hot_paths: &mut [RuntimeCoverageHotPath],
    statics: &FxHashMap<String, StaticTarget>,
    inner_index: &InnerIterationIndex,
) -> usize {
    let mut canonical_paths: FxHashMap<&Path, PathBuf> = FxHashMap::default();
    let mut unmatched = 0;
    for hot in hot_paths {
        let Some(target) = hot.stable_id.as_ref().and_then(|id| statics.get(id)) else {
            unmatched += 1;
            continue;
        };
        let inner = if target.shares_line || inner_index.is_empty() {
            None
        } else {
            let canonical = canonical_paths
                .entry(target.path.as_path())
                .or_insert_with(|| {
                    dunce::canonicalize(&target.path).unwrap_or_else(|_| target.path.clone())
                });
            inner_index.get(&(canonical.clone(), target.line)).copied()
        };
        hot.optimization_target = Some(optimization_target(hot.invocations, target.cost, inner));
    }
    unmatched
}

/// Number of decimals kept for `inner_iterations_per_call`, so the JSON value
/// stays readable. The score uses the exact integer totals.
const RATIO_DECIMALS_SCALE: f64 = 100.0;

/// Build the optimization target of one hot function.
#[must_use]
pub fn optimization_target(
    invocations: u64,
    cost: StaticCost,
    inner: Option<InnerIterations>,
) -> RuntimeCoverageOptimizationTarget {
    let inner = inner.filter(|inner| inner.calls > 0);
    let (cost_score, cost_basis, inner_iterations_per_call) = match inner {
        Some(inner) => (
            scaled_score(invocations, inner),
            RuntimeCoverageCostBasis::InnerIterations,
            Some(rounded_ratio(inner)),
        ),
        None => (
            invocations.saturating_mul(u64::from(cost.cognitive)),
            RuntimeCoverageCostBasis::Cognitive,
            None,
        ),
    };
    RuntimeCoverageOptimizationTarget {
        cost_score,
        cost_basis,
        cognitive: cost.cognitive,
        cyclomatic: cost.cyclomatic,
        line_count: cost.line_count,
        inner_iterations_per_call,
    }
}

/// `invocations * peak / calls`, rounded to the nearest integer.
fn scaled_score(invocations: u64, inner: InnerIterations) -> u64 {
    let numerator = u128::from(invocations) * u128::from(inner.peak_block_executions);
    let calls = u128::from(inner.calls);
    let rounded = (numerator + calls / 2) / calls;
    u64::try_from(rounded).unwrap_or(u64::MAX)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "block counts above 2^52 lose only digits far below the two kept decimals"
)]
fn rounded_ratio(inner: InnerIterations) -> f64 {
    let ratio = inner.peak_block_executions as f64 / inner.calls as f64;
    (ratio * RATIO_DECIMALS_SCALE).round() / RATIO_DECIMALS_SCALE
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_v8_coverage::{CoverageRange, FunctionCoverage};

    const COST: StaticCost = StaticCost {
        cognitive: 4,
        cyclomatic: 3,
        line_count: 12,
    };

    fn range(count: u64) -> CoverageRange {
        CoverageRange {
            start_offset: 0,
            end_offset: 1,
            count,
        }
    }

    fn function(block: bool, counts: &[u64]) -> FunctionCoverage {
        FunctionCoverage {
            function_name: "resolve".to_owned(),
            ranges: counts.iter().copied().map(range).collect(),
            is_block_coverage: block,
        }
    }

    #[test]
    fn inner_iterations_take_the_peak_block_per_call() {
        let inner = InnerIterations::from_v8_function(&function(true, &[600, 1800, 600]));
        assert_eq!(
            inner,
            Some(InnerIterations {
                calls: 600,
                peak_block_executions: 1800,
            })
        );
    }

    #[test]
    fn untaken_branches_do_not_drop_the_peak_below_the_calls() {
        let inner = InnerIterations::from_v8_function(&function(true, &[50, 0, 0]));
        assert_eq!(inner.map(|inner| inner.peak_block_executions), Some(50));
    }

    #[test]
    fn function_level_or_uncalled_coverage_has_no_inner_iterations() {
        assert_eq!(
            InnerIterations::from_v8_function(&function(false, &[600, 1800])),
            None
        );
        assert_eq!(
            InnerIterations::from_v8_function(&function(true, &[0, 0])),
            None
        );
        assert_eq!(
            InnerIterations::from_v8_function(&function(true, &[])),
            None
        );
    }

    #[test]
    fn script_wrapper_has_no_inner_iterations() {
        let mut wrapper = function(true, &[1, 200]);
        wrapper.function_name = String::new();
        assert_eq!(InnerIterations::from_v8_function(&wrapper), None);
    }

    #[test]
    fn block_counts_set_the_score_when_present() {
        let target = optimization_target(
            600,
            COST,
            Some(InnerIterations {
                calls: 600,
                peak_block_executions: 1800,
            }),
        );
        assert_eq!(target.cost_score, 1800);
        assert_eq!(target.cost_basis, RuntimeCoverageCostBasis::InnerIterations);
        assert_eq!(target.inner_iterations_per_call, Some(3.0));
        assert_eq!(target.cognitive, 4);
        assert_eq!(target.cyclomatic, 3);
        assert_eq!(target.line_count, 12);
    }

    #[test]
    fn cognitive_sets_the_score_without_block_counts() {
        let target = optimization_target(600, COST, None);
        assert_eq!(target.cost_score, 2400);
        assert_eq!(target.cost_basis, RuntimeCoverageCostBasis::Cognitive);
        assert_eq!(target.inner_iterations_per_call, None);
    }

    #[test]
    fn score_rounds_and_ratio_keeps_two_decimals() {
        let target = optimization_target(
            10,
            COST,
            Some(InnerIterations {
                calls: 3,
                peak_block_executions: 7,
            }),
        );
        assert_eq!(target.cost_score, 23);
        assert_eq!(target.inner_iterations_per_call, Some(2.33));
    }

    fn hot(stable_id: Option<&str>) -> RuntimeCoverageHotPath {
        RuntimeCoverageHotPath {
            id: "fallow:hot:test".to_owned(),
            stable_id: stable_id.map(str::to_owned),
            path: PathBuf::from("src/app.js"),
            function: "resolve".to_owned(),
            line: 1,
            end_line: 7,
            invocations: 600,
            percentile: 100,
            actions: Vec::new(),
            optimization_target: None,
        }
    }

    fn static_target(path: &Path, shares_line: bool) -> StaticTarget {
        StaticTarget {
            path: path.to_path_buf(),
            line: 1,
            cost: COST,
            shares_line,
        }
    }

    fn one_inner_index(path: &Path) -> InnerIterationIndex {
        let mut index = InnerIterationIndex::default();
        index.insert(
            (path.to_path_buf(), 1),
            InnerIterations {
                calls: 600,
                peak_block_executions: 1800,
            },
        );
        index
    }

    #[test]
    fn attach_joins_by_stable_id_and_counts_unmatched_hot_paths() {
        let path = PathBuf::from("/fallow-missing-dir/app.js");
        let mut statics = FxHashMap::default();
        statics.insert("fallow:fn:a".to_owned(), static_target(&path, false));
        let mut hot_paths = vec![
            hot(Some("fallow:fn:a")),
            hot(Some("fallow:fn:b")),
            hot(None),
        ];

        let unmatched =
            attach_optimization_targets(&mut hot_paths, &statics, &one_inner_index(&path));

        assert_eq!(unmatched, 2);
        let target = hot_paths[0].optimization_target.as_ref().expect("joined");
        assert_eq!(target.cost_basis, RuntimeCoverageCostBasis::InnerIterations);
        assert_eq!(target.cost_score, 1800);
        assert!(hot_paths[1].optimization_target.is_none());
        assert!(hot_paths[2].optimization_target.is_none());
    }

    #[test]
    fn attach_uses_cognitive_when_functions_share_the_start_line() {
        let path = PathBuf::from("/fallow-missing-dir/app.js");
        let mut statics = FxHashMap::default();
        statics.insert("fallow:fn:a".to_owned(), static_target(&path, true));
        let mut hot_paths = vec![hot(Some("fallow:fn:a"))];

        attach_optimization_targets(&mut hot_paths, &statics, &one_inner_index(&path));

        let target = hot_paths[0].optimization_target.as_ref().expect("joined");
        assert_eq!(target.cost_basis, RuntimeCoverageCostBasis::Cognitive);
        assert_eq!(target.cost_score, 2400);
    }

    #[test]
    fn first_function_on_a_line_represents_the_line() {
        let path = PathBuf::from("/src/app.js");
        let start = |column, peak| FunctionStart {
            path: path.clone(),
            line: 3,
            column,
            inner: InnerIterations {
                calls: 10,
                peak_block_executions: peak,
            },
        };
        let mut index = InnerIterationIndex::default();
        record_function_starts(&mut index, vec![start(20, 90), start(4, 30)]);
        record_function_starts(&mut index, vec![start(4, 50)]);

        assert_eq!(
            index.get(&(path.clone(), 3)),
            Some(&InnerIterations {
                calls: 20,
                peak_block_executions: 80,
            })
        );
    }

    #[test]
    fn dependency_scripts_are_not_read() {
        let path = PathBuf::from("/repo/node_modules/pkg/index.js");
        let starts = raw_script_function_starts(&path, &[function(true, &[5, 50])]);
        assert!(starts.is_empty());
    }

    #[test]
    fn score_saturates_instead_of_overflowing() {
        let target = optimization_target(
            u64::MAX,
            COST,
            Some(InnerIterations {
                calls: 1,
                peak_block_executions: 2,
            }),
        );
        assert_eq!(target.cost_score, u64::MAX);
        assert_eq!(
            optimization_target(u64::MAX, COST, None).cost_score,
            u64::MAX
        );
    }
}
