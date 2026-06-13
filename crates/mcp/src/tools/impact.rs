use crate::params::{ImpactAllParams, ImpactParams};

use super::push_str_flag;

/// Build CLI arguments for the `impact` tool.
///
/// `fallow impact` (bare, no subcommand) renders the read-only value report.
/// The mutating `enable` / `disable` subcommands are deliberately not exposed:
/// enabling local tracking is a one-time human setup step, not an agent action.
pub fn build_impact_args(params: &ImpactParams) -> Vec<String> {
    let mut args = vec![
        "impact".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_str_flag(&mut args, "--root", params.root.as_deref());

    args
}

/// Build CLI arguments for the `impact_all` cross-repo aggregate tool.
///
/// `fallow impact --all` rolls every tracked project on this machine into one
/// read-only view. It takes no `root`: the aggregate reads the user config dir,
/// independent of any single repo. Invalid `sort` values are rejected by the
/// CLI (clap value-enum) and surface as a structured exit-2 error.
pub fn build_impact_all_args(params: &ImpactAllParams) -> Vec<String> {
    let mut args = vec![
        "impact".to_string(),
        "--all".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_str_flag(&mut args, "--sort", params.sort.as_deref());
    if let Some(limit) = params.limit {
        args.push("--limit".to_string());
        args.push(limit.to_string());
    }

    args
}
