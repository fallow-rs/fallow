//! Builds the file nodes of the visualization graph.

use super::*;

pub(super) fn build_files(
    input: &VizBuildInput<'_>,
    index: &FileIndex<'_>,
    maps: &FilePropertyMaps<'_>,
) -> Vec<VizFile> {
    let graph = input.graph.as_graph();
    let unused_file_paths: rustc_hash::FxHashSet<&Path> = input
        .results
        .unused_files
        .iter()
        .map(|f| f.file.path.as_path())
        .collect();

    let mut unused_exports_by_file: FxHashMap<&Path, Vec<String>> = FxHashMap::default();
    for export in &input.results.unused_exports {
        unused_exports_by_file
            .entry(export.export.path.as_path())
            .or_default()
            .push(export.export.export_name.clone());
    }
    for export in &input.results.unused_types {
        unused_exports_by_file
            .entry(export.export.path.as_path())
            .or_default()
            .push(export.export.export_name.clone());
    }

    let mut complexity_by_file_id: FxHashMap<u32, ComplexityRollup> = FxHashMap::default();
    if let Some(modules) = input.modules {
        for module in modules {
            if !module.complexity.is_empty() {
                complexity_by_file_id
                    .insert(module.file_id.0, rollup_complexity(&module.complexity));
            }
        }
    }

    let mut in_cycle = vec![false; index.ordered.len()];
    for cycle in maps.cycles {
        for &idx in cycle {
            if let Some(slot) = in_cycle.get_mut(idx as usize) {
                *slot = true;
            }
        }
    }

    index
        .ordered
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let viz_idx = clamp_u32(i);
            let node_idx = file.id.0 as usize;
            let node = graph.modules.get(node_idx);
            let is_entry = node.is_some_and(|n| n.is_entry_point());
            let export_count = node.map_or(0, |n| clamp_u16(n.exports.len()));
            let import_count = clamp_u16(graph.edges_for(file.id).len());
            let importer_count = clamp_u16(input.graph.direct_importer_count(file.id));

            let unused_export_names = unused_exports_by_file
                .remove(file.path.as_path())
                .unwrap_or_default();
            let unused_export_count = clamp_u16(unused_export_names.len());

            let status = if unused_file_paths.contains(file.path.as_path()) {
                VizFileStatus::Unused
            } else if unused_export_count > 0 {
                VizFileStatus::HasUnusedExports
            } else if is_entry {
                VizFileStatus::EntryPoint
            } else {
                VizFileStatus::Clean
            };

            let complexity = complexity_by_file_id.remove(&file.id.0).unwrap_or_default();

            VizFile {
                path: relative_path(&file.path, &input.config.root),
                size: file.size_bytes,
                status,
                export_count,
                unused_export_count,
                is_entry,
                importer_count,
                import_count,
                workspace: workspace_index_for(&file.path, input.workspaces),
                zone: maps.zone_by_file.get(&viz_idx).copied(),
                unused_exports: unused_export_names,
                fn_count: complexity.fn_count,
                max_cyclomatic: complexity.max_cyclomatic,
                max_cognitive: complexity.max_cognitive,
                react_hooks: complexity.react_hooks,
                jsx_depth: complexity.jsx_depth,
                functions: complexity.functions,
                dup_lines: maps.dup_lines_by_file.get(&viz_idx).copied().unwrap_or(0),
                clone_groups: maps
                    .clone_groups_by_file
                    .get(&viz_idx)
                    .cloned()
                    .unwrap_or_default(),
                in_cycle: in_cycle[i],
            }
        })
        .collect()
}
