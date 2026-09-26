//! The `--performance` span tree and the process-level clock.
//!
//! The flat stage fields of [`PipelineTimings`](crate::trace::PipelineTimings)
//! come from separate clocks. Some stages run before the `total_ms` clock
//! starts, and duplication can run at the same time as the dead-code pass. A
//! reader who adds the flat fields gets a number larger than `total_ms` and
//! cannot tell why. The span tree states the structure: which span contains
//! which, and which spans overlap their siblings.

use serde::Serialize;

use crate::trace::PipelineTimings;

/// Process-level spans around the analysis pipeline.
///
/// The pipeline stages cover only part of a command. These spans cover the
/// rest: argument parsing, the thread pool, config loading, git, the work after
/// the analysis and the report output. `wall_ms` is the time from process start
/// to this report, so `wall_ms` minus the sum of the other spans is time that
/// no span measured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ProcessTimings {
    /// Time from process start to this report.
    pub wall_ms: f64,
    /// Time from process start until the command starts: argument parsing,
    /// logging setup and the thread pool.
    pub startup_ms: f64,
    /// The part of `startup_ms` that builds the worker thread pool.
    pub thread_pool_ms: f64,
    /// Config loading and validation for the command.
    pub config_ms: f64,
    /// Git calls outside the pipeline, for example `--changed-since`.
    pub git_ms: f64,
    /// The analysis call. The pipeline stage fields are parts of this span.
    pub analysis_ms: f64,
    /// Work after the analysis and before the output: scope filters, rules,
    /// baselines and gates.
    pub post_analysis_ms: f64,
    /// Report serialization and write.
    pub output_ms: f64,
}

/// One node of the `--performance` span tree.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PipelineSpan {
    /// Stable span name. Each name occurs at most once in a tree.
    pub name: &'static str,
    /// Name of the span that contains this span. `None` for a root span.
    pub parent: Option<&'static str>,
    /// Wall-clock milliseconds of this span.
    pub ms: f64,
    /// True when the span runs at the same time as its siblings. Its time
    /// then overlaps theirs and does not add to the parent.
    pub concurrent: bool,
}

/// Inputs for [`pipeline_span_tree`].
#[derive(Debug, Clone, Copy)]
pub struct SpanTreeInput<'a> {
    /// The pipeline stage timings.
    pub timings: &'a PipelineTimings,
    /// The process-level spans, when the caller measured them.
    pub process: Option<&'a ProcessTimings>,
    /// True when duplication ran at the same time as the dead-code pass.
    pub duplication_concurrent: bool,
}

struct TreeBuilder {
    spans: Vec<PipelineSpan>,
}

impl TreeBuilder {
    fn push(&mut self, name: &'static str, parent: Option<&'static str>, ms: f64) {
        self.spans.push(PipelineSpan {
            name,
            parent,
            ms,
            concurrent: false,
        });
    }
}

/// Build the span tree for one `--performance` report.
///
/// Spans come in pipeline order, and a parent always comes before its
/// children. With process spans, `process` is the only root and the pipeline
/// stages sit under `analysis`. Without them, the pipeline stages are the
/// roots.
#[must_use]
pub fn pipeline_span_tree(input: SpanTreeInput<'_>) -> Vec<PipelineSpan> {
    let t = input.timings;
    let mut tree = TreeBuilder {
        spans: Vec::with_capacity(32),
    };

    let analysis_parent = input.process.map(|process| {
        tree.push("process", None, process.wall_ms);
        tree.push("startup", Some("process"), process.startup_ms);
        tree.push("thread_pool", Some("startup"), process.thread_pool_ms);
        tree.push("config", Some("process"), process.config_ms);
        tree.push("git", Some("process"), process.git_ms);
        tree.push("analysis", Some("process"), process.analysis_ms);
        "analysis"
    });

    tree.push("workspaces", analysis_parent, t.workspaces_ms);
    tree.push("discover_files", analysis_parent, t.discover_files_ms);
    tree.push("parse_extract", analysis_parent, t.parse_extract_ms);
    tree.push(
        "parse_cache_load",
        Some("parse_extract"),
        t.parse_cache_load_ms,
    );
    tree.push("cache_update", analysis_parent, t.cache_update_ms);
    tree.push("pipeline", analysis_parent, t.total_ms);
    tree.push("plugins", Some("pipeline"), t.plugins_ms);
    tree.push("script_analysis", Some("pipeline"), t.script_analysis_ms);
    push_entry_point_spans(&mut tree, t);
    tree.push("resolve_imports", Some("pipeline"), t.resolve_imports_ms);
    tree.push("build_graph", Some("pipeline"), t.build_graph_ms);
    tree.push("analyze", Some("pipeline"), t.analyze_ms);

    if let Some(duplication_ms) = t.duplication_ms {
        let parent = input.process.map(|_| "process");
        tree.spans.push(PipelineSpan {
            name: "duplication",
            parent,
            ms: duplication_ms,
            concurrent: input.duplication_concurrent,
        });
    }

    if let Some(process) = input.process {
        tree.push("post_analysis", Some("process"), process.post_analysis_ms);
        tree.push("output", Some("process"), process.output_ms);
    }

    tree.spans
}

