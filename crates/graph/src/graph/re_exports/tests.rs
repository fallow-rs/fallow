use rustc_hash::FxHashSet;

use super::propagate::{count_named_import_origin_index_builds, count_star_reference_set_rebuilds};
use super::{
    ReExportPropagationPlan, ReExportTuple, capture_propagation_visits,
    with_re_export_differential_check,
};
use crate::graph::ModuleGraph;
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use fallow_types::extract::{ExportName, ImportInfo, ImportedName, VisibilityTag};
use std::path::PathBuf;

#[test]
fn graph_re_export_chain_propagates_references() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
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
                    source: "./barrel".to_string(),
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
            path: PathBuf::from("/project/barrel.ts"),
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
            }],
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
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
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let source_module = &graph.modules[2];
    let foo_export = source_module
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .unwrap();
    assert!(
        !foo_export.references.is_empty(),
        "source foo should have propagated references through barrel re-export chain"
    );
}

#[test]
fn barrel_re_export_creates_export_symbol() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
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
                    source: "./barrel".to_string(),
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
            path: PathBuf::from("/project/barrel.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
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
            }],
            ..Default::default()
        },
    ];

    let (graph, origin_index_builds) = count_named_import_origin_index_builds(|| {
        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    });

    assert_eq!(
        origin_index_builds, 0,
        "named re-exports should not build the star-import origin index"
    );

    let barrel = &graph.modules[1];
    let foo_export = barrel.exports.iter().find(|e| e.name.to_string() == "foo");
    assert!(
        foo_export.is_some(),
        "barrel should have ExportSymbol for re-exported 'foo'"
    );

    let foo = foo_export.unwrap();
    assert!(
        !foo.references.is_empty(),
        "barrel's foo should have a reference from entry.ts"
    );

    let source = &graph.modules[2];
    let source_foo = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .unwrap();
    assert!(
        !source_foo.references.is_empty(),
        "source foo should have propagated references through barrel"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn barrel_unused_re_export_has_no_references() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
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
                    source: "./barrel".to_string(),
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
            path: PathBuf::from("/project/barrel.ts"),
            re_exports: vec![
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./source".to_string(),
                        imported_name: "foo".to_string(),
                        exported_name: "foo".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                },
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./source".to_string(),
                        imported_name: "bar".to_string(),
                        exported_name: "bar".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                },
            ],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
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
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(25, 45),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let barrel = &graph.modules[1];
    let foo = barrel
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .unwrap();
    assert!(!foo.references.is_empty(), "barrel's foo should be used");

    let bar = barrel
        .exports
        .iter()
        .find(|e| e.name.to_string() == "bar")
        .unwrap();
    assert!(
        bar.references.is_empty(),
        "barrel's bar should be unused (no consumer imports it)"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn type_only_re_export_creates_type_only_export_symbol() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
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
                    source: "./barrel".to_string(),
                    imported_name: ImportedName::Named("UsedType".to_string()),
                    local_name: "UsedType".to_string(),
                    is_type_only: true,
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
            path: PathBuf::from("/project/barrel.ts"),
            re_exports: vec![
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./source".to_string(),
                        imported_name: "UsedType".to_string(),
                        exported_name: "UsedType".to_string(),
                        is_type_only: true,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                },
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./source".to_string(),
                        imported_name: "UnusedType".to_string(),
                        exported_name: "UnusedType".to_string(),
                        is_type_only: true,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                },
            ],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("UsedType".to_string()),
                    local_name: Some("UsedType".to_string()),
                    is_type_only: true,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("UnusedType".to_string()),
                    local_name: Some("UnusedType".to_string()),
                    is_type_only: true,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(25, 45),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let barrel = &graph.modules[1];

    let used_type = barrel
        .exports
        .iter()
        .find(|e| e.name.to_string() == "UsedType")
        .expect("barrel should have ExportSymbol for UsedType");
    assert!(used_type.is_type_only, "UsedType should be type-only");
    assert!(
        !used_type.references.is_empty(),
        "UsedType should have references"
    );

    let unused_type = barrel
        .exports
        .iter()
        .find(|e| e.name.to_string() == "UnusedType")
        .expect("barrel should have ExportSymbol for UnusedType");
    assert!(unused_type.is_type_only, "UnusedType should be type-only");
    assert!(
        unused_type.references.is_empty(),
        "UnusedType should have no references"
    );
}

#[test]
fn default_re_export_creates_default_export_symbol() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
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
                    source: "./barrel".to_string(),
                    imported_name: ImportedName::Named("Accordion".to_string()),
                    local_name: "Accordion".to_string(),
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
            path: PathBuf::from("/project/barrel.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "default".to_string(),
                    exported_name: "Accordion".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Default,
                local_name: None,
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let barrel = &graph.modules[1];
    let accordion = barrel
        .exports
        .iter()
        .find(|e| e.name.to_string() == "Accordion")
        .expect("barrel should have ExportSymbol for Accordion");
    assert!(
        !accordion.references.is_empty(),
        "Accordion should have reference from entry.ts"
    );

    let source = &graph.modules[2];
    let default_export = source
        .exports
        .iter()
        .find(|e| matches!(e.name, ExportName::Default))
        .unwrap();
    assert!(
        !default_export.references.is_empty(),
        "source default export should have propagated references"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn multi_level_re_export_chain_propagation() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel1.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/barrel2.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(3),
            path: PathBuf::from("/project/source.ts"),
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
                    source: "./barrel1".to_string(),
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
            path: PathBuf::from("/project/barrel1.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./barrel2".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/barrel2.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(3)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/source.ts"),
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
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let barrel1 = &graph.modules[1];
    let b1_foo = barrel1
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .unwrap();
    assert!(
        !b1_foo.references.is_empty(),
        "barrel1's foo should be referenced"
    );

    let barrel2 = &graph.modules[2];
    let b2_foo = barrel2
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .unwrap();
    assert!(
        !b2_foo.references.is_empty(),
        "barrel2's foo should be referenced (propagated through chain)"
    );

    let source = &graph.modules[3];
    let src_foo = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .unwrap();
    assert!(
        !src_foo.references.is_empty(),
        "source's foo should be referenced (propagated through 2-level chain)"
    );
}

#[test]
fn entry_point_named_re_export_propagates_to_source() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/index.js"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/render.js"),
            size_bytes: 200,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.js"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/index.js"),
            re_exports: vec![
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./render".to_string(),
                        imported_name: "render".to_string(),
                        exported_name: "render".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./render".to_string(),
                        imported_name: "hydrate".to_string(),
                        exported_name: "hydrate".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
            ],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/src/render.js"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("render".to_string()),
                    local_name: Some("render".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 30),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("hydrate".to_string()),
                    local_name: Some("hydrate".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(35, 65),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    assert!(graph.modules[0].is_entry_point());

    let render_module = &graph.modules[1];
    let render_export = render_module
        .exports
        .iter()
        .find(|e| e.name.to_string() == "render")
        .expect("render.js should have render export");
    assert!(
        !render_export.references.is_empty(),
        "render should be marked as used via entry point re-export"
    );

    let hydrate_export = render_module
        .exports
        .iter()
        .find(|e| e.name.to_string() == "hydrate")
        .expect("render.js should have hydrate export");
    assert!(
        !hydrate_export.references.is_empty(),
        "hydrate should be marked as used via entry point re-export"
    );
}

