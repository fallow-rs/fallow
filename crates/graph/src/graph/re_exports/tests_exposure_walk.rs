use std::path::PathBuf;

use super::module_path_and_display;
use crate::graph::{ExportSymbol, ModuleGraph};
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use fallow_types::extract::{
    ExportInfo, ExportName, ImportInfo, ImportedName, ReExportInfo, VisibilityTag,
};

fn module(id: u32, name: &str) -> ResolvedModule {
    ResolvedModule {
        file_id: FileId(id),
        path: PathBuf::from(format!("/project/{name}.ts")),
        ..Default::default()
    }
}

fn export(name: &str) -> ExportInfo {
    ExportInfo {
        name: ExportName::Named(name.to_string()),
        local_name: Some(name.to_string()),
        is_type_only: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 20),
        members: vec![],
        is_side_effect_used: false,
        super_class: None,
        deprecated: false,
        deprecated_reason: None,
    }
}

fn re_export(target: u32, imported: &str, exported: &str) -> ResolvedReExport {
    ResolvedReExport {
        info: ReExportInfo {
            source: format!("./module{target}"),
            imported_name: imported.to_string(),
            exported_name: exported.to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
            statement_span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        },
        target: ResolveResult::InternalModule(FileId(target)),
    }
}

fn import(target: u32, imported_name: ImportedName) -> ResolvedImport {
    ResolvedImport {
        info: ImportInfo {
            source: format!("./module{target}"),
            imported_name,
            local_name: String::new(),
            is_type_only: true,
            is_type_only_star: false,
            from_style: false,
            span: oxc_span::Span::new(0, 10),
            source_span: oxc_span::Span::default(),
        },
        target: ResolveResult::InternalModule(FileId(target)),
    }
}

fn build(resolved_modules: &[ResolvedModule], entry_points: &[EntryPoint]) -> ModuleGraph {
    let files: Vec<_> = resolved_modules
        .iter()
        .map(|module| DiscoveredFile {
            id: module.file_id,
            path: module.path.clone(),
            size_bytes: 100,
        })
        .collect();
    ModuleGraph::build(resolved_modules, entry_points, &files)
}

fn symbol<'a>(graph: &'a ModuleGraph, id: usize, name: &str) -> &'a ExportSymbol {
    graph.modules[id]
        .exports
        .iter()
        .find(|export| export.name.matches_str(name))
        .unwrap_or_else(|| panic!("module {id} should export {name}"))
}

/// An ambient namespace exposes a default namespace and its transitive star exports.
#[test]
fn ambient_namespace_default_re_export_credits_deep_exports() {
    let mut ambient = module(0, "ambient");
    ambient.resolved_imports = vec![
        import(1, ImportedName::Namespace),
        import(1, ImportedName::Default),
    ];
    let mut seed = module(1, "seed");
    seed.re_exports = vec![re_export(2, "*", "default")];
    let mut surface = module(2, "surface");
    surface.re_exports = vec![re_export(3, "*", "*")];
    let mut deep = module(3, "deep");
    deep.exports = vec![export("used")].into();
    let resolved_modules = vec![ambient, seed, surface, deep];

    let graph = build(&resolved_modules, &[]);

    assert!(!graph.modules[2].is_reachable());
    assert!(
        !symbol(&graph, 3, "used").references.is_empty(),
        "the ambient default namespace should expose the deep star export"
    );
}

/// A star surface exposes named namespace re-exports and their transitive exports.
#[test]
fn ambient_star_chain_namespace_re_export_credits_deep_exports() {
    let mut ambient = module(0, "ambient");
    ambient.resolved_imports = vec![import(1, ImportedName::Namespace)];
    let mut seed = module(1, "seed");
    seed.re_exports = vec![re_export(2, "*", "*")];
    let mut member = module(2, "member");
    member.re_exports = vec![re_export(3, "*", "ns")];
    let mut surface = module(3, "surface");
    surface.re_exports = vec![re_export(4, "*", "*")];
    let mut deep = module(4, "deep");
    deep.exports = vec![export("used")].into();
    let resolved_modules = vec![ambient, seed, member, surface, deep];

    let graph = build(&resolved_modules, &[]);

    assert!(!graph.modules[3].is_reachable());
    assert!(
        !symbol(&graph, 4, "used").references.is_empty(),
        "the ambient star chain should expose the nested namespace's star export"
    );
}

/// A named re-export leaves unrelated exports on its source and star sources unused.
#[test]
fn named_re_export_keeps_unrelated_star_exports_unused() {
    let mut ambient = module(0, "ambient");
    ambient.resolved_imports = vec![import(1, ImportedName::Namespace)];
    let mut seed = module(1, "seed");
    seed.re_exports = vec![re_export(2, "a", "a")];
    let mut source = module(2, "source");
    source.exports = vec![export("a"), export("b")].into();
    source.re_exports = vec![re_export(3, "*", "*")];
    let mut deep = module(3, "deep");
    deep.exports = vec![export("unrelated")].into();
    let resolved_modules = vec![ambient, seed, source, deep];

    let graph = build(&resolved_modules, &[]);

    assert!(!symbol(&graph, 2, "a").references.is_empty());
    assert!(symbol(&graph, 2, "b").references.is_empty());
    assert!(
        symbol(&graph, 3, "unrelated").references.is_empty(),
        "a named re-export should not expose the source's unrelated star exports"
    );
}

/// An invalid re-export source leaves valid re-export references intact.
#[test]
fn out_of_range_re_export_source_preserves_valid_references() {
    let mut entry = module(0, "entry");
    entry.re_exports = vec![re_export(99, "*", "*"), re_export(1, "used", "used")];
    let entry_points = vec![EntryPoint {
        path: entry.path.clone(),
        source: EntryPointSource::PackageJsonMain,
    }];
    let mut source = module(1, "source");
    source.exports = vec![export("used"), export("unused")].into();
    let resolved_modules = vec![entry, source];

    let graph = build(&resolved_modules, &entry_points);

    assert!(!symbol(&graph, 1, "used").references.is_empty());
    assert!(symbol(&graph, 1, "unused").references.is_empty());
    assert!(graph.re_export_cycles.is_empty());
}

/// A file id just beyond the module list displays a placeholder path.
#[test]
fn module_path_at_module_count_uses_placeholder() {
    let resolved_modules = vec![module(0, "entry")];
    let graph = build(&resolved_modules, &[]);

    assert_eq!(
        module_path_and_display(FileId(0), &graph.modules),
        (
            PathBuf::from("/project/entry.ts"),
            "/project/entry.ts".to_string()
        )
    );
    assert_eq!(
        module_path_and_display(FileId(1), &graph.modules),
        (PathBuf::from("<file id 1>"), "<file id 1>".to_string())
    );
}
