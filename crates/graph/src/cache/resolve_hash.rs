//! Hash of the facts of one parsed module that import resolution reads.
//!
//! The graph cache reuses a resolver payload when this hash is equal for
//! every file, also when the content of a file changed. Two rules keep that
//! safe:
//!
//! - The hash covers each fact of a module that
//!   [`crate::resolve::resolve_all_imports`] reads. A new resolver input from
//!   `ModuleInfo` must join [`resolve_inputs_hash`]. A test of this module
//!   names each field of `ModuleInfo`, so a new field needs that decision.
//! - The hash covers each field of each value that
//!   [`super::restore_resolved_project`] copies from the payload in place of
//!   the fresh module, with its spans. The hashed structs are destructured
//!   without `..`, so a new field does not compile until it is hashed.
//!
//! Convention auto-imports are the exception: that pass reads the exports and
//! the body facts of every module, and this hash does not cover them. The
//! caller must not use this hash for a project with auto-import rules.

use std::hash::{Hash, Hasher};

use fallow_types::extract::{
    DynamicImportInfo, DynamicImportPattern, ImportInfo, ModuleInfo, ReExportInfo, RequireCallInfo,
    SemanticFact, VitestModuleMockAction, VitestModuleMockOperationFact,
};
use oxc_span::Span;
use xxhash_rust::xxh3::Xxh3;

/// The hash of a file that has no parsed module.
pub(super) const NO_MODULE_RESOLVE_HASH: u64 = 0;

/// Hash the facts of `module` that import resolution reads.
///
/// Never [`NO_MODULE_RESOLVE_HASH`], so a file with a module does not compare
/// equal to an unreadable file.
#[must_use]
pub(super) fn resolve_inputs_hash(module: &ModuleInfo) -> u64 {
    let mut hasher = Xxh3::new();
    hash_list(&mut hasher, &module.imports, hash_import);
    hash_list(&mut hasher, &module.require_calls, hash_require_call);
    hash_list(&mut hasher, &module.dynamic_imports, hash_dynamic_import);
    hash_list(
        &mut hasher,
        &module.dynamic_import_patterns,
        hash_dynamic_pattern,
    );
    hash_list(&mut hasher, &module.re_exports, hash_re_export);
    module.jsx_runtime_from_config.hash(&mut hasher);
    let mut mock_operations = 0usize;
    for fact in module.semantic_facts.iter() {
        if let SemanticFact::VitestModuleMockOperation(operation) = fact {
            hash_mock_operation(&mut hasher, operation);
            mock_operations += 1;
        }
    }
    mock_operations.hash(&mut hasher);
    match hasher.finish() {
        NO_MODULE_RESOLVE_HASH => 1,
        hash => hash,
    }
}

fn hash_list<T>(hasher: &mut Xxh3, items: &[T], hash_item: fn(&mut Xxh3, &T)) {
    items.len().hash(hasher);
    for item in items {
        hash_item(hasher, item);
    }
}

fn hash_span(hasher: &mut Xxh3, span: Span) {
    span.start.hash(hasher);
    span.end.hash(hasher);
}

fn hash_import(hasher: &mut Xxh3, import: &ImportInfo) {
    let ImportInfo {
        source,
        imported_name,
        local_name,
        is_type_only,
        is_type_only_star,
        from_style,
        span,
        source_span,
    } = import;
    source.hash(hasher);
    imported_name.hash(hasher);
    local_name.hash(hasher);
    is_type_only.hash(hasher);
    is_type_only_star.hash(hasher);
    from_style.hash(hasher);
    hash_span(hasher, *span);
    hash_span(hasher, *source_span);
}

fn hash_require_call(hasher: &mut Xxh3, call: &RequireCallInfo) {
    let RequireCallInfo {
        source,
        span,
        source_span,
        destructured_names,
        destructured_locals,
        local_name,
        is_type_only,
    } = call;
    source.hash(hasher);
    hash_span(hasher, *span);
    hash_span(hasher, *source_span);
    destructured_names.hash(hasher);
    destructured_locals.hash(hasher);
    local_name.hash(hasher);
    is_type_only.hash(hasher);
}

fn hash_dynamic_import(hasher: &mut Xxh3, import: &DynamicImportInfo) {
    let DynamicImportInfo {
        source,
        span,
        destructured_names,
        local_name,
        is_speculative,
        value_escapes,
    } = import;
    source.hash(hasher);
    hash_span(hasher, *span);
    destructured_names.hash(hasher);
    local_name.hash(hasher);
    is_speculative.hash(hasher);
    value_escapes.hash(hasher);
}