#[test]
fn entry_point_star_re_export_propagates_to_source() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/index.js"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/utils.js"),
            size_bytes: 200,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.js"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/index.js"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./utils".to_string(),
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
            path: PathBuf::from("/project/src/utils.js"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
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
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(25, 45),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        },
    ];

    let (graph, origin_index_builds) = count_named_import_origin_index_builds(|| {
        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    });

    assert_eq!(
        origin_index_builds, 0,
        "entry-point star re-exports should use the fast path without an origin index"
    );

    let utils_module = &graph.modules[1];
    let foo = utils_module
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .expect("utils should have foo export");
    assert!(
        !foo.references.is_empty(),
        "foo should be marked as used via entry point star re-export"
    );

    let bar = utils_module
        .exports
        .iter()
        .find(|e| e.name.to_string() == "bar")
        .expect("utils should have bar export");
    assert!(
        !bar.references.is_empty(),
        "bar should be marked as used via entry point star re-export"
    );
}

#[test]
fn entry_point_star_re_export_does_not_mark_default_as_used() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/index.js"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/utils.js"),
            size_bytes: 200,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.js"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/index.js"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./utils".to_string(),
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
            path: PathBuf::from("/project/src/utils.js"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: ExportName::Default,
                    local_name: None,
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(25, 45),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let utils_module = &graph.modules[1];
    let foo = utils_module
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .expect("utils should have foo export");
    assert!(
        !foo.references.is_empty(),
        "named export should be marked as used via star re-export"
    );

    let default_export = utils_module
        .exports
        .iter()
        .find(|e| matches!(e.name, ExportName::Default))
        .expect("utils should have default export");
    assert!(
        default_export.references.is_empty(),
        "default export should NOT be marked as used — export * does not re-export default"
    );
}

#[test]
fn entry_point_multi_level_named_re_export_chain() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/index.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/barrel.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/src/source.ts"),
            size_bytes: 50,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/index.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./barrel".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/src/barrel.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/src/source.ts"),
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
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let barrel = &graph.modules[1];
    let barrel_foo = barrel
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .expect("barrel should have synthetic ExportSymbol for foo");
    assert!(
        !barrel_foo.references.is_empty(),
        "barrel's foo should be referenced (from entry point synthetic ref)"
    );

    let source = &graph.modules[2];
    let source_foo = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .expect("source should have foo export");
    assert!(
        !source_foo.references.is_empty(),
        "source's foo should be referenced through entry-point → barrel → source chain"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn star_re_export_through_multiple_barrel_layers() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel_a.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/barrel_b.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(3),
            path: PathBuf::from("/project/source.ts"),
            size_bytes: 50,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./barrel_a".to_string(),
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
            path: PathBuf::from("/project/barrel_a.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./barrel_b".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/barrel_b.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(3)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/source.ts"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
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
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(25, 45),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let source = &graph.modules[3];
    let foo = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .expect("source should have foo export");
    assert!(
        !foo.references.is_empty(),
        "foo should be referenced through 2-level star re-export chain"
    );

    let bar = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "bar")
        .expect("source should have bar export");
    assert!(
        bar.references.is_empty(),
        "bar should not be referenced — no consumer imports it"
    );
}

#[test]
fn entry_point_star_re_export_through_multiple_barrel_layers() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/barrel_a.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel_b.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/barrel_c.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(3),
            path: PathBuf::from("/project/source.ts"),
            size_bytes: 50,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/barrel_a.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/barrel_a.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./barrel_b".to_string(),
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
            path: PathBuf::from("/project/barrel_b.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./barrel_c".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/barrel_c.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(3)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/source.ts"),
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
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let source = &graph.modules[3];
    let foo = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .expect("source should have foo export");
    assert!(
        !foo.references.is_empty(),
        "foo should be referenced through entry-point star barrel chain"
    );
}

#[test]
fn named_re_export_with_rename() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
            size_bytes: 50,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./barrel".to_string(),
                    imported_name: ImportedName::Named("bar".to_string()),
                    local_name: "bar".to_string(),
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
            path: PathBuf::from("/project/barrel.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "bar".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
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
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let barrel = &graph.modules[1];
    let bar_export = barrel
        .exports
        .iter()
        .find(|e| e.name.to_string() == "bar")
        .expect("barrel should have ExportSymbol for renamed re-export 'bar'");
    assert!(
        !bar_export.references.is_empty(),
        "barrel's bar should be referenced by consumer"
    );

    let source = &graph.modules[2];
    let foo_export = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .expect("source should have foo export");
    assert!(
        !foo_export.references.is_empty(),
        "source's foo should be referenced through barrel's renamed re-export"
    );
}

