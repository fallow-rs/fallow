use colored::Colorize;
use fallow_types::cache_rejection::CacheRejection;
use fallow_types::pipeline_spans::ProcessTimings;
use fallow_types::trace::{EntryPointSpans, PipelineCounters, PipelineTimings};

/// Stages below this wall-clock time are too cheap to annotate as parallel;
/// the multiplier would be noise.
const PARALLEL_FLOOR_MS: f64 = 5.0;
/// Minimum CPU-to-wall ratio before a stage is worth flagging as parallel.
const MIN_PARALLEL_RATIO: f64 = 1.5;
/// Entry-point discovery below this wall-clock time is not worth subdividing;
/// the sub-spans would be six rows of rounding noise.
const ENTRY_POINT_BREAKDOWN_FLOOR_MS: f64 = 5.0;
/// A breakdown row below this cost rounds to `0.0ms` at the table's one decimal
/// place, so it spends a line to say nothing. Sections under it fold into the
/// `(other)` row of their level, which keeps that level's sum exact.
const SPAN_ROW_FLOOR_MS: f64 = 0.05;

/// Build the ` (parallel: ~Nms CPU)` suffix for a stage that ran across rayon
/// workers, or an empty string when the stage is too cheap or shows no real
/// parallelism. `cpu_ms` is the summed work across workers; `wall_ms` is the
/// stage's elapsed time. The wall floor is checked first so a near-zero stage
/// is never annotated (and the ratio test never divides).
fn parallel_annotation(wall_ms: f64, cpu_ms: f64) -> String {
    if wall_ms < PARALLEL_FLOOR_MS || cpu_ms < wall_ms * MIN_PARALLEL_RATIO {
        return String::new();
    }
    format!("  (parallel: ~{cpu_ms:.0}ms CPU)")
}

/// Time inside a parent duration not attributed to any of the rows displayed
/// under it (report assembly, coverage load, inter-stage glue).
///
/// Clamped at 0 because a parent and its children are read from separate
/// clocks and the children can genuinely exceed the parent. In the outer
/// pipeline table that is the normal case rather than a rounding artifact, for
/// the reason `push_performance_total_lines` documents. A `0.0ms` row means
/// "no unattributed time was found", not "the rows above sum to their parent".
fn other_ms(total_ms: f64, stages_sum_ms: f64) -> f64 {
    (total_ms - stages_sum_ms).max(0.0)
}

pub(in crate::report) fn print_performance_human(
    t: &PipelineTimings,
    process: Option<&ProcessTimings>,
    duplication_concurrent: bool,
) {
    for line in build_performance_report_lines(t, process, duplication_concurrent) {
        eprintln!("{line}");
    }
}

/// The pipeline table without process rows, as a library caller renders it.
#[cfg(test)]
fn build_performance_human_lines(t: &PipelineTimings) -> Vec<String> {
    build_performance_report_lines(t, None, true)
}

/// Build human-readable output lines for pipeline performance timings.
fn build_performance_report_lines(
    t: &PipelineTimings,
    process: Option<&ProcessTimings>,
    duplication_concurrent: bool,
) -> Vec<String> {
    let mut lines = Vec::new();

    push_performance_header(&mut lines);
    push_discovery_stage_lines(&mut lines, t);
    push_dimmed(
        &mut lines,
        &format!(
            "│  parse/extract:    {:>8.1}ms  ({} modules, {} cached, {} parsed){}",
            t.parse_extract_ms,
            t.module_count,
            t.cache_hits,
            t.cache_misses,
            parallel_annotation(t.parse_extract_ms, t.parse_cpu_ms)
        ),
    );
    push_cache_rejection_line(&mut lines, "parse cache", t.cache_rejection);
    push_analysis_stage_lines(&mut lines, t);
    if let Some(duplication_ms) = t.duplication_ms {
        let relation = if duplication_concurrent {
            "concurrent"
        } else {
            "after dead code"
        };
        push_dimmed(
            &mut lines,
            &format!("│  duplication:      {duplication_ms:>8.1}ms  ({relation})"),
        );
    }
    push_performance_total_lines(&mut lines, t);
    if let Some(process) = process {
        push_process_lines(&mut lines, process);
    }
    push_work_counter_lines(&mut lines, &t.counters);
    push_dimmed(
        &mut lines,
        "└───────────────────────────────────────────────────",
    );
    lines.push(String::new());

    lines
}

fn push_dimmed(lines: &mut Vec<String>, line: &str) {
    lines.push(line.dimmed().to_string());
}

/// Name a refused cache under the stage that paid for it.
///
/// A refusal used to be signalled by the ABSENCE of a cached/parsed
/// annotation, which reads exactly like a first run on a project. The counts
/// above this line are always printed now, and this line says why they are
/// what they are.
fn push_cache_rejection_line(
    lines: &mut Vec<String>,
    label: &str,
    rejection: Option<CacheRejection>,
) {
    let Some(rejection) = rejection else {
        return;
    };
    push_dimmed(
        lines,
        &format!("│  {label} not reused: {}", rejection.describe()),
    );
}

fn push_performance_header(lines: &mut Vec<String>) {
    lines.push(String::new());
    push_dimmed(
        lines,
        "┌─ Pipeline Performance ─────────────────────────────",
    );
}

