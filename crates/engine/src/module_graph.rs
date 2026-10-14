//! Module graph contracts owned by the engine boundary.

#![allow(
    clippy::implicit_hasher,
    reason = "engine graph helpers use FxHashSet changed-file sets consistently with the rest of fallow"
)]

use std::path::{Path, PathBuf};

use fallow_output::TestAdjacency;
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
use fallow_graph::graph::{
    ExportTestReach as GraphExportTestReach, ExportTestReachResult as GraphExportTestReachResult,
    TestMaskingState as GraphTestMaskingState,
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

    /// Whether only tests that mock a module on the reference path reference
    /// `export`. Call it for an export without a covered reference.
    pub(crate) fn is_mocked_only(
        self,
        file_id: FileId,
        export: &fallow_graph::graph::ExportSymbol,
    ) -> bool {
        self.graph.test_masking_state() == GraphTestMaskingState::Active
            && self
                .graph
                .export_test_reach(file_id, export, &|_| false)
                .reach
                == GraphExportTestReach::MockedOnly
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
    /// File containing the import statement.
    pub source: FileId,
    /// Symbols this importer pulls from the target module.
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
    /// Exported name as declared by the target module.
    pub imported: String,
    /// Local binding name at the import site.
    pub local: String,
    /// True for type-only imports, which do not count as value usage.
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
    /// File declaring the export.
    pub file_id: FileId,
    /// Exported name.
    pub name: String,
    /// Byte offset where the export declaration starts.
    pub span_start: u32,
    /// True when a test file references this export.
    pub test_referenced: bool,
}

/// Engine-owned impact closure with file ids resolved to paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImpactClosurePaths {
    /// Changed files that are part of the diff under review.
    pub in_diff: Vec<String>,
    /// Files affected through the import closure but absent from the diff.
    pub affected_not_shown: Vec<String>,
    /// Changed contracts whose consumers were not updated in the same diff.
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
    /// Path of the changed file whose contract moved.
    pub changed_file: String,
    /// Path of the consumer that was not updated alongside it.
    pub consumer_file: String,
    /// Symbols the consumer imports from the changed file.
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
    /// Changed files partitioned into per-module review units.
    pub units: Vec<ReviewUnitPaths>,
    /// File paths in dependency-sensible review order (dependencies first).
    pub order: Vec<String>,
    /// Connected components of the inter-unit dependency graph: units that
    /// share no import edge with any unit outside their slice.
    pub independent_slices: Vec<Vec<String>>,
}

impl From<GraphPartitionOrderPaths> for PartitionOrderPaths {
    fn from(paths: GraphPartitionOrderPaths) -> Self {
        Self {
            units: paths.units.into_iter().map(ReviewUnitPaths::from).collect(),
            order: paths.order,
            independent_slices: paths.independent_slices,
        }
    }
}

/// Engine-owned changed-file review unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewUnitPaths {
    /// Directory the unit's files share.
    pub module_dir: String,
    /// Changed files grouped into this unit.
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
    /// Path of the changed file the facts describe.
    pub file: String,
    /// Count of distinct files importing this file (blast radius), excluding
    /// the file itself.
    pub fan_in: u32,
    /// Count of distinct files this file imports, excluding the file itself.
    pub fan_out: u32,
    /// True when the file has a dynamic-import edge in either direction, so
    /// its static reachability signal is not complete. Conservative: a file
    /// that MAY be dynamically wired carries the flag.
    pub dynamic_dispatch: bool,
    /// True when the file's reachability runs through re-export indirection,
    /// so direct importer counts understate its reach.
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

/// One consumer that a whole-word text search for an export name does not
/// find, or finds only under another local name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerSearchGapPaths {
    /// Root-relative path of the consumer, forward-slashed.
    pub consumer_file: String,
    /// The literal-search check for the consumer.
    pub check: crate::search_visibility::SearchCheck,
}

/// Literal-search gaps for one export of a changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSearchGapPaths {
    /// Root-relative path of the changed file, forward-slashed.
    pub file: String,
    /// The export name, `default` for a default export.
    pub export_name: String,
    /// 1-based line of the export.
    pub line: u32,
    /// 1-based first and last line of the top-level statement that declares
    /// the export.
    pub declaration_lines: (u32, u32),
    /// The `missed` and `renamed` consumers outside the changed files, sorted
    /// by path.
    pub consumers: Vec<ConsumerSearchGapPaths>,
}

