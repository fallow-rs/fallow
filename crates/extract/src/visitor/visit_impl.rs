//! `Visit` trait implementation for `ModuleInfoExtractor`.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_semantic::ScopeFlags;
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use crate::{
    DynamicImportInfo, DynamicImportPattern, ExportInfo, ExportName, ImportInfo, ImportedName,
    MemberAccess, ReExportInfo, RequireCallInfo, VisibilityTag,
};
use fallow_types::extract::{
    AngularComponentSelector, CalleeUse, ClassHeritageInfo, DiFramework, DiKeySite, DiRole,
    LocalTypeDeclaration, MisplacedDirectiveSite, PublicSignatureTypeReference,
};

use crate::asset_url::normalize_asset_url;
use crate::html::is_remote_url;

use super::helpers::{
    array_element_type_from_type, extract_angular_component_metadata,
    extract_angular_inputs_outputs, extract_angular_signal_query, extract_class_members,
    extract_concat_parts, extract_custom_element_tag_reference, extract_custom_elements_define,
    extract_implemented_interface_names, extract_nested_type_bindings,
    extract_query_list_element_type, extract_super_class_name, extract_type_annotation_name,
    has_angular_class_decorator, has_angular_plural_query_decorator,
    infer_array_binding_element_type, is_meta_url_arg, lit_custom_element_decorator,
    lit_custom_element_tag, regex_pattern_to_suffix, return_type_element_name,
    ts_import_type_qualifier_root,
};
use super::{
    BindingTarget, ModuleInfoExtractor, PendingLocalExportSpecifier, ROUTE_LOADER_DATA_OBJECT,
    SideEffectRegistrationTarget, collect_static_import_specifiers, extract_import_expression,
    try_extract_arrow_wrapped_import, try_extract_import_then_callback,
    try_extract_property_callback_import, try_extract_require,
};

#[path = "visit_impl_di.rs"]
mod visit_di;
#[path = "visit_impl_dynamic_imports.rs"]
mod visit_dynamic_imports;
#[path = "visit_impl_factory_returns.rs"]
mod visit_factory_returns;
#[path = "visit_impl_helpers.rs"]
mod visit_helpers;
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
#[path = "visit_impl_signature.rs"]
mod visit_signature;
#[path = "visit_impl_structural.rs"]
mod visit_structural;
#[path = "visit_impl_svelte_events.rs"]
mod visit_svelte_events;
#[path = "visit_impl_sveltekit_load.rs"]
mod visit_sveltekit_load;
#[path = "visit_impl_taint_sources.rs"]
mod visit_taint_sources;

pub(super) use visit_factory_returns::count_returns_in_statements;
use visit_factory_returns::{
    FactoryReturnFunctionInput, classify_factory_assigned_value, collect_self_scope_assignments,
    function_body_is_terminal, function_body_returns_identifier, function_body_returns_new_class,
    function_body_returns_new_class_unanimous,
};
use visit_helpers::*;
use visit_object_helpers::*;
use visit_package_resolution::*;
use visit_security_classifiers::*;
pub(super) use visit_security_routes::function_body_has_use_server;
use visit_security_routes::*;

/// Array iteration methods whose callback's FIRST parameter is an element of the
/// receiver array (so it can be typed to the receiver's element class). `reduce`
/// and `reduceRight` are intentionally excluded: their first callback parameter
/// is the accumulator, not an element. `sort` is excluded to keep the set to
/// single-element-per-call iterators. See issue #1707 follow-up.
const ITERABLE_ELEMENT_CALLBACK_METHODS: &[&str] = &[
    "map",
    "forEach",
    "filter",
    "find",
    "findLast",
    "findIndex",
    "findLastIndex",
    "flatMap",
    "some",
    "every",
];

const ROUTE_LOADER_DATA_SOURCES: &[&str] =
    &["react-router", "react-router-dom", "@remix-run/react"];

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

impl ModuleInfoExtractor {
    /// Record a same-file function whose body returns `new Class()` so a later
    /// `const x = <name>()` binding can resolve to that class. Module scope only;
    /// imported / re-exported factory wrappers are out of scope (issue #1441).
    fn record_factory_return_function(
        &mut self,
        name: &str,
        input: FactoryReturnFunctionInput<'_, '_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let Some(body) = input.body else {
            return;
        };
        // A cross-module factory must hand back the class instance synchronously:
        // an `async` fn returns `Promise<T>` and a generator returns an iterator,
        // so `const x = make(); x.member` would be on the wrong type. Such
        // factories are excluded from the STRICT (cross-module) map; the same-file
        // (loose) maps below are unaffected. See #1441 (Part A).
        let strict_eligible = !input.is_async && !input.is_generator;
        if let Some(class_name) = function_body_returns_new_class(body, input.is_expression_body) {
            // An all-paths-unanimous, non-falling-through proof additionally
            // qualifies this factory for cross-module export (see
            // `strict_factory_return_functions`); the same-file map below keeps
            // the looser last-return semantics.
            if strict_eligible
                && let Some(unanimous_class) =
                    function_body_returns_new_class_unanimous(body, input.is_expression_body)
            {
                self.strict_factory_return_functions
                    .insert(name.to_string(), unanimous_class);
            }
            self.factory_return_functions
                .insert(name.to_string(), class_name);
        } else if let Some(returned_id) =
            function_body_returns_identifier(body, input.params, input.is_expression_body)
        {
            // The alias is eligible for STRICT promotion only when it returns
            // synchronously and the body cannot fall through to `undefined`. The
            // class is value-proven later in `resolve_factory_return_aliases`.
            if strict_eligible && function_body_is_terminal(body, input.is_expression_body) {
                self.strict_alias_eligible.insert(name.to_string());
                // Collect assignments to the returned id from THIS function's own
                // body (not nested functions), tying the value-proof to the alias
                // function, an assignment in a sibling/unrelated function must not
                // prove it. See #1441 (Part A).
                let mut assignments = Vec::new();
                collect_self_scope_assignments(&body.statements, &returned_id, &mut assignments);
                if !assignments.is_empty() {
                    self.alias_in_body_assignments
                        .insert(name.to_string(), assignments);
                }
            }
            self.factory_return_alias_functions
                .insert(name.to_string(), returned_id);
        } else if strict_eligible
            && let Some(return_type) = input.return_type
            && let Some(class_name) = extract_type_annotation_name(return_type)
        {
            // #1744: no body value-proof (`return registry.get() as Ctrl`), but
            // the function's explicit return-TYPE annotation names a class. Trust
            // the declared contract as a TYPE claim: TypeScript enforces that every
            // return conforms to the annotation, so the returned value IS an
            // instance of that class. This deliberately widens the #1441
            // value-vs-type doctrine (which rejects a returned-IDENTIFIER's
            // variable annotation, `let api: RESTApi`, because an assignment can
            // contradict it) because a FUNCTION return-type annotation is the
            // author's own compiler-checked contract, not a contradictable local.
            // It stays over-credit-safe: the analyze layer credits only when the
            // name resolves to a real class-with-members export, so a wrong
            // annotation (an interface, a primitive, a different class) is a
            // harmless no-op, a false negative at worst, never a false positive.
            self.strict_factory_return_functions
                .insert(name.to_string(), class_name.clone());
            self.factory_return_functions
                .insert(name.to_string(), class_name);
        }
    }

    /// Capture `const local = callee(...)` (bare-identifier callee) as a factory
    /// return candidate. `resolve_factory_return_candidates` keeps only those
    /// whose callee is a known same-file `new Class()` factory or an imported
    /// callee (cross-module). See issue #1441.
    ///
    /// Not scope-gated, mirroring the `const n = new Class()` instance binding:
    /// the consumer is commonly inside a setup/composable function, and
    /// `binding_target_names` is module-flat by design.
    fn record_factory_return_candidate(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        let Some(callee_name) = Self::bare_call_callee_name(init) else {
            return;
        };

        match &declarator.id {
            BindingPattern::BindingIdentifier(id) => {
                self.factory_return_candidates
                    .push(super::FactoryReturnCandidate {
                        local_name: id.name.to_string(),
                        callee_name,
                    });
            }
            // `const { a, b } = useApi()`. The instance is never named, so queue one
            // direct factory-result access per statically named key. Dropping this
            // shape is what reported every member of a destructured factory result
            // as unused.
            BindingPattern::ObjectPattern(pattern) => {
                let Some(keys) = super::destructured_factory_keys(pattern) else {
                    // A rest element or computed key can read any property.
                    self.factory_whole_object_candidates.push(callee_name);
                    return;
                };
                for key in keys {
                    self.factory_unnamed_result_accesses
                        .push((callee_name.clone(), key));
                }
            }
            _ => {}
        }
    }

    /// The callee of a bare `identifier(...)` call. A member-expression callee
    /// (`obj.f()`) and a call-of-a-call (`f()()`) are deliberately not matched:
    /// neither resolves to a proven factory export, so crediting through them would
    /// be a guess.
    fn bare_call_callee_name(expression: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        Some(callee.name.to_string())
    }

    /// Record `const TOKEN = new InjectionToken<Interface>(...)` declarations
    /// so the analyze layer can follow the token's interface type argument to
    /// the classes that `implement` it. Gated on `InjectionToken` being a named
    /// import from `@angular/core` (a same-named local class is ignored). A
    /// token with no type argument carries no interface for the template-chain
    /// bridge, but a tree-shakable token (a `{ factory }` / `{ providedIn }`
    /// second argument) still records a self-provide DI site so the
    /// `unprovided-inject` detector treats it as provided. See issues #920 and
    /// the Angular `unprovided-inject` arm.
    fn record_injection_token(&mut self, name: &str, init: &Expression<'_>) {
        if !self.is_module_scope() {
            return;
        }
        let Expression::NewExpression(new_expr) = init else {
            return;
        };
        let Expression::Identifier(callee) = &new_expr.callee else {
            return;
        };
        if !self.is_named_import_from(callee.name.as_str(), "@angular/core", "InjectionToken") {
            return;
        }

        // A tree-shakable token (`new InjectionToken('x', { factory: () => ... })`
        // or `{ providedIn: 'root' }`) provides itself, so record it as a Provide
        // of its own const name. This makes a self-providing token count as
        // provided even without a `{ provide: TOKEN }` object anywhere.
        if let Some(Argument::ObjectExpression(options)) = new_expr.arguments.get(1)
            && object_has_any_key(options, &["factory", "providedIn"])
        {
            self.di_key_sites.push(DiKeySite {
                key_local: name.to_string(),
                role: DiRole::Provide,
                framework: DiFramework::Angular,
                span_start: new_expr.span.start,
            });
        }

        // Record EVERY `new InjectionToken(...)` declaration so the
        // `unprovided-inject` FP gate recognizes it as a token regardless of its
        // type argument. The interface name (consumed by the #920 template-member
        // bridge in `unused_members.rs`) comes from a type-REFERENCE type argument
        // (`new InjectionToken<Greeter>(...)`); a primitive type argument
        // (`<string>`) or no type argument yields an empty interface, which the
        // bridge harmlessly skips (it credits implementers of a named interface,
        // and no interface is named "").
        let interface_name = new_expr
            .type_arguments
            .as_deref()
            .and_then(|type_arguments| type_arguments.params.first())
            .and_then(|param| match param {
                TSType::TSTypeReference(type_ref) => {
                    type_name_root(&type_ref.type_name).map(|(interface_name, _)| interface_name)
                }
                _ => None,
            })
            .unwrap_or_default();
        self.injection_tokens
            .push((name.to_string(), interface_name));
    }

    fn clear_literal_allowlist_on_mutating_member_call(&mut self, call: &CallExpression<'_>) {
        if let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(object) = &member.object
            && !matches!(member.property.name.as_str(), "has" | "includes")
            && self.literal_allowlist_binding(&object.name)
        {
            self.record_literal_allowlist_binding(object.name.as_str(), false);
        }
    }

    fn svelte_derived_new_class(init: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = init else {
            return None;
        };
        if !Self::is_svelte_derived_call(call) {
            return None;
        }

        if let Some(expr) = call.arguments.first().and_then(Argument::as_expression)
            && let Expression::NewExpression(new_expr) = expr
            && let Expression::Identifier(callee) = &new_expr.callee
            && !super::helpers::is_builtin_constructor(callee.name.as_str())
        {
            return Some(callee.name.to_string());
        }

        super::helpers::try_extract_factory_new_class(&call.arguments)
    }

    fn is_svelte_derived_call(call: &CallExpression<'_>) -> bool {
        match &call.callee {
            Expression::Identifier(id) => id.name == "$derived",
            Expression::StaticMemberExpression(member) => {
                member.property.name == "by"
                    && matches!(&member.object, Expression::Identifier(id) if id.name == "$derived")
            }
            _ => false,
        }
    }

    /// Substitute a class type-parameter with its constraint when available.
    fn resolve_class_type_param(&self, type_name: &str) -> Option<String> {
        let Some(frame) = self.class_type_param_constraints.last() else {
            return Some(type_name.to_string());
        };
        match frame.get(type_name) {
            Some(Some(constraint)) => Some(constraint.clone()),
            Some(None) => None,
            None => Some(type_name.to_string()),
        }
    }

    /// Emit typed fluent-chain facts for chained calls.
    fn try_record_fluent_chain_access(&mut self, expr: &CallExpression<'_>) {
        let Expression::StaticMemberExpression(member) = &expr.callee else {
            return;
        };
        let Expression::CallExpression(_) = &member.object else {
            return;
        };
        let this_method = member.property.name.as_str();
        let mut chain_prefix_reversed: Vec<String> = Vec::new();
        let mut current = &member.object;
        loop {
            let Expression::CallExpression(call) = current else {
                return;
            };
            let Expression::StaticMemberExpression(inner_member) = &call.callee else {
                return;
            };
            if let Expression::Identifier(root_id) = &inner_member.object {
                chain_prefix_reversed.reverse();
                self.record_fluent_chain_member_fact(
                    root_id.name.to_string(),
                    inner_member.property.name.to_string(),
                    chain_prefix_reversed,
                    this_method.to_string(),
                );
                return;
            }
            if let Expression::NewExpression(new_expr) = &inner_member.object
                && let Expression::Identifier(class_id) = &new_expr.callee
            {
                chain_prefix_reversed.push(inner_member.property.name.to_string());
                chain_prefix_reversed.reverse();
                self.record_fluent_chain_new_member_fact(
                    class_id.name.to_string(),
                    chain_prefix_reversed,
                    this_method.to_string(),
                );
                return;
            }
            chain_prefix_reversed.push(inner_member.property.name.to_string());
            current = &inner_member.object;
        }
    }

