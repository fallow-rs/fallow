//! `Visit` trait implementation for `ModuleInfoExtractor`.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_semantic::ScopeFlags;
use oxc_span::{GetSpan, Span};
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use crate::{
    DynamicImportInfo, DynamicImportPattern, ExportInfo, ExportName, ImportInfo, ImportedName,
    MemberAccess, ReExportInfo, RequireCallInfo, VisibilityTag,
};
use fallow_types::extract::{
    CalleeUse, ClassHeritageInfo, LocalTypeDeclaration, MemberInfo, MemberKind,
    MisplacedDirectiveSite, PublicSignatureTypeReference, SanitizedSinkArg, SanitizerScope,
    SecurityControlKind, SecurityControlSite, SecurityUrlShape, SinkArgKind, SinkLiteralValue,
    SinkObjectProperty, SinkShape, SinkSite, SkippedSecurityCalleeExpressionKind,
    SkippedSecurityCalleeReason, SkippedSecurityCalleeSite, TaintedBinding,
};

use crate::asset_url::normalize_asset_url;
use crate::html::is_remote_url;

use super::helpers::{
    extract_angular_component_metadata, extract_angular_signal_query, extract_class_members,
    extract_concat_parts, extract_custom_elements_define, extract_implemented_interface_names,
    extract_nested_type_bindings, extract_query_list_element_type, extract_super_class_name,
    extract_type_annotation_name, has_angular_class_decorator, has_angular_plural_query_decorator,
    is_meta_url_arg, lit_custom_element_decorator, regex_pattern_to_suffix,
    ts_import_type_qualifier_root,
};
use super::{
    ModuleInfoExtractor, PendingLocalExportSpecifier, SecurityPathSinkBinding,
    SideEffectRegistrationTarget, try_extract_arrow_wrapped_import, try_extract_dynamic_import,
    try_extract_import_then_callback, try_extract_property_callback_import, try_extract_require,
};

#[path = "visit_impl_helpers.rs"]
mod visit_helpers;

use visit_helpers::*;

impl ModuleInfoExtractor {
    fn is_module_scope(&self) -> bool {
        self.block_depth == 0 && self.function_depth == 0 && self.namespace_depth == 0
    }

    fn is_module_or_function_runtime_scope(&self) -> bool {
        self.namespace_depth == 0
    }

    fn nested_scope_shadows(&self, name: &str) -> bool {
        self.nested_declaration_stack
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
    }

    fn record_sanitizer_binding(&mut self, name: &str, scope: Option<SanitizerScope>) {
        if self.is_module_scope() {
            self.module_sanitizer_bindings
                .insert(name.to_string(), scope);
            return;
        }
        if let Some(bindings) = self.sanitizer_binding_stack.last_mut() {
            bindings.insert(name.to_string(), scope);
        }
    }

    fn record_literal_allowlist_binding(&mut self, name: &str, trusted: bool) {
        if self.is_module_scope() {
            self.module_literal_allowlist_bindings
                .insert(name.to_string(), trusted);
            return;
        }
        if let Some(bindings) = self.literal_allowlist_binding_stack.last_mut() {
            bindings.insert(name.to_string(), trusted);
        }
    }

    fn literal_allowlist_binding(&self, name: &str) -> bool {
        for bindings in self.literal_allowlist_binding_stack.iter().rev() {
            if let Some(trusted) = bindings.get(name) {
                return *trusted;
            }
        }
        self.module_literal_allowlist_bindings
            .get(name)
            .copied()
            .unwrap_or(false)
    }

