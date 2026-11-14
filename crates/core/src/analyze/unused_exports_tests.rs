use super::*;
use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use crate::extract::{
    ExportInfo, ExportName, ImportInfo, ImportedName, ReExportInfo, VisibilityTag,
};
use crate::graph::{
    ExportNamespace, ExportSymbol, ModuleGraph, ReExportEdge, ReferenceKind, SymbolReference,
};
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};
use crate::suppress::Suppression;
use oxc_span::Span;
use std::path::PathBuf;

/// Build a minimal ModuleGraph via the build() constructor.
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
            exports: vec![].into(),
            re_exports: vec![],
            resolved_imports: vec![],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![].into(),
            semantic_facts: std::sync::Arc::default(),
            whole_object_uses: std::sync::Arc::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: std::sync::Arc::default(),
            exported_factory_return_object_shapes: std::sync::Arc::default(),
            type_member_types: std::sync::Arc::default(),
            missing_export_targets: vec![],
        })
        .collect();

    ModuleGraph::build(&resolved_modules, &entry_points, &files)
}

/// Build a default ResolvedConfig for tests.
pub(super) fn test_config() -> ResolvedConfig {
    fallow_config::FallowConfig::default().resolve(
        PathBuf::from("/tmp/test"),
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    )
}

#[test]
fn signature_backing_types_preserve_path_identity_and_unbacked_findings() {
    use fallow_types::extract::{ModuleInfo, PublicSignatureTypeReference};

    let graph = build_graph(&[("/project/a.ts", false), ("/project/b.ts", false)]);
    let reference = |type_name: &str| PublicSignatureTypeReference {
        export_name: "publicFunction".to_owned(),
        type_name: type_name.to_owned(),
        span: Span::new(0, 1),
        from_satisfies: false,
    };
    let mut known = ModuleInfo::empty(FileId(0));
    known.public_signature_type_references = vec![reference("Backed"), reference("Backed")];
    let mut unknown = ModuleInfo::empty(FileId(99));
    unknown.public_signature_type_references = vec![reference("Backed"), reference("Unbacked")];
    let finding = |path: &str, name: &str| UnusedExport {
        path: PathBuf::from(path),
        export_name: name.to_owned(),
        is_type_only: true,
        line: 1,
        col: 0,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    };
    let mut findings = vec![
        finding("/project/a.ts", "Backed"),
        finding("/project/b.ts", "Backed"),
        finding("/project/a.ts", "Unbacked"),
    ];

    suppress_signature_backing_types(&mut findings, &[], &graph, &[known, unknown]);

    let retained: Vec<_> = findings
        .iter()
        .map(|finding| (finding.path.as_path(), finding.export_name.as_str()))
        .collect();
    assert_eq!(
        retained,
        vec![
            (std::path::Path::new("/project/b.ts"), "Backed"),
            (std::path::Path::new("/project/a.ts"), "Unbacked"),
        ]
    );
}

#[test]
fn signature_backing_types_need_a_live_backer() {
    use fallow_types::extract::{ModuleInfo, PublicSignatureTypeReference};

    let graph = build_graph(&[("/project/a.ts", false)]);
    let reference = |export_name: &str, type_name: &str| PublicSignatureTypeReference {
        export_name: export_name.to_owned(),
        type_name: type_name.to_owned(),
        span: Span::new(0, 1),
        from_satisfies: false,
    };
    let mut module = ModuleInfo::empty(FileId(0));
    module.public_signature_type_references = vec![
        // Backed only by an unused value export.
        reference("summarize", "Summary"),
        // Backed by one used and one unused value export.
        reference("usedReader", "Entry"),
        reference("unusedReader", "Entry"),
        // `Outer` is backed by a used export, `Inner` only by `Outer`.
        reference("build", "Outer"),
        reference("Outer", "Inner"),
        // `Leaf` is backed only by `Branch`, which is itself unused.
        reference("summarize", "Branch"),
        reference("Branch", "Leaf"),
        // Backed only by itself.
        reference("TreeNode", "TreeNode"),
    ];
    let finding = |name: &str, is_type_only: bool| UnusedExport {
        path: PathBuf::from("/project/a.ts"),
        export_name: name.to_owned(),
        is_type_only,
        line: 1,
        col: 0,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    };
    let unused_values = vec![finding("summarize", false), finding("unusedReader", false)];
    let mut findings = vec![
        finding("Summary", true),
        finding("Entry", true),
        finding("Outer", true),
        finding("Inner", true),
        finding("Branch", true),
        finding("Leaf", true),
        finding("TreeNode", true),
    ];

    suppress_signature_backing_types(&mut findings, &unused_values, &graph, &[module]);

    let retained: Vec<_> = findings
        .iter()
        .map(|finding| finding.export_name.as_str())
        .collect();
    assert_eq!(retained, vec!["Summary", "Branch", "Leaf", "TreeNode"]);
}

