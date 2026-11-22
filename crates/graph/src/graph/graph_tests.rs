use super::*;
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use fallow_types::extract::{ExportName, ImportInfo, ImportedName, VisibilityTag};
use std::path::PathBuf;

fn build_simple_graph() -> ModuleGraph {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/utils.ts"),
            size_bytes: 50,
        },
    ];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/entry.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./utils".to_string(),
                    imported_name: ImportedName::Named("foo".to_string()),
                    local_name: "foo".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
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
            path: PathBuf::from("/project/src/utils.ts"),
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
                    deprecated: false,
                    deprecated_reason: None,
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
                    deprecated: false,
                    deprecated_reason: None,
                },
            ]
            .into(),
            ..Default::default()
        },
    ];

    ModuleGraph::build(&resolved_modules, &entry_points, &files)
}

fn declaration_seeding_graph(unseeded: &FxHashSet<FileId>) -> ModuleGraph {
    let paths = [
        "/project/types/ambient.d.ts",
        "/project/types/orphan.d.ts",
        "/project/types/helper.ts",
    ];
    let files: Vec<DiscoveredFile> = paths
        .iter()
        .zip(0u32..)
        .map(|(path, id)| DiscoveredFile {
            id: FileId(id),
            path: PathBuf::from(path),
            size_bytes: 10,
        })
        .collect();
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: files[0].path.clone(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: files[1].path.clone(),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./helper".to_string(),
                    imported_name: ImportedName::Named("Helper".to_string()),
                    local_name: "Helper".to_string(),
                    is_type_only: true,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: files[2].path.clone(),
            ..Default::default()
        },
    ];
    ModuleGraph::build_with_declaration_seeding(
        &resolved_modules,
        &[],
        &ReachabilityRoots {
            all: &[],
            runtime: &[],
            test: &[],
            consumer_checked: &FxHashSet::default(),
        },
        &files,
        unseeded,
    )
}

#[test]
fn every_declaration_file_is_seeded_by_default() {
    let graph = declaration_seeding_graph(&FxHashSet::default());
    assert!(graph.modules[0].is_entry_point());
    assert!(graph.modules[1].is_entry_point());
    assert!(graph.modules[2].is_reachable());
}

#[test]
fn unseeded_declaration_file_no_longer_keeps_its_imports_reachable() {
    let unseeded: FxHashSet<FileId> = std::iter::once(FileId(1)).collect();
    let graph = declaration_seeding_graph(&unseeded);
    assert!(graph.modules[0].is_entry_point());
    assert!(!graph.modules[1].is_entry_point());
    assert!(!graph.modules[1].is_reachable());
    assert!(!graph.modules[2].is_reachable());
}

#[test]
fn graph_module_count() {
    let graph = build_simple_graph();
    assert_eq!(graph.module_count(), 2);
}

#[test]
fn graph_edge_count() {
    let graph = build_simple_graph();
    assert_eq!(graph.edge_count(), 1);
}

#[test]
fn graph_entry_point_is_reachable() {
    let graph = build_simple_graph();
    assert!(graph.modules[0].is_entry_point());
    assert!(graph.modules[0].is_reachable());
}

#[test]
fn graph_imported_module_is_reachable() {
    let graph = build_simple_graph();
    assert!(!graph.modules[1].is_entry_point());
    assert!(graph.modules[1].is_reachable());
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "this test fixture exercises four reachability roles end-to-end; splitting it \
              would obscure the cross-role assertions"
)]
fn graph_distinguishes_runtime_test_and_support_reachability() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/main.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/runtime-only.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/tests/app.test.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(3),
            path: PathBuf::from("/project/tests/setup.ts"),
            size_bytes: 50,
        },
        DiscoveredFile {
            id: FileId(4),
            path: PathBuf::from("/project/src/covered.ts"),
            size_bytes: 50,
        },
    ];

    let all_entry_points = vec![
        EntryPoint {
            path: PathBuf::from("/project/src/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
        EntryPoint {
            path: PathBuf::from("/project/tests/app.test.ts"),
            source: EntryPointSource::TestFile,
        },
        EntryPoint {
            path: PathBuf::from("/project/tests/setup.ts"),
            source: EntryPointSource::Plugin {
                name: "vitest".to_string(),
            },
        },
    ];
    let runtime_entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/main.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let test_entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/tests/app.test.ts"),
        source: EntryPointSource::TestFile,
    }];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/main.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./runtime-only".to_string(),
                    imported_name: ImportedName::Named("runtimeOnly".to_string()),
                    local_name: "runtimeOnly".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
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
            path: PathBuf::from("/project/src/runtime-only.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("runtimeOnly".to_string()),
                local_name: Some("runtimeOnly".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            }]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/tests/app.test.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "../src/covered".to_string(),
                    imported_name: ImportedName::Named("covered".to_string()),
                    local_name: "covered".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(4)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/project/tests/setup.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "../src/runtime-only".to_string(),
                    imported_name: ImportedName::Named("runtimeOnly".to_string()),
                    local_name: "runtimeOnly".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(4),
            path: PathBuf::from("/project/src/covered.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("covered".to_string()),
                local_name: Some("covered".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            }]
            .into(),
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build_with_reachability_roots(
        &resolved_modules,
        &all_entry_points,
        &runtime_entry_points,
        &test_entry_points,
        &files,
    );

    assert!(graph.modules[1].is_reachable());
    assert!(graph.modules[1].is_runtime_reachable());
    assert!(
        !graph.modules[1].is_test_reachable(),
        "support roots should not make runtime-only modules test reachable"
    );

    assert!(graph.modules[4].is_reachable());
    assert!(graph.modules[4].is_test_reachable());
    assert!(
        !graph.modules[4].is_runtime_reachable(),
        "test-only reachability should stay separate from runtime roots"
    );
}