    fn record_static_sink_literal_binding(
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
            self.module_static_sink_literals.remove(id.name.as_str());
            return;
        }
        if let Some(value) = self.static_sink_literal_value(init) {
            self.module_static_sink_literals
                .insert(id.name.to_string(), value);
        } else {
            self.module_static_sink_literals.remove(id.name.as_str());
        }
    }

    fn static_sink_literal_value(&self, expr: &Expression<'_>) -> Option<SinkLiteralValue> {
        if let Some(value) = sink_literal_value(expr) {
            return Some(value);
        }
        match unwrap_static_expr(expr) {
            Expression::Identifier(ident) if !self.nested_scope_shadows(ident.name.as_str()) => {
                self.module_static_sink_literals
                    .get(ident.name.as_str())
                    .cloned()
            }
            Expression::UnaryExpression(unary)
                if unary.operator == UnaryOperator::UnaryNegation
                    || unary.operator == UnaryOperator::UnaryPlus =>
            {
                let SinkLiteralValue::Integer(value) =
                    self.static_sink_literal_value(&unary.argument)?
                else {
                    return None;
                };
                if unary.operator == UnaryOperator::UnaryNegation {
                    value.checked_neg().map(SinkLiteralValue::Integer)
                } else {
                    Some(SinkLiteralValue::Integer(value))
                }
            }
            Expression::CallExpression(call) => {
                let Expression::Identifier(callee) = &call.callee else {
                    return None;
                };
                if callee.name != "String"
                    || call.arguments.len() != 1
                    || self.nested_scope_shadows(callee.name.as_str())
                    || self.local_declaration_names.contains(callee.name.as_str())
                {
                    return None;
                }
                let arg = call.arguments.first()?.as_expression()?;
                Some(SinkLiteralValue::String(static_sink_literal_to_string(
                    &self.static_sink_literal_value(arg)?,
                )))
            }
            Expression::TemplateLiteral(template) => {
                let mut value = String::new();
                for (index, quasi) in template.quasis.iter().enumerate() {
                    value.push_str(quasi.value.cooked.as_ref()?);
                    if let Some(expression) = template.expressions.get(index) {
                        value.push_str(&static_sink_literal_to_string(
                            &self.static_sink_literal_value(expression)?,
                        ));
                    }
                }
                Some(SinkLiteralValue::String(value))
            }
            _ => None,
        }
    }

    fn record_risky_regex_binding(&mut self, name: &str, pattern: Option<String>) {
        if self.is_module_scope() {
            self.module_risky_regex_bindings
                .insert(name.to_string(), pattern);
            return;
        }
        if let Some(bindings) = self.risky_regex_binding_stack.last_mut() {
            bindings.insert(name.to_string(), pattern);
        }
    }

    fn risky_regex_binding(&self, name: &str) -> Option<&str> {
        for bindings in self.risky_regex_binding_stack.iter().rev() {
            if let Some(pattern) = bindings.get(name) {
                return pattern.as_deref();
            }
        }
        self.module_risky_regex_bindings
            .get(name)
            .and_then(Option::as_deref)
    }

    fn record_path_sink_binding(&mut self, name: &str, binding: Option<SecurityPathSinkBinding>) {
        if self.is_module_scope() {
            self.module_path_sink_bindings
                .insert(name.to_string(), binding);
            return;
        }
        if let Some(bindings) = self.path_sink_binding_stack.last_mut() {
            bindings.insert(name.to_string(), binding);
        }
    }

    fn path_sink_binding(&self, name: &str) -> Option<SecurityPathSinkBinding> {
        for bindings in self.path_sink_binding_stack.iter().rev() {
            if let Some(binding) = bindings.get(name) {
                return *binding;
            }
        }
        self.module_path_sink_bindings
            .get(name)
            .and_then(|binding| *binding)
    }

    fn record_path_relative_binding(&mut self, name: &str, target: Option<String>) {
        if self.is_module_scope() {
            self.module_path_relative_bindings
                .insert(name.to_string(), target);
            return;
        }
        if let Some(bindings) = self.path_relative_binding_stack.last_mut() {
            bindings.insert(name.to_string(), target);
        }
    }

    fn path_relative_binding(&self, name: &str) -> Option<&str> {
        for bindings in self.path_relative_binding_stack.iter().rev() {
            if let Some(target) = bindings.get(name) {
                return target.as_deref();
            }
        }
        self.module_path_relative_bindings
            .get(name)
            .and_then(Option::as_deref)
    }

    fn sanitizer_scope_for_identifier(&self, name: &str) -> Option<SanitizerScope> {
        for bindings in self.sanitizer_binding_stack.iter().rev() {
            if let Some(scope) = bindings.get(name) {
                return *scope;
            }
        }
        self.module_sanitizer_bindings
            .get(name)
            .and_then(|scope| *scope)
    }

    fn record_nested_declaration_names<'a>(
        &mut self,
        declarations: impl IntoIterator<Item = &'a BindingIdentifier<'a>>,
    ) {
        if self.namespace_depth > 0 {
            return;
        }
        let Some(scope) = self.nested_declaration_stack.last_mut() else {
            return;
        };
        scope.extend(declarations.into_iter().map(|id| id.name.to_string()));
    }

    fn push_function_declaration_scope(&mut self, params: &FormalParameters<'_>) {
        if self.namespace_depth > 0 {
            return;
        }

        let mut scope = FxHashSet::default();
        for param in &params.items {
            scope.extend(
                param
                    .pattern
                    .get_binding_identifiers()
                    .into_iter()
                    .map(|id| id.name.to_string()),
            );
        }
        let sanitizer_scope = scope
            .iter()
            .map(|name| (name.clone(), None))
            .collect::<FxHashMap<_, _>>();
        let allowlist_scope = scope
            .iter()
            .map(|name| (name.clone(), false))
            .collect::<FxHashMap<_, _>>();
        let risky_regex_scope = scope
            .iter()
            .map(|name| (name.clone(), None))
            .collect::<FxHashMap<_, _>>();
        let path_sink_scope = scope
            .iter()
            .map(|name| (name.clone(), None))
            .collect::<FxHashMap<_, _>>();
        let path_relative_scope = scope
            .iter()
            .map(|name| (name.clone(), None))
            .collect::<FxHashMap<_, _>>();
        self.nested_declaration_stack.push(scope);
        self.sanitizer_binding_stack.push(sanitizer_scope);
        self.literal_allowlist_binding_stack.push(allowlist_scope);
        self.risky_regex_binding_stack.push(risky_regex_scope);
        self.path_sink_binding_stack.push(path_sink_scope);
        self.path_relative_binding_stack.push(path_relative_scope);
    }

    fn pop_function_declaration_scope(&mut self) {
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.pop();
            self.sanitizer_binding_stack.pop();
            self.literal_allowlist_binding_stack.pop();
            self.risky_regex_binding_stack.pop();
            self.path_sink_binding_stack.pop();
            self.path_relative_binding_stack.pop();
        }
    }

    fn record_node_module_register_url_binding(&mut self, name: String, sources: Vec<String>) {
        let entry = self
            .node_module_register_url_bindings
            .entry(name)
            .or_default();
        for source in sources {
            if !entry.contains(&source) {
                entry.push(source);
            }
        }
    }

    fn node_module_register_url_binding(&self, name: &str) -> Vec<String> {
        self.node_module_register_url_bindings
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    fn record_local_type_declaration(&mut self, name: &str, span: Span) {
        if self
            .local_type_declarations
            .iter()
            .any(|decl| decl.name == name)
        {
            return;
        }
        self.local_type_declarations.push(LocalTypeDeclaration {
            name: name.to_string(),
            span,
        });
    }

    fn record_local_signature_refs(&mut self, owner_name: &str, refs: Vec<(String, Span)>) {
        self.local_signature_type_references
            .extend(
                refs.into_iter()
                    .map(|(type_name, span)| super::LocalSignatureTypeReference {
                        owner_name: owner_name.to_string(),
                        type_name,
                        span,
                    }),
            );
    }

    fn record_public_signature_refs(&mut self, export_name: &str, refs: Vec<(String, Span)>) {
        self.public_signature_type_references
            .extend(
                refs.into_iter()
                    .map(|(type_name, span)| PublicSignatureTypeReference {
                        export_name: export_name.to_string(),
                        type_name,
                        span,
                    }),
            );
    }

    fn collect_type_refs_from_annotation(annotation: &TSTypeAnnotation<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        collector.visit_ts_type_annotation(annotation);
        collector.refs
    }

    fn collect_function_signature_refs(function: &Function<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = function.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        if let Some(this_param) = function.this_param.as_deref() {
            collector.visit_ts_this_parameter(this_param);
        }
        for param in &function.params.items {
            if let Some(annotation) = param.type_annotation.as_deref() {
                collector.visit_ts_type_annotation(annotation);
            }
        }
        if let Some(rest) = function.params.rest.as_deref()
            && let Some(annotation) = rest.type_annotation.as_deref()
        {
            collector.visit_ts_type_annotation(annotation);
        }
        if let Some(return_type) = function.return_type.as_deref() {
            collector.visit_ts_type_annotation(return_type);
        }
        collector.refs
    }

    fn collect_arrow_signature_refs(arrow: &ArrowFunctionExpression<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = arrow.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        for param in &arrow.params.items {
            if let Some(annotation) = param.type_annotation.as_deref() {
                collector.visit_ts_type_annotation(annotation);
            }
        }
        if let Some(rest) = arrow.params.rest.as_deref()
            && let Some(annotation) = rest.type_annotation.as_deref()
        {
            collector.visit_ts_type_annotation(annotation);
        }
        if let Some(return_type) = arrow.return_type.as_deref() {
            collector.visit_ts_type_annotation(return_type);
        }
        collector.refs
    }

    fn collect_variable_signature_refs(declarator: &VariableDeclarator<'_>) -> Vec<(String, Span)> {
        let mut refs = Vec::new();
        if let Some(annotation) = declarator.type_annotation.as_deref() {
            refs.extend(Self::collect_type_refs_from_annotation(annotation));
        }
        if let Some(init) = &declarator.init {
            match init {
                Expression::ArrowFunctionExpression(arrow) => {
                    refs.extend(Self::collect_arrow_signature_refs(arrow));
                }
                Expression::FunctionExpression(function) => {
                    refs.extend(Self::collect_function_signature_refs(function));
                }
                _ => {}
            }
        }
        refs
    }

    fn collect_class_signature_refs(class: &Class<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = class.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        if let Some(super_class) = class.super_class.as_ref()
            && let Some((name, span)) = expression_root_name(super_class)
        {
            collector.refs.push((name, span));
        }
        if let Some(type_arguments) = class.super_type_arguments.as_deref() {
            collector.visit_ts_type_parameter_instantiation(type_arguments);
        }
        for implemented in &class.implements {
            if let Some((name, span)) = type_name_root(&implemented.expression) {
                collector.refs.push((name, span));
            }
            if let Some(type_arguments) = implemented.type_arguments.as_deref() {
                collector.visit_ts_type_parameter_instantiation(type_arguments);
            }
        }
        for element in &class.body.body {
            match element {
                ClassElement::MethodDefinition(method) => {
                    if matches!(method.accessibility, Some(TSAccessibility::Private))
                        || is_private_member_key(&method.key)
                    {
                        continue;
                    }
                    collector
                        .refs
                        .extend(Self::collect_function_signature_refs(&method.value));
                }
                ClassElement::PropertyDefinition(prop) => {
                    if matches!(prop.accessibility, Some(TSAccessibility::Private))
                        || is_private_member_key(&prop.key)
                    {
                        continue;
                    }
                    if let Some(annotation) = prop.type_annotation.as_deref() {
                        collector.visit_ts_type_annotation(annotation);
                    }
                }
                ClassElement::AccessorProperty(prop) => {
                    if matches!(prop.accessibility, Some(TSAccessibility::Private))
                        || is_private_member_key(&prop.key)
                    {
                        continue;
                    }
                    if let Some(annotation) = prop.type_annotation.as_deref() {
                        collector.visit_ts_type_annotation(annotation);
                    }
                }
                ClassElement::TSIndexSignature(index) => {
                    collector.visit_ts_index_signature(index);
                }
                ClassElement::StaticBlock(_) => {}
            }
        }
        collector.refs
    }

    fn collect_interface_signature_refs(iface: &TSInterfaceDeclaration<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = iface.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        for heritage in &iface.extends {
            if let Some((name, span)) = expression_root_name(&heritage.expression) {
                collector.refs.push((name, span));
            }
            if let Some(type_arguments) = heritage.type_arguments.as_deref() {
                collector.visit_ts_type_parameter_instantiation(type_arguments);
            }
        }
        collector.visit_ts_interface_body(&iface.body);
        collector.refs
    }

    fn collect_type_alias_signature_refs(
        alias: &TSTypeAliasDeclaration<'_>,
    ) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = alias.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        collector.visit_ts_type(&alias.type_annotation);
        collector.refs
    }

    fn record_typed_binding(&mut self, binding_name: &str, type_annotation: &TSTypeAnnotation<'_>) {
        if let Some(type_name) = extract_type_annotation_name(type_annotation)
            && let Some(resolved) = self.resolve_class_type_param(&type_name)
        {
            self.binding_target_names
                .insert(binding_name.to_string(), resolved);
        }

        for (property_path, type_name) in extract_nested_type_bindings(type_annotation) {
            let Some(resolved) = self.resolve_class_type_param(&type_name) else {
                continue;
            };
            self.binding_target_names
                .insert(format!("{binding_name}.{property_path}"), resolved);
        }
    }

    /// Record destructured bindings with type annotations.
    fn record_typed_destructure_binding(
        &mut self,
        pattern: &ObjectPattern<'_>,
        type_annotation: &TSTypeAnnotation<'_>,
    ) {
        let bindings = extract_object_pattern_bindings(pattern);
        if bindings.is_empty() {
            return;
        }
        if let TSType::TSTypeLiteral(type_lit) = &type_annotation.type_annotation {
            let properties = collect_object_type_property_types(&type_lit.members);
            for (local, key) in bindings {
                if let Some(class_name) = properties.get(&key) {
                    self.binding_target_names
                        .entry(local)
                        .or_insert_with(|| class_name.clone());
                }
            }
        } else if let Some(type_name) = extract_type_annotation_name(type_annotation) {
            for (local, key) in bindings {
                self.pending_typed_destructures
                    .push((local, key, type_name.clone()));
            }
        }
    }

    fn record_local_structural_function(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        body: Option<&FunctionBody<'_>>,
    ) {
        let Some(body) = body else {
            return;
        };
        let typed_params: Vec<(usize, String, String)> = params
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                    return None;
                };
                let type_annotation = param.type_annotation.as_deref()?;
                let type_name = extract_type_annotation_name(type_annotation)?;
                Some((index, id.name.to_string(), type_name))
            })
            .collect();
        if typed_params.is_empty() {
            return;
        }

        let target_params = typed_params
            .iter()
            .map(|(_, param_name, _)| param_name.clone())
            .collect();
        let mut collector = StructuralParamMemberCollector::new(target_params);
        collector.visit_function_body(body);

        let mut function = super::LocalStructuralFunction::default();
        for (index, param_name, type_name) in typed_params {
            let Some(members) = collector.members.remove(param_name.as_str()) else {
                continue;
            };
            if members.is_empty() {
                continue;
            }
            function
                .params
                .insert(index, super::StructuralParameterUse { type_name, members });
        }

        if !function.params.is_empty() {
            self.local_structural_functions
                .insert(name.to_string(), function);
        }
    }

    fn structural_call_argument(arg: &Argument<'_>) -> Option<super::StructuralCallArgument> {
        let expr = arg.as_expression()?;
        match expr {
            Expression::NewExpression(new_expr) => {
                let Expression::Identifier(callee) = &new_expr.callee else {
                    return None;
                };
                if super::helpers::is_builtin_constructor(callee.name.as_str()) {
                    return None;
                }
                Some(super::StructuralCallArgument::DirectClass(
                    callee.name.to_string(),
                ))
            }
            Expression::Identifier(ident) => Some(super::StructuralCallArgument::Binding(
                ident.name.to_string(),
            )),
            _ => None,
        }
    }

    fn record_structural_class_call_candidate(&mut self, call: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };

        let arguments: Vec<Option<super::StructuralCallArgument>> = call
            .arguments
            .iter()
            .map(Self::structural_call_argument)
            .collect();
        if arguments.iter().all(Option::is_none) {
            return;
        }

        self.structural_class_call_candidates
            .push(super::StructuralClassCallCandidate {
                callee_name: callee.name.to_string(),
                arguments,
            });
    }

    fn record_local_structural_function_from_variable_declarator(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                self.record_local_structural_function(
                    id.name.as_str(),
                    &arrow.params,
                    Some(arrow.body.as_ref()),
                );
            }
            Expression::FunctionExpression(function) => {
                self.record_local_structural_function(
                    id.name.as_str(),
                    &function.params,
                    function.body.as_deref(),
                );
            }
            _ => {}
        }
    }

    /// Record `const TOKEN = new InjectionToken<Interface>(...)` declarations
    /// so the analyze layer can follow the token's interface type argument to
    /// the classes that `implement` it. Gated on `InjectionToken` being a named
    /// import from `@angular/core` (a same-named local class is ignored). A
    /// token with no type argument carries no interface and is skipped. See
    /// issue #920.
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
        let Some(type_arguments) = new_expr.type_arguments.as_deref() else {
            return;
        };
        let Some(TSType::TSTypeReference(type_ref)) = type_arguments.params.first() else {
            return;
        };
        if let Some((interface_name, _)) = type_name_root(&type_ref.type_name) {
            self.injection_tokens
                .push((name.to_string(), interface_name));
        }
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

    /// Emit a fluent-chain sentinel `MemberAccess` for chained calls.
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
                let chain_prefix = chain_prefix_reversed.join(",");
                self.member_accesses.push(MemberAccess {
                    object: format!(
                        "{}{}:{}:{}",
                        crate::FLUENT_CHAIN_SENTINEL,
                        root_id.name,
                        inner_member.property.name,
                        chain_prefix,
                    ),
                    member: this_method.to_string(),
                });
                return;
            }
            if let Expression::NewExpression(new_expr) = &inner_member.object
                && let Expression::Identifier(class_id) = &new_expr.callee
            {
                chain_prefix_reversed.push(inner_member.property.name.to_string());
                chain_prefix_reversed.reverse();
                let chain_prefix = chain_prefix_reversed.join(",");
                self.member_accesses.push(MemberAccess {
                    object: format!(
                        "{}{}:{}",
                        crate::FLUENT_CHAIN_NEW_SENTINEL,
                        class_id.name,
                        chain_prefix,
                    ),
                    member: this_method.to_string(),
                });
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
        if method_name.as_str() != "forEach" {
            return;
        }
        let Some(receiver_name) = static_member_object_name(receiver_expr) else {
            return;
        };
        let Some(element_type) = self.iterable_element_types.get(&receiver_name).cloned() else {
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
            self.binding_target_names.insert(name, element_type);
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

    fn is_pino_factory_binding(&self, local_name: &str) -> bool {
        let imported = self.imports.iter().any(|import| {
            import.source == PINO_PACKAGE
                && import.local_name == local_name
                && !import.is_type_only
                && match &import.imported_name {
                    ImportedName::Default => true,
                    ImportedName::Named(name) => name == PINO_FACTORY_EXPORT,
                    ImportedName::Namespace | ImportedName::SideEffect => false,
                }
        });
        let required = self.require_calls.iter().any(|require| {
            require.source == PINO_PACKAGE
                && require.local_name.as_deref() == Some(local_name)
                && require.destructured_names.is_empty()
        });
        (imported || required) && !self.nested_scope_shadows(local_name)
    }

    fn try_record_pino_transport_targets(&mut self, expr: &CallExpression<'_>) {
        let Some(local_name) = pino_factory_callee_name(&expr.callee) else {
            return;
        };
        if !self.is_pino_factory_binding(&local_name) {
            return;
        }

        let Some(config) = expr.arguments.first().and_then(Argument::as_expression) else {
            return;
        };

        let mut targets = Vec::new();
        collect_pino_config_targets(config, &mut targets);
        for source in targets.into_iter().filter(|source| !source.is_empty()) {
            self.dynamic_imports.push(DynamicImportInfo {
                source,
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
                is_speculative: false,
            });
        }
    }

    /// Record `register('loader', ...)` from `node:module` as a dynamic import.
    fn try_record_node_module_register(&mut self, expr: &CallExpression<'_>) {
        let register_match = match &expr.callee {
            Expression::Identifier(ident) => {
                self.is_node_module_register(ident.name.as_str(), false)
            }
            Expression::StaticMemberExpression(member) => {
                member.property.name == "register"
                    && matches!(&member.object, Expression::Identifier(obj)
                        if self.is_node_module_register(obj.name.as_str(), true))
            }
            _ => false,
        };
        if !register_match {
            return;
        }

        let sources = self.node_module_register_sources(expr);
        for source in sources.into_iter().filter(|source| !source.is_empty()) {
            let destructured_names = loader_hook_exports_for_source(&source);
            self.dynamic_imports.push(DynamicImportInfo {
                source,
                span: expr.span,
                destructured_names,
                local_name: None,
                is_speculative: false,
            });
        }
    }

    fn node_module_register_sources(&self, call: &CallExpression<'_>) -> Vec<String> {
        if let Some(source) = node_module_register_specifier(call) {
            return vec![source];
        }

        let Some(first_arg) = call.arguments.first() else {
            return Vec::new();
        };
        first_arg
            .as_expression()
            .map(|expr| self.node_module_register_sources_from_expression(expr))
            .unwrap_or_default()
    }

    fn node_module_register_sources_from_expression(&self, expr: &Expression<'_>) -> Vec<String> {
        match expr {
            Expression::Identifier(ident) => {
                self.node_module_register_url_binding(ident.name.as_str())
            }
            Expression::NewExpression(new_expr) => {
                new_url_import_source(new_expr).into_iter().collect()
            }
            Expression::ConditionalExpression(conditional) => {
                let mut sources =
                    self.node_module_register_sources_from_expression(&conditional.consequent);
                sources.extend(
                    self.node_module_register_sources_from_expression(&conditional.alternate),
                );
                sources.sort();
                sources.dedup();
                sources
            }
            Expression::ParenthesizedExpression(paren) => {
                self.node_module_register_sources_from_expression(&paren.expression)
            }
            Expression::TSAsExpression(ts_as) => {
                self.node_module_register_sources_from_expression(&ts_as.expression)
            }
            Expression::TSSatisfiesExpression(ts_sat) => {
                self.node_module_register_sources_from_expression(&ts_sat.expression)
            }
            _ => Vec::new(),
        }
    }

    fn record_child_process_require_binding(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        source: &str,
    ) {
        if !self.is_module_scope() {
            return;
        }

        match &declarator.id {
            BindingPattern::BindingIdentifier(id) if is_child_process_source(source) => {
                self.child_process_namespace_bindings
                    .insert(id.name.to_string());
            }
            BindingPattern::ObjectPattern(obj_pat) if is_child_process_source(source) => {
                for (local_name, source_name) in extract_object_pattern_bindings(obj_pat) {
                    if source_name == "fork" {
                        self.child_process_fork_bindings.insert(local_name);
                    }
                }
            }
            BindingPattern::BindingIdentifier(id) if is_node_path_source(source) => {
                self.node_path_namespace_bindings
                    .insert(id.name.to_string());
            }
            BindingPattern::ObjectPattern(obj_pat) if is_node_url_source(source) => {
                for (local_name, source_name) in extract_object_pattern_bindings(obj_pat) {
                    if source_name == "fileURLToPath" {
                        self.node_url_file_url_to_path_bindings.insert(local_name);
                    }
                }
            }
            _ => {}
        }
    }

    fn record_current_module_file_path_binding(&mut self, name: &str, expr: &Expression<'_>) {
        if !self.is_module_scope() {
            return;
        }
        let Expression::CallExpression(call) = expr else {
            return;
        };
        let Some(first_arg) = call.arguments.first() else {
            return;
        };
        if !is_meta_url_arg(first_arg) {
            return;
        }

        let is_file_url_to_path = match &call.callee {
            Expression::Identifier(ident) => self
                .node_url_file_url_to_path_bindings
                .contains(ident.name.as_str()),
            Expression::StaticMemberExpression(member) => {
                member.property.name == "fileURLToPath"
                    && matches!(&member.object, Expression::Identifier(obj)
                        if self.node_url_file_url_to_path_bindings.contains(obj.name.as_str()))
            }
            _ => false,
        };

        if is_file_url_to_path {
            self.current_module_file_path_bindings
                .insert(name.to_string());
        }
    }

    /// Record a tainted binding together with its chain-hop depth, deduped on
    /// `(local, source_path)`. On a duplicate the existing record keeps its
    /// span (first read wins, matching the analyze layer's name-keyed
    /// `or_insert`) and the SHALLOWER hop depth, so a direct source capture
    /// (hop 1) is never demoted past the chain cap by a chained re-record of
    /// the same pair, and a chained record is never inflated by a later one.
    fn push_tainted_binding(&mut self, binding: TaintedBinding, hop: u8) {
        debug_assert_eq!(
            self.tainted_bindings.len(),
            self.tainted_binding_hops.len(),
            "tainted_binding_hops must stay aligned with tainted_bindings"
        );
        if let Some(index) = self
            .tainted_bindings
            .iter()
            .position(|b| b.local == binding.local && b.source_path == binding.source_path)
        {
            self.tainted_binding_hops[index] = self.tainted_binding_hops[index].min(hop);
            return;
        }
        self.tainted_bindings.push(binding);
        self.tainted_binding_hops.push(hop);
    }

    /// Record tainted-source bindings for `const <name> = <object>.<prop>` and
    /// for one-hop local expressions that embed a source member access. The
    /// recorded candidates include the exact member path and the flattened
    /// object path, so `const id = req.query.id` still records `req.query`
    /// while leaf sources such as `const ref = document.referrer` can match
    /// exact source rows.
    /// Captured at any scope (no `is_module_scope` gate): a sink inside a route
    /// handler reading a function-local source is exactly the target case.
    fn record_tainted_source_binding(&mut self, name: &str, expr: &Expression<'_>) {
        for source_path in binding_source_path_candidates(expr) {
            self.push_tainted_binding(
                TaintedBinding {
                    local: name.to_string(),
                    source_path,
                    // The initializer member-access read (`req.query.id`); anchors
                    // the taint trace's source node at the real read line.
                    source_span_start: expr.span().start,
                },
                DIRECT_TAINT_HOP,
            );
        }
    }

    fn record_tainted_param_binding(&mut self, name: &str, source_path: &'static str) {
        self.push_tainted_binding(
            TaintedBinding {
                local: name.to_string(),
                source_path: source_path.to_string(),
                // Synthetic framework-handler-param source (e.g. `framework.request`);
                // no concrete member-access read expression, so the analyze layer
                // anchors at the sink rather than a spurious line. `0` signals "no
                // captured read span".
                source_span_start: 0,
            },
            DIRECT_TAINT_HOP,
        );
    }

    /// Chain a declarator's taint from already-tainted local bindings that its
    /// initializer references through the conservative #1095 expression shapes
    /// plus a bare-identifier alias (issue #1146). Each chained binding carries
    /// the ORIGINAL `source_path` and `source_span_start`, so the analyze layer
    /// needs no changes: the chained local matches `source_tainted_locals` and
    /// anchors its trace at the original source read. Chains only form in
    /// lexical declarator order within one extractor walk; use-before-declaration
    /// flows and cross-SFC-block chains are silent false negatives by design
    /// (FN-preferring, matching the #885 doctrine).
    fn record_chained_tainted_binding(&mut self, name: &str, init: &Expression<'_>) {
        let mut idents = Vec::new();
        collect_chained_taint_idents(init, &mut idents);
        if idents.is_empty() {
            return;
        }
        let chained = self.chained_bindings_for_idents(name, &idents);
        for (binding, hop) in chained {
            self.push_tainted_binding(binding, hop);
        }
    }

    /// Chained `(binding, hop)` records for a new local named `name` whose
    /// initializer references `idents`. Referenced bindings already at
    /// `MAX_TAINT_BINDING_HOPS` contribute nothing, so an over-cap chain is
    /// simply not recorded and the sink degrades to module-level reachability;
    /// a false arg-level claim past the cap is structurally impossible.
    fn chained_bindings_for_idents(
        &self,
        name: &str,
        idents: &[String],
    ) -> Vec<(TaintedBinding, u8)> {
        let mut out = Vec::new();
        for ident in idents {
            // No `nested_scope_shadows` guard on the referenced ident: tainted
            // bindings are recorded scope-insensitively and name-keyed (the
            // direct recorders do the same), and the chain root is normally a
            // local declared in the SAME function body, which the nested
            // declaration stack already contains. Guarding on it would reject
            // the common `const a = req.query.id; const b = ...a...` case
            // inside a route handler. Cross-scope name collisions are the
            // accepted Risk 1, bounded by the hop cap.
            if ident == name {
                continue;
            }
            for (index, binding) in self.tainted_bindings.iter().enumerate() {
                if binding.local != *ident {
                    continue;
                }
                let hop = self.tainted_binding_hops[index];
                if hop >= MAX_TAINT_BINDING_HOPS {
                    tracing::debug!(
                        local = name,
                        via = ident.as_str(),
                        source_path = binding.source_path.as_str(),
                        "taint binding chain dropped: would exceed MAX_TAINT_BINDING_HOPS"
                    );
                    continue;
                }
                out.push((
                    TaintedBinding {
                        local: name.to_string(),
                        source_path: binding.source_path.clone(),
                        source_span_start: binding.source_span_start,
                    },
                    hop + 1,
                ));
            }
        }
        out
    }

    /// Chain an object destructure from a tainted local (`const { id } = a`
    /// where `a` is source-tainted), mirroring the shipped
    /// destructure-from-source capture (issue #1146). Only a bare-identifier
    /// initializer chains; member-expression initializers stay out for the
    /// same reason member roots are excluded from
    /// `collect_chained_taint_idents`.
    fn record_chained_tainted_destructure_bindings(
        &mut self,
        obj_pat: &ObjectPattern<'_>,
        init: &Expression<'_>,
    ) {
        let Expression::Identifier(ident) = unwrap_parens(init) else {
            return;
        };
        let idents = vec![ident.name.to_string()];
        let mut chained = Vec::new();
        for local in super::extract_destructured_names(obj_pat) {
            chained.extend(self.chained_bindings_for_idents(&local, &idents));
        }
        for (binding, hop) in chained {
            self.push_tainted_binding(binding, hop);
        }
    }

    fn record_first_param_source(
        &mut self,
        params: &FormalParameters<'_>,
        source_path: &'static str,
    ) {
        self.record_param_source_at_index(params, 0, source_path);
    }

    fn record_param_source_at_index(
        &mut self,
        params: &FormalParameters<'_>,
        index: usize,
        source_path: &'static str,
    ) {
        let Some(param) = params.items.get(index) else {
            return;
        };
        match &param.pattern {
            BindingPattern::BindingIdentifier(id) => {
                self.record_tainted_param_binding(id.name.as_str(), source_path);
            }
            BindingPattern::ObjectPattern(obj_pat) => {
                for local in super::extract_destructured_names(obj_pat) {
                    self.record_tainted_param_binding(&local, source_path);
                }
            }
            _ => {}
        }
    }

    fn record_named_param_source(
        &mut self,
        params: &FormalParameters<'_>,
        names: &[&str],
        source_path: &'static str,
    ) {
        for param in &params.items {
            match &param.pattern {
                BindingPattern::BindingIdentifier(id)
                    if names.iter().any(|name| *name == id.name.as_str()) =>
                {
                    self.record_tainted_param_binding(id.name.as_str(), source_path);
                }
                BindingPattern::ObjectPattern(obj_pat) => {
                    for (local, key) in extract_object_pattern_bindings(obj_pat) {
                        if names
                            .iter()
                            .any(|name| key == *name || key.starts_with(&format!("{name}.")))
                        {
                            self.record_tainted_param_binding(&local, source_path);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn record_graphql_resolver_args_source(&mut self, expr: &Expression<'_>) {
        let Some(params) = function_like_params(expr) else {
            return;
        };
        let Some(param) = params.items.get(1) else {
            return;
        };
        match &param.pattern {
            BindingPattern::BindingIdentifier(id) if id.name == "args" => {
                self.record_param_source_at_index(params, 1, GRAPHQL_ARGS_SOURCE);
            }
            BindingPattern::ObjectPattern(_) => {
                self.record_param_source_at_index(params, 1, GRAPHQL_ARGS_SOURCE);
            }
            _ => {}
        }
    }

    fn record_next_function_param_sources(&mut self, func: &Function<'_>) {
        if func
            .id
            .as_ref()
            .is_some_and(|id| is_http_route_handler_name(id.name.as_str()))
        {
            self.record_first_param_source(&func.params, NEXT_REQUEST_SOURCE);
        }
        if function_body_has_use_server(func.body.as_deref()) {
            self.record_named_param_source(&func.params, &["formData"], NEXT_FORM_DATA_SOURCE);
        }
    }

    fn record_next_arrow_param_sources(&mut self, expr: &ArrowFunctionExpression<'_>) {
        if function_body_has_use_server(Some(&expr.body)) {
            self.record_named_param_source(&expr.params, &["formData"], NEXT_FORM_DATA_SOURCE);
        }
    }

    fn record_framework_callback_param_sources(&mut self, call: &CallExpression<'_>) {
        let callee_path = flatten_callee_path(&call.callee);
        let Some(method) = callee_method_name(&call.callee, callee_path.as_deref()) else {
            return;
        };
        if is_route_registration_method(method) {
            let Some(callee_path) = callee_path.as_deref() else {
                return;
            };
            if !is_framework_route_receiver_path(callee_path, method) {
                return;
            }
            if let Some(params) = route_callback_params(&call.arguments, method) {
                self.record_first_param_source(params, FRAMEWORK_REQUEST_SOURCE);
            }
            return;
        }
        if method == "process"
            && let Some(params) = last_callback_params(&call.arguments)
        {
            self.record_first_param_source(params, QUEUE_JOB_SOURCE);
            return;
        }
        if method == "tool"
            && let Some(params) = last_callback_params(&call.arguments)
        {
            self.record_first_param_source(params, MCP_TOOL_INPUT_SOURCE);
            return;
        }
        if is_trpc_procedure_method(method)
            && is_trpc_procedure_callee(&call.callee, method)
            && let Some(params) = last_callback_params(&call.arguments)
        {
            self.record_named_param_source(params, &["input"], TRPC_INPUT_SOURCE);
        }
    }

    fn record_queue_worker_constructor_param_sources(&mut self, expr: &NewExpression<'_>) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            return;
        };
        if callee_path.rsplit('.').next() != Some("Worker") {
            return;
        }
        if let Some(params) = expr.arguments.iter().skip(1).find_map(callback_params) {
            self.record_first_param_source(params, QUEUE_JOB_SOURCE);
        }
    }

    fn record_tainted_helper_call_binding(&mut self, name: &str, expr: &Expression<'_>) {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if self.nested_scope_shadows(callee.name.as_str()) {
            return;
        }
        let Some(helper) = self
            .source_returning_helpers
            .get(callee.name.as_str())
            .cloned()
        else {
            return;
        };

        let mut source_paths = Vec::new();
        for path in &helper.paths {
            let Some(arg_expr) = call
                .arguments
                .get(path.arg_index)
                .and_then(Argument::as_expression)
            else {
                continue;
            };
            source_paths.extend(apply_source_return_path(arg_expr, &path.suffixes));
        }
        source_paths.sort();
        source_paths.dedup();
        for source_path in source_paths {
            self.push_tainted_binding(
                TaintedBinding {
                    local: name.to_string(),
                    source_path,
                    // One-hop helper-return source: the real read lives inside the
                    // helper body, not at this binding, so no concrete read span is
                    // available here. `0` makes the analyze layer anchor at the sink.
                    source_span_start: 0,
                },
                DIRECT_TAINT_HOP,
            );
        }
    }

    fn record_source_returning_function_helper(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        body: &oxc_ast::ast::FunctionBody<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let Some(expr) = extract_function_body_final_return_expr(body) else {
            self.source_returning_helpers.remove(name);
            return;
        };
        if let Some(helper) = source_returning_helper(params, expr) {
            self.source_returning_helpers
                .insert(name.to_string(), helper);
        } else {
            self.source_returning_helpers.remove(name);
        }
    }

    fn record_source_returning_function_declaration(&mut self, function: &Function<'_>) {
        let (Some(id), Some(body)) = (function.id.as_ref(), function.body.as_deref()) else {
            return;
        };
        self.record_source_returning_function_helper(id.name.as_str(), &function.params, body);
    }

    fn record_sanitizer_function_helper(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        expr: &Expression<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        if let Some(scope) = sanitizer_scope_for_local_helper(self, params, expr) {
            self.module_sanitizer_helpers
                .insert(name.to_string(), scope);
        } else {
            self.module_sanitizer_helpers.remove(name);
        }
    }

    fn record_sanitizer_function_declaration(&mut self, function: &Function<'_>) {
        let (Some(id), Some(body)) = (function.id.as_ref(), function.body.as_deref()) else {
            return;
        };
        let Some(expr) = extract_function_body_final_return_expr(body) else {
            self.module_sanitizer_helpers.remove(id.name.as_str());
            return;
        };
        self.record_sanitizer_function_helper(id.name.as_str(), &function.params, expr);
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

    fn record_sanitizer_helper_from_variable_declarator(
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
            self.module_sanitizer_helpers.remove(id.name.as_str());
            return;
        }
        let helper = match init {
            Expression::ArrowFunctionExpression(arrow) => {
                extract_arrow_return_expr(arrow).map(|expr| (&arrow.params, expr))
            }
            Expression::FunctionExpression(function) => function
                .body
                .as_deref()
                .and_then(extract_function_body_final_return_expr)
                .map(|expr| (&function.params, expr)),
            _ => None,
        };
        if let Some((params, expr)) = helper {
            self.record_sanitizer_function_helper(id.name.as_str(), params, expr);
        } else {
            self.module_sanitizer_helpers.remove(id.name.as_str());
        }
    }

    fn record_dompurify_import_binding(&mut self, source: &str, local: &str, is_type_only: bool) {
        if !is_type_only && self.is_module_scope() && is_dompurify_source(source) {
            self.dompurify_bindings.insert(local.to_string());
        }
    }

    fn record_dompurify_require_binding(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        source: &str,
    ) {
        if !self.is_module_scope() || !is_dompurify_source(source) {
            return;
        }
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            self.dompurify_bindings.insert(id.name.to_string());
        }
    }

    fn sanitizer_scope_for_expr(&self, expr: &Expression<'_>) -> Option<SanitizerScope> {
        match unwrap_parens(expr) {
            Expression::Identifier(ident) => self.sanitizer_scope_for_identifier(&ident.name),
            Expression::AwaitExpression(await_expr) => {
                self.sanitizer_scope_for_expr(&await_expr.argument)
            }
            Expression::CallExpression(call) if self.is_dompurify_sanitize_call(call) => {
                Some(SanitizerScope::Html)
            }
            Expression::CallExpression(call) => self.sanitizer_helper_scope_for_call(call),
            Expression::ObjectExpression(obj) => self.sanitizer_scope_for_object(obj),
            Expression::TemplateLiteral(template) if self.html_template_is_sanitized(template) => {
                Some(SanitizerScope::Html)
            }
            Expression::TemplateLiteral(template)
                if self.sql_identifier_template_is_sanitized(template) =>
            {
                Some(SanitizerScope::SqlIdentifier)
            }
            _ => None,
        }
    }

    fn sanitizer_scope_for_object(&self, obj: &ObjectExpression<'_>) -> Option<SanitizerScope> {
        obj.properties.iter().find_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return None;
            };
            if prop.key.static_name().is_none_or(|name| name != "__html") {
                return None;
            }
            self.sanitizer_scope_for_expr(&prop.value)
        })
    }

    fn is_dompurify_sanitize_call(&self, call: &CallExpression<'_>) -> bool {
        let Some(callee_path) = flatten_callee_path(&call.callee) else {
            return false;
        };
        let Some((object, method)) = callee_path.rsplit_once('.') else {
            return false;
        };
        method == "sanitize"
            && self.dompurify_bindings.contains(object)
            && !self.nested_scope_shadows(object)
    }

    fn sanitizer_helper_scope_for_call(&self, call: &CallExpression<'_>) -> Option<SanitizerScope> {
        let Expression::Identifier(callee) = unwrap_parens(&call.callee) else {
            return None;
        };
        if self.nested_scope_shadows(&callee.name) || call.arguments.is_empty() {
            return None;
        }
        self.module_sanitizer_helpers
            .get(callee.name.as_str())
            .copied()
    }

    fn html_template_is_sanitized(&self, template: &TemplateLiteral<'_>) -> bool {
        !template.expressions.is_empty()
            && template
                .expressions
                .iter()
                .enumerate()
                .all(|(index, expr)| {
                    html_template_interpolation_is_text(template, index)
                        && self.sanitizer_scope_for_expr(expr) == Some(SanitizerScope::Html)
                })
    }

    fn sql_identifier_template_is_sanitized(&self, template: &TemplateLiteral<'_>) -> bool {
        !template.expressions.is_empty()
            && template
                .expressions
                .iter()
                .enumerate()
                .all(|(index, expr)| {
                    let Some(raw) = template.quasis.get(index).map(template_raw) else {
                        return false;
                    };
                    sql_identifier_template_context(raw)
                        && self.sanitizer_scope_for_expr(expr)
                            == Some(SanitizerScope::SqlIdentifier)
                })
    }

    fn record_sanitized_sink_arg(
        &mut self,
        span_start: u32,
        arg_index: u32,
        expr: &Expression<'_>,
    ) {
        let Some(scope) = self.sanitizer_scope_for_expr(expr) else {
            return;
        };
        self.sanitized_sink_args.push(SanitizedSinkArg {
            span_start,
            arg_index,
            scope,
        });
        self.security_control_sites.push(SecurityControlSite {
            kind: SecurityControlKind::Sanitization,
            callee_path: "sanitized-sink-argument".to_string(),
            span_start,
            span_end: span_start,
        });
    }

    fn record_guarded_path_sink_arg(&mut self, local: &str) {
        let Some(binding) = self.path_sink_binding(local) else {
            return;
        };
        self.sanitized_sink_args.push(SanitizedSinkArg {
            span_start: binding.span_start,
            arg_index: binding.arg_index,
            scope: SanitizerScope::Path,
        });
    }

    fn record_fail_closed_guard_after_statement(&mut self, stmt: &Statement<'_>) {
        let Statement::IfStatement(if_stmt) = stmt else {
            return;
        };
        if if_stmt.alternate.is_some() || !statement_exits_current_flow(&if_stmt.consequent) {
            return;
        }
        if let Some(target) = self.url_allowlist_guard_target(&if_stmt.test) {
            self.record_sanitizer_binding(&target, Some(SanitizerScope::Url));
        }
        if let Some(target) = self.path_containment_guard_target(&if_stmt.test) {
            self.record_sanitizer_binding(&target, Some(SanitizerScope::Path));
            self.record_guarded_path_sink_arg(&target);
        }
    }

    fn url_allowlist_guard_target(&self, expr: &Expression<'_>) -> Option<String> {
        let Expression::UnaryExpression(unary) = unwrap_parens(expr) else {
            return None;
        };
        if unary.operator != UnaryOperator::LogicalNot {
            return None;
        }
        let Expression::CallExpression(call) = unwrap_parens(&unary.argument) else {
            return None;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        if !matches!(member.property.name.as_str(), "has" | "includes") {
            return None;
        }
        let Expression::Identifier(allowlist) = &member.object else {
            return None;
        };
        if !self.literal_allowlist_binding(&allowlist.name) {
            return None;
        }
        let Some(Argument::Identifier(target)) = call.arguments.first() else {
            return None;
        };
        Some(target.name.to_string())
    }

    fn path_containment_guard_target(&self, expr: &Expression<'_>) -> Option<String> {
        let Expression::LogicalExpression(logical) = unwrap_parens(expr) else {
            return None;
        };
        if logical.operator != LogicalOperator::Or {
            return None;
        }
        let left = path_relative_starts_with_parent(&logical.left)
            .or_else(|| self.path_is_absolute_relative_arg(&logical.left));
        let right = path_relative_starts_with_parent(&logical.right)
            .or_else(|| self.path_is_absolute_relative_arg(&logical.right));
        let (Some(left), Some(right)) = (left, right) else {
            return None;
        };
        if left != right {
            return None;
        }
        self.path_relative_binding(left).map(str::to_string)
    }

    fn path_is_absolute_relative_arg<'b>(&self, expr: &'b Expression<'_>) -> Option<&'b str> {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return None;
        };
        if !self.is_node_path_method_call(call, "isAbsolute") {
            return None;
        }
        let Some(Argument::Identifier(rel)) = call.arguments.first() else {
            return None;
        };
        Some(rel.name.as_str())
    }

    fn is_node_path_method_call(&self, call: &CallExpression<'_>, method: &str) -> bool {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        if member.property.name != method {
            return false;
        }
        let Expression::Identifier(object) = &member.object else {
            return false;
        };
        self.node_path_namespace_bindings
            .contains(object.name.as_str())
            && !self.nested_scope_shadows(object.name.as_str())
    }

    fn path_sink_binding_for_expr(&self, expr: &Expression<'_>) -> Option<SecurityPathSinkBinding> {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return None;
        };
        if !["join", "normalize", "resolve"]
            .iter()
            .any(|method| self.is_node_path_method_call(call, method))
        {
            return None;
        }
        Some(SecurityPathSinkBinding {
            span_start: call.span.start,
            arg_index: 0,
        })
    }

    fn path_relative_target_for_expr(&self, expr: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return None;
        };
        if !self.is_node_path_method_call(call, "relative") {
            return None;
        }
        let Some(Argument::Identifier(target)) = call.arguments.get(1) else {
            return None;
        };
        Some(target.name.to_string())
    }

    /// Record tainted-source bindings for `const { a, b } = <object>.<prop>`,
    /// where the destructured initializer is a member-access chain (or bare
    /// identifier root). Each bound local maps to the FULL flattened init path:
    /// `const { id } = req.query` records `{ local: "id", source_path:
    /// "req.query" }`. Rest patterns are skipped (whole-object capture is out of
    /// the cheap scope). Nested patterns are not destructured.
    fn record_tainted_destructure_bindings(
        &mut self,
        obj_pat: &ObjectPattern<'_>,
        expr: &Expression<'_>,
    ) {
        let Some(source_path) = destructure_source_path(expr) else {
            return;
        };
        for local in super::extract_destructured_names(obj_pat) {
            self.push_tainted_binding(
                TaintedBinding {
                    local,
                    source_path: source_path.clone(),
                    // The destructured initializer read (`req.query` in
                    // `const { id } = req.query`); anchors the source node at the
                    // real read line.
                    source_span_start: expr.span().start,
                },
                DIRECT_TAINT_HOP,
            );
        }
    }

    fn record_child_process_fork_target_binding(&mut self, name: &str, expr: &Expression<'_>) {
        if !self.is_module_scope() {
            return;
        }
        let sources = self.child_process_fork_sources_from_expression(expr);
        if !sources.is_empty() {
            self.child_process_fork_target_bindings
                .insert(name.to_string(), sources);
        }
    }

    fn child_process_fork_sources_from_expression(&self, expr: &Expression<'_>) -> Vec<String> {
        match expr {
            Expression::StringLiteral(lit) => local_fork_source(&lit.value)
                .into_iter()
                .collect::<Vec<_>>(),
            Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => tpl
                .quasis
                .first()
                .and_then(|quasi| local_fork_source(&quasi.value.raw))
                .into_iter()
                .collect(),
            Expression::Identifier(ident) => self
                .child_process_fork_target_bindings
                .get(ident.name.as_str())
                .filter(|_| !self.nested_scope_shadows(ident.name.as_str()))
                .cloned()
                .unwrap_or_default(),
            Expression::NewExpression(new_expr) => new_url_import_source(new_expr)
                .and_then(|source| local_fork_source(&source))
                .into_iter()
                .collect(),
            Expression::CallExpression(call) => self.child_process_fork_sources_from_call(call),
            Expression::ParenthesizedExpression(paren) => {
                self.child_process_fork_sources_from_expression(&paren.expression)
            }
            Expression::TSAsExpression(ts_as) => {
                self.child_process_fork_sources_from_expression(&ts_as.expression)
            }
            Expression::TSSatisfiesExpression(ts_sat) => {
                self.child_process_fork_sources_from_expression(&ts_sat.expression)
            }
            _ => Vec::new(),
        }
    }

    fn child_process_fork_sources_from_call(&self, call: &CallExpression<'_>) -> Vec<String> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Vec::new();
        };
        if member.property.name != "resolve" {
            return Vec::new();
        }
        let Expression::Identifier(object) = &member.object else {
            return Vec::new();
        };
        if !self
            .node_path_namespace_bindings
            .contains(object.name.as_str())
        {
            return Vec::new();
        }
        let Some(Argument::Identifier(base)) = call.arguments.first() else {
            return Vec::new();
        };
        if !self
            .current_module_file_path_bindings
            .contains(base.name.as_str())
        {
            return Vec::new();
        }
        let Some(Argument::StringLiteral(relative)) = call.arguments.get(1) else {
            return Vec::new();
        };
        normalize_module_file_relative_path(&relative.value)
            .and_then(|source| local_fork_source(&source))
            .into_iter()
            .collect()
    }

    fn try_record_child_process_fork(&mut self, expr: &CallExpression<'_>) {
        if !self.is_module_or_function_runtime_scope() {
            return;
        }

        let is_fork_call = match &expr.callee {
            Expression::Identifier(ident) => {
                self.child_process_fork_bindings
                    .contains(ident.name.as_str())
                    && !self.nested_scope_shadows(ident.name.as_str())
            }
            Expression::StaticMemberExpression(member) => {
                member.property.name == "fork"
                    && matches!(&member.object, Expression::Identifier(obj)
                        if self.child_process_namespace_bindings.contains(obj.name.as_str())
                            && !self.nested_scope_shadows(obj.name.as_str()))
            }
            _ => false,
        };
        if !is_fork_call {
            return;
        }

        let Some(first_arg) = expr.arguments.first().and_then(Argument::as_expression) else {
            return;
        };
        for source in self.child_process_fork_sources_from_expression(first_arg) {
            self.dynamic_imports.push(DynamicImportInfo {
                source,
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
                is_speculative: false,
            });
        }
    }

    fn extract_angular_inject_target(&self, call: &CallExpression<'_>) -> Option<String> {
        super::helpers::extract_angular_inject_target(call, &|local_name, source, imported_name| {
            self.is_named_import_from(local_name, source, imported_name)
        })
    }

    fn copy_nested_binding_targets(&mut self, source_binding: &str, target_binding: &str) -> bool {
        let source_prefix = format!("{source_binding}.");
        let target_prefix = format!("{target_binding}.");
        let copied: Vec<(String, String)> = self
            .binding_target_names
            .iter()
            .filter_map(|(binding, target)| {
                binding
                    .strip_prefix(&source_prefix)
                    .map(|suffix| (format!("{target_prefix}{suffix}"), target.clone()))
            })
            .collect();

        let mut changed = false;
        for (binding, target) in copied {
            changed |= self.insert_binding_target(binding, target);
        }
        changed
    }

    fn insert_binding_target(&mut self, binding: String, target: String) -> bool {
        if self.binding_target_names.get(&binding) == Some(&target) {
            return false;
        }
        self.binding_target_names.insert(binding, target);
        true
    }

    pub(super) fn resolve_object_binding_candidate(
        &mut self,
        candidate: &super::ObjectBindingCandidate,
    ) -> bool {
        let mut changed = false;
        if self
            .namespace_binding_names
            .iter()
            .any(|name| name == candidate.source_name.as_str())
        {
            changed |= self.insert_binding_target(
                candidate.binding_path.clone(),
                candidate.source_name.clone(),
            );
        } else if let Some(target_name) = self
            .binding_target_names
            .get(candidate.source_name.as_str())
            .cloned()
        {
            changed |= self.insert_binding_target(candidate.binding_path.clone(), target_name);
        }
        changed | self.copy_nested_binding_targets(&candidate.source_name, &candidate.binding_path)
    }

    fn record_object_binding_targets(&mut self, binding_name: &str, obj: &ObjectExpression<'_>) {
        self.record_object_binding_targets_at_path(binding_name, obj);
    }

    fn record_object_binding_targets_at_path(
        &mut self,
        object_path: &str,
        obj: &ObjectExpression<'_>,
    ) {
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                continue;
            };
            let Some(key_name) = prop.key.static_name() else {
                continue;
            };

            let binding_path = format!("{object_path}.{key_name}");
            match &prop.value {
                Expression::Identifier(ident) => {
                    self.object_binding_candidates
                        .push(super::ObjectBindingCandidate {
                            binding_path,
                            source_name: ident.name.to_string(),
                        });
                }
                Expression::ObjectExpression(child) => {
                    self.record_object_binding_targets_at_path(&binding_path, child);
                }
                _ => {}
            }
        }
    }

    fn record_static_package_values(&mut self, name: &str, init: &Expression<'_>) {
        match init {
            Expression::StringLiteral(lit) => {
                self.static_string_bindings
                    .insert(name.to_string(), lit.value.to_string());
            }
            Expression::ArrayExpression(array) => {
                let values: Vec<String> = array
                    .elements
                    .iter()
                    .filter_map(|element| match element {
                        ArrayExpressionElement::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    })
                    .collect();
                if !values.is_empty() {
                    self.static_string_arrays.insert(name.to_string(), values);
                }
            }
            Expression::ObjectExpression(obj) => {
                let values = static_object_string_property_values(obj);
                if !values.is_empty() {
                    self.static_object_property_values
                        .insert(name.to_string(), values);
                }
            }
            _ => {}
        }
    }

    fn try_record_package_path_reference(&mut self, call: &CallExpression<'_>) {
        if is_require_resolve_callee(&call.callee)
            && let Some(arg) = call.arguments.first()
        {
            let references = self.package_references_from_argument(arg);
            self.push_package_path_references(references);
        }

        if let Expression::Identifier(callee) = &call.callee
            && let Some(arg_index) = self
                .package_resolution_function_args
                .get(callee.name.as_str())
                .copied()
            && let Some(arg) = call.arguments.get(arg_index)
        {
            let references = self.package_references_from_argument(arg);
            self.push_package_path_references(references);
        }
    }

    fn push_package_path_references(&mut self, references: Vec<String>) {
        for package_name in references {
            if !self.package_path_references.contains(&package_name) {
                self.package_path_references.push(package_name);
            }
        }
    }

    fn package_references_from_argument(&self, arg: &Argument<'_>) -> Vec<String> {
        match arg {
            Argument::StringLiteral(lit) => package_from_resolution_specifier(lit.value.as_str())
                .into_iter()
                .collect(),
            Argument::TemplateLiteral(tpl) => self.package_references_from_template(tpl),
            Argument::Identifier(ident) => self.package_values_for_identifier(&ident.name),
            Argument::StaticMemberExpression(member) => {
                self.package_values_for_static_member(member)
            }
            _ => arg.as_expression().map_or_else(Vec::new, |expr| {
                self.package_references_from_expression(expr)
            }),
        }
    }

    fn package_references_from_expression(&self, expr: &Expression<'_>) -> Vec<String> {
        match expr {
            Expression::StringLiteral(lit) => package_from_resolution_specifier(lit.value.as_str())
                .into_iter()
                .collect(),
            Expression::TemplateLiteral(tpl) => self.package_references_from_template(tpl),
            Expression::Identifier(ident) => self.package_values_for_identifier(&ident.name),
            Expression::StaticMemberExpression(member) => {
                self.package_values_for_static_member(member)
            }
            _ => Vec::new(),
        }
    }

    fn package_references_from_template(&self, tpl: &TemplateLiteral<'_>) -> Vec<String> {
        if tpl.expressions.is_empty() {
            return tpl
                .quasis
                .first()
                .and_then(|quasi| package_from_resolution_specifier(quasi.value.raw.as_str()))
                .into_iter()
                .collect();
        }

        if tpl.expressions.len() != 1 || tpl.quasis.len() != 2 {
            return Vec::new();
        }

        let Some(first) = tpl.quasis.first() else {
            return Vec::new();
        };
        let Some(last) = tpl.quasis.last() else {
            return Vec::new();
        };
        if !first.value.raw.is_empty() || last.value.raw.as_str() != "/package.json" {
            return Vec::new();
        }

        self.package_references_from_expression(&tpl.expressions[0])
    }

    fn package_values_for_identifier(&self, name: &str) -> Vec<String> {
        for scope in self.loop_string_bindings.iter().rev() {
            if let Some(values) = scope.get(name) {
                return package_values_from_raw_values(values);
            }
        }

        self.static_string_bindings
            .get(name)
            .map_or_else(Vec::new, |value| {
                package_from_resolution_specifier(value)
                    .into_iter()
                    .collect()
            })
    }

    fn package_values_for_static_member(&self, member: &StaticMemberExpression<'_>) -> Vec<String> {
        let Expression::Identifier(object) = &member.object else {
            return Vec::new();
        };
        let property = member.property.name.as_str();

        for scope in self.loop_object_property_values.iter().rev() {
            if let Some(properties) = scope.get(object.name.as_str())
                && let Some(values) = properties.get(property)
            {
                return package_values_from_raw_values(values);
            }
        }

        self.static_object_property_values
            .get(object.name.as_str())
            .and_then(|properties| properties.get(property))
            .map_or_else(Vec::new, |values| package_values_from_raw_values(values))
    }

    fn static_package_loop_bindings(
        &self,
        stmt: &ForOfStatement<'_>,
    ) -> Option<StaticPackageLoopBindings> {
        let loop_name = for_of_binding_name(&stmt.left)?;
        let mut strings = FxHashMap::default();
        let mut objects = FxHashMap::default();

        if let Expression::Identifier(iterable) = &stmt.right
            && let Some(values) = self.static_string_arrays.get(iterable.name.as_str())
        {
            strings.insert(loop_name.clone(), values.clone());
        }

        if let Some(object_name) = object_values_or_entries_argument_name(&stmt.right)
            && let Some(properties) = self.static_object_property_values.get(&object_name)
        {
            objects.insert(loop_name, properties.clone());
        }

        (!strings.is_empty() || !objects.is_empty()).then_some((strings, objects))
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

    fn collect_playwright_fixture_type_bindings(&self, ty: &TSType<'_>) -> Vec<(String, String)> {
        let mut bindings = Vec::new();
        collect_fixture_type_bindings_from_type(
            ty,
            "",
            &self.playwright_fixture_types,
            &mut bindings,
        );
        bindings.sort_unstable();
        bindings.dedup();
        bindings
    }

    fn record_playwright_fixture_type_alias(&mut self, alias: &TSTypeAliasDeclaration<'_>) {
        let bindings = self.collect_playwright_fixture_type_bindings(&alias.type_annotation);
        if !bindings.is_empty() {
            self.playwright_fixture_types
                .insert(alias.id.name.to_string(), bindings.clone());
            self.member_accesses
                .extend(
                    bindings
                        .into_iter()
                        .map(|(fixture_name, type_name)| MemberAccess {
                            object: format!(
                                "{}{}:{}",
                                crate::PLAYWRIGHT_FIXTURE_TYPE_SENTINEL,
                                alias.id.name,
                                fixture_name
                            ),
                            member: type_name,
                        }),
                );
        }
    }

    fn record_playwright_fixture_definitions(
        &mut self,
        test_name: &str,
        call: &CallExpression<'_>,
    ) {
        let Some(base_name) = playwright_extend_base_name(call) else {
            return;
        };
        if !self.is_named_import_from(base_name.as_str(), "@playwright/test", "test") {
            return;
        }
        let Some(type_arguments) = call.type_arguments.as_deref() else {
            return;
        };
        let mut bindings = Vec::new();
        for type_arg in &type_arguments.params {
            bindings.extend(self.collect_playwright_fixture_type_bindings(type_arg));
        }
        bindings.sort_unstable();
        bindings.dedup();
        self.member_accesses
            .extend(
                bindings
                    .into_iter()
                    .map(|(fixture_name, type_name)| MemberAccess {
                        object: format!(
                            "{}{}:{}",
                            crate::PLAYWRIGHT_FIXTURE_DEF_SENTINEL,
                            test_name,
                            fixture_name
                        ),
                        member: type_name,
                    }),
            );
    }

    fn record_playwright_fixture_alias(&mut self, test_name: &str, base_name: &str) {
        self.member_accesses.push(MemberAccess {
            object: format!("{}{}:", crate::PLAYWRIGHT_FIXTURE_ALIAS_SENTINEL, test_name),
            member: base_name.to_string(),
        });
    }

    fn record_playwright_wrapper_aliases(&mut self, test_name: &str, call: &CallExpression<'_>) {
        if let Some(base_name) = playwright_extend_base_name(call) {
            if !self.is_named_import_from(base_name.as_str(), "@playwright/test", "test") {
                self.record_playwright_fixture_alias(test_name, &base_name);
            }
            return;
        }

        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if !self.is_named_import_from(callee.name.as_str(), "@playwright/test", "mergeTests") {
            return;
        }

        let mut base_names: Vec<String> = call
            .arguments
            .iter()
            .filter_map(|argument| match argument {
                Argument::Identifier(ident) => Some(ident.name.to_string()),
                _ => None,
            })
            .collect();
        base_names.sort();
        base_names.dedup();
        for base_name in base_names {
            self.record_playwright_fixture_alias(test_name, &base_name);
        }
    }

    /// Capture helper-function Playwright fixtures or aliases from returns.
    pub(super) fn try_capture_playwright_factory_helper(
        &mut self,
        test_name: &str,
        call: &CallExpression<'_>,
    ) {
        if let Some(base_name) = playwright_extend_base_name(call) {
            let Some(type_arguments) = call.type_arguments.as_deref() else {
                return;
            };
            let mut bindings = Vec::new();
            for type_arg in &type_arguments.params {
                bindings.extend(self.collect_playwright_fixture_type_bindings(type_arg));
            }
            bindings.sort_unstable();
            bindings.dedup();
            if bindings.is_empty() {
                return;
            }
            self.pending_playwright_factory_calls
                .push(super::PendingPlaywrightFactory {
                    test_name: test_name.to_string(),
                    base_name,
                    type_bindings: bindings,
                });
        } else if let Expression::Identifier(ident) = &call.callee {
            self.pending_playwright_factory_aliases
                .push((test_name.to_string(), ident.name.to_string()));
        }
    }

    fn record_vitest_mock_dynamic_imports(&mut self, expr: &CallExpression<'_>) {
        let Some(target_source) = vitest_mock_source(expr) else {
            return;
        };

        self.dynamic_imports.push(DynamicImportInfo {
            source: target_source.clone(),
            span: expr.span,
            destructured_names: Vec::new(),
            local_name: None,
            is_speculative: false,
        });

        if !vi_mock_has_factory(expr)
            && let Some(mock_source) = vitest_auto_mock_source(&target_source)
        {
            self.dynamic_imports.push(DynamicImportInfo {
                source: mock_source,
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: Some(String::new()),
                is_speculative: true,
            });
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
            self.whole_object_uses.push(arg_name);
        }
    }

    fn push_relative_dynamic_import_pattern(&mut self, prefix: String, span: Span) {
        if prefix.starts_with("./") || prefix.starts_with("../") {
            self.dynamic_import_patterns.push(DynamicImportPattern {
                prefix,
                suffix: None,
                span,
            });
        }
    }

    fn record_import_meta_glob_patterns(&mut self, expr: &CallExpression<'_>) {
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && member.property.name == "glob"
            && matches!(member.object, Expression::MetaProperty(_))
            && let Some(first_arg) = expr.arguments.first()
        {
            match first_arg {
                Argument::StringLiteral(lit) => {
                    self.push_relative_dynamic_import_pattern(lit.value.to_string(), expr.span);
                }
                Argument::ArrayExpression(arr) => {
                    for elem in &arr.elements {
                        if let ArrayExpressionElement::StringLiteral(lit) = elem {
                            self.push_relative_dynamic_import_pattern(
                                lit.value.to_string(),
                                expr.span,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn record_require_context_pattern(&mut self, expr: &CallExpression<'_>) {
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && member.property.name == "context"
            && let Expression::Identifier(obj) = &member.object
            && obj.name == "require"
            && let Some(Argument::StringLiteral(dir_lit)) = expr.arguments.first()
        {
            let dir = dir_lit.value.to_string();
            if dir.starts_with("./") || dir.starts_with("../") {
                let recursive = expr
                    .arguments
                    .get(1)
                    .is_some_and(|arg| matches!(arg, Argument::BooleanLiteral(b) if b.value));
                let prefix = if recursive {
                    format!("{dir}/**/")
                } else {
                    format!("{dir}/")
                };
                let suffix = expr.arguments.get(2).and_then(|arg| match arg {
                    Argument::RegExpLiteral(re) => regex_pattern_to_suffix(&re.regex.pattern.text),
                    _ => None,
                });
                self.dynamic_import_patterns.push(DynamicImportPattern {
                    prefix,
                    suffix,
                    span: expr.span,
                });
            }
        }
    }

    fn record_import_callback_dynamic_imports(&mut self, expr: &CallExpression<'_>) {
        if let Some(then_cb) = try_extract_import_then_callback(expr) {
            if let Some(local) = &then_cb.local_name {
                self.namespace_binding_names.push(local.clone());
            }
            self.handled_import_spans.insert(then_cb.import_span);
            self.dynamic_imports.push(DynamicImportInfo {
                source: then_cb.source,
                span: then_cb.import_span,
                destructured_names: then_cb.destructured_names,
                local_name: then_cb.local_name,
                is_speculative: false,
            });
        }
    }

    fn record_arrow_wrapped_dynamic_import(&mut self, expr: &CallExpression<'_>) {
        if let Some((import_expr, source)) = try_extract_arrow_wrapped_import(&expr.arguments) {
            self.dynamic_imports.push(DynamicImportInfo {
                source: source.to_string(),
                span: import_expr.span,
                destructured_names: vec!["default".to_string()],
                local_name: None,
                is_speculative: false,
            });
            self.handled_import_spans.insert(import_expr.span);

            // Record the `import()` span when this is a
            // `next/dynamic(() => import('./X'), { ssr: false })` call. ssr:false
            // is Next.js's sanctioned client-only escape hatch, so the security
            // `client-server-leak` BFS must NOT treat a server-only module reached
            // only through it as a leak. The span lets the BFS exclude exactly
            // this edge. Recorded conservatively (next/dynamic provenance AND a
            // literal `ssr: false`) so a real leak is never hidden by mistake.
            if self.is_next_dynamic_ssr_false_call(expr) {
                self.client_only_dynamic_import_spans
                    .push(import_expr.span.start);
            }
        }
    }

    /// Whether `expr` is a `next/dynamic(callback, { ssr: false })` call: the
    /// callee is the local binding of the `next/dynamic` default import, and the
    /// second argument is an object literal with a literal `ssr: false` property.
    fn is_next_dynamic_ssr_false_call(&self, expr: &CallExpression<'_>) -> bool {
        let Expression::Identifier(callee) = &expr.callee else {
            return false;
        };
        if !self.is_default_import_from(&callee.name, "next/dynamic") {
            return false;
        }
        let Some(Argument::ObjectExpression(options)) = expr.arguments.get(1) else {
            return false;
        };
        options.properties.iter().any(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return false;
            };
            prop.key.static_name().as_deref() == Some("ssr")
                && matches!(&prop.value, Expression::BooleanLiteral(lit) if !lit.value)
        })
    }

    /// Whether `local_name` is bound to the default import of `source`
    /// (`import dynamic from "next/dynamic"`, or an aliased default).
    pub(super) fn is_default_import_from(&self, local_name: &str, source: &str) -> bool {
        self.imports.iter().any(|import| {
            import.source == source
                && import.local_name == local_name
                && matches!(import.imported_name, ImportedName::Default)
        })
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
        let refs = Self::collect_type_alias_signature_refs(alias);
        self.record_local_signature_refs(&alias.id.name, refs);
        if let TSType::TSTypeLiteral(type_lit) = &alias.type_annotation {
            let properties = collect_object_type_property_types(&type_lit.members);
            if !properties.is_empty() {
                self.interface_property_types
                    .insert(alias.id.name.to_string(), properties);
            }
        }
    }

    fn record_top_level_interface_declaration(&mut self, iface: &TSInterfaceDeclaration<'_>) {
        self.record_local_declaration_name(&iface.id.name);
        self.record_local_type_declaration(&iface.id.name, iface.id.span);
        let refs = Self::collect_interface_signature_refs(iface);
        self.record_local_signature_refs(&iface.id.name, refs);
        let properties = collect_object_type_property_types(&iface.body.body);
        if !properties.is_empty() {
            self.interface_property_types
                .insert(iface.id.name.to_string(), properties);
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

impl<'a> Visit<'a> for ModuleInfoExtractor {
    fn visit_program(&mut self, program: &Program<'a>) {
        // Capture file-level string directives (`"use client"`, `"use server"`)
        // for the security client-server-leak detector. `directive.directive` is
        // the cooked directive text without surrounding quotes.
        for directive in &program.directives {
            self.directives
                .push(directive.directive.as_str().to_string());
        }
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
                // Detect MISPLACED `"use client"` / `"use server"`
                // directives. oxc places honored leading-prologue directives in
                // `program.directives` (handled above), so any string-literal
                // expression statement here is by definition NOT in the leading
                // position: a non-directive statement (an import, a const)
                // preceded it and the RSC bundler parses the string as an
                // ordinary expression, silently ignoring it. Match ONLY the two
                // RSC directive strings; a stray `"use strict"` is harmless.
                Statement::ExpressionStatement(stmt) => {
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
                _ => {}
            }
        }
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

        walk::walk_formal_parameter(self, param);
    }

    fn visit_property_definition(&mut self, prop: &PropertyDefinition<'a>) {
        if let Some(name) = prop.key.static_name() {
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
                self.binding_target_names
                    .insert(format!("this.{name}"), callee.name.to_string());
            }

            if let Some(Expression::CallExpression(call)) = &prop.value
                && let Some(type_name) = self.extract_angular_inject_target(call)
            {
                self.binding_target_names
                    .insert(format!("this.{name}"), type_name);
            }

            if let Some(value) = prop.value.as_ref()
                && let Some(query) = extract_angular_signal_query(value)
            {
                let call_key = format!("this.{name}()");
                if query.plural {
                    self.iterable_element_types.insert(call_key, query.type_arg);
                } else {
                    self.binding_target_names.insert(call_key, query.type_arg);
                }
            }

            if let Some(value) = prop.value.as_ref() {
                self.capture_hardcoded_secret_literal_sink(name.as_ref(), value, prop.span);
            }
        }

        walk::walk_property_definition(self, prop);
    }

    fn visit_block_statement(&mut self, stmt: &BlockStatement<'a>) {
        self.block_depth += 1;
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.push(FxHashSet::default());
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
        walk::walk_function(self, func, flags);
        self.function_depth -= 1;
        self.pop_function_declaration_scope();
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        self.record_next_arrow_param_sources(expr);
        self.push_function_declaration_scope(&expr.params);
        self.function_depth += 1;
        walk::walk_arrow_function_expression(self, expr);
        self.function_depth -= 1;
        self.pop_function_declaration_scope();
    }

    fn visit_function_body(&mut self, body: &FunctionBody<'a>) {
        for statement in &body.statements {
            self.visit_statement(statement);
            if self.namespace_depth == 0 {
                self.record_fail_closed_guard_after_statement(statement);
            }
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
                        if self.is_module_scope()
                            && is_child_process_source(&source)
                            && s.imported.name() == "fork"
                        {
                            self.child_process_fork_bindings
                                .insert(s.local.name.to_string());
                        }
                        if self.is_module_scope()
                            && is_node_url_source(&source)
                            && s.imported.name() == "fileURLToPath"
                        {
                            self.node_url_file_url_to_path_bindings
                                .insert(s.local.name.to_string());
                        }
                        self.imports.push(ImportInfo {
                            source: source.clone(),
                            imported_name: ImportedName::Named(s.imported.name().to_string()),
                            local_name: s.local.name.to_string(),
                            is_type_only: is_type_only || s.import_kind.is_type(),
                            from_style: false,
                            span: s.span,
                            source_span,
                        });
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                        self.record_dompurify_import_binding(
                            &source,
                            s.local.name.as_str(),
                            is_type_only,
                        );
                        if self.is_module_scope() && is_node_path_source(&source) {
                            self.node_path_namespace_bindings
                                .insert(s.local.name.to_string());
                        }
                        self.imports.push(ImportInfo {
                            source: source.clone(),
                            imported_name: ImportedName::Default,
                            local_name: s.local.name.to_string(),
                            is_type_only,
                            from_style: false,
                            span: s.span,
                            source_span,
                        });
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        let local = s.local.name.to_string();
                        self.record_dompurify_import_binding(&source, &local, is_type_only);
                        if self.is_module_scope() && is_child_process_source(&source) {
                            self.child_process_namespace_bindings.insert(local.clone());
                        }
                        if self.is_module_scope() && is_node_path_source(&source) {
                            self.node_path_namespace_bindings.insert(local.clone());
                        }
                        if self.is_module_scope() && is_node_url_source(&source) {
                            self.node_url_file_url_to_path_bindings
                                .insert(local.clone());
                        }
                        self.namespace_binding_names.push(local.clone());
                        self.imports.push(ImportInfo {
                            source: source.clone(),
                            imported_name: ImportedName::Namespace,
                            local_name: local,
                            is_type_only,
                            from_style: false,
                            span: s.span,
                            source_span,
                        });
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
            for spec in &decl.specifiers {
                self.re_exports.push(ReExportInfo {
                    source: source.value.to_string(),
                    imported_name: spec.local.name().to_string(),
                    exported_name: spec.exported.name().to_string(),
                    is_type_only: is_type_only || spec.export_kind.is_type(),
                    span: spec.span,
                });
            }
        } else {
            if let Some(declaration) = &decl.declaration {
                self.extract_declaration_exports(declaration, is_type_only);
            }
            for spec in &decl.specifiers {
                let local_name_str = spec.local.name().as_str();
                let spec_type_only = is_type_only || spec.export_kind.is_type();

                self.pending_local_export_specifiers
                    .push(PendingLocalExportSpecifier {
                        local_name: local_name_str.to_string(),
                        exported_name: spec.exported.name().to_string(),
                        is_type_only: spec_type_only,
                        span: spec.span,
                    });
            }
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

        match &expr.source {
            Expression::StringLiteral(lit) => {
                self.dynamic_imports.push(DynamicImportInfo {
                    source: lit.value.to_string(),
                    span: expr.span,
                    destructured_names: Vec::new(),
                    local_name: None,
                    is_speculative: false,
                });
            }
            Expression::TemplateLiteral(tpl)
                if !tpl.quasis.is_empty() && !tpl.expressions.is_empty() =>
            {
                let first_quasi = tpl.quasis[0].value.raw.to_string();
                if first_quasi.starts_with("./") || first_quasi.starts_with("../") {
                    let prefix = if tpl.expressions.len() > 1 {
                        format!("{first_quasi}**/")
                    } else {
                        first_quasi
                    };
                    let suffix = if tpl.quasis.len() > 1 {
                        let last = &tpl.quasis[tpl.quasis.len() - 1];
                        let s = last.value.raw.to_string();
                        if s.is_empty() { None } else { Some(s) }
                    } else {
                        None
                    };
                    self.dynamic_import_patterns.push(DynamicImportPattern {
                        prefix,
                        suffix,
                        span: expr.span,
                    });
                }
            }
            Expression::TemplateLiteral(tpl)
                if !tpl.quasis.is_empty() && tpl.expressions.is_empty() =>
            {
                let value = tpl.quasis[0].value.raw.to_string();
                if !value.is_empty() {
                    self.dynamic_imports.push(DynamicImportInfo {
                        source: value,
                        span: expr.span,
                        destructured_names: Vec::new(),
                        local_name: None,
                        is_speculative: false,
                    });
                }
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

        walk::walk_import_expression(self, expr);
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        for declarator in &decl.declarations {
            self.record_variable_declarator_metadata(declarator);

            let Some(init) = &declarator.init else {
                self.record_uninitialized_variable_bindings(declarator);
                continue;
            };

            self.record_initialized_variable_bindings(decl, declarator, init);
            self.record_playwright_variable_helpers(declarator, init);
            self.record_pinia_store(declarator, init);

            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && let Expression::ObjectExpression(obj) = init
            {
                self.record_object_binding_targets(id.name.as_str(), obj);
            }

            if let Some((call, source)) = try_extract_require(init) {
                self.record_dompurify_require_binding(declarator, source);
                self.record_child_process_require_binding(declarator, source);
                self.handle_require_declaration(declarator, call, source);
                continue;
            }

            if let Expression::NewExpression(new_expr) = init
                && let Expression::Identifier(callee) = &new_expr.callee
                && let BindingPattern::BindingIdentifier(id) = &declarator.id
                && !super::helpers::is_builtin_constructor(callee.name.as_str())
            {
                self.binding_target_names
                    .insert(id.name.to_string(), callee.name.to_string());
            }

            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && let Some(class_name) = Self::svelte_derived_new_class(init)
            {
                self.binding_target_names
                    .insert(id.name.to_string(), class_name);
            }

            if let Expression::CallExpression(call) = init
                && let BindingPattern::ArrayPattern(arr_pat) = &declarator.id
                && let Some(Some(BindingPattern::BindingIdentifier(id))) = arr_pat.elements.first()
                && let Some(class_name) =
                    super::helpers::try_extract_factory_new_class(&call.arguments)
            {
                self.binding_target_names
                    .insert(id.name.to_string(), class_name);
            }

            // `const svc = useMemo(() => new Svc())`: useMemo returns the
            // factory's product directly, so the non-destructured binding is a
            // class instance. Scoped to useMemo (see `is_value_returning_memo_callee`)
            // so arbitrary wrappers and tuple-returning hooks like useState are
            // not over-credited. `or_insert` so a stronger pre-existing binding
            // wins. See issue #844.
            if let Expression::CallExpression(call) = init
                && let BindingPattern::BindingIdentifier(id) = &declarator.id
                && is_value_returning_memo_callee(&call.callee)
                && let Some(class_name) =
                    super::helpers::try_extract_factory_new_class(&call.arguments)
            {
                self.binding_target_names
                    .entry(id.name.to_string())
                    .or_insert(class_name);
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

            if let Expression::Identifier(ident) = init
                && self
                    .namespace_binding_names
                    .iter()
                    .any(|n| n == ident.name.as_str())
            {
                self.handle_namespace_destructuring(declarator, &ident.name);
                continue;
            }

            let Some((import_expr, source)) = try_extract_dynamic_import(init) else {
                continue;
            };
            self.handle_dynamic_import_declaration(declarator, import_expr, source);
        }
        walk::walk_variable_declaration(self, decl);
    }

    fn visit_object_property(&mut self, prop: &ObjectProperty<'a>) {
        self.record_graphql_resolver_args_source(&prop.value);

        if let Some((import_expr, source)) = try_extract_property_callback_import(prop) {
            self.dynamic_imports.push(DynamicImportInfo {
                source: source.to_string(),
                span: import_expr.span,
                destructured_names: vec!["default".to_string()],
                local_name: None,
                is_speculative: false,
            });
            self.handled_import_spans.insert(import_expr.span);
        }

        if let Some(name) = prop.key.static_name() {
            self.capture_hardcoded_secret_literal_sink(name.as_ref(), &prop.value, prop.span);
        }

        walk::walk_object_property(self, prop);
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        self.record_structural_class_call_candidate(expr);
        self.clear_literal_allowlist_on_mutating_member_call(expr);
        self.record_framework_callback_param_sources(expr);

        if let Some(test_name) = playwright_test_callee_name(&expr.callee) {
            self.member_accesses
                .extend(collect_playwright_fixture_member_uses(
                    test_name.as_str(),
                    &expr.arguments,
                ));
        }

        if let Some((_tag, class_name)) = extract_custom_elements_define(expr) {
            self.side_effect_registered_class_names.insert(class_name);
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

        self.capture_security_call_sites(expr);

        self.record_callee_use(expr);

        walk::walk_call_expression(self, expr);
    }

    fn visit_for_of_statement(&mut self, stmt: &ForOfStatement<'a>) {
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

    #[expect(
        clippy::excessive_nesting,
        reason = "CJS export pattern matching requires deep nesting"
    )]
    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        if let Some(name) = assignment_target_security_context_name(&expr.left) {
            self.capture_math_random_context_sink(name.as_str(), &expr.right, expr.span);
            self.capture_hardcoded_secret_literal_sink(name.as_str(), &expr.right, expr.span);
        }

        if let Some(name) = assignment_target_identifier_name(&expr.left) {
            self.record_sanitizer_binding(name, None);
            self.record_literal_allowlist_binding(name, false);
            self.record_risky_regex_binding(name, None);
            self.record_path_sink_binding(name, None);
            self.record_path_relative_binding(name, None);
        } else if let Some(name) = assignment_target_member_object_name(&expr.left)
            && self.literal_allowlist_binding(name)
        {
            self.record_literal_allowlist_binding(name, false);
        }

        if let AssignmentTarget::StaticMemberExpression(member) = &expr.left {
            if let Expression::Identifier(obj) = &member.object {
                if obj.name == "module" && member.property.name == "exports" {
                    self.has_cjs_exports = true;
                    if let Expression::ObjectExpression(obj_expr) = &expr.right {
                        for prop in &obj_expr.properties {
                            if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop
                                && let Some(name) = p.key.static_name()
                            {
                                self.exports.push(ExportInfo {
                                    name: ExportName::Named(name.to_string()),
                                    local_name: None,
                                    is_type_only: false,
                                    visibility: VisibilityTag::None,
                                    span: p.span,
                                    members: vec![],
                                    is_side_effect_used: false,
                                    super_class: None,
                                });
                            }
                        }
                    }
                }
                if obj.name == "exports" {
                    self.has_cjs_exports = true;
                    self.exports.push(ExportInfo {
                        name: ExportName::Named(member.property.name.to_string()),
                        local_name: None,
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: expr.span,
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    });
                }
            } else if let Expression::StaticMemberExpression(inner) = &member.object
                && let Expression::Identifier(obj) = &inner.object
                && obj.name == "module"
                && inner.property.name == "exports"
            {
                self.has_cjs_exports = true;
                self.exports.push(ExportInfo {
                    name: ExportName::Named(member.property.name.to_string()),
                    local_name: None,
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: expr.span,
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                });
            }
            if matches!(member.object, Expression::ThisExpression(_)) {
                self.member_accesses.push(MemberAccess {
                    object: "this".to_string(),
                    member: member.property.name.to_string(),
                });
                if let Expression::NewExpression(new_expr) = &expr.right
                    && let Expression::Identifier(callee) = &new_expr.callee
                    && !super::helpers::is_builtin_constructor(callee.name.as_str())
                {
                    self.binding_target_names.insert(
                        format!("this.{}", member.property.name),
                        callee.name.to_string(),
                    );
                } else if let Expression::Identifier(ident) = &expr.right
                    && let Some(target_name) =
                        self.binding_target_names.get(ident.name.as_str()).cloned()
                {
                    self.binding_target_names
                        .insert(format!("this.{}", member.property.name), target_name);
                }
                if let Expression::Identifier(ident) = &expr.right {
                    self.copy_nested_binding_targets(
                        ident.name.as_str(),
                        format!("this.{}", member.property.name).as_str(),
                    );
                }
            }
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
        if let Some(object_name) = static_member_object_name(&expr.object) {
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
            if let Expression::StringLiteral(lit) = &expr.expression {
                self.member_accesses.push(MemberAccess {
                    object: obj.name.to_string(),
                    member: lit.value.to_string(),
                });
            } else {
                self.whole_object_uses.push(obj.name.to_string());
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
            self.whole_object_uses.push(ident.name.to_string());
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
            self.binding_target_names.entry(local).or_insert(class_name);
        }
        walk::walk_if_statement(self, stmt);
    }

    fn visit_spread_element(&mut self, elem: &SpreadElement<'a>) {
        if let Expression::Identifier(ident) = &elem.argument {
            self.whole_object_uses.push(ident.name.to_string());
        }
        walk::walk_spread_element(self, elem);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        if let Some(decorator) = lit_custom_element_decorator(class) {
            if let Some(id) = class.id.as_ref() {
                self.record_lit_custom_element_candidate(
                    decorator,
                    SideEffectRegistrationTarget::LocalClass(id.name.to_string()),
                );
            } else if let Some(export) = self.exports.last()
                && matches!(export.name, crate::ExportName::Default)
                && export.local_name.is_none()
            {
                let export_index = self.exports.len() - 1;
                self.record_lit_custom_element_candidate(
                    decorator,
                    SideEffectRegistrationTarget::AnonymousDefaultExport(export_index),
                );
            }
        }

        if let Some(meta) = extract_angular_component_metadata(class) {
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

            if let Some(ref template) = meta.inline_template {
                let refs = crate::sfc_template::angular::collect_angular_template_refs(template);
                for name in refs.identifiers {
                    self.member_accesses.push(MemberAccess {
                        object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                        member: name,
                    });
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

            for name in &meta.host_member_refs {
                self.member_accesses.push(MemberAccess {
                    object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                    member: name.clone(),
                });
            }

            for name in &meta.input_output_members {
                self.member_accesses.push(MemberAccess {
                    object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                    member: name.clone(),
                });
            }
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
            }
        }
        self.capture_tagged_template_sink(expr);
        walk::walk_tagged_template_expression(self, expr);
    }

    fn visit_jsx_attribute(&mut self, attr: &oxc_ast::ast::JSXAttribute<'a>) {
        self.capture_jsx_attr_sink(attr);
        walk::walk_jsx_attribute(self, attr);
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

fn statement_exits_current_flow(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => {
            block.body.first().is_some_and(statement_exits_current_flow)
        }
        _ => false,
    }
}

fn path_relative_starts_with_parent<'b>(expr: &'b Expression<'_>) -> Option<&'b str> {
    let Expression::CallExpression(call) = unwrap_parens(expr) else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if member.property.name != "startsWith" {
        return None;
    }
    let Expression::Identifier(relative) = &member.object else {
        return None;
    };
    let Some(Argument::StringLiteral(prefix)) = call.arguments.first() else {
        return None;
    };
    if prefix.value.as_str() != ".." {
        return None;
    }
    Some(relative.name.as_str())
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

fn contains_computed_member(expr: &Expression<'_>) -> bool {
    match unwrap_parens(expr) {
        Expression::ComputedMemberExpression(_) => true,
        Expression::StaticMemberExpression(member) => contains_computed_member(&member.object),
        _ => false,
    }
}

fn skipped_callee_expression_kind(expr: &Expression<'_>) -> SkippedSecurityCalleeExpressionKind {
    match unwrap_parens(expr) {
        Expression::StaticMemberExpression(_) => {
            SkippedSecurityCalleeExpressionKind::StaticMemberExpression
        }
        Expression::ComputedMemberExpression(_) => {
            SkippedSecurityCalleeExpressionKind::ComputedMemberExpression
        }
        Expression::Identifier(_) => SkippedSecurityCalleeExpressionKind::Identifier,
        _ => SkippedSecurityCalleeExpressionKind::Other,
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

fn is_http_route_handler_name(name: &str) -> bool {
    matches!(
        name,
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "HEAD"
    )
}

fn is_route_registration_method(method: &str) -> bool {
    matches!(
        method,
        "all" | "delete" | "get" | "head" | "options" | "patch" | "post" | "put" | "use"
    )
}

fn is_trpc_procedure_method(method: &str) -> bool {
    matches!(method, "query" | "mutation" | "subscription")
}

fn is_trpc_procedure_callee(expr: &Expression<'_>, method: &str) -> bool {
    let Expression::StaticMemberExpression(member) = unwrap_parens(expr) else {
        return false;
    };
    member.property.name == method && trpc_chain_has_procedure(&member.object)
}

fn trpc_chain_has_procedure(expr: &Expression<'_>) -> bool {
    match unwrap_parens(expr) {
        Expression::Identifier(ident) => ident.name.to_ascii_lowercase().ends_with("procedure"),
        Expression::StaticMemberExpression(member) => {
            member.property.name == "procedure" || trpc_chain_has_procedure(&member.object)
        }
        Expression::CallExpression(call) => trpc_chain_has_procedure(&call.callee),
        _ => false,
    }
}

fn is_framework_route_receiver_path(callee_path: &str, method: &str) -> bool {
    let Some(receiver_path) = callee_path.strip_suffix(&format!(".{method}")) else {
        return false;
    };
    let Some(receiver) = receiver_path.rsplit('.').next() else {
        return false;
    };
    let receiver = receiver.to_ascii_lowercase();
    matches!(
        receiver.as_str(),
        "app" | "router" | "route" | "routes" | "server" | "fastify"
    ) || receiver.ends_with("app")
        || receiver.ends_with("router")
        || receiver.ends_with("routes")
        || receiver.ends_with("server")
}

fn function_body_has_use_server(body: Option<&FunctionBody<'_>>) -> bool {
    body.is_some_and(|body| {
        body.directives
            .iter()
            .any(|directive| directive.directive.as_str() == "use server")
    })
}

fn callback_params<'a>(arg: &'a Argument<'a>) -> Option<&'a FormalParameters<'a>> {
    match arg {
        Argument::ArrowFunctionExpression(expr) => Some(&expr.params),
        Argument::FunctionExpression(expr) => Some(&expr.params),
        _ => arg.as_expression().and_then(|expr| match expr {
            Expression::ArrowFunctionExpression(expr) => Some(&*expr.params),
            Expression::FunctionExpression(expr) => Some(&*expr.params),
            _ => None,
        }),
    }
}

fn function_like_params<'a>(expr: &'a Expression<'a>) -> Option<&'a FormalParameters<'a>> {
    match unwrap_parens(expr) {
        Expression::ArrowFunctionExpression(expr) => Some(&expr.params),
        Expression::FunctionExpression(expr) => Some(&expr.params),
        _ => None,
    }
}

fn last_callback_params<'a>(args: &'a [Argument<'a>]) -> Option<&'a FormalParameters<'a>> {
    args.iter().rev().find_map(callback_params)
}

fn route_callback_params<'a>(
    args: &'a [Argument<'a>],
    method: &str,
) -> Option<&'a FormalParameters<'a>> {
    if method == "use" {
        return last_callback_params(args);
    }
    args.iter().skip(1).find_map(callback_params)
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

#[derive(Debug, Clone)]
struct ReplaceStep {
    pattern: String,
    replacement: String,
}

fn sanitizer_scope_for_local_helper(
    extractor: &ModuleInfoExtractor,
    params: &FormalParameters<'_>,
    expr: &Expression<'_>,
) -> Option<SanitizerScope> {
    if local_html_escape_helper_expr(params, expr)
        || matches!(unwrap_static_expr(expr), Expression::TemplateLiteral(template) if extractor.html_template_is_sanitized(template))
    {
        return Some(SanitizerScope::Html);
    }
    if local_sql_identifier_quote_helper_expr(params, expr) {
        return Some(SanitizerScope::SqlIdentifier);
    }
    None
}

fn simple_param_names<'a>(params: &'a FormalParameters<'a>) -> Vec<&'a str> {
    params
        .items
        .iter()
        .filter_map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
            _ => None,
        })
        .collect()
}

fn local_html_escape_helper_expr(params: &FormalParameters<'_>, expr: &Expression<'_>) -> bool {
    let names = simple_param_names(params);
    let [param] = names.as_slice() else {
        return false;
    };
    let Some(steps) = replace_chain_for_param(expr, param) else {
        return false;
    };
    has_replace_step(&steps, "&", &["&amp;"])
        && has_replace_step(&steps, "<", &["&lt;"])
        && has_replace_step(&steps, ">", &["&gt;"])
}

fn local_sql_identifier_quote_helper_expr(
    params: &FormalParameters<'_>,
    expr: &Expression<'_>,
) -> bool {
    let names = simple_param_names(params);
    let [param] = names.as_slice() else {
        return false;
    };
    let Expression::TemplateLiteral(template) = unwrap_static_expr(expr) else {
        return false;
    };
    if template.expressions.len() != 1 || template.quasis.len() != 2 {
        return false;
    }
    if template
        .quasis
        .iter()
        .any(|quasi| template_raw(quasi) != "\"")
    {
        return false;
    }
    let Some(steps) = replace_chain_for_param(&template.expressions[0], param) else {
        return false;
    };
    has_replace_step(&steps, "\"", &["\"\""])
}

fn replace_chain_for_param(expr: &Expression<'_>, param: &str) -> Option<Vec<ReplaceStep>> {
    let mut steps = Vec::new();
    let mut current = unwrap_static_expr(expr);
    loop {
        match current {
            Expression::CallExpression(call) => {
                let step = replace_step_from_call(call)?;
                let Expression::StaticMemberExpression(member) = unwrap_parens(&call.callee) else {
                    return None;
                };
                steps.push(step);
                current = unwrap_static_expr(&member.object);
            }
            Expression::Identifier(ident) if ident.name == param => return Some(steps),
            _ => return None,
        }
    }
}

fn replace_step_from_call(call: &CallExpression<'_>) -> Option<ReplaceStep> {
    let Expression::StaticMemberExpression(member) = unwrap_parens(&call.callee) else {
        return None;
    };
    if !matches!(member.property.name.as_str(), "replace" | "replaceAll") {
        return None;
    }
    Some(ReplaceStep {
        pattern: replace_pattern(call.arguments.first()?)?,
        replacement: argument_static_string(call.arguments.get(1)?)?,
    })
}

fn replace_pattern(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::RegExpLiteral(lit) => Some(lit.regex.pattern.text.to_string()),
        _ => argument_static_string(arg),
    }
}

fn argument_static_string(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::StringLiteral(lit) => Some(lit.value.to_string()),
        _ => arg.as_expression().and_then(static_string_literal_value),
    }
}

fn has_replace_step(steps: &[ReplaceStep], pattern: &str, replacements: &[&str]) -> bool {
    steps.iter().any(|step| {
        pattern_matches(&step.pattern, pattern)
            && replacements
                .iter()
                .any(|replacement| step.replacement == *replacement)
    })
}

fn pattern_matches(actual: &str, expected: &str) -> bool {
    actual == expected || actual.strip_prefix('\\') == Some(expected)
}

fn template_raw<'a>(quasi: &'a TemplateElement<'_>) -> &'a str {
    quasi.value.raw.as_str()
}

fn html_template_interpolation_is_text(template: &TemplateLiteral<'_>, index: usize) -> bool {
    template
        .quasis
        .get(index)
        .map(template_raw)
        .is_some_and(html_raw_ends_in_text_context)
}

fn html_raw_ends_in_text_context(raw: &str) -> bool {
    let Some(last_lt) = raw.rfind('<') else {
        return true;
    };
    raw.rfind('>').is_some_and(|last_gt| last_gt > last_lt)
}

fn sql_identifier_template_context(raw: &str) -> bool {
    let lower = raw.trim_end().to_ascii_lowercase();
    [
        "select",
        " from",
        " join",
        " into",
        " update",
        " table",
        " by",
        "delete from",
        "alter table",
        "truncate table",
    ]
    .iter()
    .any(|suffix| lower.ends_with(suffix))
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

/// Whether a call's callee is a bare `defineStore` identifier (Pinia). The
/// harvest is loose here; the analyzer's `pinia` / `@pinia/nuxt` dependency
/// gate is the activation boundary.
fn is_define_store_callee(callee: &Expression<'_>) -> bool {
    matches!(callee, Expression::Identifier(id) if id.name.as_str() == "defineStore")
}

fn unwrap_paren_expr<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(paren) => &paren.expression,
        other => other,
    }
}

/// Harvest declared store members from a `defineStore('id', <arg>)` second
/// argument. Option store: `state` returned-object keys + `getters` keys +
/// `actions` keys. Setup store: the last return statement's object-literal
/// keys. Names starting with `$` (Pinia's `$patch` / `$reset` / `$subscribe`
/// API) are excluded. A setup store whose returned object spreads (`...base`)
/// abstains (returns `None`), keeping the whole store opaque.
fn harvest_define_store_members(args: &[Argument<'_>]) -> Option<Vec<MemberInfo>> {
    let second = args.get(1)?.as_expression()?;
    match unwrap_paren_expr(second) {
        Expression::ObjectExpression(obj) => Some(harvest_option_store(obj)),
        Expression::ArrowFunctionExpression(arrow) => {
            harvest_setup_return(extract_arrow_return_expr(arrow)?)
        }
        Expression::FunctionExpression(func) => harvest_setup_return(
            extract_function_body_final_return_expr(func.body.as_ref()?)?,
        ),
        _ => None,
    }
}

fn harvest_option_store(obj: &ObjectExpression<'_>) -> Vec<MemberInfo> {
    let mut members = Vec::new();
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(section) = prop.key.static_name() else {
            continue;
        };
        match section.as_ref() {
            "state" => {
                if let Some(state_obj) = state_returned_object(&prop.value) {
                    collect_store_member_keys(state_obj, &mut members);
                }
            }
            "getters" | "actions" => {
                if let Expression::ObjectExpression(section_obj) = unwrap_paren_expr(&prop.value) {
                    collect_store_member_keys(section_obj, &mut members);
                }
            }
            _ => {}
        }
    }
    members
}

/// The object returned by a Pinia option-store `state` value (an arrow or
/// function returning an object literal: `state: () => ({ count: 0 })`).
fn state_returned_object<'a, 'b>(value: &'b Expression<'a>) -> Option<&'b ObjectExpression<'a>> {
    let returned = match value {
        Expression::ArrowFunctionExpression(arrow) => extract_arrow_return_expr(arrow)?,
        Expression::FunctionExpression(func) => {
            extract_function_body_final_return_expr(func.body.as_ref()?)?
        }
        _ => return None,
    };
    match unwrap_paren_expr(returned) {
        Expression::ObjectExpression(obj) => Some(obj),
        _ => None,
    }
}

/// Harvest members from a setup-store return expression. Abstains (returns
/// `None`) on a spread (`return { ...base, count }`) so the whole store stays
/// opaque rather than under-counting.
fn harvest_setup_return(returned: &Expression<'_>) -> Option<Vec<MemberInfo>> {
    let Expression::ObjectExpression(obj) = unwrap_paren_expr(returned) else {
        return None;
    };
    if obj
        .properties
        .iter()
        .any(|p| matches!(p, ObjectPropertyKind::SpreadProperty(_)))
    {
        return None;
    }
    let mut members = Vec::new();
    collect_store_member_keys(obj, &mut members);
    Some(members)
}

fn collect_store_member_keys(obj: &ObjectExpression<'_>, members: &mut Vec<MemberInfo>) {
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(name) = prop.key.static_name() else {
            continue;
        };
        if name.starts_with('$') {
            continue;
        }
        members.push(MemberInfo {
            name: name.to_string(),
            kind: MemberKind::StoreMember,
            span: prop.key.span(),
            has_decorator: false,
            decorator_names: Vec::new(),
            is_instance_returning_static: false,
            is_self_returning: false,
        });
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

/// The source path for a DESTRUCTURE binding (`const { id } = req.query`): the
/// FULL flattened init path (`req.query`), since the destructured keys are the
/// leaves. A bare-identifier init (`const { id } = req`) yields `req`.
fn destructure_source_path(expr: &Expression<'_>) -> Option<String> {
    flatten_member_path(expr)
}

/// Whether an expression is a non-literal argument (a conservative trigger for
/// sink capture). A fully-literal argument is never captured.
fn is_non_literal_arg(expr: &Expression<'_>) -> bool {
    match unwrap_static_expr(expr) {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_) => false,
        Expression::TemplateLiteral(tpl) => !tpl.expressions.is_empty(),
        _ => true,
    }
}

/// Classify a captured non-literal argument into a finer-grained [`SinkArgKind`]
/// so the catalogue can require unsafe shapes (concat, template-with-substitution)
/// and exclude safe ones (object literal, the parameterized form). Static
/// TypeScript wrappers are unwrapped first; a `+` `BinaryExpression` is treated
/// as a string concatenation (a numeric `+` reaching a SQL/HTML sink is already
/// noise-free because the catalogue gates on the sink shape and callee).
fn classify_arg_kind(expr: &Expression<'_>) -> SinkArgKind {
    match unwrap_static_expr(expr) {
        Expression::TemplateLiteral(_) => SinkArgKind::TemplateWithSubst,
        Expression::BinaryExpression(bin)
            if bin.operator == oxc_ast::ast::BinaryOperator::Addition =>
        {
            SinkArgKind::Concat
        }
        Expression::ObjectExpression(_) => SinkArgKind::Object,
        Expression::CallExpression(_) => SinkArgKind::Call,
        _ => SinkArgKind::Other,
    }
}

fn classify_url_shape(
    expr: &Expression<'_>,
    static_string_bindings: &FxHashMap<String, String>,
) -> Option<SecurityUrlShape> {
    match unwrap_static_expr(expr) {
        Expression::TemplateLiteral(tpl) => {
            classify_template_url_shape(tpl, static_string_bindings)
        }
        Expression::BinaryExpression(bin) if bin.operator == BinaryOperator::Addition => {
            classify_concat_url_shape(bin, static_string_bindings)
        }
        Expression::Identifier(ident) => Some(
            static_string_bindings
                .get(ident.name.as_str())
                .map_or(SecurityUrlShape::DynamicOrigin, |value| {
                    classify_url_prefix(value)
                }),
        ),
        Expression::StringLiteral(_) => None,
        _ => Some(SecurityUrlShape::DynamicOrigin),
    }
}

fn classify_template_url_shape(
    tpl: &TemplateLiteral<'_>,
    static_string_bindings: &FxHashMap<String, String>,
) -> Option<SecurityUrlShape> {
    let first = tpl.quasis.first()?.value.raw.as_ref();
    if !first.is_empty() {
        return Some(classify_url_prefix(first));
    }
    let first_expr = tpl.expressions.first()?;
    let Expression::Identifier(ident) = unwrap_static_expr(first_expr) else {
        return Some(SecurityUrlShape::DynamicOrigin);
    };
    Some(
        static_string_bindings
            .get(ident.name.as_str())
            .map_or(SecurityUrlShape::DynamicOrigin, |base| {
                classify_url_prefix(base)
            }),
    )
}

fn classify_concat_url_shape(
    bin: &BinaryExpression<'_>,
    static_string_bindings: &FxHashMap<String, String>,
) -> Option<SecurityUrlShape> {
    match unwrap_static_expr(&bin.left) {
        Expression::StringLiteral(lit) => Some(classify_url_prefix(lit.value.as_str())),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => tpl
            .quasis
            .first()
            .map(|quasi| classify_url_prefix(quasi.value.raw.as_ref())),
        Expression::Identifier(ident) => Some(
            static_string_bindings
                .get(ident.name.as_str())
                .map_or(SecurityUrlShape::DynamicOrigin, |value| {
                    classify_url_prefix(value)
                }),
        ),
        Expression::BinaryExpression(left) if left.operator == BinaryOperator::Addition => {
            classify_concat_url_shape(left, static_string_bindings)
        }
        _ => Some(SecurityUrlShape::DynamicOrigin),
    }
}

fn classify_url_prefix(prefix: &str) -> SecurityUrlShape {
    if has_fixed_url_origin_or_root(prefix) {
        SecurityUrlShape::FixedOriginDynamicPath
    } else {
        SecurityUrlShape::DynamicOrigin
    }
}

fn has_fixed_url_origin_or_root(prefix: &str) -> bool {
    let trimmed = prefix.trim_start();
    trimmed.starts_with('/')
        || trimmed.find("://").is_some_and(|scheme_end| {
            scheme_end > 0
                && !trimmed[scheme_end + 3..]
                    .split(['/', '?', '#'])
                    .next()
                    .unwrap_or_default()
                    .is_empty()
        })
}

fn sink_literal_value(expr: &Expression<'_>) -> Option<SinkLiteralValue> {
    match unwrap_static_expr(expr) {
        Expression::StringLiteral(lit) => Some(SinkLiteralValue::String(lit.value.to_string())),
        Expression::NumericLiteral(lit) if lit.value.is_finite() && lit.value.fract() == 0.0 => {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "finite integer JS literals in the safe i64 range are the only admitted numeric sink metadata"
            )]
            let value = lit.value as i64;
            if (value as f64 - lit.value).abs() < f64::EPSILON {
                Some(SinkLiteralValue::Integer(value))
            } else {
                None
            }
        }
        Expression::BooleanLiteral(lit) => Some(SinkLiteralValue::Boolean(lit.value)),
        Expression::NullLiteral(_) => Some(SinkLiteralValue::Null),
        _ => None,
    }
}

fn static_sink_literal_to_string(value: &SinkLiteralValue) -> String {
    match value {
        SinkLiteralValue::String(value) => value.clone(),
        SinkLiteralValue::Integer(value) => value.to_string(),
        SinkLiteralValue::Boolean(value) => value.to_string(),
        SinkLiteralValue::Null => "null".to_string(),
    }
}

fn static_string_literal_value(expr: &Expression<'_>) -> Option<String> {
    match unwrap_static_expr(expr) {
        Expression::StringLiteral(lit) => Some(lit.value.to_string()),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() && tpl.quasis.len() == 1 => {
            tpl.quasis
                .first()
                .and_then(|quasi| quasi.value.cooked.as_ref())
                .map(ToString::to_string)
        }
        _ => None,
    }
}

fn risky_redos_fragment(pattern: &str) -> Option<String> {
    let mut cursor = 0;
    while let Some(relative) = pattern[cursor..].find('(') {
        let start = cursor + relative;
        let close = find_group_close(pattern, start)?;
        let Some(outer_end) = unbounded_quantifier_end(pattern, close + 1) else {
            cursor = close + 1;
            continue;
        };
        let body_start = group_body_start(pattern, start + 1);
        let body = &pattern[body_start..close];
        if has_unbounded_quantifier(body) || has_ambiguous_alternation(body) {
            return Some(pattern[start..outer_end].to_string());
        }
        cursor = close + 1;
    }
    None
}

fn find_group_close(pattern: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_class = false;
    let mut escaped = false;
    for (idx, ch) in pattern[open..].char_indices() {
        let absolute = open + idx;
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            continue;
        }
        match ch {
            '[' => in_class = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(absolute);
                }
            }
            _ => {}
        }
    }
    None
}

fn group_body_start(pattern: &str, body_start: usize) -> usize {
    let Some(rest) = pattern.get(body_start..) else {
        return body_start;
    };
    if rest.starts_with("?:") || rest.starts_with("?=") || rest.starts_with("?!") {
        return body_start + 2;
    }
    if rest.starts_with("?<=") || rest.starts_with("?<!") {
        return body_start + 3;
    }
    if rest.starts_with("?<")
        && let Some(end) = rest.find('>')
    {
        return body_start + end + 1;
    }
    body_start
}

fn unbounded_quantifier_end(pattern: &str, idx: usize) -> Option<usize> {
    let rest = pattern.get(idx..)?;
    if rest.starts_with('+') || rest.starts_with('*') {
        return Some(idx + 1);
    }
    if !rest.starts_with('{') {
        return None;
    }
    let close = rest.find('}')?;
    let body = &rest[1..close];
    let (min, max) = body.split_once(',')?;
    if min.chars().all(|ch| ch.is_ascii_digit()) && max.is_empty() {
        return Some(idx + close + 1);
    }
    None
}

fn has_unbounded_quantifier(pattern: &str) -> bool {
    let mut in_class = false;
    let mut escaped = false;
    let mut idx = 0;
    while idx < pattern.len() {
        let Some(ch) = pattern[idx..].chars().next() else {
            break;
        };
        if escaped {
            escaped = false;
            idx += ch.len_utf8();
            continue;
        }
        if ch == '\\' {
            escaped = true;
            idx += ch.len_utf8();
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            idx += ch.len_utf8();
            continue;
        }
        if ch == '[' {
            in_class = true;
            idx += ch.len_utf8();
            continue;
        }
        if unbounded_quantifier_end(pattern, idx).is_some() {
            return true;
        }
        idx += ch.len_utf8();
    }
    false
}

fn has_ambiguous_alternation(pattern: &str) -> bool {
    let branches = top_level_alternation_branches(pattern);
    if branches.len() < 2 {
        return false;
    }
    let tokens = branches
        .iter()
        .map(|branch| regex_branch_tokens(branch))
        .collect::<Vec<_>>();
    tokens.iter().enumerate().any(|(left_idx, left)| {
        !left.is_empty()
            && tokens
                .iter()
                .enumerate()
                .any(|(right_idx, right)| left_idx != right_idx && is_prefix_tokens(left, right))
    })
}

fn top_level_alternation_branches(pattern: &str) -> Vec<&str> {
    let mut branches = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut in_class = false;
    let mut escaped = false;
    for (idx, ch) in pattern.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            continue;
        }
        match ch {
            '[' => in_class = true,
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                branches.push(&pattern[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    branches.push(&pattern[start..]);
    branches
}

fn regex_branch_tokens(branch: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = branch.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let escaped = chars
                    .next()
                    .map_or_else(|| "\\".to_string(), |next| format!("\\{next}"));
                tokens.push(escaped);
            }
            '[' => {
                let mut token = String::from("[");
                for next in chars.by_ref() {
                    token.push(next);
                    if next == ']' {
                        break;
                    }
                }
                tokens.push(token);
            }
            '^' | '$' | '+' | '*' | '?' => {}
            '{' => {
                for next in chars.by_ref() {
                    if next == '}' {
                        break;
                    }
                }
            }
            _ => tokens.push(ch.to_string()),
        }
    }
    tokens
}

fn is_prefix_tokens(left: &[String], right: &[String]) -> bool {
    left.len() < right.len() && right.starts_with(left)
}

fn should_capture_hardcoded_secret_literal(context_name: &str, value: &str) -> bool {
    is_secret_shaped_context_name(context_name) || has_provider_prefix_capture_hint(value)
}

fn is_secret_shaped_context_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "apikey",
        "api_key",
        "accesskey",
        "access_key",
        "privatekey",
        "private_key",
        "clientsecret",
        "client_secret",
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "jwt",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn has_provider_prefix_capture_hint(value: &str) -> bool {
    value.starts_with("AKIA")
        || value.starts_with("ASIA")
        || value.starts_with("ghp_")
        || value.starts_with("gho_")
        || value.starts_with("ghu_")
        || value.starts_with("ghs_")
        || value.starts_with("ghr_")
        || value.starts_with("github_pat_")
        || value.starts_with("glpat-")
        || value.starts_with("xoxb-")
        || value.starts_with("xoxp-")
        || value.starts_with("xoxa-")
        || value.starts_with("xoxr-")
        || value.starts_with("xoxs-")
        || value.starts_with("sk_live_")
        || value.starts_with("rk_live_")
        || value.starts_with("sk-ant-")
        || value.starts_with("sk-proj-")
        || value.starts_with("AIza")
        || value.starts_with("SG.")
        || value.starts_with("npm_")
        || value.starts_with("pypi-")
        || value.starts_with("sq0atp-")
        || value.starts_with("shpat_")
        || value.starts_with("shpss_")
        || value.starts_with("shpca_")
        || value.starts_with("shppa_")
        || value.starts_with("dp.pt.")
        || value.starts_with("doo_v1_")
        || value.starts_with("dop_v1_")
        || value.starts_with("dor_v1_")
        || value.starts_with("dot_v1_")
        || value.starts_with("dapi")
        || value.starts_with("lin_api_")
        || value.starts_with("PMAK-")
        || value.starts_with("hf_")
        || value.starts_with("AGE-SECRET-KEY-1")
        || value.contains("-----BEGIN")
}

fn object_literal_properties(expr: &Expression<'_>) -> Vec<SinkObjectProperty> {
    let mut properties = Vec::new();
    collect_object_literal_properties(expr, "", &mut properties);
    properties
}

fn collect_object_literal_properties(
    expr: &Expression<'_>,
    prefix: &str,
    properties: &mut Vec<SinkObjectProperty>,
) {
    let Expression::ObjectExpression(obj) = unwrap_static_expr(expr) else {
        return;
    };
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(key) = prop.key.static_name() else {
            continue;
        };
        let key = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        if let Some(value) = sink_literal_value(&prop.value) {
            properties.push(SinkObjectProperty { key, value });
        } else {
            collect_object_literal_properties(&prop.value, &key, properties);
        }
    }
}

struct ObjectKeyMetadata {
    keys: Vec<String>,
    complete: bool,
}

fn object_key_metadata(expr: &Expression<'_>) -> ObjectKeyMetadata {
    let Expression::ObjectExpression(obj) = unwrap_static_expr(expr) else {
        return ObjectKeyMetadata {
            keys: Vec::new(),
            complete: false,
        };
    };
    let mut keys = Vec::new();
    let mut complete = true;
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            complete = false;
            continue;
        };
        let Some(key) = prop.key.static_name() else {
            complete = false;
            continue;
        };
        let key = key.to_string();
        if !keys.iter().any(|existing| existing == &key) {
            keys.push(key);
        }
    }
    ObjectKeyMetadata { keys, complete }
}