fn push_discovery_stage_lines(lines: &mut Vec<String>, t: &PipelineTimings) {
    push_dimmed(
        lines,
        &format!(
            "│  discover files:   {:>8.1}ms  ({} files)",
            t.discover_files_ms, t.file_count
        ),
    );
    push_dimmed(
        lines,
        &format!(
            "│  workspaces:       {:>8.1}ms  ({} workspaces)",
            t.workspaces_ms, t.workspace_count
        ),
    );
    push_dimmed(
        lines,
        &format!(
            "│  plugin detection: {:>8.1}ms{}",
            t.plugins_ms,
            plugin_glob_cross_reference(t)
        ),
    );
    push_dimmed(
        lines,
        &format!("│  script analysis:  {:>8.1}ms", t.script_analysis_ms),
    );
}

/// Name the OTHER place plugin cost lands, next to the number that is not it.
///
/// Plugin work is timed in two different stages: detecting which plugins are
/// active, and matching their entry-point globs against every discovered file,
/// which happens inside entry-point discovery. On real projects the glob half is
/// the larger of the two, so a row labelled `plugins` carrying only the
/// detection half reads as the whole plugin bill and understates it several
/// times over. The two spans are genuinely different stretches of wall clock and
/// cannot be summed into one row without breaking the stage partition, so both
/// numbers are printed and the reader adds them.
///
/// Emitted only when the entry-point breakdown is printed, so the row this
/// points at is on screen.
fn plugin_glob_cross_reference(t: &PipelineTimings) -> String {
    if t.entry_points_ms < ENTRY_POINT_BREAKDOWN_FLOOR_MS {
        return String::new();
    }
    format!(
        "  (+{:.1}ms plugin globs under entry points)",
        t.entry_point_spans.plugins_ms
    )
}

fn push_analysis_stage_lines(lines: &mut Vec<String>, t: &PipelineTimings) {
    push_dimmed(
        lines,
        &format!("│  cache update:     {:>8.1}ms", t.cache_update_ms),
    );
    push_dimmed(
        lines,
        &format!(
            "│  entry points:     {:>8.1}ms  ({} entries)",
            t.entry_points_ms, t.entry_point_count
        ),
    );
    push_entry_point_span_lines(lines, t.entry_points_ms, t.entry_point_spans);
    push_dimmed(
        lines,
        &format!("│  resolve imports:  {:>8.1}ms", t.resolve_imports_ms),
    );
    push_dimmed(
        lines,
        &format!("│  build graph:      {:>8.1}ms", t.build_graph_ms),
    );
    push_cache_rejection_line(lines, "graph cache", t.graph_cache_rejection);
    push_dimmed(
        lines,
        &format!("│  analyze:          {:>8.1}ms", t.analyze_ms),
    );
}

/// Subdivide the entry-point stage into its discovery sections.
///
/// These rows are indented under `entry points` because they partition that
/// stage rather than add to it; `displayed_stage_sum` must keep ignoring them
/// or the `(other)` row would go negative. Rows are printed in pipeline order
/// rather than sorted by cost so two runs of the same project diff cleanly.
///
/// Both nested levels close with their own `(other)` row: `compile + match +
/// (other)` sums to `plugin globs`, and the sections plus their `(other)`
/// sum to the stage, to within rounding, because every span here is carved
/// from inside the entry-point stage's own clock. The outer table has no such
/// property (see `push_performance_total_lines`). Without these rows the two
/// nested sums silently fell short of the parents they claimed to divide, and
/// a reader had no way to tell an unmeasured remainder from an arithmetic
/// error.
///
/// A section under [`SPAN_ROW_FLOOR_MS`] renders as `0.0ms` and says nothing,
/// so it is folded into its `(other)` row rather than printed. Both sums are
/// computed from the rows that survive the floor, which keeps each level's
/// arithmetic exact while spending lines only on the sections that cost
/// something.
fn push_entry_point_span_lines(lines: &mut Vec<String>, stage_ms: f64, spans: EntryPointSpans) {
    if stage_ms < ENTRY_POINT_BREAKDOWN_FLOOR_MS {
        return;
    }
    let mut rows: Vec<(&str, f64)> = Vec::with_capacity(10);
    let mut sections_sum = 0.0;

    for (label, value) in [
        ("root package", spans.root_ms),
        ("workspaces", spans.workspaces_ms),
    ] {
        if value >= SPAN_ROW_FLOOR_MS {
            sections_sum += value;
            rows.push((label, value));
        }
    }
    if spans.plugins_ms >= SPAN_ROW_FLOOR_MS {
        sections_sum += spans.plugins_ms;
        rows.push(("plugin globs", spans.plugins_ms));
        let mut glob_sum = 0.0;
        for (label, value) in [
            ("  compile", spans.plugin_glob_build_ms),
            ("  match", spans.plugin_glob_match_ms),
        ] {
            if value >= SPAN_ROW_FLOOR_MS {
                glob_sum += value;
                rows.push((label, value));
            }
        }
        rows.push(("  (other)", other_ms(spans.plugins_ms, glob_sum)));
    }
    for (label, value) in [
        ("infrastructure", spans.infrastructure_ms),
        ("dynamic globs", spans.dynamic_ms),
        ("dedup", spans.dedup_ms),
    ] {
        if value >= SPAN_ROW_FLOOR_MS {
            sections_sum += value;
            rows.push((label, value));
        }
    }
    rows.push(("(other)", other_ms(stage_ms, sections_sum)));

    for (label, value) in rows {
        push_dimmed(lines, &format!("│    {label:<16}{value:>8.1}ms"));
    }
}

