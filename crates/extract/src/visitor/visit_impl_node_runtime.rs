#[allow(
    clippy::wildcard_imports,
    reason = "many Node runtime helper AST types used"
)]
use oxc_ast::ast::*;

use crate::js_text::JsText;
use crate::{DynamicImportInfo, ImportedName};
use fallow_types::extract::ImportLoadKind;

use super::super::ModuleInfoExtractor;
use super::super::declarations::ModulePathBinding;
use super::{
    collect_pino_config_targets, extract_object_pattern_bindings, is_child_process_source,
    is_meta_url_arg, is_node_path_source, is_node_url_source, loader_hook_exports_for_source,
    local_fork_source, new_url_import_source, node_module_register_specifier,
    normalize_module_file_relative_path, pino_factory_callee_name,
};

/// The CommonJS globals that name a path of the current module.
const MODULE_PATH_GLOBALS: &[&str] = &["__filename", "__dirname"];

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
                value_escapes: false,
            });
            self.mark_import_load_kind(expr.span, ImportLoadKind::OutOfThread);
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
                value_escapes: false,
            });
            self.mark_import_load_kind(expr.span, ImportLoadKind::OutOfThread);
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
        let binding = if self.is_current_module_dir_expression(expr) {
            ModulePathBinding::Directory
        } else if self.is_current_module_file_expression(expr) {
            ModulePathBinding::File
        } else if MODULE_PATH_GLOBALS.contains(&name) {
            ModulePathBinding::Other
        } else {
            return;
        };
        self.current_module_path_bindings
            .insert(name.to_string(), binding);
    }

    /// What `name` holds at the current scope when it names a path of the
    /// current module. The CommonJS globals `__filename` and `__dirname`
    /// count unless an import or a declaration rebinds them.
    fn module_path_binding(&self, name: &str) -> Option<ModulePathBinding> {
        if self.nested_scope_shadows(name) {
            return None;
        }
        if let Some(binding) = self.current_module_path_bindings.get(name) {
            return Some(*binding);
        }
        let imported = self.imports.iter().any(|import| import.local_name == name)
            || self
                .require_calls
                .iter()
                .any(|require| require.local_name.as_deref() == Some(name));
        match name {
            _ if imported => None,
            "__filename" => Some(ModulePathBinding::File),
            "__dirname" => Some(ModulePathBinding::Directory),
            _ => None,
        }
    }

    /// Whether `callee` is `fileURLToPath` from `node:url`, called by its
    /// imported name or through the `url` namespace.
    fn is_file_url_to_path_callee(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(ident) => {
                self.node_url_file_url_to_path_bindings
                    .contains(ident.name.as_str())
                    && !self.nested_scope_shadows(ident.name.as_str())
            }
            Expression::StaticMemberExpression(member) => {
                member.property.name == "fileURLToPath"
                    && matches!(&member.object, Expression::Identifier(obj)
                        if self.node_url_file_url_to_path_bindings.contains(obj.name.as_str())
                            && !self.nested_scope_shadows(obj.name.as_str()))
            }
            _ => false,
        }
    }

    /// Whether `callee` is one of the `names` functions of the `node:path`
    /// namespace, such as `path.join`.
    fn is_node_path_call(&self, callee: &Expression<'_>, names: &[&str]) -> bool {
        let Expression::StaticMemberExpression(member) = callee else {
            return false;
        };
        names.contains(&member.property.name.as_str())
            && matches!(&member.object, Expression::Identifier(object)
                if self.node_path_namespace_bindings.contains(object.name.as_str())
                    && !self.nested_scope_shadows(object.name.as_str()))
    }

    /// Whether `expr` is the file path of the current module: `__filename`,
    /// `fileURLToPath(import.meta.url)`, or a binding of one of them.
    fn is_current_module_file_expression(&self, expr: &Expression<'_>) -> bool {
        match expr {
            Expression::Identifier(ident) => {
                self.module_path_binding(ident.name.as_str()) == Some(ModulePathBinding::File)
            }
            Expression::CallExpression(call) => {
                self.is_file_url_to_path_callee(&call.callee)
                    && call.arguments.first().is_some_and(is_meta_url_arg)
            }
            _ => false,
        }
    }

    /// Whether `expr` is the directory of the current module: `__dirname`,
    /// `import.meta.dirname`, `path.dirname(<current module file>)`, or a
    /// binding of one of them.
    fn is_current_module_dir_expression(&self, expr: &Expression<'_>) -> bool {
        match expr {
            Expression::Identifier(ident) => {
                self.module_path_binding(ident.name.as_str()) == Some(ModulePathBinding::Directory)
            }
            Expression::StaticMemberExpression(member) => {
                member.property.name == "dirname"
                    && matches!(&member.object, Expression::ImportMeta(_))
            }
            Expression::CallExpression(call) => {
                self.is_node_path_call(&call.callee, &["dirname"])
                    && call.arguments.len() == 1
                    && call
                        .arguments
                        .first()
                        .and_then(Argument::as_expression)
                        .is_some_and(|arg| self.is_current_module_file_expression(arg))
            }
            Expression::ParenthesizedExpression(paren) => {
                self.is_current_module_dir_expression(&paren.expression)
            }
            _ => false,
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
            Expression::StringLiteral(lit) => local_fork_source(&lit.value.text())
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
        if let Some(source) = self.module_dir_join_source(call) {
            return local_fork_source(&source).into_iter().collect();
        }
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
        if !call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .is_some_and(|base| self.is_current_module_file_expression(base))
        {
            return Vec::new();
        }
        let Some(Argument::StringLiteral(relative)) = call.arguments.get(1) else {
            return Vec::new();
        };
        normalize_module_file_relative_path(&relative.value.text())
            .and_then(|source| local_fork_source(&source))
            .into_iter()
            .collect()
    }

    /// Resolve `path.join(<module dir>, 'a', 'b.js')` and the same call of
    /// `path.resolve` to a specifier relative to the current module. Every
    /// segment after the directory must be a relative string literal.
    fn module_dir_join_source(&self, call: &CallExpression<'_>) -> Option<String> {
        if !self.is_node_path_call(&call.callee, &["join", "resolve"]) {
            return None;
        }
        let (base, segments) = call.arguments.split_first()?;
        if !base
            .as_expression()
            .is_some_and(|base| self.is_current_module_dir_expression(base))
        {
            return None;
        }
        let mut parts: Vec<String> = Vec::new();
        let mut parents = 0_usize;
        for segment in segments {
            let Argument::StringLiteral(lit) = segment else {
                return None;
            };
            let text = lit.value.text();
            if text.starts_with('/') {
                return None;
            }
            for part in text.split(['/', '\\']) {
                match part {
                    "" | "." => {}
                    ".." => {
                        if parts.pop().is_none() {
                            parents += 1;
                        }
                    }
                    _ => parts.push(part.to_string()),
                }
            }
        }
        if parts.is_empty() {
            return None;
        }
        let prefix = if parents == 0 {
            "./".to_string()
        } else {
            "../".repeat(parents)
        };
        Some(format!("{prefix}{}", parts.join("/")))
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
                value_escapes: false,
            });
            self.mark_import_load_kind(expr.span, ImportLoadKind::OutOfThread);
        }
    }
}
