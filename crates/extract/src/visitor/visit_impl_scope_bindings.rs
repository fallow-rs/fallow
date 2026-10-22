//! Scope-local binding state helpers for the visitor implementation.

#[allow(clippy::wildcard_imports, reason = "many scope helper AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_semantic::ScopeFlags;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{SanitizerScope, SinkLiteralValue};

use super::super::helpers::{array_element_type_from_type, map_value_type_from_type};
use super::super::{ModuleInfoExtractor, SecurityPathSinkBinding};
use super::{sink_literal_value, static_sink_literal_to_string, unwrap_static_expr};

#[derive(Default)]
struct FunctionVarRootCollector {
    roots: FxHashSet<String>,
}

impl<'a> Visit<'a> for FunctionVarRootCollector {
    fn visit_variable_declaration(&mut self, declaration: &VariableDeclaration<'a>) {
        if declaration.kind.is_var() {
            self.roots.extend(
                declaration
                    .declarations
                    .iter()
                    .flat_map(|declarator| declarator.id.get_binding_identifiers())
                    .map(|id| id.name.to_string()),
            );
        }
    }

    fn visit_function(&mut self, _function: &Function<'a>, _flags: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _expression: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _class: &Class<'a>) {}

    fn visit_ts_namespace_declaration(&mut self, _declaration: &TSNamespaceDeclaration<'a>) {}

    fn visit_ts_external_module_declaration(
        &mut self,
        _declaration: &TSExternalModuleDeclaration<'a>,
    ) {
    }

    fn visit_ts_global_declaration(&mut self, _declaration: &TSGlobalDeclaration<'a>) {}
}

/// Whether a nested binding initialized with `expr` provably holds no class
/// instance: a primitive, a function, or an array or object literal built
/// only from such values. A type assertion can name a class, so it gives no
/// proof.
fn expression_holds_no_instance(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            expression_holds_no_instance(&paren.expression)
        }
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::TemplateLiteral(_)
        | Expression::FunctionExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::ClassExpression(_) => true,
        Expression::ArrayExpression(array) => array.elements.iter().all(|element| {
            element
                .as_expression()
                .is_some_and(expression_holds_no_instance)
        }),
        Expression::ObjectExpression(object) => {
            object.properties.iter().all(|property| match property {
                ObjectPropertyKind::ObjectProperty(property) => {
                    expression_holds_no_instance(&property.value)
                }
                ObjectPropertyKind::SpreadProperty(_) => false,
            })
        }
        _ => false,
    }
}

/// Whether a binding annotated with `ty` provably holds no class instance: a
/// primitive, a literal or function type, or an array or union built only from
/// such types. TypeScript types are structural, so a class instance satisfies
/// a named type or a type literal with matching members. Neither gives proof.
fn type_holds_no_instance(ty: &TSType<'_>) -> bool {
    match ty {
        TSType::TSStringKeyword(_)
        | TSType::TSNumberKeyword(_)
        | TSType::TSBooleanKeyword(_)
        | TSType::TSBigIntKeyword(_)
        | TSType::TSSymbolKeyword(_)
        | TSType::TSNullKeyword(_)
        | TSType::TSUndefinedKeyword(_)
        | TSType::TSVoidKeyword(_)
        | TSType::TSNeverKeyword(_)
        | TSType::TSLiteralType(_)
        | TSType::TSTemplateLiteralType(_)
        | TSType::TSFunctionType(_) => true,
        TSType::TSParenthesizedType(paren) => type_holds_no_instance(&paren.type_annotation),
        TSType::TSArrayType(array) => type_holds_no_instance(&array.element_type),
        TSType::TSUnionType(union) => union.types.iter().all(type_holds_no_instance),
        _ => false,
    }
}

