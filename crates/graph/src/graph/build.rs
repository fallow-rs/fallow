//! Phase 1 (populate_edges) and Phase 2 (populate_references) of graph construction.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{ExportName, ImportedName, VisibilityTag};

use super::narrowing::attach_symbol_reference;
use super::types::ModuleNode;
use super::types::{ExportSymbol, ReExportEdge};
use super::{Edge, ImportedSymbol, ModuleGraph};

/// Mutable accumulator state shared across all files during edge population.
struct EdgeAccumulator {
    package_usage: FxHashMap<String, Vec<FileId>>,
    type_only_package_usage: FxHashMap<String, Vec<FileId>>,
    namespace_imported: fixedbitset::FixedBitSet,
    total_capacity: usize,
}

/// Insert into the namespace-imported bitset with bounds checking.
fn record_namespace_import(
    target_id: FileId,
    namespace_imported: &mut fixedbitset::FixedBitSet,
    total_capacity: usize,
) {
    let idx = target_id.0 as usize;
    if idx < total_capacity {
        namespace_imported.insert(idx);
    }
}

/// Track that a file uses an npm package, and optionally record type-only usage.
fn record_package_usage(
    acc: &mut EdgeAccumulator,
    name: &str,
    file_id: FileId,
    is_type_only: bool,
) {
    acc.package_usage
        .entry(name.to_owned())
        .or_default()
        .push(file_id);
    if is_type_only {
        acc.type_only_package_usage
            .entry(name.to_owned())
            .or_default()
            .push(file_id);
    }
}

/// Process a single resolved import (static or dynamic), adding it to the edge map.
///
/// Internal module imports create an `ImportedSymbol` entry grouped by target.
/// Namespace imports are also recorded in the namespace-imported bitset.
/// npm package imports are recorded in the package usage maps.
fn collect_import_edge(
    import: &ResolvedImport,
    file_id: FileId,
    edges_by_target: &mut FxHashMap<FileId, Vec<ImportedSymbol>>,
    acc: &mut EdgeAccumulator,
) {
    match &import.target {
        ResolveResult::InternalModule(target_id) => {
            if matches!(import.info.imported_name, ImportedName::Namespace) {
                record_namespace_import(
                    *target_id,
                    &mut acc.namespace_imported,
                    acc.total_capacity,
                );
            }
            edges_by_target
                .entry(*target_id)
                .or_default()
                .push(ImportedSymbol {
                    imported_name: import.info.imported_name.clone(),
                    local_name: import.info.local_name.clone(),
                    import_span: import.info.span,
                    is_type_only: import.info.is_type_only,
                });
        }
        ResolveResult::NpmPackage(name) => {
            record_package_usage(acc, name, file_id, import.info.is_type_only);
        }
        _ => {}
    }
}

