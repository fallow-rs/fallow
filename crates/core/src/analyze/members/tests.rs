use super::*;
use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use crate::extract::{
    ExportInfo, ExportName, ImportInfo, ImportedName, MemberAccess, MemberInfo, MemberKind,
    ModuleInfo, VisibilityTag,
};
use crate::graph::{ExportSymbol, ModuleGraph, SymbolReference};
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};
use fallow_types::extract::{
    ClassHeritageInfo, FactoryCallMemberAccessFact, FluentChainMemberAccessFact,
    FluentChainNewMemberAccessFact, InstanceExportBindingFact, PlaywrightFixtureAliasFact,
    PlaywrightFixtureDefinitionFact, PlaywrightFixtureTypeFact, PlaywrightFixtureUseFact,
    SemanticFact,
};
use oxc_span::Span;
use std::path::PathBuf;

#[expect(
    clippy::too_many_arguments,
    reason = "test harness mirrors scanner inputs"
)]
fn find_unused_members(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    user_class_member_allowlist: &[UsedClassMemberRule],
    ignore_decorators: &[String],
) -> (Vec<UnusedMember>, Vec<UnusedMember>) {
    let results = find_unused_members_with_public_api_entry_points(UnusedMemberScanInput {
        graph,
        resolved_modules,
        modules,
        suppressions,
        line_offsets_by_file,
        user_class_member_allowlist,
        ignore_decorators,
        public_api_entry_points: &FxHashSet::default(),
        lit_active: false,
    });
    (results.enum_members, results.class_members)
}

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
            ..Default::default()
        })
        .collect();

    ModuleGraph::build(&resolved_modules, &entry_points, &files)
}

fn make_member(name: &str, kind: MemberKind) -> MemberInfo {
    MemberInfo {
        name: name.to_string(),
        kind,
        span: Span::new(10, 20),
        has_decorator: false,
        decorator_names: Vec::new(),
        is_instance_returning_static: false,
        is_self_returning: false,
    }
}

fn make_factory_member(name: &str) -> MemberInfo {
    MemberInfo {
        is_instance_returning_static: true,
        ..make_member(name, MemberKind::ClassMethod)
    }
}

fn make_self_member(name: &str) -> MemberInfo {
    MemberInfo {
        is_self_returning: true,
        ..make_member(name, MemberKind::ClassMethod)
    }
}

fn make_resolved_import(source: &str, imported: &str, local: &str, target: u32) -> ResolvedImport {
    ResolvedImport {
        info: ImportInfo {
            source: source.to_string(),
            imported_name: ImportedName::Named(imported.to_string()),
            local_name: local.to_string(),
            is_type_only: false,
            from_style: false,
            span: Span::new(0, 10),
            source_span: Span::default(),
        },
        target: ResolveResult::InternalModule(FileId(target)),
    }
}