/// Whether a nested declarator binds a value that provably holds no class
/// instance, through its type annotation or its initializer.
fn declarator_holds_no_instance(declarator: &VariableDeclarator<'_>) -> bool {
    if let Some(annotation) = declarator.type_annotation.as_deref() {
        return type_holds_no_instance(&annotation.type_annotation);
    }
    declarator
        .init
        .as_ref()
        .is_some_and(expression_holds_no_instance)
}

impl ModuleInfoExtractor {
    pub(super) fn is_module_scope(&self) -> bool {
        self.block_depth == 0 && self.function_depth == 0 && self.namespace_depth == 0
    }

    pub(in crate::visitor) fn is_module_or_function_runtime_scope(&self) -> bool {
        self.namespace_depth == 0
    }

    /// Whether the root of `object` names a nested binding that provably holds
    /// no class instance. `binding_target_names` is module-flat, so a read of
    /// such a shadow must not resolve through the class binding of the same
    /// name in an outer or sibling scope.
    pub(in crate::visitor) fn class_binding_root_is_shadowed(&self, object: &str) -> bool {
        let root = object.split_once('.').map_or(object, |(root, _)| root);
        self.nested_declaration_stack
            .iter()
            .rposition(|scope| scope.contains(root))
            .and_then(|index| self.scoped_non_instance_bindings.get(index))
            .is_some_and(|scope| scope.contains(root))
    }

    /// Record the nested declarators of `var` that provably hold no class
    /// instance. Call it together with `record_nested_declaration_names`.
    pub(super) fn record_non_instance_declarators(&mut self, var: &VariableDeclaration<'_>) {
        if self.namespace_depth > 0 {
            return;
        }
        let Some(scope) = self.scoped_non_instance_bindings.last_mut() else {
            return;
        };
        for declarator in &var.declarations {
            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && declarator_holds_no_instance(declarator)
            {
                scope.insert(id.name.to_string());
            }
        }
    }

    /// A class binding under the root of `binding` disproves the shadow that
    /// the declaring scope recorded for the root.
    pub(in crate::visitor) fn clear_non_instance_binding(&mut self, binding: &str) {
        let root = binding.split_once('.').map_or(binding, |(root, _)| root);
        if let Some(index) = self
            .nested_declaration_stack
            .iter()
            .rposition(|scope| scope.contains(root))
            && let Some(scope) = self.scoped_non_instance_bindings.get_mut(index)
        {
            scope.remove(root);
        }
    }

    /// Record a read of `member` on the name path `object`. A receiver whose
    /// root is a nested shadow keeps its raw access but never resolves through
    /// the module-flat class bindings.
    pub(in crate::visitor) fn push_receiver_member_access(
        &mut self,
        object: String,
        member: String,
    ) {
        if self.class_binding_root_is_shadowed(&object) {
            self.shadowed_member_access_indices
                .insert(self.member_accesses.len());
        }
        self.member_accesses
            .push(crate::MemberAccess { object, member });
    }

    pub(in crate::visitor) fn insert_class_binding_target(
        &mut self,
        binding: String,
        target: String,
    ) {
        self.clear_non_instance_binding(&binding);
        self.walk_order_class_bindings
            .insert(binding.clone(), target.clone());
        let target = super::super::BindingTarget::Class(target);
        self.binding_target_names
            .entry(binding)
            .and_modify(|existing| {
                if *existing != target {
                    *existing = super::super::BindingTarget::Ambiguous;
                }
            })
            .or_insert(target);
    }

    pub(in crate::visitor) fn insert_class_binding_target_if_absent(
        &mut self,
        binding: String,
        target: String,
    ) {
        self.clear_non_instance_binding(&binding);
        if !self.binding_target_names.contains_key(&binding) {
            self.walk_order_class_bindings
                .insert(binding.clone(), target.clone());
        }
        self.binding_target_names
            .entry(binding)
            .or_insert(super::super::BindingTarget::Class(target));
    }

    pub(super) fn nested_scope_shadows(&self, name: &str) -> bool {
        self.nested_declaration_stack
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
    }

