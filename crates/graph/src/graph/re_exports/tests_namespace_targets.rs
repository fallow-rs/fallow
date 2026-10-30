use std::path::PathBuf;

use rustc_hash::FxHashMap;

use super::{ExposedNamespaceTargets, Exposure};
use crate::graph::ModuleGraph;
use crate::resolve::{
    ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport, ResolvedReplacedModuleTarget,
};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use fallow_types::extract::{
    ExportInfo, ExportName, ImportInfo, ImportedName, ReExportInfo, VisibilityTag,
};
use oxc_span::Span;

fn module(id: u32, name: &str) -> ResolvedModule {
    ResolvedModule {
        file_id: FileId(id),
        path: PathBuf::from("/project").join(name),
        ..Default::default()
    }
}

fn relative_source(target: &ResolvedModule) -> String {
    let file_name = target
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("fixture paths have a UTF-8 file name");
    format!("./{file_name}")
}

fn named_export(name: &str) -> ExportInfo {
    ExportInfo {
        name: ExportName::Named(name.to_string()),
        local_name: Some(name.to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        deprecated: false,
        deprecated_reason: None,
        span: Span::default(),
        members: vec![],
        super_class: None,
    }
}

fn re_export(target: &ResolvedModule, exported_name: &str) -> ResolvedReExport {
    ResolvedReExport {
        info: ReExportInfo {
            source: relative_source(target),
            imported_name: "*".to_string(),
            exported_name: exported_name.to_string(),
            is_type_only: false,
            span: Span::default(),
            statement_span: Span::default(),
            source_span: Span::default(),
        },
        target: ResolveResult::InternalModule(target.file_id),
    }
}

/// The imports extraction records for `export *` inside a
/// `declare module '...'` body. A type-only import with an empty local name
/// marks the ambient form. The `export * as ns` spelling adds a `Default`
/// import at the same span, which the graph reads as an observation of the
/// whole namespace object.
fn ambient_imports(target: &ResolvedModule, namespace_object: bool) -> Vec<ResolvedImport> {
    let names = if namespace_object {
        vec![ImportedName::Namespace, ImportedName::Default]
    } else {
        vec![ImportedName::Namespace]
    };
    names
        .into_iter()
        .map(|imported_name| ResolvedImport {
            info: ImportInfo {
                source: relative_source(target),
                imported_name,
                local_name: String::new(),
                is_type_only: true,
                is_type_only_star: false,
                from_style: false,
                span: Span::new(25, 65),
                source_span: Span::new(50, 64),
            },
            target: ResolveResult::InternalModule(target.file_id),
        })
        .collect()
}

fn discovered_files(modules: &[ResolvedModule]) -> Vec<DiscoveredFile> {
    modules
        .iter()
        .map(|module| DiscoveredFile {
            id: module.file_id,
            path: module.path.clone(),
            size_bytes: 100,
        })
        .collect()
}

fn build_without_entry_points(modules: &[ResolvedModule]) -> ModuleGraph {
    let graph = ModuleGraph::build(modules, &[], &discovered_files(modules));
    assert!(graph.modules.iter().all(|module| !module.is_entry_point()));
    graph
}

fn export_is_used(graph: &ModuleGraph, file: FileId, name: &str) -> bool {
    let export = graph.modules[file.0 as usize]
        .exports
        .iter()
        .find(|export| export.name.to_string() == name)
        .expect("fixture export must exist");
    !export.references.is_empty()
}

#[test]
fn ambient_namespace_forwards_default_namespace_exports_without_entry_points() {
    for (observation, named_used, default_namespace_used) in [
        (None, false, false),
        (Some(false), true, false),
        (Some(true), true, true),
    ] {
        let mut ambient = module(0, "ambient.ts");
        let mut leaf = module(1, "leaf.ts");
        leaf.exports = vec![named_export("leafValue")].into();
        let mut target = module(2, "target.ts");
        target.exports = vec![named_export("namedValue")].into();
        target.re_exports = vec![re_export(&leaf, "default")];
        if let Some(namespace_object) = observation {
            ambient.resolved_imports = ambient_imports(&target, namespace_object);
        }

        let graph = build_without_entry_points(&[ambient, leaf, target]);

        assert_eq!(
            export_is_used(&graph, FileId(2), "namedValue"),
            named_used,
            "ambient observation {observation:?} must preserve the named surface"
        );
        assert_eq!(
            export_is_used(&graph, FileId(1), "leafValue"),
            default_namespace_used,
            "ambient observation {observation:?}: only a namespace object forwards \
             the target's default namespace"
        );
    }
}

#[test]
fn namespace_exposure_includes_default_after_reaching_the_same_module_through_a_star() {
    for namespace_first in [false, true] {
        let mut ambient = module(0, "ambient.ts");
        let mut leaf = module(1, "leaf.ts");
        leaf.exports = vec![named_export("leafValue")].into();
        let mut member = module(2, "member.ts");
        member.re_exports = vec![re_export(&leaf, "default")];
        let mut seed = module(3, "seed.ts");
        let star = re_export(&member, "*");
        let namespace = re_export(&member, "ns");
        seed.re_exports = if namespace_first {
            vec![namespace, star]
        } else {
            vec![star, namespace]
        };
        ambient.resolved_imports = ambient_imports(&seed, false);

        let graph = build_without_entry_points(&[ambient, leaf, member, seed]);

        assert!(
            export_is_used(&graph, FileId(1), "leafValue"),
            "namespace_first = {namespace_first}: namespace exposure must include default"
        );
    }
}

/// With nothing observed and no entry point, a namespace re-export has no
/// consumer to credit, so Phase 2c must not record a reference route for it.
/// Route provenance is only tracked when a test root replaces a module.
#[test]
fn unobserved_namespace_re_export_records_no_reference_route() {
    let test_root = module(0, "root.test.ts");
    let mut source = module(1, "source.ts");
    source.exports = vec![named_export("sourceValue")].into();
    let mut barrel = module(2, "barrel.ts");
    barrel.re_exports = vec![re_export(&source, "ns")];
    let replaced = module(3, "replaced.ts");
    let modules = [test_root, source, barrel, replaced];
    let files = discovered_files(&modules);
    let test_roots = [EntryPoint {
        path: modules[0].path.clone(),
        source: EntryPointSource::TestFile,
    }];

    let graph = ModuleGraph::build_with_reachability_roots_and_replacements(
        &modules,
        &[ResolvedReplacedModuleTarget {
            source_file: FileId(0),
            target_file: FileId(3),
        }],
        &[],
        &[],
        &test_roots,
        &files,
    );

    assert!(graph.modules.iter().all(|module| !module.is_entry_point()));
    assert!(!export_is_used(&graph, FileId(1), "sourceValue"));
    assert!(
        graph.reference_routes.graphs.is_empty(),
        "no consumer can use a route for the unobserved namespace re-export"
    );
}

/// Drain `stack` like the exposure walk, where every member re-exports the
/// next member of a cycle at `exposure`. Returns the number of pops, or
/// `None` when the walk is still running after `budget` pops.
fn walk_cycle(
    closure: &mut ExposedNamespaceTargets,
    stack: &mut Vec<(FileId, Exposure)>,
    cycle: &[FileId],
    exposure: Exposure,
    budget: usize,
) -> Option<usize> {
    let mut pops = 0;
    while let Some((file_id, _)) = stack.pop() {
        pops += 1;
        if pops > budget {
            return None;
        }
        let position = cycle.iter().position(|&member| member == file_id)?;
        let next = cycle[(position + 1) % cycle.len()];
        closure.record(stack, next, exposure);
    }
    Some(pops)
}

#[test]
fn closure_walk_visits_each_cycle_member_once_per_exposure() {
    const BUDGET: usize = 16;
    let cycle = [FileId(0), FileId(1)];
    for exposure in [Exposure::StarSurface, Exposure::NamespaceObject] {
        let mut closure = ExposedNamespaceTargets {
            members: FxHashMap::default(),
        };
        let mut stack = Vec::new();
        closure.record(&mut stack, cycle[0], exposure);

        let pops = walk_cycle(&mut closure, &mut stack, &cycle, exposure, BUDGET);

        assert_eq!(
            pops,
            Some(cycle.len()),
            "exposure {exposure:?}: a revisit at the same exposure must not walk again"
        );
    }
}

#[test]
fn closure_record_rewalks_a_member_only_when_its_exposure_widens() {
    let member = FileId(0);
    for (first, second, rewalk) in [
        (Exposure::StarSurface, Exposure::NamespaceObject, true),
        (Exposure::NamespaceObject, Exposure::StarSurface, false),
    ] {
        let mut closure = ExposedNamespaceTargets {
            members: FxHashMap::default(),
        };
        let mut stack = Vec::new();
        closure.record(&mut stack, member, first);
        stack.clear();

        closure.record(&mut stack, member, second);

        let expected: Vec<_> = if rewalk {
            vec![(member, Exposure::NamespaceObject)]
        } else {
            Vec::new()
        };
        assert_eq!(stack, expected, "{first:?} then {second:?}");
        assert!(
            closure.exposes_name(member, "default"),
            "{first:?} then {second:?}: the wider exposure must stay recorded"
        );
    }
}