fn make_export_with_members(
    name: &str,
    members: Vec<MemberInfo>,
    ref_from: Option<u32>,
) -> ExportSymbol {
    let references = ref_from
        .map(|from| {
            vec![SymbolReference {
                from_file: FileId(from),
                kind: crate::graph::ReferenceKind::NamedImport,
                import_span: Span::new(0, 10),
            }]
        })
        .unwrap_or_default();
    ExportSymbol {
        name: ExportName::Named(name.to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(0, 10),
        references,
        members,
    }
}

#[test]
fn typed_playwright_fixture_use_fact_credits_fixture_member() {
    let mut graph = build_graph(&[
        ("/src/spec.ts", true),
        ("/src/fixtures.ts", false),
        ("/src/admin-page.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members("test", vec![], Some(0))];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export_with_members(
        "AdminPage",
        vec![make_member("assertGreeting", MemberKind::ClassMethod)],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/src/spec.ts"),
        resolved_imports: vec![
            ResolvedImport {
                info: ImportInfo {
                    source: "./fixtures".to_string(),
                    imported_name: ImportedName::Named("test".to_string()),
                    local_name: "test".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 10),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            },
            ResolvedImport {
                info: ImportInfo {
                    source: "./admin-page".to_string(),
                    imported_name: ImportedName::Named("AdminPage".to_string()),
                    local_name: "AdminPage".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(11, 20),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            },
        ],
        semantic_facts: vec![
            SemanticFact::PlaywrightFixtureDefinition(PlaywrightFixtureDefinitionFact {
                test_name: "test".to_string(),
                fixture_name: "adminPage".to_string(),
                type_name: "AdminPage".to_string(),
            }),
            SemanticFact::PlaywrightFixtureUse(PlaywrightFixtureUseFact {
                test_name: "test".to_string(),
                fixture_name: "adminPage".to_string(),
                member: "assertGreeting".to_string(),
            }),
        ]
        .into(),
        ..Default::default()
    }];

    let mut accessed_members = FxHashMap::default();
    let indexes = MemberPassIndexes::build(&resolved_modules);
    propagate_playwright_fixture_accesses(
        &graph,
        &resolved_modules,
        &indexes,
        &mut accessed_members,
    );

    let credited = accessed_members
        .get(&ExportKey::new(FileId(2), "AdminPage"))
        .expect("fixture target class should be credited");
    assert!(credited.contains("assertGreeting"));
}

#[test]
fn typed_playwright_fixture_alias_fact_expands_fixture_targets() {
    let mut graph = build_graph(&[
        ("/src/spec.ts", true),
        ("/src/fixtures.ts", false),
        ("/src/wrapped-fixtures.ts", false),
        ("/src/admin-page.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members("testPrimary", vec![], Some(2))];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export_with_members("mergedTest", vec![], Some(0))];
    graph.modules[3].set_reachable(true);
    graph.modules[3].exports = vec![make_export_with_members(
        "AdminPage",
        vec![make_member("assertGreeting", MemberKind::ClassMethod)],
        Some(1),
    )];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/spec.ts"),
            resolved_imports: vec![make_resolved_import(
                "./wrapped-fixtures",
                "mergedTest",
                "mergedTest",
                2,
            )],
            semantic_facts: vec![SemanticFact::PlaywrightFixtureUse(
                PlaywrightFixtureUseFact {
                    test_name: "mergedTest".to_string(),
                    fixture_name: "adminPage".to_string(),
                    member: "assertGreeting".to_string(),
                },
            )]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/src/fixtures.ts"),
            resolved_imports: vec![make_resolved_import(
                "./admin-page",
                "AdminPage",
                "AdminPage",
                3,
            )],
            exports: vec![make_export_info("testPrimary", None)],
            semantic_facts: vec![SemanticFact::PlaywrightFixtureDefinition(
                PlaywrightFixtureDefinitionFact {
                    test_name: "testPrimary".to_string(),
                    fixture_name: "adminPage".to_string(),
                    type_name: "AdminPage".to_string(),
                },
            )]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/src/wrapped-fixtures.ts"),
            resolved_imports: vec![make_resolved_import(
                "./fixtures",
                "testPrimary",
                "testPrimary",
                1,
            )],
            exports: vec![make_export_info("mergedTest", None)],
            semantic_facts: vec![SemanticFact::PlaywrightFixtureAlias(
                PlaywrightFixtureAliasFact {
                    test_name: "mergedTest".to_string(),
                    base_name: "testPrimary".to_string(),
                },
            )]
            .into(),
            ..Default::default()
        },
    ];

    let mut accessed_members = FxHashMap::default();
    let indexes = MemberPassIndexes::build(&resolved_modules);
    propagate_playwright_fixture_accesses(
        &graph,
        &resolved_modules,
        &indexes,
        &mut accessed_members,
    );

    let credited = accessed_members
        .get(&ExportKey::new(FileId(3), "AdminPage"))
        .expect("aliased fixture target class should be credited");
    assert!(credited.contains("assertGreeting"));
}

#[test]
fn typed_playwright_fixture_type_fact_expands_nested_fixture_targets() {
    let mut graph = build_graph(&[
        ("/src/spec.ts", true),
        ("/src/fixtures.ts", false),
        ("/src/pages.ts", false),
        ("/src/admin-page.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members("test", vec![], Some(0))];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export_with_members("Pages", vec![], Some(0))];
    graph.modules[3].set_reachable(true);
    graph.modules[3].exports = vec![make_export_with_members(
        "AdminPage",
        vec![make_member("assertGreeting", MemberKind::ClassMethod)],
        Some(2),
    )];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/spec.ts"),
            resolved_imports: vec![
                make_resolved_import("./fixtures", "test", "test", 1),
                make_resolved_import("./pages", "Pages", "Pages", 2),
            ],
            semantic_facts: vec![
                SemanticFact::PlaywrightFixtureDefinition(PlaywrightFixtureDefinitionFact {
                    test_name: "test".to_string(),
                    fixture_name: "pages".to_string(),
                    type_name: "Pages".to_string(),
                }),
                SemanticFact::PlaywrightFixtureUse(PlaywrightFixtureUseFact {
                    test_name: "test".to_string(),
                    fixture_name: "pages.adminPage".to_string(),
                    member: "assertGreeting".to_string(),
                }),
            ]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/src/pages.ts"),
            resolved_imports: vec![make_resolved_import(
                "./admin-page",
                "AdminPage",
                "AdminPage",
                3,
            )],
            exports: vec![make_export_info("Pages", None)],
            semantic_facts: vec![SemanticFact::PlaywrightFixtureType(
                PlaywrightFixtureTypeFact {
                    alias_name: "Pages".to_string(),
                    fixture_name: "adminPage".to_string(),
                    type_name: "AdminPage".to_string(),
                },
            )]
            .into(),
            ..Default::default()
        },
    ];

    let mut accessed_members = FxHashMap::default();
    let indexes = MemberPassIndexes::build(&resolved_modules);
    propagate_playwright_fixture_accesses(
        &graph,
        &resolved_modules,
        &indexes,
        &mut accessed_members,
    );

    let credited = accessed_members
        .get(&ExportKey::new(FileId(3), "AdminPage"))
        .expect("nested fixture target class should be credited");
    assert!(credited.contains("assertGreeting"));
}

#[test]
fn typed_instance_export_binding_fact_builds_target_map() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/service.ts", false),
        ("/src/stale-service.ts", false),
    ]);
    graph.modules[0].exports = vec![make_export_with_members("service", vec![], Some(0))];
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members("Service", vec![], Some(0))];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export_with_members("StaleService", vec![], Some(0))];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/src/entry.ts"),
        resolved_imports: vec![
            make_resolved_import("./service", "Service", "Service", 1),
            make_resolved_import("./stale-service", "StaleService", "StaleService", 2),
        ],
        exports: vec![make_export_info("service", None)],
        semantic_facts: vec![SemanticFact::InstanceExportBinding(
            InstanceExportBindingFact {
                export_name: "service".to_string(),
                target_name: "Service".to_string(),
            },
        )]
        .into(),
        ..Default::default()
    }];

    let indexes = MemberPassIndexes::build(&resolved_modules);
    let instance_targets = build_instance_export_targets(&graph, &resolved_modules, &indexes);

    assert_eq!(
        instance_targets.get(&ExportKey::new(FileId(0), "service")),
        Some(&vec![ExportKey::new(FileId(1), "Service")])
    );
}

#[test]
fn typed_factory_call_fact_credits_class_member() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/my-class.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "MyClass",
        vec![
            make_factory_member("getInstance"),
            make_member("getData", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let class_export = ExportInfo {
        name: ExportName::Named("MyClass".to_string()),
        local_name: Some("MyClass".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(0, 10),
        members: vec![
            make_factory_member("getInstance"),
            make_member("getData", MemberKind::ClassMethod),
        ],
        super_class: None,
    };
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./my-class".to_string(),
                    imported_name: ImportedName::Named("MyClass".to_string()),
                    local_name: "MyClass".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 10),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            semantic_facts: vec![SemanticFact::FactoryCallMemberAccess(
                FactoryCallMemberAccessFact {
                    callee_object: "MyClass".to_string(),
                    callee_method: "getInstance".to_string(),
                    member: "getData".to_string(),
                },
            )]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/src/my-class.ts"),
            exports: vec![class_export],
            ..Default::default()
        },
    ];

    let mut accessed_members = FxHashMap::default();
    let indexes = MemberPassIndexes::build(&resolved_modules);
    propagate_factory_call_accesses(&graph, &resolved_modules, &indexes, &mut accessed_members);

    let credited = accessed_members
        .get(&ExportKey::new(FileId(1), "MyClass"))
        .expect("factory target class should be credited");
    assert!(credited.contains("getData"));
}