fn displayed_stage_sum(t: &PipelineTimings) -> f64 {
    t.discover_files_ms
        + t.workspaces_ms
        + t.plugins_ms
        + t.script_analysis_ms
        + t.parse_extract_ms
        + t.cache_update_ms
        + t.entry_points_ms
        + t.resolve_imports_ms
        + t.build_graph_ms
        + t.analyze_ms
}

/// Print the unattributed remainder and the TOTAL row.
///
/// TOTAL is not the sum of the rows above it and cannot be read as one. It is
/// the wall clock of the dead-code backend, started when the prelude begins
/// (workspace package loading, then plugin detection) and read after the
/// detectors finish. File discovery, workspace discovery, parse/extract and the
/// cache update are all measured before that clock starts, yet they are listed
/// as rows, so on a warm run the rows exceed TOTAL by tens of milliseconds and
/// `(other)` clamps to `0.0ms`. Duplication is left out of the sum for the
/// opposite reason: it runs concurrently with the stages above it. Read the
/// rows as per-stage costs, not as a partition of TOTAL.
///
/// That caveat is printed, not only documented here. An `(other)` row above a
/// horizontal rule above a TOTAL is summation grammar in every table a reader
/// has met, and the entry-point breakdown one indent level down really does
/// close its sums that way, so the reader has just been taught the opposite of
/// what this level means.
fn push_performance_total_lines(lines: &mut Vec<String>, t: &PipelineTimings) {
    push_dimmed(
        lines,
        &format!(
            "│  (other):          {:>8.1}ms",
            other_ms(t.total_ms, displayed_stage_sum(t))
        ),
    );
    push_dimmed(lines, "│  ────────────────────────────────────────────────");
    lines.push(
        format!("│  TOTAL:            {:>8.1}ms", t.total_ms)
            .bold()
            .dimmed()
            .to_string(),
    );
    push_dimmed(
        lines,
        "│  rows are per-stage costs; several run outside or beside the TOTAL clock",
    );
}

/// Print the process clock: the spans around the pipeline and the WALL row.
///
/// Unlike the stage rows above, these spans are disjoint parts of one clock,
/// so they close with a real sum: the rows plus `(other)` give WALL. The
/// stage rows are parts of the `analysis` row. Duplication is left out,
/// because in combined mode it can run beside the analysis.
fn push_process_lines(lines: &mut Vec<String>, p: &ProcessTimings) {
    push_dimmed(
        lines,
        "├─ Process ──────────────────────────────────────────",
    );
    push_dimmed(
        lines,
        &format!(
            "│  startup:          {:>8.1}ms  (thread pool {:.1}ms)",
            p.startup_ms, p.thread_pool_ms
        ),
    );
    push_dimmed(
        lines,
        &format!("│  config:           {:>8.1}ms", p.config_ms),
    );
    if p.git_ms > 0.0 {
        push_dimmed(lines, &format!("│  git:              {:>8.1}ms", p.git_ms));
    }
    push_dimmed(
        lines,
        &format!(
            "│  analysis:         {:>8.1}ms  (the stage rows above)",
            p.analysis_ms
        ),
    );
    push_dimmed(
        lines,
        &format!("│  after analysis:   {:>8.1}ms", p.post_analysis_ms),
    );
    push_dimmed(
        lines,
        &format!("│  output:           {:>8.1}ms", p.output_ms),
    );
    let spans_sum =
        p.startup_ms + p.config_ms + p.git_ms + p.analysis_ms + p.post_analysis_ms + p.output_ms;
    push_dimmed(
        lines,
        &format!(
            "│  (other):          {:>8.1}ms",
            other_ms(p.wall_ms, spans_sum)
        ),
    );
    push_dimmed(lines, "│  ────────────────────────────────────────────────");
    lines.push(
        format!("│  WALL:             {:>8.1}ms", p.wall_ms)
            .bold()
            .dimmed()
            .to_string(),
    );
}

/// Print the exact work counts under the clock.
///
/// A millisecond row changes from run to run. These counts do not, so a
/// reader who compares two runs can tell a stage that did more work from a
/// stage that only ran slower.
fn push_work_counter_lines(lines: &mut Vec<String>, c: &PipelineCounters) {
    push_dimmed(
        lines,
        &format!(
            "│  work: {} files read, {} source bytes, {} parse cache bytes, {} CSS masked bytes",
            c.files_read, c.source_bytes_read, c.parse_cache_bytes_read, c.css_masked_bytes
        ),
    );
    push_dimmed(
        lines,
        &format!(
            "│  resolve: {} specifier calls ({} unique), {} resolver calls, {} canonicalize calls",
            c.resolve_specifier_calls,
            c.unique_specifiers,
            c.oxc_resolve_calls,
            c.canonicalize_calls
        ),
    );
}

pub(in crate::report) fn print_health_performance_human(t: &fallow_output::HealthTimings) {
    for line in build_health_performance_lines(t) {
        eprintln!("{line}");
    }
}

fn build_health_performance_lines(t: &fallow_output::HealthTimings) -> Vec<String> {
    let mut lines = Vec::new();

    push_health_performance_header(&mut lines);
    push_health_performance_stage_lines(&mut lines, t);
    push_health_performance_total_lines(&mut lines, t);

    lines
}