#[test]
fn entry_point_star_re_export_source_has_only_default() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/index.js"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/source.js"),
            size_bytes: 200,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.js"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/index.js"),
            re_exports: vec![ResolvedReExport {
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
            path: PathBuf::from("/project/src/source.js"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Default,
                local_name: None,
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let source = &graph.modules[1];
    let default_export = source
        .exports
        .iter()
        .find(|e| matches!(e.name, ExportName::Default))
        .expect("source should have default export");
    assert!(
        default_export.references.is_empty(),
        "default export should NOT be marked used — export * skips default, \
         and source has no named exports to propagate"
    );
}

#[test]
fn cycle_detection_does_not_infinite_loop() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/a.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/b.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/consumer.ts"),
            size_bytes: 100,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/a.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./b".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/b.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./a".to_string(),
                    imported_name: "foo".to_string(),
                    exported_name: "foo".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(0)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./a".to_string(),
                    imported_name: ImportedName::Named("foo".to_string()),
                    local_name: "foo".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(0)),
            }],
            ..Default::default()
        },
    ];

    let _graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
}

#[test]
fn star_re_export_cycle_terminates() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/a.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/b.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/consumer.ts"),
            size_bytes: 100,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/a.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("x".to_string()),
                local_name: Some("x".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 10),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }],
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./b".to_string(),
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
            path: PathBuf::from("/project/b.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./a".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(0)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./a".to_string(),
                    imported_name: ImportedName::Named("x".to_string()),
                    local_name: "x".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(0)),
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let a_module = &graph.modules[0];
    let x_export = a_module
        .exports
        .iter()
        .find(|e| e.name.to_string() == "x")
        .expect("a should have x export");
    assert!(
        !x_export.references.is_empty(),
        "x should be referenced despite the cycle"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn mixed_star_and_named_re_exports_from_same_source() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
            size_bytes: 50,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: vec![
                ResolvedImport {
                    info: ImportInfo {
                        source: "./barrel".to_string(),
                        imported_name: ImportedName::Named("foo".to_string()),
                        local_name: "foo".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
                ResolvedImport {
                    info: ImportInfo {
                        source: "./barrel".to_string(),
                        imported_name: ImportedName::Named("bar".to_string()),
                        local_name: "bar".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(15, 25),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
            ],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            re_exports: vec![
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./source".to_string(),
                        imported_name: "*".to_string(),
                        exported_name: "*".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                },
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./source".to_string(),
                        imported_name: "baz".to_string(),
                        exported_name: "bar".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                },
            ],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("baz".to_string()),
                    local_name: Some("baz".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(25, 45),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let source = &graph.modules[2];

    let foo = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .expect("source should have foo export");
    assert!(
        !foo.references.is_empty(),
        "foo should be referenced through star re-export"
    );

    let baz = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "baz")
        .expect("source should have baz export");
    assert!(
        !baz.references.is_empty(),
        "baz should be referenced through named re-export 'bar'"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn entry_point_named_re_export_no_in_graph_consumers_multiple_exports() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/index.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/lib.ts"),
            size_bytes: 200,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/index.ts"),
            re_exports: vec![
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./lib".to_string(),
                        imported_name: "create".to_string(),
                        exported_name: "create".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./lib".to_string(),
                        imported_name: "destroy".to_string(),
                        exported_name: "destroy".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
            ],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/src/lib.ts"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("create".to_string()),
                    local_name: Some("create".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 30),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("destroy".to_string()),
                    local_name: Some("destroy".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(35, 65),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("internal_helper".to_string()),
                    local_name: Some("internal_helper".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(70, 100),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let lib = &graph.modules[1];

    let create = lib
        .exports
        .iter()
        .find(|e| e.name.to_string() == "create")
        .expect("lib should have create export");
    assert!(
        !create.references.is_empty(),
        "create should be marked used via entry point re-export"
    );

    let destroy = lib
        .exports
        .iter()
        .find(|e| e.name.to_string() == "destroy")
        .expect("lib should have destroy export");
    assert!(
        !destroy.references.is_empty(),
        "destroy should be marked used via entry point re-export"
    );

    let internal = lib
        .exports
        .iter()
        .find(|e| e.name.to_string() == "internal_helper")
        .expect("lib should have internal_helper export");
    assert!(
        internal.references.is_empty(),
        "internal_helper should NOT be marked used — not re-exported by entry point"
    );
}

#[test]
fn entry_point_star_re_export_skips_default() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/index.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/source.ts"),
            size_bytes: 50,
        },
    ];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/index.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/index.ts"),
            re_exports: vec![ResolvedReExport {
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
            path: PathBuf::from("/project/source.ts"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Default,
                    local_name: None,
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("named".to_string()),
                    local_name: Some("named".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(25, 45),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                },
            ],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let source = &graph.modules[1];

    let default_export = source
        .exports
        .iter()
        .find(|e| matches!(e.name, ExportName::Default))
        .unwrap();
    assert!(
        default_export.references.is_empty(),
        "default export should NOT be marked as used by `export *` (ES spec)"
    );

    let named_export = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "named")
        .unwrap();
    assert!(
        !named_export.references.is_empty(),
        "named export should be marked as used by entry point `export *`"
    );
}

#[test]
fn no_re_exports_skips_chain_resolution() {
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
                expected_unused_reason: None,
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
    let foo = utils
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .unwrap();
    assert_eq!(foo.references.len(), 1);
    assert_eq!(foo.references[0].from_file, FileId(0));
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "test file/span counts are trivially small"
)]
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn star_re_export_many_consumers_no_quadratic_blowup() {
    let consumer_count = 20;
    let barrel_id = FileId(consumer_count as u32);
    let source_id = FileId(consumer_count as u32 + 1);

    let mut files: Vec<DiscoveredFile> = (0..consumer_count)
        .map(|i| DiscoveredFile {
            id: FileId(i as u32),
            path: PathBuf::from(format!("/project/consumer{i}.ts")),
            size_bytes: 50,
        })
        .collect();
    files.push(DiscoveredFile {
        id: barrel_id,
        path: PathBuf::from("/project/barrel.ts"),
        size_bytes: 50,
    });
    files.push(DiscoveredFile {
        id: source_id,
        path: PathBuf::from("/project/source.ts"),
        size_bytes: 50,
    });

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer0.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let mut resolved_modules: Vec<ResolvedModule> = (0..consumer_count)
        .map(|i| ResolvedModule {
            file_id: FileId(i as u32),
            path: PathBuf::from(format!("/project/consumer{i}.ts")),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./barrel".to_string(),
                    imported_name: ImportedName::Named("shared".to_string()),
                    local_name: "shared".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(barrel_id),
            }],
            ..Default::default()
        })
        .collect();

    resolved_modules.push(ResolvedModule {
        file_id: barrel_id,
        path: PathBuf::from("/project/barrel.ts"),
        re_exports: vec![ResolvedReExport {
            info: fallow_types::extract::ReExportInfo {
                source: "./source".to_string(),
                imported_name: "*".to_string(),
                exported_name: "*".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(source_id),
        }],
        ..Default::default()
    });

    resolved_modules.push(ResolvedModule {
        file_id: source_id,
        path: PathBuf::from("/project/source.ts"),
        exports: vec![
            fallow_types::extract::ExportInfo {
                name: ExportName::Named("shared".to_string()),
                local_name: Some("shared".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
            fallow_types::extract::ExportInfo {
                name: ExportName::Named("other".to_string()),
                local_name: Some("other".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(25, 45),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
        ],
        ..Default::default()
    });

    let (graph, reference_set_rebuilds) = count_star_reference_set_rebuilds(|| {
        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    });

    assert!(
        (1..=2).contains(&reference_set_rebuilds),
        "the reference set should be built at most once per fixpoint iteration"
    );

    let source = &graph.modules[source_id.0 as usize];
    let shared = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "shared")
        .expect("source should have 'shared' export");
    assert_eq!(
        shared.references.len(),
        consumer_count,
        "each consumer should add exactly one reference to the source export"
    );

    let other = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "other")
        .expect("source should have 'other' export");
    assert!(
        other.references.is_empty(),
        "'other' should have no references since no consumer imports it"
    );

    let unique_from_files: FxHashSet<FileId> =
        shared.references.iter().map(|r| r.from_file).collect();
    assert_eq!(
        unique_from_files.len(),
        consumer_count,
        "all references should be from distinct consumers (no duplicates)"
    );
}

#[test]
fn deep_named_re_export_chain_propagates_25_hops() {
    fn run_chain(barrel_count: u32) {
        let consumer_id = FileId(0);
        let leaf_id = FileId(barrel_count + 1);

        let mut files: Vec<DiscoveredFile> = (0..=barrel_count + 1)
            .map(|i| DiscoveredFile {
                id: FileId(i),
                path: if i == 0 {
                    PathBuf::from("/project/consumer.ts")
                } else if i == barrel_count + 1 {
                    PathBuf::from("/project/leaf.ts")
                } else {
                    PathBuf::from(format!("/project/barrel_{i}.ts"))
                },
                size_bytes: 50,
            })
            .collect();

        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/consumer.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];

        let mut resolved_modules: Vec<ResolvedModule> = vec![ResolvedModule {
            file_id: consumer_id,
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./barrel_1".to_string(),
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
        }];

        for i in 1..=barrel_count {
            let next_id = FileId(i + 1);
            let next_source = if i == barrel_count {
                "./leaf".to_string()
            } else {
                format!("./barrel_{}", i + 1)
            };
            resolved_modules.push(ResolvedModule {
                file_id: FileId(i),
                path: PathBuf::from(format!("/project/barrel_{i}.ts")),
                re_exports: vec![ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: next_source,
                        imported_name: "foo".to_string(),
                        exported_name: "foo".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(next_id),
                }],
                ..Default::default()
            });
        }

        resolved_modules.push(ResolvedModule {
            file_id: leaf_id,
            path: PathBuf::from("/project/leaf.ts"),
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
            }],
            ..Default::default()
        });

        let _ = &mut files; // silence unused warning under expect
        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

        let leaf = &graph.modules[leaf_id.0 as usize];
        let foo = leaf
            .exports
            .iter()
            .find(|e| e.name.to_string() == "foo")
            .unwrap_or_else(|| panic!("leaf should have foo export ({barrel_count}-hop chain)"));
        assert!(
            !foo.references.is_empty(),
            "leaf's foo should be referenced through a {barrel_count}-hop chain"
        );
    }

    run_chain(21);
    run_chain(25);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "fixture construction makes the scheduling assertion self-contained"
)]
fn work_queue_does_not_revisit_unrelated_settled_edge() {
    const CHAIN_LENGTH: u32 = 6;
    let unrelated_barrel_id = FileId(CHAIN_LENGTH + 2);
    let unrelated_source_id = FileId(CHAIN_LENGTH + 3);
    let unrelated_consumer_id = FileId(CHAIN_LENGTH + 4);

    let mut files = Vec::new();
    let mut resolved_modules = Vec::new();

    files.push(discovered_file(0, "/project/leaf.ts"));
    resolved_modules.push(ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/leaf.ts"),
        exports: vec![merged_export(false)],
        ..Default::default()
    });

    for idx in 1..=CHAIN_LENGTH {
        let path = format!("/project/barrel_{idx}.ts");
        files.push(discovered_file(idx, &path));
        resolved_modules.push(ResolvedModule {
            file_id: FileId(idx),
            path: PathBuf::from(path),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: format!("./barrel_{}", idx - 1),
                    imported_name: "Merged".to_string(),
                    exported_name: "Merged".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(idx - 1)),
            }],
            ..Default::default()
        });
    }

    let chain_consumer_id = FileId(CHAIN_LENGTH + 1);
    files.push(discovered_file(
        chain_consumer_id.0,
        "/project/chain_consumer.ts",
    ));
    resolved_modules.push(ResolvedModule {
        file_id: chain_consumer_id,
        path: PathBuf::from("/project/chain_consumer.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: format!("./barrel_{CHAIN_LENGTH}"),
                imported_name: ImportedName::Named("Merged".to_string()),
                local_name: "Merged".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(CHAIN_LENGTH)),
        }],
        ..Default::default()
    });

    files.push(discovered_file(
        unrelated_barrel_id.0,
        "/project/unrelated_barrel.ts",
    ));
    resolved_modules.push(ResolvedModule {
        file_id: unrelated_barrel_id,
        path: PathBuf::from("/project/unrelated_barrel.ts"),
        re_exports: vec![ResolvedReExport {
            info: fallow_types::extract::ReExportInfo {
                source: "./unrelated_source".to_string(),
                imported_name: "Merged".to_string(),
                exported_name: "Merged".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(unrelated_source_id),
        }],
        ..Default::default()
    });

    files.push(discovered_file(
        unrelated_source_id.0,
        "/project/unrelated_source.ts",
    ));
    resolved_modules.push(ResolvedModule {
        file_id: unrelated_source_id,
        path: PathBuf::from("/project/unrelated_source.ts"),
        exports: vec![merged_export(false)],
        ..Default::default()
    });

    files.push(discovered_file(
        unrelated_consumer_id.0,
        "/project/unrelated_consumer.ts",
    ));
    resolved_modules.push(ResolvedModule {
        file_id: unrelated_consumer_id,
        path: PathBuf::from("/project/unrelated_consumer.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./unrelated_barrel".to_string(),
                imported_name: ImportedName::Named("Merged".to_string()),
                local_name: "Merged".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(unrelated_barrel_id),
        }],
        ..Default::default()
    });

    let entry_points = vec![
        EntryPoint {
            path: PathBuf::from("/project/chain_consumer.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
        EntryPoint {
            path: PathBuf::from("/project/unrelated_consumer.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
    ];

    let (_, visits) =
        capture_propagation_visits(|| ModuleGraph::build(&resolved_modules, &entry_points, &files));
    let unrelated_visits = visits
        .iter()
        .filter(|visit| **visit == (unrelated_barrel_id, unrelated_source_id))
        .count();

    assert_eq!(
        unrelated_visits, 1,
        "an unrelated settled edge should only run during the initial stable-order pass"
    );
}

#[test]
fn work_queue_matches_legacy_on_generated_small_graphs() {
    with_re_export_differential_check(|| {
        for barrel_count in 1..=8 {
            let graph = graph_for_named_chain(barrel_count);
            let leaf = &graph.modules[0];
            assert!(
                leaf.exports[0].references.iter().any(|reference| {
                    reference.from_file == FileId(barrel_count.saturating_add(1))
                }),
                "generated {barrel_count}-barrel graph should reach the leaf"
            );
        }

        graph_for_merged_star_import(
            vec![
                named_import("Merged", "MergedValue", false),
                named_import("Merged", "MergedType", true),
            ],
            vec!["MergedType"],
            vec!["MergedValue"],
        );
        graph_for_merged_star_chain_import(
            vec![
                named_import_with_span("Merged", "MergedType", false, 0, 10),
                named_import_with_span("Merged", "MergedValue", false, 20, 30),
            ],
            vec!["MergedType"],
            vec!["MergedValue"],
        );
    });
}

#[test]
fn equivalent_re_export_declaration_orders_produce_identical_results() {
    let forward = graph_for_parallel_named_re_exports(false);
    let reversed = graph_for_parallel_named_re_exports(true);

    assert_eq!(
        serde_json::to_value(&forward.modules[2].exports).unwrap(),
        serde_json::to_value(&reversed.modules[2].exports).unwrap(),
    );
    assert_eq!(
        serde_json::to_value(&forward.modules[3].exports).unwrap(),
        serde_json::to_value(&reversed.modules[3].exports).unwrap(),
    );
}

#[test]
fn propagation_plan_preserves_stable_fifo_order() {
    let entries = [
        re_export_tuple(2, 1),
        re_export_tuple(1, 0),
        re_export_tuple(4, 3),
    ];
    let mut plan = ReExportPropagationPlan::new(&entries);

    assert_eq!(plan.pop_front(), Some(0));
    assert_eq!(plan.pop_front(), Some(1));
    assert_eq!(plan.pop_front(), Some(2));
    assert_eq!(plan.pop_front(), None);
}

#[test]
fn propagation_plan_requeues_only_affected_observers_once() {
    let entries = [
        re_export_tuple(2, 1),
        re_export_tuple(1, 0),
        re_export_tuple(2, 3),
        re_export_tuple(4, 2),
    ];
    let mut plan = ReExportPropagationPlan::new(&entries);
    while plan.pop_front().is_some() {}

    plan.enqueue_observers(FileId(2));
    plan.enqueue_observers(FileId(2));

    assert_eq!(plan.pop_front(), Some(0));
    assert_eq!(plan.pop_front(), Some(2));
    assert_eq!(plan.pop_front(), None);
}

#[test]
fn named_only_chain_safety_cap_excludes_synthetic_export_states() {
    let graph = graph_for_named_chain(256);
    let re_export_info = graph.collect_re_export_tuples();
    let initial_exports = graph
        .modules
        .iter()
        .map(|module| module.exports.len())
        .sum::<usize>();
    let expected = re_export_info.len().saturating_add(
        initial_exports
            .saturating_mul(graph.modules.len())
            .saturating_mul(re_export_info.len()),
    );

    assert_eq!(
        graph.re_export_transition_safety_cap(&re_export_info),
        expected,
        "named-only chains cannot create synthetic exports"
    );
}

fn re_export_tuple(barrel: u32, source: u32) -> ReExportTuple {
    ReExportTuple {
        barrel: FileId(barrel),
        source: FileId(source),
        imported_name: "Merged".to_string(),
        exported_name: "Merged".to_string(),
        is_type_only: false,
    }
}

fn graph_for_named_chain(barrel_count: u32) -> ModuleGraph {
    let consumer_id = FileId(barrel_count + 1);
    let mut files = vec![discovered_file(0, "/project/leaf.ts")];
    let mut resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/leaf.ts"),
        exports: vec![merged_export(false)],
        ..Default::default()
    }];

    for idx in 1..=barrel_count {
        let path = format!("/project/barrel_{idx}.ts");
        files.push(discovered_file(idx, &path));
        resolved_modules.push(ResolvedModule {
            file_id: FileId(idx),
            path: PathBuf::from(path),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: format!("./barrel_{}", idx - 1),
                    imported_name: "Merged".to_string(),
                    exported_name: "Merged".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(idx - 1)),
            }],
            ..Default::default()
        });
    }

    files.push(discovered_file(consumer_id.0, "/project/consumer.ts"));
    resolved_modules.push(ResolvedModule {
        file_id: consumer_id,
        path: PathBuf::from("/project/consumer.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: format!("./barrel_{barrel_count}"),
                imported_name: ImportedName::Named("Merged".to_string()),
                local_name: "Merged".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(barrel_count)),
        }],
        ..Default::default()
    });

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    ModuleGraph::build(&resolved_modules, &entry_points, &files)
}

