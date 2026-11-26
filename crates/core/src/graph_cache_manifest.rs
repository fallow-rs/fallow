//! The manifest that keys the persisted analysis graph.

use super::*;

/// The run state that keys the persisted graph cache.
pub struct GraphCacheManifestInput<'a> {
    pub config: &'a ResolvedConfig,
    pub plugin_result: &'a plugins::AggregatedPluginResult,
    pub entry_points: &'a discover::CategorizedEntryPoints,
    pub files: &'a [discover::DiscoveredFile],
    pub workspaces: &'a [fallow_config::WorkspaceInfo],
    pub modules: &'a [extract::ModuleInfo],
    pub unseeded_declaration_files: &'a FxHashSet<discover::FileId>,
}

/// Build the current `GraphCacheManifest` from the run's discovered files and
/// graph-affecting option hashes.
pub fn build_graph_cache_manifest(
    input: &GraphCacheManifestInput<'_>,
) -> graph_cache::GraphCacheManifest {
    let GraphCacheManifestInput {
        config,
        plugin_result,
        entry_points,
        files,
        workspaces,
        modules,
        unseeded_declaration_files,
    } = *input;
    let mode = graph_cache::GraphCacheMode::new(
        graph_cache_key::resolver_options_hash(config, workspaces, files),
        graph_cache_key::entry_points_hash(
            entry_points,
            files,
            unseeded_declaration_files,
            &graph_build::consumer_checked_entries(config, entry_points, files, workspaces),
            &config.root,
        ),
        graph_cache_key::plugin_config_hash(plugin_result, &config.root),
    );
    let hashes = graph_cache_file_hashes(plugin_result, modules, files.len());
    graph_cache::GraphCacheManifest::from_discovered_files(&config.root, files, mode, |file| {
        hashes
            .get(file.id.0 as usize)
            .copied()
            .unwrap_or(graph_cache::GraphCacheFileHashes::NO_MODULE)
    })
}

/// The manifest hashes of each file, indexed by `FileId`.
///
/// The parse stage already hashed every file it read, so keying the manifest
/// on content costs nothing and does not inherit mtime's blind spot for a
/// same-size rewrite. A file with no module (unreadable) hashes to 0, which
/// compares equal only against another run that also failed to read it.
///
/// The auto-import pass of the resolver reads the exports and the body facts
/// of every module, which the resolve hash does not cover. With an
/// auto-import rule, each content change therefore counts as a resolution
/// change.
fn graph_cache_file_hashes(
    plugin_result: &plugins::AggregatedPluginResult,
    modules: &[extract::ModuleInfo],
    file_count: usize,
) -> Vec<graph_cache::GraphCacheFileHashes> {
    let resolution_reads_content = !plugin_result.auto_imports.is_empty();
    let mut hashes = vec![graph_cache::GraphCacheFileHashes::NO_MODULE; file_count];
    for module in modules {
        if let Some(slot) = hashes.get_mut(module.file_id.0 as usize) {
            *slot = if resolution_reads_content {
                graph_cache::GraphCacheFileHashes::content_only(module.content_hash)
            } else {
                graph_cache::GraphCacheFileHashes::of_module(module)
            };
        }
    }
    hashes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_auto_import_rule_makes_each_content_change_a_resolution_change() {
        let mut module = extract::ModuleInfo::empty(crate::discover::FileId(1));
        module.content_hash = 42;

        let without_rule = crate::plugins::AggregatedPluginResult::default();
        let hashes = graph_cache_file_hashes(&without_rule, std::slice::from_ref(&module), 2);
        assert_eq!(
            hashes[0],
            crate::graph_cache::GraphCacheFileHashes::NO_MODULE
        );
        assert_eq!(hashes[1].content_hash, 42);
        assert_ne!(
            hashes[1].resolve_hash, 42,
            "without a rule, the resolve hash does not follow the content"
        );

        let mut with_rule = crate::plugins::AggregatedPluginResult::default();
        with_rule
            .auto_imports
            .push(fallow_config::AutoImportRule::new(
                "useThing".to_string(),
                std::path::PathBuf::from("/project/composables/useThing.ts"),
                fallow_config::AutoImportKind::Named,
            ));
        let hashes = graph_cache_file_hashes(&with_rule, std::slice::from_ref(&module), 2);
        assert_eq!(
            hashes[1],
            crate::graph_cache::GraphCacheFileHashes::content_only(42),
            "the auto-import pass reads body facts that the resolve hash does not cover"
        );
    }
}