fn make_export(name: &str, span_start: u32, span_end: u32) -> ExportSymbol {
    ExportSymbol {
        name: ExportName::Named(name.to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(span_start, span_end),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "test file counts are trivially small"
)]
fn build_graph_with_referenced_export(
    file_specs: &[(&str, bool)],
    target: usize,
    name: &str,
    span_start: u32,
    span_end: u32,
    from: u32,
) -> ModuleGraph {
    let files: Vec<DiscoveredFile> = file_specs
        .iter()
        .enumerate()
        .map(|(index, (path, _))| DiscoveredFile {
            id: FileId(index as u32),
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
    let mut resolved_modules: Vec<ResolvedModule> = files
        .iter()
        .map(|file| ResolvedModule {
            file_id: file.id,
            path: file.path.clone(),
            ..Default::default()
        })
        .collect();
    resolved_modules[target].exports = vec![ExportInfo {
        name: ExportName::Named(name.to_string()),
        local_name: None,
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(span_start, span_end),
        members: vec![],
        super_class: None,
        deprecated: false,
        deprecated_reason: None,
    }]
    .into();
    resolved_modules[from as usize]
        .resolved_imports
        .push(ResolvedImport {
            info: ImportInfo {
                source: format!("./fixture-{target}"),
                imported_name: ImportedName::Named(name.to_string()),
                local_name: name.to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: Span::new(0, 10),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(target as u32)),
        });

    ModuleGraph::build(&resolved_modules, &entry_points, &files)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "test file counts are trivially small"
)]
pub(super) fn build_duplicate_graph(
    file_specs: &[(&str, bool)],
    exports: &[(usize, &str, bool)],
    imports: &[(usize, usize, &str, bool)],
) -> ModuleGraph {
    let files: Vec<DiscoveredFile> = file_specs
        .iter()
        .enumerate()
        .map(|(index, (path, _))| DiscoveredFile {
            id: FileId(index as u32),
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
    let mut resolved_modules: Vec<ResolvedModule> = files
        .iter()
        .map(|file| ResolvedModule {
            file_id: file.id,
            path: file.path.clone(),
            ..Default::default()
        })
        .collect();

    let mut exports_by_module = vec![Vec::new(); resolved_modules.len()];
    for &(module, name, is_type_only) in exports {
        exports_by_module[module].push(ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: Span::new(10, 20),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        });
    }
    for (module, module_exports) in resolved_modules.iter_mut().zip(exports_by_module) {
        module.exports = module_exports.into();
    }
    for &(importer, source, name, is_type_only) in imports {
        resolved_modules[importer]
            .resolved_imports
            .push(ResolvedImport {
                info: ImportInfo {
                    source: format!("./{source}"),
                    imported_name: ImportedName::Named(name.to_string()),
                    local_name: format!("{name}{source}"),
                    is_type_only,
                    is_type_only_star: false,
                    from_style: false,
                    span: Span::new(1, 4),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(source as u32)),
            });
    }

    ModuleGraph::build(&resolved_modules, &entry_points, &files)
}

#[test]
fn duplicate_exports_empty_graph() {
    let graph = build_graph(&[]);
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(result.is_empty());
}

#[test]
fn duplicate_exports_no_duplicates_single_module() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/utils.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("foo", 10, 20), make_export("bar", 30, 40)];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(result.is_empty());
}

#[test]
fn duplicate_exports_detects_same_name_in_two_modules() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ],
        &[(1, "helper", false), (2, "helper", false)],
        &[(0, 1, "helper", false), (0, 2, "helper", false)],
    );
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].export_name, "helper");
    assert_eq!(result[0].locations.len(), 2);
}

#[test]
fn duplicate_exports_skips_default_exports() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/a.ts", false),
        ("/src/b.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![ExportSymbol {
        name: ExportName::Default,
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(10, 20),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![ExportSymbol {
        name: ExportName::Default,
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(10, 20),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(result.is_empty());
}

#[test]
fn duplicate_exports_skips_synthetic_re_export_entries() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/a.ts", false),
        ("/src/b.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("helper", 0, 0)]; // synthetic
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export("helper", 10, 20)]; // real
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(result.is_empty());
}

#[test]
fn duplicate_exports_skips_unreachable_modules() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/a.ts", false),
        ("/src/b.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("helper", 10, 20)];
    graph.modules[2].exports = vec![make_export("helper", 10, 20)];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(result.is_empty());
}

#[test]
fn duplicate_exports_skips_entry_points() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/b.ts", false)]);
    graph.modules[0].exports = vec![make_export("helper", 10, 20)];
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("helper", 10, 20)];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(result.is_empty());
}

#[test]
fn duplicate_exports_filters_re_export_chains() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/index.ts", false),
        ("/src/helper.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("helper", 10, 20)];
    graph.modules[1].re_exports = vec![ReExportEdge {
        source_file: FileId(2),
        imported_name: "helper".to_string(),
        exported_name: "helper".to_string(),
        is_type_only: false,
        span: oxc_span::Span::default(),
    }];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export("helper", 5, 15)];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(result.is_empty());
}

#[test]
fn duplicate_exports_follow_type_only_star_routes_for_value_backed_exports() {
    let paths = ["/src/entry.ts", "/src/barrel.ts", "/src/a.ts", "/src/b.ts"];
    let files: Vec<DiscoveredFile> = paths
        .iter()
        .enumerate()
        .map(|(index, path)| DiscoveredFile {
            id: FileId(index as u32),
            path: PathBuf::from(path),
            size_bytes: 0,
        })
        .collect();
    let entry_points = vec![EntryPoint {
        path: files[0].path.clone(),
        source: EntryPointSource::ManualEntry,
    }];
    let class_export = |file_id: u32| ResolvedModule {
        file_id: FileId(file_id),
        path: files[file_id as usize].path.clone(),
        exports: vec![ExportInfo {
            name: ExportName::Named("Foo".to_string()),
            local_name: Some("Foo".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: Span::new(10, 20),
            members: vec![],
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        }]
        .into(),
        ..Default::default()
    };
    let type_star = |source: &str, target: u32| ResolvedReExport {
        info: ReExportInfo {
            source: source.to_string(),
            imported_name: "*".to_string(),
            exported_name: "*".to_string(),
            is_type_only: true,
            span: Span::new(10, 20),
            statement_span: Span::new(10, 30),
            source_span: Span::new(24, 29),
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
        },
        target: ResolveResult::InternalModule(FileId(target)),
    };
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: files[0].path.clone(),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./barrel".to_string(),
                    imported_name: ImportedName::Named("Foo".to_string()),
                    local_name: "Foo".to_string(),
                    is_type_only: true,
                    is_type_only_star: false,
                    from_style: false,
                    span: Span::new(0, 10),
                    source_span: Span::new(0, 1),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: files[1].path.clone(),
            re_exports: vec![type_star("./a", 2), type_star("./b", 3)],
            ..Default::default()
        },
        class_export(2),
        class_export(3),
    ];
    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let result = find_duplicate_exports(
        &graph,
        &test_config(),
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &resolved_modules,
    );

    assert_eq!(
        result.len(),
        1,
        "ambiguous type-only stars must retain both class exports"
    );
    assert_eq!(result[0].export_name, "Foo");
    assert_eq!(result[0].locations.len(), 2);
}