    /// Recognize `.forEach(...)` on iterables and bind the callback element.
    fn bind_iterable_callback_parameter(&mut self, expr: &CallExpression<'_>) {
        let (receiver_expr, method_name) = match &expr.callee {
            Expression::StaticMemberExpression(member) => (&member.object, &member.property.name),
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::StaticMemberExpression(member) => {
                    (&member.object, &member.property.name)
                }
                _ => return,
            },
            _ => return,
        };
        // Array iteration methods whose callback's FIRST parameter is an element.
        // `reduce` / `reduceRight` are excluded because their first callback
        // parameter is the accumulator, not an element. See issue #1707 follow-up.
        if !ITERABLE_ELEMENT_CALLBACK_METHODS.contains(&method_name.as_str()) {
            return;
        }
        let Some(receiver_name) = static_member_object_name(receiver_expr) else {
            return;
        };
        let Some(element_type) = self.iterable_element_type_for(&receiver_name) else {
            return;
        };
        let Some(first_arg) = expr.arguments.first() else {
            return;
        };
        let param_name = match first_arg {
            Argument::ArrowFunctionExpression(arrow) => {
                arrow.params.items.first().and_then(|p| match &p.pattern {
                    BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
                    _ => None,
                })
            }
            Argument::FunctionExpression(func) => {
                func.params.items.first().and_then(|p| match &p.pattern {
                    BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
                    _ => None,
                })
            }
            _ => None,
        };
        if let Some(name) = param_name {
            self.insert_class_binding_target(name, element_type);
        }
    }

    /// The element class of an iterable receiver, consulting the Angular
    /// query-list map first (`this.items()`) then the general array / reactive
    /// array binding map (`const utils: Util[]`). Reused by the `for...of` and
    /// array-callback iteration-binding paths (issue #1707 follow-up).
    fn iterable_element_type_for(&self, receiver_name: &str) -> Option<String> {
        self.iterable_element_types
            .get(receiver_name)
            .or_else(|| self.array_binding_element_types.get(receiver_name))
            .or_else(|| {
                self.scoped_array_binding_element_types
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(receiver_name))
            })
            .cloned()
    }

    fn record_array_binding_element_type(&mut self, binding: String, element: String) {
        if self.is_module_scope() {
            self.array_binding_element_types.insert(binding, element);
        } else if let Some(scope) = self.scoped_array_binding_element_types.last_mut() {
            scope.insert(binding, element);
        }
    }

    fn record_vue_ref_value_array_binding_type(
        &mut self,
        binding: &str,
        init: &Expression<'_>,
        element: &str,
    ) {
        let Expression::CallExpression(call) = init else {
            return;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if matches!(callee.name.as_str(), "ref" | "shallowRef" | "computed") {
            self.record_array_binding_element_type(format!("{binding}.value"), element.to_string());
        }
    }

    fn record_object_array_member_binding_types(
        &mut self,
        binding: &str,
        type_annotation: Option<&TSTypeAnnotation<'_>>,
    ) {
        let Some(type_annotation) = type_annotation else {
            return;
        };
        let TSType::TSTypeLiteral(literal) = &type_annotation.type_annotation else {
            return;
        };
        for member in &literal.members {
            let TSSignature::TSPropertySignature(prop) = member else {
                continue;
            };
            let Some(property_name) = prop.key.static_name() else {
                continue;
            };
            let Some(property_type) = prop.type_annotation.as_deref() else {
                continue;
            };
            let Some(element) = array_element_type_from_type(&property_type.type_annotation) else {
                continue;
            };
            self.record_array_binding_element_type(format!("{binding}.{property_name}"), element);
        }
    }

    /// Bind a `for (const util of utils)` loop variable to the element class of
    /// `utils` so member accesses on the loop variable (`util.getter`) credit the
    /// class. Bare-identifier loop bindings over an identifier receiver only;
    /// destructured bindings and non-identifier receivers are out of scope.
    fn bind_for_of_element(&mut self, stmt: &ForOfStatement<'_>) {
        let Some(receiver_name) = static_member_object_name(&stmt.right) else {
            return;
        };
        let Some(element_type) = self.iterable_element_type_for(&receiver_name) else {
            return;
        };
        let ForStatementLeft::VariableDeclaration(decl) = &stmt.left else {
            return;
        };
        if let Some(declarator) = decl.declarations.first()
            && let BindingPattern::BindingIdentifier(id) = &declarator.id
        {
            self.insert_class_binding_target(id.name.to_string(), element_type);
        }
    }

    pub(super) fn is_named_import_from(
        &self,
        local_name: &str,
        source: &str,
        imported_name: &str,
    ) -> bool {
        self.imports.iter().any(|import| {
            import.source == source
                && import.local_name == local_name
                && matches!(&import.imported_name, ImportedName::Named(name) if name == imported_name)
        })
    }

    fn record_source_returning_helper_from_variable_declarator(
        &mut self,
        decl: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        if decl.kind != VariableDeclarationKind::Const {
            self.source_returning_helpers.remove(id.name.as_str());
            return;
        }
        let helper = match init {
            Expression::ArrowFunctionExpression(arrow) => extract_arrow_return_expr(arrow)
                .and_then(|expr| source_returning_helper(&arrow.params, expr)),
            Expression::FunctionExpression(function) => function
                .body
                .as_deref()
                .and_then(extract_function_body_final_return_expr)
                .and_then(|expr| source_returning_helper(&function.params, expr)),
            _ => None,
        };
        if let Some(helper) = helper {
            self.source_returning_helpers
                .insert(id.name.to_string(), helper);
        } else {
            self.source_returning_helpers.remove(id.name.as_str());
        }
    }

    fn record_initialized_declarator_bindings(
        &mut self,
        decl: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            if decl.kind == VariableDeclarationKind::Const && self.is_module_scope() {
                self.record_static_package_values(id.name.as_str(), init);
            }

            self.record_static_sink_literal_binding(decl, declarator, init);
            let sources = self.node_module_register_sources_from_expression(init);
            if !sources.is_empty() {
                self.record_node_module_register_url_binding(id.name.to_string(), sources);
            }
            self.record_current_module_file_path_binding(id.name.as_str(), init);
            self.record_injection_token(id.name.as_str(), init);
            self.record_di_string_key_const(id.name.as_str(), decl, init);
            self.record_event_dispatch_binding(id.name.as_str(), init);
            self.record_child_process_fork_target_binding(id.name.as_str(), init);
            self.record_tainted_source_binding(id.name.as_str(), init);
            self.record_tainted_helper_call_binding(id.name.as_str(), init);
            // #1146 chain step AFTER the direct captures: a same-declarator
            // direct source read seeds hop 1 first, so the dedup min-merge in
            // `push_tainted_binding` keeps the direct depth.
            self.record_chained_tainted_binding(id.name.as_str(), init);
            let sanitizer_scope = self.sanitizer_scope_for_expr(init);
            self.record_sanitizer_binding(id.name.as_str(), sanitizer_scope);
            let allowlist = decl.kind == VariableDeclarationKind::Const
                && is_literal_string_allowlist_expr(init);
            self.record_literal_allowlist_binding(id.name.as_str(), allowlist);
            self.record_path_sink_binding(id.name.as_str(), self.path_sink_binding_for_expr(init));
            self.record_path_relative_binding(
                id.name.as_str(),
                self.path_relative_target_for_expr(init),
            );
        } else {
            for id in declarator.id.get_binding_identifiers() {
                self.record_sanitizer_binding(id.name.as_str(), None);
                self.record_literal_allowlist_binding(id.name.as_str(), false);
                self.record_path_sink_binding(id.name.as_str(), None);
                self.record_path_relative_binding(id.name.as_str(), None);
            }
        }
    }

    fn record_bare_require_call(&mut self, expr: &CallExpression<'_>) {
        if let Expression::Identifier(ident) = &expr.callee
            && ident.name == "require"
            && let Some(Argument::StringLiteral(lit)) = expr.arguments.first()
            && !self.handled_require_spans.contains(&expr.span)
        {
            self.require_calls.push(RequireCallInfo {
                source: lit.value.to_string(),
                span: expr.span,
                source_span: lit.span,
                destructured_names: Vec::new(),
                local_name: None,
            });
        }
    }

    fn record_whole_object_call_use(&mut self, expr: &CallExpression<'_>) {
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && let Expression::Identifier(obj) = &member.object
            && obj.name == "Object"
            && matches!(
                member.property.name.as_str(),
                "values" | "keys" | "entries" | "getOwnPropertyNames"
            )
            && let Some(arg_name) = expr.arguments.first().and_then(static_argument_object_name)
        {
            self.record_whole_object_identifier_use(arg_name.as_str());
        }
    }

    fn record_whole_object_identifier_use(&mut self, name: &str) {
        if name == "loaderData" || self.route_loader_data_bindings.contains(name) {
            self.whole_object_uses
                .push(ROUTE_LOADER_DATA_OBJECT.to_string());
        }
        self.whole_object_uses.push(name.to_string());
    }
}

impl ModuleInfoExtractor {
    fn record_top_level_declaration(&mut self, decl: &Declaration<'_>) {
        match decl {
            Declaration::VariableDeclaration(var) => {
                for declarator in &var.declarations {
                    for id in declarator.id.get_binding_identifiers() {
                        self.record_local_declaration_name(&id.name);
                    }
                }
            }
            Declaration::ClassDeclaration(class) => self.record_top_level_class_declaration(class),
            Declaration::FunctionDeclaration(function) => {
                self.record_top_level_function_declaration(function);
            }
            Declaration::TSTypeAliasDeclaration(alias) => {
                self.record_top_level_type_alias_declaration(alias);
            }
            Declaration::TSInterfaceDeclaration(iface) => {
                self.record_top_level_interface_declaration(iface);
            }
            Declaration::TSEnumDeclaration(enumd) => {
                self.record_local_declaration_name(&enumd.id.name);
                self.record_local_type_declaration(&enumd.id.name, enumd.id.span);
            }
            Declaration::TSModuleDeclaration(module) => {
                if let TSModuleDeclarationName::Identifier(id) = &module.id {
                    self.record_local_declaration_name(&id.name);
                    self.record_local_type_declaration(&id.name, id.span);
                }
            }
            _ => {}
        }
    }

    fn record_top_level_class_declaration(&mut self, class: &Class<'_>) {
        if let Some(id) = class.id.as_ref() {
            self.record_local_declaration_name(&id.name);
            self.record_sanitizer_binding(id.name.as_str(), None);
            self.record_literal_allowlist_binding(id.name.as_str(), false);
            self.record_risky_regex_binding(id.name.as_str(), None);
            self.record_path_sink_binding(id.name.as_str(), None);
            self.record_path_relative_binding(id.name.as_str(), None);
            self.record_local_type_declaration(&id.name, id.span);
            let is_angular = has_angular_class_decorator(class);
            self.record_angular_inputs_outputs(class, is_angular);
            let instance_bindings = super::helpers::extract_class_instance_bindings(
                class,
                |local_name, source, imported_name| {
                    self.is_named_import_from(local_name, source, imported_name)
                },
            );
            self.record_local_class_export(
                id.name.to_string(),
                extract_class_members(class, is_angular),
                extract_super_class_name(class),
                extract_implemented_interface_names(class),
                instance_bindings,
            );
            let refs = Self::collect_class_signature_refs(class);
            self.record_local_signature_refs(&id.name, refs);
        }
    }

    /// Harvest Angular `@Input()` / `@Output()` / signal `input()` / `output()` /
    /// `model()` members from an Angular-decorated class onto the module-level
    /// input/output accumulators. Gated on `is_angular` so a non-Angular class
    /// with a same-named `input` / `Input` helper never contributes. Called from
    /// every class-extraction site (named export, default export, local
    /// declaration) so the SFC and plain-`.ts` paths both populate it.
    pub(super) fn record_angular_inputs_outputs(&mut self, class: &Class<'_>, is_angular: bool) {
        if !is_angular {
            return;
        }
        // An `export class FooComponent` reaches this from both the named-export
        // declaration path and the top-level class-declaration path; dedup on the
        // class span so each declared input/output is harvested exactly once.
        if !self.harvested_angular_class_spans.insert(class.span) {
            return;
        }
        let (inputs, outputs) = extract_angular_inputs_outputs(class);
        self.angular_inputs.extend(inputs);
        self.angular_outputs.extend(outputs);
    }

    fn record_top_level_function_declaration(&mut self, function: &Function<'_>) {
        if let Some(id) = function.id.as_ref() {
            self.record_local_declaration_name(&id.name);
            self.record_sanitizer_binding(id.name.as_str(), None);
            self.record_literal_allowlist_binding(id.name.as_str(), false);
            self.record_risky_regex_binding(id.name.as_str(), None);
            self.record_path_sink_binding(id.name.as_str(), None);
            self.record_path_relative_binding(id.name.as_str(), None);
            let refs = Self::collect_function_signature_refs(function);
            self.record_local_signature_refs(&id.name, refs);
            self.record_local_structural_function(
                id.name.as_str(),
                &function.params,
                function.body.as_deref(),
            );
            self.record_factory_return_function(
                id.name.as_str(),
                FactoryReturnFunctionInput {
                    params: &function.params,
                    body: function.body.as_deref(),
                    is_expression_body: false,
                    is_async: function.r#async,
                    is_generator: function.generator,
                    return_type: function.return_type.as_deref(),
                },
            );
            self.record_source_returning_function_declaration(function);
            self.record_package_resolution_function_arg(function, id.name.as_str());
            self.record_playwright_factory_helper(function, id.name.as_str());
        }
    }

