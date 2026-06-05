use crate::params::HealthParams;

use super::{push_baseline, push_global, push_scope, push_str_flag};

/// Build CLI arguments for the `check_health` tool.
pub fn build_health_args(params: &HealthParams) -> Vec<String> {
    let mut args = vec![
        "health".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_scope(&mut args, params.production, params.workspace.as_deref());

    if let Some(max_cyclomatic) = params.max_cyclomatic {
        args.extend(["--max-cyclomatic".to_string(), max_cyclomatic.to_string()]);
    }
    if let Some(max_cognitive) = params.max_cognitive {
        args.extend(["--max-cognitive".to_string(), max_cognitive.to_string()]);
    }
    if let Some(max_crap) = params.max_crap {
        args.extend(["--max-crap".to_string(), format!("{max_crap}")]);
    }
    if let Some(top) = params.top {
        args.extend(["--top".to_string(), top.to_string()]);
    }
    push_str_flag(&mut args, "--sort", params.sort.as_deref());
    push_str_flag(
        &mut args,
        "--changed-since",
        params.changed_since.as_deref(),
    );
    if params.complexity == Some(true) {
        args.push("--complexity".to_string());
    }
    if params.complexity_breakdown == Some(true) {
        args.push("--complexity-breakdown".to_string());
    }
    if params.file_scores == Some(true) {
        args.push("--file-scores".to_string());
    }
    let ownership_active = params.ownership == Some(true) || params.ownership_email_mode.is_some();
    if params.hotspots == Some(true) || ownership_active {
        args.push("--hotspots".to_string());
    }
    if ownership_active {
        args.push("--ownership".to_string());
    }
    if let Some(mode) = params.ownership_email_mode {
        args.extend(["--ownership-emails".to_string(), mode.as_cli().to_string()]);
    }
    if params.targets == Some(true) {
        args.push("--targets".to_string());
    }
    if params.coverage_gaps == Some(true) {
        args.push("--coverage-gaps".to_string());
    }
    if params.score == Some(true) {
        args.push("--score".to_string());
    }
    if let Some(min_score) = params.min_score {
        args.extend(["--min-score".to_string(), min_score.to_string()]);
    }
    push_str_flag(&mut args, "--min-severity", params.min_severity.as_deref());
    push_str_flag(&mut args, "--since", params.since.as_deref());
    if let Some(min_commits) = params.min_commits {
        args.extend(["--min-commits".to_string(), min_commits.to_string()]);
    }
    push_str_flag(&mut args, "--churn-file", params.churn_file.as_deref());
    if let Some(ref path) = params.save_snapshot {
        if path.is_empty() {
            args.push("--save-snapshot".to_string());
        } else {
            args.extend(["--save-snapshot".to_string(), path.clone()]);
        }
    }
    push_baseline(
        &mut args,
        params.baseline.as_deref(),
        params.save_baseline.as_deref(),
    );
    if params.trend == Some(true) {
        args.push("--trend".to_string());
    }
    push_str_flag(&mut args, "--effort", params.effort.as_deref());
    if params.summary == Some(true) {
        args.push("--summary".to_string());
    }
    push_str_flag(&mut args, "--coverage", params.coverage.as_deref());
    push_str_flag(
        &mut args,
        "--coverage-root",
        params.coverage_root.as_deref(),
    );
    push_str_flag(
        &mut args,
        "--runtime-coverage",
        params.runtime_coverage.as_deref(),
    );
    if let Some(min_invocations_hot) = params.min_invocations_hot {
        args.extend([
            "--min-invocations-hot".to_string(),
            min_invocations_hot.to_string(),
        ]);
    }
    if let Some(min_observation_volume) = params.min_observation_volume {
        args.extend([
            "--min-observation-volume".to_string(),
            min_observation_volume.to_string(),
        ]);
    }
    if let Some(low_traffic_threshold) = params.low_traffic_threshold {
        args.extend([
            "--low-traffic-threshold".to_string(),
            format!("{low_traffic_threshold}"),
        ]);
    }
    push_str_flag(&mut args, "--group-by", params.group_by.as_deref());

    args
}
