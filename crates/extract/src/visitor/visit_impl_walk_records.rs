//! The facts that the call and assignment visits record around the walk.

use super::*;

impl<'a> ModuleInfoExtractor {
    /// Record the facts of a call before the walk visits its children.
    pub(super) fn record_call_expression_facts(&mut self, expr: &CallExpression<'a>) {
        if let Expression::Identifier(callee) = &expr.callee
            && self.template_object_locals.contains(callee.name.as_str())
        {
            self.structured_namespace_reference_spans
                .insert(callee.span);
        }
        if Self::is_cjs_es_module_marker_call(expr) {
            self.has_cjs_es_module_marker = true;
            self.cjs_single_static_object_map = false;
        }
        self.record_structural_class_call_candidate(expr);
        if let Expression::Identifier(callee) = &expr.callee
            && callee.name == "String"
            && let Some(arg) = expr.arguments.first()
            && let Some(arg_expr) = arg.as_expression()
        {
            self.record_string_coercion_to_string(arg_expr);
        }
        self.clear_literal_allowlist_on_mutating_member_call(expr);
        self.record_filesystem_path_new_url_arguments(expr);
        self.record_og_image_template_call(expr);
        self.record_framework_callback_param_sources(expr);
        self.react_record_hook_call(expr);

        if let Some(test_name) = playwright_test_callee_name(&expr.callee) {
            let fixture_uses = collect_playwright_fixture_member_uses(&expr.arguments);
            for access in &fixture_uses {
                self.semantic_facts.push(SemanticFact::PlaywrightFixtureUse(
                    PlaywrightFixtureUseFact {
                        test_name: test_name.clone(),
                        fixture_name: access.fixture_name.clone(),
                        member: access.member.clone(),
                    },
                ));
            }
        }

        if let Some((tag, class_name)) = extract_custom_elements_define(expr) {
            // Record the registration for the Lit `unrendered-component` arm
            // (anchored at the `customElements.define(...)` call), then keep the
            // class credited as side-effect-used.
            self.registered_custom_elements
                .push(fallow_types::extract::RegisteredCustomElement {
                    tag,
                    class_local_name: class_name.clone(),
                    span_start: expr.span.start,
                });
            self.side_effect_registered_class_names.insert(class_name);
        }

        // Imperative element render / lookup (`document.createElement('x-foo')`,
        // `customElements.get('x-foo')`): credit the tag as rendered so the Lit
        // `unrendered-component` arm does not flag an element created without an
        // `html` template.
        if let Some(tag) = extract_custom_element_tag_reference(expr) {
            self.used_custom_element_tags.insert(tag);
        }

        self.bind_iterable_callback_parameter(expr);
        self.record_vitest_mock_imports(expr);
        self.record_vitest_do_mock(expr);
        self.record_vitest_unmock(expr);

        self.record_node_runtime_call(expr);
        self.try_record_package_path_reference(expr);
        self.try_record_relative_require_resolve(expr);
        self.record_bare_require_call(expr);
        self.record_whole_object_call_use(expr);
        self.record_reflective_whole_object_use(expr);
        self.record_namespace_spy_call(expr);
        self.record_import_meta_glob_patterns(expr);
        self.record_require_context_pattern(expr);
        self.record_import_callback_dynamic_imports(expr);
        self.record_parameterless_then_import(expr);
        self.record_arrow_wrapped_dynamic_import(expr);
        self.record_ssr_load_module(expr);

        self.try_record_fluent_chain_access(expr);
        self.record_pinia_map_helpers(expr);
        self.record_di_key_site(expr);
        self.record_component_event_calls(expr);
        self.record_load_data_whole_arg_use(expr);
        self.record_route_loader_data_whole_arg_use(expr);

        self.capture_security_call_sites(expr);

        self.record_angular_dynamic_component_render(expr);
        self.record_angular_bootstrap_call(expr);
        self.record_angular_dynamic_providers(expr);

        self.record_callee_use(&expr.callee, expr.span.start, false);
        self.record_imported_call_site(expr);
        self.record_this_binding_call(expr);
        self.record_mobx_annotation_members(expr);
    }

    /// Record the facts of an assignment before the walk visits its children.
    pub(super) fn record_assignment_expression_facts(&mut self, expr: &AssignmentExpression<'a>) {
        self.record_pinia_state_write(expr);
        // FP-1 (unused-load-data-key): a destructure-ASSIGNMENT from the `data`
        // prop (`({ guests } = data)` in a Svelte `$effect`, or `$: ({a} = data)`)
        // is NOT a `const/let` declaration, so Primitive A does not credit the
        // individual keys. Treat the whole `data` RHS as a whole-object use so the
        // detector abstains rather than false-flag. Name-gated on `data`; read
        // only by the load-data detector, so byte-identity-safe.
        if matches!(&expr.right, Expression::Identifier(id) if id.name == "data")
            && matches!(
                &expr.left,
                AssignmentTarget::ObjectAssignmentTarget(_)
                    | AssignmentTarget::ArrayAssignmentTarget(_)
            )
        {
            self.has_load_data_whole_use = true;
        }

        if let Some(name) = assignment_target_security_context_name(&expr.left) {
            self.capture_math_random_context_sink(name.as_str(), &expr.right, expr.span);
            self.capture_hardcoded_secret_literal_sink(name.as_str(), &expr.right, expr.span);
        }

        self.record_assignment_target_bindings(&expr.left);
        self.record_assignment_member_destructuring(expr);

        // Track writes to a bare identifier for the alias value-proof. POISON
        // (any scope, ANY operator): a write that can leave the binding holding a
        // non-class value, incl. a compound/logical `api ??= {} as any`, must
        // abstain an otherwise class-proven binding. PROOF (module scope, plain
        // `=` only): a dominating module-scope initializer. See #1441 (Part A).
        if let AssignmentTarget::AssignmentTargetIdentifier(ident) = &expr.left {
            let classified = classify_factory_assigned_value(&expr.right);
            // Module-scope plain `=` writes also serve as proof; clone only for
            // that rarer case so the common poison-only path moves the value.
            if self.is_module_scope() && matches!(expr.operator, AssignmentOperator::Assign) {
                self.module_scope_initializers
                    .entry(ident.name.to_string())
                    .or_default()
                    .push(classified.clone());
            }
            self.identifier_write_values
                .entry(ident.name.to_string())
                .or_default()
                .push(classified);
        }

        if let AssignmentTarget::StaticMemberExpression(member) = &expr.left {
            self.handle_cjs_member_export(member, expr);
            if matches!(member.object, Expression::ThisExpression(_)) {
                self.handle_this_member_assignment(member.property.name.as_str(), expr);
            }
            self.record_member_assigned_function(member, &expr.right);
        }
        if !matches!(expr.left, AssignmentTarget::StaticMemberExpression(_))
            && Self::assignment_target_mutates_cjs_exports(&expr.left)
        {
            self.record_cjs_assignment(false);
        }
        // `this.#dep = new Dep()` / `this.#dep = dep`: oxc routes a `#`-private
        // assignment target through the `PrivateFieldExpression` variant, so it
        // needs its own arm to reach the same instance-binding propagation as a
        // public `this.dep = ...` assignment (issue #1821).
        if let AssignmentTarget::PrivateFieldExpression(member) = &expr.left
            && matches!(member.object, Expression::ThisExpression(_))
        {
            self.handle_this_member_assignment(format!("#{}", member.field.name).as_str(), expr);
        }
        self.capture_member_assign_sink(expr);
        // `tool = await import("./tool")`: mark the import span handled before
        // the walk so `visit_import_expression` does not record it again.
        if matches!(expr.operator, AssignmentOperator::Assign)
            && let AssignmentTarget::AssignmentTargetIdentifier(ident) = &expr.left
        {
            self.handle_dynamic_import_assignment(ident, &expr.right);
            self.record_sink_assignment_binding(ident.name.as_str(), &expr.right);
        }
    }

    /// Update the direct object bindings after the walk of an assignment.
    pub(super) fn record_direct_binding_assignment(&mut self, expr: &AssignmentExpression<'a>) {
        if self.invalidate_projected_assignment(&expr.left) {
            return;
        }

        let is_plain_assignment = matches!(expr.operator, AssignmentOperator::Assign);
        match &expr.left {
            AssignmentTarget::AssignmentTargetIdentifier(ident) => {
                self.record_direct_object_binding_assignment(
                    ident.name.as_str(),
                    &expr.right,
                    is_plain_assignment,
                );
            }
            AssignmentTarget::StaticMemberExpression(member) => {
                if let Some(object_name) = static_member_object_name(&member.object) {
                    self.record_direct_object_binding_assignment(
                        &format!("{object_name}.{}", member.property.name),
                        &expr.right,
                        is_plain_assignment,
                    );
                }
            }
            AssignmentTarget::ComputedMemberExpression(member) => {
                if let Some(object_name) = static_member_object_name(&member.object) {
                    if let Some(property_name) = member.static_property_name() {
                        self.record_direct_object_binding_assignment(
                            &format!("{object_name}.{}", property_name.text()),
                            &expr.right,
                            is_plain_assignment,
                        );
                    } else {
                        self.invalidate_direct_binding_path(&object_name, true);
                    }
                }
            }
            AssignmentTarget::ObjectAssignmentTarget(_)
            | AssignmentTarget::ArrayAssignmentTarget(_) => {
                self.invalidate_assignment_target_proof(&expr.left);
            }
            _ => {}
        }
    }

    pub(super) fn record_string_coercion_to_string(&mut self, expr: &Expression<'_>) {
        if let Some(class_name) = new_expression_class_name(unwrap_static_expr(expr)) {
            self.member_accesses.push(MemberAccess {
                object: class_name,
                member: "toString".to_string(),
            });
        }
    }
}