#[test]
fn duplicate_exports_preserves_local_export_with_unrelated_re_export() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            size_bytes: 0,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/src/source.ts"),
            size_bytes: 0,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/src/barrel.ts"),
            size_bytes: 0,
        },
    ];
    let entry_points = vec![EntryPoint {
        path: files[0].path.clone(),
        source: EntryPointSource::ManualEntry,
    }];
    let imported_foo = |target: FileId| ResolvedImport {
        info: ImportInfo {
            source: format!("./{}", target.0),
            imported_name: ImportedName::Named("Foo".to_string()),
            local_name: format!("Foo{}", target.0),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: Span::new(1, 4),
            source_span: Span::default(),
        },
        target: ResolveResult::InternalModule(target),
    };
    let export = |name: &str, span: Span| ExportInfo {
        name: ExportName::Named(name.to_string()),
        local_name: Some(name.to_string()),
        is_type_only: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span,
        members: vec![],
        is_side_effect_used: false,
        super_class: None,
        deprecated: false,
        deprecated_reason: None,
    };
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: files[0].path.clone(),
            resolved_imports: vec![imported_foo(FileId(1)), imported_foo(FileId(2))],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: files[1].path.clone(),
            exports: vec![
                export("Foo", Span::new(10, 13)),
                export("Bar", Span::new(20, 23)),
            ]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: files[2].path.clone(),
            exports: vec![export("Foo", Span::new(30, 33))].into(),
            re_exports: vec![ResolvedReExport {
                info: ReExportInfo {
                    source: "./source".to_string(),
                    imported_name: "Bar".to_string(),
                    exported_name: "Bar".to_string(),
                    is_type_only: false,
                    span: Span::new(40, 43),
                    statement_span: Span::new(35, 44),
                    source_span: Span::new(35, 39),
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        },
    ];
    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let result = find_duplicate_exports(
        &graph,
        &test_config(),
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &resolved_modules,
    );

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].export_name, "Foo");
    assert_eq!(result[0].locations.len(), 2);
}

#[test]
fn duplicate_exports_suppressed_file_wide() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/a.ts", false),
        ("/src/b.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("helper", 10, 20)];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export("helper", 10, 20)];

    let supp = vec![Suppression::issue(0, 1, IssueKind::DuplicateExport)];
    let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
    supp_map.insert(FileId(2), &supp);
    let suppressions = SuppressionContext::from_map(supp_map);

    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(result.is_empty());
}

#[test]
fn duplicate_exports_three_modules_same_name() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
            ("/src/c.ts", false),
        ],
        &[
            (1, "sharedFn", false),
            (2, "sharedFn", false),
            (3, "sharedFn", false),
        ],
        &[
            (0, 1, "sharedFn", false),
            (0, 2, "sharedFn", false),
            (0, 3, "sharedFn", false),
        ],
    );
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].export_name, "sharedFn");
    assert_eq!(result[0].locations.len(), 3);
}

#[test]
fn duplicate_exports_unrelated_leaf_files_not_flagged() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/routes/foo/page.ts", false),
        ("/src/routes/bar/page.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("Area", 10, 20)];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export("Area", 10, 20)];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(
        result.is_empty(),
        "unrelated leaf files should not be flagged as duplicates"
    );
}

#[test]
fn duplicate_exports_different_names_not_duplicated() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/a.ts", false),
        ("/src/b.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("foo", 10, 20)];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export("bar", 10, 20)];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(result.is_empty());
}

#[test]
fn duplicate_exports_value_type_merging_not_flagged() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/schema.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![
        make_export("Status", 10, 20),
        make_type_export("Status", 50, 60),
    ];
    graph.reverse_deps[1] = vec![FileId(0)];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(
        result.is_empty(),
        "value+type merging should not be flagged as duplicate"
    );
}

#[test]
fn duplicate_exports_value_type_cross_file_not_flagged() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/a.ts", false),
        ("/src/b.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("Status", 10, 20)];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_type_export("Status", 10, 20)];
    graph.reverse_deps[1] = vec![FileId(0)];
    graph.reverse_deps[2] = vec![FileId(0)];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(
        result.is_empty(),
        "cross-file value+type should not be flagged"
    );
}

#[test]
fn duplicate_exports_same_namespace_still_flagged() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ],
        &[(1, "helper", false), (2, "helper", false)],
        &[(0, 1, "helper", false), (0, 2, "helper", false)],
    );
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert_eq!(
        result.len(),
        1,
        "same-namespace duplicates should still be flagged"
    );
}

/// Regression test for issue #848.
///
/// Setup:
///   module 1 (backend): `class Label` (value) -- no shared importer with frontend
///   module 2 (frontend value): `function Label` (value)
///   module 3 (frontend type): `export type Label` (type)
///   module 4 (frontend consumer): imports from both module 2 and module 3
///
/// Before the fix, `value_modules` contained both module 1 and module 2, so
/// `.len() > 1` defeated the self-suppression check and a false duplicate was
/// reported. After the fix the group is partitioned: module 1 is a singleton
/// (dropped), and the {module 2, module 3} pair self-suppresses via value+type.
#[test]
fn duplicate_exports_cross_package_backend_does_not_defeat_value_type_suppression() {
    // Files: 0=backend-entry, 1=frontend-entry, 2=backend Label, 3=frontend Label value,
    // 4=frontend Label type, 5=frontend consumer
    let mut graph = build_graph(&[
        ("/src/backend/entry.ts", true),
        ("/src/frontend/entry.ts", true),
        ("/src/backend/label.ts", false),
        ("/src/frontend/label.ts", false),
        ("/src/frontend/label.types.ts", false),
        ("/src/frontend/consumer.ts", false),
    ]);
    // Reachable non-entry modules
    graph.modules[2].set_reachable(true);
    graph.modules[3].set_reachable(true);
    graph.modules[4].set_reachable(true);
    graph.modules[5].set_reachable(true);

    // Backend: value export `Label`
    graph.modules[2].exports = vec![make_export("Label", 10, 20)];
    // Frontend value: `function Label`
    graph.modules[3].exports = vec![make_export("Label", 10, 20)];
    // Frontend type: `export type Label`
    graph.modules[4].exports = vec![make_type_export("Label", 10, 20)];

    // Backend label is imported only by the backend entry (FileId 0).
    graph.reverse_deps[2] = vec![FileId(0)];
    // Frontend consumer (FileId 5) imports from both the frontend value and type.
    graph.reverse_deps[3] = vec![FileId(5)];
    graph.reverse_deps[4] = vec![FileId(5)];
    // Frontend consumer is imported by the frontend entry.
    graph.reverse_deps[5] = vec![FileId(1)];

    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(
        result.is_empty(),
        "backend `class Label` shares no importer with the frontend value+type pair; \
            the frontend pair must self-suppress, so no duplicate should be reported, \
            but got: {result:?}",
    );
}