fn should_capture_literal_sink_value(
    callee_path: &str,
    sink_shape: SinkShape,
    arg_index: u32,
    literal: &SinkLiteralValue,
) -> bool {
    match sink_shape {
        SinkShape::Call | SinkShape::MemberCall => match literal {
            SinkLiteralValue::String(value) => {
                (arg_index == 1 && is_post_message_callee(callee_path) && value == "*")
                    || (arg_index == 0 && is_weak_crypto_literal_callee(callee_path))
                    || (arg_index == 0 && is_string_code_callee(callee_path))
                    || (arg_index == 0 && is_temp_file_literal_callee(callee_path))
                    || (arg_index == 0
                        && is_cleartext_transport_literal_callee(callee_path)
                        && is_cleartext_transport_literal(value))
                    || (arg_index == 0
                        && is_literal_metadata_url_callee(callee_path)
                        && is_metadata_service_literal(value))
            }
            SinkLiteralValue::Integer(_) => arg_index == 1 && is_chmod_literal_callee(callee_path),
            SinkLiteralValue::Boolean(_) | SinkLiteralValue::Null => false,
        },
        SinkShape::NewExpression => match literal {
            SinkLiteralValue::String(value) => {
                arg_index == 0
                    && (callee_path == "Function"
                        || (callee_path == "WebSocket" && is_cleartext_websocket_literal(value)))
            }
            SinkLiteralValue::Integer(_)
            | SinkLiteralValue::Boolean(_)
            | SinkLiteralValue::Null => false,
        },
        SinkShape::MemberAssign => {
            arg_index == 0
                && callee_path == "process.env.NODE_TLS_REJECT_UNAUTHORIZED"
                && matches!(literal, SinkLiteralValue::String(value) if value == "0")
        }
        SinkShape::TaggedTemplate | SinkShape::JsxAttr | SinkShape::SecretLiteral => false,
    }
}

