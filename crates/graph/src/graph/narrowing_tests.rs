use super::*;
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{
    ExportInfo, ExportName, ImportInfo, ModuleLoadMechanism, VisibilityTag,
};

use super::super::ModuleGraph;
use super::super::effective_exports::EffectiveExportIndex;

const TARGET: FileId = FileId(9);

/// Build the effective export index for a target module that declares
/// `exports` in the same slot order.
fn target_export_index(exports: &[ExportSymbol]) -> EffectiveExportIndex {
    let infos: Vec<ExportInfo> = exports
        .iter()
        .map(|export| ExportInfo {
            name: export.name.clone(),
            local_name: Some(export.name.to_string()),
            is_type_only: export.is_type_only,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: export.span,
            members: Vec::new(),
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        })
        .collect();
    EffectiveExportIndex::build(&[ResolvedModule {
        file_id: TARGET,
        exports: infos.into(),
        ..Default::default()
    }])
}

fn namespace_site(source_id: FileId, reference_paths: &mut ReferencePathInterner) -> ReferenceSite {
    ReferenceSite::exact(
        source_id,
        oxc_span::Span::new(0, 10),
        reference_paths.direct(TARGET, ModuleLoadMechanism::EsModule),
    )
}

/// Run the production whole-namespace marker for a value namespace import.
fn mark_all(
    exports: &mut [ExportSymbol],
    source_id: FileId,
    reference_paths: &mut ReferencePathInterner,
) {
    let index = target_export_index(exports);
    let mut dedup = ReferenceDedup::default();
    let mut context = NamespaceMarkContext {
        module_id: TARGET,
        site: namespace_site(source_id, reference_paths),
        kind: ReferenceKind::NamespaceImport,
        namespace: ExportNamespace::Value,
        effective_exports: &index,
        dedup: &mut dedup,
    };
    mark_all_exports_referenced_at_site(exports, &mut context);
}

/// Run the production member marker for a value namespace import.
fn mark_members(
    exports: &mut [ExportSymbol],
    source_id: FileId,
    accessed: &[String],
    reference_paths: &mut ReferencePathInterner,
) -> FxHashSet<String> {
    let index = target_export_index(exports);
    let mut dedup = ReferenceDedup::default();
    let mut context = NamespaceMarkContext {
        module_id: TARGET,
        site: namespace_site(source_id, reference_paths),
        kind: ReferenceKind::NamespaceImport,
        namespace: ExportNamespace::Value,
        effective_exports: &index,
        dedup: &mut dedup,
    };
    mark_member_exports_referenced_at_site(exports, accessed, &mut context)
}

fn create_synthetic(
    exports: &mut Vec<ExportSymbol>,
    re_exports: &[ReExportEdge],
    accessed: &[String],
    found: &FxHashSet<String>,
) {
    create_synthetic_exports_for_star_re_exports_at_site(
        exports,
        re_exports,
        ReferenceSite::exact(FileId(0), oxc_span::Span::new(0, 10), None),
        accessed,
        found,
        ExportNamespace::Value,
    );
}

#[test]
fn is_unused_binding_true() {
    let resolved = ResolvedModule {
        path: std::path::PathBuf::from("/project/entry.ts"),
        unused_import_bindings: FxHashSet::from_iter(["unusedVar".to_string()]),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        ..Default::default()
    };
    assert!(is_unused_import_binding(
        "unusedVar",
        &ImportedName::Named("x".to_string()),
        Some(&&resolved),
    ));
}

#[test]
fn is_unused_binding_false_when_used() {
    let resolved = ResolvedModule {
        path: std::path::PathBuf::from("/project/entry.ts"),
        unused_import_bindings: FxHashSet::from_iter(["otherVar".to_string()]),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        ..Default::default()
    };
    assert!(!is_unused_import_binding(
        "usedVar",
        &ImportedName::Named("x".to_string()),
        Some(&&resolved),
    ));
}

#[test]
fn is_unused_binding_false_for_side_effect() {
    let resolved = ResolvedModule {
        path: std::path::PathBuf::from("/project/entry.ts"),
        unused_import_bindings: FxHashSet::from_iter(["x".to_string()]),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        ..Default::default()
    };
    assert!(!is_unused_import_binding(
        "x",
        &ImportedName::SideEffect,
        Some(&&resolved),
    ));
}

