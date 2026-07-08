use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::UnusedFile;
use crate::suppress::{IssueKind, SuppressionContext};

use super::predicates::{
    is_barrel_with_reachable_sources, is_config_file, is_declaration_file, is_html_file,
};

/// Find files that are not reachable from any entry point.
///
/// TypeScript declaration files (`.d.ts`) are excluded because they are consumed
/// by the TypeScript compiler via `tsconfig.json` includes, not via explicit
/// import statements. Flagging them as unused is a false positive.
///
/// Configuration files (e.g., `babel.config.js`, `.eslintrc.js`, `knip.config.ts`)
/// are also excluded because they are consumed by tools, not via imports.
///
/// HTML files are excluded because they are entry-point-like: nothing imports
/// an HTML file, so "unused" is meaningless. They serve as app shells in
/// Vite/Parcel-style projects and their referenced assets are tracked via edges.
///
/// Barrel files (index.ts that only re-export) are excluded when their re-export
/// sources are reachable , they serve an organizational purpose even if consumers
/// import directly from the source files rather than through the barrel.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for typed output; serialize with fallow_api::serialize_dead_code_programmatic_json for JSON output. See docs/fallow-core-migration.md."
)]
pub fn find_unused_files(
    graph: &ModuleGraph,
    suppressions: &SuppressionContext<'_>,
) -> Vec<UnusedFile> {
    graph
        .modules
        .iter()
        .filter(|m| !m.is_reachable() && !m.is_entry_point())
        .filter(|m| !is_declaration_file(&m.path))
        .filter(|m| !is_config_file(&m.path))
        .filter(|m| !is_html_file(&m.path))
        .filter(|m| !is_barrel_with_reachable_sources(m, graph))
        .filter(|m| !has_reachable_importer(m.file_id, graph))
        .filter(|m| !has_reachable_export_reference(m.file_id, graph))
        .filter(|m| m.path.exists())
        .filter(|m| !suppressions.is_file_suppressed(m.file_id, IssueKind::UnusedFile))
        .map(|m| UnusedFile {
            path: m.path.clone(),
        })
        .collect()
}

/// Check if any reachable module has an edge to this file.
fn has_reachable_importer(file_id: FileId, graph: &ModuleGraph) -> bool {
    let idx = file_id.0 as usize;
    if idx >= graph.reverse_deps.len() {
        return false;
    }
    graph.reverse_deps[idx].iter().any(|&dep_id| {
        let dep_idx = dep_id.0 as usize;
        dep_idx < graph.modules.len() && graph.modules[dep_idx].is_reachable()
    })
}

/// Check if any export on this file is referenced by a reachable module.
fn has_reachable_export_reference(file_id: FileId, graph: &ModuleGraph) -> bool {
    graph.modules.get(file_id.0 as usize).is_some_and(|module| {
        module.exports.iter().any(|export| {
            export.references.iter().any(|reference| {
                graph
                    .modules
                    .get(reference.from_file.0 as usize)
                    .is_some_and(|m| m.is_reachable())
            })
        })
    })
}

#[cfg(test)]
#[expect(
    deprecated,
    reason = "Core-internal policy keeps direct detector unit tests while the public warning targets external callers"
)]
mod tests {
    use super::*;
    use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
    use crate::extract::{ExportName, VisibilityTag};
    use crate::graph::{ExportSymbol, ModuleGraph, ReferenceKind, SymbolReference};
    use crate::resolve::ResolvedModule;
    use crate::suppress::Suppression;
    use oxc_span::Span;
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::path::PathBuf;