/// Companion to the regression test: two value modules that DO share a common
/// importer must still be reported as genuine duplicates even when an unrelated
/// third module with the same name (but no shared importer) exists.
#[test]
fn duplicate_exports_genuine_value_duplicate_still_flagged_despite_unrelated_backend() {
    // Files: 0=entry, 1=backend Label (no shared importer), 2=frontend-a Label,
    // 3=frontend-b Label, 4=shared consumer of frontend-a and frontend-b
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/backend/label.ts", false),
            ("/src/frontend/a.ts", false),
            ("/src/frontend/b.ts", false),
            ("/src/frontend/consumer.ts", false),
        ],
        &[
            (1, "Label", false),
            (2, "Label", false),
            (3, "Label", false),
            (4, "consumer", false),
        ],
        &[
            (0, 1, "Label", false),
            (0, 4, "consumer", false),
            (4, 2, "Label", false),
            (4, 3, "Label", false),
        ],
    );

    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert_eq!(
        result.len(),
        1,
        "frontend-a and frontend-b share a common importer and both export a value \
            `Label`, so a genuine duplicate must be reported even though an unrelated \
            backend module also exports `Label`"
    );
    assert_eq!(result[0].export_name, "Label");
    // The finding must include only the two connected frontend locations, not backend.
    assert_eq!(
        result[0].locations.len(),
        2,
        "only the two connected frontend locations should be in the finding"
    );
}

fn make_resolved_module(file_id: u32, path: &str) -> crate::resolve::ResolvedModule {
    crate::resolve::ResolvedModule {
        file_id: FileId(file_id),
        path: std::path::PathBuf::from(path),
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![].into(),
        semantic_facts: std::sync::Arc::default(),
        whole_object_uses: std::sync::Arc::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: std::sync::Arc::default(),
        exported_factory_return_object_shapes: std::sync::Arc::default(),
        type_member_types: std::sync::Arc::default(),
        missing_export_targets: vec![],
    }
}

fn dynamic_import_to(
    target_id: u32,
    imported_name: crate::extract::ImportedName,
) -> crate::resolve::ResolvedImport {
    crate::resolve::ResolvedImport {
        info: fallow_types::extract::ImportInfo {
            source: "./target".to_string(),
            imported_name,
            local_name: "_local".to_string(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: oxc_span::Span::new(0, 1),
            source_span: oxc_span::Span::default(),
        },
        target: crate::resolve::ResolveResult::InternalModule(FileId(target_id)),
    }
}

#[test]
fn dynamic_import_then_member_not_flagged_as_duplicate() {
    use crate::extract::ImportedName;

    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/Foo.tsx", false),
        ("/src/Foo-lazy.tsx", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("Foo", 10, 30)];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export("Foo", 10, 30)];
    graph.reverse_deps[1] = vec![FileId(0)];
    graph.reverse_deps[2] = vec![FileId(0)];

    let mut foo_lazy = make_resolved_module(2, "/src/Foo-lazy.tsx");
    foo_lazy.resolved_dynamic_imports =
        vec![dynamic_import_to(1, ImportedName::Named("Foo".to_string()))];

    let resolved_modules = vec![make_resolved_module(1, "/src/Foo.tsx"), foo_lazy];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(
        &graph,
        &config,
        &suppressions,
        &FxHashMap::default(),
        &resolved_modules,
    );
    assert!(
        result.is_empty(),
        "dynamic(import().then(m=>m.Foo)) wrapper must not be flagged as duplicate-export"
    );
}

#[test]
fn dynamic_import_without_then_default_not_flagged_as_duplicate() {
    use crate::extract::{ExportName, ImportedName, VisibilityTag};
    use crate::graph::ExportSymbol;

    let make_default_export = || ExportSymbol {
        name: ExportName::Default,
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 10),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    };

    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/Foo.tsx", false),
        ("/src/Foo-lazy.tsx", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_default_export()];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_default_export()];
    graph.reverse_deps[1] = vec![FileId(0)];
    graph.reverse_deps[2] = vec![FileId(0)];

    let mut foo_lazy = make_resolved_module(2, "/src/Foo-lazy.tsx");
    foo_lazy.resolved_dynamic_imports = vec![dynamic_import_to(1, ImportedName::Default)];

    let resolved_modules = vec![make_resolved_module(1, "/src/Foo.tsx"), foo_lazy];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(
        &graph,
        &config,
        &suppressions,
        &FxHashMap::default(),
        &resolved_modules,
    );
    assert!(
        result.is_empty(),
        "dynamic(import('./Foo')) wrapper with matching default export must not be flagged"
    );
}

#[test]
fn dynamic_import_named_without_matching_export_still_flagged() {
    use crate::extract::ImportedName;

    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/Foo.tsx", false),
            ("/src/other-foo.tsx", false),
            ("/src/wrapper.tsx", false),
        ],
        &[(1, "Foo", false), (2, "Foo", false), (3, "Bar", false)],
        &[(0, 1, "Foo", false), (0, 2, "Foo", false)],
    );

    let mut wrapper = make_resolved_module(3, "/src/wrapper.tsx");
    wrapper.resolved_dynamic_imports =
        vec![dynamic_import_to(1, ImportedName::Named("Foo".to_string()))];

    let resolved_modules = vec![
        make_resolved_module(1, "/src/Foo.tsx"),
        make_resolved_module(2, "/src/other-foo.tsx"),
        wrapper,
    ];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(
        &graph,
        &config,
        &suppressions,
        &FxHashMap::default(),
        &resolved_modules,
    );
    assert_eq!(
        result.len(),
        1,
        "Foo duplication between unrelated modules must be flagged"
    );
    assert_eq!(result[0].export_name, "Foo");
}