#[test]
fn is_unused_binding_false_for_empty_local_name() {
    let resolved = ResolvedModule {
        path: std::path::PathBuf::from("/project/entry.ts"),
        ..Default::default()
    };
    assert!(!is_unused_import_binding(
        "",
        &ImportedName::Named("x".to_string()),
        Some(&&resolved),
    ));
}

#[test]
fn is_unused_binding_false_for_no_source_module() {
    assert!(!is_unused_import_binding(
        "x",
        &ImportedName::Named("x".to_string()),
        None,
    ));
}

#[test]
fn extract_accessed_members_found() {
    let resolved = ResolvedModule {
        path: std::path::PathBuf::from("/project/entry.ts"),
        member_accesses: vec![
            fallow_types::extract::MemberAccess {
                object: "ns".to_string(),
                member: "foo".to_string(),
            },
            fallow_types::extract::MemberAccess {
                object: "ns".to_string(),
                member: "bar".to_string(),
            },
            fallow_types::extract::MemberAccess {
                object: "other".to_string(),
                member: "baz".to_string(),
            },
        ]
        .into(),
        ..Default::default()
    };
    let members = extract_accessed_members(Some(&&resolved), "ns");
    assert_eq!(members, vec!["foo".to_string(), "bar".to_string()]);
}

#[test]
fn extract_accessed_members_none_module() {
    let members = extract_accessed_members(None, "ns");
    assert!(members.is_empty());
}

