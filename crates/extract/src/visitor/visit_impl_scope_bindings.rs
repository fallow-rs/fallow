//! Scope-local binding state helpers for the visitor implementation.

#[allow(clippy::wildcard_imports, reason = "many scope helper AST types used")]
use oxc_ast::ast::*;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{SanitizerScope, SinkLiteralValue};

use super::super::{ModuleInfoExtractor, SecurityPathSinkBinding};
use super::{sink_literal_value, static_sink_literal_to_string, unwrap_static_expr};

impl ModuleInfoExtractor {
    pub(super) fn is_module_scope(&self) -> bool {
        self.block_depth == 0 && self.function_depth == 0 && self.namespace_depth == 0
    }

    pub(super) fn is_module_or_function_runtime_scope(&self) -> bool {
        self.namespace_depth == 0
    }

    pub(super) fn nested_scope_shadows(&self, name: &str) -> bool {
        self.nested_declaration_stack
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
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

    pub(super) fn push_function_declaration_scope(&mut self, params: &FormalParameters<'_>) {
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

    pub(super) fn pop_function_declaration_scope(&mut self) {
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.pop();
            self.sanitizer_binding_stack.pop();
            self.literal_allowlist_binding_stack.pop();
            self.risky_regex_binding_stack.pop();
            self.path_sink_binding_stack.pop();
            self.path_relative_binding_stack.pop();
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