/// Collect edges from a resolved module's static imports, re-exports, dynamic imports,
/// and dynamic import patterns into a grouped edge map.
///
/// Returns the grouped edges sorted by target `FileId` for deterministic ordering.
fn collect_edges_for_module(
    resolved: &ResolvedModule,
    file_id: FileId,
    acc: &mut EdgeAccumulator,
) -> Vec<(FileId, Vec<ImportedSymbol>)> {
    let mut edges_by_target: FxHashMap<FileId, Vec<ImportedSymbol>> = FxHashMap::default();

    // Static imports
    for import in &resolved.resolved_imports {
        collect_import_edge(import, file_id, &mut edges_by_target, acc);
    }

    // Re-exports — use SideEffect edges to avoid marking source exports as "used"
    // just because they're re-exported. Re-export chain propagation handles tracking
    // which specific names consumers actually import.
    for re_export in &resolved.re_exports {
        if let ResolveResult::InternalModule(target_id) = &re_export.target {
            edges_by_target
                .entry(*target_id)
                .or_default()
                .push(ImportedSymbol {
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    import_span: oxc_span::Span::new(0, 0),
                    is_type_only: re_export.info.is_type_only,
                });
        } else if let ResolveResult::NpmPackage(name) = &re_export.target {
            record_package_usage(acc, name, file_id, re_export.info.is_type_only);
        }
    }

    // Dynamic imports — Named imports create Named edges, Namespace imports create
    // Namespace edges with a local_name (enabling member access narrowing),
    // Side-effect imports create SideEffect edges.
    for import in &resolved.resolved_dynamic_imports {
        collect_import_edge(import, file_id, &mut edges_by_target, acc);
    }

    // Dynamic import patterns (template literals, string concat, import.meta.glob)
    for (_pattern, matched_ids) in &resolved.resolved_dynamic_patterns {
        for target_id in matched_ids {
            record_namespace_import(*target_id, &mut acc.namespace_imported, acc.total_capacity);
            edges_by_target
                .entry(*target_id)
                .or_default()
                .push(ImportedSymbol {
                    imported_name: ImportedName::Namespace,
                    local_name: String::new(),
                    import_span: oxc_span::Span::new(0, 0),
                    is_type_only: false,
                });
        }
    }

    // Sort by target FileId for deterministic edge order across runs
    let mut sorted: Vec<_> = edges_by_target.into_iter().collect();
    sorted.sort_by_key(|(target_id, _)| target_id.0);
    sorted
}

