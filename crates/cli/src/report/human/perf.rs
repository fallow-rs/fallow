use colored::Colorize;
use fallow_types::trace::PipelineTimings;

/// Stages below this wall-clock time are too cheap to annotate as parallel;
/// the multiplier would be noise.
const PARALLEL_FLOOR_MS: f64 = 5.0;
/// Minimum CPU-to-wall ratio before a stage is worth flagging as parallel.
const MIN_PARALLEL_RATIO: f64 = 1.5;

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

/// Time inside TOTAL not attributed to any displayed stage (report assembly,
/// coverage load, inter-stage glue). Clamped at 0 so floating-point rounding
/// where the stage sum slightly exceeds TOTAL never renders a negative row.
/// Surfacing this makes every breakdown's rows provably sum to TOTAL.
fn other_ms(total_ms: f64, stages_sum_ms: f64) -> f64 {
    (total_ms - stages_sum_ms).max(0.0)
}

pub(in crate::report) fn print_performance_human(t: &PipelineTimings) {
    for line in build_performance_human_lines(t) {
        eprintln!("{line}");
    }
}

/// Build human-readable output lines for pipeline performance timings.
pub(in crate::report) fn build_performance_human_lines(t: &PipelineTimings) -> Vec<String> {
    let mut lines = Vec::new();

    push_performance_header(&mut lines);
    push_discovery_stage_lines(&mut lines, t);
    let cache_detail = if t.cache_hits > 0 {
        format!(", {} cached, {} parsed", t.cache_hits, t.cache_misses)
    } else {
        String::new()
    };
    push_dimmed(
        &mut lines,
        &format!(
            "│  parse/extract:    {:>8.1}ms  ({} modules{}){}",
            t.parse_extract_ms,
            t.module_count,
            cache_detail,
            parallel_annotation(t.parse_extract_ms, t.parse_cpu_ms)
        ),
    );
    push_analysis_stage_lines(&mut lines, t);
    if let Some(duplication_ms) = t.duplication_ms {
        push_dimmed(
            &mut lines,
            &format!("│  duplication:      {duplication_ms:>8.1}ms  (concurrent)"),
        );
    }
    push_performance_total_lines(&mut lines, t);

    lines
}

fn push_dimmed(lines: &mut Vec<String>, line: &str) {
    lines.push(line.dimmed().to_string());
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
        &format!("│  plugins:          {:>8.1}ms", t.plugins_ms),
    );
    push_dimmed(
        lines,
        &format!("│  script analysis:  {:>8.1}ms", t.script_analysis_ms),
    );
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
    push_dimmed(
        lines,
        &format!("│  resolve imports:  {:>8.1}ms", t.resolve_imports_ms),
    );
    push_dimmed(
        lines,
        &format!("│  build graph:      {:>8.1}ms", t.build_graph_ms),
    );
    push_dimmed(
        lines,
        &format!("│  analyze:          {:>8.1}ms", t.analyze_ms),
    );
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
        "└───────────────────────────────────────────────────",
    );
    lines.push(String::new());
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
    let cache_note = if t.git_churn_cache_hit {
        " (cached)"
    } else {
        " (cold)"
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
            module_count: 80,
            cache_hits: 0,
            cache_misses: 80,
            cache_update_ms: 5.0,
            entry_points_ms: 0.5,
            entry_point_count: 10,
            resolve_imports_ms: 8.0,
            build_graph_ms: 15.0,
            analyze_ms: 10.0,
            duplication_ms: Some(7.2),
            total_ms: 102.7,
        };
        let lines = build_performance_human_lines(&timings);
        let text = plain(&lines);
        assert!(text.contains("Pipeline Performance"));
        assert!(text.contains("discover files"));
        assert!(text.contains("100 files"));
        assert!(text.contains("workspaces"));
        assert!(text.contains("3 workspaces"));
        assert!(text.contains("plugins"));
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
            module_count: 40,
            cache_hits: 30,
            cache_misses: 10,
            cache_update_ms: 2.0,
            entry_points_ms: 0.3,
            entry_point_count: 5,
            resolve_imports_ms: 3.0,
            build_graph_ms: 5.0,
            analyze_ms: 4.0,
            duplication_ms: None,
            total_ms: 46.8,
        };
        let lines = build_performance_human_lines(&timings);
        let text = plain(&lines);
        assert!(text.contains("30 cached"));
        assert!(text.contains("10 parsed"));
    }

    #[test]
    fn performance_output_omits_cache_detail_when_no_cache_hits() {
        let timings = PipelineTimings {
            discover_files_ms: 10.0,
            file_count: 50,
            workspaces_ms: 1.0,
            workspace_count: 1,
            plugins_ms: 0.5,
            script_analysis_ms: 1.0,
            parse_extract_ms: 20.0,
            parse_cpu_ms: 20.0,
            module_count: 40,
            cache_hits: 0,
            cache_misses: 40,
            cache_update_ms: 2.0,
            entry_points_ms: 0.3,
            entry_point_count: 5,
            resolve_imports_ms: 3.0,
            build_graph_ms: 5.0,
            analyze_ms: 4.0,
            duplication_ms: None,
            total_ms: 46.8,
        };
        let lines = build_performance_human_lines(&timings);
        let text = plain(&lines);
        assert!(!text.contains("cached"));
        assert!(!text.contains("parsed"));
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
            module_count: 40,
            cache_hits: 0,
            cache_misses: 40,
            cache_update_ms: 2.0,
            entry_points_ms: 0.3,
            entry_point_count: 5,
            resolve_imports_ms: 3.0,
            build_graph_ms: 5.0,
            analyze_ms: 4.0,
            duplication_ms: None,
            total_ms: 200.0,
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
}
