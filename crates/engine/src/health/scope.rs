use fallow_config::ResolvedConfig;

use super::HealthOptions;
use super::ignore::build_ignore_set;
use super::pipeline::{HealthScope, HealthScopeInputs};

pub(super) fn prepare_health_scope<'a, R>(
    opts: &HealthOptions<'a>,
    config: &ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    scope_inputs: HealthScopeInputs<'a, R>,
) -> HealthScope<'a, R> {
    let max_cyclomatic = opts
        .thresholds
        .max_cyclomatic
        .unwrap_or(config.health.max_cyclomatic);
    let max_cognitive = opts
        .thresholds
        .max_cognitive
        .unwrap_or(config.health.max_cognitive);
    let max_crap = opts.thresholds.max_crap.unwrap_or(config.health.max_crap);
    let ignore_set = build_ignore_set(&config.health.ignore);
    let HealthScopeInputs {
        changed_files,
        diff_index,
        ws_roots,
        group_resolver,
    } = scope_inputs;
    let file_paths = files.iter().map(|f| (f.id, &f.path)).collect();

    HealthScope {
        max_cyclomatic,
        max_cognitive,
        max_crap,
        enforce_crap: max_crap > 0.0,
        ignore_set,
        changed_files,
        diff_index,
        ws_roots,
        group_resolver,
        file_paths,
    }
}