fn graph_for_parallel_named_re_exports(reverse: bool) -> ModuleGraph {
    let files = vec![
        discovered_file(0, "/project/consumer.ts"),
        discovered_file(1, "/project/barrel.ts"),
        discovered_file(2, "/project/a.ts"),
        discovered_file(3, "/project/b.ts"),
    ];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let mut re_exports = vec![
        ResolvedReExport {
            info: fallow_types::extract::ReExportInfo {
                source: "./a".to_string(),
                imported_name: "A".to_string(),
                exported_name: "A".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(2)),
        },
        ResolvedReExport {
            info: fallow_types::extract::ReExportInfo {
                source: "./b".to_string(),
                imported_name: "B".to_string(),
                exported_name: "B".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(3)),
        },
    ];
    if reverse {
        re_exports.reverse();
    }

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: vec![
                ResolvedImport {
                    info: named_import("A", "A", false).info,
                    target: ResolveResult::InternalModule(FileId(1)),
                },
                ResolvedImport {
                    info: named_import("B", "B", false).info,
                    target: ResolveResult::InternalModule(FileId(1)),
                },
            ],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            re_exports,
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/a.ts"),
            exports: vec![named_export("A", false)],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/b.ts"),
            exports: vec![named_export("B", false)],
            ..Default::default()
        },
    ];

    ModuleGraph::build(&resolved_modules, &entry_points, &files)
}