#[test]
fn graph_export_has_reference() {
    let graph = build_simple_graph();
    let utils = &graph.modules[1];
    let foo_export = utils
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .unwrap();
    assert!(
        !foo_export.references.is_empty(),
        "foo should have references"
    );
}

#[test]
fn graph_unused_export_no_reference() {
    let graph = build_simple_graph();
    let utils = &graph.modules[1];
    let bar_export = utils
        .exports
        .iter()
        .find(|e| e.name.to_string() == "bar")
        .unwrap();
    assert!(
        bar_export.references.is_empty(),
        "bar should have no references"
    );
}

#[test]
fn graph_no_namespace_import() {
    let graph = build_simple_graph();
    assert!(!graph.has_namespace_import(FileId(0)));
    assert!(!graph.has_namespace_import(FileId(1)));
}

#[test]
fn graph_has_namespace_import() {
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
                    imported_name: ImportedName::Namespace,
                    local_name: "utils".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
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
                deprecated: false,
                deprecated_reason: None,
            }]
            .into(),
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    assert!(
        graph.has_namespace_import(FileId(1)),
        "utils should have namespace import"
    );
}

#[test]
fn graph_has_namespace_import_out_of_bounds() {
    let graph = build_simple_graph();
    assert!(!graph.has_namespace_import(FileId(999)));
}

/// The persisted graph cache skips `namespace_imported` and rebuilds it from
/// the edge set on load. This asserts the reconstruction reproduces the
/// fresh-built bitset BIT-FOR-BIT on a graph that exercises `import * as ns`,
/// matching what `build.rs` records at build time.
#[test]
fn reconstruct_namespace_imported_matches_fresh_build() {
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
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/named-only.ts"),
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
            resolved_imports: vec![
                ResolvedImport {
                    info: ImportInfo {
                        source: "./utils".to_string(),
                        imported_name: ImportedName::Namespace,
                        local_name: "utils".to_string(),
                        is_type_only: false,
                        is_type_only_star: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
                ResolvedImport {
                    info: ImportInfo {
                        source: "./named-only".to_string(),
                        imported_name: ImportedName::Named("foo".to_string()),
                        local_name: "foo".to_string(),
                        is_type_only: false,
                        is_type_only_star: false,
                        from_style: false,
                        span: oxc_span::Span::new(11, 20),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                },
            ],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/utils.ts"),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/named-only.ts"),
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
                deprecated: false,
                deprecated_reason: None,
            }]
            .into(),
            ..Default::default()
        },
    ];

    let mut graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let fresh = graph.namespace_imported.clone();

    // Sanity: the namespace target is set, the named-only target is not.
    assert!(graph.has_namespace_import(FileId(1)));
    assert!(!graph.has_namespace_import(FileId(2)));

    // Simulate the cache load: the bitset arrives empty (serde-skipped), then
    // the loader reconstructs it from the persisted edges.
    graph.namespace_imported = FixedBitSet::default();
    graph.reconstruct_namespace_imported();

    assert_eq!(
        graph.namespace_imported, fresh,
        "reconstructed namespace_imported must equal the fresh-built bitset"
    );
    assert!(graph.has_namespace_import(FileId(1)));
    assert!(!graph.has_namespace_import(FileId(2)));
}

#[test]
fn graph_unreachable_module() {
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
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/orphan.ts"),
            size_bytes: 30,
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
                    is_type_only_star: false,
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
                deprecated: false,
                deprecated_reason: None,
            }]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/orphan.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("orphan".to_string()),
                local_name: Some("orphan".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            }]
            .into(),
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    assert!(graph.modules[0].is_reachable(), "entry should be reachable");
    assert!(graph.modules[1].is_reachable(), "utils should be reachable");
    assert!(
        !graph.modules[2].is_reachable(),
        "orphan should NOT be reachable"
    );
}