    fn record_package_resolution_function_arg(&mut self, function: &Function<'_>, name: &str) {
        if let Some(body) = function.body.as_deref()
            && let Some(arg_index) = package_resolution_arg_index(
                &function.params,
                body,
                &self.package_resolution_function_args,
            )
        {
            self.package_resolution_function_args
                .insert(name.to_string(), arg_index);
        }
    }

    fn record_playwright_factory_helper(&mut self, function: &Function<'_>, name: &str) {
        if let Some(body) = function.body.as_deref()
            && let Some(call) = extract_function_body_final_return_call(body)
        {
            self.try_capture_playwright_factory_helper(name, call);
        }
    }

    fn record_top_level_type_alias_declaration(&mut self, alias: &TSTypeAliasDeclaration<'_>) {
        self.record_local_declaration_name(&alias.id.name);
        self.record_local_type_declaration(&alias.id.name, alias.id.span);
        self.record_playwright_fixture_type_alias(alias);
        if let Some(factory) = Self::store_factory_from_return_type(&alias.type_annotation) {
            // `type CounterStore = ReturnType<typeof useCounterStore>`: remember the
            // store factory so a param typed `CounterStore` credits store members
            // (issue #1489 Case 2).
            self.type_alias_store_factory
                .insert(alias.id.name.to_string(), factory);
        }
        let refs = Self::collect_type_alias_signature_refs(alias);
        self.record_local_signature_refs(&alias.id.name, refs);
        if let TSType::TSTypeLiteral(type_lit) = &alias.type_annotation {
            let properties = collect_object_type_property_types(&type_lit.members);
            if !properties.is_empty() {
                self.interface_property_types
                    .insert(alias.id.name.to_string(), properties);
            }
            // React props harvest (Feature A): a `type X = { a; b }` whose
            // annotation is a plain object literal with NO type parameters can
            // back a `(props: X) => props.a` component. A generic alias
            // (`type X<T> = ...`) is left out (fallow cannot substitute T), so
            // such a typed param abstains.
            if alias.type_parameters.is_none() {
                self.record_react_object_type_props(&alias.id.name, &type_lit.members);
            }
        }
    }

    fn record_top_level_interface_declaration(&mut self, iface: &TSInterfaceDeclaration<'_>) {
        self.record_local_declaration_name(&iface.id.name);
        self.record_local_type_declaration(&iface.id.name, iface.id.span);
        self.record_playwright_fixture_interface(iface);
        let refs = Self::collect_interface_signature_refs(iface);
        self.record_local_signature_refs(&iface.id.name, refs);
        let properties = collect_object_type_property_types(&iface.body.body);
        if !properties.is_empty() {
            self.interface_property_types
                .insert(iface.id.name.to_string(), properties);
        }
        // React props harvest (Feature A): a plain `interface X { a; b }` with no
        // `extends` heritage and no type parameters can back a
        // `(props: X) => props.a` component. An `interface X extends Y` or a
        // generic `interface X<T>` is excluded (fallow cannot expand the parent
        // members / substitute T), so such a typed param abstains.
        if iface.extends.is_empty() && iface.type_parameters.is_none() {
            self.record_react_object_type_props(&iface.id.name, &iface.body.body);
        }
    }

    /// Record the `(prop_name, span_start)` members of a plain object-type
    /// declaration so a React component typed by it (`(props: X) => ...`) can
    /// harvest the names in finalize. Only static identifier / string keys with a
    /// property signature contribute; an index signature, method signature, call
    /// signature, computed key, or spread is NOT a named prop and is skipped (its
    /// presence does not abstain the others, mirroring the destructure harvest's
    /// per-property tolerance).
    fn record_react_object_type_props(&mut self, type_name: &str, members: &[TSSignature<'_>]) {
        let mut props: Vec<(String, u32)> = Vec::new();
        for member in members {
            let TSSignature::TSPropertySignature(prop) = member else {
                continue;
            };
            let Some(property_name) = prop.key.static_name() else {
                continue;
            };
            props.push((property_name.to_string(), prop.span.start));
        }
        if !props.is_empty() {
            self.react_object_type_props
                .insert(type_name.to_string(), props);
        }
    }

    fn record_nested_declaration(&mut self, decl: &Declaration<'_>) {
        match decl {
            Declaration::VariableDeclaration(var) => {
                for declarator in &var.declarations {
                    self.record_nested_declaration_names(declarator.id.get_binding_identifiers());
                }
            }
            Declaration::ClassDeclaration(class) => {
                if let Some(id) = class.id.as_ref() {
                    self.record_nested_named_declaration(id);
                }
            }
            Declaration::FunctionDeclaration(function) => {
                if let Some(id) = function.id.as_ref() {
                    self.record_nested_named_declaration(id);
                }
            }
            _ => {}
        }
    }

    fn record_nested_named_declaration(&mut self, id: &BindingIdentifier<'_>) {
        self.record_nested_declaration_names(std::iter::once(id));
        self.record_sanitizer_binding(id.name.as_str(), None);
        self.record_literal_allowlist_binding(id.name.as_str(), false);
        self.record_risky_regex_binding(id.name.as_str(), None);
        self.record_path_sink_binding(id.name.as_str(), None);
        self.record_path_relative_binding(id.name.as_str(), None);
    }

    fn record_variable_declarator_metadata(&mut self, declarator: &VariableDeclarator<'_>) {
        if self.is_module_scope() {
            let refs = Self::collect_variable_signature_refs(declarator);
            for id in declarator.id.get_binding_identifiers() {
                self.record_local_signature_refs(&id.name, refs.clone());
            }
        }

        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Some(type_annotation) = declarator.type_annotation.as_deref()
        {
            self.record_typed_binding(id.name.as_str(), type_annotation);
        }

        if let BindingPattern::ObjectPattern(obj_pat) = &declarator.id
            && let Some(type_annotation) = declarator.type_annotation.as_deref()
        {
            self.record_typed_destructure_binding(obj_pat, type_annotation);
        }
    }

    fn record_uninitialized_variable_bindings(&mut self, declarator: &VariableDeclarator<'_>) {
        for id in declarator.id.get_binding_identifiers() {
            self.record_sanitizer_binding(id.name.as_str(), None);
            self.record_literal_allowlist_binding(id.name.as_str(), false);
            self.record_risky_regex_binding(id.name.as_str(), None);
            self.record_path_sink_binding(id.name.as_str(), None);
            self.record_path_relative_binding(id.name.as_str(), None);
        }
    }

    fn record_initialized_variable_bindings(
        &mut self,
        decl: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        self.record_local_structural_function_from_variable_declarator(declarator, init);
        self.record_factory_return_candidate(declarator, init);
        self.record_source_returning_helper_from_variable_declarator(decl, declarator, init);
        self.record_sanitizer_helper_from_variable_declarator(decl, declarator, init);
        self.record_initialized_declarator_bindings(decl, declarator, init);
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            self.capture_math_random_context_sink(id.name.as_str(), init, declarator.span);
            self.capture_hardcoded_secret_literal_sink(id.name.as_str(), init, declarator.span);
            let risky_pattern = if decl.kind == VariableDeclarationKind::Const {
                self.risky_regex_fragment_for_expr(init)
            } else {
                None
            };
            self.record_risky_regex_binding(id.name.as_str(), risky_pattern);
        }

        if let BindingPattern::ObjectPattern(obj_pat) = &declarator.id {
            self.record_tainted_destructure_bindings(obj_pat, init);
            self.record_chained_tainted_destructure_bindings(obj_pat, init);
        }
    }

    fn record_playwright_variable_helpers(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Expression::CallExpression(call) = init
        {
            self.record_playwright_fixture_definitions(id.name.as_str(), call);
            self.record_playwright_wrapper_aliases(id.name.as_str(), call);
        }

        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            let helper_call = match init {
                Expression::ArrowFunctionExpression(arrow) => extract_arrow_return_call(arrow),
                Expression::FunctionExpression(func) => func
                    .body
                    .as_deref()
                    .and_then(extract_function_body_final_return_call),
                _ => None,
            };
            if let Some(call) = helper_call {
                self.try_capture_playwright_factory_helper(id.name.as_str(), call);
            }
        }
    }
}

