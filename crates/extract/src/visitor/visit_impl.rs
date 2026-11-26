//! `Visit` trait implementation for `ModuleInfoExtractor`.

use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_semantic::ScopeFlags;
use oxc_span::{GetSpan, Span};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    AngularComponentFieldArrayTypeFact, AngularThisSpreadFact, ComputedEnumKeyUseFact,
    DefaultImportWholeObjectUseFact, DynamicCustomElementRenderFact, DynamicImportInfo,
    DynamicImportPattern, ExportInfo, ExportName, ImportInfo, ImportedName, MemberAccess,
    ModuleLoadMechanism, PlaywrightFixtureUseFact, ReExportInfo, RequiredTypeMemberFact,
    SemanticFact, VisibilityTag,
    js_text::{JsText, static_name_text},
};
use fallow_types::extract::{
    AngularComponentSelector, ClassHeritageInfo, DiFramework, DiKeySite, DiRole, ImportLoadKind,
    ImportedCallSite, LocalTypeDeclaration, MisplacedDirectiveSite, PublicSignatureTypeReference,
};

use crate::asset_url::normalize_asset_url;
use crate::function_body::BodyRef;

use super::helpers::{
    array_element_type_from_type, escape_glob_literal, extract_angular_component_metadata,
    extract_angular_inputs_outputs, extract_angular_signal_query,
    extract_class_generic_instance_bindings, extract_class_members,
    extract_class_type_parameter_names, extract_concat_parts, extract_custom_element_tag_reference,
    extract_custom_elements_define, extract_implemented_interface_names,
    extract_query_list_element_type, extract_super_class_name, extract_super_class_type_args,
    extract_type_annotation_name, extract_type_reference_name, has_angular_class_decorator,
    has_angular_plural_query_decorator, is_meta_url_arg, lit_custom_element_decorator,
    lit_custom_element_tag, map_value_type_from_new, map_value_type_from_type,
    regex_pattern_to_suffix, return_type_element_name, ts_import_type_qualifier_root,
};
use super::{
    BindingTarget, ModuleInfoExtractor, PendingBindingForward, PendingLocalExportSpecifier,
    ROUTE_LOADER_DATA_OBJECT, SideEffectRegistrationTarget, collect_static_import_specifiers,
    extract_import_expression, extract_promise_all_elements, is_unawaited_import,
    ssr_load_module_call, ssr_load_module_source, try_extract_arrow_wrapped_import,
    try_extract_import_then_callback, try_extract_property_callback_import,
};

#[path = "visit_impl_callback_properties.rs"]
mod visit_callback_properties;
#[path = "visit_impl_chain_names.rs"]
mod visit_chain_names;
#[path = "visit_impl_constructor_alternatives.rs"]
mod visit_constructor_alternatives;
#[path = "visit_impl_di.rs"]
mod visit_di;
#[path = "visit_impl_dynamic_imports.rs"]
pub(super) mod visit_dynamic_imports;
#[path = "visit_impl_factory_properties.rs"]
mod visit_factory_properties;
#[path = "visit_impl_factory_returns.rs"]
mod visit_factory_returns;
#[path = "visit_impl_for_of_bindings.rs"]
mod visit_for_of_bindings;
pub(super) use visit_for_of_bindings::ITERABLE_ELEMENT_CALLBACK_METHODS;
#[path = "visit_impl_computed_keys.rs"]
pub(super) mod visit_computed_keys;
#[path = "visit_impl_function_returns.rs"]
mod visit_function_returns;
#[path = "visit_impl_helpers.rs"]
pub(super) mod visit_helpers;
#[path = "visit_impl_html_element_alias.rs"]
mod visit_html_element_alias;
#[path = "visit_impl_html_template.rs"]
mod visit_html_template;
#[path = "visit_impl_jsx_runtime.rs"]
mod visit_jsx_runtime;
#[path = "visit_impl_member_destructuring.rs"]
mod visit_member_destructuring;
#[path = "visit_impl_value_alias.rs"]
pub(super) mod visit_value_alias;
use visit_value_alias::unwrap_static_expr;
#[path = "visit_impl_type_flow.rs"]
mod type_flow;
#[path = "visit_impl_module_require.rs"]
mod visit_module_require;
#[path = "visit_impl_node_runtime.rs"]
mod visit_node_runtime;
#[path = "visit_impl_object_bindings.rs"]
mod visit_object_bindings;
#[path = "visit_impl_object_helpers.rs"]
mod visit_object_helpers;
#[path = "visit_impl_package_resolution.rs"]
mod visit_package_resolution;
#[path = "visit_impl_pinia.rs"]
mod visit_pinia;
#[path = "visit_impl_playwright.rs"]
mod visit_playwright;
#[path = "visit_impl_program_facts.rs"]
mod visit_program_facts;
#[path = "visit_impl_projections.rs"]
pub(super) mod visit_projections;
#[path = "visit_impl_reflection.rs"]
mod visit_reflection;
#[path = "visit_impl_scope_bindings.rs"]
mod visit_scope_bindings;
#[path = "visit_impl_security_classifiers.rs"]
mod visit_security_classifiers;
#[path = "visit_impl_security_controls.rs"]
mod visit_security_controls;
#[path = "visit_impl_security_routes.rs"]
mod visit_security_routes;
#[path = "visit_impl_security_sanitizers.rs"]
mod visit_security_sanitizers;
#[path = "visit_impl_security_sinks.rs"]
mod visit_security_sinks;
#[path = "visit_impl_server_actions.rs"]
mod visit_server_actions;
#[path = "visit_impl_signature.rs"]
pub(super) mod visit_signature;
#[path = "visit_impl_source_paths.rs"]
mod visit_source_paths;
#[path = "visit_impl_spy_calls.rs"]
mod visit_spy_calls;
#[path = "visit_impl_structural.rs"]
mod visit_structural;
#[path = "visit_impl_svelte_events.rs"]
mod visit_svelte_events;
#[path = "visit_impl_sveltekit_load.rs"]
mod visit_sveltekit_load;
#[path = "visit_impl_taint_sources.rs"]
mod visit_taint_sources;
#[path = "visit_impl_this_assignments.rs"]
mod visit_this_assignments;

pub(super) use visit_chain_names::jsx_member_object_name;
pub use visit_chain_names::push_member_tag_accesses;
use visit_chain_names::{call_receiver_name, static_member_object_name, type_name_member_path};
pub(super) use visit_dynamic_imports::{ComponentLoaderImport, NewUrlContext};
pub(super) use visit_factory_returns::count_returns_in_statements;
use visit_factory_returns::{
    FactoryReturnFunctionInput, classify_factory_assigned_value, collect_self_scope_assignments,
    function_body_is_terminal, function_body_returns_identifier, function_body_returns_new_class,
    function_body_returns_new_class_unanimous, function_body_returns_object_shape,
};
use visit_helpers::*;
use visit_object_helpers::*;
use visit_package_resolution::*;
use visit_scope_bindings::loop_head_declaration;
use visit_security_classifiers::*;
pub(super) use visit_security_routes::function_body_has_use_server;
use visit_security_routes::*;
pub(super) use visit_security_sinks::SinkAliasState;
pub(super) use visit_security_sinks::flatten_callee_path;
pub(super) use visit_svelte_events::MarkupContext;
#[path = "visit_impl_taint_chain.rs"]
mod taint_chain;
#[path = "visit_impl_cjs_exports.rs"]
mod visit_cjs_exports;
#[path = "visit_impl_expression_shapes.rs"]
mod visit_expression_shapes;
#[path = "visit_impl_framework_elements.rs"]
mod visit_framework_elements;
#[path = "visit_impl_module_items.rs"]
mod visit_module_items;
#[path = "visit_impl_namespace_references.rs"]
mod visit_namespace_references;
#[path = "visit_impl_top_level_declarations.rs"]
mod visit_top_level_declarations;
#[path = "visit_impl_value_bindings.rs"]
mod visit_value_bindings;
#[path = "visit_impl_walk_records.rs"]
mod visit_walk_records;
use taint_chain::collect_chained_taint_idents;
use visit_expression_shapes::*;
pub(super) use visit_source_paths::join_member_path;
use visit_source_paths::{
    binding_source_path_candidates, flatten_member_path, push_unique_string, source_path_candidates,
};
pub(super) use visit_spy_calls::SpyApi;

const ROUTE_LOADER_DATA_SOURCES: &[&str] =
    &["react-router", "react-router-dom", "@remix-run/react"];