#[test]
fn graph_package_usage_tracked() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/entry.ts"),
        size_bytes: 100,
    }];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/entry.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/entry.ts"),
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![
            ResolvedImport {
                info: ImportInfo {
                    source: "react".to_string(),
                    imported_name: ImportedName::Default,
                    local_name: "React".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("react".to_string()),
            },
            ResolvedImport {
                info: ImportInfo {
                    source: "lodash".to_string(),
                    imported_name: ImportedName::Named("merge".to_string()),
                    local_name: "merge".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(15, 30),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("lodash".to_string()),
            },
        ],
        ..Default::default()
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    assert!(graph.package_usage.contains_key("react"));
    assert!(graph.package_usage.contains_key("lodash"));
    assert!(!graph.package_usage.contains_key("express"));
}

#[test]
fn graph_empty() {
    let graph = ModuleGraph::build(&[], &[], &[]);
    assert_eq!(graph.module_count(), 0);
    assert_eq!(graph.edge_count(), 0);
}

/// The persisted graph cache postcard-encodes the whole `ModuleGraph` and
/// decodes it on a warm run. This proves the serde round-trip is lossless
/// for the structural surface analysis reads: module / edge / export /
/// reference counts and the `namespace_imported` bitset (reconstructed on
/// load) all survive.
#[test]
fn graph_postcard_round_trip_is_lossless() {
    let graph = build_simple_graph();

    let encoded = postcard::to_allocvec(&graph).expect("encode graph");
    let mut decoded: ModuleGraph = postcard::from_bytes(&encoded).expect("decode graph");
    // The store does this on load; do it here so the bitset is restored.
    decoded.reconstruct_namespace_imported();

    assert_eq!(decoded.module_count(), graph.module_count());
    assert_eq!(decoded.edge_count(), graph.edge_count());
    assert_eq!(decoded.namespace_imported, graph.namespace_imported);

    // Export + reference + member surface survives byte-for-byte.
    let utils = &decoded.modules[1];
    let foo = utils
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .expect("foo export survives round-trip");
    assert!(!foo.references.is_empty());
    let bar = utils
        .exports
        .iter()
        .find(|e| e.name.to_string() == "bar")
        .expect("bar export survives round-trip");
    assert!(bar.references.is_empty());

    // Reachability flags and entry-point sets survive.
    assert!(decoded.modules[0].is_entry_point());
    assert!(decoded.modules[0].is_reachable());
    assert!(decoded.modules[1].is_reachable());
    assert_eq!(decoded.entry_points, graph.entry_points);
}

#[test]
fn graph_cjs_exports_tracked() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/entry.ts"),
        size_bytes: 100,
    }];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/entry.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/entry.ts"),
        has_cjs_exports: true,
        has_angular_component_template_url: false,
        ..Default::default()
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    assert!(graph.modules[0].has_cjs_exports());
}

#[test]
fn graph_edges_for_returns_targets() {
    let graph = build_simple_graph();
    let targets = graph.edges_for(FileId(0));
    assert_eq!(targets, vec![FileId(1)]);
}

#[test]
fn graph_edges_for_no_imports() {
    let graph = build_simple_graph();
    let targets = graph.edges_for(FileId(1));
    assert!(targets.is_empty());
}

#[test]
fn graph_edges_for_out_of_bounds() {
    let graph = build_simple_graph();
    let targets = graph.edges_for(FileId(999));
    assert!(targets.is_empty());
}

#[test]
fn graph_direct_importer_summaries_include_symbols() {
    let graph = build_simple_graph();
    let summaries = graph.direct_importer_summaries(FileId(1));

    assert_eq!(
        summaries,
        vec![DirectImporterSummary {
            source: FileId(0),
            symbols: vec![ImportedSymbolSummary {
                imported: "foo".to_string(),
                local: "foo".to_string(),
                type_only: false,
            }],
        }]
    );
}

#[test]
fn graph_find_import_span_start_found() {
    let graph = build_simple_graph();
    let span_start = graph.find_import_span_start(FileId(0), FileId(1));
    assert!(span_start.is_some());
    assert_eq!(span_start.unwrap(), 0);
}