#[expect(
    clippy::too_many_lines,
    reason = "fixture construction dominates; assertions stay tight"
)]
#[test]
fn re_export_cycle_terminates_and_does_not_block_unrelated_propagation() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/a.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/b.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/c.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(3),
            path: PathBuf::from("/project/outside.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(4),
            path: PathBuf::from("/project/consumer.ts"),
            size_bytes: 100,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/a.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("x".to_string()),
                local_name: Some("x".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 10),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }],
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./b".to_string(),
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
            path: PathBuf::from("/project/b.ts"),
            re_exports: vec![
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./c".to_string(),
                        imported_name: "*".to_string(),
                        exported_name: "*".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                },
                ResolvedReExport {
                    info: fallow_types::extract::ReExportInfo {
                        source: "./a".to_string(),
                        imported_name: "*".to_string(),
                        exported_name: "*".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(0)),
                },
            ],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/c.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./a".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(0)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/outside.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("y".to_string()),
                local_name: Some("y".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 10),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(4),
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: vec![
                ResolvedImport {
                    info: ImportInfo {
                        source: "./a".to_string(),
                        imported_name: ImportedName::Named("x".to_string()),
                        local_name: "x".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(0)),
                },
                ResolvedImport {
                    info: ImportInfo {
                        source: "./outside".to_string(),
                        imported_name: ImportedName::Named("y".to_string()),
                        local_name: "y".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(15, 25),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(3)),
                },
            ],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let a = &graph.modules[0];
    let x = a
        .exports
        .iter()
        .find(|e| e.name.to_string() == "x")
        .expect("a should have x export");
    assert!(
        !x.references.is_empty(),
        "x should be referenced despite the cycle"
    );

    let outside = &graph.modules[3];
    let y = outside
        .exports
        .iter()
        .find(|e| e.name.to_string() == "y")
        .expect("outside should have y export");
    assert!(
        !y.references.is_empty(),
        "y should be referenced from consumer (cycle elsewhere must not block this)"
    );
}