/// Per-module breadth cap on recorded factory-return candidates (issue #1843
/// follow-up): the companion to `MAX_TAINTED_BINDINGS_PER_MODULE` for the
/// `const local = callee(...)` factory-return channel. `factory_return_candidates`
/// grows once per bare-callee assignment in the module and is resolved by an
/// O(n) finalize pass, so a dense machine-generated bundle that emits tens of
/// thousands of such assignments drove the working set (and the finalize scan)
/// super-linearly. Past the cap no NEW candidate is recorded, degrading an
/// over-cap file to the pre-existing module-level reachability instead of an
/// arg-level factory-return claim, matching the false-negative-preferring
/// direction of the depth/breadth taint caps. Deliberately a constant, not a
/// config knob: real hand-written modules stay far below it.
const MAX_FACTORY_RETURN_CANDIDATES: usize = 4096;

fn is_css_module_import_source(source: &str) -> bool {
    let path = source.split(['?', '#']).next().unwrap_or(source);
    let Some(file_name) = path.rsplit('/').next() else {
        return false;
    };
    let Some((stem, ext)) = file_name.rsplit_once('.') else {
        return false;
    };
    stem.ends_with(".module") && matches!(ext, "css" | "scss" | "sass" | "less")
}

impl<'a> Visit<'a> for ModuleInfoExtractor {
    fn visit_program(&mut self, program: &Program<'a>) {
        // An SFC or Astro file visits each script with one extractor, and its
        // spans can repeat, so a mark must not reach the next program.
        self.fluent_chain_inner_calls.clear();
        self.record_top_level_runtime_statement(program);
        self.record_program_directives(program);
        self.record_html_tag_provenance(program);
        self.is_server_action_module = visit_server_actions::is_server_action_module(program);
        self.is_module_file = program_has_module_syntax(program);
        self.has_global_declarations =
            program_has_global_declarations(program, self.is_module_file);
        self.record_triple_slash_references(program);
        self.capture_subclass_provenance = program.body.iter().any(|statement| match statement {
            Statement::ClassDeclaration(class) => class.heritage.is_some(),
            Statement::ExportDeclaration(export) => matches!(&export.declaration, Declaration::ClassDeclaration(class) if class.heritage.is_some()),
            _ => false,
        });
        self.capture_factory_properties =
            visit_factory_properties::program_has_object_factory(program)
                || visit_factory_properties::program_has_captured_callback(program);
        self.prepare_factory_property_imports(program);
        self.record_program_namespace_import_locals(program);
        self.record_program_module_require_names(program);
        self.record_program_spy_api_locals(program);
        self.record_program_type_aliases(program);
        self.record_program_prologue(program);
        self.record_program_function_facts(program);
        self.record_local_function_return_types(program);
        self.preseed_direct_object_binding_targets(&program.body);
        self.record_module_vars(&program.body);
        self.record_trusted_literal_keys(program);
        self.record_program_local_import_loaders(program);
        self.record_program_component_loader_imports(program);
        self.walk_program_and_finish_inert_callees(program);
        self.finish_local_import_loaders();
        self.finish_promise_import_bindings();
        self.finish_register_loader_new_urls();
        self.record_jsx_import_source_pragma(program);
    }

    fn visit_formal_parameter(&mut self, param: &FormalParameter<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &param.pattern
            && let Some(type_annotation) = param.type_annotation.as_deref()
        {
            self.record_binding_declaration(id.name.as_str(), id.span);
            if let Some(name) = extract_type_annotation_name(type_annotation)
                && let Some(target) = self.resolve_class_type_param(&name)
            {
                let origin = self.resolve_class_type_param_origin(
                    &name,
                    super::helpers::extract_type_reference_origin(&type_annotation.type_annotation),
                );
                self.record_binding_class_proof(id.name.as_str(), &target, origin);
            }
            // The parameter itself is resolved by the scope-owned body collector.
            // Only nested paths remain in the module fallback for consumers such
            // as typed store properties; conflicting scopes become Ambiguous.
            self.record_typed_nested_bindings(id.name.as_str(), type_annotation);
            if let Some((element, origin)) =
                super::helpers::array_element_target_from_type(&type_annotation.type_annotation)
            {
                self.record_binding_iteration_proof(id.name.as_str(), &element, origin, false);
            }
            if param.accessibility.is_some() {
                let key = self.this_member_key(id.name.as_str());
                self.record_typed_binding(key.as_str(), type_annotation);
            }
        }

        for id in param.pattern.get_binding_identifiers() {
            self.record_binding_declaration(id.name.as_str(), id.span);
        }
        if let BindingPattern::ObjectPattern(obj_pat) = &param.pattern
            && let Some(type_annotation) = param.type_annotation.as_deref()
        {
            self.record_typed_destructure_binding(obj_pat, type_annotation);
        }

        self.record_angular_param_inject(param);

        if let BindingPattern::BindingIdentifier(id) = &param.pattern
            && let Some(annotation) = param.type_annotation.as_deref()
        {
            self.record_projection_contract(id.name.as_str(), id.span, &annotation.type_annotation);
        }
        walk::walk_formal_parameter(self, param);
    }

    fn visit_method_definition(&mut self, method: &MethodDefinition<'a>) {
        let previous = self.enter_method_definition(method);
        walk::walk_method_definition(self, method);
        self.exit_method_definition(previous);
    }

    fn visit_property_definition(&mut self, prop: &PropertyDefinition<'a>) {
        // Effective member key: a static name (`dep`) or a `#`-private field
        // (`#dep`, spelled with the leading `#` so the binding key is
        // `this.#dep`). Private-field receivers must record the same
        // binding-key + member-access pair public fields do, so `this.#dep.m()`
        // credits the imported class member cross-module (issue #1821).
        let member_key = prop.key.static_name().map_or_else(
            || match &prop.key {
                PropertyKey::PrivateIdentifier(id) => Some(format!("#{}", id.name)),
                _ => None,
            },
            |name| Some(name.text().into_owned()),
        );

        if let Some(name) = member_key.as_deref() {
            // Class-scoped key so a same-named field on a sibling class does not
            // collide in the module-flat `binding_target_names` (issue #1821).
            let this_key = self.this_member_key(name);
            if let Some(type_annotation) = prop.type_annotation.as_deref() {
                self.record_typed_binding(this_key.as_str(), type_annotation);

                if has_angular_plural_query_decorator(&prop.decorators)
                    && let Some(element_type) = extract_query_list_element_type(type_annotation)
                {
                    self.iterable_element_types
                        .insert(this_key.clone(), element_type);
                }
            }

            self.record_map_binding_from_declaration(
                this_key.clone(),
                prop.type_annotation.as_deref(),
                prop.value.as_ref(),
            );

            if let Some(Expression::NewExpression(new_expr)) = &prop.value
                && let Expression::Identifier(callee) = &new_expr.callee
                && !super::helpers::is_builtin_constructor(callee.name.as_str())
            {
                self.insert_class_binding_target(this_key.clone(), callee.name.to_string());
                self.record_binding_class_proof(&this_key, callee.name.as_str(), Some(callee.span));
            }

            if let Some(Expression::CallExpression(call)) = &prop.value
                && let Some(type_name) = self.extract_angular_inject_target(call)
            {
                self.insert_class_binding_target(this_key.clone(), type_name);
            }

            if let Some(value) = prop.value.as_ref() {
                self.record_value_alias(this_key.clone(), value);
            }

            if let Some(value) = prop.value.as_ref()
                && let Some(query) = extract_angular_signal_query(value)
            {
                let call_key = format!("{this_key}()");
                if query.plural {
                    self.iterable_element_types.insert(call_key, query.type_arg);
                } else {
                    self.insert_class_binding_target(call_key, query.type_arg);
                }
            }
        }

        // The hardcoded-secret sink stays gated on `static_name()` only: a
        // `#`-private field is never a security-detector secret target, so
        // widening the sink here is out of scope for the DI-credit fix.
        if let Some(name) = prop.key.static_name()
            && let Some(value) = prop.value.as_ref()
        {
            self.capture_hardcoded_secret_literal_sink(&name.text(), value, prop.span);
        }
        self.record_this_bound_function(prop.value.as_ref());

        walk::walk_property_definition(self, prop);
    }

    fn visit_block_statement(&mut self, stmt: &BlockStatement<'a>) {
        self.block_depth += 1;
        let type_alias_scope_pushed =
            self.namespace_depth == 0 && self.push_function_type_alias_scope(&stmt.body);
        self.push_direct_object_binding_scope(FxHashSet::default());
        self.preseed_direct_object_binding_scope_roots(&stmt.body);
        if self.namespace_depth == 0 {
            self.push_nested_scope_frames();
            self.preseed_scope_declarations(&stmt.body);
        }
        self.preseed_direct_object_binding_targets(&stmt.body);
        for statement in &stmt.body {
            self.visit_statement(statement);
            if self.namespace_depth == 0 {
                self.record_fail_closed_guard_after_statement(statement);
            }
        }
        if type_alias_scope_pushed {
            self.pop_function_type_alias_scope();
        }
        if self.namespace_depth == 0 {
            self.pop_nested_scope_frames();
        }
        self.pop_direct_object_binding_scope();
        self.block_depth -= 1;
    }

