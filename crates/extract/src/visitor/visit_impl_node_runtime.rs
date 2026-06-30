#[allow(
    clippy::wildcard_imports,
    reason = "many Node runtime helper AST types used"
)]
use oxc_ast::ast::*;

use crate::{DynamicImportInfo, ImportedName};

use super::super::ModuleInfoExtractor;
use super::{
    collect_pino_config_targets, extract_object_pattern_bindings, is_child_process_source,
    is_meta_url_arg, is_node_path_source, is_node_url_source, loader_hook_exports_for_source,
    local_fork_source, new_url_import_source, node_module_register_specifier,
    normalize_module_file_relative_path, pino_factory_callee_name,
};

impl ModuleInfoExtractor {
    fn is_pino_factory_binding(&self, local_name: &str) -> bool {
        let imported = self.imports.iter().any(|import| {
            import.source == super::PINO_PACKAGE
                && import.local_name == local_name
                && !import.is_type_only
                && match &import.imported_name {
                    ImportedName::Default => true,
                    ImportedName::Named(name) => name == super::PINO_FACTORY_EXPORT,
                    ImportedName::Namespace | ImportedName::SideEffect => false,
                }
        });
        let required = self.require_calls.iter().any(|require| {
            require.source == super::PINO_PACKAGE
                && require.local_name.as_deref() == Some(local_name)
                && require.destructured_names.is_empty()
        });
        (imported || required) && !self.nested_scope_shadows(local_name)
    }

    pub(super) fn try_record_pino_transport_targets(&mut self, expr: &CallExpression<'_>) {
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
    pub(super) fn try_record_node_module_register(&mut self, expr: &CallExpression<'_>) {
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

    pub(super) fn node_module_register_sources_from_expression(
        &self,
        expr: &Expression<'_>,
    ) -> Vec<String> {
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

    pub(super) fn record_child_process_require_binding(
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

    pub(super) fn record_current_module_file_path_binding(
        &mut self,
        name: &str,
        expr: &Expression<'_>,
    ) {
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

    pub(super) fn record_child_process_fork_target_binding(
        &mut self,
        name: &str,
        expr: &Expression<'_>,
    ) {
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

    pub(super) fn try_record_child_process_fork(&mut self, expr: &CallExpression<'_>) {
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
}