#[test]
fn typed_fluent_chain_fact_credits_class_member() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/event-builder.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "EventBuilder",
        vec![
            make_factory_member("create"),
            make_self_member("setProcessId"),
            make_self_member("setSubject"),
            make_member("build", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let class_export = ExportInfo {
        name: ExportName::Named("EventBuilder".to_string()),
        local_name: Some("EventBuilder".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(0, 10),
        members: vec![
            make_factory_member("create"),
            make_self_member("setProcessId"),
            make_self_member("setSubject"),
            make_member("build", MemberKind::ClassMethod),
        ],
        super_class: None,
    };
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./event-builder".to_string(),
                    imported_name: ImportedName::Named("EventBuilder".to_string()),
                    local_name: "EventBuilder".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 10),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            semantic_facts: vec![SemanticFact::FluentChainMemberAccess(
                FluentChainMemberAccessFact {
                    root_object: "EventBuilder".to_string(),
                    root_method: "create".to_string(),
                    chain: vec!["setProcessId".to_string(), "setSubject".to_string()],
                    member: "build".to_string(),
                },
            )]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/src/event-builder.ts"),
            exports: vec![class_export],
            ..Default::default()
        },
    ];

    let mut accessed_members = FxHashMap::default();
    let indexes = MemberPassIndexes::build(&resolved_modules);
    propagate_fluent_chain_accesses(&graph, &resolved_modules, &indexes, &mut accessed_members);

    let credited = accessed_members
        .get(&ExportKey::new(FileId(1), "EventBuilder"))
        .expect("fluent target class should be credited");
    assert!(credited.contains("build"));
}

#[test]
fn typed_fluent_chain_new_fact_credits_class_member() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/option-builder.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "OptionBuilder",
        vec![
            make_self_member("addDefault"),
            make_self_member("addFromCli"),
            make_member("build", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let class_export = ExportInfo {
        name: ExportName::Named("OptionBuilder".to_string()),
        local_name: Some("OptionBuilder".to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(0, 10),
        members: vec![
            make_self_member("addDefault"),
            make_self_member("addFromCli"),
            make_member("build", MemberKind::ClassMethod),
        ],
        super_class: None,
    };
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./option-builder".to_string(),
                    imported_name: ImportedName::Named("OptionBuilder".to_string()),
                    local_name: "OptionBuilder".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 10),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            semantic_facts: vec![SemanticFact::FluentChainNewMemberAccess(
                FluentChainNewMemberAccessFact {
                    class_name: "OptionBuilder".to_string(),
                    chain: vec!["addDefault".to_string(), "addFromCli".to_string()],
                    member: "build".to_string(),
                },
            )]
            .into(),
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/src/option-builder.ts"),
            exports: vec![class_export],
            ..Default::default()
        },
    ];

    let mut accessed_members = FxHashMap::default();
    let indexes = MemberPassIndexes::build(&resolved_modules);
    propagate_fluent_chain_new_accesses(&graph, &resolved_modules, &indexes, &mut accessed_members);

    let credited = accessed_members
        .get(&ExportKey::new(FileId(1), "OptionBuilder"))
        .expect("fluent-new target class should be credited");
    assert!(credited.contains("build"));
}

fn make_module_with_class_heritage(
    file_id: u32,
    export_name: &str,
    super_class: Option<&str>,
    implements: &[&str],
) -> ModuleInfo {
    ModuleInfo {
        file_id: FileId(file_id),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        dynamic_import_patterns: vec![],
        require_calls: vec![],
        package_path_references: Box::default(),
        member_accesses: vec![],
        semantic_facts: Box::default(),
        whole_object_uses: Box::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![ClassHeritageInfo {
            export_name: export_name.to_string(),
            super_class: super_class.map(str::to_string),
            implements: implements.iter().map(ToString::to_string).collect(),
            instance_bindings: Vec::new(),
            super_class_type_args: Vec::new(),
            generic_instance_bindings: Vec::new(),
        }],
        exported_factory_returns: Box::default(),
        exported_factory_return_object_shapes: Box::default(),
        type_member_types: Box::default(),
        injection_tokens: Vec::new(),
        local_type_declarations: vec![],
        public_signature_type_references: vec![],
        namespace_object_aliases: vec![],
        iconify_prefixes: vec![],
        iconify_icon_names: vec![],
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        inline_server_action_exports: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: Vec::new(),
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        angular_inputs: Vec::new(),
        angular_outputs: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
        has_route_loader_data_whole_use: false,
        component_functions: Vec::new(),
        react_props: Vec::new(),
        hook_uses: Vec::new(),
        render_edges: Vec::new(),
        svelte_dispatched_events: Vec::new(),
        svelte_listened_events: Vec::new(),
        angular_component_selectors: Vec::new(),
        registered_custom_elements: Vec::new(),
        used_custom_element_tags: Vec::new(),
        angular_used_selectors: Vec::new(),
        angular_entry_component_refs: Vec::new(),
        has_dynamic_component_render: false,
        has_dynamic_dispatch: false,
    }
}

#[test]
fn unused_members_empty_graph() {
    let graph = build_graph(&[]);

    let (enum_members, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(enum_members.is_empty());
    assert!(class_members.is_empty());
}

#[test]
fn unused_enum_member_detected() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Status",
        vec![
            make_member("Active", MemberKind::EnumMember),
            make_member("Inactive", MemberKind::EnumMember),
        ],
        Some(0), // referenced from entry
    )];

    let (enum_members, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(enum_members.len(), 2);
    assert!(class_members.is_empty());
    let names: FxHashSet<&str> = enum_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();
    assert!(names.contains("Active"));
    assert!(names.contains("Inactive"));
}

#[test]
fn accessed_enum_member_not_flagged() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Status",
        vec![
            make_member("Active", MemberKind::EnumMember),
            make_member("Inactive", MemberKind::EnumMember),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/src/entry.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./enums".to_string(),
                imported_name: ImportedName::Named("Status".to_string()),
                local_name: "Status".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        member_accesses: vec![MemberAccess {
            object: "Status".to_string(),
            member: "Active".to_string(),
        }],
        ..Default::default()
    }];

    let (enum_members, _) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(enum_members.len(), 1);
    assert_eq!(enum_members[0].member_name, "Inactive");
}

