use fallow_config::ResolvedConfig;
use fallow_output::{CoverageModel, HealthActionContext, HealthActionOptions};

use super::HealthExecutionOptions;

pub(super) const fn active_health_coverage_model(has_istanbul_coverage: bool) -> CoverageModel {
    if has_istanbul_coverage {
        CoverageModel::Istanbul
    } else {
        CoverageModel::StaticEstimated
    }
}

pub(super) fn build_health_action_context(
    opts: &HealthExecutionOptions<'_>,
    config: &ResolvedConfig,
    max_cyclomatic: u16,
    max_cognitive: u16,
    max_crap: f64,
) -> HealthActionContext {
    let baseline_active = opts.baseline.is_some() || opts.save_baseline.is_some();
    let action_opts = if baseline_active {
        HealthActionOptions {
            omit_suppress_line: true,
            omit_reason: Some("baseline-active"),
        }
    } else if !config.health.suggest_inline_suppression {
        HealthActionOptions {
            omit_suppress_line: true,
            omit_reason: Some("config-disabled"),
        }
    } else {
        HealthActionOptions::default()
    };
    HealthActionContext {
        opts: action_opts,
        max_cyclomatic_threshold: max_cyclomatic,
        max_cognitive_threshold: max_cognitive,
        max_crap_threshold: max_crap,
        crap_refactor_band: config.health.crap_refactor_band,
    }
}
