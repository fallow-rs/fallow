use crate::params::{CheckRuntimeCoverageParams, GetTokenBlastRadiusParams};

use super::{push_global, push_scope};

/// Build CLI arguments for the `check_runtime_coverage` tool.
pub fn build_check_runtime_coverage_args(params: &CheckRuntimeCoverageParams) -> Vec<String> {
    let mut args = vec![
        "health".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
        "--runtime-coverage".to_string(),
        params.coverage.clone(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_scope(&mut args, params.production, params.workspace.as_deref());

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
    if let Some(max_crap) = params.max_crap {
        args.extend(["--max-crap".to_string(), format!("{max_crap}")]);
    }
    if let Some(top) = params.top {
        args.extend(["--top".to_string(), top.to_string()]);
    }
    if let Some(ref gb) = params.group_by {
        args.extend(["--group-by".to_string(), gb.clone()]);
    }

    args
}

/// Build CLI arguments for `get_hot_paths`.
pub fn build_get_hot_paths_args(params: &CheckRuntimeCoverageParams) -> Vec<String> {
    build_check_runtime_coverage_args(params)
}

/// Build CLI arguments for `get_blast_radius`.
pub fn build_get_blast_radius_args(params: &CheckRuntimeCoverageParams) -> Vec<String> {
    build_check_runtime_coverage_args(params)
}

/// Build CLI arguments for `get_importance`.
pub fn build_get_importance_args(params: &CheckRuntimeCoverageParams) -> Vec<String> {
    build_check_runtime_coverage_args(params)
}

/// Build CLI arguments for `get_cleanup_candidates`.
pub fn build_get_cleanup_candidates_args(params: &CheckRuntimeCoverageParams) -> Vec<String> {
    build_check_runtime_coverage_args(params)
}

/// Build CLI arguments for `get_token_blast_radius`.
///
/// Forces `--css` (the reverse index lives in the `--css` health section) and
/// emits `health --css --format json` plus the shared root/config and global
/// `--no-cache` / `--threads` plumbing. No scope flags: `token_consumers`
/// abstains on partial scope, so workspace/changed-since would only ever
/// return an empty index.
pub fn build_get_token_blast_radius_args(params: &GetTokenBlastRadiusParams) -> Vec<String> {
    let mut args = vec![
        "health".to_string(),
        "--css".to_string(),
        "--format".to_string(),
        "json".to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );

    args
}