impl<'a> ModuleInfoExtractor {
    /// Record a misplaced `"use client"` / `"use server"` directive: oxc places
    /// honored leading-prologue directives in `program.directives`, so any
    /// string-literal expression statement reaching `program.body` is by
    /// definition NOT in the leading position (some non-directive statement
    /// preceded it), so the RSC bundler parses it as an ordinary expression and
    /// silently ignores it. Only the two RSC directive strings match; a stray
    /// `"use strict"` is harmless.
    fn record_misplaced_directive_statement(&mut self, stmt: &ExpressionStatement<'a>) {
        if let Expression::StringLiteral(lit) = &stmt.expression {
            let is_server = match lit.value.as_str() {
                "use server" => Some(true),
                "use client" => Some(false),
                _ => None,
            };
            if let Some(is_server) = is_server {
                self.misplaced_directives.push(MisplacedDirectiveSite {
                    is_server,
                    span_start: stmt.span.start,
                });
            }
        }
    }

    /// First top-level pass: record source-returning + sanitizer function
    /// declarations (bare and `export`-prefixed) and misplaced RSC directives.
    fn record_program_prologue(&mut self, program: &Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    self.record_source_returning_function_declaration(function);
                    self.record_sanitizer_function_declaration(function);
                }
                Statement::ExportNamedDeclaration(export)
                    if export.source.is_none()
                        && matches!(
                            export.declaration,
                            Some(Declaration::FunctionDeclaration(_))
                        ) =>
                {
                    if let Some(Declaration::FunctionDeclaration(function)) = &export.declaration {
                        self.record_source_returning_function_declaration(function);
                        self.record_sanitizer_function_declaration(function);
                    }
                }
                Statement::ExpressionStatement(stmt) => {
                    self.record_misplaced_directive_statement(stmt);
                }
                _ => {}
            }
        }
    }

    /// Second top-level pass: re-record sanitizer function declarations (bare and
    /// `export`-prefixed) after the prologue pass has populated module state.
    fn record_program_sanitizer_functions(&mut self, program: &Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    self.record_sanitizer_function_declaration(function);
                }
                Statement::ExportNamedDeclaration(export)
                    if export.source.is_none()
                        && matches!(
                            export.declaration,
                            Some(Declaration::FunctionDeclaration(_))
                        ) =>
                {
                    if let Some(Declaration::FunctionDeclaration(function)) = &export.declaration {
                        self.record_sanitizer_function_declaration(function);
                    }
                }
                _ => {}
            }
        }
    }

    /// Pre-pass over top-level declarations recording each local function's
    /// declared return element class (`Promise<T>` / `T`, non-builtin) keyed by
    /// name, so `promise_all_map_element_type` resolves a map callback's callee
    /// even when it is declared AFTER its consumer (declaration-order
    /// independence: the repro's `createDb` follows `generate`). See issue #1793.
    fn record_local_function_return_types(&mut self, program: &Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    self.record_local_function_return_type(function);
                }
                Statement::VariableDeclaration(decl) => {
                    for declarator in &decl.declarations {
                        self.record_local_const_function_return_type(declarator);
                    }
                }
                Statement::ExportNamedDeclaration(export) if export.source.is_none() => {
                    match &export.declaration {
                        Some(Declaration::FunctionDeclaration(function)) => {
                            self.record_local_function_return_type(function);
                        }
                        Some(Declaration::VariableDeclaration(decl)) => {
                            for declarator in &decl.declarations {
                                self.record_local_const_function_return_type(declarator);
                            }
                        }
                        _ => {}
                    }
                }
                Statement::ExportDefaultDeclaration(export) => {
                    if let ExportDefaultDeclarationKind::FunctionDeclaration(function) =
                        &export.declaration
                    {
                        self.record_local_function_return_type(function);
                    }
                }
                _ => {}
            }
        }
    }

    fn record_local_function_return_type(&mut self, function: &Function<'_>) {
        let Some(id) = function.id.as_ref() else {
            return;
        };
        let Some(return_type) = function.return_type.as_deref() else {
            return;
        };
        if let Some(element) = return_type_element_name(&return_type.type_annotation) {
            self.local_function_return_types
                .entry(id.name.to_string())
                .or_insert(element);
        }
    }

    fn record_local_const_function_return_type(&mut self, declarator: &VariableDeclarator<'_>) {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        let return_type = match declarator.init.as_ref() {
            Some(Expression::ArrowFunctionExpression(arrow)) => arrow.return_type.as_deref(),
            Some(Expression::FunctionExpression(function)) => function.return_type.as_deref(),
            _ => None,
        };
        let Some(return_type) = return_type else {
            return;
        };
        if let Some(element) = return_type_element_name(&return_type.type_annotation) {
            self.local_function_return_types
                .entry(id.name.to_string())
                .or_insert(element);
        }
    }

    /// Infer the element class of `await Promise.all(<expr>.map(cb))` where `cb`
    /// returns a call to a same-module function whose declared return type is
    /// `Promise<T>` / `T` for a non-builtin class `T`. `Promise.allSettled` /
    /// `any` / `race` are excluded (only `all` yields the resolved element
    /// array). Requires the `local_function_return_types` pre-pass. See #1793.
    fn promise_all_map_element_type(&self, init: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = unwrap_await_paren_expr(init) else {
            return None;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Promise" || member.property.name != "all" || call.arguments.len() != 1 {
            return None;
        }
        let map_expr = call.arguments.first()?.as_expression()?;
        let Expression::CallExpression(map_call) = unwrap_await_paren_expr(map_expr) else {
            return None;
        };
        let Expression::StaticMemberExpression(map_member) = &map_call.callee else {
            return None;
        };
        if map_member.property.name != "map" {
            return None;
        }
        let callee_name = map_callback_returned_call_name(map_call.arguments.first()?)?;
        self.local_function_return_types.get(&callee_name).cloned()
    }

    /// Record a named import specifier (`import { fork } from ...`), tracking the
    /// `child_process.fork` and `node:url` `fileURLToPath` provenance bindings.
    fn handle_import_specifier(
        &mut self,
        s: &ImportSpecifier<'a>,
        source: &str,
        is_type_only: bool,
        source_span: Span,
    ) {
        if self.is_module_scope() && is_child_process_source(source) && s.imported.name() == "fork"
        {
            self.child_process_fork_bindings
                .insert(s.local.name.to_string());
        }
        if self.is_module_scope()
            && is_node_url_source(source)
            && s.imported.name() == "fileURLToPath"
        {
            self.node_url_file_url_to_path_bindings
                .insert(s.local.name.to_string());
        }
        self.imports.push(ImportInfo {
            source: source.to_string(),
            imported_name: ImportedName::Named(s.imported.name().to_string()),
            local_name: s.local.name.to_string(),
            is_type_only: is_type_only || s.import_kind.is_type(),
            from_style: false,
            span: s.span,
            source_span,
        });
    }

    /// Record a default import specifier (`import x from ...`), tracking the
    /// DOMPurify and `node:path` provenance bindings.
    fn handle_import_default_specifier(
        &mut self,
        s: &ImportDefaultSpecifier<'a>,
        source: &str,
        is_type_only: bool,
        source_span: Span,
    ) {
        let local = s.local.name.to_string();
        self.record_dompurify_import_binding(source, &local, is_type_only);
        if self.is_module_scope() && is_node_path_source(source) {
            self.node_path_namespace_bindings.insert(local.clone());
        }
        if is_css_module_import_source(source) {
            self.namespace_binding_names.push(local.clone());
        }
        self.imports.push(ImportInfo {
            source: source.to_string(),
            imported_name: ImportedName::Default,
            local_name: local,
            is_type_only,
            from_style: false,
            span: s.span,
            source_span,
        });
    }

    /// Record a namespace import specifier (`import * as ns from ...`), tracking
    /// the DOMPurify, `child_process`, `node:path`, and `node:url` provenance
    /// bindings plus the namespace binding name.
    fn handle_import_namespace_specifier(
        &mut self,
        s: &ImportNamespaceSpecifier<'a>,
        source: &str,
        is_type_only: bool,
        source_span: Span,
    ) {
        let local = s.local.name.to_string();
        self.record_dompurify_import_binding(source, &local, is_type_only);
        if self.is_module_scope() && is_child_process_source(source) {
            self.child_process_namespace_bindings.insert(local.clone());
        }
        if self.is_module_scope() && is_node_path_source(source) {
            self.node_path_namespace_bindings.insert(local.clone());
        }
        if self.is_module_scope() && is_node_url_source(source) {
            self.node_url_file_url_to_path_bindings
                .insert(local.clone());
        }
        self.namespace_binding_names.push(local.clone());
        self.imports.push(ImportInfo {
            source: source.to_string(),
            imported_name: ImportedName::Namespace,
            local_name: local,
            is_type_only,
            from_style: false,
            span: s.span,
            source_span,
        });
    }

    /// Record `export { x } from './src'` re-export specifiers, abstaining the
    /// SvelteKit load-data harvest on a re-exported `load`.
    fn record_export_re_exports(
        &mut self,
        decl: &ExportNamedDeclaration<'a>,
        source: &oxc_ast::ast::StringLiteral<'a>,
        is_type_only: bool,
    ) {
        for spec in &decl.specifiers {
            // `export { load } from './x'` re-exports the load: the terminal
            // object is not a direct literal here, so the load-data harvest
            // abstains on this file.
            if !is_type_only
                && !spec.export_kind.is_type()
                && spec.exported.name().as_str() == "load"
            {
                self.has_unharvestable_load = true;
            }
            self.re_exports.push(ReExportInfo {
                source: source.value.to_string(),
                imported_name: spec.local.name().to_string(),
                exported_name: spec.exported.name().to_string(),
                is_type_only: is_type_only || spec.export_kind.is_type(),
                span: spec.span,
            });
        }
    }

    /// Record local declaration exports and `export { x }` local specifiers
    /// (no source), abstaining the load-data harvest on a bare `export { load }`.
    fn record_export_local_specifiers(
        &mut self,
        decl: &ExportNamedDeclaration<'a>,
        is_type_only: bool,
    ) {
        if let Some(declaration) = &decl.declaration {
            self.extract_declaration_exports(declaration, is_type_only);
            if !is_type_only {
                self.try_harvest_load_export(declaration);
            }
        }
        for spec in &decl.specifiers {
            let local_name_str = spec.local.name().as_str();
            let spec_type_only = is_type_only || spec.export_kind.is_type();

            // A local `export { load }` re-exports a `const load` declared
            // elsewhere in the file; the declaration-side harvest above already
            // covered a direct `const load = ...`. A bare export with no
            // matching local declaration means the terminal object is not
            // visible here, so abstain.
            if !spec_type_only
                && self.should_harvest_route_load_export(local_name_str)
                && self.load_return_keys.is_empty()
            {
                self.has_unharvestable_load = true;
            }

            self.pending_local_export_specifiers
                .push(PendingLocalExportSpecifier {
                    local_name: local_name_str.to_string(),
                    exported_name: spec.exported.name().to_string(),
                    is_type_only: spec_type_only,
                    span: spec.span,
                });
        }
    }

    /// Record public-API / local signature type references for a default export
    /// class, function, or interface declaration. A named declaration records a
    /// local type declaration + local refs; an anonymous one records public refs
    /// keyed on `"default"`.
    fn record_default_export_signature_refs(&mut self, decl: &ExportDefaultDeclaration<'a>) {
        match &decl.declaration {
            ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                let refs = Self::collect_class_signature_refs(class);
                if let Some(id) = class.id.as_ref() {
                    self.record_local_type_declaration(&id.name, id.span);
                    self.record_local_signature_refs(&id.name, refs);
                } else {
                    self.record_public_signature_refs("default", refs);
                }
            }
            ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                let refs = Self::collect_function_signature_refs(function);
                if let Some(id) = function.id.as_ref() {
                    self.record_local_signature_refs(&id.name, refs);
                } else {
                    self.record_public_signature_refs("default", refs);
                }
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(iface) => {
                self.record_local_type_declaration(&iface.id.name, iface.id.span);
                let refs = Self::collect_interface_signature_refs(iface);
                self.record_public_signature_refs("default", refs);
            }
            _ => {}
        }
    }

    /// Record `binding_target_names` / factory-call candidates from an
    /// initialized declarator: a `new Class()` RHS, a Svelte `$derived(new ...)`,
    /// a factory `[svc] = wrap(...)` array destructure, a `useMemo(() => new ...)`
    /// product binding, and a `Obj.method(...)` factory-call candidate.
    fn record_declarator_instance_bindings(
        &mut self,
        declarator: &VariableDeclarator<'a>,
        init: &Expression<'a>,
    ) {
        if let Expression::NewExpression(new_expr) = init
            && let Expression::Identifier(callee) = &new_expr.callee
            && let BindingPattern::BindingIdentifier(id) = &declarator.id
            && !super::helpers::is_builtin_constructor(callee.name.as_str())
        {
            self.insert_class_binding_target(id.name.to_string(), callee.name.to_string());
        }

        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Some(class_name) = Self::svelte_derived_new_class(init)
        {
            self.insert_class_binding_target(id.name.to_string(), class_name);
        }

        if let Expression::CallExpression(call) = init
            && let BindingPattern::ArrayPattern(arr_pat) = &declarator.id
            && let Some(Some(BindingPattern::BindingIdentifier(id))) = arr_pat.elements.first()
            && let Some(class_name) = super::helpers::try_extract_factory_new_class(&call.arguments)
        {
            self.insert_class_binding_target(id.name.to_string(), class_name);
        }

        // `const svc = useMemo(() => new Svc())`: useMemo returns the factory's
        // product directly, so the non-destructured binding is a class instance.
        // Scoped to useMemo (see `is_value_returning_memo_callee`) so arbitrary
        // wrappers and tuple-returning hooks like useState are not over-credited.
        // `or_insert` so a stronger pre-existing binding wins. See issue #844.
        if let Expression::CallExpression(call) = init
            && let BindingPattern::BindingIdentifier(id) = &declarator.id
            && is_value_returning_memo_callee(&call.callee)
            && let Some(class_name) = super::helpers::try_extract_factory_new_class(&call.arguments)
        {
            self.insert_class_binding_target_if_absent(id.name.to_string(), class_name);
        }

        if let Expression::CallExpression(call) = init
            && let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(callee_object) = &member.object
        {
            self.factory_call_candidates
                .push(super::FactoryCallCandidate {
                    local_name: id.name.to_string(),
                    callee_object: callee_object.name.to_string(),
                    callee_method: member.property.name.to_string(),
                });
        }
    }

    fn record_route_loader_data_declarator(
        &mut self,
        declarator: &VariableDeclarator<'a>,
        init: &Expression<'a>,
    ) {
        if !self.is_route_loader_data_call(init) {
            return;
        }

        match &declarator.id {
            BindingPattern::BindingIdentifier(id) => {
                self.route_loader_data_bindings.insert(id.name.to_string());
            }
            BindingPattern::ObjectPattern(obj_pat) => {
                if obj_pat.rest.is_some() {
                    self.whole_object_uses
                        .push(ROUTE_LOADER_DATA_OBJECT.to_string());
                    return;
                }
                for prop in &obj_pat.properties {
                    let Some(name) = prop.key.static_name() else {
                        self.whole_object_uses
                            .push(ROUTE_LOADER_DATA_OBJECT.to_string());
                        return;
                    };
                    self.member_accesses.push(MemberAccess {
                        object: ROUTE_LOADER_DATA_OBJECT.to_string(),
                        member: name.to_string(),
                    });
                }
            }
            _ => {
                self.whole_object_uses
                    .push(ROUTE_LOADER_DATA_OBJECT.to_string());
            }
        }
    }

    fn record_route_loader_data_whole_arg_use(&mut self, expr: &CallExpression<'a>) {
        for arg in &expr.arguments {
            if let Some(arg_expr) = arg.as_expression()
                && self.is_route_loader_data_call(arg_expr)
            {
                self.whole_object_uses
                    .push(ROUTE_LOADER_DATA_OBJECT.to_string());
                return;
            }
        }
    }

    fn is_route_loader_data_call(&self, expr: &Expression<'_>) -> bool {
        let Expression::CallExpression(call) = unwrap_route_loader_data_expr(expr) else {
            return false;
        };
        match &call.callee {
            Expression::Identifier(callee) => ROUTE_LOADER_DATA_SOURCES
                .iter()
                .any(|source| self.is_named_import_from(&callee.name, source, "useLoaderData")),
            Expression::StaticMemberExpression(member)
                if member.property.name == "useLoaderData" =>
            {
                let Expression::Identifier(namespace) = &member.object else {
                    return false;
                };
                ROUTE_LOADER_DATA_SOURCES.iter().any(|source| {
                    self.imports.iter().any(|import| {
                        import.source == *source
                            && import.local_name == namespace.name.as_str()
                            && matches!(import.imported_name, ImportedName::Namespace)
                    })
                })
            }
            _ => false,
        }
    }

    /// Process a single variable declarator: metadata, binding-target
    /// extraction, require / namespace-destructure / dynamic-import handling.
    /// Early returns mirror the original loop's `continue` control flow.
    fn record_variable_declarator(
        &mut self,
        decl: &VariableDeclaration<'a>,
        declarator: &VariableDeclarator<'a>,
    ) {
        self.record_variable_declarator_metadata(declarator);

        let Some(init) = &declarator.init else {
            self.record_uninitialized_variable_bindings(declarator);
            return;
        };

        self.record_initialized_variable_bindings(decl, declarator, init);
        self.record_playwright_variable_helpers(declarator, init);
        self.record_pinia_store(declarator, init);
        self.record_route_loader_data_declarator(declarator, init);

        // Capture a MODULE-SCOPE `let id = new Class()` / `let id = factory()`
        // initializer for the alias value-proof. Module scope only: a declarator
        // inside another function is a local binding, not the module binding an
        // alias factory returns. See #1441 (Part A).
        if self.is_module_scope()
            && let BindingPattern::BindingIdentifier(id) = &declarator.id
        {
            self.module_scope_initializers
                .entry(id.name.to_string())
                .or_default()
                .push(classify_factory_assigned_value(init));
        }

        // Type array / reactive-array bindings to their element class so
        // iteration callbacks and `for...of` loops can credit item member
        // accesses. Module-scope entries remain visible to template scanners;
        // function/block locals stay scoped to the active visitor stack.
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Some(element) =
                infer_array_binding_element_type(declarator.type_annotation.as_deref(), Some(init))
                    .or_else(|| self.promise_all_map_element_type(init))
        {
            let binding = id.name.to_string();
            self.record_vue_ref_value_array_binding_type(&binding, init, &element);
            self.record_array_binding_element_type(binding, element);
        }

        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            self.record_object_array_member_binding_types(
                id.name.as_str(),
                declarator.type_annotation.as_deref(),
            );
        }

        // FP-1 (unused-load-data-key): `const X = data` passes the whole
        // SvelteKit `data` prop opaquely, so a child could read any key the
        // detector cannot see. Name-gated on the bare `data` identifier; read
        // only by the load-data detector, so capturing it everywhere is
        // byte-identity-safe. The `{...data}` script-spread and
        // `{a, ...rest} = data` rest forms are already in `whole_object_uses`.
        if matches!(init, Expression::Identifier(id) if id.name == "data") {
            self.has_load_data_whole_use = true;
        }
        if let Expression::Identifier(id) = init
            && self.route_loader_data_bindings.contains(id.name.as_str())
        {
            self.whole_object_uses
                .push(ROUTE_LOADER_DATA_OBJECT.to_string());
        }
        if matches!(init, Expression::Identifier(id) if id.name == "loaderData") {
            self.whole_object_uses
                .push(ROUTE_LOADER_DATA_OBJECT.to_string());
        }

        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Expression::ObjectExpression(obj) = init
        {
            self.record_object_binding_targets(id.name.as_str(), obj);
        }

        if let Some((call, source)) = try_extract_require(init) {
            self.record_dompurify_require_binding(declarator, source);
            self.record_child_process_require_binding(declarator, source);
            self.handle_require_declaration(declarator, call, source);
            return;
        }

        self.record_declarator_instance_bindings(declarator, init);

        if let Expression::Identifier(ident) = init
            && self
                .namespace_binding_names
                .iter()
                .any(|n| n == ident.name.as_str())
        {
            self.handle_namespace_destructuring(declarator, &ident.name);
            return;
        }

        // Primitive A (unused-load-data-key): a destructure off the SvelteKit
        // `data` prop local (`const { user } = data` / `let { user } = data`)
        // emits `data.<key>` member accesses so the cross-file detector can see
        // the consumed load-return keys. Rooted on the `data` local (not an
        // import); a rest element (`const { a, ...rest } = data`) records a
        // whole-object use of `data` (abstain). Crediting a member against a
        // binding named `data` is inert for every other detector unless a
        // tracked export / instance is also named `data`; the load-data-key
        // join is the only consumer of `data.<key>`.
        if let Expression::Identifier(ident) = init
            && ident.name == "data"
        {
            self.handle_namespace_destructuring(declarator, &ident.name);
            return;
        }

        let Some(import_expr) = extract_import_expression(init) else {
            return;
        };
        let mut sources = Vec::new();
        collect_static_import_specifiers(&import_expr.source, &mut sources);
        if sources.is_empty() {
            return;
        }
        self.handle_dynamic_import_declaration(declarator, import_expr, &sources);
    }

    /// Record a CommonJS named export (`module.exports.X = ...` /
    /// `exports.X = ...` / a `module.exports = {...}` key) and flag the module
    /// as carrying CJS exports.
    fn push_cjs_named_export(&mut self, name: String, span: Span) {
        self.has_cjs_exports = true;
        self.exports.push(ExportInfo {
            name: ExportName::Named(name),
            local_name: None,
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span,
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        });
    }

    /// Handle CommonJS export assignments: `module.exports = { a, b }` (each key
    /// becomes a named export), `exports.X = ...`, and `module.exports.X = ...`.
    fn handle_cjs_member_export(
        &mut self,
        member: &StaticMemberExpression<'a>,
        expr: &AssignmentExpression<'a>,
    ) {
        if let Expression::Identifier(obj) = &member.object {
            if obj.name == "module" && member.property.name == "exports" {
                self.has_cjs_exports = true;
                if let Expression::ObjectExpression(obj_expr) = &expr.right {
                    for prop in &obj_expr.properties {
                        if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop
                            && let Some(name) = p.key.static_name()
                        {
                            self.push_cjs_named_export(name.to_string(), p.span);
                        }
                    }
                }
            }
            if obj.name == "exports" {
                self.push_cjs_named_export(member.property.name.to_string(), expr.span);
            }
        } else if let Expression::StaticMemberExpression(inner) = &member.object
            && let Expression::Identifier(obj) = &inner.object
            && obj.name == "module"
            && inner.property.name == "exports"
        {
            self.push_cjs_named_export(member.property.name.to_string(), expr.span);
        }
    }

    /// Handle `this.member = ...` / `this.#member = ...` assignments: record the
    /// member access and propagate the instance-binding target name from a
    /// `new Class()` RHS, an identifier bound to a known class, and nested
    /// binding targets. `member_name` is the property spelling (`dep` for a
    /// public field, `#dep` for a `#`-private field, issue #1821), so the
    /// recorded key is `this.<member_name>` and matches the receiver key
    /// `static_member_object_name` produces for the same field.
    fn handle_this_member_assignment(
        &mut self,
        member_name: &str,
        expr: &AssignmentExpression<'a>,
    ) {
        self.member_accesses.push(MemberAccess {
            object: "this".to_string(),
            member: member_name.to_string(),
        });
        if let Expression::NewExpression(new_expr) = &expr.right
            && let Expression::Identifier(callee) = &new_expr.callee
            && !super::helpers::is_builtin_constructor(callee.name.as_str())
        {
            self.insert_class_binding_target(
                format!("this.{member_name}"),
                callee.name.to_string(),
            );
        } else if let Expression::Identifier(ident) = &expr.right
            && let Some(target_name) = self.binding_target_names.get(ident.name.as_str()).cloned()
        {
            self.binding_target_names
                .insert(format!("this.{member_name}"), target_name);
        }
        if let Expression::Identifier(ident) = &expr.right {
            self.copy_nested_binding_targets(
                ident.name.as_str(),
                format!("this.{member_name}").as_str(),
            );
        }
    }

    /// Record sanitizer / allowlist / regex / path bindings cleared by an
    /// assignment to an identifier or member-object target.
    fn record_assignment_target_bindings(&mut self, left: &AssignmentTarget<'a>) {
        if let Some(name) = assignment_target_identifier_name(left) {
            self.record_sanitizer_binding(name, None);
            self.record_literal_allowlist_binding(name, false);
            self.record_risky_regex_binding(name, None);
            self.record_path_sink_binding(name, None);
            self.record_path_relative_binding(name, None);
        } else if let Some(name) = assignment_target_member_object_name(left)
            && self.literal_allowlist_binding(name)
        {
            self.record_literal_allowlist_binding(name, false);
        }
    }

    /// Record a Lit `@customElement('tag')` class as a side-effect registration
    /// candidate, keyed on the local class name or the pending anonymous default
    /// export slot.
    fn record_lit_custom_element(&mut self, class: &Class<'a>) {
        let Some(decorator) = lit_custom_element_decorator(class) else {
            return;
        };
        // The registered tag (`@customElement('x-foo')`) and the class span anchor
        // the Lit `unrendered-component` finding at the element, not line 1.
        let tag = lit_custom_element_tag(class);
        let span_start = class.span.start;
        if let Some(id) = class.id.as_ref() {
            self.record_lit_custom_element_candidate(
                decorator,
                SideEffectRegistrationTarget::LocalClass(id.name.to_string()),
                tag,
                span_start,
            );
        } else if let Some(export) = self.exports.last()
            && matches!(export.name, crate::ExportName::Default)
            && export.local_name.is_none()
        {
            let export_index = self.exports.len() - 1;
            self.record_lit_custom_element_candidate(
                decorator,
                SideEffectRegistrationTarget::AnonymousDefaultExport(export_index),
                tag,
                span_start,
            );
        }
    }

    /// Harvest the `@Component` selector(s) + class name + span for the Angular
    /// arm of the `unrendered-component` detector. Only a `@Component` (never
    /// `@Directive`) carries a selector here. A multi-selector string is split on
    /// `,` into the list; the detector restricts first-cut scope to
    /// all-element-selector components.
    fn record_angular_selector(
        &mut self,
        class: &Class<'a>,
        meta: &super::helpers::AngularComponentMetadata,
    ) {
        if let Some(ref selector_raw) = meta.selector
            && let Some(id) = class.id.as_ref()
        {
            let selectors = split_angular_selectors(selector_raw);
            if !selectors.is_empty() {
                self.angular_component_selectors
                    .push(AngularComponentSelector {
                        selectors,
                        span_start: class.span.start,
                        class_name: id.name.to_string(),
                    });
            }
        }
    }

    /// Record `templateUrl` / `styleUrls` external asset references as
    /// `SideEffect` imports for an Angular component.
    fn record_angular_template_assets(&mut self, meta: &super::helpers::AngularComponentMetadata) {
        if let Some(ref template_url) = meta.template_url {
            self.imports.push(ImportInfo {
                source: normalize_asset_url(template_url),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            });
            self.has_angular_component_template_url = true;
        }
        for style_url in &meta.style_urls {
            self.imports.push(ImportInfo {
                source: normalize_asset_url(style_url),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            });
        }
    }

    /// Scan an Angular inline `template:` string: record used selectors, the
    /// dynamic-render abstain, template member-access refs, offset-remapped
    /// security sinks, and the inline-template complexity finding.
    fn record_angular_inline_template(
        &mut self,
        class: &Class<'_>,
        meta: &super::helpers::AngularComponentMetadata,
    ) {
        let Some(ref template) = meta.inline_template else {
            return;
        };
        self.angular_used_selectors
            .extend(crate::sfc_template::angular::collect_angular_used_selectors(template));
        // `*ngComponentOutlet` dynamically renders a component from a non-literal
        // class reference; abstain project-wide.
        if template.contains("ngComponentOutlet") {
            self.has_dynamic_component_render = true;
        }

        // Type `@for` / `*ngFor` loop variables over a component field typed as an
        // array of a class (`utils: Util[]`) to the element class, so template
        // member accesses on the loop item (`{{ util.getName() }}`) remap onto the
        // class instead of being dropped (issue #1712).
        let field_types = super::helpers::collect_component_field_array_types(class);
        let refs = crate::sfc_template::angular::collect_angular_template_refs_with_field_types(
            template,
            &field_types,
        );
        for name in refs.identifiers {
            self.record_angular_template_member_fact(name.clone());
        }
        self.member_accesses.extend(refs.member_accesses);
        let template_offset = meta
            .inline_template_offset
            .unwrap_or(meta.decorator_span.start);
        self.security_sinks
            .extend(refs.security_sinks.into_iter().map(|mut sink| {
                sink.span_start = sink.span_start.saturating_add(template_offset);
                sink.span_end = sink.span_end.saturating_add(template_offset);
                sink
            }));

        self.inline_template_findings
            .push(super::InlineTemplateFinding {
                template_source: template.clone(),
                decorator_start: meta.decorator_span.start,
            });
    }

    fn record_angular_component_field_array_types(&mut self, class: &Class<'_>) {
        for (field, element_class) in super::helpers::collect_component_field_array_types(class) {
            self.record_angular_component_field_array_type_fact(field, element_class);
        }
    }

    /// Record Angular `host:` binding and `inputs:` / `outputs:` member refs as
    /// typed template member facts.
    fn record_angular_template_members(&mut self, meta: &super::helpers::AngularComponentMetadata) {
        for name in &meta.host_member_refs {
            self.record_angular_template_member_fact(name.clone());
        }
        for name in &meta.input_output_members {
            self.record_angular_template_member_fact(name.clone());
        }
    }

    fn record_string_coercion_to_string(&mut self, expr: &Expression<'_>) {
        if let Some(class_name) = new_expression_class_name(unwrap_static_expr(expr)) {
            self.member_accesses.push(MemberAccess {
                object: class_name,
                member: "toString".to_string(),
            });
        }
    }
}