/// Build a `ModuleNode` for a file, including exports, re-export edges, and metadata.
fn build_module_node(
    file: &DiscoveredFile,
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    entry_point_ids: &FxHashSet<FileId>,
    edge_range: std::ops::Range<usize>,
) -> ModuleNode {
    let mut exports: Vec<ExportSymbol> = module_by_id
        .get(&file.id)
        .map(|m| {
            m.exports
                .iter()
                .map(|e| ExportSymbol {
                    name: e.name.clone(),
                    is_type_only: e.is_type_only,
                    is_side_effect_used: e.is_side_effect_used,
                    visibility: e.visibility,
                    span: e.span,
                    references: Vec::new(),
                    members: e.members.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    // Create ExportSymbol entries for re-exports so that consumers
    // importing from this barrel can have their references attached.
    // Without this, `export { Foo } from './source'` on a barrel would
    // not be trackable as an export of the barrel module.
    if let Some(resolved) = module_by_id.get(&file.id) {
        for re in &resolved.re_exports {
            // Skip star re-exports without an alias (`export * from './x'`)
            // — they don't create a named export on the barrel.
            // But `export * as name from './x'` does create one.
            if re.info.exported_name == "*" {
                continue;
            }

            // Avoid duplicates: if an export with this name already exists
            // (e.g. the module both declares and re-exports the same name),
            // skip creating another one.
            let export_name = if re.info.exported_name == "default" {
                ExportName::Default
            } else {
                ExportName::Named(re.info.exported_name.clone())
            };
            let already_exists = exports.iter().any(|e| e.name == export_name);
            if already_exists {
                continue;
            }

            exports.push(ExportSymbol {
                name: export_name,
                is_type_only: re.info.is_type_only,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                // Use the real span from the visitor when available; falls back
                // to (0, 0) for re-exports synthesized inside the graph layer.
                span: re.info.span,
                references: Vec::new(),
                members: Vec::new(),
            });
        }
    }

    let has_cjs_exports = module_by_id
        .get(&file.id)
        .is_some_and(|m| m.has_cjs_exports);

    // Build re-export edges
    let re_export_edges: Vec<ReExportEdge> = module_by_id
        .get(&file.id)
        .map(|m| {
            m.re_exports
                .iter()
                .filter_map(|re| {
                    if let ResolveResult::InternalModule(target_id) = &re.target {
                        Some(ReExportEdge {
                            source_file: *target_id,
                            imported_name: re.info.imported_name.clone(),
                            exported_name: re.info.exported_name.clone(),
                            is_type_only: re.info.is_type_only,
                            span: re.info.span,
                        })
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    ModuleNode {
        file_id: file.id,
        path: file.path.clone(),
        edge_range,
        exports,
        re_exports: re_export_edges,
        flags: ModuleNode::flags_from(entry_point_ids.contains(&file.id), false, has_cjs_exports),
    }
}

impl ModuleGraph {
    /// Build flat edge storage from resolved modules.
    ///
    /// Creates `ModuleNode` entries, flat `Edge` storage, reverse dependency
    /// indices, package usage maps, and the namespace-imported bitset.
    pub(super) fn populate_edges(
        files: &[DiscoveredFile],
        module_by_id: &FxHashMap<FileId, &ResolvedModule>,
        entry_point_ids: &FxHashSet<FileId>,
        runtime_entry_point_ids: &FxHashSet<FileId>,
        test_entry_point_ids: &FxHashSet<FileId>,
        module_count: usize,
        total_capacity: usize,
    ) -> Self {
        let mut all_edges = Vec::new();
        let mut modules = Vec::with_capacity(module_count);
        let mut reverse_deps = vec![Vec::new(); total_capacity];
        let mut acc = EdgeAccumulator {
            package_usage: FxHashMap::default(),
            type_only_package_usage: FxHashMap::default(),
            namespace_imported: fixedbitset::FixedBitSet::with_capacity(total_capacity),
            total_capacity,
        };

        for file in files {
            let edge_start = all_edges.len();

            if let Some(resolved) = module_by_id.get(&file.id) {
                let sorted_edges = collect_edges_for_module(resolved, file.id, &mut acc);

                for (target_id, symbols) in sorted_edges {
                    all_edges.push(Edge {
                        source: file.id,
                        target: target_id,
                        symbols,
                    });

                    if (target_id.0 as usize) < reverse_deps.len() {
                        reverse_deps[target_id.0 as usize].push(file.id);
                    }
                }
            }

            let edge_end = all_edges.len();

            modules.push(build_module_node(
                file,
                module_by_id,
                entry_point_ids,
                edge_start..edge_end,
            ));
        }

        Self {
            modules,
            edges: all_edges,
            package_usage: acc.package_usage,
            type_only_package_usage: acc.type_only_package_usage,
            entry_points: entry_point_ids.clone(),
            runtime_entry_points: runtime_entry_point_ids.clone(),
            test_entry_points: test_entry_point_ids.clone(),
            reverse_deps,
            namespace_imported: acc.namespace_imported,
        }
    }

    /// Record which files reference which exports from edges.
    ///
    /// Walks every edge and attaches `SymbolReference` entries to the target
    /// module's exports. Includes namespace import narrowing (member access
    /// tracking) and CSS Module default-import narrowing.
    pub(super) fn populate_references(
        &mut self,
        module_by_id: &FxHashMap<FileId, &ResolvedModule>,
        entry_point_ids: &FxHashSet<FileId>,
    ) {
        for edge_idx in 0..self.edges.len() {
            let source_id = self.edges[edge_idx].source;
            let target_idx = self.edges[edge_idx].target.0 as usize;
            if target_idx >= self.modules.len() {
                continue;
            }
            for sym_idx in 0..self.edges[edge_idx].symbols.len() {
                let sym = &self.edges[edge_idx].symbols[sym_idx];
                attach_symbol_reference(
                    &mut self.modules[target_idx],
                    source_id,
                    sym,
                    module_by_id,
                    entry_point_ids,
                );
            }
        }
    }
}

/// Check if a path is a CSS Module file (`.module.css` or `.module.scss`).
pub(super) fn is_css_module_path(path: &std::path::Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|stem| stem.ends_with(".module"))
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext == "css" || ext == "scss")
}

/// Check if an export name matches an imported name.
pub(super) fn export_matches(export: &ExportName, import: &ImportedName) -> bool {
    match (export, import) {
        (ExportName::Named(e), ImportedName::Named(i)) => e == i,
        (ExportName::Default, ImportedName::Default) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
    use fallow_types::discover::{DiscoveredFile, FileId};
    use fallow_types::extract::ImportedName;

    // ── export_matches ─────────────────────────────────────────────────

    #[test]
    fn export_matches_named_same() {
        assert!(export_matches(
            &ExportName::Named("foo".to_string()),
            &ImportedName::Named("foo".to_string())
        ));
    }

    #[test]
    fn export_matches_named_different() {
        assert!(!export_matches(
            &ExportName::Named("foo".to_string()),
            &ImportedName::Named("bar".to_string())
        ));
    }

    #[test]
    fn export_matches_default() {
        assert!(export_matches(&ExportName::Default, &ImportedName::Default));
    }

    #[test]
    fn export_matches_named_vs_default() {
        assert!(!export_matches(
            &ExportName::Named("foo".to_string()),
            &ImportedName::Default
        ));
    }

    #[test]
    fn export_matches_default_vs_named() {
        assert!(!export_matches(
            &ExportName::Default,
            &ImportedName::Named("foo".to_string())
        ));
    }

    #[test]
    fn export_matches_namespace_no_match() {
        assert!(!export_matches(
            &ExportName::Named("foo".to_string()),
            &ImportedName::Namespace
        ));
        assert!(!export_matches(
            &ExportName::Default,
            &ImportedName::Namespace
        ));
    }

    #[test]
    fn export_matches_side_effect_no_match() {
        assert!(!export_matches(
            &ExportName::Named("foo".to_string()),
            &ImportedName::SideEffect
        ));
    }

    // ── is_css_module_path ──────────────────────────────────────────────

    #[test]
    fn css_module_path_css() {
        assert!(is_css_module_path(std::path::Path::new(
            "Button.module.css"
        )));
    }

    #[test]
    fn css_module_path_scss() {
        assert!(is_css_module_path(std::path::Path::new(
            "Button.module.scss"
        )));
    }

    #[test]
    fn css_module_path_plain_css() {
        assert!(!is_css_module_path(std::path::Path::new("Button.css")));
    }

    #[test]
    fn css_module_path_ts() {
        assert!(!is_css_module_path(std::path::Path::new(
            "Button.module.ts"
        )));
    }

    #[test]
    fn css_module_path_less_not_matched() {
        // .module.less is not supported (only .css and .scss)
        assert!(!is_css_module_path(std::path::Path::new(
            "Button.module.less"
        )));
    }

    #[test]
    fn css_module_path_nested_directory() {
        assert!(is_css_module_path(std::path::Path::new(
            "/project/src/components/Button.module.css"
        )));
    }

    #[test]
    fn css_module_path_no_extension() {
        assert!(!is_css_module_path(std::path::Path::new("Button.module")));
    }

    #[test]
    fn css_module_path_double_module() {
        // Edge case: file like "Button.module.module.css"
        assert!(is_css_module_path(std::path::Path::new(
            "Button.module.module.css"
        )));
    }

    // ── record_namespace_import ─────────────────────────────────────────

    #[test]
    fn record_namespace_import_within_bounds() {
        let mut bitset = fixedbitset::FixedBitSet::with_capacity(4);
        record_namespace_import(FileId(2), &mut bitset, 4);
        assert!(bitset.contains(2));
    }

    #[test]
    fn record_namespace_import_out_of_bounds() {
        let mut bitset = fixedbitset::FixedBitSet::with_capacity(4);
        record_namespace_import(FileId(10), &mut bitset, 4);
        // Should silently skip — bitset unchanged
        assert!(!bitset.contains(3));
    }

    // ── record_package_usage ────────────────────────────────────────────

    #[test]
    fn record_package_usage_non_type_only() {
        let mut acc = EdgeAccumulator {
            package_usage: FxHashMap::default(),
            type_only_package_usage: FxHashMap::default(),
            namespace_imported: fixedbitset::FixedBitSet::with_capacity(4),
            total_capacity: 4,
        };
        record_package_usage(&mut acc, "react", FileId(0), false);
        assert_eq!(acc.package_usage["react"], vec![FileId(0)]);
        assert!(!acc.type_only_package_usage.contains_key("react"));
    }

    #[test]
    fn record_package_usage_type_only() {
        let mut acc = EdgeAccumulator {
            package_usage: FxHashMap::default(),
            type_only_package_usage: FxHashMap::default(),
            namespace_imported: fixedbitset::FixedBitSet::with_capacity(4),
            total_capacity: 4,
        };
        record_package_usage(&mut acc, "react", FileId(1), true);
        assert_eq!(acc.package_usage["react"], vec![FileId(1)]);
        assert_eq!(acc.type_only_package_usage["react"], vec![FileId(1)]);
    }

    #[test]
    fn record_package_usage_multiple_files() {
        let mut acc = EdgeAccumulator {
            package_usage: FxHashMap::default(),
            type_only_package_usage: FxHashMap::default(),
            namespace_imported: fixedbitset::FixedBitSet::with_capacity(4),
            total_capacity: 4,
        };
        record_package_usage(&mut acc, "lodash", FileId(0), false);
        record_package_usage(&mut acc, "lodash", FileId(1), true);
        assert_eq!(acc.package_usage["lodash"], vec![FileId(0), FileId(1)]);
        assert_eq!(acc.type_only_package_usage["lodash"], vec![FileId(1)]);
    }

    // ── collect_import_edge ─────────────────────────────────────────────

    fn make_acc(cap: usize) -> EdgeAccumulator {
        EdgeAccumulator {
            package_usage: FxHashMap::default(),
            type_only_package_usage: FxHashMap::default(),
            namespace_imported: fixedbitset::FixedBitSet::with_capacity(cap),
            total_capacity: cap,
        }
    }

    fn make_import(imported_name: ImportedName, target: ResolveResult) -> ResolvedImport {
        ResolvedImport {
            info: fallow_types::extract::ImportInfo {
                source: "./target".to_string(),
                imported_name,
                local_name: "localVar".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target,
        }
    }

    #[test]
    fn collect_import_edge_named_internal() {
        let mut acc = make_acc(4);
        let mut edges: FxHashMap<FileId, Vec<ImportedSymbol>> = FxHashMap::default();
        let import = make_import(
            ImportedName::Named("foo".to_string()),
            ResolveResult::InternalModule(FileId(2)),
        );
        collect_import_edge(&import, FileId(0), &mut edges, &mut acc);

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[&FileId(2)].len(), 1);
        assert!(matches!(
            edges[&FileId(2)][0].imported_name,
            ImportedName::Named(ref n) if n == "foo"
        ));
        assert!(!acc.namespace_imported.contains(2));
    }

    #[test]
    fn collect_import_edge_default_internal() {
        let mut acc = make_acc(4);
        let mut edges: FxHashMap<FileId, Vec<ImportedSymbol>> = FxHashMap::default();
        let import = make_import(
            ImportedName::Default,
            ResolveResult::InternalModule(FileId(1)),
        );
        collect_import_edge(&import, FileId(0), &mut edges, &mut acc);

        assert_eq!(edges[&FileId(1)].len(), 1);
        assert!(matches!(
            edges[&FileId(1)][0].imported_name,
            ImportedName::Default
        ));
    }

    #[test]
    fn collect_import_edge_namespace_sets_bitset() {
        let mut acc = make_acc(4);
        let mut edges: FxHashMap<FileId, Vec<ImportedSymbol>> = FxHashMap::default();
        let import = make_import(
            ImportedName::Namespace,
            ResolveResult::InternalModule(FileId(3)),
        );
        collect_import_edge(&import, FileId(0), &mut edges, &mut acc);

        assert!(acc.namespace_imported.contains(3));
        assert_eq!(edges[&FileId(3)].len(), 1);
    }

    #[test]
    fn collect_import_edge_side_effect_internal() {
        let mut acc = make_acc(4);
        let mut edges: FxHashMap<FileId, Vec<ImportedSymbol>> = FxHashMap::default();
        let import = make_import(
            ImportedName::SideEffect,
            ResolveResult::InternalModule(FileId(1)),
        );
        collect_import_edge(&import, FileId(0), &mut edges, &mut acc);

        assert_eq!(edges[&FileId(1)].len(), 1);
        assert!(matches!(
            edges[&FileId(1)][0].imported_name,
            ImportedName::SideEffect
        ));
        // Side-effect should NOT set namespace bitset
        assert!(!acc.namespace_imported.contains(1));
    }

    #[test]
    fn collect_import_edge_npm_package() {
        let mut acc = make_acc(4);
        let mut edges: FxHashMap<FileId, Vec<ImportedSymbol>> = FxHashMap::default();
        let import = make_import(
            ImportedName::Named("merge".to_string()),
            ResolveResult::NpmPackage("lodash".to_string()),
        );
        collect_import_edge(&import, FileId(0), &mut edges, &mut acc);

        assert!(edges.is_empty(), "npm packages should not create edges");
        assert_eq!(acc.package_usage["lodash"], vec![FileId(0)]);
    }

    #[test]
    fn collect_import_edge_npm_type_only() {
        let mut acc = make_acc(4);
        let mut edges: FxHashMap<FileId, Vec<ImportedSymbol>> = FxHashMap::default();
        let import = ResolvedImport {
            info: fallow_types::extract::ImportInfo {
                source: "react".to_string(),
                imported_name: ImportedName::Named("FC".to_string()),
                local_name: "FC".to_string(),
                is_type_only: true,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("react".to_string()),
        };
        collect_import_edge(&import, FileId(0), &mut edges, &mut acc);

        assert_eq!(acc.package_usage["react"], vec![FileId(0)]);
        assert_eq!(acc.type_only_package_usage["react"], vec![FileId(0)]);
    }

    #[test]
    fn collect_import_edge_external_file_ignored() {
        let mut acc = make_acc(4);
        let mut edges: FxHashMap<FileId, Vec<ImportedSymbol>> = FxHashMap::default();
        let import = make_import(
            ImportedName::Named("x".to_string()),
            ResolveResult::ExternalFile(std::path::PathBuf::from("/node_modules/foo/index.js")),
        );
        collect_import_edge(&import, FileId(0), &mut edges, &mut acc);

        assert!(edges.is_empty());
        assert!(acc.package_usage.is_empty());
    }

    #[test]
    fn collect_import_edge_unresolvable_ignored() {
        let mut acc = make_acc(4);
        let mut edges: FxHashMap<FileId, Vec<ImportedSymbol>> = FxHashMap::default();
        let import = make_import(
            ImportedName::Named("x".to_string()),
            ResolveResult::Unresolvable("./missing".to_string()),
        );
        collect_import_edge(&import, FileId(0), &mut edges, &mut acc);

        assert!(edges.is_empty());
    }

    // ── collect_edges_for_module ─────────────────────────────────────────

    #[test]
    fn collect_edges_sorted_by_target_id() {
        let resolved = ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            resolved_imports: vec![
                ResolvedImport {
                    info: fallow_types::extract::ImportInfo {
                        source: "./c".to_string(),
                        imported_name: ImportedName::Named("c".to_string()),
                        local_name: "c".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 5),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(3)),
                },
                ResolvedImport {
                    info: fallow_types::extract::ImportInfo {
                        source: "./a".to_string(),
                        imported_name: ImportedName::Named("a".to_string()),
                        local_name: "a".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(10, 15),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
            ],
            ..Default::default()
        };
        let mut acc = make_acc(4);
        let sorted = collect_edges_for_module(&resolved, FileId(0), &mut acc);

        // Should be sorted: FileId(1) before FileId(3)
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].0, FileId(1));
        assert_eq!(sorted[1].0, FileId(3));
    }

    #[test]
    fn collect_edges_re_exports_use_side_effect() {
        let resolved = ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/barrel.ts"),
            re_exports: vec![crate::resolve::ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./utils".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        };
        let mut acc = make_acc(4);
        let sorted = collect_edges_for_module(&resolved, FileId(0), &mut acc);

        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].0, FileId(1));
        assert!(matches!(
            sorted[0].1[0].imported_name,
            ImportedName::SideEffect
        ));
    }

    #[test]
    fn collect_edges_re_export_npm_records_usage() {
        let resolved = ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/barrel.ts"),
            re_exports: vec![crate::resolve::ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "react".to_string(),
                    imported_name: "useState".to_string(),
                    exported_name: "useState".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("react".to_string()),
            }],
            ..Default::default()
        };
        let mut acc = make_acc(4);
        let sorted = collect_edges_for_module(&resolved, FileId(0), &mut acc);

        assert!(sorted.is_empty(), "npm re-exports should not create edges");
        assert_eq!(acc.package_usage["react"], vec![FileId(0)]);
    }

    #[test]
    fn collect_edges_dynamic_patterns_set_namespace() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./locales/".to_string(),
            suffix: Some(".json".to_string()),
            span: oxc_span::Span::new(0, 10),
        };
        let resolved = ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/i18n.ts"),
            resolved_dynamic_patterns: vec![(pattern, vec![FileId(1), FileId(2)])],
            ..Default::default()
        };
        let mut acc = make_acc(4);
        let sorted = collect_edges_for_module(&resolved, FileId(0), &mut acc);

        assert_eq!(sorted.len(), 2);
        assert!(acc.namespace_imported.contains(1));
        assert!(acc.namespace_imported.contains(2));
    }

    // ── build_module_node: star re-export skips creating export symbol ──

    #[test]
    fn star_re_export_does_not_create_named_export_symbol() {
        // `export * from './source'` should NOT create an ExportSymbol on the barrel
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: std::path::PathBuf::from("/project/barrel.ts"),
                size_bytes: 50,
            },
            DiscoveredFile {
                id: FileId(1),
                path: std::path::PathBuf::from("/project/source.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![fallow_types::discover::EntryPoint {
            path: std::path::PathBuf::from("/project/barrel.ts"),
            source: fallow_types::discover::EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: std::path::PathBuf::from("/project/barrel.ts"),
                re_exports: vec![crate::resolve::ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./source".to_string(),
                        imported_name: "*".to_string(),
                        exported_name: "*".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: std::path::PathBuf::from("/project/source.ts"),
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
        let barrel = &graph.modules[0];
        // Star re-exports should NOT create named ExportSymbol entries
        // (they are handled by re-export chain propagation instead)
        assert!(
            barrel.exports.is_empty(),
            "star re-export should not create named export symbols on barrel"
        );
    }

    // ── duplicate re-export: skip if export already exists ──────────

    #[test]
    fn re_export_skips_duplicate_export_name() {
        // If a module both declares and re-exports the same name, only one
        // ExportSymbol should exist.
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: std::path::PathBuf::from("/project/barrel.ts"),
            size_bytes: 50,
        }];
        let entry_points = vec![fallow_types::discover::EntryPoint {
            path: std::path::PathBuf::from("/project/barrel.ts"),
            source: fallow_types::discover::EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/barrel.ts"),
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
            re_exports: vec![crate::resolve::ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        }];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let barrel = &graph.modules[0];
        assert_eq!(
            barrel
                .exports
                .iter()
                .filter(|e| e.name.to_string() == "foo")
                .count(),
            1,
            "duplicate export name from re-export should be skipped"
        );
    }
}