fn is_direct_numeric_clamp_expr(expr: &Expression<'_>) -> bool {
    let Expression::CallExpression(call) = unwrap_static_expr(expr) else {
        return false;
    };
    let Some(callee_path) = flatten_callee_path(&call.callee) else {
        return false;
    };
    if callee_path == "Math.min" {
        return call
            .arguments
            .iter()
            .filter_map(Argument::as_expression)
            .any(|arg| matches!(sink_literal_value(arg), Some(SinkLiteralValue::Integer(_))));
    }
    callee_path == "Math.max"
        && call
            .arguments
            .iter()
            .filter_map(Argument::as_expression)
            .any(is_direct_numeric_clamp_expr)
}

fn is_resource_amplification_callee(
    callee_path: &str,
    sink_shape: SinkShape,
    arg_index: u32,
) -> bool {
    if arg_index != 0 {
        return false;
    }
    match sink_shape {
        SinkShape::Call | SinkShape::NewExpression => callee_path == "Array",
        SinkShape::MemberCall => {
            matches!(
                callee_path,
                "Buffer.alloc" | "Buffer.allocUnsafe" | "Buffer.allocUnsafeSlow"
            ) || matches!(
                callee_path.rsplit('.').next(),
                Some("repeat" | "padStart" | "padEnd")
            )
        }
        SinkShape::MemberAssign
        | SinkShape::TaggedTemplate
        | SinkShape::JsxAttr
        | SinkShape::SecretLiteral => false,
    }
}