#[test]
fn accessed_enum_member_via_re_export_not_flagged() {
    let mut graph = build_graph(&[
        ("/app/consumer.ts", true),
        ("/lib/index.ts", true),
        ("/lib/types.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[2].set_reachable(true);

    graph.modules[1].exports = vec![make_export_with_members(
        "Status",
        vec![],
        Some(0), // referenced from consumer
    )];
    graph.modules[1].re_exports = vec![crate::graph::ReExportEdge {
        source_file: FileId(2),
        imported_name: "Status".to_string(),
        exported_name: "Status".to_string(),
        is_type_only: false,
        span: Span::default(),
    }];

    graph.modules[2].exports = vec![make_export_with_members(
        "Status",
        vec![
            make_member("Active", MemberKind::EnumMember),
            make_member("Inactive", MemberKind::EnumMember),
            make_member("Archived", MemberKind::EnumMember),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/app/consumer.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "@scope/lib".to_string(),
                imported_name: ImportedName::Named("Status".to_string()),
                local_name: "Status".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        member_accesses: vec![
            MemberAccess {
                object: "Status".to_string(),
                member: "Active".to_string(),
            },
            MemberAccess {
                object: "Status".to_string(),
                member: "Inactive".to_string(),
            },
        ],
        ..Default::default()
    }];

    let (enum_members, _) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );

    assert_eq!(enum_members.len(), 1, "{enum_members:?}");
    assert_eq!(enum_members[0].member_name, "Archived");
    assert_eq!(enum_members[0].parent_name, "Status");
}

#[test]
fn accessed_class_static_member_via_re_export_not_flagged() {
    let mut graph = build_graph(&[
        ("/app/consumer.ts", true),
        ("/lib/index.ts", true),
        ("/lib/utils.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[2].set_reachable(true);

    graph.modules[1].exports = vec![make_export_with_members("StringUtils", vec![], Some(0))];
    graph.modules[1].re_exports = vec![crate::graph::ReExportEdge {
        source_file: FileId(2),
        imported_name: "StringUtils".to_string(),
        exported_name: "StringUtils".to_string(),
        is_type_only: false,
        span: Span::default(),
    }];

    graph.modules[2].exports = vec![make_export_with_members(
        "StringUtils",
        vec![
            make_member("toUpper", MemberKind::ClassMethod),
            make_member("toLower", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/app/consumer.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "@scope/lib".to_string(),
                imported_name: ImportedName::Named("StringUtils".to_string()),
                local_name: "StringUtils".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        member_accesses: vec![MemberAccess {
            object: "StringUtils".to_string(),
            member: "toUpper".to_string(),
        }],
        ..Default::default()
    }];

    let (_, class_members) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );

    assert_eq!(class_members.len(), 1, "{class_members:?}");
    assert_eq!(class_members[0].member_name, "toLower");
}

#[test]
fn accessed_member_via_renamed_re_export_not_flagged() {
    let mut graph = build_graph(&[
        ("/app/consumer.ts", true),
        ("/lib/index.ts", true),
        ("/lib/types.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[2].set_reachable(true);

    graph.modules[1].exports = vec![make_export_with_members("Renamed", vec![], Some(0))];
    graph.modules[1].re_exports = vec![crate::graph::ReExportEdge {
        source_file: FileId(2),
        imported_name: "Original".to_string(),
        exported_name: "Renamed".to_string(),
        is_type_only: false,
        span: Span::default(),
    }];

    graph.modules[2].exports = vec![make_export_with_members(
        "Original",
        vec![
            make_member("A", MemberKind::EnumMember),
            make_member("B", MemberKind::EnumMember),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/app/consumer.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "@scope/lib".to_string(),
                imported_name: ImportedName::Named("Renamed".to_string()),
                local_name: "Renamed".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        member_accesses: vec![MemberAccess {
            object: "Renamed".to_string(),
            member: "A".to_string(),
        }],
        ..Default::default()
    }];

    let (enum_members, _) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );

    assert_eq!(enum_members.len(), 1, "{enum_members:?}");
    assert_eq!(enum_members[0].member_name, "B");
    assert_eq!(enum_members[0].parent_name, "Original");
}

#[test]
fn accessed_member_via_star_re_export_not_flagged() {
    let mut graph = build_graph(&[
        ("/app/consumer.ts", true),
        ("/lib/index.ts", true),
        ("/lib/types.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[2].set_reachable(true);

    graph.modules[1].re_exports = vec![crate::graph::ReExportEdge {
        source_file: FileId(2),
        imported_name: "*".to_string(),
        exported_name: "*".to_string(),
        is_type_only: false,
        span: Span::default(),
    }];

    graph.modules[2].exports = vec![make_export_with_members(
        "Status",
        vec![
            make_member("Active", MemberKind::EnumMember),
            make_member("Inactive", MemberKind::EnumMember),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/app/consumer.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "@scope/lib".to_string(),
                imported_name: ImportedName::Named("Status".to_string()),
                local_name: "Status".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        member_accesses: vec![MemberAccess {
            object: "Status".to_string(),
            member: "Active".to_string(),
        }],
        ..Default::default()
    }];

    let (enum_members, _) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );

    assert_eq!(enum_members.len(), 1, "{enum_members:?}");
    assert_eq!(enum_members[0].member_name, "Inactive");
}

#[test]
fn whole_object_use_skips_all_members() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Status",
        vec![
            make_member("Active", MemberKind::EnumMember),
            make_member("Inactive", MemberKind::EnumMember),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/src/entry.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./enums".to_string(),
                imported_name: ImportedName::Named("Status".to_string()),
                local_name: "Status".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        whole_object_uses: vec!["Status".to_string()].into(),
        ..Default::default()
    }];

    let (enum_members, class_members) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(enum_members.is_empty());
    assert!(class_members.is_empty());
}

#[test]
fn decorated_class_member_not_flagged() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/entity.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "User",
        vec![MemberInfo {
            name: "name".to_string(),
            kind: MemberKind::ClassProperty,
            span: Span::new(10, 20),
            has_decorator: true, // @Column() etc.
            decorator_names: vec!["Column".to_string()],
            is_instance_returning_static: false,
            is_self_returning: false,
        }],
        Some(0),
    )];

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(class_members.is_empty());
}

#[test]
fn ignore_decorator_set_record_seen_marks_entries() {
    let set = IgnoreDecoratorSet::from_config(&["@step".to_string()]);
    assert!(!set.entries[0].matched.load(Ordering::Relaxed));
    set.record_seen("step");
    assert!(
        set.entries[0].matched.load(Ordering::Relaxed),
        "record_seen should mark a bare-name entry as seen on a matching decorator path"
    );
}

#[test]
fn ignore_decorator_set_dotted_record_seen_distinct_from_bare() {
    let set = IgnoreDecoratorSet::from_config(&[
        "decorators.log".to_string(),
        "decorators.audit".to_string(),
    ]);
    set.record_seen("decorators.log");
    assert!(
        set.entries[0].matched.load(Ordering::Relaxed),
        "decorators.log entry should be marked seen by an exact dotted match"
    );
    assert!(
        !set.entries[1].matched.load(Ordering::Relaxed),
        "decorators.audit entry must NOT be marked seen by record_seen('decorators.log')"
    );
}

#[test]
fn react_lifecycle_method_not_flagged() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/component.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "MyComponent",
        vec![
            make_member("render", MemberKind::ClassMethod),
            make_member("componentDidMount", MemberKind::ClassMethod),
            make_member("customMethod", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(class_members.len(), 1);
    assert_eq!(class_members[0].member_name, "customMethod");
}

#[test]
fn angular_lifecycle_method_not_flagged() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/component.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "AppComponent",
        vec![
            make_member("ngOnInit", MemberKind::ClassMethod),
            make_member("ngOnDestroy", MemberKind::ClassMethod),
            make_member("myHelper", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(class_members.len(), 1);
    assert_eq!(class_members[0].member_name, "myHelper");
}

#[test]
fn user_class_member_allowlist_not_flagged() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/renderer.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "MyRendererComponent",
        vec![
            make_member("agInit", MemberKind::ClassMethod),
            make_member("refresh", MemberKind::ClassMethod),
            make_member("customHelper", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let allowlist = vec![
        UsedClassMemberRule::from("agInit"),
        UsedClassMemberRule::from("refresh"),
    ];

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &allowlist,
        &[],
    );
    assert_eq!(
        class_members.len(),
        1,
        "only customHelper should remain unused"
    );
    assert_eq!(class_members[0].member_name, "customHelper");
}

#[test]
fn user_class_member_allowlist_globs_match_member_names() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/listener.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "GrammarListener",
        vec![
            make_member("enterRule", MemberKind::ClassMethod),
            make_member("exitRule", MemberKind::ClassMethod),
            make_member("onNodeEvent", MemberKind::ClassMethod),
            make_member("customHelper", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let allowlist = vec![
        UsedClassMemberRule::from("enter*"),
        UsedClassMemberRule::from("exit*"),
        UsedClassMemberRule::from("on?odeEvent"),
    ];

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &allowlist,
        &[],
    );
    assert_eq!(
        class_members.len(),
        1,
        "only customHelper should remain unused"
    );
    assert_eq!(class_members[0].member_name, "customHelper");
}

#[test]
fn member_glob_patterns_track_whether_they_matched() {
    let rules = vec![
        UsedClassMemberRule::from("enter*"),
        UsedClassMemberRule::from("missing*"),
    ];
    let allowlist = ClassMemberAllowlist::from_rules(&rules);

    assert!(allowlist.matches("enterRule", None, &[]));

    assert!(allowlist.global_patterns[0].matched.load(Ordering::Relaxed));
    assert!(!allowlist.global_patterns[1].matched.load(Ordering::Relaxed));
}

#[test]
fn user_class_member_allowlist_does_not_affect_enums() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/status.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Status",
        vec![make_member("refresh", MemberKind::EnumMember)],
        Some(0),
    )];

    let allowlist = vec![UsedClassMemberRule::from("refresh")];

    let (enum_members, _) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &allowlist,
        &[],
    );
    assert_eq!(enum_members.len(), 1);
    assert_eq!(enum_members[0].member_name, "refresh");
}

#[test]
fn scoped_allowlist_matches_implements_only() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/renderer.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "MyRendererComponent",
        vec![
            make_member("refresh", MemberKind::ClassMethod),
            make_member("customHelper", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let modules = vec![make_module_with_class_heritage(
        1,
        "MyRendererComponent",
        None,
        &["ICellRendererAngularComp"],
    )];
    let allowlist = vec![UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: None,
        implements: Some("ICellRendererAngularComp".to_string()),
        members: vec!["refresh".to_string()],
    })];

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &modules,
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &allowlist,
        &[],
    );

    assert_eq!(class_members.len(), 1);
    assert_eq!(class_members[0].member_name, "customHelper");
}

#[test]
fn scoped_allowlist_globs_match_only_matching_heritage() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/listener.ts", false),
        ("/src/unrelated.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "GrammarListener",
        vec![
            make_member("enterRule", MemberKind::ClassMethod),
            make_member("exitRule", MemberKind::ClassMethod),
            make_member("customHelper", MemberKind::ClassMethod),
        ],
        Some(0),
    )];
    graph.modules[2].set_reachable(true);
    graph.modules[2].exports = vec![make_export_with_members(
        "DashboardComponent",
        vec![make_member("enterRule", MemberKind::ClassMethod)],
        Some(0),
    )];

    let modules = vec![make_module_with_class_heritage(
        1,
        "GrammarListener",
        Some("BaseListener"),
        &[],
    )];
    let allowlist = vec![UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some("BaseListener".to_string()),
        implements: None,
        members: vec!["enter*".to_string(), "exit*".to_string()],
    })];

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &modules,
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &allowlist,
        &[],
    );
    assert_eq!(
        class_members.len(),
        2,
        "only unrelated enterRule and listener customHelper should remain unused: {class_members:?}"
    );
    assert!(
        class_members
            .iter()
            .any(|member| member.parent_name == "DashboardComponent"
                && member.member_name == "enterRule"),
        "scoped glob must not suppress unrelated classes: {class_members:?}"
    );
    assert!(
        class_members
            .iter()
            .any(|member| member.parent_name == "GrammarListener"
                && member.member_name == "customHelper"),
        "scoped glob must not suppress unmatched members: {class_members:?}"
    );
    assert!(
        !class_members
            .iter()
            .any(|member| member.parent_name == "GrammarListener"
                && (member.member_name == "enterRule" || member.member_name == "exitRule")),
        "scoped glob should suppress matching listener members: {class_members:?}"
    );
}