fn hash_dynamic_pattern(hasher: &mut Xxh3, pattern: &DynamicImportPattern) {
    let DynamicImportPattern {
        prefix,
        suffix,
        excludes,
        span,
        mechanism,
        imported_name,
    } = pattern;
    prefix.hash(hasher);
    suffix.hash(hasher);
    excludes.hash(hasher);
    hash_span(hasher, *span);
    (*mechanism as u8).hash(hasher);
    imported_name.hash(hasher);
}

fn hash_re_export(hasher: &mut Xxh3, re_export: &ReExportInfo) {
    let ReExportInfo {
        source,
        imported_name,
        exported_name,
        is_type_only,
        span,
        statement_span,
        source_span,
        visibility,
        expected_unused_reason,
    } = re_export;
    source.hash(hasher);
    imported_name.hash(hasher);
    exported_name.hash(hasher);
    is_type_only.hash(hasher);
    hash_span(hasher, *span);
    hash_span(hasher, *statement_span);
    hash_span(hasher, *source_span);
    (*visibility as u8).hash(hasher);
    expected_unused_reason.hash(hasher);
}

fn hash_mock_operation(hasher: &mut Xxh3, operation: &VitestModuleMockOperationFact) {
    let VitestModuleMockOperationFact {
        source,
        call_start,
        action,
    } = operation;
    source.hash(hasher);
    call_start.hash(hasher);
    match action {
        VitestModuleMockAction::Mock {
            factory_replaces_original,
        } => {
            0u8.hash(hasher);
            factory_replaces_original.hash(hasher);
        }
        VitestModuleMockAction::Unmock => 1u8.hash(hasher),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_types::cache_rejection::CacheRejection;
    use fallow_types::discover::{DiscoveredFile, FileId};
    use fallow_types::extract::{ImportedName, ModuleLoadMechanism, VisibilityTag};

    use super::super::{GraphCacheFileHashes, GraphCacheManifest, GraphCacheMode};
    use super::*;

    fn module() -> ModuleInfo {
        let mut module = ModuleInfo::empty(FileId(0));
        module.imports.push(ImportInfo {
            source: "./util".to_string(),
            imported_name: ImportedName::Named("alpha".to_string()),
            local_name: "alpha".to_string(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: Span::new(0, 30),
            source_span: Span::new(22, 30),
        });
        module.require_calls.push(RequireCallInfo {
            source: "./cjs".to_string(),
            span: Span::new(40, 56),
            source_span: Span::new(48, 55),
            destructured_names: Vec::new(),
            destructured_locals: Vec::new(),
            local_name: Some("cjs".to_string()),
            is_type_only: false,
        });
        module.dynamic_imports.push(DynamicImportInfo {
            source: "./lazy".to_string(),
            span: Span::new(60, 76),
            destructured_names: Vec::new(),
            local_name: None,
            is_speculative: false,
            value_escapes: false,
        });
        module.dynamic_import_patterns.push(DynamicImportPattern {
            prefix: "./locales/".to_string(),
            suffix: Some(".json".to_string()),
            excludes: Vec::new(),
            span: Span::new(80, 110),
            mechanism: ModuleLoadMechanism::EsModule,
            imported_name: ImportedName::Namespace,
        });
        module.re_exports.push(ReExportInfo {
            source: "./b".to_string(),
            imported_name: "b".to_string(),
            exported_name: "b".to_string(),
            is_type_only: false,
            span: Span::new(120, 121),
            statement_span: Span::new(111, 135),
            source_span: Span::new(130, 135),
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
        });
        module.semantic_facts = vec![SemanticFact::VitestModuleMockOperation(
            VitestModuleMockOperationFact {
                source: "./mocked".to_string(),
                call_start: 140,
                action: VitestModuleMockAction::Mock {
                    factory_replaces_original: true,
                },
            },
        )]
        .into();
        module
    }

    /// Apply `change` to a copy of the base module and report whether the
    /// hash differs from the base hash.
    fn changes_hash(change: impl FnOnce(&mut ModuleInfo)) -> bool {
        let base = module();
        let mut changed = module();
        change(&mut changed);
        resolve_inputs_hash(&base) != resolve_inputs_hash(&changed)
    }

    fn mock_fact(module: &mut ModuleInfo, change: impl FnOnce(&mut VitestModuleMockOperationFact)) {
        let mut facts = module.semantic_facts.to_vec();
        let SemanticFact::VitestModuleMockOperation(operation) = &mut facts[0] else {
            panic!("the base module holds one mock operation");
        };
        change(operation);
        module.semantic_facts = facts.into();
    }

    /// Each field of `ModuleInfo` is named here, so a new field does not
    /// compile until its author decides: a field that import resolution
    /// reads must join [`resolve_inputs_hash`], and any other field joins the
    /// second list.
    #[test]
    fn each_module_field_is_hashed_or_not_a_resolver_input() {
        let ModuleInfo {
            // Hashed by `resolve_inputs_hash`.
            imports: _,
            require_calls: _,
            dynamic_imports: _,
            dynamic_import_patterns: _,
            re_exports: _,
            jsx_runtime_from_config: _,
            semantic_facts: _,
            // Not read by import resolution. The auto-import pass reads
            // `auto_import_candidates`, `exports`, `member_accesses`,
            // `whole_object_uses` and `unused_import_bindings`, and a project
            // with auto-import rules does not use this hash.
            file_id: _,
            exports: _,
            package_path_references: _,
            type_package_references: _,
            bin_path_references: _,
            package_resolve_sites: _,
            member_accesses: _,
            whole_object_uses: _,
            has_cjs_exports: _,
            has_angular_component_template_url: _,
            content_hash: _,
            parse_error_count: _,
            parse_panicked: _,
            parse_nesting_too_deep: _,
            suppressions: _,
            unknown_suppression_kinds: _,
            range_marker_problems: _,
            inline_threshold_overrides: _,
            unused_import_bindings: _,
            type_referenced_import_bindings: _,
            value_referenced_import_bindings: _,
            line_offsets: _,
            complexity: _,
            flag_uses: _,
            flag_registry_facts: _,
            class_heritage: _,
            exported_factory_returns: _,
            exported_factory_return_object_shapes: _,
            type_member_types: _,
            injection_tokens: _,
            local_type_declarations: _,
            public_signature_type_references: _,
            namespace_object_aliases: _,
            iconify_prefixes: _,
            iconify_icon_names: _,
            auto_import_candidates: _,
            directives: _,
            client_only_dynamic_import_spans: _,
            env_reads: _,
            security_sinks: _,
            security_sinks_skipped: _,
            security_unresolved_callee_sites: _,
            inert_function_exports: _,
            tainted_bindings: _,
            sanitized_sink_args: _,
            security_control_sites: _,
            callee_uses: _,
            command_invocations: _,
            imported_call_sites: _,
            import_binding_references: _,
            misplaced_directives: _,
            inline_server_action_exports: _,
            di_key_sites: _,
            has_dynamic_provide: _,
            is_server_action_module: _,
            has_global_declarations: _,
            triple_slash_reference_paths: _,
            referenced_import_bindings: _,
            component_props: _,
            component_contracts: _,
            effect_facts: _,
            function_contracts: _,
            has_props_attrs_fallthrough: _,
            has_define_expose: _,
            has_define_model: _,
            has_unharvestable_props: _,
            component_emits: _,
            angular_inputs: _,
            angular_outputs: _,
            angular_component_selectors: _,
            registered_custom_elements: _,
            used_custom_element_tags: _,
            angular_used_selectors: _,
            angular_entry_component_refs: _,
            has_dynamic_component_render: _,
            has_unharvestable_emits: _,
            has_dynamic_emit: _,
            has_emit_whole_object_use: _,
            load_return_keys: _,
            has_unharvestable_load: _,
            has_load_data_whole_use: _,
            has_page_data_store_whole_use: _,
            has_route_loader_data_whole_use: _,
            component_functions: _,
            react_props: _,
            hook_uses: _,
            render_edges: _,
            svelte_dispatched_events: _,
            svelte_listened_events: _,
            has_dynamic_dispatch: _,
        } = module();
    }

    #[test]
    fn equal_resolver_inputs_give_an_equal_hash() {
        assert_eq!(
            resolve_inputs_hash(&module()),
            resolve_inputs_hash(&module())
        );
        assert_ne!(resolve_inputs_hash(&module()), NO_MODULE_RESOLVE_HASH);
    }

    #[test]
    fn a_module_without_resolver_inputs_differs_from_no_module() {
        let empty = ModuleInfo::empty(FileId(0));
        assert_ne!(resolve_inputs_hash(&empty), NO_MODULE_RESOLVE_HASH);
    }

    #[test]
    fn facts_that_resolution_does_not_read_keep_the_hash() {
        assert!(!changes_hash(|module| {
            module.content_hash = 7;
            module.whole_object_uses = vec!["alpha".to_string()].into();
            module.unused_import_bindings.push("alpha".to_string());
            module.auto_import_candidates.push("useThing".to_string());
            module.line_offsets.push(12);
        }));
    }

    #[test]
    fn each_import_field_changes_the_hash() {
        assert!(changes_hash(|m| m.imports[0].source = "./other".to_string()));
        assert!(changes_hash(|m| {
            m.imports[0].imported_name = ImportedName::Named("bravo".to_string());
        }));
        assert!(changes_hash(
            |m| m.imports[0].local_name = "renamed".to_string()
        ));
        assert!(changes_hash(|m| m.imports[0].is_type_only = true));
        assert!(changes_hash(|m| m.imports[0].is_type_only_star = true));
        assert!(changes_hash(|m| m.imports[0].from_style = true));
        assert!(changes_hash(|m| m.imports[0].span = Span::new(1, 31)));
        assert!(changes_hash(
            |m| m.imports[0].source_span = Span::new(23, 31)
        ));
        assert!(changes_hash(|m| m.imports.clear()));
    }

    #[test]
    fn each_require_call_field_changes_the_hash() {
        assert!(changes_hash(
            |m| m.require_calls[0].source = "./x".to_string()
        ));
        assert!(changes_hash(|m| m.require_calls[0].span = Span::new(41, 57)));
        assert!(changes_hash(
            |m| m.require_calls[0].source_span = Span::new(49, 56)
        ));
        assert!(changes_hash(|m| {
            m.require_calls[0].destructured_names = vec!["a".to_string()];
        }));
        assert!(changes_hash(|m| {
            m.require_calls[0].destructured_locals = vec![("a".to_string(), "b".to_string())];
        }));
        assert!(changes_hash(|m| m.require_calls[0].local_name = None));
        assert!(changes_hash(|m| m.require_calls[0].is_type_only = true));
    }

    #[test]
    fn each_dynamic_import_field_changes_the_hash() {
        assert!(changes_hash(
            |m| m.dynamic_imports[0].source = "./x".to_string()
        ));
        assert!(changes_hash(
            |m| m.dynamic_imports[0].span = Span::new(61, 77)
        ));
        assert!(changes_hash(|m| {
            m.dynamic_imports[0].destructured_names = vec!["a".to_string()];
        }));
        assert!(changes_hash(|m| {
            m.dynamic_imports[0].local_name = Some("lazy".to_string());
        }));
        assert!(changes_hash(|m| m.dynamic_imports[0].is_speculative = true));
        assert!(changes_hash(|m| m.dynamic_imports[0].value_escapes = true));
    }

    #[test]
    fn each_dynamic_pattern_field_changes_the_hash() {
        assert!(changes_hash(|m| {
            m.dynamic_import_patterns[0].prefix = "./pages/".to_string();
        }));
        assert!(changes_hash(|m| m.dynamic_import_patterns[0].suffix = None));
        assert!(changes_hash(|m| {
            m.dynamic_import_patterns[0].excludes = vec!["./locales/skip.json".to_string()];
        }));
        assert!(changes_hash(|m| {
            m.dynamic_import_patterns[0].span = Span::new(81, 111);
        }));
        assert!(changes_hash(|m| {
            m.dynamic_import_patterns[0].mechanism = ModuleLoadMechanism::CommonJsRequire;
        }));
        assert!(changes_hash(|m| {
            m.dynamic_import_patterns[0].imported_name = ImportedName::Default;
        }));
    }

    #[test]
    fn each_re_export_field_changes_the_hash() {
        assert!(changes_hash(|m| m.re_exports[0].source = "./c".to_string()));
        assert!(changes_hash(
            |m| m.re_exports[0].imported_name = "c".to_string()
        ));
        assert!(changes_hash(
            |m| m.re_exports[0].exported_name = "c".to_string()
        ));
        assert!(changes_hash(|m| m.re_exports[0].is_type_only = true));
        assert!(changes_hash(|m| m.re_exports[0].span = Span::new(121, 122)));
        assert!(changes_hash(|m| {
            m.re_exports[0].statement_span = Span::new(112, 136);
        }));
        assert!(changes_hash(
            |m| m.re_exports[0].source_span = Span::new(131, 136)
        ));
        assert!(changes_hash(
            |m| m.re_exports[0].visibility = VisibilityTag::Public
        ));
        assert!(changes_hash(|m| {
            m.re_exports[0].expected_unused_reason = Some("kept".to_string());
        }));
    }

    #[test]
    fn each_mock_operation_field_changes_the_hash() {
        assert!(changes_hash(|m| {
            mock_fact(m, |operation| operation.source = "./x".to_string());
        }));
        assert!(changes_hash(|m| mock_fact(m, |operation| operation
            .call_start =
            141)));
        assert!(changes_hash(|m| {
            mock_fact(m, |operation| {
                operation.action = VitestModuleMockAction::Mock {
                    factory_replaces_original: false,
                };
            });
        }));
        assert!(changes_hash(|m| {
            mock_fact(m, |operation| {
                operation.action = VitestModuleMockAction::Unmock;
            });
        }));
    }

    #[test]
    fn the_jsx_runtime_flag_changes_the_hash() {
        assert!(changes_hash(|m| m.jsx_runtime_from_config = true));
    }

    fn manifest(hashes: GraphCacheFileHashes) -> GraphCacheManifest {
        let files = [DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/a.ts"),
            size_bytes: 1,
        }];
        GraphCacheManifest::from_discovered_files(
            Path::new("/project"),
            &files,
            GraphCacheMode::new(1, 2, 3),
            |_| hashes,
        )
    }

    #[test]
    fn a_content_change_with_equal_resolver_inputs_keeps_the_resolver_payload() {
        let cached = manifest(GraphCacheFileHashes {
            content_hash: 10,
            resolve_hash: 5,
        });
        let current = manifest(GraphCacheFileHashes {
            content_hash: 11,
            resolve_hash: 5,
        });

        assert!(
            !cached.matches_inputs(&current),
            "the persisted graph holds facts of the old content"
        );
        assert!(cached.content_differs(&current));
        assert_eq!(cached.classify_resolution_mismatch(&current), None);
    }

    #[test]
    fn a_resolver_input_change_rejects_the_resolver_payload() {
        let cached = manifest(GraphCacheFileHashes {
            content_hash: 10,
            resolve_hash: 5,
        });
        let current = manifest(GraphCacheFileHashes {
            content_hash: 11,
            resolve_hash: 6,
        });

        assert_eq!(
            cached.classify_resolution_mismatch(&current),
            Some(CacheRejection::FingerprintChanged)
        );
    }

    #[test]
    fn a_file_that_gains_or_loses_its_module_rejects_the_resolver_payload() {
        let without_module = manifest(GraphCacheFileHashes::NO_MODULE);
        let with_module = manifest(GraphCacheFileHashes::of_module(&module()));
        let same_resolve_hash = manifest(GraphCacheFileHashes {
            content_hash: 11,
            resolve_hash: NO_MODULE_RESOLVE_HASH,
        });

        for (cached, current) in [
            (&without_module, &with_module),
            (&with_module, &without_module),
            (&without_module, &same_resolve_hash),
            (&same_resolve_hash, &without_module),
        ] {
            assert_eq!(
                cached.classify_resolution_mismatch(current),
                Some(CacheRejection::FingerprintChanged)
            );
        }
    }

    #[test]
    fn content_only_hashes_reject_each_content_change() {
        let cached = manifest(GraphCacheFileHashes::content_only(10));
        let current = manifest(GraphCacheFileHashes::content_only(11));

        assert_eq!(
            cached.classify_resolution_mismatch(&current),
            Some(CacheRejection::FingerprintChanged)
        );
    }

    #[test]
    fn a_fact_that_moves_between_lists_changes_the_hash() {
        let mut one_list = ModuleInfo::empty(FileId(0));
        one_list.dynamic_imports.push(DynamicImportInfo {
            source: "./a".to_string(),
            span: Span::new(0, 5),
            destructured_names: vec!["x".to_string(), "y".to_string()],
            local_name: None,
            is_speculative: false,
            value_escapes: false,
        });
        let mut split = ModuleInfo::empty(FileId(0));
        split.dynamic_imports.push(DynamicImportInfo {
            source: "./a".to_string(),
            span: Span::new(0, 5),
            destructured_names: vec!["x".to_string()],
            local_name: None,
            is_speculative: false,
            value_escapes: false,
        });
        split.dynamic_imports.push(DynamicImportInfo {
            source: "y".to_string(),
            span: Span::new(0, 5),
            destructured_names: Vec::new(),
            local_name: None,
            is_speculative: false,
            value_escapes: false,
        });
        assert_ne!(resolve_inputs_hash(&one_list), resolve_inputs_hash(&split));
    }
}