fn should_skip_clamped_resource_amplification_arg(
    callee_path: &str,
    sink_shape: SinkShape,
    arg_index: u32,
    expr: &Expression<'_>,
) -> bool {
    is_resource_amplification_callee(callee_path, sink_shape, arg_index)
        && is_direct_numeric_clamp_expr(expr)
}

fn is_post_message_callee(callee_path: &str) -> bool {
    callee_path == "postMessage" || callee_path.ends_with(".postMessage")
}

fn is_weak_crypto_literal_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "createHash"
            | "createCipher"
            | "createDecipher"
            | "createCipheriv"
            | "createDecipheriv"
            | "crypto.createHash"
            | "crypto.createCipher"
            | "crypto.createDecipher"
            | "crypto.createCipheriv"
            | "crypto.createDecipheriv"
    )
}

fn is_string_code_callee(callee_path: &str) -> bool {
    matches!(callee_path, "setTimeout" | "setInterval")
}

fn is_chmod_literal_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "fs.chmod" | "fs.chmodSync" | "fs.promises.chmod" | "chmod" | "chmodSync"
    )
}

fn is_temp_file_literal_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "fs.writeFile"
            | "fs.writeFileSync"
            | "fs.appendFile"
            | "fs.appendFileSync"
            | "fs.createWriteStream"
            | "fs.promises.writeFile"
            | "fs.promises.appendFile"
            | "writeFile"
            | "writeFileSync"
            | "appendFile"
            | "appendFileSync"
            | "createWriteStream"
    )
}