/// The names of the exports that `source` declares, parsed as the file at
/// `path`. The names use the format of [`ChangedExport::export_name`].
#[must_use]
pub fn export_names_in_source(path: &Path, source: &str) -> FxHashSet<String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let module = fallow_extract::parse_source_to_module(FileId(0), path, source, 0, false);
    module
        .exports
        .iter()
        .map(|export| export.name.to_string())
        .collect()
}

/// One export of a changed file, with the line facts that the review brief
/// joins on the diff.
///
/// [`changed_exports_for_changed_paths`] reads and parses each changed file
/// once. The literal-search gap pass and the test reach pass share the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedExport {
    file_id: FileId,
    export_index: usize,
    /// Root-relative path of the changed file, forward-slashed.
    pub file: String,
    /// The export name, `default` for a default export.
    pub export_name: String,
    /// 1-based line of the export.
    pub line: u32,
    /// 1-based first and last line of the top-level statement that declares
    /// the export.
    pub declaration_lines: (u32, u32),
    /// True for a type-only export.
    pub is_type_only: bool,
    /// True when the export is a variable whose initializer is a literal, or
    /// an object or array of literals.
    pub literal_value: bool,
    /// True when a re-export statement (`export { x } from`) makes the export.
    pub is_re_export: bool,
}

/// List the exports of absolute changed paths with their declaration lines.
///
/// Each changed file is read and parsed once. The exports keep graph order
/// within a file, and the files are in `FileId` order. `None` when no module
/// matches a changed path.
#[must_use]
pub fn changed_exports_for_changed_paths(
    graph: &RetainedModuleGraph,
    root: &Path,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<Vec<ChangedExport>> {
    let graph = graph.as_graph();
    let mut changed_ids = changed_file_ids(graph, changed_files);
    if changed_ids.is_empty() {
        return None;
    }
    changed_ids.sort_unstable_by_key(|id| id.0);
    let mut changed_exports = Vec::new();
    for file_id in changed_ids {
        let Some(module) = graph.modules.get(file_id.0 as usize) else {
            continue;
        };
        if module.exports.is_empty() {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&module.path) else {
            continue;
        };
        let offsets = fallow_types::extract::compute_line_offsets(&source);
        let starts: Vec<u32> = module
            .exports
            .iter()
            .map(|export| export.span.start)
            .collect();
        let declarations =
            crate::search_visibility::export_declarations(&module.path, &source, &starts);
        let file = relative_key_path(&module.path, root);
        for (export_index, export) in module.exports.iter().enumerate() {
            let line =
                fallow_types::extract::byte_offset_to_line_col(&offsets, export.span.start).0;
            let declaration = declarations.get(&export.span.start);
            changed_exports.push(ChangedExport {
                file_id,
                export_index,
                file: file.clone(),
                export_name: export.name.to_string(),
                line,
                declaration_lines: declaration.map_or((line, line), |found| found.lines),
                is_type_only: export.is_type_only,
                literal_value: declaration.is_some_and(|found| found.literal_value),
                is_re_export: declaration.is_some_and(|found| found.is_re_export),
            });
        }
    }
    Some(changed_exports)
}

/// Compute the literal-search gaps of the changed exports.
///
/// For each export of a changed file, the consumers outside the changed set
/// are checked against a whole-word text search for the export name. Only
/// exports with at least one `missed` or `renamed` consumer are returned. The
/// consumer sources are read on demand.
#[must_use]
pub fn search_gaps_for_changed_paths(
    graph: &RetainedModuleGraph,
    root: &Path,
    changed_files: &FxHashSet<PathBuf>,
    changed_exports: &[ChangedExport],
) -> Option<Vec<ExportSearchGapPaths>> {
    use crate::search_visibility::{SearchTarget, SourceCache};
    use fallow_graph::graph::ExportNamespace;
    use fallow_types::trace::SearchVisibility;

    let graph = graph.as_graph();
    let changed_set: FxHashSet<FileId> =
        changed_file_ids(graph, changed_files).into_iter().collect();
    if changed_set.is_empty() {
        return None;
    }
    let mut sources = SourceCache::new(graph);
    let mut gaps = Vec::new();
    for changed in changed_exports {
        let Some(export) = graph
            .modules
            .get(changed.file_id.0 as usize)
            .and_then(|module| module.exports.get(changed.export_index))
        else {
            continue;
        };
        let mut seen = FxHashSet::default();
        let candidates: Vec<_> = export
            .references
            .iter()
            .filter(|reference| {
                !changed_set.contains(&reference.from_file)
                    && graph
                        .modules
                        .get(reference.from_file.0 as usize)
                        .is_some_and(fallow_graph::graph::ModuleNode::is_reachable)
                    && seen.insert(reference.from_file)
            })
            .collect();
        if candidates.is_empty() {
            continue;
        }
        let namespace = if export.is_type_only {
            ExportNamespace::Type
        } else {
            ExportNamespace::Value
        };
        let target = SearchTarget::new(
            graph,
            &mut sources,
            changed.file_id,
            &changed.export_name,
            namespace,
        );
        let mut consumers: Vec<ConsumerSearchGapPaths> = candidates
            .into_iter()
            .filter_map(|reference| {
                let check = target.check(graph, &mut sources, reference)?;
                if check.visibility == SearchVisibility::Found {
                    return None;
                }
                let consumer = graph.modules.get(reference.from_file.0 as usize)?;
                Some(ConsumerSearchGapPaths {
                    consumer_file: relative_key_path(&consumer.path, root),
                    check,
                })
            })
            .collect();
        if consumers.is_empty() {
            continue;
        }
        consumers.sort_by(|a, b| a.consumer_file.cmp(&b.consumer_file));
        gaps.push(ExportSearchGapPaths {
            file: changed.file.clone(),
            export_name: changed.export_name.clone(),
            line: changed.line,
            declaration_lines: changed.declaration_lines,
            consumers,
        });
    }
    Some(gaps)
}

/// Package names whose presence marks an end-to-end test runner. Such tests
/// drive a running app and do not import the source files.
const E2E_RUNNER_PACKAGES: &[&str] = &["@playwright/test", "playwright", "cypress"];

/// How tests reach one export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportTestReach {
    /// A test file references the export, and a test root covers it.
    Direct,
    /// A covered reference exists, but no test file names the export.
    Transitive,
    /// No reference is covered, and a mock replaces a module on a reference
    /// path.
    MockedOnly,
    /// A test root reaches the declaring file, but no reference is covered.
    None,
    /// No test root reaches the declaring file.
    NoneFile,
}

