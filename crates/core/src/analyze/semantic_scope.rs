//! Capture semantic evidence eligibility before report filters alter findings.

use fallow_config::ResolvedConfig;
use fallow_types::results::AnalysisResults;

use crate::graph::ModuleGraph;
use crate::plugins::AggregatedPluginResult;

pub(super) fn attach(
    results: &mut AnalysisResults,
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    plugins: Option<&AggregatedPluginResult>,
) {
    results.semantic_reachable_files = graph
        .modules
        .iter()
        .filter(|module| module.is_reachable())
        .map(|module| module.path.clone())
        .collect();
    if config.type_aware.enabled {
        results.semantic_framework_contracts = plugins.map_or_else(Vec::new, |plugins| {
            plugins.framework_class_member_contracts.clone()
        });
    }
}