#[test]
fn scoped_allowlist_matches_extends_only() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/command.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "GenerateReport",
        vec![
            make_member("execute", MemberKind::ClassMethod),
            make_member("customHelper", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let modules = vec![make_module_with_class_heritage(
        1,
        "GenerateReport",
        Some("BaseCommand"),
        &[],
    )];
    let allowlist = vec![UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some("BaseCommand".to_string()),
        implements: None,
        members: vec!["execute".to_string()],
    })];

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &modules,
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &allowlist,
        &[],
    );

    assert_eq!(class_members.len(), 1);
    assert_eq!(class_members[0].member_name, "customHelper");
}

fn make_export_info(name: &str, super_class: Option<&str>) -> ExportInfo {
    ExportInfo {
        name: ExportName::Named(name.to_string()),
        local_name: Some(name.to_string()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(0, 10),
        members: vec![],
        super_class: super_class.map(str::to_string),
    }
}

#[test]
fn is_native_error_base_name_recognizes_native_errors() {
    for base in [
        "Error",
        "TypeError",
        "RangeError",
        "SyntaxError",
        "ReferenceError",
        "EvalError",
        "URIError",
        "AggregateError",
    ] {
        assert!(
            is_native_error_base_name(base),
            "{base} should be a native error base"
        );
    }
    assert!(!is_native_error_base_name("Person"));
    assert!(!is_native_error_base_name("HttpException"));
    assert!(!is_native_error_base_name("error")); // case-sensitive
    assert!(!is_native_error_base_name("DOMException")); // out of scope
}

#[test]
fn error_subclass_name_member_not_flagged_but_other_members_are() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/errors.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "DomainError",
        vec![
            make_member("name", MemberKind::ClassProperty),
            make_member("unusedHelper", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let modules = vec![make_module_with_class_heritage(
        1,
        "DomainError",
        Some("Error"),
        &[],
    )];

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &modules,
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );

    assert_eq!(class_members.len(), 1);
    assert_eq!(class_members[0].member_name, "unusedHelper");
}

#[test]
fn ordinary_class_name_member_still_flagged() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/person.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Person",
        vec![make_member("name", MemberKind::ClassProperty)],
        Some(0),
    )];

    let modules = vec![make_module_with_class_heritage(1, "Person", None, &[])];

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &modules,
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );

    assert_eq!(class_members.len(), 1);
    assert_eq!(class_members[0].member_name, "name");
}

