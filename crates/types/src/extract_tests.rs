use super::*;

fn span() -> Span {
    Span::new(0, 1)
}

macro_rules! assert_released {
    ($values:expr) => {{
        assert!($values.is_empty());
    }};
}

#[test]
fn only_path_and_asset_references_do_not_run_their_target() {
    use ImportLoadKind::{
        AssetReference, Dynamic, DynamicPattern, OutOfThread, PathReference, Static,
    };
    // (kind, loads_target, runs_target_code, eager value when not type-only)
    let table = [
        (Static, true, true, true),
        (Dynamic, true, true, false),
        (DynamicPattern, true, true, false),
        (OutOfThread, true, true, false),
        (PathReference, false, true, false),
        (AssetReference, false, false, false),
    ];
    for (kind, loads, runs, eager) in table {
        assert_eq!(kind.loads_target(), loads, "{kind:?} loads_target");
        assert_eq!(kind.runs_target_code(), runs, "{kind:?} runs_target_code");
        assert_eq!(kind.is_eager_value(false), eager, "{kind:?} is_eager_value");
        assert!(
            !kind.is_eager_value(true),
            "a type-only {kind:?} carries no value"
        );
    }
}

#[test]
fn public_env_var_includes_public_ci_metadata() {
    for name in ["TAG_REF", "GITHUB_SHA", "CI_COMMIT_BRANCH", "APP_MODE"] {
        assert!(is_public_env_var(name), "{name} should be public metadata");
    }
}

#[test]
fn public_env_var_keeps_secret_shaped_names_source_backed() {
    for name in ["GITHUB_TOKEN", "REFRESH_TOKEN", "API_KEY", "SECRET_SHA"] {
        assert!(
            !is_public_env_var(name),
            "{name} should remain secret-shaped"
        );
    }
}

