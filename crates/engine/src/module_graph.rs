//! Module graph contracts owned by the engine boundary.

#![allow(
    clippy::implicit_hasher,
    reason = "engine graph helpers use FxHashSet changed-file sets consistently with the rest of fallow"
)]

use std::path::{Path, PathBuf};

use fallow_types::discover::FileId;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_graph::graph::{
    CoordinationGapPaths as GraphCoordinationGapPaths,
    FocusFileFactsPaths as GraphFocusFileFactsPaths, ImpactClosurePaths as GraphImpactClosurePaths,
    ModuleGraph, PartitionOrderPaths as GraphPartitionOrderPaths,
    ReviewUnitPaths as GraphReviewUnitPaths,
};
use fallow_graph::graph::{
    DirectImporterSummary as GraphDirectImporterSummary,
    ImportedSymbolSummary as GraphImportedSymbolSummary,
};

/// Engine-owned retained graph handle.
///
/// Downstream crates can request stable graph facts through engine helpers
/// without depending on `fallow-graph` node internals.
#[derive(Debug)]
pub struct RetainedModuleGraph {
    inner: ModuleGraph,
}

impl RetainedModuleGraph {
    /// Wrap a freshly built module graph for engine result contracts.
    #[must_use]
    const fn new(inner: ModuleGraph) -> Self {
        Self { inner }
    }

    pub(crate) const fn as_graph(&self) -> &ModuleGraph {
        &self.inner
    }

    /// Borrow the shared static test-coverage view for health analysis.
    pub(crate) const fn static_test_coverage(&self) -> StaticTestCoverage<'_> {
        StaticTestCoverage::new(&self.inner)
    }

    /// Number of modules in the retained graph.
    #[must_use]
    pub fn module_count(&self) -> usize {
        self.inner.module_count()
    }

    /// Number of edges in the retained graph.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.inner.edge_count()
    }

    /// Build public export keys for a precomputed public-entry set.
    #[must_use]
    pub(crate) fn public_export_keys(
        &self,
        public_entries: &FxHashSet<FileId>,
        root: &Path,
    ) -> FxHashSet<String> {
        self.inner.public_export_keys(public_entries, root)
    }

    /// Count direct importer modules for one file id.
    #[must_use]
    pub fn direct_importer_count(&self, file_id: FileId) -> usize {
        self.inner
            .reverse_deps
            .get(file_id.0 as usize)
            .map_or(0, Vec::len)
    }

    /// Summaries for modules that directly import one file.
    #[must_use]
    pub fn direct_importer_summaries(&self, target: FileId) -> Vec<DirectImporterSummary> {
        self.inner
            .direct_importer_summaries(target)
            .into_iter()
            .map(DirectImporterSummary::from)
            .collect()
    }
}

/// Engine-owned view of root-correlated static test reachability.
///
/// This keeps health consumers on one contract while the graph crate owns the
/// traversal and replacement-mask representation.
#[derive(Clone, Copy)]
pub(crate) struct StaticTestCoverage<'a> {
    graph: &'a ModuleGraph,
}

impl<'a> StaticTestCoverage<'a> {
    pub(crate) const fn new(graph: &'a ModuleGraph) -> Self {
        Self { graph }
    }

    pub(crate) fn covers_file(self, file_id: FileId) -> bool {
        self.graph.is_test_reachable(file_id)
    }

    pub(crate) fn covers_any_reference(self, export: &fallow_graph::graph::ExportSymbol) -> bool {
        self.graph.is_any_test_reference_covered(export)
    }
}

impl From<ModuleGraph> for RetainedModuleGraph {
    fn from(inner: ModuleGraph) -> Self {
        Self::new(inner)
    }
}

/// Engine-owned importer details for one file that directly imports a target module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectImporterSummary {
    pub source: FileId,
    pub symbols: Vec<ImportedSymbolSummary>,
}

impl From<GraphDirectImporterSummary> for DirectImporterSummary {
    fn from(summary: GraphDirectImporterSummary) -> Self {
        Self {
            source: summary.source,
            symbols: summary.symbols.into_iter().map(Into::into).collect(),
        }
    }
}

/// Engine-owned symbol details for a direct import edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedSymbolSummary {
    pub imported: String,
    pub local: String,
    pub type_only: bool,
}