#[test]
fn transitive_error_subclass_name_member_not_flagged() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/errors.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![
        make_export_with_members(
            "DomainError",
            vec![make_member("name", MemberKind::ClassProperty)],
            Some(0),
        ),
        make_export_with_members(
            "ApiError",
            vec![make_member("name", MemberKind::ClassProperty)],
            Some(0),
        ),
    ];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(1),
        path: PathBuf::from("/src/errors.ts"),
        exports: vec![
            make_export_info("DomainError", Some("Error")),
            make_export_info("ApiError", Some("DomainError")),
        ],
        ..Default::default()
    }];

    let mut errors_module = make_module_with_class_heritage(1, "DomainError", Some("Error"), &[]);
    errors_module.class_heritage.push(ClassHeritageInfo {
        export_name: "ApiError".to_string(),
        super_class: Some("DomainError".to_string()),
        implements: Vec::new(),
        instance_bindings: Vec::new(),
        super_class_type_args: Vec::new(),
        generic_instance_bindings: Vec::new(),
    });
    let modules = vec![errors_module];

    let (_, class_members) = find_unused_members(
        &graph,
        &resolved_modules,
        &modules,
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );

    assert!(
        class_members.is_empty(),
        "both DomainError.name and ApiError.name should be credited, got {class_members:?}"
    );
}

#[test]
fn this_member_access_not_flagged() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/service.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Service",
        vec![
            make_member("label", MemberKind::ClassProperty),
            make_member("unused_prop", MemberKind::ClassProperty),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(1), // same file as the service
        path: PathBuf::from("/src/service.ts"),
        member_accesses: vec![MemberAccess {
            object: "this".to_string(),
            member: "label".to_string(),
        }],
        ..Default::default()
    }];

    let (_, class_members) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(class_members.len(), 1);
    assert_eq!(class_members[0].member_name, "unused_prop");
}

#[test]
fn unreferenced_export_skips_member_analysis() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Status",
        vec![make_member("Active", MemberKind::EnumMember)],
        None, // no references
    )];

    let (enum_members, _) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(enum_members.is_empty());
}

#[test]
fn unreachable_module_skips_member_analysis() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/dead.ts", false)]);
    graph.modules[1].exports = vec![make_export_with_members(
        "DeadEnum",
        vec![make_member("X", MemberKind::EnumMember)],
        Some(0),
    )];

    let (enum_members, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(enum_members.is_empty());
    assert!(class_members.is_empty());
}

#[test]
fn entry_point_module_skips_member_analysis() {
    let mut graph = build_graph(&[("/src/entry.ts", true)]);
    graph.modules[0].exports = vec![make_export_with_members(
        "EntryEnum",
        vec![make_member("X", MemberKind::EnumMember)],
        None,
    )];

    let (enum_members, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(enum_members.is_empty());
    assert!(class_members.is_empty());
}

#[test]
fn enum_member_kind_routed_to_enum_results() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Status",
        vec![make_member("Active", MemberKind::EnumMember)],
        Some(0),
    )];

    let (enum_members, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(enum_members.len(), 1);
    assert_eq!(enum_members[0].kind, MemberKind::EnumMember);
    assert!(class_members.is_empty());
}

#[test]
fn class_member_kind_routed_to_class_results() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/class.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "MyClass",
        vec![
            make_member("myMethod", MemberKind::ClassMethod),
            make_member("myProp", MemberKind::ClassProperty),
        ],
        Some(0),
    )];

    let (enum_members, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(enum_members.is_empty());
    assert_eq!(class_members.len(), 2);
    assert!(
        class_members
            .iter()
            .any(|m| m.kind == MemberKind::ClassMethod)
    );
    assert!(
        class_members
            .iter()
            .any(|m| m.kind == MemberKind::ClassProperty)
    );
}

#[test]
fn instance_member_access_not_flagged() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/service.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "MyService",
        vec![
            make_member("greet", MemberKind::ClassMethod),
            make_member("unusedMethod", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/src/entry.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./service".to_string(),
                imported_name: ImportedName::Named("MyService".to_string()),
                local_name: "MyService".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        member_accesses: vec![MemberAccess {
            object: "MyService".to_string(),
            member: "greet".to_string(),
        }],
        ..Default::default()
    }];

    let (_, class_members) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(class_members.len(), 1);
    assert_eq!(class_members[0].member_name, "unusedMethod");
}