#[test]
fn mark_all_exports_referenced_adds_refs() {
    let mut reference_paths = ReferencePathInterner::default();
    let mut exports = vec![
        ExportSymbol {
            name: ExportName::Named("a".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 5),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
            deprecated: false,
            deprecated_reason: None,
        },
        ExportSymbol {
            name: ExportName::Named("b".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(10, 15),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
            deprecated: false,
            deprecated_reason: None,
        },
    ];
    mark_all(&mut exports, FileId(5), &mut reference_paths);
    assert_eq!(exports[0].references.len(), 1);
    assert_eq!(exports[0].references[0].from_file, FileId(5));
    assert_eq!(exports[1].references.len(), 1);
}

#[test]
fn mark_all_exports_referenced_deduplicates() {
    let mut reference_paths = ReferencePathInterner::default();
    let mut exports = vec![ExportSymbol {
        name: ExportName::Named("a".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 5),
        references: vec![SymbolReference {
            from_file: FileId(5),
            kind: ReferenceKind::NamedImport,
            namespace: ExportNamespace::Value,
            import_span: oxc_span::Span::new(0, 10),
            decorator_metadata: None,
        }],
        reference_paths: vec![reference_paths.direct(TARGET, ModuleLoadMechanism::EsModule)],
        members: Vec::new(),
        deprecated: false,
        deprecated_reason: None,
    }];
    mark_all(&mut exports, FileId(5), &mut reference_paths);
    assert_eq!(exports[0].references.len(), 1);
}

#[test]
fn untracked_reference_sites_preserve_distinct_import_spans() {
    let mut export = ExportSymbol {
        name: ExportName::Named("a".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 5),
        references: Vec::new(),
        reference_paths: Vec::new(),
        members: Vec::new(),
        deprecated: false,
        deprecated_reason: None,
    };

    let mut dedup = ReferenceDedup::default();
    attach_reference(
        &mut export,
        (FileId(9), 0),
        ReferenceSite::exact(FileId(5), oxc_span::Span::new(0, 10), None),
        ReferenceKind::NamedImport,
        ExportNamespace::Value,
        &mut dedup,
    );
    attach_reference(
        &mut export,
        (FileId(9), 0),
        ReferenceSite::exact(FileId(5), oxc_span::Span::new(20, 30), None),
        ReferenceKind::NamespaceImport,
        ExportNamespace::Value,
        &mut dedup,
    );

    assert_eq!(export.references.len(), 2);
    assert_eq!(export.references[0].kind, ReferenceKind::NamedImport);
    assert_eq!(export.references[0].import_span, oxc_span::Span::new(0, 10));
    assert_eq!(export.references[1].kind, ReferenceKind::NamespaceImport);
    assert_eq!(
        export.references[1].import_span,
        oxc_span::Span::new(20, 30)
    );
}

#[test]
fn attach_reference_dedup_matches_linear_scan_across_threshold() {
    let mut export = ExportSymbol {
        name: ExportName::Named("hot".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 5),
        references: Vec::new(),
        reference_paths: Vec::new(),
        members: Vec::new(),
        deprecated: false,
        deprecated_reason: None,
    };
    let mut dedup = ReferenceDedup::default();
    let total = u32::try_from(REFERENCE_DEDUP_THRESHOLD * 3).unwrap_or(u32::MAX);
    for _ in 0..2 {
        for consumer in 0..total {
            attach_reference(
                &mut export,
                (FileId(9), 0),
                ReferenceSite::exact(FileId(consumer), oxc_span::Span::new(0, 10), None),
                ReferenceKind::NamedImport,
                ExportNamespace::Value,
                &mut dedup,
            );
        }
    }

    assert_eq!(export.references.len(), total as usize);
    for (consumer, reference) in export.references.iter().enumerate() {
        assert_eq!(reference.from_file.0 as usize, consumer);
    }
}

#[test]
fn attach_reference_dedup_seeds_from_existing_references() {
    let mut export = ExportSymbol {
        name: ExportName::Named("hot".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 5),
        references: (0..u32::try_from(REFERENCE_DEDUP_THRESHOLD).unwrap_or(u32::MAX))
            .map(|consumer| SymbolReference {
                from_file: FileId(consumer),
                kind: ReferenceKind::NamedImport,
                namespace: ExportNamespace::Value,
                import_span: oxc_span::Span::new(0, 10),
                decorator_metadata: None,
            })
            .collect(),
        reference_paths: Vec::new(),
        members: Vec::new(),
        deprecated: false,
        deprecated_reason: None,
    };

    let mut dedup = ReferenceDedup::default();
    attach_reference(
        &mut export,
        (FileId(9), 0),
        ReferenceSite::exact(FileId(0), oxc_span::Span::new(0, 10), None),
        ReferenceKind::NamedImport,
        ExportNamespace::Value,
        &mut dedup,
    );
    assert_eq!(export.references.len(), REFERENCE_DEDUP_THRESHOLD);

    attach_reference(
        &mut export,
        (FileId(9), 0),
        ReferenceSite::exact(FileId(999), oxc_span::Span::new(0, 10), None),
        ReferenceKind::NamedImport,
        ExportNamespace::Value,
        &mut dedup,
    );
    assert_eq!(export.references.len(), REFERENCE_DEDUP_THRESHOLD + 1);
}

#[test]
fn mark_member_exports_referenced_only_accessed() {
    let mut reference_paths = ReferencePathInterner::default();
    let mut exports = vec![
        ExportSymbol {
            name: ExportName::Named("foo".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 5),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
            deprecated: false,
            deprecated_reason: None,
        },
        ExportSymbol {
            name: ExportName::Named("bar".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(10, 15),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
            deprecated: false,
            deprecated_reason: None,
        },
    ];
    let accessed = vec!["foo".to_string()];
    let found = mark_members(&mut exports, FileId(0), &accessed, &mut reference_paths);

    assert_eq!(exports[0].references.len(), 1);
    assert!(exports[1].references.is_empty());
    assert!(found.contains("foo"));
    assert!(!found.contains("bar"));
}

#[test]
fn create_synthetic_exports_with_star_re_export() {
    let mut exports = vec![ExportSymbol {
        name: ExportName::Named("existing".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 5),
        references: Vec::new(),
        reference_paths: Vec::new(),
        members: Vec::new(),
        deprecated: false,
        deprecated_reason: None,
    }];
    let re_exports = vec![ReExportEdge {
        source_file: FileId(2),
        imported_name: "*".to_string(),
        exported_name: "*".to_string(),
        is_type_only: false,
        span: oxc_span::Span::default(),
    }];
    let accessed = vec!["missing".to_string()];
    let found = FxHashSet::default(); // nothing found among own exports

    create_synthetic(&mut exports, &re_exports, &accessed, &found);

    assert_eq!(exports.len(), 2);
    assert_eq!(exports[1].name, ExportName::Named("missing".to_string()));
    assert_eq!(exports[1].references.len(), 1);
}

#[test]
fn create_synthetic_exports_skips_already_found() {
    let mut exports = Vec::new();
    let re_exports = vec![ReExportEdge {
        source_file: FileId(2),
        imported_name: "*".to_string(),
        exported_name: "*".to_string(),
        is_type_only: false,
        span: oxc_span::Span::default(),
    }];
    let accessed = vec!["already".to_string()];
    let mut found = FxHashSet::default();
    found.insert("already".to_string());

    create_synthetic(&mut exports, &re_exports, &accessed, &found);

    assert!(
        exports.is_empty(),
        "should not create synthetic for already-found members"
    );
}

#[test]
fn create_synthetic_exports_no_star_re_exports() {
    let mut exports = Vec::new();
    let re_exports = vec![ReExportEdge {
        source_file: FileId(2),
        imported_name: "foo".to_string(),
        exported_name: "foo".to_string(),
        is_type_only: false,
        span: oxc_span::Span::default(),
    }];
    let accessed = vec!["missing".to_string()];
    let found = FxHashSet::default();

    create_synthetic(&mut exports, &re_exports, &accessed, &found);

    assert!(
        exports.is_empty(),
        "should not create synthetic without star re-exports"
    );
}

#[test]
fn reference_kind_for_named() {
    assert_eq!(
        reference_kind_for(&ImportedName::Named("x".to_string())),
        ReferenceKind::NamedImport,
    );
}

#[test]
fn reference_kind_for_default() {
    assert_eq!(
        reference_kind_for(&ImportedName::Default),
        ReferenceKind::DefaultImport,
    );
}

#[test]
fn reference_kind_for_namespace() {
    assert_eq!(
        reference_kind_for(&ImportedName::Namespace),
        ReferenceKind::NamespaceImport,
    );
}

#[test]
fn reference_kind_for_side_effect() {
    assert_eq!(
        reference_kind_for(&ImportedName::SideEffect),
        ReferenceKind::SideEffectImport,
    );
}

#[test]
fn attach_ref_skips_unused_binding() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: std::path::PathBuf::from("/project/utils.ts"),
            size_bytes: 50,
        },
    ];
    let entry_points = vec![fallow_types::discover::EntryPoint {
        path: std::path::PathBuf::from("/project/entry.ts"),
        source: fallow_types::discover::EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: fallow_types::extract::ImportInfo {
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
            unused_import_bindings: FxHashSet::from_iter(["foo".to_string()]),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: std::path::PathBuf::from("/project/utils.ts"),
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
    let foo_export = graph.modules[1]
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .unwrap();
    assert!(
        foo_export.references.is_empty(),
        "unused binding should not create a reference"
    );
}

#[test]
fn attach_ref_namespace_narrows_to_member_accesses() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: std::path::PathBuf::from("/project/utils.ts"),
            size_bytes: 50,
        },
    ];
    let entry_points = vec![fallow_types::discover::EntryPoint {
        path: std::path::PathBuf::from("/project/entry.ts"),
        source: fallow_types::discover::EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: fallow_types::extract::ImportInfo {
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
            member_accesses: vec![fallow_types::extract::MemberAccess {
                object: "utils".to_string(),
                member: "foo".to_string(),
            }]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: std::path::PathBuf::from("/project/utils.ts"),
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
    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let foo_export = graph.modules[1]
        .exports
        .iter()
        .find(|e| e.name.to_string() == "foo")
        .unwrap();
    assert!(
        !foo_export.references.is_empty(),
        "foo should be referenced via namespace narrowing"
    );

    let bar_export = graph.modules[1]
        .exports
        .iter()
        .find(|e| e.name.to_string() == "bar")
        .unwrap();
    assert!(
        bar_export.references.is_empty(),
        "bar should not be referenced when only foo is accessed"
    );
}

#[test]
fn attach_ref_namespace_whole_object_marks_all() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: std::path::PathBuf::from("/project/utils.ts"),
            size_bytes: 50,
        },
    ];
    let entry_points = vec![fallow_types::discover::EntryPoint {
        path: std::path::PathBuf::from("/project/entry.ts"),
        source: fallow_types::discover::EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: fallow_types::extract::ImportInfo {
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
            whole_object_uses: vec!["utils".to_string()].into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: std::path::PathBuf::from("/project/utils.ts"),
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
    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    for export in &graph.modules[1].exports {
        assert!(
            !export.references.is_empty(),
            "{} should be referenced when namespace is used as whole object",
            export.name
        );
    }
}

/// A type-only import with no local binding: the shape of a re-export
/// inside a `declare module '...'` body (issue #2357).
fn unbound_type_only_symbol(imported_name: ImportedName) -> ImportedSymbol {
    ImportedSymbol {
        imported_name,
        local_name: String::new(),
        import_span: oxc_span::Span::new(0, 10),
        is_type_only: true,
        is_type_only_star: false,
        mechanism: ModuleLoadMechanism::EsModule,
        load_kind: fallow_types::extract::ImportLoadKind::Static,
        binding_use: crate::graph::import_elision::BindingUse::Value,
    }
}

#[test]
fn desired_import_namespaces_credits_both_lanes_only_for_the_ambient_star() {
    for imported_name in [ImportedName::Namespace, ImportedName::Default] {
        let star = unbound_type_only_symbol(imported_name.clone());
        assert_eq!(
            desired_import_namespaces(&star, None).namespaces(),
            (true, true),
            "the ambient star surface ({imported_name:?}) forwards both meanings"
        );
    }

    let named = unbound_type_only_symbol(ImportedName::Named("Foo".to_string()));
    assert_eq!(
        desired_import_namespaces(&named, None).namespaces(),
        (true, false),
        "an ambient named re-export or an `import()` type reference stays in type space"
    );

    let bound = ImportedSymbol {
        local_name: "Foo".to_string(),
        ..unbound_type_only_symbol(ImportedName::Named("Foo".to_string()))
    };
    assert_eq!(
        desired_import_namespaces(&bound, None).namespaces(),
        (true, false),
        "`import type {{ Foo }}` restricts its binding to type space"
    );

    for imported_name in [ImportedName::Namespace, ImportedName::Default] {
        let type_star = ImportedSymbol {
            is_type_only_star: true,
            ..unbound_type_only_symbol(imported_name.clone())
        };
        assert_eq!(
            desired_import_namespaces(&type_star, None).namespaces(),
            (true, false),
            "`export type *` ({imported_name:?}) erases every value meaning (issue #2375)"
        );
    }
}

/// Target module for the ambient star tests: `Foo` is both an interface
/// and a const, `bar` is a plain value, and there is a default export.
fn type_value_pair_module(file_id: FileId) -> ResolvedModule {
    let export = |name: ExportName, is_type_only: bool, start: u32| ExportInfo {
        name,
        local_name: Some("x".to_string()),
        is_type_only,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(start, start + 10),
        members: vec![],
        is_side_effect_used: false,
        super_class: None,
        deprecated: false,
        deprecated_reason: None,
    };
    ResolvedModule {
        file_id,
        path: std::path::PathBuf::from("/project/impl.ts"),
        exports: vec![
            export(ExportName::Named("Foo".to_string()), true, 0),
            export(ExportName::Named("Foo".to_string()), false, 20),
            export(ExportName::Named("bar".to_string()), false, 40),
            export(ExportName::Default, false, 60),
        ]
        .into(),
        ..Default::default()
    }
}

fn ambient_star_graph(imports: Vec<ImportInfo>) -> ModuleGraph {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: std::path::PathBuf::from("/project/ambient.d.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: std::path::PathBuf::from("/project/impl.ts"),
            size_bytes: 100,
        },
    ];
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/ambient.d.ts"),
            resolved_imports: imports
                .into_iter()
                .map(|info| ResolvedImport {
                    info,
                    target: ResolveResult::InternalModule(FileId(1)),
                })
                .collect(),
            ..Default::default()
        },
        type_value_pair_module(FileId(1)),
    ];
    ModuleGraph::build(&resolved_modules, &[], &files)
}

