//! Speed-work inputs for runtime hot paths.
//!
//! `importance` ranks the risk of a change. The optimization target ranks where
//! speed work gives the largest gain: how often a function runs, multiplied by
//! the work each call does. The per-call work comes from V8 block counts when the dump has
//! them, and from static cognitive complexity when it does not.

use std::path::{Component, Path, PathBuf};

use fallow_output::{
    RuntimeCoverageCostBasis, RuntimeCoverageHotPath, RuntimeCoverageOptimizationTarget,
};
use rustc_hash::FxHashMap;

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

/// Block totals of the coverage input, ready to join with static functions.
#[derive(Debug, Default)]
pub struct InnerIterationIndex {
    /// Totals keyed by the canonical source path and the 1-based start line.
    starts: FxHashMap<(PathBuf, u32), InnerIterations>,
    /// Measured functions of scripts without a source map, keyed by the
    /// canonical script path and the UTF-16 offset of the function start. The
    /// dumps of one script add up in one entry per function. The source file
    /// is read only when a hot path joins with it.
    raw_scripts: FxHashMap<PathBuf, FxHashMap<u32, RawFunctionStart>>,
    /// Canonical path of each raw script path, so that each script path is
    /// canonicalized once, not once per dump.
    raw_canonical_paths: FxHashMap<PathBuf, PathBuf>,
    /// V8 function name of the function that represents a line of a raw
    /// script, keyed like `starts`. Empty names are not kept.
    raw_names: FxHashMap<(PathBuf, u32), String>,
}

/// One measured V8 function of a script without a source map.
#[derive(Debug, Clone)]
struct RawFunctionStart {
    /// V8 function name, empty for an anonymous function.
    name: String,
    inner: InnerIterations,
}

/// One function start of a raw script in source coordinates.
struct RawLineStart {
    line: u32,
    column: u32,
    name: String,
    inner: InnerIterations,
}

impl InnerIterationIndex {
    /// True when the coverage input has no block totals.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.starts.is_empty() && self.raw_scripts.is_empty()
    }

    /// Add the function starts of one script.
    ///
    /// Static analysis keys a function by its start line only. When several V8
    /// functions start on one line, the first one on the line (the outer
    /// function) represents the line, so a nested callback does not blend into
    /// its parent.
    pub fn record_function_starts(&mut self, mut starts: Vec<FunctionStart>) {
        starts.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.line.cmp(&right.line))
                .then(left.column.cmp(&right.column))
        });
        starts.dedup_by(|later, first| later.path == first.path && later.line == first.line);
        for start in starts {
            self.starts
                .entry((start.path, start.line))
                .or_default()
                .add(start.inner);
        }
    }

    /// Keep the measured functions of a script that has no source map.
    /// Dependency scripts are skipped because static analysis never reports
    /// hot paths for them.
    pub fn record_raw_script(
        &mut self,
        path: &Path,
        functions: &[fallow_v8_coverage::FunctionCoverage],
    ) {
        if path
            .components()
            .any(|component| component == Component::Normal("node_modules".as_ref()))
        {
            return;
        }
        let mut measured = functions
            .iter()
            .filter_map(|function| {
                let offset = function.ranges.first()?.start_offset;
                InnerIterations::from_v8_function(function).map(|inner| (offset, function, inner))
            })
            .peekable();
        if measured.peek().is_none() {
            return;
        }
        let canonical = match self.raw_canonical_paths.get(path) {
            Some(canonical) => canonical.clone(),
            None => {
                let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                self.raw_canonical_paths
                    .insert(path.to_path_buf(), canonical.clone());
                canonical
            }
        };
        let by_offset = self.raw_scripts.entry(canonical).or_default();
        for (offset, function, inner) in measured {
            by_offset
                .entry(offset)
                .and_modify(|start| start.inner.add(inner))
                .or_insert_with(|| RawFunctionStart {
                    name: function.function_name.clone(),
                    inner,
                });
        }
    }

    /// Number of kept raw-script function entries, over all scripts.
    #[cfg(test)]
    fn raw_function_entries(&self) -> usize {
        self.raw_scripts.values().map(FxHashMap::len).sum()
    }

    /// Block totals of the function `name` that starts on `line` of the file
    /// at the canonical path `canonical`. For a raw script, the V8 function
    /// name must agree with `name`, so a changed file does not join a line
    /// with another function.
    pub fn lookup(&mut self, canonical: &Path, line: u32, name: &str) -> Option<InnerIterations> {
        if let Some(functions) = self.raw_scripts.remove(canonical) {
            self.resolve_raw_script(canonical, functions);
        }
        let key = (canonical.to_path_buf(), line);
        if self
            .raw_names
            .get(&key)
            .is_some_and(|v8_name| !names_agree(v8_name, name))
        {
            return None;
        }
        self.starts.get(&key).copied()
    }

    /// Map the offsets of a raw script to lines with the source on disk. The
    /// first function on a line represents the line, as in
    /// [`Self::record_function_starts`].
    fn resolve_raw_script(
        &mut self,
        canonical: &Path,
        functions: FxHashMap<u32, RawFunctionStart>,
    ) {
        let Ok(source) = std::fs::read_to_string(canonical) else {
            return;
        };
        // Node removes a UTF-8 byte order mark before it compiles a module, so
        // the V8 offsets start after it.
        let source = source.strip_prefix('\u{FEFF}').unwrap_or(&source);
        let lines = fallow_v8_coverage::LineOffsetTable::from_source(source);
        let mut starts = functions
            .into_iter()
            .map(|(offset, start)| {
                let position = lines.position(offset);
                RawLineStart {
                    line: position.line,
                    column: position.column,
                    name: start.name,
                    inner: start.inner,
                }
            })
            .collect::<Vec<_>>();
        starts.sort_by_key(|start| (start.line, start.column));
        starts.dedup_by_key(|start| start.line);
        for start in starts {
            let key = (canonical.to_path_buf(), start.line);
            if !start.name.is_empty() {
                self.raw_names.insert(key.clone(), start.name);
            }
            self.starts.entry(key).or_default().add(start.inner);
        }
    }
}