#[test]
fn ordinary_access_helpers_keep_source_accesses() {
    let member_accesses = vec![
        MemberAccess {
            object: "this".to_string(),
            member: "render".to_string(),
        },
        MemberAccess {
            object: "service".to_string(),
            member: "run".to_string(),
        },
    ];
    let ordinary = SemanticFactView::new(&[], &member_accesses)
        .ordinary_member_accesses()
        .map(|access| (access.object.as_str(), access.member.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(ordinary, vec![("this", "render"), ("service", "run")]);

    let whole_object_uses = vec!["model".to_string(), "service".to_string()];

    assert_eq!(
        ordinary_whole_object_uses(&whole_object_uses).collect::<Vec<_>>(),
        vec!["model", "service"]
    );
}

#[test]
fn angular_template_member_names_use_typed_facts() {
    let mut module = minimal_module_info();
    push_semantic_fact(
        &mut module,
        SemanticFact::AngularTemplateMemberAccess(AngularTemplateMemberAccessFact {
            member: "typed".to_string(),
        }),
    );

    let names: Vec<&str> = angular_template_member_names(&module).collect();

    assert_eq!(names, vec!["typed"]);
    assert!(has_angular_template_members(&module));
}

#[test]
fn angular_this_spread_uses_typed_fact() {
    let mut typed = minimal_module_info();
    push_semantic_fact(
        &mut typed,
        SemanticFact::AngularThisSpread(AngularThisSpreadFact),
    );

    assert!(has_angular_this_spread(&typed));
    assert!(!has_angular_this_spread(&minimal_module_info()));
}

#[test]
fn semantic_fact_view_iterates_typed_facts() {
    let mut module = minimal_module_info();
    push_semantic_fact(
        &mut module,
        SemanticFact::FactoryCallMemberAccess(FactoryCallMemberAccessFact {
            callee_object: "Svc".to_string(),
            callee_method: "make".to_string(),
            member: "run".to_string(),
        }),
    );

    let facts = SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
        .facts()
        .collect::<Vec<_>>();

    assert_eq!(
        facts[0],
        &SemanticFact::FactoryCallMemberAccess(FactoryCallMemberAccessFact {
            callee_object: "Svc".to_string(),
            callee_method: "make".to_string(),
            member: "run".to_string(),
        })
    );
}

#[test]
fn typed_fact_helpers_collect_each_family() {
    let mut module = minimal_module_info();
    push_semantic_fact(
        &mut module,
        SemanticFact::InstanceExportBinding(InstanceExportBindingFact {
            export_name: "exported".to_string(),
            target_name: "target".to_string(),
        }),
    );
    push_semantic_fact(
        &mut module,
        SemanticFact::FactoryCallMemberAccess(FactoryCallMemberAccessFact {
            callee_object: "Svc".to_string(),
            callee_method: "create".to_string(),
            member: "run".to_string(),
        }),
    );
    push_semantic_fact(
        &mut module,
        SemanticFact::FluentChainMemberAccess(FluentChainMemberAccessFact {
            root_object: "Builder".to_string(),
            root_method: "start".to_string(),
            chain: vec!["next".to_string()],
            member: "value".to_string(),
        }),
    );
    push_semantic_fact(
        &mut module,
        SemanticFact::FluentChainNewMemberAccess(FluentChainNewMemberAccessFact {
            class_name: "Builder".to_string(),
            chain: vec!["next".to_string(), "finish".to_string()],
            member: "done".to_string(),
        }),
    );

    assert_eq!(
        SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
            .instance_export_bindings(),
        vec![InstanceExportBindingFact {
            export_name: "exported".to_string(),
            target_name: "target".to_string(),
        }]
    );
    assert_eq!(
        SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
            .factory_call_member_accesses(),
        vec![FactoryCallMemberAccessFact {
            callee_object: "Svc".to_string(),
            callee_method: "create".to_string(),
            member: "run".to_string(),
        }]
    );
    assert_eq!(
        SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
            .fluent_chain_member_accesses(),
        vec![FluentChainMemberAccessFact {
            root_object: "Builder".to_string(),
            root_method: "start".to_string(),
            chain: vec!["next".to_string()],
            member: "value".to_string(),
        }]
    );
    assert_eq!(
        SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
            .fluent_chain_new_member_accesses(),
        vec![FluentChainNewMemberAccessFact {
            class_name: "Builder".to_string(),
            chain: vec!["next".to_string(), "finish".to_string()],
            member: "done".to_string(),
        }]
    );
}

#[test]
fn semantic_fact_view_exposes_typed_first_contract() {
    let mut module = minimal_module_info();
    push_semantic_fact(
        &mut module,
        SemanticFact::FactoryCallMemberAccess(FactoryCallMemberAccessFact {
            callee_object: "Svc".to_string(),
            callee_method: "create".to_string(),
            member: "run".to_string(),
        }),
    );
    push_semantic_fact(
        &mut module,
        SemanticFact::PlaywrightFixtureUse(PlaywrightFixtureUseFact {
            test_name: "test".to_string(),
            fixture_name: "page".to_string(),
            member: "goto".to_string(),
        }),
    );
    push_semantic_fact(
        &mut module,
        SemanticFact::InstanceExportBinding(InstanceExportBindingFact {
            export_name: "exported".to_string(),
            target_name: "target".to_string(),
        }),
    );

    let view = SemanticFactView::new(&module.semantic_facts, &module.member_accesses);

    assert_eq!(
        view.factory_call_member_accesses(),
        vec![FactoryCallMemberAccessFact {
            callee_object: "Svc".to_string(),
            callee_method: "create".to_string(),
            member: "run".to_string(),
        }]
    );
    assert_eq!(
        view.playwright_fixture_uses(),
        vec![PlaywrightFixtureUseFact {
            test_name: "test".to_string(),
            fixture_name: "page".to_string(),
            member: "goto".to_string(),
        }]
    );
    assert_eq!(
        view.instance_export_bindings(),
        vec![InstanceExportBindingFact {
            export_name: "exported".to_string(),
            target_name: "target".to_string(),
        }]
    );
}

#[test]
fn playwright_fixture_fact_helpers_select_each_fact_family() {
    let mut module = minimal_module_info();
    push_semantic_fact(
        &mut module,
        SemanticFact::PlaywrightFixtureUse(PlaywrightFixtureUseFact {
            test_name: "test".to_string(),
            fixture_name: "page".to_string(),
            member: "goto".to_string(),
        }),
    );
    push_semantic_fact(
        &mut module,
        SemanticFact::PlaywrightFixtureDefinition(PlaywrightFixtureDefinitionFact {
            test_name: "test".to_string(),
            fixture_name: "adminPage".to_string(),
            type_name: "AdminPage".to_string(),
        }),
    );
    push_semantic_fact(
        &mut module,
        SemanticFact::PlaywrightFixtureAlias(PlaywrightFixtureAliasFact {
            test_name: "mergedTest".to_string(),
            base_name: "test".to_string(),
        }),
    );
    push_semantic_fact(
        &mut module,
        SemanticFact::PlaywrightFixtureType(PlaywrightFixtureTypeFact {
            alias_name: "Pages".to_string(),
            fixture_name: "adminPage".to_string(),
            type_name: "AdminPage".to_string(),
        }),
    );

    assert_eq!(
        playwright_fixture_use_facts(&module.semantic_facts)
            .map(|fact| fact.member.as_str())
            .collect::<Vec<_>>(),
        vec!["goto"]
    );
    assert_eq!(
        playwright_fixture_definition_facts(&module.semantic_facts)
            .map(|fact| fact.type_name.as_str())
            .collect::<Vec<_>>(),
        vec!["AdminPage"]
    );
    assert_eq!(
        playwright_fixture_alias_facts(&module.semantic_facts)
            .map(|fact| fact.base_name.as_str())
            .collect::<Vec<_>>(),
        vec!["test"]
    );
    assert_eq!(
        playwright_fixture_type_facts(&module.semantic_facts)
            .map(|fact| fact.fixture_name.as_str())
            .collect::<Vec<_>>(),
        vec!["adminPage"]
    );
}

#[test]
fn line_offsets_empty_string() {
    assert_eq!(compute_line_offsets(""), vec![0]);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive field-by-field construction + release assertions for every ModuleInfo field"
)]
fn release_resolution_payload_drops_copied_vectors_only() {
    let mut module = ModuleInfo {
        file_id: FileId(7),
        exports: vec![ExportInfo {
            name: ExportName::Named("kept".to_string()),
            local_name: None,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: span(),
            members: Vec::new(),
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        }]
        .into(),
        imports: vec![ImportInfo {
            source: "node:child_process".to_string(),
            imported_name: ImportedName::Default,
            local_name: "childProcess".to_string(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: span(),
            source_span: span(),
        }],
        re_exports: vec![ReExportInfo {
            source: "./kept".to_string(),
            imported_name: "kept".to_string(),
            exported_name: "kept".to_string(),
            is_type_only: false,
            span: span(),
            statement_span: span(),
            source_span: span(),
        }],
        dynamic_imports: vec![DynamicImportInfo {
            source: "./dynamic".to_string(),
            span: span(),
            destructured_names: vec!["value".to_string()],
            local_name: None,
            is_speculative: false,
            value_escapes: false,
        }],
        dynamic_import_patterns: vec![DynamicImportPattern {
            prefix: "./pages/".to_string(),
            suffix: Some(".tsx".to_string()),
            span: span(),
            mechanism: ModuleLoadMechanism::EsModule,
            excludes: Vec::new(),
        }],
        require_calls: vec![RequireCallInfo {
            source: "./required".to_string(),
            span: span(),
            source_span: span(),
            destructured_names: Vec::new(),
            local_name: Some("required".to_string()),
            is_type_only: false,
        }],
        package_path_references: vec!["react".to_string()].into(),
        type_package_references: vec!["react".to_string()].into(),
        bin_path_references: vec!["vite".to_string()].into(),
        package_resolve_sites: vec![("react".to_string(), 0)].into(),
        member_accesses: vec![MemberAccess {
            object: "Status".to_string(),
            member: "Active".to_string(),
        }]
        .into(),
        semantic_facts: std::sync::Arc::default(),
        whole_object_uses: vec!["Status".to_string()].into(),
        has_cjs_exports: true,
        has_angular_component_template_url: true,
        content_hash: 42,
        parse_error_count: 0,
        parse_panicked: false,
        suppressions: Vec::new(),
        unknown_suppression_kinds: Vec::new(),
        unused_import_bindings: vec!["unused".to_string()],
        type_referenced_import_bindings: vec!["TypeOnly".to_string()],
        value_referenced_import_bindings: vec!["Value".to_string()],
        line_offsets: vec![0, 8],
        complexity: vec![FunctionComplexity {
            name: "work".to_string(),
            is_private_member: false,
            context_named: None,
            line: 1,
            col: 0,
            cyclomatic: 2,
            cognitive: 3,
            line_count: 4,
            param_count: 1,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: Some("hash".to_string()),
            contributions: Vec::new(),
        }],
        flag_uses: vec![FlagUse {
            flag_name: "FEATURE_X".to_string(),
            kind: FlagUseKind::EnvVar,
            line: 1,
            col: 0,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
            facts: FlagSiteFacts::default(),
        }],
        flag_registry_facts: None,
        class_heritage: vec![ClassHeritageInfo {
            export_name: "Child".to_string(),
            super_class: Some("Parent".to_string()),
            implements: vec!["Contract".to_string()],
            type_parameters: Vec::new(),
            instance_bindings: Vec::new(),
            super_class_type_args: Vec::new(),
            generic_instance_bindings: Vec::new(),
        }],
        exported_factory_returns: std::sync::Arc::from([FactoryReturnExport {
            export_name: "useApi".to_string(),
            class_local_name: "RESTApi".to_string(),
            is_async: false,
        }]),
        exported_factory_return_object_shapes: std::sync::Arc::from([
            FactoryReturnObjectShapeExport {
                export_name: "createUi".to_string(),
                properties: Box::from([FactoryReturnObjectProperty {
                    property_path: "orders".to_string(),
                    class_local_name: "OrdersPage".to_string(),
                }]),
            },
        ]),
        type_member_types: std::sync::Arc::from([TypeMemberTypeEntry {
            type_name: "Opts".to_string(),
            property: "c".to_string(),
            property_type: "OptDep".to_string(),
        }]),
        injection_tokens: vec![("TOKEN".to_string(), "Contract".to_string())],
        local_type_declarations: vec![LocalTypeDeclaration {
            name: "Contract".to_string(),
            span: span(),
        }],
        public_signature_type_references: vec![PublicSignatureTypeReference {
            export_name: "kept".to_string(),
            type_name: "Contract".to_string(),
            span: span(),
            from_satisfies: false,
        }],
        namespace_object_aliases: vec![NamespaceObjectAlias {
            via_export_name: "api".to_string(),
            suffix: "read".to_string(),
            namespace_local: "ns".to_string(),
        }],
        iconify_prefixes: vec!["hero".to_string()],
        iconify_icon_names: vec!["hero-home".to_string()],
        auto_import_candidates: vec!["useState".to_string()],
        directives: vec!["use client".to_string()],
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 1,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        imported_call_sites: Arc::default(),
        import_binding_references: Arc::default(),
        misplaced_directives: Vec::new(),
        inline_server_action_exports: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        is_server_action_module: false,
        has_global_declarations: false,
        triple_slash_reference_paths: Box::default(),
        jsx_runtime_from_config: false,
        referenced_import_bindings: Vec::new(),
        component_props: Vec::new(),
        component_contracts: None,
        effect_facts: None,
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        angular_inputs: Vec::new(),
        angular_outputs: Vec::new(),
        angular_component_selectors: Vec::new(),
        registered_custom_elements: Vec::new(),
        used_custom_element_tags: Vec::new(),
        angular_used_selectors: Vec::new(),
        angular_entry_component_refs: Vec::new(),
        has_dynamic_component_render: false,
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
        has_dynamic_dispatch: false,
    };

    module.release_resolution_payload();

    assert_eq!(module.file_id, FileId(7));
    assert_eq!(module.content_hash, 42);
    assert_eq!(module.line_offsets, vec![0, 8]);
    assert_eq!(module.imports.len(), 1);
    assert_eq!(module.exports.len(), 1);
    assert_eq!(module.re_exports.len(), 1);
    assert_eq!(module.dynamic_import_patterns.len(), 1);
    assert_eq!(module.member_accesses.len(), 1);
    assert_eq!(module.complexity.len(), 1);
    assert_eq!(module.flag_uses.len(), 1);
    assert_eq!(module.class_heritage.len(), 1);
    assert_eq!(module.exported_factory_returns.len(), 1);
    assert_eq!(module.injection_tokens.len(), 1);
    assert_eq!(module.local_type_declarations.len(), 1);
    assert_eq!(module.public_signature_type_references.len(), 1);
    assert_eq!(module.iconify_prefixes.len(), 1);
    assert_eq!(module.iconify_icon_names.len(), 1);
    assert_eq!(module.directives.len(), 1);
    assert_eq!(module.security_sinks_skipped, 1);
    assert_eq!(module.package_resolve_sites.len(), 1);
    assert_released!(module.dynamic_imports);
    assert_released!(module.require_calls);
    assert_released!(module.package_path_references);
    assert_released!(module.type_package_references);
    assert_released!(module.bin_path_references);
    assert_released!(module.whole_object_uses);
    assert_released!(module.unused_import_bindings);
    assert_released!(module.type_referenced_import_bindings);
    assert_released!(module.value_referenced_import_bindings);
    assert_released!(module.namespace_object_aliases);
    assert_released!(module.auto_import_candidates);
    assert_eq!(
        module.referenced_import_bindings,
        vec!["childProcess".to_string()]
    );
}

#[test]
fn line_offsets_single_line_no_newline() {
    assert_eq!(compute_line_offsets("hello"), vec![0]);
}

#[test]
fn line_offsets_single_line_with_newline() {
    assert_eq!(compute_line_offsets("hello\n"), vec![0, 6]);
}

#[test]
fn line_offsets_multiple_lines() {
    assert_eq!(compute_line_offsets("abc\ndef\nghi"), vec![0, 4, 8]);
}

#[test]
fn line_offsets_trailing_newline() {
    assert_eq!(compute_line_offsets("abc\ndef\n"), vec![0, 4, 8]);
}

#[test]
fn line_offsets_consecutive_newlines() {
    assert_eq!(compute_line_offsets("\n\n\n"), vec![0, 1, 2, 3]);
}

#[test]
fn line_offsets_multibyte_utf8() {
    assert_eq!(compute_line_offsets("á\n"), vec![0, 3]);
}

#[test]
fn line_col_offset_zero() {
    let offsets = compute_line_offsets("abc\ndef\nghi");
    let (line, col) = byte_offset_to_line_col(&offsets, 0);
    assert_eq!((line, col), (1, 0));
}

#[test]
fn line_col_middle_of_first_line() {
    let offsets = compute_line_offsets("abc\ndef\nghi");
    let (line, col) = byte_offset_to_line_col(&offsets, 2);
    assert_eq!((line, col), (1, 2));
}

#[test]
fn line_col_start_of_second_line() {
    let offsets = compute_line_offsets("abc\ndef\nghi");
    let (line, col) = byte_offset_to_line_col(&offsets, 4);
    assert_eq!((line, col), (2, 0));
}

#[test]
fn line_col_middle_of_second_line() {
    let offsets = compute_line_offsets("abc\ndef\nghi");
    let (line, col) = byte_offset_to_line_col(&offsets, 5);
    assert_eq!((line, col), (2, 1));
}

#[test]
fn line_col_start_of_third_line() {
    let offsets = compute_line_offsets("abc\ndef\nghi");
    let (line, col) = byte_offset_to_line_col(&offsets, 8);
    assert_eq!((line, col), (3, 0));
}

#[test]
fn line_col_end_of_file() {
    let offsets = compute_line_offsets("abc\ndef\nghi");
    let (line, col) = byte_offset_to_line_col(&offsets, 10);
    assert_eq!((line, col), (3, 2));
}

#[test]
fn line_col_single_line() {
    let offsets = compute_line_offsets("hello");
    let (line, col) = byte_offset_to_line_col(&offsets, 3);
    assert_eq!((line, col), (1, 3));
}

#[test]
fn line_col_at_newline_byte() {
    let offsets = compute_line_offsets("abc\ndef");
    let (line, col) = byte_offset_to_line_col(&offsets, 3);
    assert_eq!((line, col), (1, 3));
}

/// Columns count bytes, not chars: a 4-byte emoji advances the column by 4.
#[test]
fn line_col_counts_bytes_after_emoji() {
    let offsets = compute_line_offsets("hi\n\u{1F600}x");
    assert_eq!(byte_offset_to_line_col(&offsets, 3), (2, 0));
    assert_eq!(byte_offset_to_line_col(&offsets, 7), (2, 4));
}

/// Columns count bytes, not chars: a 2-byte accented char advances the column by 2.
#[test]
fn line_col_counts_bytes_after_accented_char() {
    let offsets = compute_line_offsets("caf\u{00E9}\nbar");
    assert_eq!(byte_offset_to_line_col(&offsets, 3), (1, 3));
    assert_eq!(byte_offset_to_line_col(&offsets, 5), (1, 5));
    assert_eq!(byte_offset_to_line_col(&offsets, 6), (2, 0));
}

#[test]
fn export_name_matches_str_named() {
    let name = ExportName::Named("foo".to_string());
    assert!(name.matches_str("foo"));
    assert!(!name.matches_str("bar"));
    assert!(!name.matches_str("default"));
}

#[test]
fn export_name_matches_str_default() {
    let name = ExportName::Default;
    assert!(name.matches_str("default"));
    assert!(!name.matches_str("foo"));
}

#[test]
fn export_name_display_named() {
    let name = ExportName::Named("myExport".to_string());
    assert_eq!(name.to_string(), "myExport");
}

#[test]
fn export_name_display_default() {
    let name = ExportName::Default;
    assert_eq!(name.to_string(), "default");
}

#[test]
fn export_name_matches_str_empty_string() {
    let name = ExportName::Named(String::new());
    assert!(name.matches_str(""));
    assert!(!name.matches_str("foo"));
}

#[test]
fn export_name_default_does_not_match_empty() {
    let name = ExportName::Default;
    assert!(!name.matches_str(""));
}

#[test]
fn line_offsets_crlf_only_counts_lf() {
    let offsets = compute_line_offsets("ab\r\ncd");
    assert_eq!(offsets, vec![0, 4]);
}

#[test]
fn line_col_empty_file_offset_zero() {
    let offsets = compute_line_offsets("");
    let (line, col) = byte_offset_to_line_col(&offsets, 0);
    assert_eq!((line, col), (1, 0));
}

// --- VisibilityTag ---

#[test]
fn visibility_tag_default_is_none_variant() {
    assert_eq!(VisibilityTag::default(), VisibilityTag::None);
}

#[test]
fn visibility_tag_is_none_only_for_none_variant() {
    assert!(VisibilityTag::None.is_none());
    assert!(!VisibilityTag::Public.is_none());
    assert!(!VisibilityTag::Internal.is_none());
    assert!(!VisibilityTag::Beta.is_none());
    assert!(!VisibilityTag::Alpha.is_none());
    assert!(!VisibilityTag::ExpectedUnused.is_none());
}

#[test]
fn visibility_tag_suppresses_unused_for_api_tags() {
    assert!(VisibilityTag::Public.suppresses_unused());
    assert!(VisibilityTag::Internal.suppresses_unused());
    assert!(VisibilityTag::Beta.suppresses_unused());
    assert!(VisibilityTag::Alpha.suppresses_unused());
}

#[test]
fn visibility_tag_does_not_suppress_none_or_expected_unused() {
    assert!(!VisibilityTag::None.suppresses_unused());
    assert!(!VisibilityTag::ExpectedUnused.suppresses_unused());
}

// --- is_public_env_path ---

#[test]
fn is_public_env_path_process_env_public_prefix() {
    assert!(is_public_env_path("process.env.NEXT_PUBLIC_API_URL"));
    assert!(is_public_env_path("process.env.VITE_APP_KEY"));
    assert!(is_public_env_path("process.env.REACT_APP_TITLE"));
    assert!(is_public_env_path("process.env.NODE_ENV"));
}

#[test]
fn is_public_env_path_import_meta_env_public_prefix() {
    assert!(is_public_env_path("import.meta.env.VITE_BASE_URL"));
    assert!(is_public_env_path("import.meta.env.PUBLIC_API"));
}

#[test]
fn is_public_env_path_secret_env_vars_are_not_public() {
    assert!(!is_public_env_path("process.env.SECRET_KEY"));
    assert!(!is_public_env_path("process.env.DATABASE_PASSWORD"));
    assert!(!is_public_env_path("import.meta.env.API_TOKEN"));
}

#[test]
fn is_public_env_path_non_env_paths_are_not_public() {
    assert!(!is_public_env_path("req.query.id"));
    assert!(!is_public_env_path("process.argv"));
    assert!(!is_public_env_path("window.location.href"));
}

// --- is_public_env_var edge cases ---

#[test]
fn is_public_env_var_exact_matches() {
    assert!(is_public_env_var("NODE_ENV"));
}

#[test]
fn is_public_env_var_all_known_prefixes() {
    assert!(is_public_env_var("NUXT_PUBLIC_API_URL"));
    assert!(is_public_env_var("PUBLIC_API_KEY"));
    assert!(is_public_env_var("GATSBY_APP_ID"));
    assert!(is_public_env_var("EXPO_PUBLIC_SENTRY_DSN"));
    assert!(is_public_env_var("STORYBOOK_ENV"));
}

#[test]
fn is_public_env_var_secret_token_beats_metadata_token() {
    // "SECRET_SHA": has SECRET (wins) and SHA (metadata); should NOT be public
    assert!(!is_public_env_var("SECRET_SHA"));
    // "REF_TOKEN": has TOKEN (secret) and REF (metadata); should NOT be public
    assert!(!is_public_env_var("REF_TOKEN"));
}

#[test]
fn is_public_env_var_plain_unknown_names_are_not_public() {
    assert!(!is_public_env_var("MY_SERVICE_URL"));
    assert!(!is_public_env_var("FEATURE_FLAG"));
    assert!(!is_public_env_var("DATABASE_URL"));
}

// --- SinkSite::span ---

#[test]
fn sink_site_span_reconstructs_from_offsets() {
    let site = SinkSite {
        sink_shape: SinkShape::Call,
        callee_path: "eval".to_string(),
        arg_index: 0,
        arg_is_non_literal: true,
        arg_kind: SinkArgKind::Other,
        arg_literal: None,
        regex_pattern: None,
        object_properties: Vec::new(),
        object_property_keys: Vec::new(),
        object_property_keys_complete: false,
        arg_idents: Vec::new(),
        arg_source_paths: Vec::new(),
        span_start: 5,
        span_end: 15,
        url_arg_literal: None,
        url_shape: None,
    };
    let s = site.span();
    assert_eq!(s.start, 5);
    assert_eq!(s.end, 15);
}

// --- SecurityControlKind ---

#[test]
fn security_control_kind_ordering() {
    assert!(SecurityControlKind::Sanitization < SecurityControlKind::Validation);
    assert!(SecurityControlKind::Authentication < SecurityControlKind::Authorization);
}

// --- SanitizerScope ---

#[test]
fn sanitizer_scope_ordering() {
    assert!(SanitizerScope::Html < SanitizerScope::Url);
}

// --- release_resolution_payload: page data store whole-use derivation ---

#[test]
fn release_payload_derives_page_data_store_whole_use_from_page_data() {
    let mut m = minimal_module_info();
    m.whole_object_uses = vec!["page.data".to_string()].into();
    m.release_resolution_payload();
    assert!(m.has_page_data_store_whole_use);
}

#[test]
fn release_payload_derives_page_data_store_whole_use_from_dollar_page_data() {
    let mut m = minimal_module_info();
    m.whole_object_uses = vec!["$page.data".to_string()].into();
    m.release_resolution_payload();
    assert!(m.has_page_data_store_whole_use);
}

#[test]
fn release_payload_does_not_set_page_data_store_whole_use_for_other_names() {
    let mut m = minimal_module_info();
    m.whole_object_uses = vec!["data".to_string(), "page".to_string()].into();
    m.release_resolution_payload();
    assert!(!m.has_page_data_store_whole_use);
}

#[test]
fn release_payload_derives_route_loader_data_whole_use() {
    let mut m = minimal_module_info();
    m.whole_object_uses = vec!["$fallow.routeLoaderData".to_string()].into();
    m.release_resolution_payload();
    assert!(m.has_route_loader_data_whole_use);
}

// --- release_resolution_payload: referenced_import_bindings derivation ---

#[test]
fn release_payload_referenced_bindings_excludes_empty_local_names() {
    let mut m = minimal_module_info();
    m.imports = vec![
        ImportInfo {
            source: "./styles.css".to_string(),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(), // empty = side-effect import
            is_type_only: false,
            is_type_only_star: false,
            from_style: true,
            span: span(),
            source_span: span(),
        },
        ImportInfo {
            source: "react".to_string(),
            imported_name: ImportedName::Default,
            local_name: "React".to_string(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: span(),
            source_span: span(),
        },
    ];
    m.unused_import_bindings = vec!["React".to_string()];
    m.release_resolution_payload();
    // "React" was unused, empty local is filtered; result should be empty
    assert!(m.referenced_import_bindings.is_empty());
}

#[test]
fn release_payload_referenced_bindings_sorted_and_deduped() {
    let mut m = minimal_module_info();
    // Two imports with the same local name (unusual but possible via re-exports)
    m.imports = vec![
        ImportInfo {
            source: "a".to_string(),
            imported_name: ImportedName::Named("foo".to_string()),
            local_name: "foo".to_string(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: span(),
            source_span: span(),
        },
        ImportInfo {
            source: "b".to_string(),
            imported_name: ImportedName::Named("bar".to_string()),
            local_name: "bar".to_string(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: span(),
            source_span: span(),
        },
        ImportInfo {
            source: "c".to_string(),
            imported_name: ImportedName::Named("foo".to_string()),
            local_name: "foo".to_string(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: span(),
            source_span: span(),
        },
    ];
    m.unused_import_bindings = Vec::new();
    m.release_resolution_payload();
    // sorted: ["bar", "foo"] with "foo" deduped
    assert_eq!(
        m.referenced_import_bindings,
        vec!["bar".to_string(), "foo".to_string()]
    );
}

// --- Helper to build a minimal ModuleInfo for targeted tests ---

fn minimal_module_info() -> ModuleInfo {
    ModuleInfo::empty(FileId(0))
}

fn push_semantic_fact(module: &mut ModuleInfo, fact: SemanticFact) {
    let mut facts = std::mem::take(&mut module.semantic_facts).to_vec();
    facts.push(fact);
    module.semantic_facts = facts.into();
}

#[test]
fn dynamic_custom_element_render_helper_prefers_typed_fact() {
    let mut module = minimal_module_info();
    push_semantic_fact(
        &mut module,
        SemanticFact::DynamicCustomElementRender(DynamicCustomElementRenderFact),
    );

    assert!(has_dynamic_custom_element_render(&module));
}