#[test]
fn type_only_star_chain_synthesizes_type_only_stub() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/barrel.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/source.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/leaf.ts"),
            size_bytes: 50,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/barrel.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/barrel.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: true,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/source.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./leaf".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/leaf.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("X".to_string()),
                local_name: Some("X".to_string()),
                is_type_only: true,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let leaf = &graph.modules[2];
    let x = leaf
        .exports
        .iter()
        .find(|e| e.name.to_string() == "X")
        .expect("leaf should have X export");
    assert!(
        !x.references.is_empty(),
        "X should be referenced through the entry-point type-only star chain"
    );

    let source = &graph.modules[1];
    if let Some(stub) = source.exports.iter().find(|e| e.name.to_string() == "X") {
        assert!(
            stub.is_type_only,
            "synthetic stub on source for X must inherit is_type_only=true \
             from the triggering `export type *` edge on barrel"
        );
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn type_only_star_chain_named_consumer_synthesizes_type_only_stub() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(3),
            path: PathBuf::from("/project/leaf.ts"),
            size_bytes: 50,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./barrel".to_string(),
                    imported_name: ImportedName::Named("X".to_string()),
                    local_name: "X".to_string(),
                    is_type_only: true,
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
            path: PathBuf::from("/project/barrel.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: true,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./leaf".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(3)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/leaf.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("X".to_string()),
                local_name: Some("X".to_string()),
                is_type_only: true,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let source = &graph.modules[2];
    let stub = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "X")
        .expect("source should have a synthetic stub for X");
    assert!(
        stub.is_type_only,
        "synthetic stub on source for X must inherit is_type_only=true \
         from the triggering `export type *` edge on barrel"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "fixture enumerates a mixed star-export graph across value and type paths"
)]
fn mixed_type_only_and_value_star_paths_synthesize_value_stub() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/consumer_type.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/consumer_val.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/barrel_type.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(3),
            path: PathBuf::from("/project/barrel_val.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(4),
            path: PathBuf::from("/project/source.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(5),
            path: PathBuf::from("/project/leaf.ts"),
            size_bytes: 50,
        },
    ];

    let entry_points = vec![
        EntryPoint {
            path: PathBuf::from("/project/consumer_type.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
        EntryPoint {
            path: PathBuf::from("/project/consumer_val.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
    ];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/consumer_type.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./barrel_type".to_string(),
                    imported_name: ImportedName::Named("X".to_string()),
                    local_name: "X".to_string(),
                    is_type_only: true,
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
            path: PathBuf::from("/project/consumer_val.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./barrel_val".to_string(),
                    imported_name: ImportedName::Named("X".to_string()),
                    local_name: "X".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(3)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/barrel_type.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: true,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(4)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/barrel_val.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(4)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(4),
            path: PathBuf::from("/project/source.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./leaf".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(5)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(5),
            path: PathBuf::from("/project/leaf.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("X".to_string()),
                local_name: Some("X".to_string()),
                is_type_only: true,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            }],
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let source = &graph.modules[4];
    let type_stub = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "X" && e.is_type_only)
        .expect("source should have a synthetic type stub for X");
    let value_stub = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "X" && !e.is_type_only)
        .expect("source should have a synthetic value stub for X");
    assert!(
        !type_stub.references.is_empty(),
        "type-only star edge should keep a type synthetic stub"
    );
    assert!(
        !value_stub.references.is_empty(),
        "value star edge should keep a value synthetic stub"
    );
}

#[test]
fn self_re_export_does_not_panic() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/barrel.ts"),
        size_bytes: 50,
    }];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/barrel.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/barrel.ts"),
        re_exports: vec![ResolvedReExport {
            info: fallow_types::extract::ReExportInfo {
                source: "./barrel".to_string(),
                imported_name: "*".to_string(),
                exported_name: "*".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(0)),
        }],
        ..Default::default()
    }];

    let _graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn re_export_cycle_payload_lists_member_paths() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/cycle_a.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/cycle_b.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/cycle_c.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(3),
            path: PathBuf::from("/project/consumer.ts"),
            size_bytes: 100,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/cycle_a.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./cycle_b".to_string(),
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
            path: PathBuf::from("/project/cycle_b.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./cycle_c".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/cycle_c.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./cycle_a".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(0)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/consumer.ts"),
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let cycle = graph
        .re_export_cycles
        .iter()
        .find(|cycle| !cycle.is_self_loop)
        .expect("expected a multi-file re-export cycle");
    let members = cycle
        .files
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        members,
        vec![
            "/project/cycle_a.ts",
            "/project/cycle_b.ts",
            "/project/cycle_c.ts",
        ]
    );
    assert_eq!(
        cycle.file_ids,
        vec![FileId(0), FileId(1), FileId(2)],
        "expected file ids to stay parallel to the sorted paths"
    );
}