    fn visit_switch_statement(&mut self, stmt: &SwitchStatement<'a>) {
        let type_alias_scope_pushed = self.namespace_depth == 0
            && self.push_function_type_alias_scope(
                stmt.cases.iter().flat_map(|case| &case.consequent),
            );
        walk::walk_switch_statement(self, stmt);
        if type_alias_scope_pushed {
            self.pop_function_type_alias_scope();
        }
    }

    fn visit_switch_cases(&mut self, cases: &oxc_allocator::Vec<'a, SwitchCase<'a>>) {
        self.push_direct_object_binding_scope(FxHashSet::default());
        for case in cases {
            self.preseed_direct_object_binding_scope_roots(&case.consequent);
            self.preseed_direct_object_binding_targets(&case.consequent);
        }
        for case in cases {
            let values = self.push_conditional_binding_scope(&[]);
            self.visit_switch_case(case);
            self.pop_optional_binding_scope(values);
        }
        self.pop_direct_object_binding_scope();
    }

    fn visit_static_block(&mut self, block: &StaticBlock<'a>) {
        let type_alias_scope_pushed =
            self.namespace_depth == 0 && self.push_function_type_alias_scope(&block.body);
        let var_roots = Self::var_binding_roots(&block.body);
        self.push_var_owner_direct_object_binding_scope(var_roots);
        self.preseed_direct_object_binding_scope_roots(&block.body);
        self.preseed_direct_object_binding_targets(&block.body);
        walk::walk_static_block(self, block);
        self.pop_direct_object_binding_scope();
        if type_alias_scope_pushed {
            self.pop_function_type_alias_scope();
        }
    }

    fn visit_catch_clause(&mut self, clause: &CatchClause<'a>) {
        let roots: FxHashSet<String> = clause
            .param
            .as_ref()
            .into_iter()
            .flat_map(|param| param.pattern.get_binding_identifiers())
            .map(|id| id.name.to_string())
            .collect();
        self.push_catch_clause_scope(roots);
        walk::walk_catch_clause(self, clause);
        self.pop_catch_clause_scope();
    }

    fn visit_declaration(&mut self, decl: &Declaration<'a>) {
        if self.block_depth == 0 && self.function_depth == 0 && self.namespace_depth == 0 {
            self.record_top_level_declaration(decl);
        } else if self.namespace_depth == 0 {
            self.record_nested_declaration(decl);
        }

        walk::walk_declaration(self, decl);
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        let constructor_function = self.active_constructor_function.replace(func.span);
        let function_this = self.enter_function_this(func);
        self.record_next_function_param_sources(func);
        let type_parameter_scope_pushed = func
            .type_parameters
            .as_deref()
            .is_some_and(|params| self.push_function_type_parameter_scope(params));
        self.record_function_member_destructuring(func);
        self.record_scoped_typed_parameter_accesses(
            &func.params,
            func.body.as_deref().map(BodyRef::Block),
            func.return_type.as_deref(),
        );
        let var_roots = func
            .body
            .as_deref()
            .map_or_else(FxHashSet::default, |body| {
                Self::var_binding_roots(&body.statements)
            });
        let self_roots = func
            .id
            .as_ref()
            .map(|id| FxHashSet::from_iter([id.name.to_string()]))
            .unwrap_or_default();
        let mut direct_roots = self_roots;
        for param in &func.params.items {
            direct_roots.extend(
                param
                    .pattern
                    .get_binding_identifiers()
                    .into_iter()
                    .map(|id| id.name.to_string()),
            );
        }
        self.push_var_owner_direct_object_binding_scope(direct_roots);
        self.direct_object_binding_function_scopes
            .push(self.scoped_direct_object_binding_targets.len() - 1);
        self.push_function_declaration_scope(&func.params);
        self.bind_map_callback_value(func.span.start);
        self.bind_then_callback_namespace(func.span.start);
        self.deferred_function_var_binding_roots.push(var_roots);
        self.function_depth += 1;
        let component_pushed = self.react_enter_function(func);
        walk::walk_function(self, func, flags);
        self.react_exit_component(component_pushed);
        self.function_depth -= 1;
        self.deferred_function_var_binding_roots.pop();
        if type_parameter_scope_pushed {
            self.pop_function_type_alias_scope();
        }
        self.pop_function_declaration_scope();
        self.direct_object_binding_function_scopes.pop();
        self.pop_direct_object_binding_scope();
        self.exit_function_this(function_this);
        self.active_constructor_function = constructor_function;
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        let constructor_function = self.active_constructor_function.replace(expr.span);
        self.record_next_arrow_param_sources(expr);
        let type_parameter_scope_pushed = expr
            .type_parameters
            .as_deref()
            .is_some_and(|params| self.push_function_type_parameter_scope(params));
        self.record_parameter_member_destructuring(&expr.params);
        self.record_scoped_typed_parameter_accesses(
            &expr.params,
            Some(BodyRef::arrow(&expr.body)),
            expr.return_type.as_deref(),
        );
        // A concise body holds no `var` declaration outside the nested functions
        // and classes that the root collector skips.
        let var_roots = expr
            .body
            .as_function_body()
            .map_or_else(FxHashSet::default, |body| {
                Self::var_binding_roots(&body.statements)
            });
        let direct_roots = expr
            .params
            .items
            .iter()
            .flat_map(|param| param.pattern.get_binding_identifiers())
            .map(|id| id.name.to_string())
            .collect();
        self.push_var_owner_direct_object_binding_scope(direct_roots);
        self.direct_object_binding_function_scopes
            .push(self.scoped_direct_object_binding_targets.len() - 1);
        self.push_function_declaration_scope(&expr.params);
        self.bind_map_callback_value(expr.span.start);
        self.bind_then_callback_namespace(expr.span.start);
        self.deferred_function_var_binding_roots.push(var_roots);
        self.function_depth += 1;
        let component_pushed = self.react_enter_arrow(expr);
        walk::walk_arrow_function_expression(self, expr);
        self.react_exit_component(component_pushed);
        self.function_depth -= 1;
        self.deferred_function_var_binding_roots.pop();
        if type_parameter_scope_pushed {
            self.pop_function_type_alias_scope();
        }
        self.pop_function_declaration_scope();
        self.direct_object_binding_function_scopes.pop();
        self.pop_direct_object_binding_scope();
        self.active_constructor_function = constructor_function;
    }

    fn visit_arrow_function_body(&mut self, body: &ArrowFunctionBody<'a>) {
        self.visit_body(BodyRef::arrow(body));
    }

    fn visit_function_body(&mut self, body: &FunctionBody<'a>) {
        self.visit_body(BodyRef::Block(body));
    }

    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        let source = decl.source.value.text().into_owned();
        let is_type_only = decl.import_kind.is_type();
        let source_span = decl.source.span;

