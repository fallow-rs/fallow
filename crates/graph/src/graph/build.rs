//! Phase 1 (populate_edges) and Phase 2 (populate_references) of graph construction.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::resolve::{ResolvedImport, ResolvedModule};
use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{ExportName, ImportedName, ModuleLoadMechanism, VisibilityTag};

use super::narrowing::{AttachContext, ReferenceDedup, attach_symbol_reference};
use super::types::{ExportSymbol, ReExportEdge};
use super::types::{ModuleNode, ReferencePathInterner};
use super::{Edge, ImportedSymbol, ModuleGraph};

pub(super) struct PopulateEdgesInput<'a> {
    pub(super) files: &'a [DiscoveredFile],
    pub(super) module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    pub(super) entry_point_ids: &'a FxHashSet<FileId>,
    pub(super) runtime_entry_point_ids: &'a FxHashSet<FileId>,
    pub(super) test_entry_point_ids: &'a FxHashSet<FileId>,
    pub(super) module_count: usize,
    pub(super) total_capacity: usize,
}

/// The one importable name that both `ExportName` and `ImportedName` can
/// spell either as their `Default` variant or as a `Named` string.
const DEFAULT_EXPORT_NAME: &str = "default";

#[derive(Clone, Copy, Default)]
pub(super) struct NamespaceFeatures {
    pub(super) has_aliases: bool,
    pub(super) has_re_exports: bool,
}

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
    if let Some(package_name) = import.target.package_usage_name() {
        record_package_usage(acc, package_name, file_id, import.info.is_type_only);
    }

    if let Some(target_id) = import.target.internal_file_id() {
        if matches!(import.info.imported_name, ImportedName::Namespace) {
            record_namespace_import(target_id, &mut acc.namespace_imported, acc.total_capacity);
        }
        edges_by_target
            .entry(target_id)
            .or_default()
            .push(ImportedSymbol {
                imported_name: import.info.imported_name.clone(),
                local_name: import.info.local_name.clone(),
                import_span: import.info.span,
                is_type_only: import.info.is_type_only,
                mechanism: if import.target.is_commonjs_require() {
                    ModuleLoadMechanism::CommonJsRequire
                } else {
                    ModuleLoadMechanism::EsModule
                },
            });
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

    for import in &resolved.resolved_imports {
        collect_import_edge(import, file_id, &mut edges_by_target, acc);
    }

    for re_export in &resolved.re_exports {
        if let Some(package_name) = re_export.target.package_usage_name() {
            record_package_usage(acc, package_name, file_id, re_export.info.is_type_only);
        }
        if let Some(target_id) = re_export.target.internal_file_id() {
            edges_by_target
                .entry(target_id)
                .or_default()
                .push(ImportedSymbol {
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    import_span: oxc_span::Span::new(0, 0),
                    is_type_only: re_export.info.is_type_only,
                    mechanism: ModuleLoadMechanism::EsModule,
                });
        }
    }

    for import in &resolved.resolved_dynamic_imports {
        collect_import_edge(import, file_id, &mut edges_by_target, acc);
    }

    // Patterns from `import()`, `import.meta.glob`, and `require.context` each
    // resolve to a set of target files. A single importer can hold many patterns
    // whose match sets overlap heavily, so duplicate matches would otherwise add
    // redundant namespace symbols and references. Deduplicate by target and load
    // mechanism: matches with the same mechanism carry no additional information,
    // while ESM and CommonJS matches must remain distinct for mock-aware coverage.
    // The set is per-file, so different importers still create their own edges.
    let mut credited_pattern_targets: FxHashSet<(FileId, ModuleLoadMechanism)> =
        FxHashSet::default();
    for (pattern, matched_ids) in &resolved.resolved_dynamic_patterns {
        for target_id in matched_ids {
            if !credited_pattern_targets.insert((*target_id, pattern.mechanism)) {
                continue;
            }
            record_namespace_import(*target_id, &mut acc.namespace_imported, acc.total_capacity);
            edges_by_target
                .entry(*target_id)
                .or_default()
                .push(ImportedSymbol {
                    imported_name: ImportedName::Namespace,
                    local_name: String::new(),
                    import_span: oxc_span::Span::new(0, 0),
                    is_type_only: false,
                    mechanism: pattern.mechanism,
                });
        }
    }

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
) -> (ModuleNode, NamespaceFeatures) {
    let resolved = module_by_id.get(&file.id).copied();

    let mut exports = build_export_symbols(resolved);
    if let Some(resolved) = resolved {
        append_named_re_export_stubs(&mut exports, resolved);
    }

    let has_cjs_exports = resolved.is_some_and(|m| m.has_cjs_exports);
    let (re_export_edges, has_namespace_re_exports) = build_re_export_edges(resolved);
    let has_namespace_aliases = resolved.is_some_and(|m| !m.namespace_object_aliases.is_empty());

    (
        ModuleNode {
            file_id: file.id,
            path: file.path.clone(),
            edge_range,
            exports,
            re_exports: re_export_edges,
            flags: ModuleNode::flags_from(
                entry_point_ids.contains(&file.id),
                false,
                has_cjs_exports,
            ),
        },
        NamespaceFeatures {
            has_aliases: has_namespace_aliases,
            has_re_exports: has_namespace_re_exports,
        },
    )
}

