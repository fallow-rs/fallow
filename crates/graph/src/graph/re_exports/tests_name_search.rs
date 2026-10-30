use std::path::PathBuf;

use crate::graph::ModuleGraph;
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
        name: if name == "default" {
            ExportName::Default
        } else {
            ExportName::Named(name.to_string())
        },
        local_name: Some(name.to_string()),
        is_type_only: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::default(),
        members: vec![],
        is_side_effect_used: false,
        super_class: None,
        deprecated: false,
        deprecated_reason: None,
    }
}

fn re_export(target: &ResolvedModule, imported: &str, exported: &str) -> ResolvedReExport {
    ResolvedReExport {
        info: ReExportInfo {
            source: target.path.to_string_lossy().into_owned(),
            imported_name: imported.to_string(),
            exported_name: exported.to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
            statement_span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        },
        target: ResolveResult::InternalModule(target.file_id),
    }
}

fn import(target: &ResolvedModule, imported: ImportedName, local: &str) -> ResolvedImport {
    ResolvedImport {
        info: ImportInfo {
            source: target.path.to_string_lossy().into_owned(),
            imported_name: imported,
            local_name: local.to_string(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        },
        target: ResolveResult::InternalModule(target.file_id),
    }
}

fn build(modules: &[ResolvedModule], entry: FileId) -> ModuleGraph {
    let files: Vec<_> = modules
        .iter()
        .map(|module| DiscoveredFile {
            id: module.file_id,
            path: module.path.clone(),
            size_bytes: 100,
        })
        .collect();
    let entry_points = vec![EntryPoint {
        path: modules[entry.0 as usize].path.clone(),
        source: EntryPointSource::PackageJsonMain,
    }];
    ModuleGraph::build(modules, &entry_points, &files)
}

fn assert_credit(graph: &ModuleGraph, file: FileId, name: &str, expected: bool) {
    let module = &graph.modules[file.0 as usize];
    let export = module
        .exports
        .iter()
        .find(|export| export.name.matches_str(name))
        .expect("the target must export the name");
    assert_eq!(
        !export.references.is_empty(),
        expected,
        "{}:{name} has references {:?}",
        module.path.display(),
        export.references
    );
}

fn whole_object_graph(imported: ImportedName, exported: &str) -> ModuleGraph {
    let mut consumer = module(0, "consumer");
    let mut barrel = module(1, "barrel");
    let mut target = module(2, "target");
    let mut leaf = module(3, "leaf");
    leaf.exports = vec![export("value"), export("default")].into();
    target.exports = vec![export("direct")].into();
    target.re_exports = vec![re_export(&leaf, "*", "*")];
    barrel.re_exports = vec![re_export(&target, "*", exported)];
    consumer.resolved_imports = vec![import(&barrel, imported, "api")];
    consumer.whole_object_uses = vec!["api".to_string()].into();
    build(&[consumer, barrel, target, leaf], FileId(0))
}

#[test]
fn default_import_whole_object_use_credits_star_source() {
    let graph = whole_object_graph(ImportedName::Default, "default");

    assert_credit(&graph, FileId(2), "direct", true);
    // The target credits its direct export without the whole-object use, so
    // only the leaf behind its star shows that the namespace object is exposed.
    assert_credit(&graph, FileId(3), "value", true);
    assert_credit(&graph, FileId(3), "default", false);
}

#[test]
fn named_import_whole_object_use_credits_star_source() {
    let graph = whole_object_graph(ImportedName::Named("ns".to_string()), "ns");

    assert_credit(&graph, FileId(2), "direct", true);
    assert_credit(&graph, FileId(3), "value", true);
    assert_credit(&graph, FileId(3), "default", false);
}

fn two_names_graph(whole_object_uses: &[&str]) -> ModuleGraph {
    let mut consumer = module(0, "consumer");
    let mut barrel = module(1, "barrel");
    let mut a = module(2, "a");
    let mut a_leaf = module(3, "a-leaf");
    let mut b = module(4, "b");
    let mut b_leaf = module(5, "b-leaf");
    a_leaf.exports = vec![export("aValue")].into();
    b_leaf.exports = vec![export("bValue")].into();
    a.re_exports = vec![re_export(&a_leaf, "*", "*")];
    b.re_exports = vec![re_export(&b_leaf, "*", "*")];
    barrel.re_exports = vec![re_export(&a, "*", "a"), re_export(&b, "*", "b")];
    consumer.resolved_imports = vec![
        import(&barrel, ImportedName::Named("a".to_string()), "aApi"),
        import(&barrel, ImportedName::Named("b".to_string()), "bApi"),
    ];
    consumer.whole_object_uses = whole_object_uses
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    build(&[consumer, barrel, a, a_leaf, b, b_leaf], FileId(0))
}

#[test]
fn whole_object_use_does_not_credit_another_import() {
    let graph = two_names_graph(&["bApi"]);

    assert_credit(&graph, FileId(3), "aValue", false);
    assert_credit(&graph, FileId(5), "bValue", true);
}

#[test]
fn distinct_whole_object_imports_credit_both_star_sources() {
    let graph = two_names_graph(&["aApi", "bApi"]);

    assert_credit(&graph, FileId(3), "aValue", true);
    assert_credit(&graph, FileId(5), "bValue", true);
}

#[test]
fn entry_namespace_exposure_reaches_a_later_named_hop() {
    let mut entry = module(0, "entry");
    let mut m1 = module(1, "m1");
    let mut x = module(2, "x");
    let mut t2 = module(3, "t2");
    let mut leaf = module(4, "leaf");
    leaf.exports = vec![export("value"), export("default")].into();
    t2.exports = vec![export("direct")].into();
    t2.re_exports = vec![re_export(&leaf, "*", "*")];
    x.re_exports = vec![re_export(&t2, "*", "ns2")];
    m1.re_exports = vec![re_export(&x, "ns2", "ns2")];
    entry.re_exports = vec![re_export(&m1, "*", "ns1")];

    let graph = build(&[entry, m1, x, t2, leaf], FileId(0));

    assert_credit(&graph, FileId(3), "direct", true);
    // The named hop exposes t2 only after m1 joins the namespace closure.
    assert_credit(&graph, FileId(4), "value", true);
    assert_credit(&graph, FileId(4), "default", false);
}

fn named_hop_graph(shadowed: bool) -> ModuleGraph {
    let mut entry = module(0, "entry");
    let mut barrel = module(1, "barrel");
    let mut target = module(2, "target");
    let mut leaf = module(3, "leaf");
    leaf.exports = vec![export("value")].into();
    target.exports = vec![export("direct")].into();
    target.re_exports = vec![re_export(&leaf, "*", "*")];
    barrel.re_exports = vec![re_export(&target, "*", "ns")];
    entry.re_exports = vec![re_export(&barrel, "ns", "api")];
    if shadowed {
        entry.exports = vec![export("api")].into();
    }
    build(&[entry, barrel, target, leaf], FileId(0))
}

#[test]
fn local_export_blocks_a_named_namespace_hop() {
    let exposed = named_hop_graph(false);
    assert_credit(&exposed, FileId(2), "direct", true);
    assert_credit(&exposed, FileId(3), "value", true);

    // Keep these exact unused assertions. A hop that skips the forwarding,
    // `failed` or `visited` check shows only as a wrong exposure here.
    let shadowed = named_hop_graph(true);
    assert_credit(&shadowed, FileId(2), "direct", false);
    assert_credit(&shadowed, FileId(3), "value", false);
}
