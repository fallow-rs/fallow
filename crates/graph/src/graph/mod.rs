//! Module dependency graph with re-export chain propagation and reachability analysis.
//!
//! The graph is built from resolved modules and entry points, then used to determine
//! which files are reachable and which exports are referenced.

mod build;
mod cycles;
mod narrowing;
mod re_exports;
mod reachability;
pub mod types;

use std::path::Path;

use fixedbitset::FixedBitSet;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::resolve::ResolvedModule;
use fallow_types::discover::{DiscoveredFile, EntryPoint, FileId};
use fallow_types::extract::ImportedName;

// Re-export all public types so downstream sees the same API as before.
pub use types::{ExportSymbol, ModuleNode, ReExportEdge, ReferenceKind, SymbolReference};

/// The core module dependency graph.
#[derive(Debug)]
pub struct ModuleGraph {
    /// All modules indexed by `FileId`.
    pub modules: Vec<ModuleNode>,
    /// Flat edge storage for cache-friendly iteration.
    edges: Vec<Edge>,
    /// Maps npm package names to the set of `FileId`s that import them.
    pub package_usage: FxHashMap<String, Vec<FileId>>,
    /// Maps npm package names to the set of `FileId`s that import them with type-only imports.
    /// A package appearing here but not in `package_usage` (or only in both) indicates
    /// it's only used for types and could be a devDependency.
    pub type_only_package_usage: FxHashMap<String, Vec<FileId>>,
    /// All entry point `FileId`s.
    pub entry_points: FxHashSet<FileId>,
    /// Runtime/application entry point `FileId`s.
    pub runtime_entry_points: FxHashSet<FileId>,
    /// Test entry point `FileId`s.
    pub test_entry_points: FxHashSet<FileId>,
    /// Reverse index: for each `FileId`, which files import it.
    pub reverse_deps: Vec<Vec<FileId>>,
    /// Precomputed: which modules have namespace imports (import * as ns).
    namespace_imported: FixedBitSet,
}

/// An edge in the module graph.
#[derive(Debug)]
pub(super) struct Edge {
    pub(super) source: FileId,
    pub(super) target: FileId,
    pub(super) symbols: Vec<ImportedSymbol>,
}

/// A symbol imported across an edge.
#[derive(Debug)]
pub(super) struct ImportedSymbol {
    pub(super) imported_name: ImportedName,
    pub(super) local_name: String,
    /// Byte span of the import statement in the source file.
    pub(super) import_span: oxc_span::Span,
    /// Whether this import is type-only (`import type { ... }`).
    /// Used to skip type-only edges in circular dependency detection.
    pub(super) is_type_only: bool,
}

// Size assertions to prevent memory regressions in hot-path graph types.
// `Edge` is stored in a flat contiguous Vec for cache-friendly traversal.
// `ImportedSymbol` is stored in a Vec per Edge.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Edge>() == 32);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ImportedSymbol>() == 64);

impl ModuleGraph {
    fn resolve_entry_point_ids(
        entry_points: &[EntryPoint],
        path_to_id: &FxHashMap<&Path, FileId>,
    ) -> FxHashSet<FileId> {
        entry_points
            .iter()
            .filter_map(|ep| {
                path_to_id.get(ep.path.as_path()).copied().or_else(|| {
                    dunce::canonicalize(&ep.path)
                        .ok()
                        .and_then(|path| path_to_id.get(path.as_path()).copied())
                })
            })
            .collect()
    }

    /// Build the module graph from resolved modules and entry points.
    pub fn build(
        resolved_modules: &[ResolvedModule],
        entry_points: &[EntryPoint],
        files: &[DiscoveredFile],
    ) -> Self {
        Self::build_with_reachability_roots(
            resolved_modules,
            entry_points,
            entry_points,
            &[],
            files,
        )
    }