fn push_health_performance_header(lines: &mut Vec<String>) {
    lines.push(String::new());
    push_dimmed(
        lines,
        "┌─ Health Pipeline Performance ─────────────────────",
    );
}

fn push_health_performance_stage_lines(lines: &mut Vec<String>, t: &fallow_output::HealthTimings) {
    push_dimmed(
        lines,
        &format!("│  config:           {:>8.1}ms", t.config_ms),
    );
    let discover_line = if t.shared_parse {
        "│  discover files:   (measured above)".to_string()
    } else {
        format!("│  discover files:   {:>8.1}ms", t.discover_ms)
    };
    push_dimmed(lines, &discover_line);
    let parse_line = if t.shared_parse {
        "│  parse/extract:    (measured above)".to_string()
    } else {
        format!(
            "│  parse/extract:    {:>8.1}ms{}",
            t.parse_ms,
            parallel_annotation(t.parse_ms, t.parse_cpu_ms)
        )
    };
    push_dimmed(lines, &parse_line);
    push_dimmed(
        lines,
        &format!("│  complexity:       {:>8.1}ms", t.complexity_ms),
    );
    push_dimmed(
        lines,
        &format!("│  file scores:      {:>8.1}ms", t.file_scores_ms),
    );
    let cache_state = if t.git_churn_cache_hit {
        "cached"
    } else {
        "cold"
    };
    let cache_note = if t.git_log_bytes > 0 {
        format!(" ({cache_state}, {} git log bytes)", t.git_log_bytes)
    } else {
        format!(" ({cache_state})")
    };
    push_dimmed(
        lines,
        &format!(
            "│  git churn:        {:>8.1}ms{}",
            t.git_churn_ms, cache_note
        ),
    );
    push_dimmed(
        lines,
        &format!("│  hotspots:         {:>8.1}ms", t.hotspots_ms),
    );
    push_dimmed(
        lines,
        &format!("│  duplication:      {:>8.1}ms", t.duplication_ms),
    );
    push_dimmed(
        lines,
        &format!("│  targets:          {:>8.1}ms", t.targets_ms),
    );
}

fn health_performance_stage_sum(t: &fallow_output::HealthTimings) -> f64 {
    t.config_ms
        + t.discover_ms
        + t.parse_ms
        + t.complexity_ms
        + t.file_scores_ms
        + t.git_churn_ms
        + t.hotspots_ms
        + t.duplication_ms
        + t.targets_ms
}