    /// Whether a dotted receiver (`Ns.Class`) starts with an import local that
    /// a nested declaration shadows. The analysis credits `Ns.Class.member`
    /// through the import of `Ns`, so a shadowed root must not emit the access.
    pub(in crate::visitor) fn namespace_import_root_is_shadowed(&self, object: &str) -> bool {
        object.split_once('.').is_some_and(|(root, _)| {
            self.nested_scope_shadows(root)
                && (self.namespace_import_locals.contains(root)
                    || self.imports.iter().any(|import| import.local_name == root))
        })
    }

    pub(in crate::visitor) fn namespace_like_binding_is_shadowed(&self, name: &str) -> bool {
        if let Some(owner) = self
            .scoped_namespace_binding_names
            .iter()
            .rposition(|scope| scope.contains(name))
        {
            return self
                .nested_declaration_stack
                .iter()
                .skip(owner + 1)
                .any(|scope| scope.contains(name));
        }
        if self.module_namespace_binding_names.contains(name)
            || self.namespace_import_locals.contains(name)
            || self.default_import_locals.contains(name)
        {
            return self.nested_scope_shadows(name);
        }
        self.namespace_binding_names
            .iter()
            .any(|local| local == name)
    }

    pub(in crate::visitor) fn record_namespace_binding_name(&mut self, name: String) {
        self.namespace_binding_names.push(name.clone());
        if let Some(scope) = self.scoped_namespace_binding_names.last_mut() {
            scope.insert(name);
        } else {
            self.module_namespace_binding_names.insert(name);
        }
    }

    /// Record a namespace binding that an assignment fills, in the scope that
    /// declares the binding rather than the scope of the assignment. A reader
    /// in a sibling function then sees the same binding, not a shadow.
    pub(in crate::visitor) fn record_assigned_namespace_binding_name(&mut self, name: String) {
        self.namespace_binding_names.push(name.clone());
        let declaring_scope = self
            .nested_declaration_stack
            .iter()
            .rposition(|scope| scope.contains(&name));
        match declaring_scope.and_then(|index| self.scoped_namespace_binding_names.get_mut(index)) {
            Some(scope) => {
                scope.insert(name);
            }
            None => {
                self.module_namespace_binding_names.insert(name);
            }
        }
    }

    pub(super) fn record_sanitizer_binding(&mut self, name: &str, scope: Option<SanitizerScope>) {
        if self.is_module_scope() {
            self.module_sanitizer_bindings
                .insert(name.to_string(), scope);
            return;
        }
        if let Some(bindings) = self.sanitizer_binding_stack.last_mut() {
            bindings.insert(name.to_string(), scope);
        }
    }

    pub(super) fn record_literal_allowlist_binding(&mut self, name: &str, trusted: bool) {
        if self.is_module_scope() {
            self.module_literal_allowlist_bindings
                .insert(name.to_string(), trusted);
            return;
        }
        if let Some(bindings) = self.literal_allowlist_binding_stack.last_mut() {
            bindings.insert(name.to_string(), trusted);
        }
    }

