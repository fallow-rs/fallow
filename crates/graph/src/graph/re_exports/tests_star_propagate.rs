use super::propagate::count_star_name_checks;
use super::test_support::{
    build, discovered_files, export, module, named_import, re_export, symbol, type_export,
    type_import,
};
use super::{capture_propagation_visits, with_re_export_differential_check};
use crate::graph::types::ReferencePathNode;
use crate::graph::{ExportNamespace, ExportSymbol, ModuleGraph, ReachabilityRoots, ReferenceKind};
use crate::resolve::ResolvedReplacedModuleTarget;
use fallow_types::discover::{EntryPoint, EntryPointSource, FileId};
use fallow_types::extract::ModuleLoadMechanism;
use rustc_hash::FxHashSet;

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
    let mut states: Vec<_> = graph.modules[module]
        .exports
        .iter()
        .filter(|export| export.name.matches_str(name))
        .map(|export| (export.is_type_only, reference_namespaces(export)))
        .collect();
    states.sort_unstable_by_key(|(is_type_only, _)| !*is_type_only);
    states
}

/// A single star checks each imported source name exactly once.
#[test]
fn star_name_checks_count_each_imported_name() {
    let mut consumer = module(0, "a_consumer");
    consumer.resolved_imports = vec![named_import(1, "first"), named_import(1, "second")];
    let mut barrel = module(1, "b_barrel");
    barrel.re_exports = vec![re_export(2, "*", "*")];
    let mut source = module(2, "c_source");
    source.exports = vec![export("first"), export("second"), export("unused")].into();

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
    inner.re_exports = vec![re_export(1, "*", "*")];
    let mut leaf = module(1, "b_leaf");
    leaf.exports = vec![export("Item")].into();
    let mut outer = module(2, "c_outer");
    outer.re_exports = vec![re_export(0, "*", "*")];
    let mut consumer = module(3, "d_consumer");
    consumer.resolved_imports = vec![named_import(2, "Item")];

    let (graph, visits) = capture_propagation_visits(|| build(&[inner, leaf, outer, consumer], 3));

    let inner_edge = (FileId(0), FileId(1));
    let outer_edge = (FileId(2), FileId(0));
    let first_inner = visits.iter().position(|visit| *visit == inner_edge);
    let first_outer = visits.iter().position(|visit| *visit == outer_edge);
    assert!(
        first_inner < first_outer,
        "the inner star must run before the outer star, so only a requeue can forward the reference: {visits:?}"
    );
    assert_eq!(
        visits.iter().filter(|visit| **visit == inner_edge).count(),
        2,
        "the outer star change must requeue the inner star once: {visits:?}"
    );
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
    consumer.resolved_imports = vec![named_import(1, "Item")];
    let mut barrel = module(1, "b_barrel");
    barrel.re_exports = vec![re_export(2, "*", "*")];
    let mut source = module(2, "c_source");
    source.exports = vec![type_export("Item")].into();
    source.re_exports = vec![re_export(3, "*", "*")];
    let mut leaf = module(3, "d_leaf");
    leaf.exports = vec![export("Item")].into();

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
            let mut aliased_import = type_import(1, "Item");
            aliased_import.info.local_name = "ItemType".to_string();
            consumer.resolved_imports = vec![aliased_import, named_import(1, "Item")];
            let mut barrel = module(1, "b_barrel");
            let mut triggering_star = re_export(2, "*", "*");
            triggering_star.info.is_type_only = triggering_is_type_only;
            barrel.re_exports = vec![triggering_star, re_export(3, "*", "*")];
            let mut source = module(2, "c_source");
            source.exports = vec![type_export("Item")].into();
            if source_has_star {
                source.re_exports = vec![re_export(4, "*", "*")];
            }
            let mut value_leaf = module(3, "d_value_leaf");
            value_leaf.exports = vec![export("Item")].into();
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
    consumer.resolved_imports = vec![type_import(1, "Item")];
    let mut barrel = module(1, "b_barrel");
    barrel.re_exports = vec![re_export(2, "*", "*")];
    let mut source = module(2, "c_source");
    source.exports = vec![type_export("Origin")].into();
    source.re_exports = vec![re_export(2, "Origin", "Item"), re_export(3, "*", "*")];
    let empty_leaf = module(3, "d_empty_leaf");

    let graph = build(&[consumer, barrel, source, empty_leaf], 0);

    assert_eq!(
        export_states(&graph, 2, "Item"),
        vec![(true, vec![ExportNamespace::Type]), (false, vec![])],
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
    let mut aliased_import = type_import(2, "Item");
    aliased_import.info.local_name = "ItemType".to_string();
    consumer.resolved_imports = vec![named_import(1, "Item"), aliased_import];
    let mut value_barrel = module(1, "b_value_barrel");
    value_barrel.re_exports = vec![re_export(3, "*", "*")];
    let mut type_barrel = module(2, "c_type_barrel");
    type_barrel.exports = vec![type_export("Item")].into();
    type_barrel.re_exports = vec![re_export(3, "*", "*")];
    let mut source = module(3, "d_source");
    source.re_exports = vec![
        re_export(4, "*", "*"),
        re_export(5, "*", "*"),
        re_export(6, "*", "*"),
    ];
    let mut first_type = module(4, "e_first_type");
    first_type.exports = vec![type_export("Item")].into();
    let mut second_type = module(5, "f_second_type");
    second_type.exports = vec![type_export("Item")].into();
    let mut value_leaf = module(6, "g_value_leaf");
    value_leaf.exports = vec![export("Item")].into();

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

#[test]
fn entry_named_type_re_export_credits_declaration_through_star_barrel() {
    let mut entry = module(0, "index");
    let mut entry_re = re_export(1, "ViaStar", "ViaStar");
    entry_re.info.is_type_only = true;
    entry.re_exports = vec![entry_re];

    let mut kit = module(1, "kit_index");
    kit.re_exports = vec![re_export(2, "*", "*")];

    let mut types = module(2, "kit_types");
    types.exports = vec![type_export("ViaStar"), type_export("UnusedSibling")].into();

    let graph = with_re_export_differential_check(|| build(&[entry, kit, types], 0));

    let via_star = symbol(&graph, 2, "ViaStar");
    assert_eq!(via_star.references.len(), 1);
    assert_eq!(via_star.references[0].from_file, FileId(0));
    assert_eq!(via_star.references[0].kind, ReferenceKind::ReExport);
    assert_eq!(via_star.references[0].namespace, ExportNamespace::Type);
    assert!(symbol(&graph, 2, "UnusedSibling").references.is_empty());

    let surface_refs =
        graph.effective_export_surface_references(FileId(1), "ViaStar", ExportNamespace::Type);
    assert_eq!(surface_refs.len(), 1);
    assert_eq!(surface_refs[0].from_file, FileId(0));
}

#[test]
fn entry_named_re_export_credits_through_multi_hop_and_cyclic_star_barrels() {
    let mut entry = module(0, "index");
    let mut type_re = re_export(1, "ViaStar", "PublicStar");
    type_re.info.is_type_only = true;
    let value_re = re_export(1, "runtimeFn", "runtimeFn");
    entry.re_exports = vec![type_re, value_re];

    let mut star_a = module(1, "star_a");
    star_a.re_exports = vec![re_export(2, "*", "*")];

    let mut star_b = module(2, "star_b");
    star_b.re_exports = vec![re_export(1, "*", "*"), re_export(3, "*", "*")];

    let mut leaf = module(3, "leaf");
    leaf.exports = vec![
        type_export("ViaStar"),
        export("runtimeFn"),
        export("unexported"),
    ]
    .into();

    let graph = with_re_export_differential_check(|| build(&[entry, star_a, star_b, leaf], 0));

    assert_eq!(
        reference_namespaces(symbol(&graph, 3, "ViaStar")),
        vec![ExportNamespace::Type]
    );
    assert_eq!(
        reference_namespaces(symbol(&graph, 3, "runtimeFn")),
        vec![ExportNamespace::Type, ExportNamespace::Value]
    );
    assert!(
        symbol(&graph, 3, "runtimeFn")
            .references
            .iter()
            .any(|r| r.from_file == FileId(0)
                && r.kind == ReferenceKind::ReExport
                && r.namespace == ExportNamespace::Value)
    );
    assert!(symbol(&graph, 3, "unexported").references.is_empty());
}

#[test]
fn entry_named_re_export_through_type_only_star_credits_only_type_namespace() {
    let mut entry = module(0, "index");
    entry.re_exports = vec![re_export(1, "Merged", "Merged")];

    let mut star = module(1, "star");
    let mut type_star = re_export(2, "*", "*");
    type_star.info.is_type_only = true;
    star.re_exports = vec![type_star];

    let mut leaf = module(2, "leaf");
    leaf.exports = vec![type_export("Merged"), export("Merged")].into();

    let graph = with_re_export_differential_check(|| build(&[entry, star, leaf], 0));

    assert_eq!(
        export_states(&graph, 2, "Merged"),
        vec![(true, vec![ExportNamespace::Type]), (false, vec![])]
    );
}

#[test]
fn entry_type_only_named_re_export_through_star_credits_value_backed_type_and_isolates_value() {
    let mut entry = module(0, "index");
    let mut merged_re = re_export(1, "Merged", "Merged");
    merged_re.info.is_type_only = true;
    let mut class_re = re_export(1, "Widget", "Widget");
    class_re.info.is_type_only = true;
    entry.re_exports = vec![merged_re, class_re];

    let mut star = module(1, "star");
    star.re_exports = vec![re_export(2, "*", "*")];

    let mut leaf = module(2, "leaf");
    leaf.exports = vec![type_export("Merged"), export("Merged"), export("Widget")].into();

    let graph = with_re_export_differential_check(|| build(&[entry, star, leaf], 0));

    assert_eq!(
        export_states(&graph, 2, "Merged"),
        vec![(true, vec![ExportNamespace::Type]), (false, vec![])]
    );
    assert_eq!(
        export_states(&graph, 2, "Widget"),
        vec![(false, vec![ExportNamespace::Type])]
    );
}

#[test]
fn entry_named_re_export_mixed_consumer_credits_unconsumed_namespace_through_star() {
    let mut entry = module(0, "index");
    entry.re_exports = vec![re_export(1, "Merged", "Merged")];
    entry.resolved_imports = vec![named_import(3, "keep")];

    let mut star = module(1, "star");
    star.re_exports = vec![re_export(2, "*", "*")];

    let mut leaf = module(2, "leaf");
    leaf.exports = vec![type_export("Merged"), export("Merged")].into();

    let mut internal = module(3, "internal");
    internal.exports = vec![export("keep")].into();
    internal.resolved_imports = vec![type_import(0, "Merged")];
    internal.type_referenced_import_bindings = vec!["Merged".to_string()];

    let graph = with_re_export_differential_check(|| build(&[entry, star, leaf, internal], 0));

    assert_eq!(
        export_states(&graph, 2, "Merged"),
        vec![
            (true, vec![ExportNamespace::Type]),
            (false, vec![ExportNamespace::Value]),
        ]
    );
}

#[test]
fn entry_named_re_export_mixed_consumer_credits_unconsumed_namespace_from_direct_source() {
    let mut entry = module(0, "index");
    entry.re_exports = vec![re_export(1, "Merged", "Merged")];
    entry.resolved_imports = vec![named_import(2, "keep")];

    let mut leaf = module(1, "leaf");
    leaf.exports = vec![type_export("Merged"), export("Merged")].into();

    let mut internal = module(2, "internal");
    internal.exports = vec![export("keep")].into();
    internal.resolved_imports = vec![type_import(0, "Merged")];
    internal.type_referenced_import_bindings = vec!["Merged".to_string()];

    let graph = with_re_export_differential_check(|| build(&[entry, leaf, internal], 0));

    // The entry re-export is public API, so the value side is credited even
    // though the only in-graph consumer reads the type side.
    assert_eq!(
        export_states(&graph, 1, "Merged"),
        vec![
            (true, vec![ExportNamespace::Type]),
            (false, vec![ExportNamespace::Value]),
        ]
    );
}

#[test]
fn non_entry_named_re_export_through_star_without_consumer_stays_uncredited() {
    let mut entry = module(0, "index");
    entry.resolved_imports = vec![named_import(1, "alive")];

    let mut mid_barrel = module(1, "mid_barrel");
    mid_barrel.exports = vec![export("alive")].into();
    let mut dead_re = re_export(2, "DeadViaStar", "DeadViaStar");
    dead_re.info.is_type_only = true;
    mid_barrel.re_exports = vec![dead_re];

    let mut star = module(2, "star");
    star.re_exports = vec![re_export(3, "*", "*")];

    let mut leaf = module(3, "leaf");
    leaf.exports = vec![type_export("DeadViaStar")].into();

    let graph = with_re_export_differential_check(|| build(&[entry, mid_barrel, star, leaf], 0));

    assert!(symbol(&graph, 3, "DeadViaStar").references.is_empty());
}

#[test]
fn entry_named_re_export_through_star_records_route_provenance_without_duplicate_hops() {
    let mut entry = module(0, "index");
    let mut entry_re = re_export(1, "ViaStar", "ViaStar");
    entry_re.info.is_type_only = true;
    entry.re_exports = vec![entry_re];

    let mut kit = module(1, "kit_index");
    kit.re_exports = vec![re_export(2, "*", "*")];

    let mut types = module(2, "kit_types");
    types.exports = vec![type_export("ViaStar")].into();

    let test_root = module(3, "entry_test");
    let modules = [entry, kit, types, test_root];
    let files = discovered_files(&modules);
    let all_entries = [
        EntryPoint {
            path: modules[0].path.clone(),
            source: EntryPointSource::PackageJsonMain,
        },
        EntryPoint {
            path: modules[3].path.clone(),
            source: EntryPointSource::TestFile,
        },
    ];
    let runtime_entries = [all_entries[0].clone()];
    let test_entries = [all_entries[1].clone()];
    let replacements = [ResolvedReplacedModuleTarget {
        source_file: FileId(3),
        target_file: FileId(1),
    }];

    let graph = with_re_export_differential_check(|| {
        ModuleGraph::build_with_declaration_seeding(
            &modules,
            &replacements,
            &ReachabilityRoots {
                all: &all_entries,
                runtime: &runtime_entries,
                test: &test_entries,
            },
            &files,
            &FxHashSet::default(),
        )
    });

    let via_star = symbol(&graph, 2, "ViaStar");
    assert_eq!(via_star.references.len(), 1);
    let path_id = via_star
        .reference_path(0)
        .expect("provenance-enabled build must record a route path");
    let ReferencePathNode::Route {
        parent,
        graph: route_graph,
        start,
        terminal,
        start_mechanism,
    } = graph.reference_paths[path_id.index()]
    else {
        panic!("entry star-route reference must use compact Route provenance");
    };
    assert!(
        parent.is_none(),
        "entry is the reference origin, so Route must not carry a duplicate parent hop"
    );
    assert_eq!(
        graph
            .reference_routes
            .canonical_hops(route_graph, start, terminal, start_mechanism),
        vec![
            (FileId(2), ModuleLoadMechanism::EsModule),
            (FileId(1), ModuleLoadMechanism::EsModule),
        ]
    );
}