impl From<GraphExportTestReach> for ExportTestReach {
    fn from(reach: GraphExportTestReach) -> Self {
        match reach {
            GraphExportTestReach::Direct => Self::Direct,
            GraphExportTestReach::Transitive => Self::Transitive,
            GraphExportTestReach::MockedOnly => Self::MockedOnly,
            GraphExportTestReach::None => Self::None,
            GraphExportTestReach::NoneFile => Self::NoneFile,
        }
    }
}

/// Why replacement masking is or is not active for test reachability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestMaskingState {
    /// At least one test root replaces a module with a closed factory mock.
    Active,
    /// No test root declares a closed factory mock.
    NoMocks,
    /// Too many distinct mock sets, so masking is off.
    Capped,
}

impl From<GraphTestMaskingState> for TestMaskingState {
    fn from(state: GraphTestMaskingState) -> Self {
        match state {
            GraphTestMaskingState::Active => Self::Active,
            GraphTestMaskingState::NoMocks => Self::NoMocks,
            GraphTestMaskingState::Capped => Self::Capped,
        }
    }
}

/// How tests reach one changed value export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportTestReachPaths {
    /// Root-relative path of the changed file, forward-slashed.
    pub file: String,
    /// The export name, `default` for a default export.
    pub export_name: String,
    /// 1-based line of the export.
    pub line: u32,
    /// 1-based first and last line of the declaring statement.
    pub declaration_lines: (u32, u32),
    /// True when the export is a variable with a literal initializer.
    pub literal_value: bool,
    /// The reach class.
    pub reach: ExportTestReach,
    /// Test files that name the export, sorted: the covered test references
    /// for `Direct`, the masked test references for `MockedOnly`.
    pub tests: Vec<String>,
    /// For `MockedOnly`: the masked modules on the reference paths, sorted.
    pub mocked_targets: Vec<String>,
}

/// Test reach of the changed value exports of a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestReachPaths {
    /// False when the project has no test entry points. Every export is then
    /// `NoneFile`.
    pub has_tests: bool,
    /// Whether test reachability applies replacement masks.
    pub masking: TestMaskingState,
    /// True when the project imports an end-to-end test runner package.
    pub has_e2e_runner: bool,
    /// One row per changed value export of a changed source file.
    pub exports: Vec<ExportTestReachPaths>,
}