impl<'a> Visit<'a> for ModuleInfoExtractor {
    fn visit_program(&mut self, program: &Program<'a>) {
        // Capture file-level string directives (`"use client"`, `"use server"`)
        // for the security client-server-leak detector. `directive.directive` is
        // the cooked directive text without surrounding quotes.
        for directive in &program.directives {
            self.directives
                .push(directive.directive.as_str().to_string());
        }
        self.record_program_prologue(program);
        self.record_program_sanitizer_functions(program);
        self.record_local_function_return_types(program);
        walk::walk_program(self, program);
    }

    fn visit_formal_parameter(&mut self, param: &FormalParameter<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &param.pattern
            && let Some(type_annotation) = param.type_annotation.as_deref()
        {
            self.record_typed_binding(id.name.as_str(), type_annotation);
            if param.accessibility.is_some() {
                self.record_typed_binding(format!("this.{}", id.name).as_str(), type_annotation);
            }
        }

        if let BindingPattern::ObjectPattern(obj_pat) = &param.pattern
            && let Some(type_annotation) = param.type_annotation.as_deref()
        {
            self.record_typed_destructure_binding(obj_pat, type_annotation);
        }

        self.record_angular_param_inject(param);

        walk::walk_formal_parameter(self, param);
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
            |name| Some(name.to_string()),
        );

        if let Some(name) = member_key.as_deref() {
            if let Some(type_annotation) = prop.type_annotation.as_deref() {
                self.record_typed_binding(format!("this.{name}").as_str(), type_annotation);

                if has_angular_plural_query_decorator(&prop.decorators)
                    && let Some(element_type) = extract_query_list_element_type(type_annotation)
                {
                    self.iterable_element_types
                        .insert(format!("this.{name}"), element_type);
                }
            }

            if let Some(Expression::NewExpression(new_expr)) = &prop.value
                && let Expression::Identifier(callee) = &new_expr.callee
                && !super::helpers::is_builtin_constructor(callee.name.as_str())
            {
                self.insert_class_binding_target(format!("this.{name}"), callee.name.to_string());
            }

            if let Some(Expression::CallExpression(call)) = &prop.value
                && let Some(type_name) = self.extract_angular_inject_target(call)
            {
                self.insert_class_binding_target(format!("this.{name}"), type_name);
            }

            if let Some(value) = prop.value.as_ref()
                && let Some(query) = extract_angular_signal_query(value)
            {
                let call_key = format!("this.{name}()");
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
            self.capture_hardcoded_secret_literal_sink(name.as_ref(), value, prop.span);
        }

        walk::walk_property_definition(self, prop);
    }