        match &decl.specifiers {
            // `import {} from 'x'` and `import type {} from 'x'` bind nothing
            // but still name the module, so record them as side-effect
            // imports. The type-only form keeps its type-only flag.
            Some(specifiers) if specifiers.is_empty() => {
                self.push_side_effect_import(decl, source, is_type_only);
            }
            Some(specifiers) => {
                for spec in specifiers {
                    match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            self.handle_import_specifier(s, &source, is_type_only, source_span);
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            self.handle_import_default_specifier(
                                s,
                                &source,
                                is_type_only,
                                source_span,
                            );
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            self.handle_import_namespace_specifier(
                                s,
                                &source,
                                is_type_only,
                                source_span,
                            );
                        }
                    }
                }
            }
            None => self.push_side_effect_import(decl, source, false),
        }
        self.mark_macro_import_bindings(decl);
    }

    fn visit_ts_export_assignment(&mut self, decl: &TSExportAssignment<'a>) {
        // `export = x` inside `declare module` or a namespace body describes
        // that declaration, not this file.
        if self.ambient_module_depth == 0
            && self.namespace_depth == 0
            && self.local_namespace_depth == 0
            && let Expression::Identifier(identifier) = &decl.expression
        {
            self.pending_binding_forwards
                .push(PendingBindingForward::ExportAssignment {
                    local_name: identifier.name.to_string(),
                    span: decl.span,
                });
        }
        walk::walk_ts_export_assignment(self, decl);
    }

    fn visit_ts_import_equals_declaration(&mut self, decl: &TSImportEqualsDeclaration<'a>) {
        self.handle_import_equals_declaration(decl);
        walk::walk_ts_import_equals_declaration(self, decl);
    }

    fn visit_export_declaration(&mut self, decl: &ExportDeclaration<'a>) {
        let is_namespace = matches!(
            &decl.declaration,
            Declaration::TSNamespaceDeclaration(_) | Declaration::TSExternalModuleDeclaration(_)
        );

        // Exports inside a namespace declared without the `export` keyword are
        // members of that local binding, not exports of the containing file
        // (issue #2356). TS2395 forbids merging a local namespace with an
        // exported declaration of the same name, so there is no exported owner
        // to attach members to: nothing is recorded here and nothing is queued
        // in `pending_namespace_members`. The body is still walked so imports
        // referenced inside it keep their credit and nested namespaces reach
        // `visit_ts_namespace_declaration`.
        if self.local_namespace_depth > 0 {
            walk::walk_export_declaration(self, decl);
            return;
        }

        if self.namespace_depth > 0 {
            self.extract_namespace_members(&decl.declaration);
            if let Declaration::TSNamespaceDeclaration(namespace) = &decl.declaration {
                self.exported_namespace_member = Some(namespace.span);
            }
            if is_namespace {
                self.namespace_depth += 1;
            }
            walk::walk_export_declaration(self, decl);
            if is_namespace {
                self.namespace_depth -= 1;
            }
            return;
        }

        // Exports inside `declare module '<specifier>' { ... }` augment the
        // named module; they are not exports of the containing file, so they
        // must not feed unused-export/unused-type findings (issue #2349). The
        // body is still walked so `typeof import()` references and type usage
        // keep extracting (#396/#397).
        if self.ambient_module_depth > 0 {
            walk::walk_export_declaration(self, decl);
            return;
        }

        self.record_export_declaration(&decl.declaration, decl.export_kind().is_type());

        if is_namespace {
            self.namespace_depth += 1;
            self.pending_namespace_members.clear();
            self.namespace_member_path.clear();
            if let Declaration::TSNamespaceDeclaration(namespace) = &decl.declaration {
                self.exported_namespace_root = Some(namespace.span);
            }
        }
        walk::walk_export_declaration(self, decl);
        if is_namespace {
            self.namespace_depth -= 1;
            if let Some(ns_export) = self.exports.last_mut() {
                ns_export.members = std::mem::take(&mut self.pending_namespace_members);
            }
        }
    }

    fn visit_export_named_declaration(&mut self, decl: &ExportNamedDeclaration<'a>) {
        self.mark_export_specifier_namespace_references(&decl.specifiers);
        // See the local-namespace, namespace and ambient-module guards in
        // `visit_export_declaration`.
        if self.local_namespace_depth > 0
            || self.namespace_depth > 0
            || self.ambient_module_depth > 0
        {
            walk::walk_export_named_declaration(self, decl);
            return;
        }
        self.record_export_local_specifiers(decl, decl.export_kind.is_type());
        walk::walk_export_named_declaration(self, decl);
    }

    fn visit_export_from_declaration(&mut self, decl: &ExportFromDeclaration<'a>) {
        self.mark_export_specifier_namespace_references(&decl.specifiers);
        // See the local-namespace and namespace guards in
        // `visit_export_declaration`.
        if self.local_namespace_depth > 0 || self.namespace_depth > 0 {
            walk::walk_export_from_declaration(self, decl);
            return;
        }

        // Inside `declare module '<specifier>'` (see `visit_export_declaration`)
        // a named re-export becomes one type-space import per specifier
        // (mirroring `visit_ts_import_type`) so the target file stays reachable
        // AND each re-exported symbol keeps its export credit; a bare
        // `export {} from '...'` falls back to a side-effect reference for
        // reachability alone.
        if self.ambient_module_depth > 0 {
            let source = &decl.source;
            if decl.specifiers.is_empty() {
                self.imports.push(ImportInfo {
                    source: source.value.text().into_owned(),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: true,
                    is_type_only_star: false,
                    from_style: false,
                    span: decl.span,
                    source_span: source.span,
                });
            } else {
                for spec in &decl.specifiers {
                    self.imports.push(ImportInfo {
                        source: source.value.text().into_owned(),
                        imported_name: ImportedName::Named(spec.local.name().to_string()),
                        local_name: String::new(),
                        is_type_only: true,
                        is_type_only_star: false,
                        from_style: false,
                        span: spec.span,
                        source_span: source.span,
                    });
                }
            }
            walk::walk_export_from_declaration(self, decl);
            return;
        }

        self.record_export_re_exports(decl, decl.export_kind.is_type());
        walk::walk_export_from_declaration(self, decl);
    }

    fn visit_export_default_declaration(&mut self, decl: &ExportDefaultDeclaration<'a>) {
        // See the ambient-module guard in `visit_export_named_declaration`:
        // a default export declared inside `declare module '<specifier>'`
        // belongs to the named module, not the containing file (issue #2349).
        if self.ambient_module_depth > 0 {
            walk::walk_export_default_declaration(self, decl);
            return;
        }

        let (members, super_class, implemented_interfaces, instance_bindings) =
            if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &decl.declaration {
                let is_angular = has_angular_class_decorator(class);
                self.record_angular_inputs_outputs(class, is_angular);
                let bindings = super::helpers::extract_class_instance_bindings(
                    class,
                    |local_name, source, imported_name| {
                        self.is_named_import_from(local_name, source, imported_name)
                    },
                );
                (
                    extract_class_members(class, is_angular, self.heritage_objects()),
                    extract_super_class_name(class),
                    extract_implemented_interface_names(class),
                    bindings,
                )
            } else {
                (vec![], None, vec![], vec![])
            };
        let local_name = match &decl.declaration {
            ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                class.id.as_ref().map(|id| id.name.to_string())
            }
            ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                function.id.as_ref().map(|id| id.name.to_string())
            }
            ExportDefaultDeclarationKind::Identifier(identifier) => {
                self.pending_binding_forwards
                    .push(PendingBindingForward::ExportDefault {
                        local_name: identifier.name.to_string(),
                    });
                Some(identifier.name.to_string())
            }
            _ => None,
        };

        self.record_default_export_signature_refs(decl);

        if super_class.is_some()
            || !implemented_interfaces.is_empty()
            || !instance_bindings.is_empty()
        {
            let (type_parameters, super_class_type_args, generic_instance_bindings) =
                if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &decl.declaration {
                    (
                        extract_class_type_parameter_names(class),
                        extract_super_class_type_args(class),
                        extract_class_generic_instance_bindings(class),
                    )
                } else {
                    (vec![], vec![], vec![])
                };
            self.class_heritage.push(ClassHeritageInfo {
                export_name: "default".to_string(),
                super_class: super_class.clone(),
                implements: implemented_interfaces,
                type_parameters,
                instance_bindings,
                super_class_type_args,
                generic_instance_bindings,
                ..Default::default()
            });
        }
        self.exports.push(ExportInfo {
            name: ExportName::Default,
            local_name,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: decl.span,
            members,
            super_class,
            deprecated: false,
            deprecated_reason: None,
        });

        walk::walk_export_default_declaration(self, decl);
    }

    fn visit_ts_external_module_declaration(&mut self, decl: &TSExternalModuleDeclaration<'a>) {
        // A string-literal module name (`declare module './library'`,
        // `declare module 'pkg'`) marks a module augmentation or ambient
        // module declaration. Track the depth so export visitors skip
        // file-level recording inside the body (issue #2349).
        self.record_module_augmentation(decl);
        self.ambient_module_depth += 1;
        let body_statements = decl.body.as_ref().map(|block| &block.body);
        self.with_module_body_binding_scope(body_statements, None, |visitor| {
            walk::walk_ts_external_module_declaration(visitor, decl);
        });
        self.ambient_module_depth -= 1;
    }

    fn visit_ts_namespace_declaration(&mut self, decl: &TSNamespaceDeclaration<'a>) {
        // A namespace reached outside an exported namespace body
        // (`export namespace Foo` raises `namespace_depth` before its
        // declaration is walked) and outside an ambient module is a local
        // binding: `namespace Foo {}`, `declare namespace Foo {}`, legacy
        // `module Foo {}`, each segment of a dotted `namespace A.B.C {}`, and a
        // namespace nested in one of those or in `declare global`. Its inner
        // `export` statements are members of the local binding, not file
        // exports (issue #2356).
        //
        // Inside an exported namespace body, a nested namespace is a member
        // only when it is exported itself (or is a segment of a dotted
        // `namespace A.B`). A nested namespace without `export` is local to
        // the body, so its inner exports are not members of the root.
        let in_exported_body = self.namespace_depth > 0
            && self.ambient_module_depth == 0
            && self.local_namespace_depth == 0;
        let is_root = in_exported_body && self.exported_namespace_root == Some(decl.span);
        let is_member = in_exported_body
            && !is_root
            && self.exported_namespace_member.take() == Some(decl.span);
        let is_local_namespace = (self.namespace_depth == 0 && self.ambient_module_depth == 0)
            || (in_exported_body && !is_root && !is_member);
        if is_root {
            self.exported_namespace_root = None;
        }
        if is_local_namespace {
            self.local_namespace_depth += 1;
        }
        if is_member {
            self.namespace_member_path.push(decl.id.name.to_string());
        }
        if (is_root || is_member)
            && let TSNamespaceDeclarationBody::TSNamespaceDeclaration(child) = &decl.body
        {
            self.push_namespace_member(child.id.name.to_string(), child.id.span);
            self.exported_namespace_member = Some(child.span);
        }
        let body_statements = match &decl.body {
            TSNamespaceDeclarationBody::TSModuleBlock(block) => Some(&block.body),
            TSNamespaceDeclarationBody::TSNamespaceDeclaration(_) => None,
        };
        self.with_module_body_binding_scope(body_statements, Some(&decl.id.name), |visitor| {
            walk::walk_ts_namespace_declaration(visitor, decl);
        });
        if is_member {
            self.namespace_member_path.pop();
        }
        if is_local_namespace {
            self.local_namespace_depth -= 1;
        }
    }

    fn visit_export_all_declaration(&mut self, decl: &ExportAllDeclaration<'a>) {
        // `export *` / `export * as ns` inside `declare module '<specifier>'`
        // states that every export of the target is reachable through the
        // declared module name (issue #2357). Recording it as a file-level star
        // re-export laundered the target's exports into the declaring file's
        // surface (see the ambient guard in `visit_export_named_declaration`),
        // while a bare side-effect edge would drop the credit entirely. A
        // type-space namespace import with no local binding is the star
        // surface: the graph credits every named export of the target, and a
        // bare specifier counts as type-only package usage. `export *` never
        // forwards `default`; `export * as ns` forwards the namespace object,
        // whose `default` member is the target's default export, so that form
        // records it as its own import. `export type *` takes the same shape
        // with `is_type_only_star` set (issue #2375): it forwards the same
        // names with every value meaning erased, so the graph credits that
        // surface in the type namespace alone instead of in both.
        if self.ambient_module_depth > 0 {
            let source = decl.source.value.text().into_owned();
            let is_type_only_star = decl.export_kind.is_type();
            self.imports.push(ImportInfo {
                source: source.clone(),
                imported_name: ImportedName::Namespace,
                local_name: String::new(),
                is_type_only: true,
                is_type_only_star,
                from_style: false,
                span: decl.span,
                source_span: decl.source.span,
            });
            if decl.exported.is_some() {
                self.imports.push(ImportInfo {
                    source,
                    imported_name: ImportedName::Default,
                    local_name: String::new(),
                    is_type_only: true,
                    is_type_only_star,
                    from_style: false,
                    span: decl.span,
                    source_span: decl.source.span,
                });
            }
            walk::walk_export_all_declaration(self, decl);
            return;
        }

        let exported_name = decl
            .exported
            .as_ref()
            .map_or_else(|| "*".to_string(), |e| e.name().to_string());

        self.re_exports.push(ReExportInfo {
            source: decl.source.value.text().into_owned(),
            imported_name: "*".to_string(),
            exported_name,
            is_type_only: decl.export_kind.is_type(),
            span: decl.span,
            statement_span: decl.span,
            source_span: decl.source.span,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
        });

        walk::walk_export_all_declaration(self, decl);
    }

    fn visit_await_expression(&mut self, expr: &AwaitExpression<'a>) {
        // A module-level `await import('./x')` holds module evaluation until
        // the target has loaded, so the target loads eagerly, like a static
        // import. Inside a function the import runs on demand. The top level
        // of a component body (Vue `<script setup>`, Svelte instance script,
        // Astro frontmatter) is a function after compilation, so it is lazy.
        if self.function_depth == 0
            && !self.top_level_is_component_body
            && let Expression::ImportExpression(import_expr) = expr.argument.without_parentheses()
        {
            self.mark_import_load_kind(import_expr.span, ImportLoadKind::Static);
        }
        walk::walk_await_expression(self, expr);
    }

    fn visit_import_expression(&mut self, expr: &ImportExpression<'a>) {
        let mut sources = Vec::new();
        self.collect_dynamic_import_specifiers(&expr.source, &mut sources);
        if !sources.is_empty() {
            self.record_dynamic_import_branch_patterns(&expr.source, expr.span);
        }
        if self.handled_import_spans.contains(&expr.span) {
            walk::walk_import_expression(self, expr);
            return;
        }
        // Without a static specifier, fall back to the pattern shapes.
        if sources.is_empty() {
            self.record_dynamic_import_pattern(&expr.source, expr.span);
        } else {
            self.push_unbound_dynamic_import_branches(&sources, expr.span);
        }

        walk::walk_import_expression(self, expr);
    }

    fn visit_expression_statement(&mut self, statement: &ExpressionStatement<'a>) {
        self.record_discarded_dynamic_import(&statement.expression);
        walk::walk_expression_statement(self, statement);
    }

    fn visit_return_statement(&mut self, statement: &ReturnStatement<'a>) {
        walk::walk_return_statement(self, statement);
        self.complete_projection_array_return(statement);
        self.complete_factory_property_return(statement);
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        // Pre-register named arrow / function-expression component bindings so
        // the function-body walk (below) can push the component stack with the
        // binding name. Runs before the body is walked. No-op on non-JSX files.
        self.react_prescan_variable_declaration(decl);
        // Export-wrapped declarations miss the statement pre-scan, so seed their direct
        // object targets here too. Ordinary declarations are idempotent: target sets dedupe.
        self.preseed_direct_object_binding_declaration(decl);
        for declarator in &decl.declarations {
            self.record_variable_declarator(decl, declarator);
            self.record_literal_key_declarator(declarator);
            self.record_declarator_function_name(declarator);
            self.record_handler_object_declarator(declarator);
            self.record_destructured_env_reads(declarator);
        }
        walk::walk_variable_declaration(self, decl);
    }

    fn visit_ts_satisfies_expression(&mut self, expr: &TSSatisfiesExpression<'a>) {
        self.record_exported_handler_env(&expr.expression, &expr.type_annotation);
        walk::walk_ts_satisfies_expression(self, expr);
    }

    fn visit_ts_as_expression(&mut self, expr: &TSAsExpression<'a>) {
        self.record_exported_handler_env(&expr.expression, &expr.type_annotation);
        walk::walk_ts_as_expression(self, expr);
    }

    fn visit_object_property(&mut self, prop: &ObjectProperty<'a>) {
        self.capture_factory_property_observation(&prop.value);

        if let Some((import_expr, sources)) = try_extract_property_callback_import(prop) {
            self.push_dynamic_import_branches(
                &sources,
                import_expr.span,
                &["default".to_string()],
                None,
            );
            self.handled_import_spans.insert(import_expr.span);
        }

        if let Some(name) = prop.key.static_name() {
            self.capture_hardcoded_secret_literal_sink(&name.text(), &prop.value, prop.span);
        }

        self.record_angular_entry_component_refs(prop);
        self.record_property_function_name(prop);

        walk::walk_object_property(self, prop);
    }

    fn visit_object_expression(&mut self, obj: &ObjectExpression<'a>) {
        self.record_angular_provider_object(obj);
        self.record_graphql_resolver_map_sources(obj);

        walk::walk_object_expression(self, obj);
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        self.record_call_expression_facts(expr);
        walk::walk_call_expression(self, expr);
        self.react_record_memo_comparator(expr);
    }

    fn visit_for_statement(&mut self, stmt: &ForStatement<'a>) {
        let mut roots = FxHashSet::default();
        if let Some(ForStatementInit::VariableDeclaration(declaration)) = &stmt.init
            && declaration.kind.is_lexical()
        {
            roots.extend(
                declaration
                    .declarations
                    .iter()
                    .flat_map(|declarator| declarator.id.get_binding_identifiers())
                    .map(|id| id.name.to_string()),
            );
        }
        self.push_direct_object_binding_scope(roots);
        let init_declaration = match &stmt.init {
            Some(ForStatementInit::VariableDeclaration(declaration)) => Some(&**declaration),
            _ => None,
        };
        let loop_scope = self.push_loop_head_scope(init_declaration);
        if let Some(declaration) = init_declaration {
            self.preseed_direct_object_binding_declaration(declaration);
        }
        if let Some(init) = &stmt.init {
            self.visit_for_statement_init(init);
        }
        if let Some(test) = &stmt.test {
            self.visit_expression(test);
        }
        let values = self.push_conditional_binding_scope(&[]);
        if let Some(update) = &stmt.update {
            let update_values = self.push_conditional_binding_scope(&[]);
            self.start_loop_update_observations();
            self.visit_expression(update);
            let observations = self.finish_loop_update_observations();
            let update_branch = self.pop_conditional_binding_scope(update_values);
            self.join_conditional_binding_scopes(
                &update_branch,
                &super::DirectObjectBindingBranch::default(),
            );
            self.visit_statement(&stmt.body);
            self.replay_loop_update_observations(observations);
            self.join_conditional_binding_scopes(
                &update_branch,
                &super::DirectObjectBindingBranch::default(),
            );
        } else {
            self.visit_statement(&stmt.body);
        }
        self.pop_optional_binding_scope(values);
        self.pop_loop_head_scope(loop_scope);
        self.pop_direct_object_binding_scope();
    }

    fn visit_for_of_statement(&mut self, stmt: &ForOfStatement<'a>) {
        let static_bindings = self.static_package_loop_bindings(stmt);
        let has_static_bindings = static_bindings.is_some();
        if let Some((strings, objects)) = static_bindings {
            self.loop_string_bindings.push(strings);
            self.loop_object_property_values.push(objects);
        }

        let mut roots = FxHashSet::default();
        if let ForStatementLeft::VariableDeclaration(declaration) = &stmt.left
            && declaration.kind.is_lexical()
        {
            roots.extend(
                declaration
                    .declarations
                    .iter()
                    .flat_map(|declarator| declarator.id.get_binding_identifiers())
                    .map(|id| id.name.to_string()),
            );
        }
        self.push_direct_object_binding_scope(roots);
        let loop_scope = self.push_loop_head_scope(loop_head_declaration(&stmt.left));
        self.visit_for_statement_left(&stmt.left);
        self.visit_expression(&stmt.right);
        let values = self.push_conditional_binding_scope(&[]);
        if let Some(target) = stmt.left.as_assignment_target() {
            self.invalidate_assignment_target_proof(target);
        }
        self.bind_for_of_element(stmt);
        self.bind_for_of_map_value(stmt);
        self.record_for_of_member_destructuring(stmt);
        self.visit_statement(&stmt.body);
        self.pop_optional_binding_scope(values);
        self.pop_loop_head_scope(loop_scope);
        self.pop_direct_object_binding_scope();

        if has_static_bindings {
            self.loop_object_property_values.pop();
            self.loop_string_bindings.pop();
        }
    }

    fn visit_string_literal(&mut self, lit: &StringLiteral<'a>) {
        self.record_bin_path_references(&lit.value.text());
        walk::walk_string_literal(self, lit);
    }

    fn visit_template_literal(&mut self, tpl: &TemplateLiteral<'a>) {
        for quasi in &tpl.quasis {
            self.record_bin_path_references(quasi.value.raw.as_str());
        }
        let suppress = self.in_tagged_template_quasi;
        self.in_tagged_template_quasi = false;
        if !suppress {
            for interpolation in &tpl.expressions {
                self.record_string_coercion_to_string(interpolation);
            }
        }
        walk::walk_template_literal(self, tpl);
    }

    fn visit_binary_expression(&mut self, expr: &BinaryExpression<'a>) {
        self.record_in_operator_member_probe(expr);
        if expr.operator == oxc_ast::ast::BinaryOperator::Addition {
            if is_string_coercion_sibling(&expr.right) {
                self.record_string_coercion_to_string(&expr.left);
            }
            if is_string_coercion_sibling(&expr.left) {
                self.record_string_coercion_to_string(&expr.right);
            }
        }
        walk::walk_binary_expression(self, expr);
    }

    fn visit_new_expression(&mut self, expr: &oxc_ast::ast::NewExpression<'a>) {
        self.record_queue_worker_constructor_param_sources(expr);
        self.record_callee_use(&expr.callee, expr.span.start, true);
        self.record_new_url_package_reference(expr);
        self.record_worker_new_url_arguments(expr);

        self.record_new_url_edge(expr);

        self.capture_declarative_validation_new_expression(expr);
        self.capture_new_expression_sink(expr);

        walk::walk_new_expression(self, expr);
    }

    /// Trace `typeof import('./path').X` references inside type positions.
    ///
    /// `auto-imports.d.ts` (unplugin-auto-import) and `components.d.ts`
    /// (unplugin-vue-components) embed these references inside
    /// `declare global { ... }` and `declare module 'vue' { ... }` ambient
    /// declarations. Without this handler, oxc walks the bodies but the
    /// `TSImportType` node has no extractor, so the referenced files end up
    /// flagged as `unused-files`. See issues #396 and #397.
    fn visit_ts_import_type(&mut self, node: &oxc_ast::ast::TSImportType<'a>) {
        let source = node.source.value.text().into_owned();
        let source_span = node.source.span;

        let imported_name = node.qualifier.as_ref().map_or_else(
            || ImportedName::SideEffect,
            |q| ImportedName::Named(ts_import_type_qualifier_root(q).to_string()),
        );

        self.imports.push(ImportInfo {
            source,
            imported_name,
            local_name: String::new(),
            is_type_only: true,
            is_type_only_star: false,
            from_style: false,
            span: node.span,
            source_span,
        });

        walk::walk_ts_import_type(self, node);
    }

    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        self.record_assignment_expression_facts(expr);
        walk::walk_assignment_expression(self, expr);
        self.record_direct_binding_assignment(expr);
    }

    fn visit_update_expression(&mut self, expr: &UpdateExpression<'a>) {
        match &expr.argument {
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                if !self.invalidate_projected_property(
                    &member.object,
                    Some(member.property.name.as_str()),
                ) && let Some(path) = static_member_object_name(&member.object)
                {
                    self.invalidate_direct_binding_path(
                        &format!("{path}.{}", member.property.name),
                        false,
                    );
                }
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                if !self.invalidate_projected_computed_property(member)
                    && let Some(path) = static_member_object_name(&member.object)
                {
                    match member.static_property_name().map(JsText::text) {
                        Some(key) => {
                            self.invalidate_direct_binding_path(&format!("{path}.{key}"), false);
                        }
                        None => self.invalidate_direct_binding_path(&path, true),
                    }
                }
            }
            SimpleAssignmentTarget::AssignmentTargetIdentifier(reference) => {
                self.invalidate_direct_binding_path(reference.name.as_str(), false);
            }
            _ => {}
        }
        let mutates_cjs_exports = match &expr.argument {
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                Self::is_static_cjs_assignment_target(member)
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                Self::is_computed_cjs_assignment_target(member)
            }
            _ => false,
        };
        if mutates_cjs_exports {
            self.record_cjs_assignment(false);
        }
        walk::walk_update_expression(self, expr);
    }

    fn visit_unary_expression(&mut self, expr: &UnaryExpression<'a>) {
        if matches!(expr.operator, UnaryOperator::Delete) {
            match &expr.argument {
                Expression::StaticMemberExpression(member) => {
                    if !self.invalidate_projected_property(
                        &member.object,
                        Some(member.property.name.as_str()),
                    ) && let Some(path) = static_member_object_name(&member.object)
                    {
                        self.invalidate_direct_binding_path(
                            &format!("{path}.{}", member.property.name),
                            false,
                        );
                    }
                }
                Expression::ComputedMemberExpression(member) => {
                    if !self.invalidate_projected_computed_property(member)
                        && let Some(path) = static_member_object_name(&member.object)
                    {
                        match member.static_property_name() {
                            Some(key) => {
                                self.invalidate_direct_binding_path(
                                    &format!("{path}.{}", key.text()),
                                    false,
                                );
                            }
                            None => self.invalidate_direct_binding_path(&path, true),
                        }
                    }
                }
                _ => {}
            }
        }
        if matches!(expr.operator, UnaryOperator::Delete)
            && Self::is_cjs_mutation_expression(&expr.argument)
        {
            self.record_cjs_assignment(false);
        }
        walk::walk_unary_expression(self, expr);
    }

    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        self.record_dotted_env_read(expr);
        self.record_pinia_state_access(expr);
        self.capture_factory_property_access(&expr.object, expr.property.name.as_str());
        self.record_projected_member_access(&expr.object, expr.property.name.as_str());
        self.record_subclass_member_access(&expr.object, expr.property.name.as_str());
        self.record_awaited_dynamic_import_member(&expr.object, expr.property.name.as_str());
        if is_import_meta_env_object(&expr.object) {
            self.member_accesses.push(MemberAccess {
                object: "import.meta.env".to_string(),
                member: expr.property.name.to_string(),
            });
        }
        if let Some(type_name) = self.asserted_receiver_type_name(&expr.object) {
            self.member_accesses.push(MemberAccess {
                object: type_name,
                member: expr.property.name.to_string(),
            });
        }
        if let Some(type_name) = self.map_get_value_type(&expr.object) {
            self.member_accesses.push(MemberAccess {
                object: type_name,
                member: expr.property.name.to_string(),
            });
        }
        if let Some(type_name) = self.local_call_array_index_type(&expr.object) {
            self.member_accesses.push(MemberAccess {
                object: type_name,
                member: expr.property.name.to_string(),
            });
        }
        if let Some(callee_name) = Self::bare_call_callee_name(&expr.object)
            && Self::inline_store_factory_receiver(&expr.object).is_none()
        {
            // `useApi().member`, with no local ever naming the result. Only this
            // first-level member belongs to the factory's class: in `f().a.b`, `b` is
            // read off whatever type `a` has, and the outer member expression's
            // object is a member expression, not a call, so it never matches here.
            self.factory_unnamed_result_accesses.push((
                callee_name,
                expr.property.name.to_string(),
                false,
            ));
        }
        if let Some(store_factory) = Self::inline_store_factory_receiver(&expr.object) {
            // `useFooStore().member` has no bound local: credit the factory import,
            // as `record_pinia_store` does for `s.member` (issue #1489 Case 1).
            self.member_accesses.push(MemberAccess {
                object: store_factory,
                member: expr.property.name.to_string(),
            });
        } else if let Some(object_name) = call_receiver_name(&expr.object) {
            let root_span = visit_chain_names::receiver_root_span(&expr.object);
            self.record_named_member_access_at(
                &object_name,
                expr.property.name.as_str(),
                root_span,
            );
        } else if let Expression::ComputedMemberExpression(receiver) = &expr.object
            && receiver.static_property_name().is_none()
            && let Some(object_name) = static_member_object_name(&receiver.object)
        {
            self.record_dynamic_direct_object_binding_member_access(
                &object_name,
                expr.property.name.as_str(),
            );
        }
        if matches!(expr.object, Expression::Super(_))
            && let Some(Some((super_local, origin))) = self.class_super_stack.last().cloned()
        {
            self.member_accesses.push(MemberAccess {
                object: super_local.clone(),
                member: expr.property.name.to_string(),
            });
            if let Some(target_reference_span) = origin {
                self.pending_subclass_member_accesses
                    .push(super::PendingSubclassMemberAccess {
                        proof: super::SubclassAccessProof::Known(super::BindingClassProof {
                            builtin_containers: std::sync::Arc::default(),
                            target_name: super_local,
                            target_reference_span,
                            requirements: super::BindingRequirements::Empty,
                        }),
                        member: expr.property.name.to_string(),
                    });
            }
        }
        if let Expression::Identifier(object) = &expr.object {
            self.mark_structured_namespace_reference(object);
        }
        walk::walk_static_member_expression(self, expr);
    }

    fn visit_computed_member_expression(&mut self, expr: &ComputedMemberExpression<'a>) {
        self.record_computed_env_read(expr);
        if let Some(member) = expr.static_property_name() {
            self.record_pinia_state_key_read(&expr.object, &member.text());
            self.record_subclass_member_access(&expr.object, &member.text());
            self.record_awaited_dynamic_import_member(&expr.object, &member.text());
        }
        if let Expression::StaticMemberExpression(key) = &expr.expression
            && let Expression::Identifier(key_object) = &key.object
        {
            self.pending_computed_enum_key_uses
                .push(super::PendingComputedEnumKeyUse {
                    fact: ComputedEnumKeyUseFact {
                        key_object: key_object.name.to_string(),
                        key_member: key.property.name.to_string(),
                    },
                    key_object_span: key_object.span,
                });
        }
        if let Expression::Identifier(obj) = &expr.object
            && (self.route_loader_data_bindings.contains(obj.name.as_str())
                || obj.name == "loaderData")
            && let Expression::StringLiteral(lit) = &expr.expression
        {
            self.member_accesses.push(MemberAccess {
                object: ROUTE_LOADER_DATA_OBJECT.to_string(),
                member: lit.value.text().into_owned(),
            });
        }
        self.record_computed_key_access(expr);
        walk::walk_computed_member_expression(self, expr);
    }

    fn visit_ts_interface_heritage(&mut self, it: &TSInterfaceHeritage<'a>) {
        self.visit_heritage_type_name(&it.type_name);
        if let Some(type_arguments) = &it.type_arguments {
            self.visit_ts_type_parameter_instantiation(type_arguments);
        }
    }

    fn visit_ts_qualified_name(&mut self, it: &TSQualifiedName<'a>) {
        if let TSTypeName::IdentifierReference(obj) = &it.left
            && !self.namespace_like_binding_is_shadowed(obj.name.as_str())
        {
            self.member_accesses.push(MemberAccess {
                object: obj.name.to_string(),
                member: it.right.name.to_string(),
            });
            self.mark_structured_namespace_reference(obj);
        }
        walk::walk_ts_qualified_name(self, it);
    }

    fn visit_ts_mapped_type(&mut self, it: &TSMappedType<'a>) {
        if let TSType::TSTypeReference(type_ref) = &it.constraint
            && let TSTypeName::IdentifierReference(ident) = &type_ref.type_name
        {
            self.push_whole_object_use(ident.name.to_string());
        }
        if let TSType::TSTypeOperatorType(op) = &it.constraint
            && op.operator == TSTypeOperatorOperator::Keyof
            && let TSType::TSTypeQuery(query) = &op.type_annotation
            && let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name
        {
            self.push_whole_object_use(ident.name.to_string());
        }
        walk::walk_ts_mapped_type(self, it);
    }

    fn visit_ts_type_reference(&mut self, it: &TSTypeReference<'a>) {
        if let TSTypeName::IdentifierReference(name) = &it.type_name
            && name.name == "Record"
            && let Some(type_args) = &it.type_arguments
            && let Some(first_arg) = type_args.params.first()
            && let TSType::TSTypeReference(key_ref) = first_arg
            && let TSTypeName::IdentifierReference(key_ident) = &key_ref.type_name
        {
            self.push_whole_object_use(key_ident.name.to_string());
        }
        walk::walk_ts_type_reference(self, it);
    }

    fn visit_for_in_statement(&mut self, stmt: &ForInStatement<'a>) {
        if let Expression::Identifier(ident) = &stmt.right {
            self.record_whole_object_identifier_use(ident.name.as_str());
        }
        self.visit_for_statement_left(&stmt.left);
        self.visit_expression(&stmt.right);

        let mut roots = FxHashSet::default();
        if let ForStatementLeft::VariableDeclaration(declaration) = &stmt.left
            && declaration.kind.is_lexical()
        {
            roots.extend(
                declaration
                    .declarations
                    .iter()
                    .flat_map(|declarator| declarator.id.get_binding_identifiers())
                    .map(|id| id.name.to_string()),
            );
        }
        self.push_direct_object_binding_scope(roots);
        let loop_scope = self.push_loop_head_scope(loop_head_declaration(&stmt.left));
        let values = self.push_conditional_binding_scope(&[]);
        if let Some(target) = stmt.left.as_assignment_target() {
            self.invalidate_assignment_target_proof(target);
        }
        self.visit_statement(&stmt.body);
        self.pop_optional_binding_scope(values);
        self.pop_loop_head_scope(loop_scope);
        self.pop_direct_object_binding_scope();
    }

    fn visit_while_statement(&mut self, stmt: &WhileStatement<'a>) {
        self.visit_expression(&stmt.test);
        let values = self.push_conditional_binding_scope(&[]);
        self.visit_statement(&stmt.body);
        self.pop_optional_binding_scope(values);
    }

    fn visit_logical_expression(&mut self, expr: &LogicalExpression<'a>) {
        self.visit_expression(&expr.left);
        let values = self.push_conditional_binding_scope(&[]);
        self.visit_expression(&expr.right);
        self.pop_optional_binding_scope(values);
    }

    fn visit_conditional_expression(&mut self, expr: &ConditionalExpression<'a>) {
        self.visit_expression(&expr.test);
        let values = self.push_conditional_binding_scope(&[]);
        self.visit_expression(&expr.consequent);
        let consequent = self.pop_conditional_binding_scope(values);
        let values = self.push_conditional_binding_scope(&[]);
        self.visit_expression(&expr.alternate);
        let alternate = self.pop_conditional_binding_scope(values);
        self.join_conditional_binding_scopes(&consequent, &alternate);
    }

    fn visit_if_statement(&mut self, stmt: &IfStatement<'a>) {
        self.capture_origin_guard_observation(stmt);
        // Record `x instanceof ClassName` narrowings from the test condition so
        // that method calls on `x` inside the body (e.g. `x.getMessage()`) are
        // credited as uses of `ClassName.getMessage`, preventing false
        // unused-class-member findings. The bindings are module-scoped (not
        // strictly block-scoped), which is conservative: it may credit accesses
        // outside the guard, but that produces at most false negatives, not false
        // positives.
        let mut narrowings = Vec::new();
        collect_instanceof_narrowings(&stmt.test, &mut narrowings);
        for (local, class_name, _, _) in &narrowings {
            self.insert_class_binding_target_if_absent(local.clone(), class_name.clone());
        }
        self.visit_expression(&stmt.test);
        let values = self.push_conditional_binding_scope(&narrowings);
        self.visit_statement(&stmt.consequent);
        let consequent = self.pop_conditional_binding_scope(values);
        let values = self.push_conditional_binding_scope(&[]);
        if let Some(alternate) = &stmt.alternate {
            self.visit_statement(alternate);
        }
        let alternate = self.pop_conditional_binding_scope(values);
        self.join_conditional_binding_scopes(&consequent, &alternate);
    }

    fn visit_spread_element(&mut self, elem: &SpreadElement<'a>) {
        match &elem.argument {
            Expression::Identifier(ident) => {
                self.record_whole_object_identifier_use(ident.name.as_str());
            }
            // `{ ...this }` forwards every member opaquely (the Angular "headless
            // pattern" convention spreads `this` into a behavior pattern). Record
            // a typed fact so the Angular input/output detectors abstain the
            // whole component instead of false-flagging spread inputs.
            Expression::ThisExpression(_) => {
                self.semantic_facts
                    .push(SemanticFact::AngularThisSpread(AngularThisSpreadFact));
            }
            _ => {}
        }
        walk::walk_spread_element(self, elem);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        let member_access_start = self.member_accesses.len();
        let whole_object_start = self.whole_object_uses.len();
        let type_scope_pushed =
            self.push_class_type_scope(class.id.as_ref(), class.type_parameters.as_deref());
        self.record_lit_custom_element(class);
        self.record_worker_env_bodies(class);

        if let Some(meta) = extract_angular_component_metadata(class).filter(|_| !self.sfc_script) {
            self.record_angular_selector(class, &meta);
            self.record_angular_component_field_array_types(class);
            self.record_angular_template_assets(&meta);
            self.record_angular_inline_template(class, &meta);
            self.record_angular_template_members(&meta);
        }
        self.class_super_stack
            .push(super::helpers::extract_super_class_name(class).map(|name| {
                (
                    name,
                    class
                        .heritage
                        .as_ref()
                        .and_then(|base| iteration_source_reference(&base.expression)),
                )
            }));
        self.class_type_param_constraints.push(
            super::helpers::collect_class_type_param_constraint_origins(class),
        );
        // Assign this class body a unique scope id so its `this.<field>` binding
        // keys are qualified per-class (issue #1821, Fix B). A nested class
        // pushes its own id, so `this` inside the inner body binds to the inner
        // class.
        self.class_scope_counter += 1;
        let class_scope_id = self.class_scope_counter;
        self.class_scope_stack.push(class_scope_id);
        // Iteration consumers and `this` method results can precede their
        // fields, constructor or method in source order.
        self.preseed_class_bindings(class);
        let direct_scope_roots = class
            .id
            .as_ref()
            .map(|id| FxHashSet::from_iter([id.name.to_string()]))
            .unwrap_or_default();
        self.push_direct_object_binding_scope(direct_scope_roots);
        // `this` in the class body is the instance of this class, also when
        // the class is declared in a function with its own `this`.
        let outer_independent_this_depth = std::mem::replace(&mut self.independent_this_depth, 0);
        walk::walk_class(self, class);
        self.independent_this_depth = outer_independent_this_depth;
        self.pop_direct_object_binding_scope();
        self.record_class_this_facts(
            class,
            class_scope_id,
            member_access_start,
            whole_object_start,
        );
        self.class_scope_stack.pop();
        self.class_type_param_constraints.pop();
        self.class_super_stack.pop();
        if type_scope_pushed {
            self.pop_function_type_alias_scope();
        }
    }

    /// Track asset references inside `` html`...` `` tagged template literals
    /// as `SideEffect` imports.
    ///
    /// SSR helpers like `hono/html`, `lit-html`, and `htm` emit HTML via a
    /// tagged template whose tag is the identifier `html`. The static markup
    /// lives in the template quasis, and `${...}` interpolations are used for
    /// dynamic content only. When a layout component writes
    /// `` html`<script src="/static/app.js"></script>` ``, the `/static/app.js`
    /// file must stay reachable from that module, exactly like the HTML parser
    /// handles the same markup in `.html` files. See issue #105 (till's
    /// follow-up comment).
    ///
    /// Only the `Expression::Identifier` tag named `html` is matched. Member
    /// expressions (`lit.html`), call expressions, and other identifiers are
    /// deliberately skipped to avoid conflating unrelated tagged templates
    /// (`css`, `sql`, `gql`, `styled.div`) with HTML. Each quasi is scanned
    /// independently so an asset reference spanning an interpolation boundary
    /// is ignored rather than producing a garbled, unresolvable specifier.
    fn visit_tagged_template_expression(&mut self, expr: &TaggedTemplateExpression<'a>) {
        self.try_record_command_template(expr);
        if visit_html_template::is_html_tagged_template(&expr.tag) {
            for quasi in &expr.quasi.quasis {
                let text = quasi
                    .value
                    .cooked
                    .map_or_else(|| quasi.value.raw.as_str().into(), JsText::text);
                self.push_html_template_asset_imports(quasi, &text);
                // Record custom-element tags rendered in the template
                // (`<x-foo>`), feeding the Lit `unrendered-component` arm's
                // project-wide rendered-tag union.
                for tag in crate::html::collect_custom_element_tags(&text) {
                    self.used_custom_element_tags.insert(tag);
                }
            }
            // Detect a dynamic tag interpolation (`` html`<${tag}>` ``): a quasi
            // that ends at a tag-open boundary (`<` / `</`) right before an
            // expression renders an unknowable element, so mark the project
            // dynamic (the Lit arm then abstains on every element).
            //
            // The bare `<` / `</` test is intentionally broad: in Lit's own
            // template grammar a quasi ending in `<` immediately before an
            // interpolation is a tag-open position, so a false trigger only
            // over-abstains (suppresses Lit findings) while a missed trigger
            // would risk a false `unrendered-component`. Over-abstain is the
            // zero-FP-safe direction, so do not narrow this to require a
            // preceding space.
            let quasis = &expr.quasi.quasis;
            for (i, quasi) in quasis.iter().enumerate() {
                if i + 1 >= quasis.len() {
                    break;
                }
                let text = quasi
                    .value
                    .cooked
                    .map_or_else(|| quasi.value.raw.as_str().into(), JsText::text);
                if text.ends_with('<') || text.ends_with("</") {
                    self.semantic_facts
                        .push(SemanticFact::DynamicCustomElementRender(
                            DynamicCustomElementRenderFact,
                        ));
                }
            }
        }
        self.capture_tagged_template_sink(expr);
        let prev_tagged = self.in_tagged_template_quasi;
        self.in_tagged_template_quasi = true;
        walk::walk_tagged_template_expression(self, expr);
        self.in_tagged_template_quasi = prev_tagged;
    }

    fn visit_jsx_attribute(&mut self, attr: &oxc_ast::ast::JSXAttribute<'a>) {
        self.capture_jsx_attr_sink(attr);
        walk::walk_jsx_attribute(self, attr);
    }

    fn visit_jsx_spread_attribute(&mut self, spread: &oxc_ast::ast::JSXSpreadAttribute<'a>) {
        self.capture_jsx_spread_sink(spread);
        walk::walk_jsx_spread_attribute(self, spread);
    }

    fn visit_jsx_fragment(&mut self, fragment: &oxc_ast::ast::JSXFragment<'a>) {
        self.has_jsx = true;
        walk::walk_jsx_fragment(self, fragment);
    }

    fn visit_jsx_element(&mut self, element: &oxc_ast::ast::JSXElement<'a>) {
        self.has_jsx = true;
        // Record a render edge for a component tag (capitalized or
        // member-expression) plus the passed attribute names + spread presence.
        // Lowercase host elements are skipped for render purposes. No-op on
        // non-JSX files (perf gate). The walk continues into children so nested
        // renders and host-element nesting depth are still visited.
        self.react_record_jsx_element(element);
        // A member-expression tag (`<SC.UsedStyle />`) is the JSX spelling of
        // `SC.UsedStyle`, so it must feed the same member-access stream that
        // namespace-import narrowing consumes (issue #2348). Opening element
        // only, so a paired closing tag does not double-record.
        if let JSXElementName::MemberExpression(member) = &element.opening_element.name {
            self.record_jsx_member_tag_accesses(member);
        }
        // Both tags name the same member, so neither root is a bare pass of the
        // namespace object (issue #2377). The closing tag is excluded on its own
        // because `record_jsx_member_tag_accesses` deliberately skips it.
        self.mark_jsx_member_tag_root(Some(&element.opening_element.name));
        self.mark_jsx_member_tag_root(
            element
                .closing_element
                .as_ref()
                .map(|closing| &closing.name),
        );
        walk::walk_jsx_element(self, element);
    }

    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        self.complete_factory_result_observation(ident);
        self.record_bare_namespace_reference(ident);
        self.count_local_import_loader_reference(ident.name.as_str());
        walk::walk_identifier_reference(self, ident);
    }
}
