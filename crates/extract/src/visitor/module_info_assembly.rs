//! The resolution phase after the walk, the load kinds of imports, and the
//! assembly of a `ModuleInfo` from the extractor, also the merge of another
//! extractor into it.

use super::*;

impl ModuleInfoExtractor {
    /// Run every finalize/resolve pass shared by `into_module_info` and
    /// `merge_into`, returning the collected namespace object aliases.
    fn finalize_resolution_phase(&mut self) -> Vec<fallow_types::extract::NamespaceObjectAlias> {
        self.begin_resolution_phase();
        self.resolve_pending_local_export_specifiers();
        self.resolve_native_html_element_aliases();
        self.finalize_local_class_heritage();
        self.enrich_store_exports();
        self.finalize_di_key_sites();
        // Before `record_exported_instance_bindings` / `resolve_object_binding_candidates`,
        // which read `binding_target_names`, so a factory-return-bound local also
        // propagates through object literals and exported-instance bindings (parity
        // with the during-the-walk `new Class()` binding). See issue #1441.
        // Aliases first: promote `useApi(){ return api }` to a factory-return via
        // the typed local, so the `const x = useApi()` candidate below resolves.
        self.resolve_factory_return_aliases();
        // Before `resolve_factory_return_candidates`: it reads
        // `factory_return_object_shapes` for the same-file object-literal consumer
        // arm. Needs `enrich_local_class_exports` (above) for the typed-property
        // hop on `factory.prop` values. See issue #1858.
        self.resolve_factory_return_object_shapes();
        self.resolve_factory_property_observations();
        self.resolve_factory_property_accesses();
        self.resolve_factory_return_candidates();
        // Separate from the candidate pass, which early-returns when no member
        // candidate exists: an unnamed factory result records no member candidate.
        self.resolve_factory_unnamed_result_accesses();
        self.resolve_factory_whole_object_candidates();
        // Before `record_exported_instance_bindings`, so that
        // `export const sm = Cls.getInstance()` binds the export to the class.
        // See issue #5233.
        self.resolve_factory_call_candidates();
        self.record_exported_instance_bindings();
        self.resolve_object_binding_candidates();
        self.resolve_playwright_factory_call_definitions();
        self.record_playwright_test_binding_facts();
        self.resolve_structural_class_calls();
        // Before `resolve_bound_member_accesses` so the re-emitted accesses reach
        // the same typed-property expansion as unambiguous ones.
        self.resolve_ambiguous_walk_order_uses();
        self.resolve_bound_member_accesses();
        self.resolve_imported_call_element_accesses();
        self.complete_binding_producer_calls();
        self.propagate_local_subclass_member_accesses();
        self.resolve_projected_member_accesses();
        self.map_local_signature_refs_to_exports();
        self.apply_side_effect_registrations();
        self.resolve_typed_react_props();
        self.record_exported_direct_object_binding_facts();
        let namespace_object_aliases = self.collect_namespace_object_aliases();
        // Last: every resolution pass above relies on the per-class `this@<id>.`
        // keys, so the qualifier is stripped only once they have run, before any
        // spelling is emitted into `ModuleInfo`. See issue #1821 (Fix B).
        self.finish_this_reads();
        namespace_object_aliases
    }

    /// Record each import load-kind mark as a semantic fact, once per span.
    pub(super) fn mark_import_load_kind(&mut self, span: Span, kind: ImportLoadKind) {
        self.import_load_kind_marks.push((span, kind));
    }

    fn finalize_import_load_kinds(&mut self) {
        let mut marks = std::mem::take(&mut self.import_load_kind_marks);
        marks.sort_unstable_by_key(|(span, kind)| (span.start, *kind));
        marks.dedup_by_key(|(span, _)| span.start);
        self.semantic_facts
            .extend(marks.into_iter().map(|(span, kind)| {
                SemanticFact::ImportLoadKindOverride(ImportLoadKindOverrideFact {
                    span_start: span.start,
                    kind,
                })
            }));
    }