    fn visit_block_statement(&mut self, stmt: &BlockStatement<'a>) {
        self.block_depth += 1;
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.push(FxHashSet::default());
            self.scoped_array_binding_element_types
                .push(FxHashMap::default());
            self.sanitizer_binding_stack.push(FxHashMap::default());
            self.literal_allowlist_binding_stack
                .push(FxHashMap::default());
            self.risky_regex_binding_stack.push(FxHashMap::default());
            self.path_sink_binding_stack.push(FxHashMap::default());
            self.path_relative_binding_stack.push(FxHashMap::default());
        }
        for statement in &stmt.body {
            self.visit_statement(statement);
            if self.namespace_depth == 0 {
                self.record_fail_closed_guard_after_statement(statement);
            }
        }
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.pop();
            self.scoped_array_binding_element_types.pop();
            self.sanitizer_binding_stack.pop();
            self.literal_allowlist_binding_stack.pop();
            self.risky_regex_binding_stack.pop();
            self.path_sink_binding_stack.pop();
            self.path_relative_binding_stack.pop();
        }
        self.block_depth -= 1;
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
        self.record_next_function_param_sources(func);
        self.push_function_declaration_scope(&func.params);
        self.function_depth += 1;
        let component_pushed = self.react_enter_function(func);
        walk::walk_function(self, func, flags);
        self.react_exit_component(component_pushed);
        self.function_depth -= 1;
        self.pop_function_declaration_scope();
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        self.record_next_arrow_param_sources(expr);
        self.push_function_declaration_scope(&expr.params);
        self.function_depth += 1;
        let component_pushed = self.react_enter_arrow(expr);
        walk::walk_arrow_function_expression(self, expr);
        self.react_exit_component(component_pushed);
        self.function_depth -= 1;
        self.pop_function_declaration_scope();
    }

    fn visit_function_body(&mut self, body: &FunctionBody<'a>) {
        if self.namespace_depth == 0 {
            self.scoped_array_binding_element_types
                .push(FxHashMap::default());
        }
        for statement in &body.statements {
            self.visit_statement(statement);
            if self.namespace_depth == 0 {
                self.record_fail_closed_guard_after_statement(statement);
            }
        }
        if self.namespace_depth == 0 {
            self.scoped_array_binding_element_types.pop();
        }
    }

    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        let source = decl.source.value.to_string();
        let is_type_only = decl.import_kind.is_type();

        let source_span = decl.source.span;

        if let Some(specifiers) = &decl.specifiers {
            for spec in specifiers {
                match spec {
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        self.handle_import_specifier(s, &source, is_type_only, source_span);
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                        self.handle_import_default_specifier(s, &source, is_type_only, source_span);
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
        } else {
            self.imports.push(ImportInfo {
                source,
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: decl.span,
                source_span,
            });
        }
    }

    fn visit_export_named_declaration(&mut self, decl: &ExportNamedDeclaration<'a>) {
        let is_namespace = matches!(&decl.declaration, Some(Declaration::TSModuleDeclaration(_)));

        if self.namespace_depth > 0 {
            if let Some(declaration) = &decl.declaration {
                self.extract_namespace_members(declaration);
            }
            if is_namespace {
                self.namespace_depth += 1;
            }
            walk::walk_export_named_declaration(self, decl);
            if is_namespace {
                self.namespace_depth -= 1;
            }
            return;
        }

        let is_type_only = decl.export_kind.is_type();

        if let Some(source) = &decl.source {
            self.record_export_re_exports(decl, source, is_type_only);
        } else {
            self.record_export_local_specifiers(decl, is_type_only);
        }

        if is_namespace {
            self.namespace_depth += 1;
            self.pending_namespace_members.clear();
        }
        walk::walk_export_named_declaration(self, decl);
        if is_namespace {
            self.namespace_depth -= 1;
            if let Some(ns_export) = self.exports.last_mut() {
                ns_export.members = std::mem::take(&mut self.pending_namespace_members);
            }
        }
    }

    fn visit_export_default_declaration(&mut self, decl: &ExportDefaultDeclaration<'a>) {
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
                    extract_class_members(class, is_angular),
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
            _ => None,
        };

        self.record_default_export_signature_refs(decl);

        if super_class.is_some()
            || !implemented_interfaces.is_empty()
            || !instance_bindings.is_empty()
        {
            self.class_heritage.push(ClassHeritageInfo {
                export_name: "default".to_string(),
                super_class: super_class.clone(),
                implements: implemented_interfaces,
                instance_bindings,
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
        });

        walk::walk_export_default_declaration(self, decl);
    }

    fn visit_export_all_declaration(&mut self, decl: &ExportAllDeclaration<'a>) {
        let exported_name = decl
            .exported
            .as_ref()
            .map_or_else(|| "*".to_string(), |e| e.name().to_string());

        self.re_exports.push(ReExportInfo {
            source: decl.source.value.to_string(),
            imported_name: "*".to_string(),
            exported_name,
            is_type_only: decl.export_kind.is_type(),
            span: decl.span,
        });

        walk::walk_export_all_declaration(self, decl);
    }

    fn visit_import_expression(&mut self, expr: &ImportExpression<'a>) {
        if self.handled_import_spans.contains(&expr.span) {
            walk::walk_import_expression(self, expr);
            return;
        }

        // Static specifiers first (string literals, no-substitution templates,
        // and every statically-resolvable conditional/logical/parenthesized
        // branch); only when none resolve, fall back to the pattern shapes
        // (substitution templates and `'./x/' + y` concatenations).
        let mut sources = Vec::new();
        collect_static_import_specifiers(&expr.source, &mut sources);
        if !sources.is_empty() {
            self.push_dynamic_import_branches(&sources, expr.span, &[], None);
        } else {
            match &expr.source {
                Expression::TemplateLiteral(tpl)
                    if !tpl.quasis.is_empty() && !tpl.expressions.is_empty() =>
                {
                    self.record_dynamic_import_template_pattern(tpl, expr.span);
                }
                Expression::BinaryExpression(bin)
                    if bin.operator == oxc_ast::ast::BinaryOperator::Addition =>
                {
                    if let Some((prefix, suffix)) = extract_concat_parts(bin)
                        && (prefix.starts_with("./") || prefix.starts_with("../"))
                    {
                        self.dynamic_import_patterns.push(DynamicImportPattern {
                            prefix,
                            suffix,
                            span: expr.span,
                        });
                    }
                }
                _ => {}
            }
        }

        walk::walk_import_expression(self, expr);
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        // Pre-register named arrow / function-expression component bindings so
        // the function-body walk (below) can push the component stack with the
        // binding name. Runs before the body is walked. No-op on non-JSX files.
        self.react_prescan_variable_declaration(decl);
        for declarator in &decl.declarations {
            self.record_variable_declarator(decl, declarator);
        }
        walk::walk_variable_declaration(self, decl);
    }

    fn visit_object_property(&mut self, prop: &ObjectProperty<'a>) {
        self.record_graphql_resolver_args_source(&prop.value);

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
            self.capture_hardcoded_secret_literal_sink(name.as_ref(), &prop.value, prop.span);
        }

        self.record_angular_entry_component_refs(prop);

        walk::walk_object_property(self, prop);
    }

    fn visit_object_expression(&mut self, obj: &ObjectExpression<'a>) {
        self.record_angular_provider_object(obj);

        walk::walk_object_expression(self, obj);
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        self.record_structural_class_call_candidate(expr);
        if let Expression::Identifier(callee) = &expr.callee
            && callee.name == "String"
            && let Some(arg) = expr.arguments.first()
            && let Some(arg_expr) = arg.as_expression()
        {
            self.record_string_coercion_to_string(arg_expr);
        }
        self.clear_literal_allowlist_on_mutating_member_call(expr);
        self.record_framework_callback_param_sources(expr);
        self.react_record_hook_call(expr);

        if let Some(test_name) = playwright_test_callee_name(&expr.callee) {
            let fixture_uses = collect_playwright_fixture_member_uses(&expr.arguments);
            for access in &fixture_uses {
                self.record_playwright_fixture_use_fact(
                    test_name.clone(),
                    access.fixture_name.clone(),
                    access.member.clone(),
                );
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
        self.record_vitest_mock_dynamic_imports(expr);

        self.try_record_pino_transport_targets(expr);
        self.try_record_node_module_register(expr);
        self.try_record_child_process_fork(expr);
        self.try_record_package_path_reference(expr);
        self.record_bare_require_call(expr);
        self.record_whole_object_call_use(expr);
        self.record_import_meta_glob_patterns(expr);
        self.record_require_context_pattern(expr);
        self.record_import_callback_dynamic_imports(expr);
        self.record_arrow_wrapped_dynamic_import(expr);

        self.try_record_fluent_chain_access(expr);
        self.record_pinia_map_helpers(expr);
        self.record_di_key_site(expr);
        self.record_svelte_dispatch_call(expr);
        self.record_svelte_dispatch_whole_arg_use(expr);
        self.record_load_data_whole_arg_use(expr);
        self.record_route_loader_data_whole_arg_use(expr);

        self.capture_security_call_sites(expr);

        self.record_angular_dynamic_component_render(expr);
        self.record_angular_bootstrap_call(expr);
        self.record_angular_dynamic_providers(expr);

        self.record_callee_use(expr);

        walk::walk_call_expression(self, expr);
    }

    fn visit_for_of_statement(&mut self, stmt: &ForOfStatement<'a>) {
        self.bind_for_of_element(stmt);

        if let Some((strings, objects)) = self.static_package_loop_bindings(stmt) {
            self.loop_string_bindings.push(strings);
            self.loop_object_property_values.push(objects);
            walk::walk_for_of_statement(self, stmt);
            self.loop_object_property_values.pop();
            self.loop_string_bindings.pop();
            return;
        }

        walk::walk_for_of_statement(self, stmt);
    }

    fn visit_template_literal(&mut self, tpl: &TemplateLiteral<'a>) {
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

        if let Some(source) = new_url_import_source(expr) {
            // A `new URL(specifier, import.meta.url)` whose specifier has no file
            // extension may refer to a directory rather than a module (e.g.
            // `new URL("./services", import.meta.url)` to obtain the directory URL
            // via `fileURLToPath(...)`). Such a specifier cannot be resolved to a
            // module, so marking it speculative causes the resolver to silently drop
            // it when the target is unresolvable. Specifiers with an extension
            // (e.g. `./worker.js`) keep `is_speculative = false` so genuinely
            // missing files are still reported as `unresolved-import`.
            // See issue #840.
            let is_speculative = PathBuf::from(&source).extension().is_none();
            self.dynamic_imports.push(DynamicImportInfo {
                source,
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
                is_speculative,
            });
        }

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
        let source = node.source.value.to_string();
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
            from_style: false,
            span: node.span,
            source_span,
        });

        walk::walk_ts_import_type(self, node);
    }

    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
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
        walk::walk_assignment_expression(self, expr);
    }

    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        if is_import_meta_env_object(&expr.object) {
            self.member_accesses.push(MemberAccess {
                object: "import.meta.env".to_string(),
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
            self.factory_unnamed_result_accesses
                .push((callee_name, expr.property.name.to_string()));
        }
        if let Some(store_factory) = Self::inline_store_factory_receiver(&expr.object) {
            // `useFooStore().member` with no bound local: the generic receiver
            // name would be the inert `useFooStore()` string. Credit the member on
            // the factory import directly, mirroring the bound
            // `const s = useFooStore(); s.member` path (see `record_pinia_store`,
            // issue #1489 Case 1).
            self.member_accesses.push(MemberAccess {
                object: store_factory,
                member: expr.property.name.to_string(),
            });
        } else if let Some(object_name) = static_member_object_name(&expr.object) {
            if self
                .route_loader_data_bindings
                .contains(object_name.as_str())
            {
                self.member_accesses.push(MemberAccess {
                    object: ROUTE_LOADER_DATA_OBJECT.to_string(),
                    member: expr.property.name.to_string(),
                });
            }
            self.member_accesses.push(MemberAccess {
                object: object_name,
                member: expr.property.name.to_string(),
            });
        }
        if matches!(expr.object, Expression::Super(_))
            && let Some(Some(super_local)) = self.class_super_stack.last()
        {
            self.member_accesses.push(MemberAccess {
                object: super_local.clone(),
                member: expr.property.name.to_string(),
            });
        }
        walk::walk_static_member_expression(self, expr);
    }

    fn visit_computed_member_expression(&mut self, expr: &ComputedMemberExpression<'a>) {
        if let Expression::Identifier(obj) = &expr.object {
            if (self.route_loader_data_bindings.contains(obj.name.as_str())
                || obj.name == "loaderData")
                && let Expression::StringLiteral(lit) = &expr.expression
            {
                self.member_accesses.push(MemberAccess {
                    object: ROUTE_LOADER_DATA_OBJECT.to_string(),
                    member: lit.value.to_string(),
                });
            }
            if let Expression::StringLiteral(lit) = &expr.expression {
                self.member_accesses.push(MemberAccess {
                    object: obj.name.to_string(),
                    member: lit.value.to_string(),
                });
            } else {
                self.record_whole_object_identifier_use(obj.name.as_str());
            }
        }
        walk::walk_computed_member_expression(self, expr);
    }

    fn visit_ts_qualified_name(&mut self, it: &TSQualifiedName<'a>) {
        if let TSTypeName::IdentifierReference(obj) = &it.left {
            self.member_accesses.push(MemberAccess {
                object: obj.name.to_string(),
                member: it.right.name.to_string(),
            });
        }
        walk::walk_ts_qualified_name(self, it);
    }

    fn visit_ts_mapped_type(&mut self, it: &TSMappedType<'a>) {
        if let TSType::TSTypeReference(type_ref) = &it.constraint
            && let TSTypeName::IdentifierReference(ident) = &type_ref.type_name
        {
            self.whole_object_uses.push(ident.name.to_string());
        }
        if let TSType::TSTypeOperatorType(op) = &it.constraint
            && op.operator == TSTypeOperatorOperator::Keyof
            && let TSType::TSTypeQuery(query) = &op.type_annotation
            && let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name
        {
            self.whole_object_uses.push(ident.name.to_string());
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
            self.whole_object_uses.push(key_ident.name.to_string());
        }
        walk::walk_ts_type_reference(self, it);
    }

    fn visit_for_in_statement(&mut self, stmt: &ForInStatement<'a>) {
        if let Expression::Identifier(ident) = &stmt.right {
            self.record_whole_object_identifier_use(ident.name.as_str());
        }
        walk::walk_for_in_statement(self, stmt);
    }

    fn visit_if_statement(&mut self, stmt: &IfStatement<'a>) {
        // Record `x instanceof ClassName` narrowings from the test condition so
        // that method calls on `x` inside the body (e.g. `x.getMessage()`) are
        // credited as uses of `ClassName.getMessage`, preventing false
        // unused-class-member findings. The bindings are module-scoped (not
        // strictly block-scoped), which is conservative: it may credit accesses
        // outside the guard, but that produces at most false negatives, not false
        // positives.
        let mut narrowings = Vec::new();
        collect_instanceof_narrowings(&stmt.test, &mut narrowings);
        for (local, class_name) in narrowings {
            self.insert_class_binding_target_if_absent(local, class_name);
        }
        walk::walk_if_statement(self, stmt);
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
            Expression::ThisExpression(_) => self.record_angular_this_spread_fact(),
            _ => {}
        }
        walk::walk_spread_element(self, elem);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        self.record_lit_custom_element(class);

        if let Some(meta) = extract_angular_component_metadata(class) {
            self.record_angular_selector(class, &meta);
            self.record_angular_component_field_array_types(class);
            self.record_angular_template_assets(&meta);
            self.record_angular_inline_template(class, &meta);
            self.record_angular_template_members(&meta);
        }
        self.class_super_stack
            .push(super::helpers::extract_super_class_name(class));
        self.class_type_param_constraints
            .push(super::helpers::collect_class_type_param_constraints(class));
        walk::walk_class(self, class);
        self.class_type_param_constraints.pop();
        self.class_super_stack.pop();
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
        if is_html_tagged_template(&expr.tag) {
            for quasi in &expr.quasi.quasis {
                let text = quasi
                    .value
                    .cooked
                    .as_ref()
                    .map_or_else(|| quasi.value.raw.as_str(), |c| c.as_str());
                for raw in crate::html::collect_asset_refs(text) {
                    self.push_html_template_asset_import(&raw);
                }
                // Record custom-element tags rendered in the template
                // (`<x-foo>`), feeding the Lit `unrendered-component` arm's
                // project-wide rendered-tag union.
                for tag in crate::html::collect_custom_element_tags(text) {
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
                    .as_ref()
                    .map_or_else(|| quasi.value.raw.as_str(), |c| c.as_str());
                if text.ends_with('<') || text.ends_with("</") {
                    self.record_dynamic_custom_element_render_fact();
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

    fn visit_jsx_element(&mut self, element: &oxc_ast::ast::JSXElement<'a>) {
        // Record a render edge for a component tag (capitalized or
        // member-expression) plus the passed attribute names + spread presence.
        // Lowercase host elements are skipped for render purposes. No-op on
        // non-JSX files (perf gate). The walk continues into children so nested
        // renders and host-element nesting depth are still visited.
        self.react_record_jsx_element(element);
        walk::walk_jsx_element(self, element);
    }
}

/// Split an Angular `@Component` selector string into its individual selectors.
///
/// A selector value can be a comma-separated list (`'app-foo, [appBar], .baz'`).
/// Each part is trimmed; empty parts are dropped. The raw shape of each selector
/// is preserved (element `app-foo`, attribute `[appFoo]`, class `.foo`,
/// `:not(...)` etc.) so the detector can classify them and restrict its
/// first-cut scope to all-element-selector components.
fn split_angular_selectors(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Extract the class name from a route `loadComponent` value of the shape
/// `() => import('./x').then(m => m.FooComponent)`. Returns the member name
/// (`FooComponent`) accessed on the `.then` callback's parameter, which is the
/// lazily-loaded component class. Returns `None` for any other shape (the
/// `.then` already credits the named import elsewhere; this is the
/// entry-point-abstain capture for the Angular `unrendered-component` rule).
fn then_callback_member_class(value: &Expression<'_>) -> Option<String> {
    // Unwrap the outer `() => <body>` arrow.
    let body = arrow_expression_body(value)?;
    // The body must be a `<expr>.then(<callback>)` call.
    let Expression::CallExpression(call) = body else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if member.property.name.as_str() != "then" {
        return None;
    }
    // The callback is `m => m.FooComponent` (or `({ FooComponent }) => ...`,
    // which is covered by the named-import credit elsewhere; here we only need the
    // member-access shape).
    let Some(Argument::ArrowFunctionExpression(_)) = call.arguments.first() else {
        return None;
    };
    let Some(Argument::ArrowFunctionExpression(cb)) = call.arguments.first() else {
        return None;
    };
    let cb_body = arrow_fn_expression_body(cb)?;
    let Expression::StaticMemberExpression(member) = cb_body else {
        return None;
    };
    Some(member.property.name.to_string())
}

/// The expression body of a `() => <expr>` arrow, unwrapping the optional
/// parenthesized-expression-statement form. Returns `None` for a block body that
/// is not a sole expression statement.
fn arrow_expression_body<'a, 'b>(value: &'b Expression<'a>) -> Option<&'b Expression<'a>> {
    let Expression::ArrowFunctionExpression(arrow) = value else {
        return None;
    };
    arrow_fn_expression_body(arrow)
}

/// The sole expression of an arrow function's body (expression-bodied arrow, or a
/// block body with a single `return <expr>` / `<expr>;` statement).
fn arrow_fn_expression_body<'a, 'b>(
    arrow: &'b oxc_ast::ast::ArrowFunctionExpression<'a>,
) -> Option<&'b Expression<'a>> {
    if arrow.expression {
        return match arrow.body.statements.first() {
            Some(Statement::ExpressionStatement(stmt)) => Some(&stmt.expression),
            _ => None,
        };
    }
    match arrow.body.statements.first() {
        Some(Statement::ReturnStatement(ret)) => ret.argument.as_ref(),
        Some(Statement::ExpressionStatement(stmt)) => Some(&stmt.expression),
        _ => None,
    }
}

fn static_argument_object_name(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::Identifier(ident) => Some(ident.name.to_string()),
        Argument::ThisExpression(_) => Some("this".to_string()),
        Argument::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            static_member_object_name(&member.object)?,
            member.property.name
        )),
        _ => None,
    }
}

fn assignment_target_identifier_name<'b>(target: &'b AssignmentTarget<'_>) -> Option<&'b str> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(ident) => Some(ident.name.as_str()),
        AssignmentTarget::TSAsExpression(ts_as) => expression_identifier_name(&ts_as.expression),
        AssignmentTarget::TSSatisfiesExpression(ts_sat) => {
            expression_identifier_name(&ts_sat.expression)
        }
        AssignmentTarget::TSNonNullExpression(ts_non_null) => {
            expression_identifier_name(&ts_non_null.expression)
        }
        AssignmentTarget::TSTypeAssertion(ts_assertion) => {
            expression_identifier_name(&ts_assertion.expression)
        }
        _ => None,
    }
}