#[test]
fn dynamic_import_default_without_default_export_still_flagged() {
    use crate::extract::ImportedName;

    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/source.ts", false),
            ("/src/wrapper.ts", false),
        ],
        &[(1, "helper", false), (2, "helper", false)],
        &[(0, 1, "helper", false), (0, 2, "helper", false)],
    );

    let mut wrapper = make_resolved_module(2, "/src/wrapper.ts");
    wrapper.resolved_dynamic_imports = vec![dynamic_import_to(1, ImportedName::Default)];

    let resolved_modules = vec![make_resolved_module(1, "/src/source.ts"), wrapper];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(
        &graph,
        &config,
        &suppressions,
        &FxHashMap::default(),
        &resolved_modules,
    );
    assert_eq!(
        result.len(),
        1,
        "Default dynamic import without matching Default export must not suppress duplicate-export"
    );
    assert_eq!(result[0].export_name, "helper");
}

#[test]
fn duplicate_without_dynamic_link_still_flagged() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ],
        &[(1, "helper", false), (2, "helper", false)],
        &[(0, 1, "helper", false), (0, 2, "helper", false)],
    );

    let resolved_modules = vec![
        make_resolved_module(1, "/src/a.ts"),
        make_resolved_module(2, "/src/b.ts"),
    ];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(
        &graph,
        &config,
        &suppressions,
        &FxHashMap::default(),
        &resolved_modules,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].export_name, "helper");
}

#[test]
fn dynamic_import_named_mismatched_with_wrapper_export_still_flagged() {
    use crate::extract::ImportedName;

    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/source.ts", false),
            ("/src/wrapper.ts", false),
        ],
        &[(1, "Foo", false), (2, "Foo", false)],
        &[(0, 1, "Foo", false), (0, 2, "Foo", false)],
    );

    let mut wrapper = make_resolved_module(2, "/src/wrapper.ts");
    wrapper.resolved_dynamic_imports =
        vec![dynamic_import_to(1, ImportedName::Named("Bar".to_string()))];

    let resolved_modules = vec![make_resolved_module(1, "/src/source.ts"), wrapper];
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(
        &graph,
        &config,
        &suppressions,
        &FxHashMap::default(),
        &resolved_modules,
    );
    assert_eq!(
        result.len(),
        1,
        "Named dynamic import whose name does not match the wrapper's own export must not suppress the duplicate"
    );
    assert_eq!(result[0].export_name, "Foo");
}

#[test]
fn duplicate_exports_skipped_when_ignore_exports_matches_with_wildcard() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/ui/dialog/index.ts", false),
        ("/src/ui/card/index.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("Root", 10, 30), make_export("Content", 40, 60)];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export("Root", 10, 30), make_export("Content", 40, 60)];
    graph.reverse_deps[1] = vec![FileId(0)];
    graph.reverse_deps[2] = vec![FileId(0)];

    let suppressions = SuppressionContext::empty();
    let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
        file: "**/ui/**".to_owned(),
        exports: vec!["*".to_owned()],
    }]);
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(
        result.is_empty(),
        "wildcard ignoreExports on a glob matching both barrels must clear all duplicate-export groups, got: {result:?}"
    );
}

#[test]
fn duplicate_exports_skipped_when_ignore_exports_lists_specific_names() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/ui/dialog/index.ts", false),
            ("/src/ui/card/index.ts", false),
        ],
        &[
            (1, "Root", false),
            (1, "Helper", false),
            (2, "Root", false),
            (2, "Helper", false),
        ],
        &[
            (0, 1, "Root", false),
            (0, 1, "Helper", false),
            (0, 2, "Root", false),
            (0, 2, "Helper", false),
        ],
    );

    let suppressions = SuppressionContext::empty();
    let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
        file: "**/ui/**".to_owned(),
        exports: vec!["Root".to_owned()],
    }]);
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    let names: Vec<_> = result.iter().map(|d| d.export_name.as_str()).collect();
    assert!(
        !names.contains(&"Root"),
        "Root listed in ignoreExports must not surface, got: {names:?}"
    );
    assert!(
        names.contains(&"Helper"),
        "Helper not in the ignore list must still surface, got: {names:?}"
    );
}

fn test_config_with_ignore_exports(rules: Vec<fallow_config::IgnoreExportRule>) -> ResolvedConfig {
    fallow_config::FallowConfig {
        ignore_exports: rules,
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        ..Default::default()
    }
    .resolve(
        PathBuf::from("/tmp/test"),
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    )
}

fn make_plugin_result(
    used_exports: Vec<(String, Vec<String>)>,
) -> crate::plugins::AggregatedPluginResult {
    crate::plugins::AggregatedPluginResult {
        used_exports: used_exports
            .into_iter()
            .map(|(pattern, exports)| {
                crate::plugins::PluginUsedExportRule::new(
                    "test-plugin",
                    crate::plugins::UsedExportRule::new(pattern, exports),
                )
            })
            .collect(),
        ..Default::default()
    }
}

fn make_type_export(name: &str, span_start: u32, span_end: u32) -> ExportSymbol {
    ExportSymbol {
        name: ExportName::Named(name.to_string()),
        is_type_only: true,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(span_start, span_end),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }
}

#[test]
fn unused_exports_empty_graph() {
    let graph = build_graph(&[]);
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(exports.is_empty());
    assert!(types.is_empty());
}

#[test]
fn unused_exports_detects_unreferenced_export() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("helper", 10, 20)];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].export_name, "helper");
    assert!(types.is_empty());
}

#[test]
fn unused_exports_skips_referenced_export() {
    let graph = build_graph_with_referenced_export(
        &[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ],
        1,
        "helper",
        10,
        20,
        0,
    );
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(exports.is_empty());
    assert!(types.is_empty());
}

#[test]
fn unused_exports_skips_public_export() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![ExportSymbol {
        name: ExportName::Named("publicFn".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::Public,
        expected_unused_reason: None,
        span: Span::new(10, 20),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(exports.is_empty());
    assert!(types.is_empty());
}

#[test]
fn unused_exports_separates_types_from_values() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![
        make_export("valueFn", 10, 20),
        make_type_export("MyType", 30, 40),
    ];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].export_name, "valueFn");
    assert_eq!(types.len(), 1);
    assert_eq!(types[0].export_name, "MyType");
}

#[test]
fn unused_exports_skips_unreachable_module() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/dead.ts", false),
    ]);
    graph.modules[1].exports = vec![make_export("orphan", 10, 20)];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(exports.is_empty());
    assert!(types.is_empty());
}