    pub(super) fn literal_allowlist_binding(&self, name: &str) -> bool {
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

    pub(super) fn record_static_sink_literal_binding(
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

    pub(super) fn static_sink_literal_value(
        &self,
        expr: &Expression<'_>,
    ) -> Option<SinkLiteralValue> {
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

    pub(super) fn record_risky_regex_binding(&mut self, name: &str, pattern: Option<String>) {
        if self.is_module_scope() {
            self.module_risky_regex_bindings
                .insert(name.to_string(), pattern);
            return;
        }
        if let Some(bindings) = self.risky_regex_binding_stack.last_mut() {
            bindings.insert(name.to_string(), pattern);
        }
    }

    pub(super) fn risky_regex_binding(&self, name: &str) -> Option<&str> {
        for bindings in self.risky_regex_binding_stack.iter().rev() {
            if let Some(pattern) = bindings.get(name) {
                return pattern.as_deref();
            }
        }
        self.module_risky_regex_bindings
            .get(name)
            .and_then(Option::as_deref)
    }

    pub(super) fn record_path_sink_binding(
        &mut self,
        name: &str,
        binding: Option<SecurityPathSinkBinding>,
    ) {
        if self.is_module_scope() {
            self.module_path_sink_bindings
                .insert(name.to_string(), binding);
            return;
        }
        if let Some(bindings) = self.path_sink_binding_stack.last_mut() {
            bindings.insert(name.to_string(), binding);
        }
    }

    pub(super) fn path_sink_binding(&self, name: &str) -> Option<SecurityPathSinkBinding> {
        for bindings in self.path_sink_binding_stack.iter().rev() {
            if let Some(binding) = bindings.get(name) {
                return *binding;
            }
        }
        self.module_path_sink_bindings
            .get(name)
            .and_then(|binding| *binding)
    }

    pub(super) fn record_path_relative_binding(&mut self, name: &str, target: Option<String>) {
        if self.is_module_scope() {
            self.module_path_relative_bindings
                .insert(name.to_string(), target);
            return;
        }
        if let Some(bindings) = self.path_relative_binding_stack.last_mut() {
            bindings.insert(name.to_string(), target);
        }
    }

    pub(super) fn path_relative_binding(&self, name: &str) -> Option<&str> {
        for bindings in self.path_relative_binding_stack.iter().rev() {
            if let Some(target) = bindings.get(name) {
                return target.as_deref();
            }
        }
        self.module_path_relative_bindings
            .get(name)
            .and_then(Option::as_deref)
    }

    pub(super) fn sanitizer_scope_for_identifier(&self, name: &str) -> Option<SanitizerScope> {
        for bindings in self.sanitizer_binding_stack.iter().rev() {
            if let Some(scope) = bindings.get(name) {
                return *scope;
            }
        }
        self.module_sanitizer_bindings
            .get(name)
            .and_then(|scope| *scope)
    }

    pub(super) fn record_nested_declaration_names<'a>(
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

    pub(super) fn var_binding_roots<'a>(
        statements: &oxc_allocator::Vec<'a, Statement<'a>>,
    ) -> FxHashSet<String> {
        let mut collector = FunctionVarRootCollector::default();
        collector.visit_statements(statements);
        collector.roots
    }

    pub(super) fn push_function_declaration_scope(&mut self, params: &FormalParameters<'_>) {
        if let Some(rest) = &params.rest {
            self.record_direct_object_binding_scope_roots(
                rest.rest
                    .argument
                    .get_binding_identifiers()
                    .into_iter()
                    .map(|id| id.name.to_string()),
            );
        }
        if self.namespace_depth > 0 {
            return;
        }

        let mut scope = FxHashSet::default();
        let mut non_instance_scope = FxHashSet::default();
        // Array-typed parameters (`items: Item[]`) mapped to their element class,
        // so a `for...of` loop or `.map`/`.forEach` callback over the parameter in
        // the function body credits the item's member accesses onto the class.
        // Recorded here (before the body walk) rather than in
        // `visit_formal_parameter`: at param-visit time the function-body scoped
        // frame is not yet pushed, so a top-level function's array params would be
        // silently dropped. See issue #1793.
        let mut array_element_scope: FxHashMap<String, String> = FxHashMap::default();
        let mut map_value_scope: FxHashMap<String, String> = FxHashMap::default();
        for param in &params.items {
            scope.extend(
                param
                    .pattern
                    .get_binding_identifiers()
                    .into_iter()
                    .map(|id| id.name.to_string()),
            );
            if let BindingPattern::BindingIdentifier(id) = &param.pattern
                && let Some(type_annotation) = param.type_annotation.as_deref()
                && type_holds_no_instance(&type_annotation.type_annotation)
            {
                non_instance_scope.insert(id.name.to_string());
            }
            if let BindingPattern::BindingIdentifier(id) = &param.pattern
                && let Some(type_annotation) = param.type_annotation.as_deref()
                && let Some(element) =
                    array_element_type_from_type(&type_annotation.type_annotation)
            {
                array_element_scope.insert(id.name.to_string(), element);
            }
            if let BindingPattern::BindingIdentifier(id) = &param.pattern
                && let Some(type_annotation) = param.type_annotation.as_deref()
                && let Some(value) = map_value_type_from_type(&type_annotation.type_annotation)
                && let Some(value) = self.resolve_class_type_param(&value)
            {
                map_value_scope.insert(id.name.to_string(), value);
            }
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
        self.scoped_non_instance_bindings.push(non_instance_scope);
        self.scoped_namespace_binding_names
            .push(FxHashSet::default());
        self.scoped_array_binding_element_types
            .push(array_element_scope);
        if let Some(map_scope) = self.scoped_map_binding_value_types.last_mut() {
            map_scope.extend(map_value_scope);
        }
        self.sanitizer_binding_stack.push(sanitizer_scope);
        self.literal_allowlist_binding_stack.push(allowlist_scope);
        self.risky_regex_binding_stack.push(risky_regex_scope);
        self.path_sink_binding_stack.push(path_sink_scope);
        self.path_relative_binding_stack.push(path_relative_scope);
    }

    pub(super) fn pop_function_declaration_scope(&mut self) {
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.pop();
            self.scoped_non_instance_bindings.pop();
            self.scoped_namespace_binding_names.pop();
            self.scoped_array_binding_element_types.pop();
            self.sanitizer_binding_stack.pop();
            self.literal_allowlist_binding_stack.pop();
            self.risky_regex_binding_stack.pop();
            self.path_sink_binding_stack.pop();
            self.path_relative_binding_stack.pop();
        }
    }

    pub(super) fn push_direct_object_binding_scope(&mut self, roots: FxHashSet<String>) {
        self.direct_object_binding_generation =
            self.direct_object_binding_generation.saturating_add(1);
        self.scoped_direct_object_binding_roots.push(roots);
        self.scoped_map_binding_value_types
            .push(FxHashMap::default());
        self.scoped_direct_object_binding_targets
            .push(FxHashMap::default());
        self.scoped_direct_object_binding_generations
            .push(FxHashMap::default());
        self.scoped_direct_object_binding_paths_by_root
            .push(FxHashMap::default());
    }

    pub(super) fn push_var_owner_direct_object_binding_scope(&mut self, roots: FxHashSet<String>) {
        self.push_direct_object_binding_scope(roots);
    }

    pub(super) fn pop_direct_object_binding_scope(&mut self) {
        self.direct_object_binding_generation =
            self.direct_object_binding_generation.saturating_add(1);
        self.scoped_direct_object_binding_roots.pop();
        self.scoped_map_binding_value_types.pop();
        self.scoped_direct_object_binding_targets.pop();
        self.scoped_direct_object_binding_generations.pop();
        self.scoped_direct_object_binding_paths_by_root.pop();
    }

    pub(super) fn record_direct_object_binding_scope_roots(
        &mut self,
        roots: impl IntoIterator<Item = String>,
    ) {
        if let Some(scope) = self.scoped_direct_object_binding_roots.last_mut() {
            let previous_len = scope.len();
            scope.extend(roots);
            if scope.len() != previous_len {
                self.direct_object_binding_generation =
                    self.direct_object_binding_generation.saturating_add(1);
            }
        }
    }

    pub(super) fn record_node_module_register_url_binding(
        &mut self,
        name: String,
        sources: Vec<String>,
    ) {
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

    pub(super) fn node_module_register_url_binding(&self, name: &str) -> Vec<String> {
        self.node_module_register_url_bindings
            .get(name)
            .cloned()
            .unwrap_or_default()
    }
}
