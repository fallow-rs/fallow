use crate::params::ImpactParams;

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