#[test]
fn unused_exports_skips_entry_point() {
    let mut graph = build_graph(&[("/tmp/test/src/entry.ts", true)]);
    graph.modules[0].exports = vec![make_export("main", 10, 20)];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(exports.is_empty());
    assert!(types.is_empty());
}

#[test]
fn unused_exports_reports_non_framework_exports_in_entry_point_with_plugin_rules() {
    let mut graph = build_graph(&[("/tmp/test/src/app/page.tsx", true)]);
    graph.modules[0].set_reachable(true);
    graph.modules[0].exports = vec![
        make_export("default", 10, 20),
        make_export("generateMetadata", 30, 40),
        make_export("helper", 50, 60),
    ];

    let plugin = make_plugin_result(vec![(
        "src/app/**/page.{ts,tsx,js,jsx}".to_string(),
        vec!["default".to_string(), "generateMetadata".to_string()],
    )]);
    let config = test_config();
    let suppressions = SuppressionContext::empty();

    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        Some(&plugin),
        &suppressions,
        &FxHashMap::default(),
    );

    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].export_name, "helper");
    assert!(types.is_empty());
}

#[test]
fn unused_exports_skips_cjs_only_module() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/legacy.js", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].set_cjs_exports(true);
    graph.modules[1].exports = vec![];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(exports.is_empty());
    assert!(types.is_empty());
}

#[test]
fn unused_exports_does_not_skip_cjs_module_with_named_exports() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/mixed.js", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].set_cjs_exports(true);
    graph.modules[1].exports = vec![make_export("namedFn", 10, 20)];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].export_name, "namedFn");
}

#[test]
fn unused_exports_skips_svelte_files() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/Component.svelte", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("count", 10, 20)];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(exports.is_empty());
    assert!(types.is_empty());
}

#[test]
fn unused_exports_reports_reachable_non_entry_non_cjs_non_svelte() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].set_cjs_exports(false);
    graph.modules[1].exports = vec![make_export("helper", 10, 20)];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].export_name, "helper");
}

#[test]
fn unused_exports_empty_ignore_config() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("foo", 10, 20)];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(
        exports.len(),
        1,
        "no ignore rules, export should be reported"
    );
}

#[test]
fn unused_exports_ignore_multiple_patterns() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/types.ts", false),
        ("/tmp/test/src/constants.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("MyType", 10, 20)];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export("MY_CONST", 10, 20)];

    let config = test_config_with_ignore_exports(vec![
        fallow_config::IgnoreExportRule {
            file: "src/types.ts".to_string(),
            exports: vec!["*".to_string()],
        },
        fallow_config::IgnoreExportRule {
            file: "src/constants.ts".to_string(),
            exports: vec!["MY_CONST".to_string()],
        },
    ]);
    let suppressions = SuppressionContext::empty();
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(
        exports.is_empty(),
        "both exports should be ignored by config rules"
    );
}

#[test]
#[should_panic(expected = "validated at config load time")]
fn unused_exports_panics_on_unvalidated_invalid_ignore_glob() {
    let _ = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
        file: "[invalid".to_string(),
        exports: vec!["*".to_string()],
    }]);
}

#[test]
fn unused_exports_ignore_wildcard_matches_all() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/types.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("TypeA", 10, 20), make_export("TypeB", 30, 40)];

    let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
        file: "src/types.ts".to_string(),
        exports: vec!["*".to_string()],
    }]);
    let suppressions = SuppressionContext::empty();
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(
        exports.is_empty(),
        "wildcard * should ignore all exports in matching file"
    );
}

#[test]
fn unused_exports_ignore_specific_name_only() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![
        make_export("ignored", 10, 20),
        make_export("reported", 30, 40),
    ];

    let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
        file: "src/utils.ts".to_string(),
        exports: vec!["ignored".to_string()],
    }]);
    let suppressions = SuppressionContext::empty();
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].export_name, "reported");
}

#[test]
fn unused_exports_ignore_rule_wrong_file_no_effect() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("foo", 10, 20)];

    let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
        file: "src/other.ts".to_string(),
        exports: vec!["*".to_string()],
    }]);
    let suppressions = SuppressionContext::empty();
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(
        exports.len(),
        1,
        "ignore rule for different file should not suppress"
    );
}

#[test]
fn unused_exports_no_plugin_result() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("foo", 10, 20)];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(
        exports.len(),
        1,
        "None plugin_result means no plugin matchers"
    );
}

#[test]
fn unused_exports_plugin_no_used_exports() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("foo", 10, 20)];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let pr = make_plugin_result(vec![]);
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        Some(&pr),
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(
        exports.len(),
        1,
        "plugin with no used_exports should not suppress"
    );
}

#[test]
fn unused_exports_plugin_used_exports_suppresses() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/pages/index.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![
        make_export("getStaticProps", 10, 20),
        make_export("unusedHelper", 30, 40),
    ];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let pr = make_plugin_result(vec![(
        "src/pages/**".to_string(),
        vec!["getStaticProps".to_string()],
    )]);
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        Some(&pr),
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].export_name, "unusedHelper");
}

#[test]
fn unused_exports_both_config_and_plugin_ignore() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/api/handler.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("handler", 10, 20)];

    let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
        file: "src/api/*.ts".to_string(),
        exports: vec!["handler".to_string()],
    }]);
    let suppressions = SuppressionContext::empty();
    let pr = make_plugin_result(vec![(
        "src/api/**".to_string(),
        vec!["handler".to_string()],
    )]);
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        Some(&pr),
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(
        exports.is_empty(),
        "export matching both config and plugin should be ignored"
    );
}

#[test]
fn unused_exports_invalid_plugin_glob_skipped() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("foo", 10, 20)];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let pr = make_plugin_result(vec![("[invalid".to_string(), vec!["foo".to_string()])]);
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        Some(&pr),
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(exports.len(), 1, "invalid plugin glob should be skipped");
}

#[test]
fn unused_exports_marks_re_export_semantically() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/barrel.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export("reexported", 100, 120)];
    graph.modules[1].re_exports = vec![ReExportEdge {
        source_file: FileId(0),
        imported_name: "reexported".to_string(),
        exported_name: "reexported".to_string(),
        is_type_only: false,
        span: oxc_span::Span::default(),
    }];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(exports.len(), 1);
    assert!(
        exports[0].is_re_export,
        "export with matching ReExportEdge should be flagged as re-export"
    );
    assert_eq!(exports[0].span_start, 100);
}