#[test]
fn self_re_export_payload_names_file() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/self_barrel.ts"),
        size_bytes: 50,
    }];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/self_barrel.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/self_barrel.ts"),
        re_exports: vec![ResolvedReExport {
            info: fallow_types::extract::ReExportInfo {
                source: "./self_barrel".to_string(),
                imported_name: "*".to_string(),
                exported_name: "*".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(0)),
        }],
        ..Default::default()
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    assert_eq!(graph.re_export_cycles.len(), 1);
    let cycle = &graph.re_export_cycles[0];
    assert!(cycle.is_self_loop, "expected self-loop cycle payload");
    assert_eq!(cycle.files, vec![PathBuf::from("/project/self_barrel.ts")]);
    assert_eq!(cycle.file_ids, vec![FileId(0)]);
}

#[test]
fn star_re_export_duplicate_name_value_import_credits_value_export() {
    let graph = graph_for_merged_star_import(
        vec![named_import("Merged", "Local", false)],
        vec![],
        vec!["Local"],
    );

    let (type_export, value_export) = merged_exports(&graph);
    assert!(
        type_export.references.is_empty(),
        "value use through a star barrel must not credit the type-only namespace"
    );
    assert!(
        !value_export.references.is_empty(),
        "value use through a star barrel should credit the value export"
    );
}

#[test]
fn star_re_export_origin_index_is_built_once_per_graph() {
    let (_, builds) = count_named_import_origin_index_builds(|| {
        graph_for_merged_star_chain_import(
            vec![named_import("Merged", "Local", false)],
            vec![],
            vec!["Local"],
        )
    });

    assert_eq!(builds, 1, "the whole-edge origin index must be shared");
}

#[test]
fn star_re_export_duplicate_name_type_import_credits_type_export() {
    let graph = graph_for_merged_star_import(
        vec![named_import("Merged", "MergedType", true)],
        vec!["MergedType"],
        vec![],
    );

    let (type_export, value_export) = merged_exports(&graph);
    assert!(
        !type_export.references.is_empty(),
        "type-only use through a star barrel should credit the type export"
    );
    assert!(
        value_export.references.is_empty(),
        "type-only use through a star barrel must not credit the value export"
    );
}