    fn finalize_cjs_provenance(&mut self) {
        if self.cjs_single_static_object_map && !self.has_cjs_es_module_marker {
            self.semantic_facts
                .push(SemanticFact::CjsSingleStaticObjectMap);
        }
    }

    #[expect(clippy::too_many_lines, reason = "one field per module fact")]
    pub(crate) fn into_module_info(
        mut self,
        file_id: fallow_types::discover::FileId,
        content_hash: u64,
        parsed: ParsedSuppressions,
    ) -> ModuleInfo {
        self.finalize_cjs_provenance();
        self.finalize_import_load_kinds();
        let namespace_object_aliases = self.finalize_resolution_phase();
        let exported_factory_returns = self.collect_exported_factory_returns();
        let exported_factory_return_object_shapes =
            self.collect_exported_factory_return_object_shapes();
        let type_member_types = self.collect_type_member_types();
        Self::deduplicate_local_type_declarations(&mut self.local_type_declarations);
        self.keep_value_merged_namespaces_as_values();
        ModuleInfo {
            file_id,
            exports: self.exports.into(),
            imports: self.imports,
            re_exports: self.re_exports,
            dynamic_imports: self.dynamic_imports,
            dynamic_import_patterns: self.dynamic_import_patterns,
            require_calls: self.require_calls,
            package_path_references: self.package_path_references.into_boxed_slice(),
            type_package_references: self.type_package_references.into_boxed_slice(),
            bin_path_references: self.bin_path_references.into_boxed_slice(),
            package_resolve_sites: finish_package_resolve_sites(self.package_resolve_sites),
            member_accesses: self.member_accesses.into(),
            semantic_facts: self.semantic_facts.into(),
            whole_object_uses: self.whole_object_uses.into(),
            has_cjs_exports: self.has_cjs_exports,
            has_angular_component_template_url: self.has_angular_component_template_url,
            content_hash,
            parse_error_count: 0, // Set by the parse layer, like the two below.
            parse_panicked: false,
            parse_nesting_too_deep: false,
            suppressions: parsed.suppressions,
            unknown_suppression_kinds: parsed.unknown_kinds,
            range_marker_problems: parsed.range_problems,
            inline_threshold_overrides: parsed.inline_thresholds,
            unused_import_bindings: Vec::new(),
            type_referenced_import_bindings: Vec::new(),
            value_referenced_import_bindings: Vec::new(),
            line_offsets: Vec::new(),
            complexity: Vec::new(),
            flag_uses: Vec::new(),
            flag_registry_facts: None,
            class_heritage: self.class_heritage,
            exported_factory_returns: exported_factory_returns.into(),
            exported_factory_return_object_shapes: exported_factory_return_object_shapes.into(),
            type_member_types: type_member_types.into(),
            injection_tokens: self.injection_tokens,
            local_type_declarations: self.local_type_declarations,
            public_signature_type_references: self.public_signature_type_references,
            namespace_object_aliases,
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: self.og_image_template_candidates,
            directives: self.directives,
            client_only_dynamic_import_spans: self.client_only_dynamic_import_spans,
            env_reads: self.env_reads,
            security_sinks: self.security_sinks,
            security_sinks_skipped: self.security_sinks_skipped,
            security_unresolved_callee_sites: self.security_unresolved_callee_sites,
            inert_function_exports: self.inert_function_exports,
            tainted_bindings: self.tainted_bindings,
            sanitized_sink_args: self.sanitized_sink_args,
            security_control_sites: self.security_control_sites,
            callee_uses: self.callee_uses,
            command_invocations: self.command_invocations,
            imported_call_sites: self.imported_call_sites.into(),
            import_binding_references: self.import_binding_references.into(),
            misplaced_directives: self.misplaced_directives,
            inline_server_action_exports: self.inline_server_action_exports,
            di_key_sites: self.di_key_sites,
            has_dynamic_provide: self.has_dynamic_provide,
            is_server_action_module: self.is_server_action_module,
            has_global_declarations: self.has_global_declarations,
            triple_slash_reference_paths: self.triple_slash_reference_paths.into_boxed_slice(),
            jsx_runtime_from_config: self.jsx_runtime_from_config,
            // Populated in `release_resolution_payload`; empty at construction.
            referenced_import_bindings: Vec::new(),
            component_props: Vec::new(),
            component_contracts: None,
            effect_facts: None,
            function_contracts: (!self.function_contracts.is_empty())
                .then(|| Box::new(self.function_contracts)),
            has_props_attrs_fallthrough: false,
            has_define_expose: false,
            has_define_model: false,
            has_unharvestable_props: false,
            component_emits: Vec::new(),
            angular_inputs: self.angular_inputs,
            angular_outputs: self.angular_outputs,
            angular_component_selectors: self.angular_component_selectors,
            registered_custom_elements: self.registered_custom_elements,
            used_custom_element_tags: {
                let mut tags: Vec<String> = self.used_custom_element_tags.into_iter().collect();
                tags.sort_unstable();
                tags
            },
            angular_used_selectors: self.angular_used_selectors,
            angular_entry_component_refs: self.angular_entry_component_refs,
            has_dynamic_component_render: self.has_dynamic_component_render,
            has_unharvestable_emits: false,
            has_dynamic_emit: false,
            has_emit_whole_object_use: false,
            load_return_keys: self.load_return_keys,
            has_unharvestable_load: self.has_unharvestable_load,
            has_load_data_whole_use: self.has_load_data_whole_use,
            // Both derived in `release_resolution_payload` from `whole_object_uses`.
            has_page_data_store_whole_use: false,
            has_route_loader_data_whole_use: false,
            component_functions: self.component_functions,
            react_props: self.react_props,
            hook_uses: self.hook_uses,
            render_edges: self.render_edges,
            svelte_dispatched_events: self.svelte_dispatched_events,
            svelte_listened_events: Vec::new(),
            has_dynamic_dispatch: self.has_dynamic_dispatch,
        }
    }