fn ambient_import(imported_name: ImportedName) -> ImportInfo {
    ImportInfo {
        source: "./impl".to_string(),
        imported_name,
        local_name: String::new(),
        is_type_only: true,
        is_type_only_star: false,
        from_style: false,
        span: oxc_span::Span::new(0, 10),
        source_span: oxc_span::Span::default(),
    }
}

fn ambient_type_star_import(imported_name: ImportedName) -> ImportInfo {
    ImportInfo {
        is_type_only_star: true,
        ..ambient_import(imported_name)
    }
}

fn lanes_of(
    graph: &ModuleGraph,
    name: &str,
    is_type_only: bool,
) -> Vec<(ReferenceKind, ExportNamespace)> {
    graph.modules[1]
        .exports
        .iter()
        .find(|e| e.name.matches_str(name) && e.is_type_only == is_type_only)
        .unwrap_or_else(|| panic!("impl.ts must export `{name}` (type-only: {is_type_only})"))
        .references
        .iter()
        .map(|r| (r.kind, r.namespace))
        .collect()
}

#[test]
fn attach_ref_ambient_star_credits_both_declarations_and_skips_default() {
    // `declare module 'pkg' { export * from './impl' }`: the star surface
    // forwards `Foo` in both meanings and never forwards `default`.
    let graph = ambient_star_graph(vec![ambient_import(ImportedName::Namespace)]);

    assert_eq!(
        lanes_of(&graph, "Foo", true),
        vec![(ReferenceKind::NamespaceImport, ExportNamespace::Type)],
        "the interface half is credited in the type namespace"
    );
    assert_eq!(
        lanes_of(&graph, "Foo", false),
        vec![(ReferenceKind::NamespaceImport, ExportNamespace::Value)],
        "the const half is credited in the value namespace"
    );
    let bar_lanes = lanes_of(&graph, "bar", false);
    assert!(
        bar_lanes.contains(&(ReferenceKind::NamespaceImport, ExportNamespace::Value)),
        "a plain value export is credited in the value namespace, found {bar_lanes:?}"
    );
    assert!(
        lanes_of(&graph, "default", false).is_empty(),
        "ES `export *` never forwards `default`"
    );
}