#[test]
fn star_re_export_duplicate_name_mixed_import_credits_both_exports() {
    let graph = graph_for_merged_star_import(
        vec![
            named_import("Merged", "MergedValue", false),
            named_import("Merged", "MergedType", true),
        ],
        vec!["MergedType"],
        vec!["MergedValue"],
    );

    let (type_export, value_export) = merged_exports(&graph);
    assert!(
        !type_export.references.is_empty(),
        "type use through a star barrel should credit the type export"
    );
    assert!(
        !value_export.references.is_empty(),
        "value use through a star barrel should credit the value export"
    );
}

#[test]
fn star_re_export_duplicate_name_multi_hop_type_usage_credits_type_export() {
    let graph = graph_for_merged_star_chain_import(
        vec![named_import("Merged", "MergedType", false)],
        vec!["MergedType"],
        vec![],
    );

    let (type_export, value_export) = merged_exports(&graph);
    assert!(
        !type_export.references.is_empty(),
        "normal import used only as a type through a multi-hop star barrel should credit the type export"
    );
    assert!(
        value_export.references.is_empty(),
        "normal import used only as a type through a multi-hop star barrel must not credit the value export"
    );
}

#[test]
fn star_re_export_duplicate_name_multi_hop_mixed_usage_credits_both_exports() {
    let graph = graph_for_merged_star_chain_import(
        vec![
            named_import_with_span("Merged", "MergedType", false, 0, 10),
            named_import_with_span("Merged", "MergedValue", false, 20, 30),
        ],
        vec!["MergedType"],
        vec!["MergedValue"],
    );

    let (type_export, value_export) = merged_exports(&graph);
    assert!(
        !type_export.references.is_empty(),
        "type use forwarded through an intermediate star stub should credit the type export"
    );
    assert!(
        !value_export.references.is_empty(),
        "value use forwarded through an intermediate star stub should credit the value export"
    );
}

fn graph_for_merged_star_import(
    imports: Vec<ResolvedImport>,
    type_usages: Vec<&str>,
    value_usages: Vec<&str>,
) -> ModuleGraph {
    let files = vec![
        discovered_file(0, "/project/consumer.ts"),
        discovered_file(1, "/project/barrel.ts"),
        discovered_file(2, "/project/merged.ts"),
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: imports,
            type_referenced_import_bindings: type_usages.into_iter().map(str::to_string).collect(),
            value_referenced_import_bindings: value_usages
                .into_iter()
                .map(str::to_string)
                .collect(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./merged".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/merged.ts"),
            exports: vec![merged_export(true), merged_export(false)],
            ..Default::default()
        },
    ];

    ModuleGraph::build(&resolved_modules, &entry_points, &files)
}

fn graph_for_merged_star_chain_import(
    imports: Vec<ResolvedImport>,
    type_usages: Vec<&str>,
    value_usages: Vec<&str>,
) -> ModuleGraph {
    let files = vec![
        discovered_file(0, "/project/consumer.ts"),
        discovered_file(1, "/project/barrel.ts"),
        discovered_file(2, "/project/intermediate.ts"),
        discovered_file(3, "/project/merged.ts"),
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/consumer.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/consumer.ts"),
            resolved_imports: imports,
            type_referenced_import_bindings: type_usages.into_iter().map(str::to_string).collect(),
            value_referenced_import_bindings: value_usages
                .into_iter()
                .map(str::to_string)
                .collect(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./intermediate".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/intermediate.ts"),
            re_exports: vec![ResolvedReExport {
                info: fallow_types::extract::ReExportInfo {
                    source: "./merged".to_string(),
                    imported_name: "*".to_string(),
                    exported_name: "*".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(3)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/merged.ts"),
            exports: vec![merged_export(true), merged_export(false)],
            ..Default::default()
        },
    ];

    ModuleGraph::build(&resolved_modules, &entry_points, &files)
}

fn discovered_file(id: u32, path: &str) -> DiscoveredFile {
    DiscoveredFile {
        id: FileId(id),
        path: PathBuf::from(path),
        size_bytes: 100,
    }
}

fn named_import(imported_name: &str, local_name: &str, is_type_only: bool) -> ResolvedImport {
    named_import_with_span(imported_name, local_name, is_type_only, 0, 10)
}

fn named_import_with_span(
    imported_name: &str,
    local_name: &str,
    is_type_only: bool,
    span_start: u32,
    span_end: u32,
) -> ResolvedImport {
    ResolvedImport {
        info: ImportInfo {
            source: "./barrel".to_string(),
            imported_name: ImportedName::Named(imported_name.to_string()),
            local_name: local_name.to_string(),
            is_type_only,
            from_style: false,
            span: oxc_span::Span::new(span_start, span_end),
            source_span: oxc_span::Span::default(),
        },
        target: ResolveResult::InternalModule(FileId(1)),
    }
}

fn merged_export(is_type_only: bool) -> fallow_types::extract::ExportInfo {
    named_export("Merged", is_type_only)
}

fn named_export(name: &str, is_type_only: bool) -> fallow_types::extract::ExportInfo {
    fallow_types::extract::ExportInfo {
        name: ExportName::Named(name.to_string()),
        local_name: Some(name.to_string()),
        is_type_only,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 20),
        members: vec![],
        is_side_effect_used: false,
        super_class: None,
    }
}

fn merged_exports(
    graph: &ModuleGraph,
) -> (
    &crate::graph::types::ExportSymbol,
    &crate::graph::types::ExportSymbol,
) {
    let source = graph
        .modules
        .iter()
        .find(|module| module.path.ends_with("merged.ts"))
        .expect("source module should exist");
    let type_export = source
        .exports
        .iter()
        .find(|export| export.name.to_string() == "Merged" && export.is_type_only)
        .expect("type export should exist");
    let value_export = source
        .exports
        .iter()
        .find(|export| export.name.to_string() == "Merged" && !export.is_type_only)
        .expect("value export should exist");
    (type_export, value_export)
}