#[test]
fn collect_usages_empty_graph() {
    let graph = build_graph(&[]);
    let result = collect_export_usages(&graph, &FxHashMap::default());
    assert!(result.is_empty());
}

#[test]
fn collect_usages_skips_unreachable_modules() {
    let mut graph = build_graph(&[("/src/dead.ts", false)]);
    graph.modules[0].exports = vec![make_export("unused", 10, 20)];
    let result = collect_export_usages(&graph, &FxHashMap::default());
    assert!(result.is_empty());
}

#[test]
fn collect_usages_skips_synthetic_exports() {
    let mut graph = build_graph(&[("/src/barrel.ts", true)]);
    graph.modules[0].exports = vec![make_export("reexported", 0, 0)];
    let result = collect_export_usages(&graph, &FxHashMap::default());
    assert!(result.is_empty());
}

#[test]
fn collect_usages_counts_references() {
    let graph = build_graph_with_referenced_export(
        &[("/src/utils.ts", true), ("/src/app.ts", false)],
        0,
        "helper",
        10,
        20,
        1,
    );
    let result = collect_export_usages(&graph, &FxHashMap::default());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].export_name, "helper");
    assert_eq!(result[0].reference_count, 1);
}

#[test]
fn collect_usages_collapses_namespaces_at_one_physical_site() {
    let mut graph = build_graph(&[("/src/utils.ts", true), ("/src/app.ts", false)]);
    let mut export = make_export("helper", 10, 20);
    export.references = [ExportNamespace::Type, ExportNamespace::Value]
        .into_iter()
        .map(|namespace| SymbolReference {
            from_file: FileId(1),
            kind: ReferenceKind::NamedImport,
            namespace,
            import_span: Span::new(0, 10),
        })
        .collect();
    graph.modules[0].exports = vec![export];

    let result = collect_export_usages(&graph, &FxHashMap::default());
    assert_eq!(result[0].reference_count, 1);
    assert_eq!(result[0].reference_locations.len(), 1);
}

fn named_import_from(file: u32, start: u32, end: u32) -> SymbolReference {
    SymbolReference {
        from_file: FileId(file),
        kind: ReferenceKind::NamedImport,
        namespace: ExportNamespace::Value,
        import_span: Span::new(start, end),
    }
}

#[test]
fn collect_usages_counts_two_import_sites_in_one_file_as_one_file() {
    let mut graph = build_graph(&[("/src/utils.ts", true), ("/src/app.ts", false)]);
    let mut export = make_export("helper", 10, 20);
    export.references = vec![named_import_from(1, 0, 10), named_import_from(1, 20, 30)];
    graph.modules[0].exports = vec![export];

    let result = collect_export_usages(&graph, &FxHashMap::default());
    assert_eq!(result[0].reference_count, 1);
    assert_eq!(result[0].reference_locations.len(), 2);
}

#[test]
fn collect_usages_counts_distinct_importing_files() {
    let mut graph = build_graph(&[
        ("/src/utils.ts", true),
        ("/src/app.ts", false),
        ("/src/page.ts", false),
    ]);
    let mut export = make_export("helper", 10, 20);
    export.references = vec![
        named_import_from(1, 0, 10),
        named_import_from(2, 0, 10),
        named_import_from(1, 20, 30),
    ];
    graph.modules[0].exports = vec![export];

    let result = collect_export_usages(&graph, &FxHashMap::default());
    assert_eq!(result[0].reference_count, 2);
}

#[test]
fn collect_usages_counts_file_whose_import_has_no_span() {
    let mut graph = build_graph(&[("/src/utils.ts", true), ("/src/app.ts", false)]);
    let mut export = make_export("helper", 10, 20);
    export.references = vec![named_import_from(1, 0, 0)];
    graph.modules[0].exports = vec![export];

    let result = collect_export_usages(&graph, &FxHashMap::default());
    assert_eq!(result[0].reference_count, 1);
    assert!(result[0].reference_locations.is_empty());
}

#[test]
fn collect_usages_zero_references_still_reported() {
    let mut graph = build_graph(&[("/src/utils.ts", true)]);
    graph.modules[0].exports = vec![make_export("unused", 10, 20)];
    let result = collect_export_usages(&graph, &FxHashMap::default());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].export_name, "unused");
    assert_eq!(result[0].reference_count, 0);
    assert!(result[0].reference_locations.is_empty());
}

#[test]
fn collect_usages_multiple_exports_same_module() {
    let mut graph = build_graph(&[("/src/utils.ts", true)]);
    graph.modules[0].exports = vec![make_export("alpha", 10, 20), make_export("beta", 30, 40)];
    let result = collect_export_usages(&graph, &FxHashMap::default());
    assert_eq!(result.len(), 2);
    let names: FxHashSet<&str> = result.iter().map(|u| u.export_name.as_str()).collect();
    assert!(names.contains("alpha"));
    assert!(names.contains("beta"));
}

#[test]
fn unused_exports_checks_unreachable_module_with_mixed_references() {
    let mut graph = build_graph_with_referenced_export(
        &[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/helpers.ts", false),
            ("/tmp/test/src/setup.ts", false),
        ],
        1,
        "usedByUnreachable",
        10,
        30,
        2,
    );
    graph.modules[1]
        .exports
        .push(make_export("totallyUnused", 40, 55));

    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    let names: FxHashSet<&str> = exports.iter().map(|e| e.export_name.as_str()).collect();
    assert!(
        names.contains("usedByUnreachable"),
        "reference from unreachable module should not save an export"
    );
    assert!(
        names.contains("totallyUnused"),
        "completely unreferenced export should be flagged"
    );
    assert_eq!(exports.len(), 2);
    assert!(types.is_empty());
}

#[test]
fn unused_exports_skips_export_referenced_by_reachable() {
    let graph = build_graph_with_referenced_export(
        &[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/helpers.ts", false),
        ],
        1,
        "usedByReachable",
        10,
        28,
        0,
    );

    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(
        exports.is_empty(),
        "export referenced by reachable module should not be flagged"
    );
    assert!(types.is_empty());
}

