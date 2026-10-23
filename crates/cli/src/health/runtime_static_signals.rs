//! Static signals for the runtime coverage request.
//!
//! The index records which exports are unused and which exports a test
//! references. In production mode the analysis graph has no test files, so the
//! test references come from the full-project graph of the health run.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

/// Static signals per file for the runtime coverage request.
#[derive(Default)]
pub(super) struct StaticSignalIndex {
    pub(super) unused_files: FxHashSet<PathBuf>,
    pub(super) exported_names: FxHashMap<PathBuf, FxHashSet<String>>,
    pub(super) exported_lines: FxHashMap<PathBuf, FxHashSet<u32>>,
    pub(super) unused_export_names: FxHashMap<PathBuf, FxHashSet<String>>,
    pub(super) unused_export_lines: FxHashMap<PathBuf, FxHashSet<u32>>,
    pub(super) test_referenced_export_names: FxHashMap<PathBuf, FxHashSet<String>>,
    pub(super) test_referenced_export_lines: FxHashMap<PathBuf, FxHashSet<u32>>,
}

/// Build the static signal index from the analysis output.
///
/// `test_reach` is the full-project graph of a production run. When it is
/// `None`, the test references come from the analysis graph.
pub(super) fn build_static_signal_index(
    modules: &[fallow_types::extract::ModuleInfo],
    analysis_output: &fallow_engine::dead_code::DeadCodeAnalysisArtifacts,
    file_paths: &FxHashMap<fallow_types::discover::FileId, &PathBuf>,
    test_reach: Option<&fallow_engine::health::TestReachGraph>,
) -> Result<StaticSignalIndex, String> {
    let graph = analysis_output
        .graph
        .as_ref()
        .ok_or_else(|| "analysis graph not available for runtime coverage".to_owned())?;
    let mut index = StaticSignalIndex::default();
    index_dead_code_signals(&mut index, analysis_output);

    let module_by_id: FxHashMap<_, _> = modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();
    for export in
        fallow_engine::module_graph::module_value_exports_with_test_reach(graph, test_reach)
    {
        let Some(&path) = file_paths.get(&export.file_id) else {
            continue;
        };
        index_graph_value_export(
            &mut index,
            &export,
            module_by_id.get(&export.file_id).copied(),
            path,
        );
    }

    Ok(index)
}

/// Seed the signal index with unused-file and unused-export dead-code signals.
fn index_dead_code_signals(
    index: &mut StaticSignalIndex,
    analysis_output: &fallow_engine::dead_code::DeadCodeAnalysisArtifacts,
) {
    for file in &analysis_output.results.unused_files {
        index.unused_files.insert(file.file.path.clone());
    }
    for export in &analysis_output.results.unused_exports {
        index
            .unused_export_names
            .entry(export.export.path.clone())
            .or_default()
            .insert(export.export.export_name.clone());
        index
            .unused_export_lines
            .entry(export.export.path.clone())
            .or_default()
            .insert(export.export.line);
    }
}

/// Index one graph value export, including test-referenced state.
fn index_graph_value_export(
    index: &mut StaticSignalIndex,
    export: &fallow_engine::module_graph::ModuleValueExport,
    module: Option<&fallow_types::extract::ModuleInfo>,
    path: &Path,
) {
    index
        .exported_names
        .entry(path.to_path_buf())
        .or_default()
        .insert(export.name.clone());

    if let Some(module) = module {
        let (line, _) =
            fallow_types::extract::byte_offset_to_line_col(&module.line_offsets, export.span_start);
        index
            .exported_lines
            .entry(path.to_path_buf())
            .or_default()
            .insert(line);

        if export.test_referenced {
            index
                .test_referenced_export_names
                .entry(path.to_path_buf())
                .or_default()
                .insert(export.name.clone());
            index
                .test_referenced_export_lines
                .entry(path.to_path_buf())
                .or_default()
                .insert(line);
        }
    }
}