    /// Build the module graph with explicit runtime and test reachability roots.
    pub fn build_with_reachability_roots(
        resolved_modules: &[ResolvedModule],
        entry_points: &[EntryPoint],
        runtime_entry_points: &[EntryPoint],
        test_entry_points: &[EntryPoint],
        files: &[DiscoveredFile],
    ) -> Self {
        let _span = tracing::info_span!("build_graph").entered();

        let module_count = files.len();

        // Compute the total capacity needed, accounting for workspace FileIds
        // that may exceed files.len() if IDs are assigned beyond the file count.
        let max_file_id = files
            .iter()
            .map(|f| f.id.0 as usize)
            .max()
            .map_or(0, |m| m + 1);
        let total_capacity = max_file_id.max(module_count);

        // Build path -> FileId index (borrows paths from files slice to avoid cloning)
        let path_to_id: FxHashMap<&Path, FileId> =
            files.iter().map(|f| (f.path.as_path(), f.id)).collect();

        // Build FileId -> ResolvedModule index
        let module_by_id: FxHashMap<FileId, &ResolvedModule> =
            resolved_modules.iter().map(|m| (m.file_id, m)).collect();

        // Build entry point set — use path_to_id map instead of O(n) scan per entry
        let entry_point_ids = Self::resolve_entry_point_ids(entry_points, &path_to_id);
        let runtime_entry_point_ids =
            Self::resolve_entry_point_ids(runtime_entry_points, &path_to_id);
        let test_entry_point_ids = Self::resolve_entry_point_ids(test_entry_points, &path_to_id);

        // Phase 1: Build flat edge storage, module nodes, and package usage from resolved modules
        let mut graph = Self::populate_edges(
            files,
            &module_by_id,
            &entry_point_ids,
            &runtime_entry_point_ids,
            &test_entry_point_ids,
            module_count,
            total_capacity,
        );

        // Phase 2: Record which files reference which exports (namespace + CSS module narrowing)
        graph.populate_references(&module_by_id, &entry_point_ids);

        // Phase 3: BFS from entry points to mark overall/runtime/test reachability
        graph.mark_reachable(
            &entry_point_ids,
            &runtime_entry_point_ids,
            &test_entry_point_ids,
            total_capacity,
        );

        // Phase 4: Propagate references through re-export chains
        graph.resolve_re_export_chains();

        graph
    }

    /// Total number of modules.
    #[must_use]
    pub const fn module_count(&self) -> usize {
        self.modules.len()
    }

    /// Total number of edges.
    #[must_use]
    pub const fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Check if any importer uses `import * as ns` for this module.
    /// Uses precomputed bitset — O(1) lookup.
    #[must_use]
    pub fn has_namespace_import(&self, file_id: FileId) -> bool {
        let idx = file_id.0 as usize;
        if idx >= self.namespace_imported.len() {
            return false;
        }
        self.namespace_imported.contains(idx)
    }

    /// Get the target `FileId`s of all outgoing edges for a module.
    #[must_use]
    pub fn edges_for(&self, file_id: FileId) -> Vec<FileId> {
        let idx = file_id.0 as usize;
        if idx >= self.modules.len() {
            return Vec::new();
        }
        let range = &self.modules[idx].edge_range;
        self.edges[range.clone()].iter().map(|e| e.target).collect()
    }

