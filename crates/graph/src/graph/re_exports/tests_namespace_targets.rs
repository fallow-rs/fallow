use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::graph::ModuleGraph;
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};
use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{
    ExportInfo, ExportName, ImportInfo, ImportedName, ReExportInfo, VisibilityTag,
};
use oxc_span::Span;

/// Far above the few milliseconds the cycle fixture needs in a debug build.
const CYCLE_BUILD_DEADLINE: Duration = Duration::from_secs(20);

fn module(id: u32, name: &str) -> ResolvedModule {
    ResolvedModule {
        file_id: FileId(id),
        path: PathBuf::from("/project").join(name),
        ..Default::default()
    }
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
            source: format!("./{}", target.path.file_name().unwrap().to_str().unwrap()),
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

fn ambient_imports(target: &ResolvedModule, namespace_object: bool) -> Vec<ResolvedImport> {
    let names = if namespace_object {
        vec![ImportedName::Namespace, ImportedName::Default]
    } else {
        vec![ImportedName::Namespace]
    };
    // Matching spans let the graph recognize the ambient namespace object.
    names
        .into_iter()
        .map(|imported_name| ResolvedImport {
            info: ImportInfo {
                source: format!("./{}", target.path.file_name().unwrap().to_str().unwrap()),
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

fn build_without_entry_points(modules: &[ResolvedModule]) -> ModuleGraph {
    let files: Vec<_> = modules
        .iter()
        .map(|module| DiscoveredFile {
            id: module.file_id,
            path: module.path.clone(),
            size_bytes: 100,
        })
        .collect();
    let graph = ModuleGraph::build(modules, &[], &files);
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
            "only a namespace object forwards the target's default namespace"
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
            "namespace exposure must include default regardless of re-export order"
        );
    }
}

#[test]
fn ambient_star_cycle_terminates_and_credits_named_exports() {
    let mut ambient = module(0, "ambient.ts");
    let mut first = module(1, "first.ts");
    let mut second = module(2, "second.ts");
    first.re_exports = vec![re_export(&second, "*")];
    second.re_exports = vec![re_export(&first, "*")];
    second.exports = vec![named_export("cycleValue")].into();
    ambient.resolved_imports = ambient_imports(&first, false);

    // A walk that revisits members without end would hang the whole suite, so
    // the build runs on its own thread and the test fails at the deadline.
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let graph = build_without_entry_points(&[ambient, first, second]);
        let _ = sender.send(export_is_used(&graph, FileId(2), "cycleValue"));
    });
    let cycle_value_used = receiver
        .recv_timeout(CYCLE_BUILD_DEADLINE)
        .expect("graph build over a star cycle must terminate");

    assert!(
        cycle_value_used,
        "ambient star exposure must reach exports across the cycle"
    );
}
