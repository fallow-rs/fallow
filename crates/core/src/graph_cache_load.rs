//! The warm path that loads the persisted analysis graph.

use super::*;

/// Try to reuse the persisted module graph.
///
/// # Errors
///
/// `Err(Some(reason))` names why persisted work was refused. Two of these
/// branches are decided AFTER a full decode of a multi-megabyte blob, so they
/// are the most expensive refusals in the pipeline; leaving them silent made a
/// fully paid, fully wasted load indistinguishable from a first run.
/// `Err(None)` means there was nothing to refuse: the run disabled caching.
pub fn try_load_analysis_graph_cache(
    input: &AnalysisCoreSharedInput<'_>,
    entry_points: &TimedEntryPoints,
    modules: &[extract::ModuleInfo],
) -> Result<GraphCacheHit, Option<CacheRejection>> {
    if input.config.no_cache {
        return Err(None);
    }

    let t = Instant::now();
    input.progress.set_stage("loading module graph cache...");
    let unseeded_declaration_files = orphan_module_declaration_files(input, modules);
    let current = build_graph_cache_manifest(&GraphCacheManifestInput {
        config: input.config,
        plugin_result: input.plugin_result,
        entry_points: &entry_points.entry_points,
        files: input.files,
        workspaces: input.workspaces,
        modules,
        unseeded_declaration_files: &unseeded_declaration_files,
    });
    let store = graph_cache::GraphCacheStore::load(&input.config.cache_dir).map_err(Some)?;
    if store.manifest.matches_inputs(&current) {
        let mut project = restore_cached_resolved_project(input, modules, &store.resolved_project)?;
        tracing::debug!("Graph cache hit: skipping import resolution and graph build");
        let excluded = production_scope::excluded_file_ids(
            &entry_points.entry_points,
            input.files,
            &project.modules,
        );
        production_scope::neutralize(&mut project.modules, &excluded);
        let mut graph = store.graph;
        let bins = &input.plugin_result.dependency_binaries;
        credit_bin_path_references(&mut graph, modules, bins, &excluded);

        return Ok(GraphCacheHit {
            graph,
            project,
            elapsed_ms: t.elapsed().as_secs_f64() * 1000.0,
        });
    }

    if let Some(rejection) = store.manifest.classify_resolution_mismatch(&current) {
        // The level is the rejection's own judgement of whether the reader can
        // act on it. Content drift is what a cache is for, so it stays off
        // stderr; doctor and the performance table show it.
        if rejection.discarded_existing_work() {
            tracing::warn!(
                target: fallow_types::ANALYSIS_DIAGNOSTIC_LOG_TARGET,
                "Graph cache decoded but not reused: {}",
                rejection.describe()
            );
        } else {
            tracing::debug!(
                "Graph cache decoded but not reused: {}",
                rejection.describe()
            );
        }
        return Err(Some(rejection));
    }

    let mut project = restore_cached_resolved_project(input, modules, &store.resolved_project)?;
    tracing::debug!("Graph resolver cache hit: skipping import resolution and rebuilding graph");
    let graph = build_analysis_graph_timed(input, &mut project, entry_points, modules);

    Ok(GraphCacheHit {
        graph: graph.graph,
        project,
        elapsed_ms: t.elapsed().as_secs_f64() * 1000.0,
    })
}