impl From<GraphImportedSymbolSummary> for ImportedSymbolSummary {
    fn from(symbol: GraphImportedSymbolSummary) -> Self {
        Self {
            imported: symbol.imported,
            local: symbol.local,
            type_only: symbol.type_only,
        }
    }
}

/// Engine-owned snapshot of one value export in a module graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleValueExport {
    pub file_id: FileId,
    pub name: String,
    pub span_start: u32,
    pub test_referenced: bool,
}

/// Engine-owned impact closure with file ids resolved to paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImpactClosurePaths {
    pub in_diff: Vec<String>,
    pub affected_not_shown: Vec<String>,
    pub coordination_gap: Vec<CoordinationGapPaths>,
}

impl From<GraphImpactClosurePaths> for ImpactClosurePaths {
    fn from(paths: GraphImpactClosurePaths) -> Self {
        Self {
            in_diff: paths.in_diff,
            affected_not_shown: paths.affected_not_shown,
            coordination_gap: paths
                .coordination_gap
                .into_iter()
                .map(CoordinationGapPaths::from)
                .collect(),
        }
    }
}

/// Engine-owned coordination gap between a changed contract and consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationGapPaths {
    pub changed_file: String,
    pub consumer_file: String,
    pub consumed_symbols: Vec<String>,
}

impl From<GraphCoordinationGapPaths> for CoordinationGapPaths {
    fn from(paths: GraphCoordinationGapPaths) -> Self {
        Self {
            changed_file: paths.changed_file,
            consumer_file: paths.consumer_file,
            consumed_symbols: paths.consumed_symbols,
        }
    }
}

/// Engine-owned review partition and dependency-sensible order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartitionOrderPaths {
    pub units: Vec<ReviewUnitPaths>,
    pub order: Vec<String>,
}

impl From<GraphPartitionOrderPaths> for PartitionOrderPaths {
    fn from(paths: GraphPartitionOrderPaths) -> Self {
        Self {
            units: paths.units.into_iter().map(ReviewUnitPaths::from).collect(),
            order: paths.order,
        }
    }
}

/// Engine-owned changed-file review unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewUnitPaths {
    pub module_dir: String,
    pub files: Vec<String>,
}

impl From<GraphReviewUnitPaths> for ReviewUnitPaths {
    fn from(paths: GraphReviewUnitPaths) -> Self {
        Self {
            module_dir: paths.module_dir,
            files: paths.files,
        }
    }
}

/// Engine-owned focus facts for one changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusFileFactsPaths {
    pub file: String,
    pub fan_in: u32,
    pub fan_out: u32,
    pub dynamic_dispatch: bool,
    pub re_export_indirection: bool,
}

impl From<GraphFocusFileFactsPaths> for FocusFileFactsPaths {
    fn from(paths: GraphFocusFileFactsPaths) -> Self {
        Self {
            file: paths.file,
            fan_in: paths.fan_in,
            fan_out: paths.fan_out,
            dynamic_dispatch: paths.dynamic_dispatch,
            re_export_indirection: paths.re_export_indirection,
        }
    }
}

/// Return value exports with test-reference state without exposing graph node
/// internals to downstream crates.
#[must_use]
pub fn module_value_exports(graph: &RetainedModuleGraph) -> Vec<ModuleValueExport> {
    let test_coverage = graph.static_test_coverage();
    let graph = graph.as_graph();

    graph
        .modules
        .iter()
        .flat_map(|node| {
            node.exports
                .iter()
                .filter(|export| !export.is_type_only)
                .map(|export| ModuleValueExport {
                    file_id: node.file_id,
                    name: export.name.to_string(),
                    span_start: export.span.start,
                    test_referenced: test_coverage.covers_any_reference(export),
                })
        })
        .collect()
}