#[test]
fn this_access_does_not_skip_enum_members() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Direction",
        vec![
            make_member("Up", MemberKind::EnumMember),
            make_member("Down", MemberKind::EnumMember),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(1),
        path: PathBuf::from("/src/enums.ts"),
        member_accesses: vec![MemberAccess {
            object: "this".to_string(),
            member: "Up".to_string(),
        }],
        ..Default::default()
    }];

    let (enum_members, _) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(enum_members.len(), 2);
}

#[test]
fn mixed_enum_and_class_in_same_module() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/mixed.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![
        make_export_with_members(
            "Status",
            vec![make_member("Active", MemberKind::EnumMember)],
            Some(0),
        ),
        make_export_with_members(
            "Service",
            vec![make_member("doWork", MemberKind::ClassMethod)],
            Some(0),
        ),
    ];

    let (enum_members, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(enum_members.len(), 1);
    assert_eq!(enum_members[0].parent_name, "Status");
    assert_eq!(class_members.len(), 1);
    assert_eq!(class_members[0].parent_name, "Service");
}

#[test]
fn local_name_mapped_to_imported_name() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Status",
        vec![
            make_member("Active", MemberKind::EnumMember),
            make_member("Inactive", MemberKind::EnumMember),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/src/entry.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./enums".to_string(),
                imported_name: ImportedName::Named("Status".to_string()),
                local_name: "S".to_string(), // aliased
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        member_accesses: vec![MemberAccess {
            object: "S".to_string(), // uses local alias
            member: "Active".to_string(),
        }],
        ..Default::default()
    }];

    let (enum_members, _) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(enum_members.len(), 1);
    assert_eq!(enum_members[0].member_name, "Inactive");
}

#[test]
fn default_import_maps_to_default_export() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "default",
        vec![
            make_member("X", MemberKind::EnumMember),
            make_member("Y", MemberKind::EnumMember),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/src/entry.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./enums".to_string(),
                imported_name: ImportedName::Default,
                local_name: "MyEnum".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        member_accesses: vec![MemberAccess {
            object: "MyEnum".to_string(),
            member: "X".to_string(),
        }],
        ..Default::default()
    }];

    let (enum_members, _) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(enum_members.len(), 1);
    assert_eq!(enum_members[0].member_name, "Y");
}

#[test]
fn suppressed_enum_member_not_flagged() {
    use crate::suppress::{IssueKind, Suppression};

    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Status",
        vec![make_member("Active", MemberKind::EnumMember)],
        Some(0),
    )];

    let supps = vec![Suppression::issue(1, 0, IssueKind::UnusedEnumMember)];
    let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
    supp_map.insert(FileId(1), &supps);
    let suppressions = SuppressionContext::from_map(supp_map);

    let (enum_members, _) = find_unused_members(
        &graph,
        &[],
        &[],
        &suppressions,
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(
        enum_members.is_empty(),
        "suppressed enum member should not be flagged"
    );
}

#[test]
fn suppressed_class_member_not_flagged() {
    use crate::suppress::{IssueKind, Suppression};

    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/service.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Service",
        vec![make_member("doWork", MemberKind::ClassMethod)],
        Some(0),
    )];

    let supps = vec![Suppression::issue(1, 0, IssueKind::UnusedClassMember)];
    let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
    supp_map.insert(FileId(1), &supps);
    let suppressions = SuppressionContext::from_map(supp_map);

    let (_, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &suppressions,
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(
        class_members.is_empty(),
        "suppressed class member should not be flagged"
    );
}

#[test]
fn whole_object_use_via_aliased_import() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Status",
        vec![
            make_member("A", MemberKind::EnumMember),
            make_member("B", MemberKind::EnumMember),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/src/entry.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./enums".to_string(),
                imported_name: ImportedName::Named("Status".to_string()),
                local_name: "S".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        whole_object_uses: vec!["S".to_string()].into(), // aliased local name
        ..Default::default()
    }];

    let (enum_members, _) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(
        enum_members.is_empty(),
        "whole object use via alias should suppress all members"
    );
}