/// Classify how tests reach the value exports in `changed_exports`.
///
/// Type-only exports, re-exports, exports of test files, of declaration files
/// and of stylesheets are skipped. `is_test_path` classifies a root-relative,
/// forward-slashed path; a test entry point always counts as a test file.
/// The rows keep the order of `changed_exports`. `None` when the list holds no
/// such export.
#[must_use]
pub fn test_reach_for_changed_paths(
    graph: &RetainedModuleGraph,
    root: &Path,
    changed_exports: &[ChangedExport],
    is_test_path: &dyn Fn(&str) -> bool,
) -> Option<TestReachPaths> {
    let graph = graph.as_graph();
    let mut test_files: FxHashMap<FileId, bool> = FxHashMap::default();
    let mut is_test_file = |file_id: FileId| -> bool {
        *test_files.entry(file_id).or_insert_with(|| {
            graph.test_entry_points.contains(&file_id)
                || graph
                    .modules
                    .get(file_id.0 as usize)
                    .is_some_and(|module| is_test_path(&relative_key_path(&module.path, root)))
        })
    };
    let has_tests = !graph.test_entry_points.is_empty();
    let mut exports = Vec::new();
    for changed in changed_exports {
        if changed.is_type_only || changed.is_re_export {
            continue;
        }
        let Some(module) = graph.modules.get(changed.file_id.0 as usize) else {
            continue;
        };
        let Some(export) = module.exports.get(changed.export_index) else {
            continue;
        };
        if !is_reach_subject(&module.path) || is_test_file(changed.file_id) {
            continue;
        }
        let result = if has_tests {
            let test_flags: FxHashMap<FileId, bool> = export
                .references
                .iter()
                .map(|reference| (reference.from_file, is_test_file(reference.from_file)))
                .collect();
            graph.export_test_reach(changed.file_id, export, &|file_id| {
                test_flags.get(&file_id).copied().unwrap_or(false)
            })
        } else {
            GraphExportTestReachResult {
                reach: GraphExportTestReach::NoneFile,
                tests: Vec::new(),
                mocked_targets: Vec::new(),
            }
        };
        let paths_of = |files: &[FileId]| -> Vec<String> {
            let mut paths: Vec<String> = files
                .iter()
                .filter_map(|file_id| graph.modules.get(file_id.0 as usize))
                .map(|node| relative_key_path(&node.path, root))
                .collect();
            paths.sort_unstable();
            paths
        };
        exports.push(ExportTestReachPaths {
            file: changed.file.clone(),
            export_name: changed.export_name.clone(),
            line: changed.line,
            declaration_lines: changed.declaration_lines,
            literal_value: changed.literal_value,
            reach: result.reach.into(),
            tests: paths_of(&result.tests),
            mocked_targets: paths_of(&result.mocked_targets),
        });
    }
    if exports.is_empty() {
        return None;
    }
    let has_e2e_runner = E2E_RUNNER_PACKAGES.iter().any(|name| {
        graph.package_usage.contains_key(*name) || graph.type_only_package_usage.contains_key(*name)
    });
    Some(TestReachPaths {
        has_tests,
        masking: graph.test_masking_state().into(),
        has_e2e_runner,
        exports,
    })
}

/// Whether test reach applies to a file: runtime source code, not a
/// declaration file or a stylesheet.
fn is_reach_subject(path: &Path) -> bool {
    if fallow_graph::graph::is_declaration_file_path(path) {
        return false;
    }
    !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "css" | "scss" | "less" | "sass"))
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

/// Compute per-changed-file test adjacency for the review direction: whether a
/// test file imports the changed module directly, and whether one of those tests
/// is itself in the changed set. `is_test_path` classifies a root-relative,
/// forward-slashed path; the caller owns that definition so the brief's
/// weakening and direction surfaces agree on it. Changed test files get no
/// entry of their own. `None` when no module matches a changed path.
#[must_use]
pub fn test_adjacency_for_changed_paths(
    graph: &RetainedModuleGraph,
    root: &Path,
    changed_files: &FxHashSet<PathBuf>,
    is_test_path: &dyn Fn(&str) -> bool,
) -> Option<FxHashMap<String, TestAdjacency>> {
    let graph = graph.as_graph();
    // A project with no test files at all gets no adjacency claims: "no direct
    // test" on every unit would say nothing about the change.
    if !graph
        .modules
        .iter()
        .any(|module| is_test_path(&relative_key_path(&module.path, root)))
    {
        return None;
    }
    let changed_norm = normalized_changed_paths(changed_files);
    let mut map: FxHashMap<String, TestAdjacency> = FxHashMap::default();
    for module in &graph.modules {
        if !changed_norm.contains(&normalize_path(&module.path)) {
            continue;
        }
        let rel = relative_key_path(&module.path, root);
        if is_test_path(&rel) {
            continue;
        }
        let mut adjacency = TestAdjacency::None;
        for importer in graph.importers_of(module.file_id) {
            let Some(node) = graph.modules.get(importer.0 as usize) else {
                continue;
            };
            if !is_test_path(&relative_key_path(&node.path, root)) {
                continue;
            }
            if changed_norm.contains(&normalize_path(&node.path)) {
                adjacency = TestAdjacency::Changed;
                break;
            }
            adjacency = TestAdjacency::Untouched;
        }
        map.insert(rel, adjacency);
    }
    if map.is_empty() {
        return None;
    }
    Some(map)
}