/// Copy a resolved module's own exports into fresh `ExportSymbol` entries
/// (references start empty; they are populated in Phase 2).
fn build_export_symbols(resolved: Option<&ResolvedModule>) -> Vec<ExportSymbol> {
    resolved
        .map(|m| {
            m.exports
                .iter()
                .map(|e| ExportSymbol {
                    name: e.name.clone(),
                    is_type_only: e.is_type_only,
                    is_side_effect_used: e.is_side_effect_used,
                    visibility: e.visibility,
                    expected_unused_reason: e.expected_unused_reason.clone(),
                    span: e.span,
                    references: Vec::new(),
                    reference_paths: Vec::new(),
                    members: e.members.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Add a synthetic `ExportSymbol` for each named re-export that does not already
/// have a same-named local export. Star re-exports are skipped.
fn append_named_re_export_stubs(exports: &mut Vec<ExportSymbol>, resolved: &ResolvedModule) {
    const LINEAR_SCAN_LIMIT: usize = 8;
    if resolved.re_exports.len() <= LINEAR_SCAN_LIMIT {
        append_named_re_export_stubs_linear(exports, resolved);
        return;
    }

    let named_re_export_count = resolved
        .re_exports
        .iter()
        .filter(|re_export| re_export.info.exported_name != "*")
        .count();
    exports.reserve(named_re_export_count);

    let mut named_exports: FxHashSet<&str> = FxHashSet::default();
    named_exports.reserve(resolved.exports.len().saturating_add(named_re_export_count));
    let mut has_default = false;
    for export in resolved.exports.iter() {
        match &export.name {
            ExportName::Named(name) => {
                named_exports.insert(name.as_str());
            }
            ExportName::Default => has_default = true,
        }
    }

    for re in &resolved.re_exports {
        if re.info.exported_name == "*" {
            continue;
        }
        let export_name = if re.info.exported_name == "default" {
            if std::mem::replace(&mut has_default, true) {
                continue;
            }
            ExportName::Default
        } else {
            if !named_exports.insert(re.info.exported_name.as_str()) {
                continue;
            }
            ExportName::Named(re.info.exported_name.clone())
        };

        push_re_export_stub(exports, export_name, re);
    }
}

fn append_named_re_export_stubs_linear(exports: &mut Vec<ExportSymbol>, resolved: &ResolvedModule) {
    for re in &resolved.re_exports {
        if re.info.exported_name == "*" {
            continue;
        }
        let export_name = if re.info.exported_name == "default" {
            ExportName::Default
        } else {
            ExportName::Named(re.info.exported_name.clone())
        };
        if exports.iter().any(|export| export.name == export_name) {
            continue;
        }
        push_re_export_stub(exports, export_name, re);
    }
}

fn push_re_export_stub(
    exports: &mut Vec<ExportSymbol>,
    name: ExportName,
    re_export: &crate::resolve::ResolvedReExport,
) {
    exports.push(ExportSymbol {
        name,
        is_type_only: re_export.info.is_type_only,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: re_export.info.span,
        references: Vec::new(),
        reference_paths: Vec::new(),
        members: Vec::new(),
    });
}

/// Build the internal re-export edge list for a module (external re-export
/// targets are dropped here; they are handled via package usage).
fn build_re_export_edges(resolved: Option<&ResolvedModule>) -> (Vec<ReExportEdge>, bool) {
    let Some(resolved) = resolved else {
        return (Vec::new(), false);
    };
    let mut has_namespace_re_exports = false;
    let edges = resolved
        .re_exports
        .iter()
        .filter_map(|re| {
            has_namespace_re_exports |=
                re.info.imported_name == "*" && re.info.exported_name != "*";
            re.target.internal_file_id().map(|target_id| ReExportEdge {
                source_file: target_id,
                imported_name: re.info.imported_name.clone(),
                exported_name: re.info.exported_name.clone(),
                is_type_only: re.info.is_type_only,
                span: re.info.span,
            })
        })
        .collect();
    (edges, has_namespace_re_exports)
}

impl ModuleGraph {
    /// Build flat edge storage from resolved modules.
    ///
    /// Creates `ModuleNode` entries, flat `Edge` storage, reverse dependency
    /// indices, package usage maps, and the namespace-imported bitset.
    pub(super) fn populate_edges(input: &PopulateEdgesInput<'_>) -> (Self, NamespaceFeatures) {
        let files = input.files;
        let module_by_id = input.module_by_id;
        let entry_point_ids = input.entry_point_ids;
        let runtime_entry_point_ids = input.runtime_entry_point_ids;
        let test_entry_point_ids = input.test_entry_point_ids;
        let module_count = input.module_count;
        let total_capacity = input.total_capacity;
        let mut all_edges = Vec::new();
        let mut modules = Vec::with_capacity(module_count);
        let mut reverse_deps = vec![Vec::new(); total_capacity];
        let mut namespace_features = NamespaceFeatures::default();
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

            let (module, features) =
                build_module_node(file, module_by_id, entry_point_ids, edge_start..edge_end);
            namespace_features.has_aliases |= features.has_aliases;
            namespace_features.has_re_exports |= features.has_re_exports;
            modules.push(module);
        }

        (
            Self {
                modules,
                edges: all_edges,
                package_usage: acc.package_usage,
                type_only_package_usage: acc.type_only_package_usage,
                entry_points: entry_point_ids.clone(),
                runtime_entry_points: runtime_entry_point_ids.clone(),
                test_entry_points: test_entry_point_ids.clone(),
                test_reachability_index: super::TestReachabilityIndex::default(),
                reference_paths: Vec::new(),
                reference_routes: super::types::ReferenceRoutes::default(),
                reverse_deps,
                namespace_imported: acc.namespace_imported,
                re_export_cycles: Vec::new(),
                effective_exports: super::effective_exports::EffectiveExportIndex::default(),
            },
            namespace_features,
        )
    }

    /// Record which files reference which exports from edges.
    ///
    /// Walks every edge and attaches `SymbolReference` entries to the target
    /// module's exports. Includes namespace import narrowing (member access
    /// tracking) and CSS Module default-import narrowing.
    ///
    /// Returns the targets whose whole namespace object a consumer observed
    /// (the seeds of `ModuleGraph::collect_exposed_namespace_targets`), so the
    /// namespace re-export and star propagation phases can credit the names
    /// those targets only expose through their own re-export chains.
    pub(super) fn populate_references(
        &mut self,
        module_by_id: &FxHashMap<FileId, &ResolvedModule>,
        entry_point_ids: &FxHashSet<FileId>,
        reference_paths: &mut ReferencePathInterner,
    ) -> super::re_exports::WholeModuleObservations {
        // Both maps are transient acceleration state for this pass: the name
        // index gives O(1) export lookup per imported symbol instead of a scan
        // over all target exports, and the dedup index keeps duplicate-
        // reference checks O(1) for high-fan-in exports. Dropping them here
        // keeps `references` as the only durable storage.
        let mut dedup = ReferenceDedup::default();
        let mut export_indices: FxHashMap<usize, ExportNameIndex> = FxHashMap::default();
        let mut whole_module_targets = super::re_exports::WholeModuleObservations::default();
        for edge_idx in 0..self.edges.len() {
            let source_id = self.edges[edge_idx].source;
            let target_id = self.edges[edge_idx].target;
            let target_idx = target_id.0 as usize;
            if target_idx >= self.modules.len() {
                continue;
            }
            for sym_idx in 0..self.edges[edge_idx].symbols.len() {
                let sym = &self.edges[edge_idx].symbols[sym_idx];
                if matches!(sym.imported_name, ImportedName::SideEffect) {
                    // Preserve the direct path interned by `attach_symbol_reference`.
                    // Side-effect imports affect reachability but never reference an
                    // export, so building the target's name index cannot change output.
                    let _ = reference_paths.direct(target_id, sym.mechanism);
                    continue;
                }
                let module = &mut self.modules[target_idx];
                let export_index = export_indices
                    .entry(target_idx)
                    .and_modify(|index| index.sync(&module.exports))
                    .or_insert_with(|| ExportNameIndex::build(&module.exports));
                attach_symbol_reference(
                    module,
                    source_id,
                    sym,
                    reference_paths,
                    AttachContext {
                        module_by_id,
                        entry_point_ids,
                        export_index,
                        effective_exports: &self.effective_exports,
                        dedup: &mut dedup,
                        whole_module_targets: &mut whole_module_targets,
                    },
                );
            }
        }
        whole_module_targets
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

/// Per-module index of exports by importable name: `ExportName::Named`
/// matches `ImportedName::Named` with the same string, the default slot
/// matches a default import, and namespace or side-effect imports match
/// nothing.
///
/// `default` is one importable name spelled two ways on each side, so both
/// spellings share the default slot (issue #2374). An export declares it as
/// `ExportName::Default` (`export default x`) or as `ExportName::Named`
/// (`export { x as default }`, which the extractor keeps under its written
/// name); an import names it as `ImportedName::Default` (`import x from`) or
/// as `ImportedName::Named` (`import { default as x } from`, and the ambient
/// `declare module '<specifier>' { export { default } from './impl' }` form,
/// which records one named type-space import per specifier). Keying those on
/// the spelling left every mixed pairing uncredited, so the target's default
/// export reported as unused.
///
/// Built once per target module in `populate_references` and reused across
/// all of that module's incoming edge symbols, so wide barrels stop paying a
/// full export scan per imported symbol (same shape as the star-propagation
/// index from the issue #1843 follow-up). Per-name lists are appended in
/// ascending export order, so lookups return exactly what the removed
/// enumerate-and-filter scan produced.
pub(super) struct ExportNameIndex {
    named: FxHashMap<String, Vec<usize>>,
    default: Vec<usize>,
    indexed_len: usize,
}

impl ExportNameIndex {
    pub(super) fn build(exports: &[ExportSymbol]) -> Self {
        let mut index = Self {
            named: FxHashMap::default(),
            default: Vec::new(),
            indexed_len: 0,
        };
        index.sync(exports);
        index
    }

    /// Index exports appended since the last sync. Namespace narrowing pushes
    /// synthetic star re-export stubs mid-pass; exports are append-only, so
    /// picking up the tail keeps every per-name list complete and ascending.
    ///
    /// `export { x as default }` lands in the default slot rather than under
    /// the string key, so the slot holds every export that declares the
    /// default in ascending order and the named map never carries a
    /// `"default"` key.
    pub(super) fn sync(&mut self, exports: &[ExportSymbol]) {
        for (idx, export) in exports.iter().enumerate().skip(self.indexed_len) {
            match &export.name {
                ExportName::Named(name) if name == DEFAULT_EXPORT_NAME => self.default.push(idx),
                ExportName::Named(name) => self.named.entry(name.clone()).or_default().push(idx),
                ExportName::Default => self.default.push(idx),
            }
        }
        self.indexed_len = exports.len();
    }

    /// Indices of exports matching `import`, in ascending export order.
    ///
    /// A named import of `default` is a default import (`import { default as
    /// x } from './m'` binds the same export as `import x from './m'`), so it
    /// reads the default slot.
    pub(super) fn matches(&self, import: &ImportedName) -> &[usize] {
        match import {
            ImportedName::Named(name) if name == DEFAULT_EXPORT_NAME => &self.default,
            ImportedName::Named(name) => self.named.get(name).map_or(&[], Vec::as_slice),
            ImportedName::Default => &self.default,
            ImportedName::Namespace | ImportedName::SideEffect => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
    use fallow_types::discover::{DiscoveredFile, FileId};
    use fallow_types::extract::ImportedName;

    fn make_export(name: ExportName) -> ExportSymbol {
        ExportSymbol {
            name,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 0),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
        }
    }

    #[test]
    fn export_name_index_matches_named_and_default() {
        let exports = vec![
            make_export(ExportName::Named("foo".to_string())),
            make_export(ExportName::Default),
            make_export(ExportName::Named("foo".to_string())),
            make_export(ExportName::Named("bar".to_string())),
        ];
        let index = ExportNameIndex::build(&exports);

        assert_eq!(
            index.matches(&ImportedName::Named("foo".to_string())),
            &[0, 2]
        );
        assert_eq!(index.matches(&ImportedName::Named("bar".to_string())), &[3]);
        assert_eq!(index.matches(&ImportedName::Default), &[1]);
        assert!(
            index
                .matches(&ImportedName::Named("missing".to_string()))
                .is_empty()
        );
    }

    /// Issue #2374: `default` is one importable name however each side spells
    /// it, so both spellings read the same slot and that slot holds both
    /// declaration forms in ascending export order.
    #[test]
    fn export_name_index_matches_default_under_both_spellings() {
        let exports = vec![
            make_export(ExportName::Named("foo".to_string())),
            make_export(ExportName::Default),
            make_export(ExportName::Named("default".to_string())),
            make_export(ExportName::Named("bar".to_string())),
        ];
        let index = ExportNameIndex::build(&exports);

        assert_eq!(index.matches(&ImportedName::Default), &[1, 2]);
        assert_eq!(
            index.matches(&ImportedName::Named("default".to_string())),
            &[1, 2]
        );
        // The default slot never leaks into an unrelated name lookup.
        assert_eq!(index.matches(&ImportedName::Named("foo".to_string())), &[0]);
        assert_eq!(index.matches(&ImportedName::Named("bar".to_string())), &[3]);
    }

    /// A module that only declares `export { x as default }` still answers a
    /// plain `import x from './m'`.
    #[test]
    fn export_name_index_matches_default_declared_only_as_a_named_export() {
        let exports = vec![make_export(ExportName::Named("default".to_string()))];
        let index = ExportNameIndex::build(&exports);

        assert_eq!(index.matches(&ImportedName::Default), &[0]);
        assert_eq!(
            index.matches(&ImportedName::Named("default".to_string())),
            &[0]
        );
    }

    #[test]
    fn export_name_index_namespace_and_side_effect_match_nothing() {
        let exports = vec![
            make_export(ExportName::Named("foo".to_string())),
            make_export(ExportName::Default),
        ];
        let index = ExportNameIndex::build(&exports);

        assert!(index.matches(&ImportedName::Namespace).is_empty());
        assert!(index.matches(&ImportedName::SideEffect).is_empty());
    }

    #[test]
    fn export_name_index_sync_picks_up_appended_exports() {
        let mut exports = vec![make_export(ExportName::Named("foo".to_string()))];
        let mut index = ExportNameIndex::build(&exports);

        exports.push(make_export(ExportName::Named("foo".to_string())));
        exports.push(make_export(ExportName::Default));
        index.sync(&exports);

        assert_eq!(
            index.matches(&ImportedName::Named("foo".to_string())),
            &[0, 1]
        );
        assert_eq!(index.matches(&ImportedName::Default), &[2]);
    }

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
        assert!(is_css_module_path(std::path::Path::new(
            "Button.module.module.css"
        )));
    }

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
        assert!(!bitset.contains(3));
    }

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
        assert_eq!(
            edges[&FileId(2)][0].mechanism,
            ModuleLoadMechanism::EsModule
        );
    }

    #[test]
    fn collect_import_edge_retains_commonjs_mechanism() {
        let mut acc = make_acc(4);
        let mut edges: FxHashMap<FileId, Vec<ImportedSymbol>> = FxHashMap::default();
        let import = make_import(
            ImportedName::Namespace,
            ResolveResult::CommonJsInternalModule(FileId(2)),
        );

        collect_import_edge(&import, FileId(0), &mut edges, &mut acc);

        assert_eq!(
            edges[&FileId(2)][0].mechanism,
            ModuleLoadMechanism::CommonJsRequire
        );
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
                    statement_span: oxc_span::Span::new(0, 0),
                    source_span: oxc_span::Span::new(0, 0),
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
                    statement_span: oxc_span::Span::new(0, 0),
                    source_span: oxc_span::Span::new(0, 0),
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
            mechanism: ModuleLoadMechanism::EsModule,
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

    #[test]
    fn collect_edges_dynamic_patterns_credit_each_target_once() {
        // One importing file holding three dynamic-import patterns whose match sets
        // all overlap on FileId(1). Without per-file dedup, FileId(1) would accrue one
        // Namespace symbol per matching pattern (the O(patterns * files) blow-up of
        // issue #963). With dedup it is credited exactly once.
        let mk = |prefix: &str| fallow_types::extract::DynamicImportPattern {
            prefix: prefix.to_string(),
            suffix: None,
            span: oxc_span::Span::new(0, 1),
            mechanism: ModuleLoadMechanism::EsModule,
        };
        let resolved = ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/loader.ts"),
            resolved_dynamic_patterns: vec![
                (mk("./a/"), vec![FileId(1), FileId(2)]),
                (mk("./b/"), vec![FileId(1)]),
                (mk("./c/"), vec![FileId(1)]),
            ],
            ..Default::default()
        };
        let mut acc = make_acc(4);
        let sorted = collect_edges_for_module(&resolved, FileId(0), &mut acc);

        assert_eq!(sorted.len(), 2, "two distinct targets (FileId 1 and 2)");
        let target_one = sorted
            .iter()
            .find(|(t, _)| *t == FileId(1))
            .expect("target 1 present");
        assert_eq!(
            target_one.1.len(),
            1,
            "FileId(1) credited once despite three matching patterns"
        );
        assert!(acc.namespace_imported.contains(1));
        assert!(acc.namespace_imported.contains(2));
    }

    #[test]
    fn collect_edges_dynamic_patterns_dedup_is_per_importing_file() {
        // Two different importing files both pattern-match the same target FileId(2).
        // The dedup set is per-file (rebuilt per `collect_edges_for_module` call), so
        // each importer independently creates its own edge to FileId(2). A global
        // dedup would silently drop the second importer's reachability contribution.
        let mk = || fallow_types::extract::DynamicImportPattern {
            prefix: "./x/".to_string(),
            suffix: None,
            span: oxc_span::Span::new(0, 1),
            mechanism: ModuleLoadMechanism::EsModule,
        };
        let importer_a = ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/a.ts"),
            resolved_dynamic_patterns: vec![(mk(), vec![FileId(2)])],
            ..Default::default()
        };
        let importer_b = ResolvedModule {
            file_id: FileId(1),
            path: std::path::PathBuf::from("/project/b.ts"),
            resolved_dynamic_patterns: vec![(mk(), vec![FileId(2)])],
            ..Default::default()
        };
        let mut acc = make_acc(4);
        let edges_a = collect_edges_for_module(&importer_a, FileId(0), &mut acc);
        let edges_b = collect_edges_for_module(&importer_b, FileId(1), &mut acc);

        assert_eq!(
            edges_a.len(),
            1,
            "importer A creates its own edge to target 2"
        );
        assert_eq!(
            edges_b.len(),
            1,
            "importer B independently creates its own edge to target 2"
        );
        assert_eq!(edges_a[0].0, FileId(2));
        assert_eq!(edges_b[0].0, FileId(2));
    }

    #[test]
    fn collect_edges_dynamic_patterns_dedup_by_target_and_mechanism() {
        let pattern = |mechanism| fallow_types::extract::DynamicImportPattern {
            prefix: "./modules/".to_string(),
            suffix: None,
            span: oxc_span::Span::new(0, 1),
            mechanism,
        };
        let resolved = ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/loader.ts"),
            resolved_dynamic_patterns: vec![
                (pattern(ModuleLoadMechanism::EsModule), vec![FileId(1)]),
                (
                    pattern(ModuleLoadMechanism::CommonJsRequire),
                    vec![FileId(1)],
                ),
            ],
            ..Default::default()
        };
        let mut acc = make_acc(2);

        let edges = collect_edges_for_module(&resolved, FileId(0), &mut acc);
        let mechanisms: FxHashSet<_> = edges[0].1.iter().map(|symbol| symbol.mechanism).collect();

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].1.len(), 2);
        assert_eq!(
            mechanisms,
            FxHashSet::from_iter([
                ModuleLoadMechanism::EsModule,
                ModuleLoadMechanism::CommonJsRequire,
            ])
        );
    }

    #[test]
    fn star_re_export_does_not_create_named_export_symbol() {
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
                        statement_span: oxc_span::Span::new(0, 0),
                        source_span: oxc_span::Span::new(0, 0),
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
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }]
                .into(),
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let barrel = &graph.modules[0];
        assert!(
            barrel.exports.is_empty(),
            "star re-export should not create named export symbols on barrel"
        );
    }

    #[test]
    fn re_export_skips_duplicate_export_name() {
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
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }]
            .into(),
            re_exports: vec![crate::resolve::ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                    statement_span: oxc_span::Span::new(0, 0),
                    source_span: oxc_span::Span::new(0, 0),
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

    #[test]
    fn duplicate_named_re_exports_keep_first_metadata_and_source_order() {
        let make_re_export =
            |name: &str, is_type_only: bool, start: u32| crate::resolve::ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: name.to_string(),
                    exported_name: name.to_string(),
                    is_type_only,
                    span: oxc_span::Span::new(start, start + 1),
                    statement_span: oxc_span::Span::new(0, 0),
                    source_span: oxc_span::Span::new(0, 0),
                },
                target: ResolveResult::Unresolvable("./source".to_string()),
            };
        let resolved = ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/barrel.ts"),
            re_exports: vec![
                make_re_export("first", true, 10),
                make_re_export("first", false, 20),
                make_re_export("second", false, 30),
                make_re_export("third", false, 40),
                make_re_export("fourth", false, 50),
                make_re_export("fifth", false, 60),
                make_re_export("sixth", false, 70),
                make_re_export("seventh", false, 80),
                make_re_export("eighth", false, 90),
            ],
            ..Default::default()
        };
        let mut exports = Vec::new();

        append_named_re_export_stubs(&mut exports, &resolved);

        assert_eq!(exports.len(), 8);
        assert_eq!(exports[0].name, ExportName::Named("first".to_string()));
        assert!(exports[0].is_type_only);
        assert_eq!(exports[0].span, oxc_span::Span::new(10, 11));
        assert_eq!(exports[1].name, ExportName::Named("second".to_string()));
    }
}