#[test]
fn attach_ref_ambient_namespace_star_credits_default_through_its_own_import() {
    // `declare module 'pkg' { export * as ns from './impl' }` records the
    // namespace object's `default` member as a separate type-only import.
    let graph = ambient_star_graph(vec![
        ambient_import(ImportedName::Namespace),
        ambient_import(ImportedName::Default),
    ]);

    let default_lanes = lanes_of(&graph, "default", false);
    assert!(
        default_lanes.contains(&(ReferenceKind::DefaultImport, ExportNamespace::Value)),
        "`ns.default` reaches the default export in value space, found {default_lanes:?}"
    );
    assert_eq!(
        lanes_of(&graph, "Foo", false),
        vec![(ReferenceKind::NamespaceImport, ExportNamespace::Value)],
        "the named surface is unchanged by the extra default import"
    );
}

#[test]
fn attach_ref_ambient_type_only_star_credits_the_star_surface_in_type_space_only() {
    // Issue #2375: `declare module 'pkg' { export type * from './impl' }`
    // forwards the same names as the plain star with every value meaning
    // erased, so the const half of the `Foo` pair keeps reporting while
    // the interface half is credited, and `default` stays unforwarded.
    let graph = ambient_star_graph(vec![ambient_type_star_import(ImportedName::Namespace)]);

    assert_eq!(
        lanes_of(&graph, "Foo", true),
        vec![(ReferenceKind::NamespaceImport, ExportNamespace::Type)],
        "the interface half is credited in the type namespace"
    );
    assert!(
        lanes_of(&graph, "Foo", false).is_empty(),
        "the const half has a type declaration to shadow it, so it keeps reporting"
    );
    assert_eq!(
        lanes_of(&graph, "bar", false),
        vec![(ReferenceKind::NamespaceImport, ExportNamespace::Type)],
        "a value-only export is still forwarded, reachable as `typeof bar`"
    );
    assert!(
        lanes_of(&graph, "default", false).is_empty(),
        "`export type *` forwards no `default`, exactly like the plain star"
    );
}