#[test]
fn graph_find_import_span_start_prefers_value_import_on_mixed_edge() {
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
            resolved_imports: vec![
                ResolvedImport {
                    info: ImportInfo {
                        source: "./utils".to_string(),
                        imported_name: ImportedName::Named("Foo".to_string()),
                        local_name: "Foo".to_string(),
                        is_type_only: true,
                        is_type_only_star: false,
                        from_style: false,
                        span: oxc_span::Span::new(10, 20),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
                ResolvedImport {
                    info: ImportInfo {
                        source: "./utils".to_string(),
                        imported_name: ImportedName::Named("foo".to_string()),
                        local_name: "foo".to_string(),
                        is_type_only: false,
                        is_type_only_star: false,
                        from_style: false,
                        span: oxc_span::Span::new(50, 60),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
            ],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/utils.ts"),
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    assert_eq!(graph.find_import_span_start(FileId(0), FileId(1)), Some(50));
}

#[test]
fn graph_find_import_span_start_wrong_target() {
    let graph = build_simple_graph();
    let span_start = graph.find_import_span_start(FileId(0), FileId(0));
    assert!(span_start.is_none());
}

#[test]
fn graph_find_import_span_start_source_out_of_bounds() {
    let graph = build_simple_graph();
    let span_start = graph.find_import_span_start(FileId(999), FileId(1));
    assert!(span_start.is_none());
}

#[test]
fn graph_find_import_span_start_no_edges() {
    let graph = build_simple_graph();
    let span_start = graph.find_import_span_start(FileId(1), FileId(0));
    assert!(span_start.is_none());
}

#[test]
fn graph_reverse_deps_populated() {
    let graph = build_simple_graph();
    assert!(graph.reverse_deps[1].contains(&FileId(0)));
    assert!(graph.reverse_deps[0].is_empty());
}

#[test]
fn graph_type_only_package_usage_tracked() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/entry.ts"),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/entry.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/entry.ts"),
        resolved_imports: vec![
            ResolvedImport {
                info: ImportInfo {
                    source: "react".to_string(),
                    imported_name: ImportedName::Named("FC".to_string()),
                    local_name: "FC".to_string(),
                    is_type_only: true,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("react".to_string()),
            },
            ResolvedImport {
                info: ImportInfo {
                    source: "react".to_string(),
                    imported_name: ImportedName::Named("useState".to_string()),
                    local_name: "useState".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(15, 30),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("react".to_string()),
            },
        ],
        ..Default::default()
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    assert!(graph.package_usage.contains_key("react"));
    assert!(graph.type_only_package_usage.contains_key("react"));
}

#[test]
fn graph_default_import_reference() {
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
                    imported_name: ImportedName::Default,
                    local_name: "Utils".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
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
                name: ExportName::Default,
                local_name: None,
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            }]
            .into(),
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let utils = &graph.modules[1];
    let default_export = utils
        .exports
        .iter()
        .find(|e| matches!(e.name, ExportName::Default))
        .unwrap();
    assert!(!default_export.references.is_empty());
    assert_eq!(
        default_export.references[0].kind,
        ReferenceKind::DefaultImport
    );
}

#[test]
fn graph_side_effect_import_no_export_reference() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/styles.ts"),
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
                    source: "./styles".to_string(),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: false,
                    is_type_only_star: false,
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
            path: PathBuf::from("/project/styles.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Named("primaryColor".to_string()),
                local_name: Some("primaryColor".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 20),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            }]
            .into(),
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    assert_eq!(graph.edge_count(), 1);
    let styles = &graph.modules[1];
    assert!(styles.is_reachable());
    let export = &styles.exports[0];
    assert!(
        export.references.is_empty(),
        "side-effect import should not reference named exports"
    );

    let encoded = postcard::to_allocvec(&graph).expect("encode graph");
    let decoded: ModuleGraph = postcard::from_bytes(&encoded).expect("decode graph");
    assert_eq!(decoded.edge_count(), 1);
    assert!(decoded.modules[1].is_reachable());
    assert!(decoded.modules[1].exports[0].references.is_empty());
}

#[test]
fn graph_multiple_entry_points() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/main.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/worker.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/shared.ts"),
            size_bytes: 50,
        },
    ];
    let entry_points = vec![
        EntryPoint {
            path: PathBuf::from("/project/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
        EntryPoint {
            path: PathBuf::from("/project/worker.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
    ];
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/main.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./shared".to_string(),
                    imported_name: ImportedName::Named("helper".to_string()),
                    local_name: "helper".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
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
            path: PathBuf::from("/project/worker.ts"),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/project/shared.ts"),
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
                deprecated: false,
                deprecated_reason: None,
            }]
            .into(),
            ..Default::default()
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    assert!(graph.modules[0].is_entry_point());
    assert!(graph.modules[1].is_entry_point());
    assert!(!graph.modules[2].is_entry_point());
    assert!(graph.modules[0].is_reachable());
    assert!(graph.modules[1].is_reachable());
    assert!(graph.modules[2].is_reachable());
}