fn push_health_performance_total_lines(lines: &mut Vec<String>, t: &fallow_output::HealthTimings) {
    push_dimmed(
        lines,
        &format!(
            "│  (other):          {:>8.1}ms",
            other_ms(t.total_ms, health_performance_stage_sum(t))
        ),
    );
    push_dimmed(lines, "│  ────────────────────────────────────────────────");
    lines.push(
        format!("│  TOTAL:            {:>8.1}ms", t.total_ms)
            .bold()
            .dimmed()
            .to_string(),
    );
    push_dimmed(
        lines,
        "└───────────────────────────────────────────────────",
    );
    lines.push(String::new());
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    #[test]
    fn performance_output_contains_all_pipeline_stages() {
        let timings = PipelineTimings {
            discover_files_ms: 12.5,
            file_count: 100,
            workspaces_ms: 3.2,
            workspace_count: 3,
            plugins_ms: 1.0,
            script_analysis_ms: 2.5,
            parse_extract_ms: 45.0,
            parse_cpu_ms: 45.0,
            parse_cache_load_ms: 0.0,
            module_count: 80,
            cache_hits: 0,
            cache_misses: 80,
            cache_rejection: None,
            graph_cache_rejection: None,
            cache_update_ms: 5.0,
            entry_points_ms: 0.5,
            entry_point_spans: EntryPointSpans::default(),
            entry_point_count: 10,
            resolve_imports_ms: 8.0,
            build_graph_ms: 15.0,
            analyze_ms: 10.0,
            duplication_ms: Some(7.2),
            total_ms: 102.7,
            counters: PipelineCounters::default(),
        };
        let lines = build_performance_human_lines(&timings);
        let text = plain(&lines);
        assert!(text.contains("Pipeline Performance"));
        assert!(text.contains("discover files"));
        assert!(text.contains("100 files"));
        assert!(text.contains("workspaces"));
        assert!(text.contains("3 workspaces"));
        assert!(text.contains("plugin detection"));
        assert!(text.contains("script analysis"));
        assert!(text.contains("parse/extract"));
        assert!(text.contains("80 modules"));
        assert!(text.contains("cache update"));
        assert!(text.contains("entry points"));
        assert!(text.contains("10 entries"));
        assert!(text.contains("resolve imports"));
        assert!(text.contains("build graph"));
        assert!(text.contains("analyze"));
        assert!(text.contains("duplication"));
        assert!(text.contains("7.2"));
        assert!(text.contains("(other)"));
        assert!(text.contains("TOTAL"));
        assert!(text.contains("102.7"));
        assert!(!text.contains("parallel"));
    }

    #[test]
    fn performance_output_shows_cache_detail_when_cache_hits_nonzero() {
        let timings = PipelineTimings {
            discover_files_ms: 10.0,
            file_count: 50,
            workspaces_ms: 1.0,
            workspace_count: 1,
            plugins_ms: 0.5,
            script_analysis_ms: 1.0,
            parse_extract_ms: 20.0,
            parse_cpu_ms: 20.0,
            parse_cache_load_ms: 0.0,
            module_count: 40,
            cache_hits: 30,
            cache_misses: 10,
            cache_rejection: None,
            graph_cache_rejection: None,
            cache_update_ms: 2.0,
            entry_points_ms: 0.3,
            entry_point_spans: EntryPointSpans::default(),
            entry_point_count: 5,
            resolve_imports_ms: 3.0,
            build_graph_ms: 5.0,
            analyze_ms: 4.0,
            duplication_ms: None,
            total_ms: 46.8,
            counters: PipelineCounters::default(),
        };
        let lines = build_performance_human_lines(&timings);
        let text = plain(&lines);
        assert!(text.contains("30 cached"));
        assert!(text.contains("10 parsed"));
    }

    /// A cold run states its counts instead of dropping the annotation: the
    /// absence of text used to be the only signal that a cache was refused,
    /// which reads exactly like a first run on a project.
    #[test]
    fn performance_output_states_zero_hits_instead_of_omitting_the_detail() {
        let timings = PipelineTimings {
            discover_files_ms: 10.0,
            file_count: 50,
            workspaces_ms: 1.0,
            workspace_count: 1,
            plugins_ms: 0.5,
            script_analysis_ms: 1.0,
            parse_extract_ms: 20.0,
            parse_cpu_ms: 20.0,
            parse_cache_load_ms: 0.0,
            module_count: 40,
            cache_hits: 0,
            cache_misses: 40,
            cache_rejection: None,
            graph_cache_rejection: None,
            cache_update_ms: 2.0,
            entry_points_ms: 0.3,
            entry_point_spans: EntryPointSpans::default(),
            entry_point_count: 5,
            resolve_imports_ms: 3.0,
            build_graph_ms: 5.0,
            analyze_ms: 4.0,
            duplication_ms: None,
            total_ms: 46.8,
            counters: PipelineCounters::default(),
        };
        let lines = build_performance_human_lines(&timings);
        let text = plain(&lines);
        assert!(text.contains("0 cached"));
        assert!(text.contains("40 parsed"));
    }

    /// The exact work counts sit under the clock, so a reader can tell a slow
    /// stage from a stage that did more work.
    #[test]
    fn performance_output_shows_the_work_counters() {
        let mut timings = pipeline_timings_with_parse(20.0, 20.0);
        timings.counters = PipelineCounters {
            files_read: 40,
            source_bytes_read: 12_345,
            parse_cache_bytes_read: 678,
            css_masked_bytes: 91,
            resolve_specifier_calls: 90,
            unique_specifiers: 60,
            oxc_resolve_calls: 75,
            canonicalize_calls: 3,
        };

        let text = plain(&build_performance_human_lines(&timings));

        assert!(
            text.contains(
                "work: 40 files read, 12345 source bytes, 678 parse cache bytes, 91 CSS masked bytes"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "resolve: 90 specifier calls (60 unique), 75 resolver calls, 3 canonicalize calls"
            ),
            "{text}"
        );
    }

    /// The process rows are disjoint parts of one clock, so they close with
    /// `(other)` and a WALL row that is their real sum.
    #[test]
    fn performance_output_closes_the_process_rows_with_wall() {
        let timings = pipeline_timings_with_parse(20.0, 20.0);
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

        let text = plain(&build_performance_report_lines(
            &timings,
            Some(&process),
            false,
        ));

        assert!(
            text.contains("startup:               4.0ms  (thread pool 1.0ms)"),
            "{text}"
        );
        assert!(text.contains("(other):               5.0ms"), "{text}");
        assert!(text.contains("WALL:                 55.0ms"), "{text}");
        assert!(
            !text.contains("git:"),
            "a run without git calls has no git row: {text}"
        );
        let wall = text.find("WALL:").expect("WALL row");
        let total = text.find("TOTAL:").expect("TOTAL row");
        assert!(
            total < wall,
            "the process rows follow the pipeline rows: {text}"
        );
    }

    /// Duplication that ran after the dead-code pass is not called concurrent.
    #[test]
    fn performance_output_names_sequential_duplication() {
        let mut timings = pipeline_timings_with_parse(20.0, 20.0);
        timings.duplication_ms = Some(7.0);
        let text = plain(&build_performance_report_lines(&timings, None, false));
        assert!(text.contains("(after dead code)"), "{text}");
        assert!(!text.contains("(concurrent)"), "{text}");
    }

    /// A refused cache is named under the stage that paid for it, and the
    /// counts stay on the row above it, so "cold" and "refused" are
    /// distinguishable in one glance.
    #[test]
    fn performance_output_names_a_refused_cache() {
        let mut timings = pipeline_timings_with_parse(20.0, 20.0);
        timings.cache_rejection = Some(CacheRejection::ConfigHashMismatch);
        timings.graph_cache_rejection = Some(CacheRejection::FileSetChanged);

        let text = plain(&build_performance_human_lines(&timings));

        assert!(text.contains("0 cached"), "{text}");
        assert!(text.contains("40 parsed"), "{text}");
        assert!(
            text.contains("parse cache not reused: extraction config changed"),
            "{text}"
        );
        assert!(
            text.contains("graph cache not reused: the analysed file set changed"),
            "{text}"
        );
    }

    /// A slow discovery stage names which section paid, instead of leaving one
    /// opaque number that can only be guessed at.
    #[test]
    fn performance_output_subdivides_a_slow_entry_point_stage() {
        let mut timings = pipeline_timings_with_parse(20.0, 20.0);
        timings.entry_points_ms = 121.0;
        timings.entry_point_spans = EntryPointSpans {
            root_ms: 96.0,
            workspaces_ms: 18.0,
            plugins_ms: 4.0,
            plugin_glob_build_ms: 1.0,
            plugin_glob_match_ms: 3.0,
            infrastructure_ms: 1.0,
            // Every section carries a cost the table can show; a section that
            // rounds to `0.0ms` folds into `(other)` instead, which
            // `breakdown_rows_below_the_floor_fold_into_their_other_row` covers.
            dynamic_ms: 0.5,
            dedup_ms: 2.0,
        };

        let text = plain(&build_performance_human_lines(&timings));

        assert!(text.contains("root package"), "{text}");
        assert!(text.contains("96.0ms"), "{text}");
        assert!(text.contains("workspaces"), "{text}");
        assert!(text.contains("18.0ms"), "{text}");
        assert!(text.contains("plugin globs"), "{text}");
        assert!(text.contains("compile"), "{text}");
        assert!(text.contains("match"), "{text}");
        assert!(text.contains("3.0ms"), "{text}");
        assert!(text.contains("infrastructure"), "{text}");
        assert!(text.contains("dynamic globs"), "{text}");
        assert!(text.contains("dedup"), "{text}");
    }

    /// The sub-rows subdivide the stage rather than adding to it, so they must
    /// not inflate the stage sum and drive `(other)` to zero.
    #[test]
    fn entry_point_subdivision_does_not_change_the_other_row() {
        let mut without = pipeline_timings_with_parse(20.0, 20.0);
        without.entry_points_ms = 121.0;
        without.total_ms = 300.0;
        let mut with_spans = pipeline_timings_with_parse(20.0, 20.0);
        with_spans.entry_points_ms = 121.0;
        with_spans.total_ms = 300.0;
        with_spans.entry_point_spans = EntryPointSpans {
            root_ms: 96.0,
            workspaces_ms: 18.0,
            plugins_ms: 4.0,
            plugin_glob_build_ms: 1.0,
            plugin_glob_match_ms: 3.0,
            infrastructure_ms: 1.0,
            dynamic_ms: 0.0,
            dedup_ms: 2.0,
        };

        let plain_sum = displayed_stage_sum(&without);
        let subdivided_sum = displayed_stage_sum(&with_spans);

        assert!(
            (plain_sum - subdivided_sum).abs() < 1e-9,
            "sub-rows must subdivide the stage, not add to it: {plain_sum} vs {subdivided_sum}"
        );
    }

    /// `compile` and `match` are the two halves of the plugin-glob span that
    /// were measured; the rest of that span is the entry-set merge and was
    /// simply missing from the table. Without an `(other)` row a reader saw two
    /// children that did not add up to their parent and no way to tell an
    /// unmeasured remainder from a bug.
    #[test]
    fn entry_point_sub_tables_close_with_their_own_other_rows() {
        let mut timings = pipeline_timings_with_parse(20.0, 20.0);
        timings.entry_points_ms = 200.0;
        timings.entry_point_spans = EntryPointSpans {
            root_ms: 10.0,
            workspaces_ms: 5.0,
            plugins_ms: 157.6,
            plugin_glob_build_ms: 100.0,
            plugin_glob_match_ms: 49.3,
            infrastructure_ms: 2.0,
            dynamic_ms: 0.0,
            dedup_ms: 1.0,
        };

        let text = plain(&build_performance_human_lines(&timings));

        assert!(
            text.contains("│      (other)            8.3ms"),
            "compile + match must close against plugin globs: {text}"
        );
        assert!(
            text.contains("│    (other)             24.4ms"),
            "the six sections must close against the entry-point stage: {text}"
        );
    }

    /// Adjacent spans are carved from separate clock reads, so rounding can put
    /// a child fractionally above its parent. Both remainders clamp at zero
    /// rather than rendering a negative row, the same guarantee `other_ms`
    /// already gave the outer table.
    #[test]
    fn entry_point_other_rows_never_go_negative_when_children_overrun() {
        let mut timings = pipeline_timings_with_parse(20.0, 20.0);
        timings.entry_points_ms = 6.0;
        timings.entry_point_spans = EntryPointSpans {
            root_ms: 5.0,
            workspaces_ms: 5.0,
            plugins_ms: 5.0,
            plugin_glob_build_ms: 4.0,
            plugin_glob_match_ms: 4.0,
            infrastructure_ms: 0.0,
            dynamic_ms: 0.0,
            dedup_ms: 0.0,
        };

        let text = plain(&build_performance_human_lines(&timings));

        assert!(
            text.contains("│      (other)            0.0ms"),
            "children that overrun their parent clamp to zero, not a negative: {text}"
        );
        assert!(
            text.contains("│    (other)              0.0ms"),
            "sections that overrun the stage clamp to zero, not a negative: {text}"
        );
    }

    /// The row labelled for plugins carried only the DETECTION half while the
    /// larger glob half sat inside the entry-point stage, so the table
    /// understated plugin cost several times over with nothing saying so. Both
    /// numbers are now on the plugin row for the reader to add.
    #[test]
    fn plugin_row_names_the_glob_cost_that_lands_in_another_stage() {
        let mut timings = pipeline_timings_with_parse(20.0, 20.0);
        timings.plugins_ms = 29.9;
        timings.entry_points_ms = 200.0;
        timings.entry_point_spans = EntryPointSpans {
            plugins_ms: 157.6,
            plugin_glob_build_ms: 100.0,
            plugin_glob_match_ms: 49.3,
            ..EntryPointSpans::default()
        };

        let text = plain(&build_performance_human_lines(&timings));

        assert!(
            text.contains(
                "plugin detection:     29.9ms  (+157.6ms plugin globs under entry points)"
            ),
            "both halves of the plugin bill must be readable off one row: {text}"
        );
    }

    /// The cross-reference points at a row, so it is silent when the breakdown
    /// that carries that row is not printed.
    #[test]
    fn plugin_row_omits_the_cross_reference_without_a_breakdown() {
        let mut timings = pipeline_timings_with_parse(20.0, 20.0);
        timings.entry_points_ms = 0.3;

        let text = plain(&build_performance_human_lines(&timings));

        assert!(
            !text.contains("plugin globs"),
            "no breakdown, nothing to point at: {text}"
        );
    }

    /// A cheap stage is not worth six rows of rounding noise.
    #[test]
    fn performance_output_omits_the_breakdown_for_a_cheap_entry_point_stage() {
        let mut timings = pipeline_timings_with_parse(20.0, 20.0);
        timings.entry_points_ms = 0.3;
        timings.entry_point_spans = EntryPointSpans {
            root_ms: 0.2,
            ..EntryPointSpans::default()
        };

        let text = plain(&build_performance_human_lines(&timings));

        assert!(!text.contains("root package"), "{text}");
        assert!(!text.contains("plugin globs"), "{text}");
    }

    /// Timings measured by running the release binary over this repository. The
    /// listed stages add to roughly 145ms against a TOTAL of 68.8ms, because
    /// discovery, workspace discovery, parse/extract and the cache update are
    /// all timed before the clock TOTAL reads starts. The table must survive
    /// that without a negative row, and nothing may claim the rows partition
    /// TOTAL.
    #[test]
    fn outer_rows_can_exceed_total_because_earlier_stages_sit_outside_its_clock() {
        let mut timings = pipeline_timings_with_parse(22.6, 22.6);
        timings.discover_files_ms = 48.6;
        timings.workspaces_ms = 5.0;
        timings.plugins_ms = 13.8;
        timings.script_analysis_ms = 6.6;
        timings.cache_update_ms = 1.0;
        timings.entry_points_ms = 21.5;
        timings.resolve_imports_ms = 0.0;
        timings.build_graph_ms = 4.5;
        timings.analyze_ms = 21.1;
        timings.total_ms = 68.8;

        let sum = displayed_stage_sum(&timings);
        assert!(
            sum > timings.total_ms + 50.0,
            "the measured stage rows must overshoot TOTAL by far more than rounding: {sum} vs {}",
            timings.total_ms
        );

        let text = plain(&build_performance_human_lines(&timings));

        assert!(
            text.contains("│  (other):               0.0ms"),
            "an overshooting stage sum clamps the remainder to zero rather than going negative: {text}"
        );
        assert!(
            text.contains(
                "rows are per-stage costs; several run outside or beside the TOTAL clock"
            ),
            "the caveat that keeps the rule from reading as a sum must be on screen: {text}"
        );
    }

    /// An `(other)` row, a rule, and a TOTAL is summation grammar, and the
    /// entry-point breakdown one indent level down really does close its sums
    /// that way. The explanation lived only in rustdoc and a test name, where
    /// no reader of the table would find it.
    #[test]
    fn the_total_row_says_on_screen_that_it_is_not_a_sum() {
        let timings = pipeline_timings_with_parse(20.0, 20.0);

        let lines = build_performance_human_lines(&timings);
        let text = plain(&lines);

        let total_idx = lines
            .iter()
            .position(|line| plain(std::slice::from_ref(line)).contains("TOTAL:"))
            .expect("the table prints a TOTAL row");
        let note_idx = lines
            .iter()
            .position(|line| plain(std::slice::from_ref(line)).contains("rows are per-stage costs"))
            .expect("the table prints the caveat");

        assert_eq!(
            note_idx,
            total_idx + 1,
            "the caveat belongs directly under TOTAL: {text}"
        );
    }

    /// A section that renders as `0.0ms` spends a line to say nothing. Six of
    /// them turned a 6.6ms stage into ten rows.
    #[test]
    fn breakdown_rows_below_the_floor_fold_into_their_other_row() {
        let mut timings = pipeline_timings_with_parse(20.0, 20.0);
        timings.entry_points_ms = 6.6;
        timings.entry_point_spans = EntryPointSpans {
            root_ms: 0.0,
            workspaces_ms: 0.0,
            plugins_ms: 6.0,
            plugin_glob_build_ms: 5.5,
            plugin_glob_match_ms: 0.0,
            infrastructure_ms: 0.0,
            dynamic_ms: 0.0,
            dedup_ms: 0.0,
        };

        let text = plain(&build_performance_human_lines(&timings));

        for silent in ["root package", "workspaces  ", "infrastructure", "dedup"] {
            assert!(
                !text.contains(silent),
                "a section that rounds to 0.0ms must not spend a row: {silent:?} in {text}"
            );
        }
        assert!(text.contains("plugin globs"), "{text}");
        assert!(text.contains("compile"), "{text}");
        assert!(
            !text.contains("match "),
            "a zero glob-match row folds into the nested (other): {text}"
        );
        assert!(
            text.contains("│      (other)            0.5ms"),
            "the nested (other) must still close against plugin globs: {text}"
        );
        assert!(
            text.contains("│    (other)              0.6ms"),
            "the section (other) must close against the stage using the shown rows: {text}"
        );
    }

    fn pipeline_timings_with_parse(parse_extract_ms: f64, parse_cpu_ms: f64) -> PipelineTimings {
        PipelineTimings {
            discover_files_ms: 10.0,
            file_count: 50,
            workspaces_ms: 1.0,
            workspace_count: 1,
            plugins_ms: 0.5,
            script_analysis_ms: 1.0,
            parse_extract_ms,
            parse_cpu_ms,
            parse_cache_load_ms: 0.0,
            module_count: 40,
            cache_hits: 0,
            cache_misses: 40,
            cache_rejection: None,
            graph_cache_rejection: None,
            cache_update_ms: 2.0,
            entry_points_ms: 0.3,
            entry_point_spans: EntryPointSpans::default(),
            entry_point_count: 5,
            resolve_imports_ms: 3.0,
            build_graph_ms: 5.0,
            analyze_ms: 4.0,
            duplication_ms: None,
            total_ms: 200.0,
            counters: PipelineCounters::default(),
        }
    }

    #[test]
    fn combined_duplication_is_concurrent_and_excluded_from_reconciliation() {
        let mut t = pipeline_timings_with_parse(20.0, 20.0);
        t.total_ms = 50.0;
        t.duplication_ms = Some(500.0); // concurrent, far exceeds TOTAL
        let text = plain(&build_performance_human_lines(&t));
        assert!(
            text.contains("duplication:") && text.contains("(concurrent)"),
            "duplication must be marked concurrent: {text}"
        );
        assert!(
            text.contains("3.2ms"),
            "(other) must reconcile sequential stages only (3.2ms), not clamp to 0 from the 500ms concurrent duplication: {text}"
        );
    }

    #[test]
    fn parse_stage_annotated_when_cpu_dominates_wall() {
        let text = plain(&build_performance_human_lines(
            &pipeline_timings_with_parse(340.0, 5440.0),
        ));
        assert!(
            text.contains("(parallel: ~5440ms CPU)"),
            "parallel parse stage should be annotated: {text}"
        );
    }

    #[test]
    fn parse_stage_not_annotated_below_wall_floor() {
        let text = plain(&build_performance_human_lines(
            &pipeline_timings_with_parse(3.0, 40.0),
        ));
        assert!(
            !text.contains("parallel"),
            "sub-floor stage must not be annotated: {text}"
        );
    }

    #[test]
    fn parse_stage_not_annotated_when_ratio_low() {
        let text = plain(&build_performance_human_lines(
            &pipeline_timings_with_parse(50.0, 60.0),
        ));
        assert!(
            !text.contains("parallel"),
            "low-parallelism stage must not be annotated: {text}"
        );
    }

    fn health_timings(shared_parse: bool) -> fallow_output::HealthTimings {
        fallow_output::HealthTimings {
            config_ms: 4.0,
            discover_ms: if shared_parse { 0.0 } else { 30.0 },
            parse_ms: if shared_parse { 0.0 } else { 340.0 },
            parse_cpu_ms: if shared_parse { 0.0 } else { 5440.0 },
            complexity_ms: 4.8,
            file_scores_ms: 50.0,
            git_churn_ms: 10.0,
            git_churn_cache_hit: true,
            git_log_bytes: 0,
            hotspots_ms: 2.0,
            duplication_ms: 0.0,
            targets_ms: 1.0,
            total_ms: 780.0,
            shared_parse,
        }
    }

    #[test]
    fn health_reused_stages_labelled_when_shared_parse() {
        let text = plain(&build_health_performance_lines(&health_timings(true)));
        assert!(
            text.matches("(measured above)").count() == 2,
            "discover + parse should both read (measured above): {text}"
        );
        assert!(!text.contains("discover files:      0.0ms"));
        assert!(!text.contains("parse/extract:       0.0ms"));
        assert!(text.contains("config"));
        assert!(text.contains("(other)"));
    }

    #[test]
    fn health_standalone_shows_real_stages_and_parse_annotation() {
        let text = plain(&build_health_performance_lines(&health_timings(false)));
        assert!(
            !text.contains("(measured above)"),
            "standalone health must show real stage numbers: {text}"
        );
        assert!(
            text.contains("(parallel: ~5440ms CPU)"),
            "standalone parse stage should be annotated: {text}"
        );
        assert!(text.contains("(other)"));
    }

    /// The churn row names the exact git bytes it read, so a slow cold churn
    /// can be told apart from a large history.
    #[test]
    fn health_churn_row_names_the_git_log_bytes_read() {
        let mut timings = health_timings(false);
        timings.git_churn_cache_hit = false;
        timings.git_log_bytes = 75_263;
        let text = plain(&build_health_performance_lines(&timings));
        assert!(text.contains("(cold, 75263 git log bytes)"), "{text}");

        let text = plain(&build_health_performance_lines(&health_timings(false)));
        assert!(text.contains("(cached)"), "{text}");
        assert!(!text.contains("git log bytes"), "{text}");
    }
}