fn is_literal_metadata_url_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "fetch"
            | "axios.get"
            | "axios.post"
            | "got"
            | "ky"
            | "needle"
            | "request"
            | "http.request"
            | "https.request"
            | "undici.request"
    )
}

fn is_cleartext_transport_literal_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "fetch"
            | "axios.get"
            | "axios.post"
            | "got"
            | "ky"
            | "needle"
            | "request"
            | "http.request"
            | "http.get"
            | "superagent.get"
            | "undici.request"
    )
}

fn is_cleartext_transport_literal(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("ftp://")
}

fn is_cleartext_websocket_literal(value: &str) -> bool {
    value.to_ascii_lowercase().starts_with("ws://")
}

fn is_metadata_service_literal(value: &str) -> bool {
    value.contains("169.254.169.254") || value.contains("metadata.google.internal")
}

fn should_capture_missing_jwt_verify_options(
    callee_path: &str,
    sink_shape: SinkShape,
    arg_len: usize,
) -> bool {
    arg_len == 2
        && matches!(sink_shape, SinkShape::Call | SinkShape::MemberCall)
        && (callee_path == "verify" || callee_path.ends_with(".verify"))
}

fn is_token_like_security_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "token",
        "secret",
        "session",
        "jwt",
        "auth",
        "csrf",
        "nonce",
        "salt",
        "password",
        "credential",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn expression_contains_math_random_call(expr: &Expression<'_>) -> bool {
    match unwrap_parens(expr) {
        Expression::CallExpression(call) => {
            if call.arguments.is_empty()
                && flatten_callee_path(&call.callee).as_deref() == Some("Math.random")
            {
                return true;
            }
            expression_callee_contains_math_random(&call.callee)
                || call
                    .arguments
                    .iter()
                    .filter_map(Argument::as_expression)
                    .any(expression_contains_math_random_call)
        }
        Expression::BinaryExpression(bin) => {
            expression_contains_math_random_call(&bin.left)
                || expression_contains_math_random_call(&bin.right)
        }
        Expression::LogicalExpression(logical) => {
            expression_contains_math_random_call(&logical.left)
                || expression_contains_math_random_call(&logical.right)
        }
        Expression::ConditionalExpression(cond) => {
            expression_contains_math_random_call(&cond.test)
                || expression_contains_math_random_call(&cond.consequent)
                || expression_contains_math_random_call(&cond.alternate)
        }
        Expression::TemplateLiteral(tpl) => tpl
            .expressions
            .iter()
            .any(expression_contains_math_random_call),
        Expression::ArrayExpression(array) => array.elements.iter().any(|element| {
            element
                .as_expression()
                .is_some_and(expression_contains_math_random_call)
        }),
        Expression::ObjectExpression(obj) => obj.properties.iter().any(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return false;
            };
            expression_contains_math_random_call(&prop.value)
        }),
        Expression::ParenthesizedExpression(paren) => {
            expression_contains_math_random_call(&paren.expression)
        }
        Expression::StaticMemberExpression(member) => {
            expression_contains_math_random_call(&member.object)
        }
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => {
                if call.arguments.is_empty()
                    && flatten_callee_path(&call.callee).as_deref() == Some("Math.random")
                {
                    return true;
                }
                expression_callee_contains_math_random(&call.callee)
            }
            ChainElement::StaticMemberExpression(member) => {
                expression_contains_math_random_call(&member.object)
            }
            _ => false,
        },
        _ => false,
    }
}