    pub(crate) fn merge_into(mut self, info: &mut ModuleInfo) {
        debug_assert!(
            self.inline_template_findings.is_empty(),
            "merge_into is the SFC-script path, which skips Angular components"
        );
        self.finalize_cjs_provenance();
        self.finalize_import_load_kinds();
        let namespace_object_aliases = self.finalize_resolution_phase();
        info.auto_import_candidates
            .append(&mut self.og_image_template_candidates);
        self.merge_module_graph(info, namespace_object_aliases);
        self.merge_security_info(info);
        self.merge_framework_info(info);
    }

    fn merge_module_graph(
        &mut self,
        info: &mut ModuleInfo,
        mut namespace_object_aliases: Vec<fallow_types::extract::NamespaceObjectAlias>,
    ) {
        // Compute before `self.exports` is drained below; the join reads exports.
        let mut exported_factory_returns = self.collect_exported_factory_returns();
        let mut exported_factory_return_object_shapes =
            self.collect_exported_factory_return_object_shapes();
        let mut type_member_types = self.collect_type_member_types();
        self.rebase_shared_state(info.imports.len() as u32);
        self.merge_import_binding_references(info);
        self.merge_function_contracts(info);
        info.imports.append(&mut self.imports);
        record_extend::append_shared(&mut info.exports, &mut self.exports);
        info.re_exports.append(&mut self.re_exports);
        info.dynamic_imports.append(&mut self.dynamic_imports);
        info.dynamic_import_patterns
            .append(&mut self.dynamic_import_patterns);
        info.require_calls.append(&mut self.require_calls);
        let mut package_path_references =
            std::mem::take(&mut info.package_path_references).into_vec();
        package_path_references.append(&mut self.package_path_references);
        info.package_path_references = package_path_references.into_boxed_slice();
        let mut type_package_references =
            std::mem::take(&mut info.type_package_references).into_vec();
        type_package_references.append(&mut self.type_package_references);
        info.type_package_references = type_package_references.into_boxed_slice();
        let mut bin_path_references = std::mem::take(&mut info.bin_path_references).into_vec();
        bin_path_references.append(&mut self.bin_path_references);
        info.bin_path_references = bin_path_references.into_boxed_slice();
        let mut package_resolve_sites = std::mem::take(&mut info.package_resolve_sites).into_vec();
        package_resolve_sites.extend(finish_package_resolve_sites(std::mem::take(
            &mut self.package_resolve_sites,
        )));
        info.package_resolve_sites = package_resolve_sites.into_boxed_slice();
        record_extend::append_shared(&mut info.member_accesses, &mut self.member_accesses);
        record_extend::append_shared(&mut info.whole_object_uses, &mut self.whole_object_uses);
        // Carry typed semantic facts through the SFC merge path; without this
        // every fact kind (factory-fn, fluent-chain, typed-property-hop, ...)
        // was silently dropped for Vue/Svelte `<script>` blocks, so the
        // analyze-layer cross-module joins never saw SFC consumers. See issue
        // #1785 (review finding).
        if !self.semantic_facts.is_empty() {
            let mut semantic_facts = std::mem::take(&mut info.semantic_facts).to_vec();
            semantic_facts.append(&mut self.semantic_facts);
            crate::shared_state::bound_merged_facts(&mut semantic_facts);
            info.semantic_facts = semantic_facts.into();
        }
        info.has_cjs_exports |= self.has_cjs_exports;
        info.has_angular_component_template_url |= self.has_angular_component_template_url;
        info.class_heritage.append(&mut self.class_heritage);
        if !exported_factory_returns.is_empty() {
            let mut merged = std::mem::take(&mut info.exported_factory_returns).to_vec();
            merged.append(&mut exported_factory_returns);
            info.exported_factory_returns = merged.into();
        }
        if !exported_factory_return_object_shapes.is_empty() {
            let mut merged =
                std::mem::take(&mut info.exported_factory_return_object_shapes).to_vec();
            merged.append(&mut exported_factory_return_object_shapes);
            info.exported_factory_return_object_shapes = merged.into();
        }
        if !type_member_types.is_empty() {
            let mut merged = std::mem::take(&mut info.type_member_types).to_vec();
            merged.append(&mut type_member_types);
            info.type_member_types = merged.into();
        }
        info.injection_tokens.append(&mut self.injection_tokens);
        info.local_type_declarations
            .append(&mut self.local_type_declarations);
        Self::deduplicate_local_type_declarations(&mut info.local_type_declarations);
        info.public_signature_type_references
            .append(&mut self.public_signature_type_references);
        info.namespace_object_aliases
            .append(&mut namespace_object_aliases);
        info.directives.append(&mut self.directives);
        info.client_only_dynamic_import_spans
            .append(&mut self.client_only_dynamic_import_spans);
        info.env_reads.append(&mut self.env_reads);
        info.callee_uses.append(&mut self.callee_uses);
        info.command_invocations
            .append(&mut self.command_invocations);
        if !self.imported_call_sites.is_empty() {
            let mut calls = std::mem::take(&mut info.imported_call_sites).to_vec();
            calls.append(&mut self.imported_call_sites);
            info.imported_call_sites = calls.into();
        }
    }

    fn merge_framework_info(&mut self, info: &mut ModuleInfo) {
        info.misplaced_directives
            .append(&mut self.misplaced_directives);
        info.inline_server_action_exports
            .append(&mut self.inline_server_action_exports);
        info.di_key_sites.append(&mut self.di_key_sites);
        info.has_dynamic_provide |= self.has_dynamic_provide;
        info.load_return_keys.append(&mut self.load_return_keys);
        info.has_unharvestable_load |= self.has_unharvestable_load;
        info.has_load_data_whole_use |= self.has_load_data_whole_use;
        info.angular_inputs.append(&mut self.angular_inputs);
        info.angular_outputs.append(&mut self.angular_outputs);
        info.angular_component_selectors
            .append(&mut self.angular_component_selectors);
        info.angular_used_selectors
            .append(&mut self.angular_used_selectors);
        info.angular_entry_component_refs
            .append(&mut self.angular_entry_component_refs);
        info.has_dynamic_component_render |= self.has_dynamic_component_render;
        info.svelte_dispatched_events
            .append(&mut self.svelte_dispatched_events);
        info.has_dynamic_dispatch |= self.has_dynamic_dispatch;
        self.merge_template_emits(info);
    }
}
