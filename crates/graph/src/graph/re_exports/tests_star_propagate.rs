use std::path::PathBuf;

use super::propagate::count_star_name_checks;
use crate::graph::{ExportNamespace, ExportSymbol, ModuleGraph};
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

fn re_export(target: u32, name: &str, is_type_only: bool) -> ResolvedReExport {
    ResolvedReExport {
        info: ReExportInfo {
            source: format!("./module{target}"),
            imported_name: name.to_string(),
            exported_name: name.to_string(),
            is_type_only,
            span: oxc_span::Span::default(),
            statement_span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        },
        target: ResolveResult::InternalModule(FileId(target)),
    }
}

fn named_import(target: u32, name: &str, is_type_only: bool) -> ResolvedImport {
    ResolvedImport {
        info: ImportInfo {
            source: format!("./module{target}"),
            imported_name: ImportedName::Named(name.to_string()),
            local_name: name.to_string(),
            is_type_only,
            is_type_only_star: false,
            from_style: false,
            span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        },
        target: ResolveResult::InternalModule(FileId(target)),
    }
}

fn named_export(name: &str, is_type_only: bool) -> ExportInfo {
    ExportInfo {
        name: ExportName::Named(name.to_string()),
        local_name: Some(name.to_string()),
        is_type_only,
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

fn build(modules: &[ResolvedModule], entry: usize) -> ModuleGraph {
    let files: Vec<_> = modules
        .iter()
        .map(|module| DiscoveredFile {
            id: module.file_id,
            path: module.path.clone(),
            size_bytes: 100,
        })
        .collect();
    let entry_points = vec![EntryPoint {
        path: files[entry].path.clone(),
        source: EntryPointSource::PackageJsonMain,
    }];
    ModuleGraph::build(modules, &entry_points, &files)
}

fn reference_namespaces(export: &ExportSymbol) -> Vec<ExportNamespace> {
    let mut namespaces: Vec<_> = export
        .references
        .iter()
        .map(|reference| reference.namespace)
        .collect();
    namespaces.sort_unstable_by_key(|namespace| match namespace {
        ExportNamespace::Type => 0,
        ExportNamespace::Value => 1,
    });
    namespaces
}

fn export_states(
    graph: &ModuleGraph,
    module: usize,
    name: &str,
) -> Vec<(bool, Vec<ExportNamespace>)> {
    graph.modules[module]
        .exports
        .iter()
        .filter(|export| export.name.matches_str(name))
        .map(|export| (export.is_type_only, reference_namespaces(export)))
        .collect()
}

/// A single star checks each imported source name exactly once.
#[test]
fn star_name_checks_count_each_imported_name() {
    let mut consumer = module(0, "a_consumer");
    consumer.resolved_imports = vec![
        named_import(1, "first", false),
        named_import(1, "second", false),
    ];
    let mut barrel = module(1, "b_barrel");
    barrel.re_exports = vec![re_export(2, "*", false)];
    let mut source = module(2, "c_source");
    source.exports = vec![
        named_export("first", false),
        named_export("second", false),
        named_export("unused", false),
    ]
    .into();

    let (graph, checks) = count_star_name_checks(|| build(&[consumer, barrel, source], 0));

    assert_eq!(checks, 2, "the star must check both imported names");
    assert_eq!(
        reference_namespaces(&graph.modules[2].exports[0]),
        vec![ExportNamespace::Value]
    );
    assert_eq!(
        reference_namespaces(&graph.modules[2].exports[1]),
        vec![ExportNamespace::Value]
    );
    assert!(graph.modules[2].exports[2].references.is_empty());
}

/// Star references revisit an earlier barrel and reach its leaf export.
#[test]
fn star_change_requeues_an_earlier_barrel() {
    let mut inner = module(0, "a_inner");
    inner.re_exports = vec![re_export(1, "*", false)];
    let mut leaf = module(1, "b_leaf");
    leaf.exports = vec![named_export("Item", false)].into();
    let mut outer = module(2, "c_outer");
    outer.re_exports = vec![re_export(0, "*", false)];
    let mut consumer = module(3, "d_consumer");
    consumer.resolved_imports = vec![named_import(2, "Item", false)];

    let graph = build(&[inner, leaf, outer, consumer], 3);

    assert_eq!(
        reference_namespaces(&graph.modules[1].exports[0]),
        vec![ExportNamespace::Value],
        "the earlier inner star must forward the reference added by the outer star"
    );
    let stub = graph.modules[0]
        .exports
        .iter()
        .find(|export| export.name.matches_str("Item"))
        .expect("the inner barrel must synthesize Item");
    assert!(!stub.is_type_only);
    assert_eq!(reference_namespaces(stub), vec![ExportNamespace::Value]);
}

/// A barrel with a local type synthesizes its forwarded value separately.
#[test]
fn star_synthesizes_value_beside_an_existing_type() {
    let mut consumer = module(0, "a_consumer");
    consumer.resolved_imports = vec![named_import(1, "Item", false)];
    let mut barrel = module(1, "b_barrel");
    barrel.re_exports = vec![re_export(2, "*", false)];
    let mut source = module(2, "c_source");
    source.exports = vec![named_export("Item", true)].into();
    source.re_exports = vec![re_export(3, "*", false)];
    let mut leaf = module(3, "d_leaf");
    leaf.exports = vec![named_export("Item", false)].into();

    let graph = build(&[consumer, barrel, source, leaf], 0);

    assert_eq!(
        export_states(&graph, 2, "Item"),
        vec![(true, vec![]), (false, vec![ExportNamespace::Value]),],
        "the forwarded value needs its own stub beside the unused local type"
    );
    assert_eq!(
        export_states(&graph, 3, "Item"),
        vec![(false, vec![ExportNamespace::Value])]
    );
}

/// A type source never gains a value stub for a sibling star's value.
#[test]
fn star_keeps_sibling_value_references_off_a_type_source() {
    for source_has_star in [false, true] {
        for triggering_is_type_only in [false, true] {
            let mut consumer = module(0, "a_consumer");
            let mut type_import = named_import(1, "Item", true);
            type_import.info.local_name = "ItemType".to_string();
            consumer.resolved_imports = vec![type_import, named_import(1, "Item", false)];
            let mut barrel = module(1, "b_barrel");
            barrel.re_exports = vec![
                re_export(2, "*", triggering_is_type_only),
                re_export(3, "*", false),
            ];
            let mut source = module(2, "c_source");
            source.exports = vec![named_export("Item", true)].into();
            if source_has_star {
                source.re_exports = vec![re_export(4, "*", false)];
            }
            let mut value_leaf = module(3, "d_value_leaf");
            value_leaf.exports = vec![named_export("Item", false)].into();
            let empty_leaf = module(4, "e_empty_leaf");

            let graph = build(&[consumer, barrel, source, value_leaf, empty_leaf], 0);

            assert_eq!(
                export_states(&graph, 2, "Item"),
                vec![(true, vec![ExportNamespace::Type])],
                "only the type resolves through this source: source_has_star={source_has_star}, triggering_is_type_only={triggering_is_type_only}"
            );
            assert_eq!(
                export_states(&graph, 3, "Item"),
                vec![(false, vec![ExportNamespace::Value])]
            );
        }
    }
}

/// A normal self-alias of a type receives a type stub through an outer star.
#[test]
fn star_synthesizes_type_for_a_self_re_exported_alias() {
    let mut consumer = module(0, "a_consumer");
    consumer.resolved_imports = vec![named_import(1, "Item", true)];
    let mut barrel = module(1, "b_barrel");
    barrel.re_exports = vec![re_export(2, "*", false)];
    let mut source = module(2, "c_source");
    source.exports = vec![named_export("Origin", true)].into();
    let mut alias = re_export(2, "Origin", false);
    alias.info.exported_name = "Item".to_string();
    source.re_exports = vec![alias, re_export(3, "*", false)];
    let empty_leaf = module(3, "d_empty_leaf");

    let graph = build(&[consumer, barrel, source, empty_leaf], 0);

    assert_eq!(
        export_states(&graph, 2, "Item"),
        vec![(false, vec![]), (true, vec![ExportNamespace::Type]),],
        "the value-shaped alias stub cannot stand in for its originating type"
    );
    assert_eq!(
        export_states(&graph, 2, "Origin"),
        vec![(true, vec![ExportNamespace::Type])]
    );
}

/// A shadowing barrel type cannot create a stub on an ambiguous type source.
#[test]
fn star_keeps_a_shadowed_type_off_an_ambiguous_source() {
    let mut consumer = module(0, "a_consumer");
    let mut type_import = named_import(2, "Item", true);
    type_import.info.local_name = "ItemType".to_string();
    consumer.resolved_imports = vec![named_import(1, "Item", false), type_import];
    let mut value_barrel = module(1, "b_value_barrel");
    value_barrel.re_exports = vec![re_export(3, "*", false)];
    let mut type_barrel = module(2, "c_type_barrel");
    type_barrel.exports = vec![named_export("Item", true)].into();
    type_barrel.re_exports = vec![re_export(3, "*", false)];
    let mut source = module(3, "d_source");
    source.re_exports = vec![
        re_export(4, "*", false),
        re_export(5, "*", false),
        re_export(6, "*", false),
    ];
    let mut first_type = module(4, "e_first_type");
    first_type.exports = vec![named_export("Item", true)].into();
    let mut second_type = module(5, "f_second_type");
    second_type.exports = vec![named_export("Item", true)].into();
    let mut value_leaf = module(6, "g_value_leaf");
    value_leaf.exports = vec![named_export("Item", false)].into();

    let graph = build(
        &[
            consumer,
            value_barrel,
            type_barrel,
            source,
            first_type,
            second_type,
            value_leaf,
        ],
        0,
    );

    assert_eq!(
        export_states(&graph, 3, "Item"),
        vec![(false, vec![ExportNamespace::Value])],
        "the earlier value star creates only a value stub; the later barrel's own type must not add a type stub"
    );
    assert_eq!(
        export_states(&graph, 2, "Item"),
        vec![(true, vec![ExportNamespace::Type])]
    );
    assert_eq!(export_states(&graph, 4, "Item"), vec![(true, vec![])]);
    assert_eq!(export_states(&graph, 5, "Item"), vec![(true, vec![])]);
    assert_eq!(
        export_states(&graph, 6, "Item"),
        vec![(false, vec![ExportNamespace::Value])]
    );
}