#[test]
fn attach_ref_ambient_type_only_namespace_star_credits_default_in_type_space() {
    // `export type * as ns` exposes the namespace object in type space,
    // so `ns.default` reaches the target's default export there.
    let graph = ambient_star_graph(vec![
        ambient_type_star_import(ImportedName::Namespace),
        ambient_type_star_import(ImportedName::Default),
    ]);

    assert_eq!(
        lanes_of(&graph, "default", false),
        vec![(ReferenceKind::DefaultImport, ExportNamespace::Type)],
        "the default is credited in the type namespace and nowhere else"
    );
    assert!(
        lanes_of(&graph, "Foo", false).is_empty(),
        "the extra default import leaves the named surface in type space"
    );
}

#[test]
fn attach_ref_unbound_named_type_only_import_credits_type_space_only() {
    // `declare module 'pkg' { export { Foo } from './impl' }` (#2349) and
    // `import('./impl').Foo` record an unbound type-only named import. It
    // is not the star form, so it keeps its type-space credit: the
    // interface half is referenced, the const half is not.
    let graph = ambient_star_graph(vec![ambient_import(ImportedName::Named("Foo".to_string()))]);

    assert!(
        lanes_of(&graph, "Foo", true)
            .contains(&(ReferenceKind::NamedImport, ExportNamespace::Type)),
        "the interface half is credited in the type namespace"
    );
    assert!(
        !lanes_of(&graph, "Foo", false)
            .contains(&(ReferenceKind::NamedImport, ExportNamespace::Value)),
        "the const half must not be credited in the value namespace"
    );
}

