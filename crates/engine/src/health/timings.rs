//! Health performance timing assembly.

use std::time::Instant;

use fallow_output::HealthTimings;

use super::HealthExecutionOptions;

pub(super) struct HealthTimingInput {
    pub(super) config_ms: f64,
    pub(super) discover_ms: f64,
    pub(super) parse_ms: f64,
    pub(super) parse_cpu_ms: f64,
    pub(super) complexity_ms: f64,
    pub(super) file_scores_ms: f64,
    pub(super) git_churn_ms: f64,
    pub(super) git_churn_cache_hit: bool,
    pub(super) hotspots_ms: f64,
    pub(super) duplication_ms: f64,
    pub(super) targets_ms: f64,
    pub(super) shared_parse: bool,
}

pub(super) fn build_health_timings(
    opts: &HealthExecutionOptions<'_>,
    start: &Instant,
    input: &HealthTimingInput,
) -> Option<HealthTimings> {
    if !opts.performance {
        return None;
    }

    let inner_ms = start.elapsed().as_secs_f64() * 1000.0;
    let total_ms = input.config_ms + input.discover_ms + input.parse_ms + inner_ms;
    Some(HealthTimings {
        config_ms: input.config_ms,
        discover_ms: input.discover_ms,
        parse_ms: input.parse_ms,
        parse_cpu_ms: input.parse_cpu_ms,
        complexity_ms: input.complexity_ms,
        file_scores_ms: input.file_scores_ms,
        git_churn_ms: input.git_churn_ms,
        git_churn_cache_hit: input.git_churn_cache_hit,
        hotspots_ms: input.hotspots_ms,
        duplication_ms: input.duplication_ms,
        targets_ms: input.targets_ms,
        total_ms,
        shared_parse: input.shared_parse,
    })
}
