//! Dynamic import capture helpers for the visitor implementation.

use super::*;
use crate::visitor::{
    LoaderImport, LocalImportLoader, collect_loader_imports, collect_returned_imports,
};

/// The default-export contract of an imported component loader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::visitor) enum ComponentLoaderImport {
    Function,
    ReactNamespace,
    VueNamespace,
    NextDynamic,
}

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
                value_escapes: false,
            });
        }
    }

    /// Push the edges of an `import()` without a binding. Only a discarded
    /// value is a plain load. Any other position hands the namespace to code
    /// that the walk does not follow, so the edge credits the whole module.
    pub(super) fn push_unbound_dynamic_import_branches(&mut self, sources: &[String], span: Span) {
        let value_escapes = !self.discarded_import_spans.contains(&span);
        for source in sources {
            self.dynamic_imports.push(DynamicImportInfo {
                source: source.clone(),
                span,
                destructured_names: Vec::new(),
                local_name: None,
                is_speculative: false,
                value_escapes,
            });
        }
    }

    /// Mark each `import()` whose value an expression statement discards:
    /// `import('./x')`, `await import('./x')`, `void import('./x')`, a
    /// `.catch()`, `.finally()` or parameterless `.then()` handler on it,
    /// `await Promise.all([import('./x')])`, and the operands of `&&`, `||`,
    /// `??`, `?:` and `,`. These forms only load the module and read no export.
    pub(super) fn record_discarded_dynamic_import(&mut self, expression: &Expression<'_>) {
        match expression.without_parentheses() {
            Expression::ImportExpression(import_expr) => {
                self.discarded_import_spans.insert(import_expr.span);
            }
            Expression::AwaitExpression(await_expr) => {
                self.record_discarded_dynamic_import(&await_expr.argument);
            }
            Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::Void => {
                self.record_discarded_dynamic_import(&unary.argument);
            }
            Expression::SequenceExpression(sequence) => {
                for expr in &sequence.expressions {
                    self.record_discarded_dynamic_import(expr);
                }
            }
            Expression::LogicalExpression(logical) => {
                self.record_discarded_dynamic_import(&logical.left);
                self.record_discarded_dynamic_import(&logical.right);
            }
            Expression::ConditionalExpression(conditional) => {
                self.record_discarded_dynamic_import(&conditional.consequent);
                self.record_discarded_dynamic_import(&conditional.alternate);
            }
            Expression::CallExpression(call) => {
                if let Some(elements) = extract_promise_all_elements(expression) {
                    for element in elements {
                        if let Some(expr) = element.as_expression() {
                            self.record_discarded_dynamic_import(expr);
                        }
                    }
                } else if let Expression::StaticMemberExpression(member) = &call.callee
                    && matches!(member.property.name.as_str(), "catch" | "finally")
                {
                    self.record_discarded_dynamic_import(&member.object);
                } else if let Some(object) = parameterless_then_object(call) {
                    self.record_discarded_dynamic_import(object);
                }
            }
            _ => {}
        }
    }

    /// Mark the `import()` of `import('./x').then(() => ...)` as discarded in
    /// any position. The callback has no parameter, so it reads no export, and
    /// the chain resolves to the callback result, not to the namespace.
    pub(super) fn record_parameterless_then_import(&mut self, call: &CallExpression<'_>) {
        if let Some(Expression::ImportExpression(import_expr)) =
            parameterless_then_object(call).map(Expression::without_parentheses)
        {
            self.discarded_import_spans.insert(import_expr.span);
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

    /// Register runtime component-loader imports before visiting calls, including
    /// imports declared later in the file. Local lookalikes provide no contract.
    pub(super) fn record_program_component_loader_imports(&mut self, program: &Program<'_>) {
        self.component_loader_imports.clear();
        for statement in &program.body {
            let Statement::ImportDeclaration(decl) = statement else {
                continue;
            };
            if decl.import_kind.is_type() {
                continue;
            }
            for specifier in decl.specifiers.iter().flatten() {
                let imported = match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(named)
                        if !named.import_kind.is_type() =>
                    {
                        Some(named.imported.name())
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => Some("default".into()),
                    _ => None,
                };
                let contract = match (decl.source.value.as_str(), imported.as_deref(), specifier) {
                    ("react", Some("lazy"), _) | ("vue", Some("defineAsyncComponent"), _) => {
                        Some(ComponentLoaderImport::Function)
                    }
                    ("react", Some("default"), _)
                    | ("react", _, ImportDeclarationSpecifier::ImportNamespaceSpecifier(_)) => {
                        Some(ComponentLoaderImport::ReactNamespace)
                    }
                    ("vue", _, ImportDeclarationSpecifier::ImportNamespaceSpecifier(_)) => {
                        Some(ComponentLoaderImport::VueNamespace)
                    }
                    ("next/dynamic", Some("default"), _) => {
                        Some(ComponentLoaderImport::NextDynamic)
                    }
                    _ => None,
                };
                if let Some(contract) = contract {
                    self.component_loader_imports
                        .insert(specifier.local().name.to_string(), contract);
                }
            }
        }
    }

    fn component_loader_binding_is_shadowed(&self, name: &str) -> bool {
        // Exported namespaces do not maintain the ordinary declaration stack.
        // Other lexical roots also include catch bindings and local namespaces.
        self.namespace_depth > 0
            || self.nested_scope_shadows(name)
            || self
                .scoped_direct_object_binding_roots
                .iter()
                .any(|roots| roots.contains(name))
    }

    fn is_component_loader_call(&self, expr: &CallExpression<'_>) -> bool {
        match expr.callee.without_parentheses() {
            Expression::Identifier(callee) => {
                !self.component_loader_binding_is_shadowed(&callee.name)
                    && matches!(
                        self.component_loader_imports.get(callee.name.as_str()),
                        Some(ComponentLoaderImport::Function | ComponentLoaderImport::NextDynamic)
                    )
            }
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(root) = member.object.without_parentheses() else {
                    return false;
                };
                if self.component_loader_binding_is_shadowed(&root.name) {
                    return false;
                }
                matches!(
                    (
                        self.component_loader_imports.get(root.name.as_str()),
                        member.property.name.as_str()
                    ),
                    (Some(ComponentLoaderImport::ReactNamespace), "lazy")
                        | (
                            Some(ComponentLoaderImport::VueNamespace),
                            "defineAsyncComponent"
                        )
                )
            }
            _ => false,
        }
    }

    pub(super) fn record_arrow_wrapped_dynamic_import(&mut self, expr: &CallExpression<'_>) {
        // Unknown callbacks can return a namespace to arbitrary code. Let the
        // ordinary import walk retain its escaping-value credit instead.
        if !self.is_component_loader_call(expr) {
            return;
        }
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
        let Some(Argument::Identifier(ident)) = expr.arguments.first() else {
            return;
        };
        if self.component_loader_binding_is_shadowed(&ident.name) {
            return;
        }
        let Some(loader) = self.local_import_loaders.get_mut(ident.name.as_str()) else {
            return;
        };
        loader.credited_references += 1;
        let imports = loader.imports.clone();
        let is_client_only = self.is_next_dynamic_ssr_false_call(expr);
        for import in imports {
            self.push_dynamic_import_branches(
                &import.sources,
                import.span,
                &["default".to_string()],
                None,
            );
            if is_client_only {
                self.client_only_dynamic_import_spans
                    .push(import.span.start);
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
        for import in loader.imports.clone() {
            self.handle_dynamic_import_declaration(&declarator.id, import.span, &import.sources);
        }
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
            {
                let imports = collect_loader_imports(init.without_parentheses());
                self.register_local_import_loader(id.name.as_str(), &imports, is_exported);
            }
        }
    }

    fn register_function_loader(&mut self, func: &Function<'_>, is_exported: bool) {
        if let Some(id) = &func.id
            && let Some(body) = &func.body
        {
            let mut imports = Vec::new();
            collect_returned_imports(&body.statements, &mut imports);
            self.register_local_import_loader(id.name.as_str(), &imports, is_exported);
        }
    }

    /// Register `name` as a loader when it returns at least one `import()` and
    /// every returned `import()` has a static specifier. A computed specifier
    /// keeps the old path, so the loader abstains as a whole.
    fn register_local_import_loader(
        &mut self,
        name: &str,
        import_exprs: &[&ImportExpression<'_>],
        is_exported: bool,
    ) {
        if import_exprs.is_empty() || self.local_import_loaders.contains_key(name) {
            return;
        }
        let mut imports = Vec::with_capacity(import_exprs.len());
        for import_expr in import_exprs {
            let mut sources = Vec::new();
            collect_static_import_specifiers(&import_expr.source, &mut sources);
            if sources.is_empty() {
                return;
            }
            imports.push(LoaderImport {
                span: import_expr.span,
                sources,
            });
        }
        for import in &imports {
            self.handled_import_spans.insert(import.span);
        }
        self.local_import_loaders.insert(
            name.to_string(),
            LocalImportLoader {
                imports,
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
        loaders.sort_unstable_by_key(|(_, loader)| loader.imports[0].span.start);
        for (name, loader) in loaders {
            if loader.references == 0 && !loader.is_exported {
                for import in &loader.imports {
                    self.push_dynamic_import_branches(&import.sources, import.span, &[], None);
                }
                continue;
            }
            if loader.is_exported
                || loader.credited_references == 0
                || loader.references > loader.credited_references
            {
                for import in &loader.imports {
                    self.push_dynamic_import_branches(
                        &import.sources,
                        import.span,
                        &[],
                        Some(&name),
                    );
                }
                self.push_whole_object_use(name);
            }
        }
    }

    /// Give each `import()` promise binding with a member read the
    /// whole-module credit. A promise binding without any use credits
    /// nothing, and a binding handed on or awaited is already a whole use.
    pub(super) fn finish_promise_import_bindings(&mut self) {
        let mut names: Vec<String> = std::mem::take(&mut self.promise_import_bindings)
            .into_iter()
            .collect();
        names.sort_unstable();
        for name in names {
            if self
                .member_accesses
                .iter()
                .any(|access| access.object == name)
            {
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
        if self.component_loader_binding_is_shadowed(&callee.name)
            || self.component_loader_imports.get(callee.name.as_str())
                != Some(&ComponentLoaderImport::NextDynamic)
        {
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

/// The receiver of `receiver.then(() => ...)` when the first callback is an
/// arrow function without parameters. An arrow has no own `arguments`, so it
/// cannot read the resolved value.
fn parameterless_then_object<'a, 'b>(call: &'b CallExpression<'a>) -> Option<&'b Expression<'a>> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if member.property.name != "then" {
        return None;
    }
    let Some(Argument::ArrowFunctionExpression(arrow)) = call.arguments.first() else {
        return None;
    };
    (arrow.params.items.is_empty() && arrow.params.rest.is_none()).then_some(&member.object)
}