fn expression_identifier_name<'b>(expr: &'b Expression<'_>) -> Option<&'b str> {
    match expr {
        Expression::Identifier(ident) => Some(ident.name.as_str()),
        Expression::ParenthesizedExpression(paren) => expression_identifier_name(&paren.expression),
        Expression::TSAsExpression(ts_as) => expression_identifier_name(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => expression_identifier_name(&ts_sat.expression),
        Expression::TSNonNullExpression(ts_non_null) => {
            expression_identifier_name(&ts_non_null.expression)
        }
        Expression::TSTypeAssertion(ts_assertion) => {
            expression_identifier_name(&ts_assertion.expression)
        }
        _ => None,
    }
}

fn assignment_target_member_object_name<'b>(target: &'b AssignmentTarget<'_>) -> Option<&'b str> {
    match target {
        AssignmentTarget::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) => Some(object.name.as_str()),
            _ => None,
        },
        AssignmentTarget::ComputedMemberExpression(member) => match &member.object {
            Expression::Identifier(object) => Some(object.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn is_literal_string_allowlist_expr(expr: &Expression<'_>) -> bool {
    match unwrap_static_expr(expr) {
        Expression::ArrayExpression(array) => is_string_literal_array(array),
        Expression::NewExpression(new_expr) => {
            let Expression::Identifier(callee) = &new_expr.callee else {
                return false;
            };
            if callee.name != "Set" {
                return false;
            }
            let Some(Argument::ArrayExpression(array)) = new_expr.arguments.first() else {
                return false;
            };
            is_string_literal_array(array)
        }
        _ => false,
    }
}

fn is_string_literal_array(array: &ArrayExpression<'_>) -> bool {
    array
        .elements
        .iter()
        .all(|element| matches!(element, ArrayExpressionElement::StringLiteral(_)))
}

fn unwrap_static_expr<'a, 'b>(mut expr: &'b Expression<'a>) -> &'b Expression<'a> {
    loop {
        match expr {
            Expression::ParenthesizedExpression(paren) => expr = &paren.expression,
            Expression::TSAsExpression(ts_as) => expr = &ts_as.expression,
            Expression::TSSatisfiesExpression(ts_sat) => expr = &ts_sat.expression,
            Expression::TSNonNullExpression(ts_non_null) => expr = &ts_non_null.expression,
            _ => return expr,
        }
    }
}

fn is_string_coercion_sibling(expr: &Expression<'_>) -> bool {
    matches!(
        unwrap_static_expr(expr),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
    )
}

fn new_expression_class_name(expr: &Expression<'_>) -> Option<String> {
    let Expression::NewExpression(new_expr) = expr else {
        return None;
    };
    let Expression::Identifier(callee) = &new_expr.callee else {
        return None;
    };
    if super::helpers::is_builtin_constructor(callee.name.as_str()) {
        return None;
    }
    Some(callee.name.to_string())
}

/// The callee identifier name of the (optionally awaited) call an iteration map
/// callback returns: `async (n) => createDb(n)`, `(n) => await createDb(n)`, or
/// a single-`return` block body. Member-expression callees and multi-statement
/// bodies (including conditional early returns) yield `None`, keeping the
/// Promise.all element inference conservative. See issue #1793.
fn map_callback_returned_call_name(arg: &Argument<'_>) -> Option<String> {
    let returned = match arg {
        Argument::ArrowFunctionExpression(arrow) => {
            if arrow.expression {
                let Statement::ExpressionStatement(stmt) = arrow.body.statements.first()? else {
                    return None;
                };
                &stmt.expression
            } else {
                single_return_expr(&arrow.body)?
            }
        }
        Argument::FunctionExpression(function) => single_return_expr(function.body.as_deref()?)?,
        _ => return None,
    };
    let Expression::CallExpression(call) = unwrap_await_paren_expr(returned) else {
        return None;
    };
    match &call.callee {
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// The single returned expression of a `{ return <expr>; }` block body. Any
/// other statement count yields `None`, so a multi-statement or conditional
/// callback body does not credit an element class. See issue #1793.
fn single_return_expr<'a, 'b>(body: &'b FunctionBody<'a>) -> Option<&'b Expression<'a>> {
    if body.statements.len() != 1 {
        return None;
    }
    let Statement::ReturnStatement(ret) = body.statements.first()? else {
        return None;
    };
    ret.argument.as_ref()
}

/// Recursively unwrap `await` and parenthesized wrappers to the inner
/// expression. Used by the Promise.all element inference. See issue #1793.
fn unwrap_await_paren_expr<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    match expr {
        Expression::AwaitExpression(await_expr) => unwrap_await_paren_expr(&await_expr.argument),
        Expression::ParenthesizedExpression(paren) => unwrap_await_paren_expr(&paren.expression),
        other => other,
    }
}

/// Recursively unwrap parenthesized expressions to reach the inner expression.
fn unwrap_parens<'a, 'b>(mut expr: &'b Expression<'a>) -> &'b Expression<'a> {
    while let Expression::ParenthesizedExpression(paren) = expr {
        expr = &paren.expression;
    }
    expr
}

/// Flatten an `Identifier` or `StaticMemberExpression` callee to a dotted path.
///
/// Deliberately narrower than `static_member_object_name`: it accepts ONLY bare
/// identifiers and static member chains (no call/new forms), so the catalogue
/// matcher sees a clean dotted callee path. Returns `None` for dynamic dispatch,
/// computed members, or aliased call forms (those count as blind spots).
fn flatten_callee_path(expr: &Expression<'_>) -> Option<String> {
    match unwrap_parens(expr) {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            flatten_callee_path(&member.object)?,
            member.property.name
        )),
        _ => None,
    }
}

fn terminal_static_member_name<'a>(expr: &'a Expression<'_>) -> Option<&'a str> {
    match unwrap_parens(expr) {
        Expression::StaticMemberExpression(member) => Some(member.property.name.as_str()),
        _ => None,
    }
}

fn callee_method_name<'a>(
    callee: &'a Expression<'_>,
    callee_path: Option<&'a str>,
) -> Option<&'a str> {
    if let Some(callee_path) = callee_path {
        return callee_path.rsplit_once('.').map(|(_, method)| method);
    }
    terminal_static_member_name(callee)
}

/// Flatten an expression to a dotted member path, unwrapping `await` and parens.
/// Returns `None` for anything that is not an identifier-rooted static-member
/// chain (call results, computed members, etc. are not flattened: a conservative
/// miss, never a wrong source link).
fn flatten_member_path(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::ParenthesizedExpression(paren) => flatten_member_path(&paren.expression),
        Expression::AwaitExpression(await_expr) => flatten_member_path(&await_expr.argument),
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        // `import.meta` is a MetaProperty, not a member chain; flattening it as
        // `import.meta` lets `import.meta.env.X` reads be modeled as a source the
        // same way `process.env.X` is (issue #890, Vite secrets).
        Expression::MetaProperty(meta) => {
            Some(format!("{}.{}", meta.meta.name, meta.property.name))
        }
        Expression::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            flatten_member_path(&member.object)?,
            member.property.name
        )),
        _ => None,
    }
}

/// The source path for a DIRECT binding (`const id = req.query.id`): the OBJECT
/// path of the member-access init, i.e. the chain with its final property
/// dropped (`req.query`). A bare-identifier init (`const x = req`) has no object
/// to drop and is not a source binding on its own (`None`). A PUBLIC env var
/// (`process.env.NEXT_PUBLIC_X`, `import.meta.env.VITE_Y`) is build-inlined and
/// is NOT a secret source, so it is dropped here (issue #890).
fn tainted_source_path(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::ParenthesizedExpression(paren) => tainted_source_path(&paren.expression),
        Expression::AwaitExpression(await_expr) => tainted_source_path(&await_expr.argument),
        Expression::StaticMemberExpression(member) => {
            if let Some(full) = flatten_member_path(expr)
                && fallow_types::extract::is_public_env_path(&full)
            {
                return None;
            }
            flatten_member_path(&member.object)
        }
        _ => None,
    }
}

fn push_unique_string(out: &mut Vec<String>, value: String) {
    if !out.iter().any(|existing| existing == &value) {
        out.push(value);
    }
}

fn source_path_candidates(expr: &Expression<'_>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(path) = flatten_member_path(expr) {
        push_unique_string(&mut out, path);
    }
    if let Some(path) = tainted_source_path(expr) {
        push_unique_string(&mut out, path);
    }
    out
}