/// Compute a path-resolved impact closure for absolute changed paths.
#[must_use]
pub fn impact_closure_for_changed_paths(
    graph: &RetainedModuleGraph,
    root: &Path,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<ImpactClosurePaths> {
    let graph = graph.as_graph();
    let changed_ids = changed_file_ids(graph, changed_files);
    if changed_ids.is_empty() {
        return None;
    }

    let closure = graph.impact_closure(&changed_ids);
    Some(graph.closure_with_paths(&closure, root).into())
}

/// Compute path-resolved partition order for absolute changed paths.
#[must_use]
pub fn partition_order_for_changed_paths(
    graph: &RetainedModuleGraph,
    root: &Path,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<PartitionOrderPaths> {
    let graph = graph.as_graph();
    let changed_ids = changed_file_ids(graph, changed_files);
    if changed_ids.is_empty() {
        return None;
    }

    let partition = graph.partition_order(&changed_ids);
    Some(graph.partition_order_with_paths(&partition, root).into())
}

/// Compute path-resolved focus graph facts for absolute changed paths.
#[must_use]
pub fn focus_facts_for_changed_paths(
    graph: &RetainedModuleGraph,
    root: &Path,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<Vec<FocusFileFactsPaths>> {
    let graph = graph.as_graph();
    let changed_ids = changed_file_ids(graph, changed_files);
    if changed_ids.is_empty() {
        return None;
    }

    let facts = graph.focus_file_facts(&changed_ids);
    Some(
        graph
            .focus_facts_with_paths(&facts, root)
            .into_iter()
            .map(FocusFileFactsPaths::from)
            .collect(),
    )
}

/// Compute changed-file export line anchors without exposing graph nodes.
#[must_use]
pub fn export_lines_for_changed_paths(
    graph: &RetainedModuleGraph,
    root: &Path,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<FxHashMap<String, Vec<(String, u32)>>> {
    let graph = graph.as_graph();
    let changed_norm = normalized_changed_paths(changed_files);
    let mut map: FxHashMap<String, Vec<(String, u32)>> = FxHashMap::default();
    for module in &graph.modules {
        let abs = normalize_path(&module.path);
        if !changed_norm.contains(&abs) || module.exports.is_empty() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&module.path) else {
            continue;
        };
        let offsets = fallow_types::extract::compute_line_offsets(&content);
        let exports: Vec<(String, u32)> = module
            .exports
            .iter()
            .map(|export| {
                let (line, _) =
                    fallow_types::extract::byte_offset_to_line_col(&offsets, export.span.start);
                (export.name.to_string(), line)
            })
            .collect();
        map.insert(relative_key_path(&module.path, root), exports);
    }
    Some(map)
}

/// Compute direct non-diff internal consumer counts for absolute changed paths.
#[must_use]
pub fn internal_consumers_for_changed_paths(
    graph: &RetainedModuleGraph,
    root: &Path,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<FxHashMap<String, u64>> {
    let graph = graph.as_graph();
    let changed_norm = normalized_changed_paths(changed_files);
    let id_to_norm: FxHashMap<FileId, String> = graph
        .modules
        .iter()
        .map(|module| (module.file_id, normalize_path(&module.path)))
        .collect();

    let mut map: FxHashMap<String, u64> = FxHashMap::default();
    for module in &graph.modules {
        let abs = normalize_path(&module.path);
        if !changed_norm.contains(&abs) {
            continue;
        }
        let count = graph
            .importers_of(module.file_id)
            .iter()
            .filter(|imp| {
                id_to_norm
                    .get(imp)
                    .is_none_or(|p| !changed_norm.contains(p))
            })
            .count() as u64;
        map.insert(relative_key_path(&module.path, root), count);
    }
    Some(map)
}

fn changed_file_ids(graph: &ModuleGraph, changed_files: &FxHashSet<PathBuf>) -> Vec<FileId> {
    let path_to_id: FxHashMap<String, FileId> = graph
        .modules
        .iter()
        .map(|module| (normalize_path(&module.path), module.file_id))
        .collect();

    changed_files
        .iter()
        .filter_map(|path| path_to_id.get(&normalize_path(path)).copied())
        .collect()
}

fn normalized_changed_paths(changed_files: &FxHashSet<PathBuf>) -> FxHashSet<String> {
    changed_files
        .iter()
        .map(|path| normalize_path(path))
        .collect()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn relative_key_path(path: &Path, root: &Path) -> String {
    let simple_path = dunce::simplified(path);
    let simple_root = dunce::simplified(root);
    simple_path
        .strip_prefix(simple_root)
        .unwrap_or(simple_path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{RetainedModuleGraph, module_value_exports};
    use fallow_graph::graph::ModuleGraph;
    use fallow_graph::resolve::{
        ResolveResult, ResolvedImport, ResolvedModule, ResolvedReplacedModuleTarget,
    };
    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use fallow_types::extract::{ExportInfo, ExportName, ImportInfo, ImportedName, VisibilityTag};
    use std::path::PathBuf;

    fn import(target: FileId, imported_name: ImportedName) -> ResolvedImport {
        import_with_mechanism(target, imported_name, false)
    }

    fn import_with_mechanism(
        target: FileId,
        imported_name: ImportedName,
        commonjs: bool,
    ) -> ResolvedImport {
        ResolvedImport {
            info: ImportInfo {
                source: "./target".to_string(),
                imported_name,
                local_name: "target".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: if commonjs {
                ResolveResult::CommonJsInternalModule(target)
            } else {
                ResolveResult::InternalModule(target)
            },
        }
    }

    fn value_export(name: &str, span_start: u32) -> ExportInfo {
        ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(span_start, span_start + 10),
            members: Vec::new(),
            is_side_effect_used: false,
            super_class: None,
        }
    }

    fn mixed_root_graph(unmasked_root_imports_export: bool) -> RetainedModuleGraph {
        let files: Vec<_> = (0..3)
            .map(|id| DiscoveredFile {
                id: FileId(id),
                path: PathBuf::from(format!("/project/file{id}.ts")),
                size_bytes: 1,
            })
            .collect();
        let modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_imports: vec![import(
                    FileId(2),
                    ImportedName::Named("target".to_string()),
                )],
                ..ResolvedModule::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                resolved_imports: vec![import(
                    FileId(2),
                    if unmasked_root_imports_export {
                        ImportedName::Named("target".to_string())
                    } else {
                        ImportedName::SideEffect
                    },
                )],
                ..ResolvedModule::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: files[2].path.clone(),
                exports: vec![value_export("target", 0)],
                ..ResolvedModule::default()
            },
        ];
        let test_entry_points = vec![
            EntryPoint {
                path: files[0].path.clone(),
                source: EntryPointSource::TestFile,
            },
            EntryPoint {
                path: files[1].path.clone(),
                source: EntryPointSource::TestFile,
            },
        ];
        let graph = ModuleGraph::build_with_reachability_roots_and_replacements(
            &modules,
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(2),
            }],
            &test_entry_points,
            &[],
            &test_entry_points,
            &files,
        );
        RetainedModuleGraph::from(graph)
    }

    #[test]
    fn export_coverage_requires_one_root_to_reach_consumer_and_target() {
        let graph = mixed_root_graph(false);

        let exports = module_value_exports(&graph);

        assert_eq!(exports.len(), 1);
        assert!(!exports[0].test_referenced);
    }

    #[test]
    fn export_coverage_accepts_an_unmasked_correlated_reference() {
        let graph = mixed_root_graph(true);

        let exports = module_value_exports(&graph);

        assert_eq!(exports.len(), 1);
        assert!(exports[0].test_referenced);
    }

    #[test]
    fn commonjs_reference_does_not_credit_a_mocked_esm_export() {
        let files: Vec<_> = (0..2)
            .map(|id| DiscoveredFile {
                id: FileId(id),
                path: PathBuf::from(format!("/project/file{id}.ts")),
                size_bytes: 1,
            })
            .collect();
        let modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_imports: vec![
                    import_with_mechanism(
                        FileId(1),
                        ImportedName::Named("esmOnly".to_string()),
                        false,
                    ),
                    import_with_mechanism(
                        FileId(1),
                        ImportedName::Named("required".to_string()),
                        true,
                    ),
                ],
                ..ResolvedModule::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                exports: vec![value_export("esmOnly", 0), value_export("required", 20)],
                ..ResolvedModule::default()
            },
        ];
        let test_entry_points = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::TestFile,
        }];
        let graph =
            RetainedModuleGraph::from(ModuleGraph::build_with_reachability_roots_and_replacements(
                &modules,
                &[ResolvedReplacedModuleTarget {
                    source_file: FileId(0),
                    target_file: FileId(1),
                }],
                &test_entry_points,
                &[],
                &test_entry_points,
                &files,
            ));

        let exports = module_value_exports(&graph);
        let coverage: rustc_hash::FxHashMap<_, _> = exports
            .into_iter()
            .map(|export| (export.name, export.test_referenced))
            .collect();

        assert_eq!(coverage.get("esmOnly"), Some(&false));
        assert_eq!(coverage.get("required"), Some(&true));
    }
}