/// Whether a V8 function name and a static function name can name the same
/// function. An empty V8 name and an anonymous static name (`<arrow>`) never
/// disagree. V8 can qualify a name (`Router.resolve`, `get size`), so only the
/// last word of each name is compared.
fn names_agree(v8_name: &str, static_name: &str) -> bool {
    fn last_word(name: &str) -> &str {
        name.rsplit(['.', ' ']).next().unwrap_or(name)
    }
    if v8_name.is_empty() || static_name.starts_with('<') {
        return true;
    }
    last_word(v8_name) == last_word(static_name)
}

/// Static facts of one function that a hot path joins with by `stable_id`.
#[derive(Debug, Clone)]
pub struct StaticTarget {
    /// Absolute module path as discovered.
    pub path: PathBuf,
    /// 1-based start line.
    pub line: u32,
    /// Function name from the static analysis.
    pub name: String,
    pub cost: StaticCost,
    /// True when another static function starts on the same line. Block counts
    /// are then ambiguous, so the score uses cognitive complexity.
    pub shares_line: bool,
}

/// Fill `optimization_target` on each hot path that has a `stable_id` with a
/// static counterpart.
pub fn attach_optimization_targets(
    hot_paths: &mut [RuntimeCoverageHotPath],
    statics: &FxHashMap<String, StaticTarget>,
    inner_index: &mut InnerIterationIndex,
) {
    let mut canonical_paths: FxHashMap<&Path, PathBuf> = FxHashMap::default();
    for hot in hot_paths {
        let Some(target) = hot.stable_id.as_ref().and_then(|id| statics.get(id)) else {
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
            inner_index.lookup(canonical, target.line, &target.name)
        };
        hot.optimization_target = Some(optimization_target(hot.invocations, target.cost, inner));
    }
}

/// Number of decimals kept for `inner_iterations_per_call`, so the JSON value
/// stays readable. The score uses the exact integer totals.
const RATIO_DECIMALS_SCALE: f64 = 100.0;