#[test]
fn this_field_chained_access_not_flagged() {
    let mut graph = build_graph(&[("/src/main.ts", true), ("/src/service.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "MyService",
        vec![
            make_member("doWork", MemberKind::ClassMethod),
            make_member("unusedMethod", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/src/main.ts"),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./service".to_string(),
                imported_name: ImportedName::Named("MyService".to_string()),
                local_name: "MyService".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 30),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        }],
        member_accesses: vec![MemberAccess {
            object: "MyService".to_string(),
            member: "doWork".to_string(),
        }],
        ..Default::default()
    }];

    let (_, class_members) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert_eq!(class_members.len(), 1);
    assert_eq!(class_members[0].member_name, "unusedMethod");
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn interface_member_usage_propagates_to_implementers() {
    let mut graph = build_graph(&[
        ("/src/main.ts", true),
        ("/src/scroll-strategy.interface.ts", false),
        ("/src/fixed-size-strategy.ts", false),
        ("/src/scroll-viewport.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[2].set_reachable(true);
    graph.modules[3].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "VirtualScrollStrategy",
        vec![],
        Some(3),
    )];
    graph.modules[2].exports = vec![make_export_with_members(
        "FixedSizeScrollStrategy",
        vec![
            make_member("attached", MemberKind::ClassProperty),
            make_member("attach", MemberKind::ClassMethod),
            make_member("detach", MemberKind::ClassMethod),
            make_member("unusedHelper", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let modules = vec![make_module_with_class_heritage(
        2,
        "FixedSizeScrollStrategy",
        None,
        &["VirtualScrollStrategy"],
    )];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(2),
            path: PathBuf::from("/src/fixed-size-strategy.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./scroll-strategy.interface".to_string(),
                    imported_name: ImportedName::Named("VirtualScrollStrategy".to_string()),
                    local_name: "VirtualScrollStrategy".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/src/scroll-viewport.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./scroll-strategy.interface".to_string(),
                    imported_name: ImportedName::Named("VirtualScrollStrategy".to_string()),
                    local_name: "VirtualScrollStrategy".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![
                MemberAccess {
                    object: "VirtualScrollStrategy".to_string(),
                    member: "attach".to_string(),
                },
                MemberAccess {
                    object: "VirtualScrollStrategy".to_string(),
                    member: "attached".to_string(),
                },
                MemberAccess {
                    object: "VirtualScrollStrategy".to_string(),
                    member: "detach".to_string(),
                },
            ],
            ..Default::default()
        },
    ];

    let (_, class_members) = find_unused_members(
        &graph,
        &resolved_modules,
        &modules,
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );

    let unused_names: FxHashSet<String> = class_members
        .iter()
        .map(|member| format!("{}.{}", member.parent_name, member.member_name))
        .collect();

    assert!(
        !unused_names.contains("FixedSizeScrollStrategy.attach"),
        "attach should be credited through interface usage: {unused_names:?}"
    );
    assert!(
        !unused_names.contains("FixedSizeScrollStrategy.attached"),
        "attached should be credited through interface usage: {unused_names:?}"
    );
    assert!(
        !unused_names.contains("FixedSizeScrollStrategy.detach"),
        "detach should be credited through interface usage: {unused_names:?}"
    );
    assert!(
        unused_names.contains("FixedSizeScrollStrategy.unusedHelper"),
        "unrelated members should still be reported: {unused_names:?}"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn same_named_interfaces_do_not_share_member_usage() {
    let mut graph = build_graph(&[
        ("/src/main.ts", true),
        ("/src/one-interface.ts", false),
        ("/src/two-interface.ts", false),
        ("/src/one-impl.ts", false),
        ("/src/two-impl.ts", false),
        ("/src/consumer.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[2].set_reachable(true);
    graph.modules[3].set_reachable(true);
    graph.modules[4].set_reachable(true);
    graph.modules[5].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members("Strategy", vec![], Some(5))];
    graph.modules[2].exports = vec![make_export_with_members("Strategy", vec![], Some(0))];
    graph.modules[3].exports = vec![make_export_with_members(
        "OneStrategy",
        vec![make_member("attach", MemberKind::ClassMethod)],
        Some(0),
    )];
    graph.modules[4].exports = vec![make_export_with_members(
        "TwoStrategy",
        vec![make_member("attach", MemberKind::ClassMethod)],
        Some(0),
    )];

    let modules = vec![
        make_module_with_class_heritage(3, "OneStrategy", None, &["Strategy"]),
        make_module_with_class_heritage(4, "TwoStrategy", None, &["Strategy"]),
    ];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(3),
            path: PathBuf::from("/src/one-impl.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./one-interface".to_string(),
                    imported_name: ImportedName::Named("Strategy".to_string()),
                    local_name: "Strategy".to_string(),
                    is_type_only: true,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(4),
            path: PathBuf::from("/src/two-impl.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./two-interface".to_string(),
                    imported_name: ImportedName::Named("Strategy".to_string()),
                    local_name: "Strategy".to_string(),
                    is_type_only: true,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            }],
            ..Default::default()
        },
        ResolvedModule {
            file_id: FileId(5),
            path: PathBuf::from("/src/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./one-interface".to_string(),
                    imported_name: ImportedName::Named("Strategy".to_string()),
                    local_name: "Strategy".to_string(),
                    is_type_only: true,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![MemberAccess {
                object: "Strategy".to_string(),
                member: "attach".to_string(),
            }],
            ..Default::default()
        },
    ];

    let (_, class_members) = find_unused_members(
        &graph,
        &resolved_modules,
        &modules,
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );

    let unused_names: FxHashSet<String> = class_members
        .iter()
        .map(|member| format!("{}.{}", member.parent_name, member.member_name))
        .collect();

    assert!(
        !unused_names.contains("OneStrategy.attach"),
        "OneStrategy.attach should be credited through its own interface export: {unused_names:?}"
    );
    assert!(
        unused_names.contains("TwoStrategy.attach"),
        "TwoStrategy.attach should remain unused when only the other interface export is used: {unused_names:?}"
    );
}

#[test]
fn same_named_exports_do_not_share_member_usage() {
    let mut graph = build_graph(&[
        ("/src/entry.ts", true),
        ("/src/one.ts", false),
        ("/src/two.ts", false),
    ]);
    graph.modules[1].set_reachable(true);
    graph.modules[2].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "Widget",
        vec![
            make_member("refresh", MemberKind::ClassMethod),
            make_member("unusedOne", MemberKind::ClassMethod),
        ],
        Some(0),
    )];
    graph.modules[2].exports = vec![make_export_with_members(
        "Widget",
        vec![
            make_member("refresh", MemberKind::ClassMethod),
            make_member("unusedTwo", MemberKind::ClassMethod),
        ],
        Some(0),
    )];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/src/entry.ts"),
        resolved_imports: vec![
            ResolvedImport {
                info: ImportInfo {
                    source: "./one".to_string(),
                    imported_name: ImportedName::Named("Widget".to_string()),
                    local_name: "FirstWidget".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            },
            ResolvedImport {
                info: ImportInfo {
                    source: "./two".to_string(),
                    imported_name: ImportedName::Named("Widget".to_string()),
                    local_name: "SecondWidget".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(31, 62),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(2)),
            },
        ],
        member_accesses: vec![MemberAccess {
            object: "FirstWidget".to_string(),
            member: "refresh".to_string(),
        }],
        ..Default::default()
    }];

    let (_, class_members) = find_unused_members(
        &graph,
        &resolved_modules,
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );

    let unused_members: FxHashSet<(String, String)> = class_members
        .iter()
        .map(|member| {
            (
                member.path.display().to_string(),
                format!("{}.{}", member.parent_name, member.member_name),
            )
        })
        .collect();

    assert_eq!(
        unused_members.len(),
        3,
        "unexpected members: {unused_members:?}"
    );
    assert!(unused_members.contains(&("/src/one.ts".to_string(), "Widget.unusedOne".to_string())));
    assert!(unused_members.contains(&("/src/two.ts".to_string(), "Widget.refresh".to_string())));
    assert!(unused_members.contains(&("/src/two.ts".to_string(), "Widget.unusedTwo".to_string())));
    assert!(
        !unused_members.contains(&("/src/one.ts".to_string(), "Widget.refresh".to_string())),
        "member usage from /src/one.ts should not leak into /src/two.ts: {unused_members:?}"
    );
}

#[test]
fn export_with_no_members_skipped() {
    let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/utils.ts", false)]);
    graph.modules[1].set_reachable(true);
    graph.modules[1].exports = vec![make_export_with_members(
        "helper",
        vec![], // no members
        Some(0),
    )];

    let (enum_members, class_members) = find_unused_members(
        &graph,
        &[],
        &[],
        &SuppressionContext::empty(),
        &FxHashMap::default(),
        &[],
        &[],
    );
    assert!(enum_members.is_empty());
    assert!(class_members.is_empty());
}