fn expression_callee_contains_math_random(callee: &Expression<'_>) -> bool {
    match callee {
        Expression::StaticMemberExpression(member) => {
            expression_contains_math_random_call(&member.object)
        }
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::StaticMemberExpression(member) => {
                expression_contains_math_random_call(&member.object)
            }
            ChainElement::CallExpression(call) => {
                expression_callee_contains_math_random(&call.callee)
            }
            _ => false,
        },
        _ => false,
    }
}

fn assignment_target_security_context_name(target: &AssignmentTarget<'_>) -> Option<String> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        AssignmentTarget::StaticMemberExpression(member) => Some(member.property.name.to_string()),
        _ => None,
    }
}

/// Collect the bare identifier names referenced anywhere inside a sink argument
/// expression, deduped in source order. Used by the analyze layer to back-trace
/// the argument to a source-tainted local binding. This is a bounded, shallow
/// structural walk over the common taint-carrying shapes (member roots, binary /
/// template / call / paren / conditional / sequence / await / unary), NOT a full
/// expression evaluator: an identifier that never surfaces in these shapes is
/// simply not collected (a conservative miss, never a false source link).
fn collect_arg_idents(expr: &Expression<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_idents_into(expr, &mut out);
    out
}

fn collect_arg_source_paths(expr: &Expression<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_source_paths_into(expr, &mut out);
    out
}

/// The arg-0 URL of a call as a static string literal, for the `secret-to-network`
/// destination signal (#890). `Some(literal)` when the destination is a plain
/// string literal or a no-substitution template (almost always intended auth);
/// `None` when it is dynamic (an interpolated URL, an env-configured base, a
/// variable) or absent. A dynamic destination is the higher-signal exfil case.
fn call_url_arg_literal(expr: &CallExpression<'_>) -> Option<String> {
    match expr.arguments.first()?.as_expression()? {
        Expression::StringLiteral(lit) => Some(lit.value.to_string()),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            tpl.quasis.first().map(|q| q.value.raw.to_string())
        }
        _ => None,
    }
}

fn push_ident(name: &str, out: &mut Vec<String>) {
    if !out.iter().any(|n| n == name) {
        out.push(name.to_string());
    }
}

fn push_source_path(path: String, out: &mut Vec<String>) {
    if !out.iter().any(|existing| existing == &path) {
        out.push(path);
    }
}

fn push_member_source_paths(path: &str, out: &mut Vec<String>) {
    // A public env var is build-inlined, not a secret source; record neither the
    // full path nor the `process.env` / `import.meta.env` object prefix (#890).
    if fallow_types::extract::is_public_env_path(path) {
        return;
    }
    push_source_path(path.to_string(), out);
    if let Some((object, _)) = path.rsplit_once('.') {
        push_source_path(object.to_string(), out);
    }
}

fn collect_source_paths_into(expr: &Expression<'_>, out: &mut Vec<String>) {
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            collect_source_paths_into(&paren.expression, out);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_source_paths_into(&ts_as.expression, out);
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            collect_source_paths_into(&ts_sat.expression, out);
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            collect_source_paths_into(&ts_non_null.expression, out);
        }
        Expression::StaticMemberExpression(member) => {
            if let Some(path) = flatten_member_path(expr) {
                // #890: a public env read contributes no secret source. Return
                // before recursing into the object, otherwise the bare
                // `process.env` / `import.meta.env` object would be re-pushed as a
                // source and defeat the exclusion.
                if fallow_types::extract::is_public_env_path(&path) {
                    return;
                }
                push_member_source_paths(&path, out);
            }
            collect_source_paths_into(&member.object, out);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_source_paths_into(&member.object, out);
            collect_source_paths_into(&member.expression, out);
        }
        Expression::BinaryExpression(bin) => {
            collect_source_paths_into(&bin.left, out);
            collect_source_paths_into(&bin.right, out);
        }
        Expression::LogicalExpression(logical) => {
            collect_source_paths_into(&logical.left, out);
            collect_source_paths_into(&logical.right, out);
        }
        Expression::ConditionalExpression(cond) => {
            collect_source_paths_into(&cond.test, out);
            collect_source_paths_into(&cond.consequent, out);
            collect_source_paths_into(&cond.alternate, out);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_source_paths_into(e, out);
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for e in &tpl.expressions {
                collect_source_paths_into(e, out);
            }
        }
        Expression::AwaitExpression(await_expr) => {
            collect_source_paths_into(&await_expr.argument, out);
        }
        Expression::UnaryExpression(unary) => collect_source_paths_into(&unary.argument, out),
        Expression::CallExpression(call) => {
            collect_source_paths_into(&call.callee, out);
            for arg in &call.arguments {
                if let Some(arg_expr) = arg.as_expression() {
                    collect_source_paths_into(arg_expr, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_idents_into(expr: &Expression<'_>, out: &mut Vec<String>) {
    match expr {
        Expression::Identifier(ident) => push_ident(&ident.name, out),
        Expression::ParenthesizedExpression(paren) => collect_idents_into(&paren.expression, out),
        Expression::TSAsExpression(ts_as) => collect_idents_into(&ts_as.expression, out),
        Expression::TSSatisfiesExpression(ts_sat) => collect_idents_into(&ts_sat.expression, out),
        Expression::TSNonNullExpression(ts_non_null) => {
            collect_idents_into(&ts_non_null.expression, out);
        }
        Expression::StaticMemberExpression(member) => {
            // The leading object root carries the taint (`id` in `id.value`,
            // `req` in `req.query.id`); the property name is a static key.
            collect_idents_into(&member.object, out);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_idents_into(&member.object, out);
            collect_idents_into(&member.expression, out);
        }
        Expression::BinaryExpression(bin) => {
            collect_idents_into(&bin.left, out);
            collect_idents_into(&bin.right, out);
        }
        Expression::LogicalExpression(logical) => {
            collect_idents_into(&logical.left, out);
            collect_idents_into(&logical.right, out);
        }
        Expression::ConditionalExpression(cond) => {
            collect_idents_into(&cond.test, out);
            collect_idents_into(&cond.consequent, out);
            collect_idents_into(&cond.alternate, out);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_idents_into(e, out);
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for e in &tpl.expressions {
                collect_idents_into(e, out);
            }
        }
        Expression::AwaitExpression(await_expr) => collect_idents_into(&await_expr.argument, out),
        Expression::UnaryExpression(unary) => collect_idents_into(&unary.argument, out),
        Expression::CallExpression(call) => {
            // The callee can carry the taint (`getId().trim()` -> getId), as can
            // each argument (`escape(id)` -> id). Bounded one level by recursion.
            collect_idents_into(&call.callee, out);
            for arg in &call.arguments {
                if let Some(arg_expr) = arg.as_expression() {
                    collect_idents_into(arg_expr, out);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(prop) = prop {
                    collect_idents_into(&prop.value, out);
                }
            }
        }
        _ => {}
    }
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

fn is_require_resolve_callee(expr: &Expression<'_>) -> bool {
    let Expression::StaticMemberExpression(member) = expr else {
        return false;
    };
    let Expression::Identifier(object) = &member.object else {
        return false;
    };
    object.name == "require" && member.property.name == "resolve"
}

fn package_from_resolution_specifier(specifier: &str) -> Option<String> {
    if !is_package_resolution_specifier(specifier) {
        return None;
    }
    let package_name = package_name_from_specifier(specifier)?;
    let suffix = specifier
        .strip_prefix(package_name.as_str())
        .unwrap_or_default();
    (suffix.is_empty() || suffix == "/package.json").then_some(package_name)
}

fn is_package_resolution_specifier(specifier: &str) -> bool {
    if specifier.is_empty()
        || specifier.starts_with('.')
        || specifier.starts_with('/')
        || specifier.starts_with('#')
        || specifier.starts_with('$')
        || specifier.contains('\\')
        || specifier.contains(' ')
        || specifier.contains('?')
        || specifier.contains('!')
        || specifier.contains(':')
    {
        return false;
    }
    specifier
        .bytes()
        .any(|b| b.is_ascii_alphabetic() || b == b'@')
}

fn package_name_from_specifier(specifier: &str) -> Option<String> {
    if specifier.starts_with('@') {
        let mut parts = specifier.split('/');
        let scope = parts.next()?;
        let package = parts.next()?;
        if package.is_empty() {
            return None;
        }
        return Some(format!("{scope}/{package}"));
    }

    specifier
        .split('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn package_values_from_raw_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| package_from_resolution_specifier(value))
        .collect()
}

fn static_object_string_property_values(
    obj: &ObjectExpression<'_>,
) -> FxHashMap<String, Vec<String>> {
    let mut values = FxHashMap::default();
    collect_static_object_string_property_values(obj, &mut values);
    values
}

fn collect_static_object_string_property_values(
    obj: &ObjectExpression<'_>,
    values: &mut FxHashMap<String, Vec<String>>,
) {
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(key_name) = prop.key.static_name() else {
            continue;
        };
        match &prop.value {
            Expression::StringLiteral(lit) => {
                values
                    .entry(key_name.to_string())
                    .or_default()
                    .push(lit.value.to_string());
            }
            Expression::ObjectExpression(child) => {
                collect_static_object_string_property_values(child, values);
            }
            _ => {}
        }
    }
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

fn package_resolution_arg_index(
    params: &FormalParameters<'_>,
    body: &FunctionBody<'_>,
    known_helpers: &FxHashMap<String, usize>,
) -> Option<usize> {
    let param_names: Vec<String> = params
        .items
        .iter()
        .filter_map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
            _ => None,
        })
        .collect();
    let param_set: FxHashSet<String> = param_names.iter().cloned().collect();
    let mut collector = PackageResolutionParamCollector {
        params: &param_set,
        known_helpers,
        matched: FxHashSet::default(),
    };
    collector.visit_function_body(body);

    param_names
        .iter()
        .position(|name| collector.matched.contains(name))
}

struct PackageResolutionParamCollector<'p> {
    params: &'p FxHashSet<String>,
    known_helpers: &'p FxHashMap<String, usize>,
    matched: FxHashSet<String>,
}

impl<'a> Visit<'a> for PackageResolutionParamCollector<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if is_require_resolve_callee(&call.callee)
            && let Some(arg) = call.arguments.first()
            && let Some(param) = package_resolution_param_from_argument(arg, self.params)
        {
            self.matched.insert(param);
        }

        if call_joins_node_modules_with_param(call, self.params)
            && let Some(param) = call
                .arguments
                .iter()
                .find_map(|arg| package_param_argument_identifier_name(arg, self.params))
        {
            self.matched.insert(param);
        }

        if let Expression::Identifier(callee) = &call.callee
            && let Some(arg_index) = self.known_helpers.get(callee.name.as_str()).copied()
            && let Some(arg) = call.arguments.get(arg_index)
            && let Some(param) = package_param_argument_identifier_name(arg, self.params)
        {
            self.matched.insert(param);
        }

        walk::walk_call_expression(self, call);
    }
}

fn package_resolution_param_from_argument(
    arg: &Argument<'_>,
    params: &FxHashSet<String>,
) -> Option<String> {
    match arg {
        Argument::Identifier(ident) if params.contains(ident.name.as_str()) => {
            Some(ident.name.to_string())
        }
        Argument::TemplateLiteral(tpl)
            if tpl.expressions.len() == 1
                && tpl.quasis.len() == 2
                && tpl.quasis.first()?.value.raw.is_empty()
                && tpl.quasis.last()?.value.raw.as_str() == "/package.json" =>
        {
            package_param_expression_identifier_name(&tpl.expressions[0], params)
        }
        _ => arg
            .as_expression()
            .and_then(|expr| package_param_expression_identifier_name(expr, params)),
    }
}

fn call_joins_node_modules_with_param(
    call: &CallExpression<'_>,
    params: &FxHashSet<String>,
) -> bool {
    let has_node_modules = call.arguments.iter().any(
        |arg| matches!(arg, Argument::StringLiteral(lit) if lit.value.as_str() == "node_modules"),
    );
    has_node_modules
        && call
            .arguments
            .iter()
            .any(|arg| package_param_argument_identifier_name(arg, params).is_some())
}

fn package_param_argument_identifier_name(
    arg: &Argument<'_>,
    params: &FxHashSet<String>,
) -> Option<String> {
    match arg {
        Argument::Identifier(ident) if params.contains(ident.name.as_str()) => {
            Some(ident.name.to_string())
        }
        _ => arg
            .as_expression()
            .and_then(|expr| package_param_expression_identifier_name(expr, params)),
    }
}

fn package_param_expression_identifier_name(
    expr: &Expression<'_>,
    params: &FxHashSet<String>,
) -> Option<String> {
    match expr {
        Expression::Identifier(ident) if params.contains(ident.name.as_str()) => {
            Some(ident.name.to_string())
        }
        _ => None,
    }
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
    /// Recognize Pinia `defineStore` declarations and store-consumption shapes.
    ///
    /// Declaration: harvest `state` / `getters` / `actions` keys (option store)
    /// or the last return statement's object-literal keys (setup store) as
    /// `MemberKind::StoreMember`, keyed by the binding's local name for
    /// `enrich_store_exports`. Consumption: mark `const s = useFooStore()`
    /// instances and credit destructure reads (`const { count } = s` /
    /// `storeToRefs(s)` / `const { count } = useFooStore()`) so a member
    /// consumed only that way is not falsely flagged.
    fn record_pinia_store(&mut self, declarator: &VariableDeclarator<'_>, init: &Expression<'_>) {
        // Declaration: const <name> = defineStore('id', <options|setup>)
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Expression::CallExpression(call) = init
            && is_define_store_callee(&call.callee)
        {
            if let Some(members) = harvest_define_store_members(&call.arguments) {
                self.store_member_decls.insert(id.name.to_string(), members);
            }
            return;
        }

        // Consumption: const s = useFooStore()  (bare store-factory call)
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Expression::CallExpression(call) = init
            && let Expression::Identifier(callee) = &call.callee
            && self.is_store_factory_call(callee.name.as_str())
        {
            self.binding_target_names
                .entry(id.name.to_string())
                .or_insert_with(|| callee.name.to_string());
            self.store_instance_locals.insert(id.name.to_string());
            return;
        }

        // Consumption: destructure forms.
        let BindingPattern::ObjectPattern(obj_pat) = &declarator.id else {
            return;
        };
        if obj_pat.rest.is_some() {
            // `const { count, ...rest } = store` is a whole-object use; the
            // existing rest-destructure path records it. Abstain here.
            return;
        }

        match init {
            Expression::CallExpression(call) => {
                let Expression::Identifier(callee) = &call.callee else {
                    return;
                };
                if matches!(callee.name.as_str(), "storeToRefs" | "toRefs") {
                    // const { count } = storeToRefs(s) / toRefs(s): credit on the
                    // store-instance argument (which resolves to its factory
                    // export via `binding_target_names`).
                    if let Some(Argument::Identifier(arg)) = call.arguments.first()
                        && self.store_instance_locals.contains(arg.name.as_str())
                    {
                        self.credit_store_pattern_members(obj_pat, arg.name.as_str());
                    }
                } else if self.is_store_factory_call(callee.name.as_str()) {
                    // const { count, inc } = useFooStore(): credit on the factory
                    // import directly (the analyzer maps it to the store export).
                    self.credit_store_pattern_members(obj_pat, callee.name.as_str());
                }
            }
            // const { count } = s: destructure on a store-instance local.
            Expression::Identifier(ident)
                if self.store_instance_locals.contains(ident.name.as_str()) =>
            {
                self.credit_store_pattern_members(obj_pat, ident.name.as_str());
            }
            _ => {}
        }
    }

    /// Options-API map helpers (`mapState` / `mapGetters` / `mapActions` /
    /// `mapWritableState` / `mapStores`) reference store members as string-array
    /// or object arguments, not `store.member` accesses. v1 does not parse those
    /// member lists; instead it conservatively marks every identifier
    /// store-factory argument as a whole-object use, so all of that store's
    /// members abstain (lose recall) rather than false-flag.
    fn record_pinia_map_helpers(&mut self, expr: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &expr.callee else {
            return;
        };
        if !matches!(
            callee.name.as_str(),
            "mapState" | "mapGetters" | "mapActions" | "mapWritableState" | "mapStores"
        ) {
            return;
        }
        for arg in &expr.arguments {
            if let Argument::Identifier(ident) = arg {
                self.whole_object_uses.push(ident.name.to_string());
            }
        }
    }

    /// Whether a bare-identifier callee is a Pinia store factory: either an
    /// imported binding (explicit `import { useFooStore } from ...`) or a name
    /// following the `use<Name>Store` convention (Nuxt `@pinia/nuxt`
    /// auto-import). Crediting a non-store import is inert (the analyzer only
    /// credits a member that actually exists), so this can be permissive
    /// without risking class/enum member drift.
    fn is_store_factory_call(&self, name: &str) -> bool {
        self.imports.iter().any(|i| i.local_name == name)
            || (name.starts_with("use") && name.ends_with("Store"))
    }

    /// Emit a `MemberAccess` for each statically-named destructured key against
    /// `object_name` (the store instance local or factory import). The KEY (the
    /// store member), not the local alias, is recorded.
    fn credit_store_pattern_members(&mut self, obj_pat: &ObjectPattern<'_>, object_name: &str) {
        for prop in &obj_pat.properties {
            let Some(member) = prop.key.static_name() else {
                continue;
            };
            self.member_accesses.push(MemberAccess {
                object: object_name.to_string(),
                member: member.to_string(),
            });
        }
    }

    /// Copy harvested store members onto the matching `ExportInfo`, mirroring
    /// `enrich_local_class_exports`. Called from both `into_module_info` and
    /// `merge_into` (SFC `<script>` parity).
    pub(super) fn enrich_store_exports(&mut self) {
        if self.store_member_decls.is_empty() {
            return;
        }
        for export in &mut self.exports {
            if !export.members.is_empty() {
                continue;
            }
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            if let Some(members) = self.store_member_decls.get(local_name) {
                export.members = members.clone();
            }
        }
    }

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

    fn capture_redos_regex_sink(&mut self, expr: &CallExpression<'_>) {
        let Some((input_expr, pattern)) = self.redos_regex_application(expr) else {
            return;
        };
        if !is_non_literal_arg(input_expr) {
            return;
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::MemberCall,
            callee_path: "RegExp.redos".to_string(),
            arg_index: 0,
            arg_is_non_literal: true,
            arg_kind: classify_arg_kind(input_expr),
            arg_literal: None,
            regex_pattern: Some(pattern),
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents: collect_arg_idents(input_expr),
            arg_source_paths: collect_arg_source_paths(input_expr),
            span_start: expr.span.start,
            span_end: expr.span.end,
            url_arg_literal: None,
            url_shape: None,
        });
    }

    fn capture_security_control_call(&mut self, callee_path: &str, span: Span) {
        let Some(kind) = security_control_kind_for_callee(callee_path) else {
            return;
        };
        self.security_control_sites.push(SecurityControlSite {
            kind,
            callee_path: callee_path.to_string(),
            span_start: span.start,
            span_end: span.end,
        });
    }

    fn record_validation_control_site(&mut self, callee_path: &str, span: Span) {
        self.security_control_sites.push(SecurityControlSite {
            kind: SecurityControlKind::Validation,
            callee_path: callee_path.to_string(),
            span_start: span.start,
            span_end: span.end,
        });
    }

    fn has_package_evidence(&self, package: &str) -> bool {
        self.imports.iter().any(|import| {
            !import.is_type_only && import_source_matches_package(&import.source, package)
        }) || self
            .require_calls
            .iter()
            .any(|require| import_source_matches_package(&require.source, package))
    }

    fn capture_declarative_validation_control(&mut self, expr: &CallExpression<'_>) {
        let Some(callee_path) =
            flatten_callee_path(&expr.callee).or_else(|| callee_leaf_name(&expr.callee))
        else {
            return;
        };

        if self.is_elysia_validation_route(&callee_path, expr) {
            self.record_validation_control_site("elysia.route.validation", expr.span);
            return;
        }

        if self.is_fastify_validation_route(&callee_path, expr) {
            self.record_validation_control_site("fastify.route.schema", expr.span);
            return;
        }

        if self.is_trpc_input_control(&callee_path, expr) {
            self.record_validation_control_site("trpc.procedure.input", expr.span);
            return;
        }

        if self.is_hono_validation_middleware(&callee_path, expr) {
            self.record_validation_control_site("hono.validator", expr.span);
            return;
        }

        if self.is_nest_validation_pipe_call(&callee_path) {
            self.record_validation_control_site("nestjs.validation-pipe", expr.span);
            return;
        }

        if self.is_express_validator_control(&callee_path, expr) {
            self.record_validation_control_site("express-validator.middleware", expr.span);
        }
    }

    fn capture_declarative_validation_new_expression(
        &mut self,
        expr: &oxc_ast::ast::NewExpression<'_>,
    ) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            return;
        };
        if self.has_package_evidence("@nestjs/common")
            && callee_path.rsplit('.').next() == Some("ValidationPipe")
        {
            self.record_validation_control_site("nestjs.validation-pipe", expr.span);
        }
    }

    fn is_elysia_validation_route(&self, callee_path: &str, expr: &CallExpression<'_>) -> bool {
        self.has_package_evidence("elysia")
            && route_method_leaf(callee_path).is_some()
            && expr
                .arguments
                .iter()
                .skip(1)
                .filter_map(Argument::as_expression)
                .any(expression_has_boundary_validation_keys)
    }

    fn is_fastify_validation_route(&self, callee_path: &str, expr: &CallExpression<'_>) -> bool {
        self.has_package_evidence("fastify")
            && route_method_leaf(callee_path).is_some()
            && expr
                .arguments
                .iter()
                .filter_map(Argument::as_expression)
                .any(expression_has_fastify_schema)
    }

    fn is_trpc_input_control(&self, callee_path: &str, expr: &CallExpression<'_>) -> bool {
        let lower = callee_path.to_ascii_lowercase();
        self.has_package_evidence("@trpc/server")
            && callee_path
                .rsplit('.')
                .next()
                .is_some_and(|leaf| leaf.eq_ignore_ascii_case("input"))
            && lower.contains("procedure")
            && !expr.arguments.is_empty()
    }

    fn is_hono_validation_middleware(&self, callee_path: &str, expr: &CallExpression<'_>) -> bool {
        (self.has_package_evidence("hono") || self.has_package_evidence("@hono/zod-validator"))
            && matches!(
                callee_path.rsplit('.').next(),
                Some("validator" | "zValidator")
            )
            && !expr.arguments.is_empty()
    }

    fn is_nest_validation_pipe_call(&self, callee_path: &str) -> bool {
        self.has_package_evidence("@nestjs/common")
            && matches!(
                callee_path.rsplit('.').next(),
                Some("UsePipes" | "ValidationPipe")
            )
    }

    fn is_express_validator_control(&self, callee_path: &str, expr: &CallExpression<'_>) -> bool {
        self.has_package_evidence("express-validator")
            && matches!(
                callee_path.rsplit('.').next(),
                Some("body" | "param" | "query" | "check")
            )
            && !expr.arguments.is_empty()
    }

    fn capture_security_call_sites(&mut self, expr: &CallExpression<'_>) {
        self.capture_redos_regex_sink(expr);
        self.capture_declarative_validation_control(expr);
        self.capture_call_sink(expr);
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

    fn record_skipped_security_callee(
        &mut self,
        callee: &Expression<'_>,
        reason: SkippedSecurityCalleeReason,
    ) {
        let callee = unwrap_parens(callee);
        self.security_sinks_skipped += 1;
        self.security_unresolved_callee_sites
            .push(SkippedSecurityCalleeSite {
                reason,
                expression_kind: skipped_callee_expression_kind(callee),
                span_start: callee.span().start,
                span_end: callee.span().end,
            });
    }

    /// Capture a call/member-call sink site (category-blind). Pushes one
    /// `SinkSite` per admitted positional argument; a callee that cannot be
    /// flattened to a static path increments the blind-spot counter instead.
    fn capture_call_sink(&mut self, expr: &CallExpression<'_>) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            if self.redos_regex_application(expr).is_some() {
                return;
            }
            let reason = if contains_computed_member(&expr.callee) {
                SkippedSecurityCalleeReason::ComputedMember
            } else {
                SkippedSecurityCalleeReason::DynamicDispatch
            };
            self.record_skipped_security_callee(&expr.callee, reason);
            return;
        };
        self.capture_security_control_call(&callee_path, expr.span);
        let sink_shape = if callee_path.contains('.') {
            SinkShape::MemberCall
        } else {
            SinkShape::Call
        };
        // The arg-0 URL literal, captured once per call so the secret-to-network
        // category (#890) can carry a destination-host signal on the arg-1 sink.
        let url_arg_literal = call_url_arg_literal(expr);
        for (index, arg) in expr.arguments.iter().enumerate() {
            let Some(arg_expr) = arg.as_expression() else {
                continue;
            };
            let Ok(arg_index) = u32::try_from(index) else {
                continue;
            };
            let arg_literal = self.static_sink_literal_value(arg_expr);
            let arg_is_non_literal = arg_literal.is_none() && is_non_literal_arg(arg_expr);
            if arg_is_non_literal
                && should_skip_clamped_resource_amplification_arg(
                    &callee_path,
                    sink_shape,
                    arg_index,
                    arg_expr,
                )
            {
                continue;
            }
            if !arg_is_non_literal
                && !arg_literal.as_ref().is_some_and(|literal| {
                    should_capture_literal_sink_value(&callee_path, sink_shape, arg_index, literal)
                })
            {
                continue;
            }
            if arg_is_non_literal {
                self.record_sanitized_sink_arg(expr.span.start, arg_index, arg_expr);
            }
            let object_keys = object_key_metadata(arg_expr);
            self.security_sinks.push(SinkSite {
                sink_shape,
                callee_path: callee_path.clone(),
                arg_index,
                arg_is_non_literal,
                arg_kind: if arg_is_non_literal {
                    classify_arg_kind(arg_expr)
                } else {
                    SinkArgKind::Literal
                },
                arg_literal,
                object_properties: object_literal_properties(arg_expr),
                object_property_keys: object_keys.keys,
                object_property_keys_complete: object_keys.complete,
                arg_idents: if arg_is_non_literal {
                    collect_arg_idents(arg_expr)
                } else {
                    Vec::new()
                },
                arg_source_paths: if arg_is_non_literal {
                    collect_arg_source_paths(arg_expr)
                } else {
                    Vec::new()
                },
                regex_pattern: None,
                span_start: expr.span.start,
                span_end: expr.span.end,
                url_arg_literal: url_arg_literal.clone(),
                url_shape: if arg_is_non_literal {
                    classify_url_shape(arg_expr, &self.static_string_bindings)
                } else {
                    None
                },
            });
        }
        if should_capture_missing_jwt_verify_options(&callee_path, sink_shape, expr.arguments.len())
        {
            self.security_sinks.push(SinkSite {
                sink_shape,
                callee_path,
                arg_index: 2,
                arg_is_non_literal: false,
                arg_kind: SinkArgKind::Object,
                arg_literal: None,
                object_properties: Vec::new(),
                object_property_keys: Vec::new(),
                object_property_keys_complete: true,
                arg_idents: Vec::new(),
                arg_source_paths: Vec::new(),
                regex_pattern: None,
                span_start: expr.span.start,
                span_end: expr.span.end,
                url_arg_literal: None,
                url_shape: None,
            });
        }
    }

    /// Capture constructor-call sink sites. This is intentionally separate from
    /// call capture because oxc represents `new Function("...")` as a
    /// `NewExpression`, not a `CallExpression`.
    fn capture_new_expression_sink(&mut self, expr: &oxc_ast::ast::NewExpression<'_>) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            return;
        };
        for (index, arg) in expr.arguments.iter().enumerate() {
            let Some(arg_expr) = arg.as_expression() else {
                continue;
            };
            let Ok(arg_index) = u32::try_from(index) else {
                continue;
            };
            let arg_literal = self.static_sink_literal_value(arg_expr);
            let arg_is_non_literal = arg_literal.is_none() && is_non_literal_arg(arg_expr);
            if arg_is_non_literal
                && should_skip_clamped_resource_amplification_arg(
                    &callee_path,
                    SinkShape::NewExpression,
                    arg_index,
                    arg_expr,
                )
            {
                continue;
            }
            if !arg_is_non_literal
                && !arg_literal.as_ref().is_some_and(|literal| {
                    should_capture_literal_sink_value(
                        &callee_path,
                        SinkShape::NewExpression,
                        arg_index,
                        literal,
                    )
                })
            {
                continue;
            }
            let object_keys = object_key_metadata(arg_expr);
            self.security_sinks.push(SinkSite {
                sink_shape: SinkShape::NewExpression,
                callee_path: callee_path.clone(),
                arg_index,
                arg_is_non_literal,
                arg_kind: if arg_is_non_literal {
                    classify_arg_kind(arg_expr)
                } else {
                    SinkArgKind::Literal
                },
                arg_literal,
                object_properties: object_literal_properties(arg_expr),
                object_property_keys: object_keys.keys,
                object_property_keys_complete: object_keys.complete,
                arg_idents: if arg_is_non_literal {
                    collect_arg_idents(arg_expr)
                } else {
                    Vec::new()
                },
                arg_source_paths: if arg_is_non_literal {
                    collect_arg_source_paths(arg_expr)
                } else {
                    Vec::new()
                },
                regex_pattern: None,
                span_start: expr.span.start,
                span_end: expr.span.end,
                url_arg_literal: None,
                url_shape: if arg_is_non_literal {
                    classify_url_shape(arg_expr, &self.static_string_bindings)
                } else {
                    None
                },
            });
        }
    }

    fn capture_math_random_context_sink(
        &mut self,
        context_name: &str,
        expr: &Expression<'_>,
        span: Span,
    ) {
        if !is_token_like_security_name(context_name) || !expression_contains_math_random_call(expr)
        {
            return;
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::MemberCall,
            callee_path: "Math.random".to_string(),
            arg_index: 0,
            arg_is_non_literal: false,
            arg_kind: SinkArgKind::NoArg,
            arg_literal: None,
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents: vec![context_name.to_string()],
            arg_source_paths: Vec::new(),
            regex_pattern: None,
            span_start: span.start,
            span_end: span.end,
            url_arg_literal: None,
            url_shape: None,
        });
    }

    fn capture_hardcoded_secret_literal_sink(
        &mut self,
        context_name: &str,
        expr: &Expression<'_>,
        span: Span,
    ) {
        let Some(value) = static_string_literal_value(expr) else {
            return;
        };
        if !should_capture_hardcoded_secret_literal(context_name, &value) {
            return;
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::SecretLiteral,
            callee_path: context_name.to_string(),
            arg_index: 0,
            arg_is_non_literal: false,
            arg_kind: SinkArgKind::Literal,
            arg_literal: Some(SinkLiteralValue::String(value)),
            regex_pattern: None,
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents: vec![context_name.to_string()],
            arg_source_paths: Vec::new(),
            span_start: span.start,
            span_end: span.end,
            url_arg_literal: None,
            url_shape: None,
        });
    }

    /// Capture a member-assignment sink site (e.g. `el.innerHTML = userInput`).
    /// Static-member targets with a non-literal RHS are captured; one exact
    /// literal TLS-env assignment is admitted because the literal value is the
    /// security signal. A target whose object cannot be flattened increments
    /// the blind-spot counter.
    fn capture_member_assign_sink(&mut self, expr: &AssignmentExpression<'_>) {
        let AssignmentTarget::StaticMemberExpression(member) = &expr.left else {
            return;
        };
        let Some(object_path) = flatten_callee_path(&member.object) else {
            self.record_skipped_security_callee(
                &member.object,
                SkippedSecurityCalleeReason::UnsupportedAssignmentObject,
            );
            return;
        };
        let callee_path = format!("{}.{}", object_path, member.property.name);
        let arg_literal = self.static_sink_literal_value(&expr.right);
        let arg_is_non_literal = arg_literal.is_none() && is_non_literal_arg(&expr.right);
        if !arg_is_non_literal
            && !arg_literal.as_ref().is_some_and(|literal| {
                should_capture_literal_sink_value(&callee_path, SinkShape::MemberAssign, 0, literal)
            })
        {
            return;
        }
        if arg_is_non_literal {
            self.record_sanitized_sink_arg(expr.span.start, 0, &expr.right);
        }
        let object_keys = object_key_metadata(&expr.right);
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::MemberAssign,
            callee_path,
            arg_index: 0,
            arg_is_non_literal,
            arg_kind: if arg_is_non_literal {
                classify_arg_kind(&expr.right)
            } else {
                SinkArgKind::Literal
            },
            arg_literal,
            object_properties: object_literal_properties(&expr.right),
            object_property_keys: object_keys.keys,
            object_property_keys_complete: object_keys.complete,
            arg_idents: if arg_is_non_literal {
                collect_arg_idents(&expr.right)
            } else {
                Vec::new()
            },
            arg_source_paths: if arg_is_non_literal {
                collect_arg_source_paths(&expr.right)
            } else {
                Vec::new()
            },
            regex_pattern: None,
            span_start: expr.span.start,
            span_end: expr.span.end,
            url_arg_literal: None,
            url_shape: if arg_is_non_literal {
                classify_url_shape(&expr.right, &self.static_string_bindings)
            } else {
                None
            },
        });
    }

    /// Capture a tagged-template sink site (e.g. ``sql`...${x}...` ``). Only
    /// templates with at least one substitution are captured.
    fn capture_tagged_template_sink(&mut self, expr: &TaggedTemplateExpression<'_>) {
        if expr.quasi.expressions.is_empty() {
            return;
        }
        let Some(callee_path) = flatten_callee_path(&expr.tag) else {
            return;
        };
        let mut arg_idents: Vec<String> = Vec::new();
        let mut arg_source_paths: Vec<String> = Vec::new();
        for substitution in &expr.quasi.expressions {
            collect_idents_into(substitution, &mut arg_idents);
            collect_source_paths_into(substitution, &mut arg_source_paths);
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::TaggedTemplate,
            callee_path,
            arg_index: 0,
            arg_is_non_literal: true,
            // A tagged template is captured only with substitutions, so the
            // argument is always a template-with-substitution.
            arg_kind: SinkArgKind::TemplateWithSubst,
            arg_literal: None,
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents,
            arg_source_paths,
            regex_pattern: None,
            span_start: expr.span.start,
            span_end: expr.span.end,
            url_arg_literal: None,
            url_shape: None,
        });
    }

    /// Capture a JSX-attribute sink site (e.g. `dangerouslySetInnerHTML={x}`).
    /// Only identifier-named attributes with a non-literal expression-container
    /// value are captured; the empty `{}` form yields no expression and is
    /// skipped without an explicit arm.
    fn capture_jsx_attr_sink(&mut self, attr: &JSXAttribute<'_>) {
        let JSXAttributeName::Identifier(name) = &attr.name else {
            return;
        };
        let Some(JSXAttributeValue::ExpressionContainer(container)) = &attr.value else {
            return;
        };
        let Some(value_expr) = container.expression.as_expression() else {
            return;
        };
        if !is_non_literal_arg(value_expr) {
            return;
        }
        self.record_sanitized_sink_arg(attr.span.start, 0, value_expr);
        let object_keys = object_key_metadata(value_expr);
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::JsxAttr,
            callee_path: name.name.to_string(),
            arg_index: 0,
            arg_is_non_literal: true,
            arg_kind: classify_arg_kind(value_expr),
            arg_literal: None,
            object_properties: object_literal_properties(value_expr),
            object_property_keys: object_keys.keys,
            object_property_keys_complete: object_keys.complete,
            arg_idents: collect_arg_idents(value_expr),
            arg_source_paths: collect_arg_source_paths(value_expr),
            regex_pattern: None,
            span_start: attr.span.start,
            span_end: attr.span.end,
            url_arg_literal: None,
            url_shape: None,
        });
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