/// Issue #2355: the template completeness guard records a whole-object use
/// for any binding mention it could not classify. On an entry-point
/// consumer that must still mean mark-all: the zero-access branch (an entry
/// file whose namespace import has no recorded access credits nothing) is
/// reserved for the case with no whole-object use either.
#[test]
fn attach_ref_namespace_whole_object_on_entry_point_marks_all_not_nothing() {
    fn build(whole_object: bool) -> ModuleGraph {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: std::path::PathBuf::from("/project/utils.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![fallow_types::discover::EntryPoint {
            path: std::path::PathBuf::from("/project/entry.ts"),
            source: fallow_types::discover::EntryPointSource::PackageJsonMain,
        }];
        let whole_object_uses: Vec<String> = if whole_object {
            vec!["utils".to_string()]
        } else {
            vec![]
        };
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: fallow_types::extract::ImportInfo {
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
                whole_object_uses: whole_object_uses.into(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: std::path::PathBuf::from("/project/utils.ts"),
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

    let graph = build(true);
    assert!(
        graph.entry_points.contains(&FileId(0)),
        "the consumer must be an entry point for this test to mean anything"
    );
    for export in &graph.modules[1].exports {
        assert!(
            !export.references.is_empty(),
            "{} should be referenced when an entry-point consumer uses the namespace whole",
            export.name
        );
    }

    let graph = build(false);
    for export in &graph.modules[1].exports {
        assert!(
            export.references.is_empty(),
            "{} should stay unreferenced when an entry-point consumer records no access",
            export.name
        );
    }
}

#[test]
fn attach_ref_css_module_narrows_to_member_accesses() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: std::path::PathBuf::from("/project/Button.module.css"),
            size_bytes: 50,
        },
    ];
    let entry_points = vec![fallow_types::discover::EntryPoint {
        path: std::path::PathBuf::from("/project/entry.ts"),
        source: fallow_types::discover::EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: fallow_types::extract::ImportInfo {
                    source: "./Button.module.css".to_string(),
                    imported_name: ImportedName::Default,
                    local_name: "styles".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![fallow_types::extract::MemberAccess {
                object: "styles".to_string(),
                member: "primary".to_string(),
            }]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: std::path::PathBuf::from("/project/Button.module.css"),
            exports: vec![
                fallow_types::extract::ExportInfo {
                    name: ExportName::Named("primary".to_string()),
                    local_name: Some("primary".to_string()),
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
                    name: ExportName::Named("secondary".to_string()),
                    local_name: Some("secondary".to_string()),
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
    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let primary = graph.modules[1]
        .exports
        .iter()
        .find(|e| e.name.to_string() == "primary")
        .unwrap();
    assert!(
        !primary.references.is_empty(),
        "primary should be referenced via CSS module narrowing"
    );

    let secondary = graph.modules[1]
        .exports
        .iter()
        .find(|e| e.name.to_string() == "secondary")
        .unwrap();
    assert!(
        secondary.references.is_empty(),
        "secondary should not be referenced — only primary is accessed"
    );
}

#[test]
fn attach_ref_default_import_creates_reference() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: std::path::PathBuf::from("/project/component.ts"),
            size_bytes: 50,
        },
    ];
    let entry_points = vec![fallow_types::discover::EntryPoint {
        path: std::path::PathBuf::from("/project/entry.ts"),
        source: fallow_types::discover::EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: fallow_types::extract::ImportInfo {
                    source: "./component".to_string(),
                    imported_name: ImportedName::Default,
                    local_name: "Component".to_string(),
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
            path: std::path::PathBuf::from("/project/component.ts"),
            exports: vec![fallow_types::extract::ExportInfo {
                name: ExportName::Default,
                local_name: Some("Component".to_string()),
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

    let default_export = graph.modules[1]
        .exports
        .iter()
        .find(|e| matches!(e.name, ExportName::Default))
        .unwrap();
    assert_eq!(default_export.references.len(), 1);
    assert_eq!(
        default_export.references[0].kind,
        ReferenceKind::DefaultImport
    );
}

#[test]
fn type_only_package_usage_tracked_through_build() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: std::path::PathBuf::from("/project/entry.ts"),
        size_bytes: 100,
    }];
    let entry_points = vec![fallow_types::discover::EntryPoint {
        path: std::path::PathBuf::from("/project/entry.ts"),
        source: fallow_types::discover::EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: std::path::PathBuf::from("/project/entry.ts"),
        resolved_imports: vec![ResolvedImport {
            info: fallow_types::extract::ImportInfo {
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
        }],
        ..Default::default()
    }];
    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    assert!(graph.package_usage.contains_key("react"));
    assert!(graph.type_only_package_usage.contains_key("react"));
}

#[test]
fn mark_member_exports_referenced_default_export() {
    let mut reference_paths = ReferencePathInterner::default();
    let mut exports = vec![ExportSymbol {
        name: ExportName::Default,
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 5),
        references: Vec::new(),
        reference_paths: Vec::new(),
        members: Vec::new(),
        deprecated: false,
        deprecated_reason: None,
    }];
    let accessed = vec!["default".to_string()];
    let found = mark_members(&mut exports, FileId(0), &accessed, &mut reference_paths);
    assert_eq!(exports[0].references.len(), 1);
    assert!(found.contains("default"));
}

#[test]
fn mark_member_exports_referenced_deduplicates() {
    let mut reference_paths = ReferencePathInterner::default();
    let mut exports = vec![ExportSymbol {
        name: ExportName::Named("foo".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 5),
        references: vec![SymbolReference {
            from_file: FileId(0),
            kind: ReferenceKind::NamedImport,
            namespace: ExportNamespace::Value,
            import_span: oxc_span::Span::new(0, 10),
            decorator_metadata: None,
        }],
        reference_paths: vec![reference_paths.direct(TARGET, ModuleLoadMechanism::EsModule)],
        members: Vec::new(),
        deprecated: false,
        deprecated_reason: None,
    }];
    let accessed = vec!["foo".to_string()];
    let found = mark_members(&mut exports, FileId(0), &accessed, &mut reference_paths);
    assert_eq!(exports[0].references.len(), 1);
    assert!(found.contains("foo"));
}

#[test]
fn mark_member_exports_referenced_skips_other_namespace_slot() {
    let mut reference_paths = ReferencePathInterner::default();
    let slot = |is_type_only: bool, start: u32| ExportSymbol {
        name: ExportName::Named("foo".to_string()),
        is_type_only,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(start, start + 5),
        references: Vec::new(),
        reference_paths: Vec::new(),
        members: Vec::new(),
        deprecated: false,
        deprecated_reason: None,
    };
    let mut exports = vec![slot(false, 0), slot(true, 10)];
    let accessed = vec!["foo".to_string()];
    let found = mark_members(&mut exports, FileId(0), &accessed, &mut reference_paths);

    assert_eq!(exports[0].references.len(), 1);
    assert!(
        exports[1].references.is_empty(),
        "a value member access must not credit the type-only slot of the same name"
    );
    assert!(found.contains("foo"));
}

#[test]
fn mark_member_exports_referenced_empty_accessed() {
    let mut reference_paths = ReferencePathInterner::default();
    let mut exports = vec![ExportSymbol {
        name: ExportName::Named("foo".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::new(0, 5),
        references: Vec::new(),
        reference_paths: Vec::new(),
        members: Vec::new(),
        deprecated: false,
        deprecated_reason: None,
    }];
    let accessed: Vec<String> = vec![];
    let found = mark_members(&mut exports, FileId(0), &accessed, &mut reference_paths);
    assert!(exports[0].references.is_empty());
    assert!(found.is_empty());
}

#[test]
fn create_synthetic_exports_skips_default_member() {
    let mut exports = Vec::new();
    let re_exports = vec![ReExportEdge {
        source_file: FileId(2),
        imported_name: "*".to_string(),
        exported_name: "*".to_string(),
        is_type_only: false,
        span: oxc_span::Span::default(),
    }];
    let accessed = vec!["default".to_string()];
    let found = FxHashSet::default();

    create_synthetic(&mut exports, &re_exports, &accessed, &found);

    assert!(exports.is_empty());
}