#[test]
fn unused_exports_skips_internal_visibility() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![ExportSymbol {
        name: ExportName::Named("internalHelper".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::Internal,
        expected_unused_reason: None,
        span: Span::new(10, 30),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(
        exports.is_empty(),
        "@internal export should not be flagged as unused"
    );
    assert!(types.is_empty());
}

#[test]
fn unused_exports_skips_beta_visibility() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![ExportSymbol {
        name: ExportName::Named("betaFeature".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::Beta,
        expected_unused_reason: None,
        span: Span::new(10, 30),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(
        exports.is_empty(),
        "@beta export should not be flagged as unused"
    );
    assert!(types.is_empty());
}

#[test]
fn unused_exports_skips_alpha_visibility() {
    let mut graph = build_graph(&[
        ("/tmp/test/src/entry.ts", true),
        ("/tmp/test/src/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![ExportSymbol {
        name: ExportName::Named("alphaFeature".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::Alpha,
        expected_unused_reason: None,
        span: Span::new(10, 30),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }];
    let config = test_config();
    let suppressions = SuppressionContext::empty();
    let (exports, types, _stale) = find_unused_exports(
        &graph,
        &[],
        &config,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(
        exports.is_empty(),
        "@alpha export should not be flagged as unused"
    );
    assert!(types.is_empty());
}

#[test]
fn unused_exports_include_entry_exports_flag() {
    let mut graph = build_graph(&[("/tmp/test/src/entry.ts", true)]);
    graph.modules[0].exports = vec![make_export("main", 10, 20)];

    let config_off = test_config();
    assert!(!config_off.include_entry_exports);
    let suppressions = SuppressionContext::empty();
    let (exports_off, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config_off,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert!(
        exports_off.is_empty(),
        "entry export should be skipped when include_entry_exports is false"
    );

    let mut config_on = test_config();
    config_on.include_entry_exports = true;
    let (exports_on, _, _stale) = find_unused_exports(
        &graph,
        &[],
        &config_on,
        None,
        &suppressions,
        &FxHashMap::default(),
    );
    assert_eq!(
        exports_on.len(),
        1,
        "entry export should be flagged when include_entry_exports is true"
    );
    assert_eq!(exports_on[0].export_name, "main");
}

fn make_export_entry(module_idx: usize, file_id: u32) -> ExportEntry {
    ExportEntry {
        module_idx,
        path: PathBuf::from(format!("f{file_id}.ts")),
        file_id: FileId(file_id),
        span_start: 0,
        is_type_only: false,
    }
}

/// Reference implementation of the direct-import union path using the
/// original O(n^2) `entries.iter().position(...)` linear scan, so the
/// O(1) `file_id -> first index` map in `partition_by_common_importer`
/// (issue #1843 follow-up) can be proven to produce identical grouping.
fn reference_partition(
    entries: &[ExportEntry],
    export_name: &str,
    graph: &ModuleGraph,
) -> Vec<Vec<usize>> {
    let mut union_find = UnionFind::new(entries.len());

    let mut importer_to_entries: FxHashMap<FileId, Vec<usize>> = FxHashMap::default();
    for (i, entry) in entries.iter().enumerate() {
        let idx = entry.file_id.0 as usize;
        if idx >= graph.reverse_deps.len() {
            continue;
        }
        for &importer in &graph.reverse_deps[idx] {
            if !entry.connects_importer(graph, importer, export_name) {
                continue;
            }
            importer_to_entries.entry(importer).or_default().push(i);
        }
    }
    for members in importer_to_entries.values() {
        if let Some((&first, rest)) = members.split_first() {
            for &other in rest {
                union_find.union(first, other);
            }
        }
    }

    let entry_file_ids: FxHashSet<FileId> = entries.iter().map(|e| e.file_id).collect();
    for (i, entry) in entries.iter().enumerate() {
        let idx = entry.file_id.0 as usize;
        if idx >= graph.reverse_deps.len() {
            continue;
        }
        for &importer in &graph.reverse_deps[idx] {
            if !entry.connects_importer(graph, importer, export_name) {
                continue;
            }
            if entry_file_ids.contains(&importer)
                && let Some(j) = entries.iter().position(|e| e.file_id == importer)
            {
                union_find.union(i, j);
            }
        }
    }

    union_find.components()
}

fn normalize_components(mut components: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    for comp in &mut components {
        comp.sort_unstable();
    }
    components.sort_unstable();
    components
}

#[test]
fn partition_direct_import_index_matches_position_scan() {
    let files: Vec<_> = (0..4)
        .map(|file_id| DiscoveredFile {
            id: FileId(file_id),
            path: PathBuf::from(format!("f{file_id}.ts")),
            size_bytes: 0,
        })
        .collect();
    let named_import = |target: FileId| ResolvedImport {
        info: ImportInfo {
            source: format!("./f{}", target.0),
            imported_name: ImportedName::Named("shared".to_string()),
            local_name: "shared".to_string(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: Span::default(),
            source_span: Span::default(),
        },
        target: ResolveResult::InternalModule(target),
    };
    let resolved_modules: Vec<_> = files
        .iter()
        .map(|file| ResolvedModule {
            file_id: file.id,
            path: file.path.clone(),
            resolved_imports: match file.id.0 {
                1 => vec![named_import(FileId(0)), named_import(FileId(2))],
                3 => vec![named_import(FileId(0))],
                _ => Vec::new(),
            },
            ..Default::default()
        })
        .collect();
    let graph = ModuleGraph::build(&resolved_modules, &[], &files);

    // Five entries with a DUPLICATE file_id (indices 1 and 2 both in file1),
    // exercising the first-index-wins semantics the map must preserve.
    let entries = vec![
        make_export_entry(0, 0),
        make_export_entry(1, 1),
        make_export_entry(2, 1),
        make_export_entry(3, 2),
        make_export_entry(4, 3),
    ];

    let actual = normalize_components(partition_by_common_importer(&entries, "shared", &graph));
    let reference = normalize_components(reference_partition(&entries, "shared", &graph));
    assert_eq!(
        actual, reference,
        "map-based partition must match the position()-scan reference"
    );

    // Hand-computed grouping: {0,1,3,4} merged via the shared/direct-import
    // unions; the duplicate-file_id entry at index 2 stays a singleton
    // because position()/the first-index map never resolve to it.
    assert_eq!(actual, vec![vec![0, 1, 3, 4], vec![2]]);
}