fn binding_source_path_candidates(expr: &Expression<'_>) -> Vec<String> {
    let mut out = Vec::new();
    collect_binding_source_path_candidates(expr, &mut out);
    out
}

fn collect_binding_source_path_candidates(expr: &Expression<'_>, out: &mut Vec<String>) {
    for path in source_path_candidates(expr) {
        push_unique_string(out, path);
    }

    match expr {
        Expression::ParenthesizedExpression(paren) => {
            collect_binding_source_path_candidates(&paren.expression, out);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_binding_source_path_candidates(&ts_as.expression, out);
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            collect_binding_source_path_candidates(&ts_sat.expression, out);
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            collect_binding_source_path_candidates(&ts_non_null.expression, out);
        }
        Expression::AwaitExpression(await_expr) => {
            collect_binding_source_path_candidates(&await_expr.argument, out);
        }
        Expression::TemplateLiteral(template) => {
            for expression in &template.expressions {
                collect_binding_source_path_candidates(expression, out);
            }
        }
        Expression::BinaryExpression(binary)
            if binary.operator == oxc_ast::ast::BinaryOperator::Addition =>
        {
            collect_binding_source_path_candidates(&binary.left, out);
            collect_binding_source_path_candidates(&binary.right, out);
        }
        Expression::ObjectExpression(object) => {
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    continue;
                };
                collect_binding_source_path_candidates(&property.value, out);
            }
        }
        _ => {}
    }
}

/// Collect bare identifier references from a declarator initializer for the
/// #1146 taint chain step, recursing through the same conservative expression
/// shapes `collect_binding_source_path_candidates` admits (static TS wrappers,
/// await, template substitutions, `+` concat operands, object-literal property
/// values) plus the top-level bare-identifier alias (`const b = a`).
///
/// Deliberately NOT collected (each chained identifier becomes a fresh
/// source-backed binding that can chain further, so every arm here is a
/// false-positive amplifier):
/// - member-expression roots (`const b = a.id`): a property read off a tainted
///   local frequently strips taint in practice (`a.length`,
///   `a.startsWith("/")`); unlike the sink-side `collect_idents_into` member
///   rule (sound because the whole expression flows into a sink), re-tainting
///   the result here would propagate boolean/length/index reads
/// - call expressions (`const b = f(a)`): the call boundary is where
///   sanitizers live; #878 already covers proven helper-return shapes
/// - logical / conditional / sequence expressions (`a || "x"`, `c ? a : b`)
fn collect_chained_taint_idents(expr: &Expression<'_>, out: &mut Vec<String>) {
    match expr {
        Expression::Identifier(ident) => push_ident(ident.name.as_str(), out),
        Expression::ParenthesizedExpression(paren) => {
            collect_chained_taint_idents(&paren.expression, out);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_chained_taint_idents(&ts_as.expression, out);
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            collect_chained_taint_idents(&ts_sat.expression, out);
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            collect_chained_taint_idents(&ts_non_null.expression, out);
        }
        Expression::AwaitExpression(await_expr) => {
            collect_chained_taint_idents(&await_expr.argument, out);
        }
        Expression::TemplateLiteral(template) => {
            for expression in &template.expressions {
                collect_chained_taint_idents(expression, out);
            }
        }
        Expression::BinaryExpression(binary)
            if binary.operator == oxc_ast::ast::BinaryOperator::Addition =>
        {
            collect_chained_taint_idents(&binary.left, out);
            collect_chained_taint_idents(&binary.right, out);
        }
        Expression::ObjectExpression(object) => {
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    continue;
                };
                collect_chained_taint_idents(&property.value, out);
            }
        }
        _ => {}
    }
}

fn source_returning_helper(
    params: &FormalParameters<'_>,
    expr: &Expression<'_>,
) -> Option<super::SourceReturningHelper> {
    let param_names = params
        .items
        .iter()
        .map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut paths = Vec::new();
    for source_path in source_path_candidates(expr) {
        for (arg_index, param_name) in param_names.iter().enumerate() {
            let Some(param_name) = param_name else {
                continue;
            };
            if source_path == *param_name {
                paths.push(super::SourceReturnPath {
                    arg_index,
                    suffixes: vec![String::new()],
                });
                break;
            }
            let Some(suffix) = source_path.strip_prefix(&format!("{param_name}.")) else {
                continue;
            };
            paths.push(super::SourceReturnPath {
                arg_index,
                suffixes: vec![suffix.to_string()],
            });
            break;
        }
    }
    if paths.is_empty() {
        None
    } else {
        Some(super::SourceReturningHelper { paths })
    }
}

fn extract_function_body_final_return_expr<'a, 'b>(
    body: &'b oxc_ast::ast::FunctionBody<'a>,
) -> Option<&'b Expression<'a>> {
    let Statement::ReturnStatement(ret) = body.statements.last()? else {
        return None;
    };
    ret.argument.as_ref()
}

fn extract_arrow_return_expr<'a, 'b>(
    arrow: &'b oxc_ast::ast::ArrowFunctionExpression<'a>,
) -> Option<&'b Expression<'a>> {
    if arrow.expression {
        if arrow.body.statements.len() != 1 {
            return None;
        }
        let Statement::ExpressionStatement(stmt) = arrow.body.statements.first()? else {
            return None;
        };
        if let Expression::ParenthesizedExpression(paren) = &stmt.expression {
            return Some(&paren.expression);
        }
        return Some(&stmt.expression);
    }
    extract_function_body_final_return_expr(&arrow.body)
}

fn unwrap_paren_expr<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(paren) => &paren.expression,
        other => other,
    }
}

fn apply_source_return_path(expr: &Expression<'_>, suffixes: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if suffixes.iter().any(String::is_empty) {
        for candidate in source_path_candidates(expr) {
            push_unique_string(&mut out, candidate);
        }
    }
    let Some(base) = flatten_member_path(expr) else {
        return out;
    };
    for suffix in suffixes {
        if suffix.is_empty() {
            push_unique_string(&mut out, base.clone());
        } else {
            push_unique_string(&mut out, format!("{base}.{suffix}"));
        }
    }
    out
}

fn push_ident(name: &str, out: &mut Vec<String>) {
    if !out.iter().any(|n| n == name) {
        out.push(name.to_string());
    }
}

/// The source path for a DESTRUCTURE binding (`const { id } = req.query`): the
/// FULL flattened init path (`req.query`), since the destructured keys are the
/// leaves. A bare-identifier init (`const { id } = req`) yields `req`.
fn destructure_source_path(expr: &Expression<'_>) -> Option<String> {
    flatten_member_path(expr)
}

fn static_member_object_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(obj) => Some(obj.name.to_string()),
        Expression::ThisExpression(_) => Some("this".to_string()),
        Expression::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            static_member_object_name(&member.object)?,
            member.property.name
        )),
        // `#`-private field receiver (`this.#dep`): oxc's `PrivateIdentifier.name`
        // excludes the leading `#`, so spell the key `this.#dep` to match the
        // binding key `visit_property_definition` / the assignment arms record
        // for private fields (issue #1821).
        Expression::PrivateFieldExpression(member) => Some(format!(
            "{}.#{}",
            static_member_object_name(&member.object)?,
            member.field.name
        )),
        Expression::CallExpression(call) if call.arguments.is_empty() => {
            Some(format!("{}()", static_member_object_name(&call.callee)?))
        }
        Expression::NewExpression(new_expr) => match &new_expr.callee {
            Expression::Identifier(callee) => Some(callee.name.to_string()),
            _ => None,
        },
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) if call.arguments.is_empty() => {
                Some(format!("{}()", static_member_object_name(&call.callee)?))
            }
            ChainElement::StaticMemberExpression(member) => Some(format!(
                "{}.{}",
                static_member_object_name(&member.object)?,
                member.property.name
            )),
            _ => None,
        },
        _ => None,
    }
}

fn unwrap_route_loader_data_expr<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::TSAsExpression(as_expr) => unwrap_route_loader_data_expr(&as_expr.expression),
        Expression::TSSatisfiesExpression(sat) => unwrap_route_loader_data_expr(&sat.expression),
        Expression::ParenthesizedExpression(paren) => {
            unwrap_route_loader_data_expr(&paren.expression)
        }
        _ => expr,
    }
}

fn is_import_meta_env_object(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::StaticMemberExpression(member)
            if member.property.name == "env"
                && matches!(
                    &member.object,
                    Expression::MetaProperty(meta)
                        if meta.meta.name == "import" && meta.property.name == "meta"
                )
    )
}

fn for_of_binding_name(left: &ForStatementLeft<'_>) -> Option<String> {
    match left {
        ForStatementLeft::VariableDeclaration(decl) => {
            let declarator = decl.declarations.first()?;
            binding_pattern_value_name(&declarator.id)
        }
        ForStatementLeft::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn binding_pattern_value_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
        BindingPattern::ArrayPattern(array) => array.elements.iter().rev().find_map(|element| {
            let Some(pattern) = element else {
                return None;
            };
            let BindingPattern::BindingIdentifier(id) = pattern else {
                return None;
            };
            Some(id.name.to_string())
        }),
        _ => None,
    }
}

fn object_values_or_entries_argument_name(expr: &Expression<'_>) -> Option<String> {
    let Expression::CallExpression(call) = expr else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    let Expression::Identifier(object) = &member.object else {
        return None;
    };
    if object.name != "Object" || !matches!(member.property.name.as_str(), "values" | "entries") {
        return None;
    }
    let Some(Argument::Identifier(arg)) = call.arguments.first() else {
        return None;
    };
    Some(arg.name.to_string())
}

/// Returns true when the tagged template's tag is the bare identifier `html`.
fn is_html_tagged_template(tag: &Expression<'_>) -> bool {
    matches!(tag, Expression::Identifier(id) if id.name == "html")
}

/// Collect `(local_name, class_name)` pairs from an `instanceof` guard expression.
///
/// Recurses through `&&`-chained conditions so `a instanceof A && b instanceof B`
/// yields both pairs. Only simple identifier left-hand sides (`x instanceof Cls`)
/// are collected; complex left-hand expressions are skipped conservatively.
fn collect_instanceof_narrowings<'a>(expr: &'a Expression<'a>, out: &mut Vec<(String, String)>) {
    match expr {
        Expression::BinaryExpression(bin) if bin.operator == BinaryOperator::Instanceof => {
            if let Expression::Identifier(left) = &bin.left
                && let Expression::Identifier(right) = &bin.right
            {
                out.push((left.name.to_string(), right.name.to_string()));
            }
        }
        Expression::LogicalExpression(logical) if logical.operator == LogicalOperator::And => {
            collect_instanceof_narrowings(&logical.left, out);
            collect_instanceof_narrowings(&logical.right, out);
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_instanceof_narrowings(&paren.expression, out);
        }
        _ => {}
    }
}

impl ModuleInfoExtractor {
    fn risky_regex_fragment_for_expr(&self, expr: &Expression<'_>) -> Option<String> {
        match unwrap_static_expr(expr) {
            Expression::RegExpLiteral(lit) => risky_redos_fragment(&lit.regex.pattern.text),
            Expression::NewExpression(new_expr) => Self::risky_regex_fragment_for_new(new_expr),
            Expression::CallExpression(call) => Self::risky_regex_fragment_for_call(call),
            Expression::Identifier(ident) => self
                .risky_regex_binding(ident.name.as_str())
                .map(ToString::to_string),
            _ => None,
        }
    }

    fn risky_regex_fragment_for_new(expr: &oxc_ast::ast::NewExpression<'_>) -> Option<String> {
        let Expression::Identifier(callee) = &expr.callee else {
            return None;
        };
        if callee.name != "RegExp" {
            return None;
        }
        let pattern = expr
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(static_string_literal_value)?;
        risky_redos_fragment(&pattern)
    }

    fn risky_regex_fragment_for_call(expr: &CallExpression<'_>) -> Option<String> {
        let Expression::Identifier(callee) = &expr.callee else {
            return None;
        };
        if callee.name != "RegExp" {
            return None;
        }
        let pattern = expr
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(static_string_literal_value)?;
        risky_redos_fragment(&pattern)
    }

    fn redos_regex_application<'b, 'c>(
        &self,
        expr: &'b CallExpression<'c>,
    ) -> Option<(&'b Expression<'c>, String)> {
        let Expression::StaticMemberExpression(member) = &expr.callee else {
            return None;
        };
        let method = member.property.name.as_str();
        if matches!(method, "test" | "exec") {
            let input = expr.arguments.first().and_then(Argument::as_expression)?;
            let pattern = self.risky_regex_fragment_for_expr(&member.object)?;
            return Some((input, pattern));
        }
        if matches!(
            method,
            "match" | "search" | "replace" | "replaceAll" | "split"
        ) {
            let pattern = expr
                .arguments
                .first()
                .and_then(Argument::as_expression)
                .and_then(|arg| self.risky_regex_fragment_for_expr(arg))?;
            return Some((&member.object, pattern));
        }
        None
    }

    /// Record the statically flattenable callee path of a call site, deduped
    /// per unique path (first occurrence wins). Capture is unconditional
    /// because extraction is config-blind; the `boundaries.calls.forbidden`
    /// detector consumes these at analyze time. Computed members, dynamic
    /// dispatch, and optional-chaining callees flatten to `None` and stay
    /// uncaptured (documented false negatives).
    fn record_callee_use(&mut self, expr: &CallExpression<'_>) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            return;
        };
        if self.seen_callee_paths.insert(callee_path.clone()) {
            self.callee_uses.push(CalleeUse {
                callee_path,
                span_start: expr.span.start,
            });
        }
    }

    /// Push an HTML-template-sourced asset reference onto `imports`, mirroring
    /// the HTML parser's remote-url, normalization, and `SideEffect` pipeline.
    fn push_html_template_asset_import(&mut self, raw: &str) {
        let trimmed = raw.trim();
        if trimmed.is_empty() || is_remote_url(trimmed) {
            return;
        }
        self.imports.push(ImportInfo {
            source: normalize_asset_url(trimmed),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: false,
            span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        });
    }
}