fn security_control_kind_for_callee(callee_path: &str) -> Option<SecurityControlKind> {
    let lower = callee_path.to_ascii_lowercase();
    let leaf = lower.rsplit('.').next().unwrap_or(lower.as_str());
    if is_validation_control(&lower, leaf) {
        return Some(SecurityControlKind::Validation);
    }
    if is_authorization_control(leaf) {
        return Some(SecurityControlKind::Authorization);
    }
    if is_authentication_control(leaf) {
        return Some(SecurityControlKind::Authentication);
    }
    if is_sanitization_control(&lower, leaf) {
        return Some(SecurityControlKind::Sanitization);
    }
    None
}

fn is_validation_control(callee_path: &str, leaf: &str) -> bool {
    (matches!(
        leaf,
        "parse" | "safeparse" | "validate" | "validateasync" | "assert" | "check" | "is"
    ) && matches!(
        control_object(callee_path),
        Some("z")
            | Some("zod")
            | Some("joi")
            | Some("yup")
            | Some("valibot")
            | Some("v")
            | Some("superstruct")
            | Some("schema")
    )) || matches!(leaf, "validatesync" | "parseasync" | "safeparseasync")
}

fn is_authentication_control(leaf: &str) -> bool {
    leaf == "authenticate"
        || leaf == "ensureauthenticated"
        || leaf == "requireauth"
        || leaf == "requireuser"
        || leaf == "authguard"
        || leaf == "verifytoken"
        || leaf == "auth"
}

fn is_authorization_control(leaf: &str) -> bool {
    leaf == "authorize"
        || leaf == "requirepermission"
        || leaf == "requirepermissions"
        || leaf == "requirerole"
        || leaf == "can"
        || leaf == "permit"
        || leaf == "enforce"
}

fn is_sanitization_control(callee_path: &str, leaf: &str) -> bool {
    leaf == "sanitize"
        || leaf == "escape"
        || leaf == "escaperegexp"
        || callee_path.ends_with(".sanitize")
}

fn control_object(callee_path: &str) -> Option<&str> {
    callee_path
        .rsplit_once('.')
        .map(|(object, _)| object.rsplit('.').next().unwrap_or(object))
}

fn import_source_matches_package(source: &str, package: &str) -> bool {
    source == package
        || source
            .strip_prefix(package)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn route_method_leaf(callee_path: &str) -> Option<&str> {
    let leaf = callee_path.rsplit('.').next()?;
    matches!(
        leaf,
        "route" | "get" | "post" | "put" | "patch" | "delete" | "all" | "head" | "options"
    )
    .then_some(leaf)
}

fn callee_leaf_name(callee: &Expression<'_>) -> Option<String> {
    match callee {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => Some(member.property.name.to_string()),
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => callee_leaf_name(&call.callee),
            ChainElement::StaticMemberExpression(member) => Some(member.property.name.to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn expression_has_boundary_validation_keys(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::ObjectExpression(obj) => object_has_any_key(obj, &["body", "query", "params"]),
        Expression::ParenthesizedExpression(paren) => {
            expression_has_boundary_validation_keys(&paren.expression)
        }
        Expression::TSAsExpression(ts_as) => {
            expression_has_boundary_validation_keys(&ts_as.expression)
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            expression_has_boundary_validation_keys(&ts_sat.expression)
        }
        _ => false,
    }
}

fn expression_has_fastify_schema(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::ObjectExpression(obj) => object_property_value(obj, "schema")
            .is_some_and(|schema| expression_has_boundary_validation_keys(schema)),
        Expression::ParenthesizedExpression(paren) => {
            expression_has_fastify_schema(&paren.expression)
        }
        Expression::TSAsExpression(ts_as) => expression_has_fastify_schema(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => {
            expression_has_fastify_schema(&ts_sat.expression)
        }
        _ => false,
    }
}

fn object_has_any_key(obj: &ObjectExpression<'_>, keys: &[&str]) -> bool {
    obj.properties.iter().any(|prop| {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return false;
        };
        prop.key
            .static_name()
            .is_some_and(|name| keys.iter().any(|key| name == *key))
    })
}

fn object_property_value<'a>(
    obj: &'a ObjectExpression<'a>,
    key: &str,
) -> Option<&'a Expression<'a>> {
    obj.properties.iter().find_map(|prop| {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return None;
        };
        prop.key
            .static_name()
            .is_some_and(|name| name == key)
            .then_some(&prop.value)
    })
}