/// Lowest per-call cost on the cognitive basis. A function with cognitive
/// complexity 0 still does work on each call, as a straight-line function
/// gives `inner_iterations_per_call` 1.0 on the measured basis.
pub const MIN_COGNITIVE_COST: u16 = 1;

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
            invocations.saturating_mul(u64::from(cost.cognitive.max(MIN_COGNITIVE_COST))),
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
    fn cognitive_zero_still_costs_one_unit_per_call() {
        let straight_line = StaticCost {
            cognitive: 0,
            ..COST
        };
        let target = optimization_target(600, straight_line, None);
        assert_eq!(target.cost_score, 600);
        assert_eq!(target.cost_basis, RuntimeCoverageCostBasis::Cognitive);
        assert_eq!(target.cognitive, 0);
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
            name: "resolve".to_owned(),
            cost: COST,
            shares_line,
        }
    }

    fn one_inner_index(path: &Path) -> InnerIterationIndex {
        let mut index = InnerIterationIndex::default();
        index.record_function_starts(vec![FunctionStart {
            path: path.to_path_buf(),
            line: 1,
            column: 0,
            inner: InnerIterations {
                calls: 600,
                peak_block_executions: 1800,
            },
        }]);
        index
    }

    #[test]
    fn attach_joins_by_stable_id_and_skips_unmatched_hot_paths() {
        let path = PathBuf::from("/fallow-missing-dir/app.js");
        let mut statics = FxHashMap::default();
        statics.insert("fallow:fn:a".to_owned(), static_target(&path, false));
        let mut hot_paths = vec![
            hot(Some("fallow:fn:a")),
            hot(Some("fallow:fn:b")),
            hot(None),
        ];

        attach_optimization_targets(&mut hot_paths, &statics, &mut one_inner_index(&path));

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

        attach_optimization_targets(&mut hot_paths, &statics, &mut one_inner_index(&path));

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
        index.record_function_starts(vec![start(20, 90), start(4, 30)]);
        index.record_function_starts(vec![start(4, 50)]);

        assert_eq!(
            index.lookup(&path, 3, "resolve"),
            Some(InnerIterations {
                calls: 20,
                peak_block_executions: 80,
            })
        );
    }

    #[test]
    fn dependency_scripts_are_not_read() {
        let mut index = InnerIterationIndex::default();
        index.record_raw_script(
            Path::new("/repo/node_modules/pkg/index.js"),
            &[function(true, &[5, 50])],
        );
        assert!(index.is_empty());
    }

    fn named_function(name: &str, start_offset: u32, counts: &[u64]) -> FunctionCoverage {
        let mut function = function(true, counts);
        function.function_name = name.to_owned();
        for range in &mut function.ranges {
            range.start_offset = start_offset;
        }
        function
    }

    fn temp_script(name: &str, source: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(name);
        std::fs::write(&path, source).expect("write script");
        let canonical = dunce::canonicalize(&path).expect("canonical script path");
        (dir, canonical)
    }

    #[test]
    fn raw_script_offsets_start_after_a_byte_order_mark() {
        let (_dir, path) = temp_script(
            "app.js",
            "\u{FEFF}const x = 1;\nfunction resolve() {\n  return x;\n}\n",
        );
        let offset_of_resolve = 13;
        let mut index = InnerIterationIndex::default();
        index.record_raw_script(
            &path,
            &[named_function("resolve", offset_of_resolve, &[5, 15])],
        );

        assert_eq!(
            index.lookup(&path, 2, "resolve"),
            Some(InnerIterations {
                calls: 5,
                peak_block_executions: 15,
            })
        );
    }

    #[test]
    fn raw_script_is_read_only_when_a_hot_path_joins_with_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dunce::canonicalize(dir.path())
            .expect("canonical temp dir")
            .join("app.js");
        let mut index = InnerIterationIndex::default();
        index.record_raw_script(&path, &[named_function("resolve", 0, &[2, 2])]);
        std::fs::write(&path, "function resolve() {}\n").expect("write script");

        assert_eq!(
            index.lookup(&path, 1, "resolve").map(|inner| inner.calls),
            Some(2)
        );
    }

    #[test]
    fn raw_script_dumps_keep_one_entry_per_function() {
        let (_dir, path) = temp_script("app.js", "function resolve() {\n  return 1;\n}\n");
        let mut index = InnerIterationIndex::default();
        for _ in 0..3 {
            index.record_raw_script(&path, &[named_function("resolve", 0, &[2, 6])]);
        }

        assert_eq!(index.raw_function_entries(), 1);
        assert_eq!(
            index.lookup(&path, 1, "resolve"),
            Some(InnerIterations {
                calls: 6,
                peak_block_executions: 18,
            })
        );
    }

    #[test]
    fn raw_script_join_needs_the_same_function_name() {
        let (_dir, path) = temp_script("app.js", "function resolve() {\n  return 1;\n}\n");
        let mut statics = FxHashMap::default();
        statics.insert("fallow:fn:a".to_owned(), static_target(&path, false));
        let join = |v8_name: &str| {
            let mut index = InnerIterationIndex::default();
            index.record_raw_script(&path, &[named_function(v8_name, 0, &[2, 6])]);
            let mut hot_paths = vec![hot(Some("fallow:fn:a"))];
            attach_optimization_targets(&mut hot_paths, &statics, &mut index);
            hot_paths[0]
                .optimization_target
                .as_ref()
                .expect("joined")
                .cost_basis
        };

        assert_eq!(join("resolve"), RuntimeCoverageCostBasis::InnerIterations);
        assert_eq!(
            join("Router.resolve"),
            RuntimeCoverageCostBasis::InnerIterations
        );
        assert_eq!(join("render"), RuntimeCoverageCostBasis::Cognitive);
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
