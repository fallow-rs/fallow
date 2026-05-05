use rustc_hash::FxHashSet;

use crate::graph::ModuleGraph;
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use fallow_types::extract::{ExportName, ImportInfo, ImportedName, VisibilityTag};
use std::path::PathBuf;

#[test]
fn graph_re_export_chain_propagates_references() {
    // entry.ts -> barrel.ts -re-exports-> source.ts
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
        // entry imports "foo" from barrel
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
        // barrel re-exports "foo" from source
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/barrel.ts"),
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
        // source has the actual export
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
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
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    // The source module's "foo" export should have references propagated through the barrel
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
    // Bug fix: entry point barrels that re-export from a source file should
    // propagate "used" status to the source, even with zero in-graph consumers.
    // The entry point's exports are consumed externally.
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
        // index.js (entry point) re-exports render and hydrate from ./render
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
        // render.js exports render and hydrate (no one imports them directly)
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/src/render.js"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("render".to_string()),
                    local_name: Some("render".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
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

    // The entry point itself should be marked as such
    assert!(graph.modules[0].is_entry_point());

    // render.js exports should have synthetic references from the entry point
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
    // Entry point with `export * from './source'` should mark all source exports as used.
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
    // `export *` does not re-export the default export per ES spec.
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
    // entry.ts (entry point) re-exports from barrel.ts, which re-exports from source.ts.
    // No internal consumer imports any of these — only the entry point exposes them.
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
        // index.ts (entry point) re-exports foo from barrel.ts
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
        // barrel.ts re-exports foo from source.ts (not an entry point)
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
        // source.ts has the actual export
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/src/source.ts"),
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
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    // barrel.ts should have a synthetic ExportSymbol for foo with a reference
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

    // source.ts's foo should be referenced through the 2-level chain
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
fn star_re_export_through_multiple_barrel_layers() {
    // consumer.ts imports { foo } from barrel_a.ts
    // barrel_a.ts: export * from './barrel_b'
    // barrel_b.ts: export * from './source'
    // source.ts: export const foo = 1; export const bar = 2;
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
        // consumer imports foo from barrel_a
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
        // barrel_a: export * from './barrel_b'
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
        // barrel_b: export * from './source'
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
        // source.ts: export const foo, bar
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/source.ts"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
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

    // source's foo should be referenced (propagated through 2 star-re-export layers)
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

    // bar was not imported by anyone, so it should remain unreferenced
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
    // consumer.ts: import { bar } from './barrel'
    // barrel.ts: export { foo as bar } from './source'
    // source.ts: export const foo = 1
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
        // consumer imports "bar" from barrel
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
        // barrel: export { foo as bar } from './source'
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
        // source: export const foo
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
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
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    // barrel should have a synthetic ExportSymbol for "bar"
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

    // source's "foo" should be referenced (imported_name="foo" maps to source)
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
    // Entry point barrel with export * from './source' where source only has a default export.
    // Per ES spec, export * does not re-export default, so nothing should be marked used.
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
        // source only has a default export
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/src/source.js"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Default,
                local_name: None,
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
    // a.ts: export { foo } from './b'  (re-exports foo from b)
    // b.ts: export { foo } from './a'  (re-exports foo from a)
    // consumer.ts: import { foo } from './a'
    // This creates a cycle. The loop should terminate (max_iterations guard)
    // without panicking.
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
        // a.ts: export { foo } from './b'
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
        // b.ts: export { foo } from './a'
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
        // consumer imports foo from a
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

    // The key assertion: this should not hang or panic
    let _graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
}

#[test]
fn star_re_export_cycle_terminates() {
    // a.ts: export * from './b'
    // b.ts: export * from './a'
    // consumer.ts: import { x } from './a'
    // Both have an actual export "x" to make propagation meaningful.
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
        // a.ts: export * from './b', also exports x
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/a.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("x".to_string()),
                local_name: Some("x".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
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
        // b.ts: export * from './a'
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
        // consumer imports x from a
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

    // Should not hang
    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    // a's x should still be referenced
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
fn mixed_star_and_named_re_exports_from_same_source() {
    // consumer.ts: import { foo, bar } from './barrel'
    // barrel.ts: export * from './source'; export { baz as bar } from './source'
    // source.ts: export const foo, baz
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
        // consumer imports foo and bar from barrel
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
        // barrel: export * from './source' AND export { baz as bar } from './source'
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
        // source: export const foo, baz
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/source.ts"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
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

    // foo should be referenced via the star re-export path
    let foo = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .expect("source should have foo export");
    assert!(
        !foo.references.is_empty(),
        "foo should be referenced through star re-export"
    );

    // baz should be referenced via the named re-export (barrel exports it as "bar")
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
fn entry_point_named_re_export_no_in_graph_consumers_multiple_exports() {
    // Entry point re-exports named symbols but nothing in the graph imports them.
    // All re-exported source exports should still be marked as used.
    // Additionally, source has an export NOT re-exported by the entry point.
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
        // index.ts (entry point) re-exports only "create" and "destroy" from lib
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
        // lib.ts: export create, destroy, internal_helper
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/src/lib.ts"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("create".to_string()),
                    local_name: Some("create".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
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
    // Per ES spec, `export * from './source'` does NOT re-export the default export.
    // Verify that entry point star re-export does not mark the source's default as used.
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
        // index.ts: export * from './source'
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
        // source.ts: export default function() {} and export const named = 42
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/source.ts"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Default,
                    local_name: None,
                    is_type_only: false,
                    visibility: VisibilityTag::None,
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
    // When there are no re-exports, chain resolution should be a no-op.
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
    // Direct import reference should still work
    assert_eq!(foo.references.len(), 1);
    assert_eq!(foo.references[0].from_file, FileId(0));
}

/// Regression test for quadratic duplicate detection in star re-export propagation.
///
/// When many consumers import the same named export through a star-re-exporting barrel,
/// the reference list grows across iterations. The duplicate check must remain efficient
/// (O(1) via HashSet, not O(n) via linear scan) to avoid quadratic blowup.
///
/// Layout:
///   consumer_0..consumer_N each import { shared } from barrel
///   barrel: export * from './source'
///   source: export const shared = 1; export const other = 2;
#[expect(
    clippy::cast_possible_truncation,
    reason = "test file/span counts are trivially small"
)]
#[test]
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

    // barrel: export * from './source'
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

    // source: export const shared = 1; export const other = 2;
    resolved_modules.push(ResolvedModule {
        file_id: source_id,
        path: PathBuf::from("/project/source.ts"),
        exports: vec![
            fallow_types::extract::ExportInfo {
                name: ExportName::Named("shared".to_string()),
                local_name: Some("shared".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
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
                span: oxc_span::Span::new(25, 45),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
        ],
        ..Default::default()
    });

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    // The source module's "shared" export should have references from all consumers
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

    // The "other" export should have no references (nobody imports it)
    let other = source
        .exports
        .iter()
        .find(|e| e.name.to_string() == "other")
        .expect("source should have 'other' export");
    assert!(
        other.references.is_empty(),
        "'other' should have no references since no consumer imports it"
    );

    // Verify no duplicate references (the HashSet dedup must work correctly)
    let unique_from_files: FxHashSet<FileId> =
        shared.references.iter().map(|r| r.from_file).collect();
    assert_eq!(
        unique_from_files.len(),
        consumer_count,
        "all references should be from distinct consumers (no duplicates)"
    );
}
