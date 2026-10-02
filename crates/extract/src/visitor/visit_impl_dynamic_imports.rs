//! Dynamic import capture helpers for the visitor implementation.

use super::*;
use crate::visitor::{
    LocalImportLoader, extract_import_from_callable, extract_import_from_return_body,
};

impl<'a> ModuleInfoExtractor {
    /// Record each `new URL(path, import.meta.url)` passed directly to a
    /// filesystem call, so `visit_new_expression` marks it speculative.
    pub(super) fn record_filesystem_path_new_url_arguments(&mut self, expr: &CallExpression<'_>) {
        if !is_filesystem_path_callee(&expr.callee) {
            return;
        }
        for arg in &expr.arguments {
            if let Argument::NewExpression(new_expr) = arg
                && new_url_import_source(new_expr).is_some()
            {
                self.filesystem_path_new_url_spans.insert(new_expr.span);
            }
        }
    }

    fn push_relative_dynamic_import_pattern(&mut self, prefix: String, span: Span) {
        if prefix.starts_with("./") || prefix.starts_with("../") {
            self.dynamic_import_patterns.push(DynamicImportPattern {
                prefix,
                suffix: None,
                span,
                mechanism: ModuleLoadMechanism::EsModule,
            });
        }
    }

    pub(super) fn record_import_meta_glob_patterns(&mut self, expr: &CallExpression<'_>) {
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && member.property.name == "glob"
            && matches!(
                member.object,
                Expression::ImportMeta(_) | Expression::NewTarget(_)
            )
            && let Some(first_arg) = expr.arguments.first()
        {
            if expr.arguments.get(1).is_some_and(is_eager_glob_options) {
                self.mark_import_load_kind(expr.span, ImportLoadKind::Static);
            }
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

    pub(super) fn record_require_context_pattern(&mut self, expr: &CallExpression<'_>) {
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
                    mechanism: ModuleLoadMechanism::CommonJsRequire,
                });
            }
        }
    }

    /// Push one `DynamicImportInfo` edge per statically-resolvable branch of a
    /// dynamic `import()`, every branch sharing the same span and bindings.
    /// Single choke point for the branch fan-out used by the declaration,
    /// bare-expression, `.then`, arrow-wrapped, and route-property paths.
    pub(in crate::visitor) fn push_dynamic_import_branches(
        &mut self,
        sources: &[String],
        span: Span,
        destructured_names: &[String],
        local_name: Option<&str>,
    ) {
        for source in sources {
            self.dynamic_imports.push(DynamicImportInfo {
                source: source.clone(),
                span,
                destructured_names: destructured_names.to_vec(),
                local_name: local_name.map(str::to_string),
                is_speculative: false,
            });
        }
    }

    /// Record a member read directly on an awaited dynamic import, such as
    /// `(await import('./x')).run()` or `new (await import('./x')).Cls()`.
    /// The read member is the only export the expression uses, so each
    /// statically resolvable branch credits that one name. The parent member
    /// expression is visited before the `import()` itself, so marking the span
    /// as handled stops `visit_import_expression` from adding a second edge.
    pub(super) fn record_awaited_dynamic_import_member(
        &mut self,
        object: &Expression<'_>,
        member: &str,
    ) {
        let Expression::AwaitExpression(await_expr) = object.without_parentheses() else {
            return;
        };
        let Expression::ImportExpression(import_expr) = await_expr.argument.without_parentheses()
        else {
            return;
        };
        if self.handled_import_spans.contains(&import_expr.span) {
            return;
        }
        let mut sources = Vec::new();
        collect_static_import_specifiers(&import_expr.source, &mut sources);
        if sources.is_empty() {
            return;
        }
        self.push_dynamic_import_branches(&sources, import_expr.span, &[member.to_string()], None);
        self.handled_import_spans.insert(import_expr.span);
    }

    pub(super) fn record_import_callback_dynamic_imports(&mut self, expr: &CallExpression<'_>) {
        if let Some(then_cb) = try_extract_import_then_callback(expr) {
            if let Some(local) = &then_cb.local_name {
                self.record_namespace_binding_name(local.clone());
            }
            self.handled_import_spans.insert(then_cb.import_span);
            self.push_dynamic_import_branches(
                &then_cb.sources,
                then_cb.import_span,
                &then_cb.destructured_names,
                then_cb.local_name.as_deref(),
            );
        }
    }

    /// Record a namespace edge for a `<receiver>.ssrLoadModule('<literal>')`
    /// call whose result has no binding. A declaration such as
    /// `const m = await server.ssrLoadModule(...)` records the edge with its
    /// bindings first and marks the call span as handled.
    pub(super) fn record_ssr_load_module(&mut self, expr: &CallExpression<'_>) {
        if self.handled_import_spans.contains(&expr.span) {
            return;
        }
        if let Some(source) = ssr_load_module_source(expr) {
            self.push_dynamic_import_branches(&[source], expr.span, &[], None);
        }
    }

    pub(super) fn record_arrow_wrapped_dynamic_import(&mut self, expr: &CallExpression<'_>) {
        self.record_loader_argument_dynamic_imports(expr);
        if let Some((import_expr, sources)) = try_extract_arrow_wrapped_import(&expr.arguments) {
            self.push_dynamic_import_branches(
                &sources,
                import_expr.span,
                &["default".to_string()],
                None,
            );
            self.handled_import_spans.insert(import_expr.span);

            // Record the `import()` span when this is a
            // `next/dynamic(() => import('./X'), { ssr: false })` call. ssr:false
            // is Next.js's sanctioned client-only escape hatch, so the security
            // `client-server-leak` BFS must not treat a server-only module reached
            // only through it as a leak.
            if self.is_next_dynamic_ssr_false_call(expr) {
                self.client_only_dynamic_import_spans
                    .push(import_expr.span.start);
            }
        }
    }

    /// Credit `default` for each argument that names a local loader function,
    /// the same credit an inline `() => import('./x')` argument gets
    /// (`const load = () => import('./x'); lazy(load)`).
    fn record_loader_argument_dynamic_imports(&mut self, expr: &CallExpression<'_>) {
        if self.local_import_loaders.is_empty() {
            return;
        }
        for arg in &expr.arguments {
            let Argument::Identifier(ident) = arg else {
                continue;
            };
            let Some(loader) = self.local_import_loaders.get_mut(ident.name.as_str()) else {
                continue;
            };
            loader.credited_references += 1;
            let import_span = loader.import_span;
            let sources = loader.sources.clone();
            self.push_dynamic_import_branches(
                &sources,
                import_span,
                &["default".to_string()],
                None,
            );
            if self.is_next_dynamic_ssr_false_call(expr) {
                self.client_only_dynamic_import_spans
                    .push(import_span.start);
            }
        }
    }

    /// Bind `const m = await load()` and `const { a } = await load()` to the
    /// `import()` that the local loader `load` returns, the same way as
    /// `const m = await import('./x')`. Returns whether the declarator had
    /// this shape.
    pub(super) fn record_loader_call_declaration(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) -> bool {
        if self.local_import_loaders.is_empty()
            || !matches!(
                declarator.id,
                BindingPattern::ObjectPattern(_) | BindingPattern::BindingIdentifier(_)
            )
        {
            return false;
        }
        let Expression::AwaitExpression(await_expr) = init.without_parentheses() else {
            return false;
        };
        let Expression::CallExpression(call) = await_expr.argument.without_parentheses() else {
            return false;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return false;
        };
        let Some(loader) = self.local_import_loaders.get_mut(callee.name.as_str()) else {
            return false;
        };
        loader.credited_references += 1;
        let import_span = loader.import_span;
        let sources = loader.sources.clone();
        self.handle_dynamic_import_declaration(&declarator.id, import_span, &sources);
        true
    }

    /// Count one reference to a local loader function. The end of the program
    /// walk compares this count with the references that credit precise names.
    pub(super) fn count_local_import_loader_reference(&mut self, name: &str) {
        if let Some(loader) = self.local_import_loaders.get_mut(name) {
            loader.references += 1;
        }
    }

    /// Register the top-level local loader functions of `program` before the
    /// body walk. A loader is a `const` arrow or function expression, or a
    /// function declaration, that returns a static `import()`. The `import()`
    /// span is marked as handled: the loader references decide its credit.
    pub(super) fn record_program_local_import_loaders(&mut self, program: &Program<'_>) {
        self.local_import_loaders.clear();
        let mut exported_names: Vec<String> = Vec::new();
        for stmt in &program.body {
            match stmt {
                Statement::VariableDeclaration(decl) => self.register_variable_loaders(decl, false),
                Statement::FunctionDeclaration(func) => self.register_function_loader(func, false),
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::VariableDeclaration(decl) => {
                        self.register_variable_loaders(decl, true);
                    }
                    Declaration::FunctionDeclaration(func) => {
                        self.register_function_loader(func, true);
                    }
                    _ => {}
                },
                Statement::ExportNamedDeclaration(export) => {
                    exported_names.extend(
                        export
                            .specifiers
                            .iter()
                            .map(|spec| spec.local.name().to_string()),
                    );
                }
                _ => {}
            }
        }
        for name in exported_names {
            if let Some(loader) = self.local_import_loaders.get_mut(&name) {
                loader.is_exported = true;
            }
        }
    }

    fn register_variable_loaders(&mut self, decl: &VariableDeclaration<'_>, is_exported: bool) {
        if decl.kind != VariableDeclarationKind::Const {
            return;
        }
        for declarator in &decl.declarations {
            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && let Some(init) = &declarator.init
                && let Some(import_expr) = extract_import_from_callable(init.without_parentheses())
            {
                self.register_local_import_loader(id.name.as_str(), import_expr, is_exported);
            }
        }
    }

    fn register_function_loader(&mut self, func: &Function<'_>, is_exported: bool) {
        if let Some(id) = &func.id
            && let Some(body) = &func.body
            && let Some(import_expr) = extract_import_from_return_body(&body.statements)
        {
            self.register_local_import_loader(id.name.as_str(), import_expr, is_exported);
        }
    }

    fn register_local_import_loader(
        &mut self,
        name: &str,
        import_expr: &ImportExpression<'_>,
        is_exported: bool,
    ) {
        if self.local_import_loaders.contains_key(name) {
            return;
        }
        let mut sources = Vec::new();
        collect_static_import_specifiers(&import_expr.source, &mut sources);
        if sources.is_empty() {
            return;
        }
        self.handled_import_spans.insert(import_expr.span);
        self.local_import_loaders.insert(
            name.to_string(),
            LocalImportLoader {
                import_span: import_expr.span,
                sources,
                references: 0,
                credited_references: 0,
                is_exported,
            },
        );
    }

    /// Give each local loader that has a reference in an unknown shape, no
    /// credited reference, or an export the whole-module credit. The loader
    /// result can reach code that the walk cannot follow, so this credit can
    /// only remove false positives. A loader without a reference and without
    /// an export gets a bare edge: the target stays reachable, but the dead
    /// loader credits no export.
    pub(super) fn finish_local_import_loaders(&mut self) {
        let mut loaders: Vec<(String, LocalImportLoader)> =
            std::mem::take(&mut self.local_import_loaders)
                .into_iter()
                .collect();
        loaders.sort_unstable_by_key(|(_, loader)| loader.import_span.start);
        for (name, loader) in loaders {
            if loader.references == 0 && !loader.is_exported {
                self.push_dynamic_import_branches(&loader.sources, loader.import_span, &[], None);
                continue;
            }
            if loader.is_exported
                || loader.credited_references == 0
                || loader.references > loader.credited_references
            {
                self.push_dynamic_import_branches(
                    &loader.sources,
                    loader.import_span,
                    &[],
                    Some(&name),
                );
                self.push_whole_object_use(name);
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
    fn is_default_import_from(&self, local_name: &str, source: &str) -> bool {
        self.imports.iter().any(|import| {
            import.source == source
                && import.local_name == local_name
                && matches!(import.imported_name, ImportedName::Default)
        })
    }

    /// Record a dynamic-import glob pattern from an interpolated template
    /// literal (`import(\`./views/${name}.js\`)`): a relative-prefixed quasi
    /// becomes a `DynamicImportPattern`; multiple interpolations widen the
    /// prefix with a recursive `**/` segment.
    pub(super) fn record_dynamic_import_template_pattern(
        &mut self,
        tpl: &TemplateLiteral<'a>,
        span: Span,
    ) {
        let first_quasi = tpl.quasis[0].value.raw.to_string();
        if !(first_quasi.starts_with("./") || first_quasi.starts_with("../")) {
            return;
        }
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
            span,
            mechanism: ModuleLoadMechanism::EsModule,
        });
    }
}

/// Whether an `import.meta.glob` options argument sets `eager: true`, which
/// makes Vite import every match before the importing module runs.
fn is_eager_glob_options(argument: &Argument<'_>) -> bool {
    let Argument::ObjectExpression(options) = argument else {
        return false;
    };
    options.properties.iter().any(|property| {
        matches!(
            property,
            ObjectPropertyKind::ObjectProperty(property)
                if property.key.static_name().as_deref() == Some("eager")
                    && matches!(&property.value, Expression::BooleanLiteral(value) if value.value)
        )
    })
}