    /// Find the byte offset of the first import statement from `source` to `target`.
    /// Returns `None` if no edge exists or the edge has no symbols.
    #[must_use]
    pub fn find_import_span_start(&self, source: FileId, target: FileId) -> Option<u32> {
        let idx = source.0 as usize;
        if idx >= self.modules.len() {
            return None;
        }
        let range = &self.modules[idx].edge_range;
        for edge in &self.edges[range.clone()] {
            if edge.target == target {
                return edge.symbols.first().map(|s| s.import_span.start);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use fallow_types::extract::{ExportName, ImportInfo, ImportedName, VisibilityTag};
    use std::path::PathBuf;

    // Helper to build a simple module graph
    fn build_simple_graph() -> ModuleGraph {
        // Two files: entry.ts imports foo from utils.ts
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/src/utils.ts"),
                size_bytes: 50,
            },
        ];

        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/src/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];

        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./utils".to_string(),
                        imported_name: ImportedName::Named("foo".to_string()),
                        local_name: "foo".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/src/utils.ts"),
                exports: vec![
                    fallow_types::extract::ExportInfo {
                        name: ExportName::Named("foo".to_string()),
                        local_name: Some("foo".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                    fallow_types::extract::ExportInfo {
                        name: ExportName::Named("bar".to_string()),
                        local_name: Some("bar".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(25, 45),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                ],
                ..Default::default()
            },
        ];

        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    }

    #[test]
    fn graph_module_count() {
        let graph = build_simple_graph();
        assert_eq!(graph.module_count(), 2);
    }

    #[test]
    fn graph_edge_count() {
        let graph = build_simple_graph();
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn graph_entry_point_is_reachable() {
        let graph = build_simple_graph();
        assert!(graph.modules[0].is_entry_point());
        assert!(graph.modules[0].is_reachable());
    }

    #[test]
    fn graph_imported_module_is_reachable() {
        let graph = build_simple_graph();
        assert!(!graph.modules[1].is_entry_point());
        assert!(graph.modules[1].is_reachable());
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "this test fixture exercises four reachability roles end-to-end; splitting it \
                  would obscure the cross-role assertions"
    )]
    fn graph_distinguishes_runtime_test_and_support_reachability() {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/src/main.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/src/runtime-only.ts"),
                size_bytes: 50,
            },
            DiscoveredFile {
                id: FileId(2),
                path: PathBuf::from("/project/tests/app.test.ts"),
                size_bytes: 50,
            },
            DiscoveredFile {
                id: FileId(3),
                path: PathBuf::from("/project/tests/setup.ts"),
                size_bytes: 50,
            },
            DiscoveredFile {
                id: FileId(4),
                path: PathBuf::from("/project/src/covered.ts"),
                size_bytes: 50,
            },
        ];

        let all_entry_points = vec![
            EntryPoint {
                path: PathBuf::from("/project/src/main.ts"),
                source: EntryPointSource::PackageJsonMain,
            },
            EntryPoint {
                path: PathBuf::from("/project/tests/app.test.ts"),
                source: EntryPointSource::TestFile,
            },
            EntryPoint {
                path: PathBuf::from("/project/tests/setup.ts"),
                source: EntryPointSource::Plugin {
                    name: "vitest".to_string(),
                },
            },
        ];
        let runtime_entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/src/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let test_entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/tests/app.test.ts"),
            source: EntryPointSource::TestFile,
        }];

        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/src/main.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./runtime-only".to_string(),
                        imported_name: ImportedName::Named("runtimeOnly".to_string()),
                        local_name: "runtimeOnly".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/src/runtime-only.ts"),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Named("runtimeOnly".to_string()),
                    local_name: Some("runtimeOnly".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/tests/app.test.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "../src/covered".to_string(),
                        imported_name: ImportedName::Named("covered".to_string()),
                        local_name: "covered".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(4)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: PathBuf::from("/project/tests/setup.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "../src/runtime-only".to_string(),
                        imported_name: ImportedName::Named("runtimeOnly".to_string()),
                        local_name: "runtimeOnly".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(4),
                path: PathBuf::from("/project/src/covered.ts"),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Named("covered".to_string()),
                    local_name: Some("covered".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build_with_reachability_roots(
            &resolved_modules,
            &all_entry_points,
            &runtime_entry_points,
            &test_entry_points,
            &files,
        );

        assert!(graph.modules[1].is_reachable());
        assert!(graph.modules[1].is_runtime_reachable());
        assert!(
            !graph.modules[1].is_test_reachable(),
            "support roots should not make runtime-only modules test reachable"
        );

        assert!(graph.modules[4].is_reachable());
        assert!(graph.modules[4].is_test_reachable());
        assert!(
            !graph.modules[4].is_runtime_reachable(),
            "test-only reachability should stay separate from runtime roots"
        );
    }

    #[test]
    fn graph_export_has_reference() {
        let graph = build_simple_graph();
        let utils = &graph.modules[1];
        let foo_export = utils
            .exports
            .iter()
            .find(|e| e.name.to_string() == "foo")
            .unwrap();
        assert!(
            !foo_export.references.is_empty(),
            "foo should have references"
        );
    }

    #[test]
    fn graph_unused_export_no_reference() {
        let graph = build_simple_graph();
        let utils = &graph.modules[1];
        let bar_export = utils
            .exports
            .iter()
            .find(|e| e.name.to_string() == "bar")
            .unwrap();
        assert!(
            bar_export.references.is_empty(),
            "bar should have no references"
        );
    }

    #[test]
    fn graph_no_namespace_import() {
        let graph = build_simple_graph();
        assert!(!graph.has_namespace_import(FileId(0)));
        assert!(!graph.has_namespace_import(FileId(1)));
    }

    #[test]
    fn graph_has_namespace_import() {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/utils.ts"),
                size_bytes: 50,
            },
        ];

        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];

        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./utils".to_string(),
                        imported_name: ImportedName::Namespace,
                        local_name: "utils".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/utils.ts"),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        assert!(
            graph.has_namespace_import(FileId(1)),
            "utils should have namespace import"
        );
    }

    #[test]
    fn graph_has_namespace_import_out_of_bounds() {
        let graph = build_simple_graph();
        assert!(!graph.has_namespace_import(FileId(999)));
    }

    #[test]
    fn graph_unreachable_module() {
        // Three files: entry imports utils, orphan is not imported
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/utils.ts"),
                size_bytes: 50,
            },
            DiscoveredFile {
                id: FileId(2),
                path: PathBuf::from("/project/orphan.ts"),
                size_bytes: 30,
            },
        ];

        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];

        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./utils".to_string(),
                        imported_name: ImportedName::Named("foo".to_string()),
                        local_name: "foo".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/utils.ts"),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/orphan.ts"),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Named("orphan".to_string()),
                    local_name: Some("orphan".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

        assert!(graph.modules[0].is_reachable(), "entry should be reachable");
        assert!(graph.modules[1].is_reachable(), "utils should be reachable");
        assert!(
            !graph.modules[2].is_reachable(),
            "orphan should NOT be reachable"
        );
    }

    #[test]
    fn graph_package_usage_tracked() {
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        }];

        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![
                ResolvedImport {
                    info: ImportInfo {
                        source: "react".to_string(),
                        imported_name: ImportedName::Default,
                        local_name: "React".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::NpmPackage("react".to_string()),
                },
                ResolvedImport {
                    info: ImportInfo {
                        source: "lodash".to_string(),
                        imported_name: ImportedName::Named("merge".to_string()),
                        local_name: "merge".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(15, 30),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::NpmPackage("lodash".to_string()),
                },
            ],
            ..Default::default()
        }];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        assert!(graph.package_usage.contains_key("react"));
        assert!(graph.package_usage.contains_key("lodash"));
        assert!(!graph.package_usage.contains_key("express"));
    }

    #[test]
    fn graph_empty() {
        let graph = ModuleGraph::build(&[], &[], &[]);
        assert_eq!(graph.module_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn graph_cjs_exports_tracked() {
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        }];

        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            has_cjs_exports: true,
            ..Default::default()
        }];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        assert!(graph.modules[0].has_cjs_exports());
    }

    #[test]
    fn graph_edges_for_returns_targets() {
        let graph = build_simple_graph();
        let targets = graph.edges_for(FileId(0));
        assert_eq!(targets, vec![FileId(1)]);
    }

    #[test]
    fn graph_edges_for_no_imports() {
        let graph = build_simple_graph();
        // utils.ts has no outgoing imports
        let targets = graph.edges_for(FileId(1));
        assert!(targets.is_empty());
    }

    #[test]
    fn graph_edges_for_out_of_bounds() {
        let graph = build_simple_graph();
        let targets = graph.edges_for(FileId(999));
        assert!(targets.is_empty());
    }

    #[test]
    fn graph_find_import_span_start_found() {
        let graph = build_simple_graph();
        let span_start = graph.find_import_span_start(FileId(0), FileId(1));
        assert!(span_start.is_some());
        assert_eq!(span_start.unwrap(), 0);
    }

    #[test]
    fn graph_find_import_span_start_wrong_target() {
        let graph = build_simple_graph();
        // No edge from entry.ts to itself
        let span_start = graph.find_import_span_start(FileId(0), FileId(0));
        assert!(span_start.is_none());
    }

    #[test]
    fn graph_find_import_span_start_source_out_of_bounds() {
        let graph = build_simple_graph();
        let span_start = graph.find_import_span_start(FileId(999), FileId(1));
        assert!(span_start.is_none());
    }

    #[test]
    fn graph_find_import_span_start_no_edges() {
        let graph = build_simple_graph();
        // utils.ts has no outgoing edges
        let span_start = graph.find_import_span_start(FileId(1), FileId(0));
        assert!(span_start.is_none());
    }

    #[test]
    fn graph_reverse_deps_populated() {
        let graph = build_simple_graph();
        // utils.ts (FileId(1)) should be imported by entry.ts (FileId(0))
        assert!(graph.reverse_deps[1].contains(&FileId(0)));
        // entry.ts (FileId(0)) should not be imported by anyone
        assert!(graph.reverse_deps[0].is_empty());
    }

    #[test]
    fn graph_type_only_package_usage_tracked() {
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        }];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            resolved_imports: vec![
                ResolvedImport {
                    info: ImportInfo {
                        source: "react".to_string(),
                        imported_name: ImportedName::Named("FC".to_string()),
                        local_name: "FC".to_string(),
                        is_type_only: true,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::NpmPackage("react".to_string()),
                },
                ResolvedImport {
                    info: ImportInfo {
                        source: "react".to_string(),
                        imported_name: ImportedName::Named("useState".to_string()),
                        local_name: "useState".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(15, 30),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::NpmPackage("react".to_string()),
                },
            ],
            ..Default::default()
        }];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        assert!(graph.package_usage.contains_key("react"));
        assert!(graph.type_only_package_usage.contains_key("react"));
    }

    #[test]
    fn graph_default_import_reference() {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/utils.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./utils".to_string(),
                        imported_name: ImportedName::Default,
                        local_name: "Utils".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/utils.ts"),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Default,
                    local_name: None,
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let utils = &graph.modules[1];
        let default_export = utils
            .exports
            .iter()
            .find(|e| matches!(e.name, ExportName::Default))
            .unwrap();
        assert!(!default_export.references.is_empty());
        assert_eq!(
            default_export.references[0].kind,
            ReferenceKind::DefaultImport
        );
    }

    #[test]
    fn graph_side_effect_import_no_export_reference() {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/styles.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./styles".to_string(),
                        imported_name: ImportedName::SideEffect,
                        local_name: String::new(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/styles.ts"),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Named("primaryColor".to_string()),
                    local_name: Some("primaryColor".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        // Side-effect import should create an edge but not reference specific exports
        assert_eq!(graph.edge_count(), 1);
        let styles = &graph.modules[1];
        let export = &styles.exports[0];
        // Side-effect import doesn't match any named export
        assert!(
            export.references.is_empty(),
            "side-effect import should not reference named exports"
        );
    }

    #[test]
    fn graph_multiple_entry_points() {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/main.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/worker.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(2),
                path: PathBuf::from("/project/shared.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![
            EntryPoint {
                path: PathBuf::from("/project/main.ts"),
                source: EntryPointSource::PackageJsonMain,
            },
            EntryPoint {
                path: PathBuf::from("/project/worker.ts"),
                source: EntryPointSource::PackageJsonMain,
            },
        ];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/main.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./shared".to_string(),
                        imported_name: ImportedName::Named("helper".to_string()),
                        local_name: "helper".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/worker.ts"),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/shared.ts"),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Named("helper".to_string()),
                    local_name: Some("helper".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        assert!(graph.modules[0].is_entry_point());
        assert!(graph.modules[1].is_entry_point());
        assert!(!graph.modules[2].is_entry_point());
        // All should be reachable — shared is reached from main
        assert!(graph.modules[0].is_reachable());
        assert!(graph.modules[1].is_reachable());
        assert!(graph.modules[2].is_reachable());
    }
}