fn push_entry_point_spans(tree: &mut TreeBuilder, t: &PipelineTimings) {
    let spans = t.entry_point_spans;
    tree.push("entry_points", Some("pipeline"), t.entry_points_ms);
    tree.push("entry_points_root", Some("entry_points"), spans.root_ms);
    tree.push(
        "entry_points_workspaces",
        Some("entry_points"),
        spans.workspaces_ms,
    );
    tree.push("plugin_globs", Some("entry_points"), spans.plugins_ms);
    tree.push(
        "plugin_glob_compile",
        Some("plugin_globs"),
        spans.plugin_glob_build_ms,
    );
    tree.push(
        "plugin_glob_match",
        Some("plugin_globs"),
        spans.plugin_glob_match_ms,
    );
    tree.push(
        "entry_points_infrastructure",
        Some("entry_points"),
        spans.infrastructure_ms,
    );
    tree.push(
        "entry_points_dynamic",
        Some("entry_points"),
        spans.dynamic_ms,
    );
    tree.push("entry_points_dedup", Some("entry_points"), spans.dedup_ms);
}

#[cfg(test)]
mod tests {
    use rustc_hash::FxHashSet;

    use super::*;
    use crate::trace::{EntryPointSpans, PipelineCounters};

    fn timings(duplication_ms: Option<f64>) -> PipelineTimings {
        PipelineTimings {
            discover_files_ms: 8.0,
            file_count: 10,
            workspaces_ms: 1.0,
            workspace_count: 0,
            plugins_ms: 2.0,
            script_analysis_ms: 0.5,
            parse_extract_ms: 18.0,
            parse_cpu_ms: 30.0,
            parse_cache_load_ms: 3.0,
            module_count: 10,
            cache_hits: 0,
            cache_misses: 10,
            cache_rejection: None,
            graph_cache_rejection: None,
            cache_update_ms: 2.0,
            entry_points_ms: 3.0,
            entry_point_spans: EntryPointSpans::default(),
            entry_point_count: 1,
            resolve_imports_ms: 8.0,
            build_graph_ms: 2.0,
            analyze_ms: 10.0,
            duplication_ms,
            total_ms: 35.0,
            counters: PipelineCounters::default(),
        }
    }

    fn parent_of<'a>(tree: &'a [PipelineSpan], name: &str) -> Option<&'a str> {
        tree.iter()
            .find(|span| span.name == name)
            .unwrap_or_else(|| panic!("span {name} missing from {tree:#?}"))
            .parent
    }

    /// Every parent is a span of the tree, it comes before its children, and
    /// no name occurs twice.
    fn assert_well_formed(tree: &[PipelineSpan]) {
        let mut seen: FxHashSet<&str> = FxHashSet::default();
        for span in tree {
            if let Some(parent) = span.parent {
                assert!(
                    seen.contains(parent),
                    "{} names parent {parent} before it",
                    span.name
                );
            }
            assert!(seen.insert(span.name), "duplicate span {}", span.name);
        }
    }

    #[test]
    fn stages_before_the_total_clock_are_siblings_of_the_pipeline() {
        let tree = pipeline_span_tree(SpanTreeInput {
            timings: &timings(None),
            process: None,
            duplication_concurrent: false,
        });
        assert_well_formed(&tree);
        for root in [
            "workspaces",
            "discover_files",
            "parse_extract",
            "cache_update",
            "pipeline",
        ] {
            assert_eq!(parent_of(&tree, root), None, "{root}");
        }
        for stage in [
            "plugins",
            "script_analysis",
            "entry_points",
            "resolve_imports",
            "build_graph",
            "analyze",
        ] {
            assert_eq!(parent_of(&tree, stage), Some("pipeline"), "{stage}");
        }
        assert_eq!(parent_of(&tree, "parse_cache_load"), Some("parse_extract"));
        assert_eq!(parent_of(&tree, "plugin_glob_match"), Some("plugin_globs"));
        assert!(tree.iter().all(|span| !span.concurrent));
        assert!(tree.iter().all(|span| span.name != "duplication"));
    }

    #[test]
    fn process_spans_put_the_pipeline_under_the_analysis_span() {
        let process = ProcessTimings {
            wall_ms: 55.0,
            startup_ms: 4.0,
            thread_pool_ms: 1.0,
            config_ms: 2.0,
            git_ms: 0.0,
            analysis_ms: 40.0,
            post_analysis_ms: 1.0,
            output_ms: 3.0,
        };
        let tree = pipeline_span_tree(SpanTreeInput {
            timings: &timings(Some(12.0)),
            process: Some(&process),
            duplication_concurrent: true,
        });
        assert_well_formed(&tree);
        assert_eq!(tree[0].name, "process");
        assert_eq!(tree.iter().filter(|span| span.parent.is_none()).count(), 1);
        assert_eq!(parent_of(&tree, "thread_pool"), Some("startup"));
        assert_eq!(parent_of(&tree, "parse_extract"), Some("analysis"));
        assert_eq!(parent_of(&tree, "pipeline"), Some("analysis"));
        assert_eq!(parent_of(&tree, "output"), Some("process"));
        let duplication = tree
            .iter()
            .find(|span| span.name == "duplication")
            .expect("duplication span");
        assert_eq!(duplication.parent, Some("process"));
        assert!(duplication.concurrent);
    }

    /// Combined mode reports no process spans. The duplication span is then
    /// a root, and it keeps the concurrency that the caller gives.
    #[test]
    fn duplication_without_process_spans_is_a_root_with_its_concurrency() {
        for concurrent in [true, false] {
            let tree = pipeline_span_tree(SpanTreeInput {
                timings: &timings(Some(12.0)),
                process: None,
                duplication_concurrent: concurrent,
            });
            assert_well_formed(&tree);
            let duplication = tree
                .iter()
                .find(|span| span.name == "duplication")
                .expect("duplication span");
            assert_eq!(duplication.parent, None);
            assert_eq!(duplication.concurrent, concurrent);
            assert!(tree.iter().all(|span| span.name != "output"));
        }
    }
}