/// In-repo importer counts for one third-party package.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageImporters {
    /// Modules that import the package (value imports only), id-sorted. Kept
    /// as ids rather than a count so a decision that batches several packages
    /// can take the union instead of double-counting a module that imports
    /// more than one of them.
    pub importers: Vec<FileId>,
    /// The subset of `importers` outside the changed set, id-sorted.
    pub out_of_diff: Vec<FileId>,
}

/// Compute per-package in-repo importer counts for every third-party package
/// the graph saw an import of, value or type-only, split by whether the
/// importer is in the changed set. Type-only importers count because a major
/// bump of a types package is a compile-wide change even when no value
/// crosses the boundary. The dependency decision arm reads these so a new or
/// bumped `package.json` entry carries the modules it actually reaches. `None`
/// when the graph recorded no package usage.
#[must_use]
pub fn package_importers_for_changed_paths(
    graph: &RetainedModuleGraph,
    changed_files: &FxHashSet<PathBuf>,
) -> Option<FxHashMap<String, PackageImporters>> {
    let graph = graph.as_graph();
    if graph.package_usage.is_empty() && graph.type_only_package_usage.is_empty() {
        return None;
    }
    let changed_norm = normalized_changed_paths(changed_files);
    let id_to_norm: FxHashMap<FileId, String> = graph
        .modules
        .iter()
        .map(|module| (module.file_id, normalize_path(&module.path)))
        .collect();
    let mut usage: FxHashMap<&str, Vec<FileId>> = FxHashMap::default();
    for (package, files) in graph
        .package_usage
        .iter()
        .chain(graph.type_only_package_usage.iter())
    {
        usage
            .entry(package.as_str())
            .or_default()
            .extend(files.iter().copied());
    }
    let map = usage
        .into_iter()
        .map(|(package, files)| {
            let mut importers: Vec<FileId> = files;
            importers.sort_unstable_by_key(|id| id.0);
            importers.dedup();
            let out_of_diff: Vec<FileId> = importers
                .iter()
                .copied()
                .filter(|id| id_to_norm.get(id).is_none_or(|p| !changed_norm.contains(p)))
                .collect();
            (
                package.to_string(),
                PackageImporters {
                    importers,
                    out_of_diff,
                },
            )
        })
        .collect();
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

/// Canonical path form used inside audit attribution keys: root-relative with
/// forward slashes on every platform. Paths outside `root` keep their
/// simplified absolute form so keys stay unique.
///
/// The engine keys its per-module maps with this function, and
/// `fallow-api` builds its lookup keys with the same function, so both sides
/// of the join always agree.
pub fn relative_key_path(path: &Path, root: &Path) -> String {
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
                is_type_only_star: false,
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
            deprecated: false,
            deprecated_reason: None,
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
                exports: vec![value_export("target", 0)].into(),
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
                exports: vec![value_export("esmOnly", 0), value_export("required", 20)].into(),
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

    #[test]
    fn type_only_test_import_does_not_credit_a_value_export() {
        let files: Vec<_> = (0..3)
            .map(|id| DiscoveredFile {
                id: FileId(id),
                path: PathBuf::from(format!("/project/file{id}.ts")),
                size_bytes: 1,
            })
            .collect();
        let mut type_only = import(FileId(1), ImportedName::Named("typed".to_string()));
        type_only.info.is_type_only = true;
        let modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_imports: vec![
                    type_only,
                    import(FileId(2), ImportedName::Named("called".to_string())),
                ],
                ..ResolvedModule::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                exports: vec![value_export("typed", 0)].into(),
                ..ResolvedModule::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: files[2].path.clone(),
                exports: vec![value_export("called", 0)].into(),
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
                &[],
                &test_entry_points,
                &[],
                &test_entry_points,
                &files,
            ));

        let coverage: rustc_hash::FxHashMap<_, _> = module_value_exports(&graph)
            .into_iter()
            .map(|export| (export.name, export.test_referenced))
            .collect();

        assert_eq!(coverage.get("typed"), Some(&false));
        assert_eq!(coverage.get("called"), Some(&true));
    }
}