    #[expect(
        clippy::cast_possible_truncation,
        reason = "test file counts are trivially small"
    )]
    fn build_graph(file_specs: &[(&str, bool)]) -> ModuleGraph {
        let files: Vec<DiscoveredFile> = file_specs
            .iter()
            .enumerate()
            .map(|(i, (path, _))| DiscoveredFile {
                id: FileId(i as u32),
                path: PathBuf::from(path),
                size_bytes: 0,
            })
            .collect();

        let entry_points: Vec<EntryPoint> = file_specs
            .iter()
            .filter(|(_, is_entry)| *is_entry)
            .map(|(path, _)| EntryPoint {
                path: PathBuf::from(path),
                source: EntryPointSource::ManualEntry,
            })
            .collect();

        let resolved_modules: Vec<ResolvedModule> = files
            .iter()
            .map(|f| ResolvedModule {
                file_id: f.id,
                path: f.path.clone(),
                exports: vec![],
                re_exports: vec![],
                resolved_imports: vec![],
                resolved_dynamic_imports: vec![],
                resolved_dynamic_patterns: vec![],
                member_accesses: vec![],
                semantic_facts: Box::default(),
                whole_object_uses: Box::default(),
                has_cjs_exports: false,
                has_angular_component_template_url: false,
                unused_import_bindings: FxHashSet::default(),
                type_referenced_import_bindings: vec![],
                value_referenced_import_bindings: vec![],
                namespace_object_aliases: vec![],
                exported_factory_returns: Box::default(),
                type_member_types: Box::default(),
            })
            .collect();

        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    }

    #[test]
    fn has_reachable_importer_out_of_bounds_file_id() {
        let graph = build_graph(&[("/src/entry.ts", true)]);
        assert!(!has_reachable_importer(FileId(999), &graph));
    }

    #[test]
    fn has_reachable_importer_empty_reverse_deps() {
        let graph = build_graph(&[("/src/entry.ts", true), ("/src/orphan.ts", false)]);
        assert!(!has_reachable_importer(FileId(1), &graph));
    }

    #[test]
    fn has_reachable_importer_with_unreachable_importer() {
        let graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        assert!(!has_reachable_importer(FileId(1), &graph));
    }

    #[test]
    fn has_reachable_export_reference_ignores_unreachable_references() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/helper.ts", false),
            ("/src/setup.ts", false),
        ]);

        graph.modules[1].exports = vec![ExportSymbol {
            name: ExportName::Named("helper".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: Span::new(0, 10),
            references: vec![SymbolReference {
                from_file: FileId(2),
                kind: ReferenceKind::NamedImport,
                import_span: Span::new(0, 10),
            }],
            members: vec![],
        }];

        assert!(
            !has_reachable_export_reference(FileId(1), &graph),
            "reference from unreachable module should not save file"
        );
    }

    #[test]
    fn has_reachable_export_reference_detects_reachable_references() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/helper.ts", false)]);

        graph.modules[1].exports = vec![ExportSymbol {
            name: ExportName::Named("helper".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: Span::new(0, 10),
            references: vec![SymbolReference {
                from_file: FileId(0),
                kind: ReferenceKind::NamedImport,
                import_span: Span::new(0, 10),
            }],
            members: vec![],
        }];

        assert!(
            has_reachable_export_reference(FileId(1), &graph),
            "reference from reachable module should keep file alive"
        );
    }

    #[test]
    fn find_unused_files_empty_graph() {
        let graph = build_graph(&[]);
        let result = find_unused_files(&graph, &SuppressionContext::empty());
        assert!(result.is_empty());
    }

    #[test]
    fn find_unused_files_entry_point_never_flagged() {
        let graph = build_graph(&[("/src/entry.ts", true)]);
        let result = find_unused_files(&graph, &SuppressionContext::empty());
        assert!(result.is_empty(), "entry point should never be flagged");
    }

    #[test]
    fn find_unused_files_skips_declaration_files() {
        let graph = build_graph(&[("/src/entry.ts", true), ("/src/types/global.d.ts", false)]);
        let result = find_unused_files(&graph, &SuppressionContext::empty());
        assert!(
            !result
                .iter()
                .any(|f| f.path.to_string_lossy().contains(".d.ts")),
            "declaration files should be skipped"
        );
    }

    #[test]
    fn find_unused_files_skips_config_files() {
        let graph = build_graph(&[("/src/entry.ts", true), ("/jest.config.ts", false)]);
        let result = find_unused_files(&graph, &SuppressionContext::empty());
        assert!(
            !result
                .iter()
                .any(|f| f.path.to_string_lossy().contains("jest.config")),
            "config files should be skipped"
        );
    }

    #[test]
    fn find_unused_files_skips_suppressed_files() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let orphan_path = dir.path().join("orphan.ts");
        std::fs::write(&orphan_path, "export const unused = 1;").expect("write temp file");

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: dir.path().join("entry.ts"),
                size_bytes: 0,
            },
            DiscoveredFile {
                id: FileId(1),
                path: orphan_path,
                size_bytes: 0,
            },
        ];
        let entry_points = vec![EntryPoint {
            path: dir.path().join("entry.ts"),
            source: EntryPointSource::ManualEntry,
        }];
        let resolved_modules: Vec<ResolvedModule> = files
            .iter()
            .map(|f| ResolvedModule {
                file_id: f.id,
                path: f.path.clone(),
                exports: vec![],
                re_exports: vec![],
                resolved_imports: vec![],
                resolved_dynamic_imports: vec![],
                resolved_dynamic_patterns: vec![],
                member_accesses: vec![],
                semantic_facts: Box::default(),
                whole_object_uses: Box::default(),
                has_cjs_exports: false,
                has_angular_component_template_url: false,
                unused_import_bindings: FxHashSet::default(),
                type_referenced_import_bindings: vec![],
                value_referenced_import_bindings: vec![],
                namespace_object_aliases: vec![],
                exported_factory_returns: Box::default(),
                type_member_types: Box::default(),
            })
            .collect();
        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

        let supps = vec![Suppression::issue(0, 1, IssueKind::UnusedFile)];
        let supps_slice: &[Suppression] = &supps;
        let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
        supp_map.insert(FileId(1), supps_slice);
        let suppressions = SuppressionContext::from_map(supp_map);

        let result = find_unused_files(&graph, &suppressions);
        assert!(result.is_empty(), "suppressed file should not be flagged");
    }

    #[test]
    fn find_unused_files_skips_nonexistent_files() {
        let graph = build_graph(&[("/src/entry.ts", true), ("/nonexistent/phantom.ts", false)]);
        let result = find_unused_files(&graph, &SuppressionContext::empty());
        assert!(
            !result
                .iter()
                .any(|f| f.path.to_string_lossy().contains("phantom")),
            "non-existent files should be filtered out"
        );
    }
}
