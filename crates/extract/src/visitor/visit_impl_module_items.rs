//! The program prologue, import and export declarations, function bodies,
//! the dispatch of a variable declarator, route loader data, and the class
//! of a receiver.

use super::*;

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
                Some("use server") => Some(true),
                Some("use client") => Some(false),
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
    pub(super) fn record_program_prologue(&mut self, program: &Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    self.record_source_returning_function_declaration(function);
                    self.record_sanitizer_function_declaration(function);
                }
                Statement::ExportDeclaration(export) => {
                    if let Declaration::FunctionDeclaration(function) = &export.declaration {
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
    pub(super) fn record_program_sanitizer_functions(&mut self, program: &Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    self.record_sanitizer_function_declaration(function);
                }
                Statement::ExportDeclaration(export) => {
                    if let Declaration::FunctionDeclaration(function) = &export.declaration {
                        self.record_sanitizer_function_declaration(function);
                    }
                }
                _ => {}
            }
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

    /// Record an import declaration that binds no name as a side-effect import
    /// of `source`.
    pub(super) fn push_side_effect_import(
        &mut self,
        decl: &ImportDeclaration<'_>,
        source: String,
        is_type_only: bool,
    ) {
        self.imports.push(ImportInfo {
            source,
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only,
            is_type_only_star: false,
            from_style: false,
            span: decl.span,
            source_span: decl.source.span,
        });
    }

    /// Record a named import specifier (`import { fork } from ...`), tracking the
    /// `child_process.fork` and `node:url` `fileURLToPath` provenance bindings.
    pub(super) fn handle_import_specifier(
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
        if !is_type_only && !s.import_kind.is_type() {
            self.record_import_sink_binding(source, &s.imported.name(), s.local.name.as_str());
        }
        if matches!(source, "vue" | "@vue/reactivity") {
            self.module_direct_object_binding_targets
                .entry(s.local.name.to_string())
                .or_default()
                .declaration_span = Some(s.local.span);
        }
        self.imports.push(ImportInfo {
            source: source.to_string(),
            imported_name: ImportedName::Named(s.imported.name().to_string()),
            local_name: s.local.name.to_string(),
            is_type_only: is_type_only || s.import_kind.is_type(),
            is_type_only_star: false,
            from_style: false,
            span: s.span,
            source_span,
        });
    }

    /// Record a default import specifier (`import x from ...`), tracking the
    /// DOMPurify, `node:path` and Node module sink bindings.
    pub(super) fn handle_import_default_specifier(
        &mut self,
        s: &ImportDefaultSpecifier<'a>,
        source: &str,
        is_type_only: bool,
        source_span: Span,
    ) {
        let local = s.local.name.to_string();
        self.record_whole_module_import_bindings(source, &local, is_type_only);
        if self.is_module_scope() && is_node_path_source(source) {
            self.node_path_namespace_bindings.insert(local.clone());
        }
        if is_css_module_import_source(source) {
            self.record_namespace_binding_name(local.clone());
        }
        self.imports.push(ImportInfo {
            source: source.to_string(),
            imported_name: ImportedName::Default,
            local_name: local,
            is_type_only,
            is_type_only_star: false,
            from_style: false,
            span: s.span,
            source_span,
        });
    }

    /// Record a namespace import specifier (`import * as ns from ...`), tracking
    /// the DOMPurify, `child_process`, `node:path`, and `node:url` provenance
    /// bindings, the Node module sink binding, and the namespace binding name.
    pub(super) fn handle_import_namespace_specifier(
        &mut self,
        s: &ImportNamespaceSpecifier<'a>,
        source: &str,
        is_type_only: bool,
        source_span: Span,
    ) {
        let local = s.local.name.to_string();
        self.record_whole_module_import_bindings(source, &local, is_type_only);
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
        self.record_namespace_binding_name(local.clone());
        if matches!(source, "vue" | "@vue/reactivity") {
            self.module_direct_object_binding_targets
                .entry(s.local.name.to_string())
                .or_default()
                .declaration_span = Some(s.local.span);
        }
        self.imports.push(ImportInfo {
            source: source.to_string(),
            imported_name: ImportedName::Namespace,
            local_name: local,
            is_type_only,
            is_type_only_star: false,
            from_style: false,
            span: s.span,
            source_span,
        });
    }

    /// `export { NS }` hands the namespace object to consumers the graph
    /// cannot enumerate, and `narrow_namespace_references` already puts that
    /// binding on mark-all through its own re-export test (issue #2373), so
    /// the specifier local is not a bare pass to record again (issue #2377).
    pub(super) fn mark_export_specifier_namespace_references(
        &mut self,
        specifiers: &[ExportSpecifier<'a>],
    ) {
        for specifier in specifiers {
            if let ModuleExportName::IdentifierReference(local) = &specifier.local {
                self.mark_structured_namespace_reference(local);
            }
        }
    }

    /// Walk a `namespace` or `declare module` body inside its own direct
    /// object binding scope. The scope holds the `var` roots of the body and
    /// the namespace name.
    pub(super) fn with_module_body_binding_scope(
        &mut self,
        body_statements: Option<&oxc_allocator::Vec<'a, Statement<'a>>>,
        name: Option<&str>,
        walk_body: impl FnOnce(&mut Self),
    ) {
        let mut direct_roots =
            body_statements.map_or_else(FxHashSet::default, |body| Self::var_binding_roots(body));
        if let Some(name) = name {
            direct_roots.insert(name.to_string());
        }
        self.push_var_owner_direct_object_binding_scope(direct_roots);
        if let Some(body) = body_statements {
            self.preseed_direct_object_binding_scope_roots(body);
            self.preseed_direct_object_binding_targets(body);
        }
        walk_body(self);
        self.pop_direct_object_binding_scope();
    }

    /// Record `<object_name>.<member>` for a static member read whose receiver
    /// is a name path (`a`, `a.b`, `this`).
    pub(super) fn record_named_member_access(&mut self, object_name: &str, member: &str) {
        self.record_inherited_this_result_access(object_name, member);
        if self.route_loader_data_bindings.contains(object_name) {
            self.member_accesses.push(MemberAccess {
                object: ROUTE_LOADER_DATA_OBJECT.to_string(),
                member: member.to_string(),
            });
        }
        if self.defer_independent_this_read(object_name, member) {
            return;
        }
        // Qualify a `this.<field>` receiver with the enclosing class scope so
        // it resolves against the same class's binding key (issue #1821);
        // stripped back to `this.` before emission. A bare `this` object and
        // any non-`this` receiver pass through unchanged.
        if !self.namespace_like_binding_is_shadowed(object_name)
            && !self.namespace_import_root_is_shadowed(object_name)
        {
            let object = self.qualify_this_scope(object_name);
            if !self.record_walk_order_member_access(&object, member) {
                self.push_receiver_member_access(object, member.to_string());
            }
        }
    }

    /// Walk an interface heritage name as the member expression it was before
    /// Oxc 0.151 (`extends ns.Base`), so it records the same member accesses
    /// and namespace marks as `visit_static_member_expression`.
    pub(super) fn visit_heritage_type_name(&mut self, name: &TSTypeName<'a>) {
        let TSTypeName::QualifiedName(qualified) = name else {
            self.visit_ts_type_name(name);
            return;
        };
        if let Some(object_name) = type_name_member_path(&qualified.left) {
            self.record_named_member_access(&object_name, qualified.right.name.as_str());
        }
        if let TSTypeName::IdentifierReference(object) = &qualified.left {
            self.mark_structured_namespace_reference(object);
        }
        self.visit_heritage_type_name(&qualified.left);
        self.visit_identifier_name(&qualified.right);
    }

    /// Walk a function or arrow body. A concise arrow body walks its
    /// expression the way the old single-statement body did: it declares
    /// nothing, preseeds nothing and is not a fail-closed guard.
    pub(super) fn visit_body(&mut self, body: BodyRef<'_, 'a>) {
        let statements = body.statements();
        let type_alias_scope_pushed =
            self.namespace_depth == 0 && self.push_function_type_alias_scope(statements);
        if let Some(var_roots) = self.deferred_function_var_binding_roots.last() {
            self.record_direct_object_binding_scope_roots(var_roots.clone());
        }
        if self.namespace_depth == 0 {
            self.scoped_array_binding_element_types
                .push(FxHashMap::default());
            self.preseed_scope_declarations(statements);
        }
        self.preseed_direct_object_binding_targets(statements);
        match body {
            BodyRef::Block(_) => {
                for statement in statements {
                    self.visit_statement(statement);
                    if self.namespace_depth == 0 {
                        self.record_fail_closed_guard_after_statement(statement);
                    }
                }
            }
            BodyRef::Concise(expr) => self.visit_expression(expr),
        }
        if type_alias_scope_pushed {
            self.pop_function_type_alias_scope();
        }
        if self.namespace_depth == 0 {
            self.scoped_array_binding_element_types.pop();
        }
    }

    /// Record the exports of an `export <declaration>` statement.
    pub(super) fn record_export_declaration(
        &mut self,
        declaration: &Declaration<'a>,
        is_type_only: bool,
    ) {
        self.extract_declaration_exports(declaration, is_type_only);
        if !is_type_only {
            self.try_harvest_load_export(declaration);
        }
    }

    /// Record `export { x }` local specifiers (no source), abstaining the
    /// load-data harvest on a bare `export { load }`.
    pub(super) fn record_export_local_specifiers(
        &mut self,
        decl: &ExportNamedDeclaration<'a>,
        is_type_only: bool,
    ) {
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

    fn record_declarator_receiver_aliases(
        &mut self,
        declarator: &VariableDeclarator<'a>,
        init: &Expression<'a>,
    ) {
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && !matches!(
                unwrap_static_expr(init),
                Expression::NewExpression(_) | Expression::CallExpression(_)
            )
            && let Some(source) = static_member_object_name(unwrap_static_expr(init))
        {
            let source = self.qualify_this_scope(&source);
            self.copy_direct_binding_proof(
                &source,
                id.name.as_str(),
                subclass_receiver_reference(unwrap_static_expr(init))
                    .map(|(reference, _)| reference.span),
            );
            // An element source (`const day = m.days[i]`) binds no local: the
            // module-flat binding map would mix the local with a binding of
            // the same name in another function.
            if !source.contains("[]")
                && let Some(BindingTarget::Class(type_name)) =
                    self.resolve_bound_object_name(&source)
            {
                self.insert_class_binding_target(id.name.to_string(), type_name);
            }
        }

        // `const { service } = input` is the destructure form of
        // `const service = input.service`. Resolve each destructured path the
        // same way, so a typed source path (for example a parameter typed by
        // an inline type literal) binds the local to its class.
        if let BindingPattern::ObjectPattern(pattern) = &declarator.id
            && let Some(source) = static_member_object_name(unwrap_static_expr(init))
        {
            let source = self.qualify_this_scope(&source);
            for (local, path) in extract_object_pattern_bindings(pattern) {
                self.copy_direct_binding_proof(
                    &format!("{source}.{path}"),
                    &local,
                    subclass_receiver_reference(unwrap_static_expr(init))
                        .map(|(reference, _)| reference.span),
                );
                if let Some(BindingTarget::Class(type_name)) =
                    self.resolve_bound_object_name(&format!("{source}.{path}"))
                {
                    self.insert_class_binding_target(local, type_name);
                }
            }
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
        self.record_declarator_member_destructuring(declarator, init);
        self.record_method_result_aliases(declarator, init);

        // `a ?? new A()`, `f ? new A() : null` and similar inits hold the
        // class instance in one operand. Bind through that operand.
        let original_init = init;
        let init = self.class_carrying_init(init);

        self.record_declarator_receiver_aliases(declarator, init);

        // `f ? (value as T) : null` binds `T`. A plain `value as T` init
        // stays out: only an unwrapped nullable operand reads the assertion.
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && !std::ptr::eq(init, original_init)
            && let Some(type_name) = self.asserted_receiver_type_name(init)
        {
            let origin = asserted_receiver_type(init)
                .and_then(crate::visitor::helpers::extract_type_reference_origin);
            self.record_binding_class_proof(id.name.as_str(), &type_name, origin);
            self.insert_class_binding_target(id.name.to_string(), type_name);
        }

        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Some(class_name) = constructed_class_name(init)
        {
            self.insert_class_binding_target(id.name.to_string(), class_name.to_string());
            if let Expression::NewExpression(expression) = unwrap_static_expr(init)
                && let Expression::Identifier(reference) = &expression.callee
            {
                self.record_binding_class_proof(id.name.as_str(), class_name, Some(reference.span));
                self.retain_new_constructor(id.name.as_str(), reference);
            }
        }

        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Some(class_name) = Self::svelte_derived_new_class(init)
        {
            let origin = match init {
                Expression::CallExpression(call) => match call.arguments.first() {
                    Some(Argument::NewExpression(expr)) => match &expr.callee {
                        Expression::Identifier(id) => Some(id.span),
                        _ => None,
                    },
                    _ => crate::visitor::helpers::try_extract_factory_new_class_origin(
                        &call.arguments,
                    )
                    .map(|(_, span)| span),
                },
                _ => None,
            };
            self.record_binding_class_proof(id.name.as_str(), &class_name, origin);
            self.insert_class_binding_target(id.name.to_string(), class_name);
        }

        if let Expression::CallExpression(call) = init
            && let BindingPattern::ArrayPattern(arr_pat) = &declarator.id
            && let Some(Some(BindingPattern::BindingIdentifier(id))) = arr_pat.elements.first()
            && let Some(class_name) =
                crate::visitor::helpers::try_extract_factory_new_class(&call.arguments)
        {
            self.record_binding_declaration(id.name.as_str(), id.span);
            let origin =
                crate::visitor::helpers::try_extract_factory_new_class_origin(&call.arguments)
                    .map(|(_, span)| span);
            self.record_binding_class_proof(id.name.as_str(), &class_name, origin);
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
            && let Some(class_name) =
                crate::visitor::helpers::try_extract_factory_new_class(&call.arguments)
        {
            let origin =
                crate::visitor::helpers::try_extract_factory_new_class_origin(&call.arguments)
                    .map(|(_, span)| span);
            self.record_binding_class_proof(id.name.as_str(), &class_name, origin);
            self.insert_class_binding_target_if_absent(id.name.to_string(), class_name);
        }

        // `await Cls.create()` gives the instance of an async static factory
        // (`static async create(): Promise<Cls>`), as `Cls.build()` gives the
        // instance of a sync one.
        let static_call_init = match init {
            Expression::AwaitExpression(awaited) => unwrap_static_expr(&awaited.argument),
            _ => init,
        };
        if let Expression::CallExpression(call) = static_call_init
            && let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(callee_object) = &member.object
        {
            if self.capture_subclass_provenance {
                let path = format!("{}.{}", callee_object.name, member.property.name);
                let awaited = !std::ptr::eq(static_call_init, init);
                if let Some(value) =
                    self.capture_binding_producer_call(&path, callee_object.span, awaited)
                {
                    self.set_static_binding_call_origin(
                        value,
                        callee_object.name.as_str(),
                        callee_object.span,
                    );
                    self.attach_captured_binding_value(id.name.as_str(), value);
                }
            }
            self.factory_call_candidates
                .push(crate::visitor::FactoryCallCandidate {
                    local_name: id.name.to_string(),
                    callee_object: callee_object.name.to_string(),
                    callee_method: member.property.name.to_string(),
                });
        }
    }

    pub(super) fn asserted_receiver_type_name(&self, expr: &Expression<'_>) -> Option<String> {
        let type_annotation = asserted_receiver_type(expr)?;
        let root_name = asserted_type_root_name(type_annotation)?;
        if self
            .function_type_alias_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(root_name))
            .is_some_and(|binding| {
                !matches!(binding, crate::visitor::FunctionTypeAliasBinding::ClassSelf)
            })
        {
            return None;
        }
        extract_type_reference_name(type_annotation)
    }

    /// The operand of a declarator init that holds the class instance.
    ///
    /// - `f ? X : null` (or `undefined`, or `void 0`) gives `X`.
    /// - `a ?? new A()` and `a || new A()` give `new A()` when `a` has no
    ///   bound class, or when `a` is bound to the same class `A`.
    /// - `a ?? fallback` gives `a` when `a` has a bound class and the
    ///   fallback constructs no class.
    ///
    /// Any other init, and a fallback whose two operands give two different
    /// classes, comes back unchanged. The binding arms then match nothing.
    pub(super) fn class_carrying_init<'e, 'b>(
        &self,
        init: &'b Expression<'e>,
    ) -> &'b Expression<'e> {
        match unwrap_static_expr(init) {
            Expression::ConditionalExpression(conditional) => {
                if is_nullish_value(&conditional.alternate) {
                    self.class_carrying_init(&conditional.consequent)
                } else if is_nullish_value(&conditional.consequent) {
                    self.class_carrying_init(&conditional.alternate)
                } else {
                    init
                }
            }
            Expression::LogicalExpression(logical)
                if matches!(
                    logical.operator,
                    LogicalOperator::Coalesce | LogicalOperator::Or
                ) =>
            {
                let right = unwrap_static_expr(&logical.right);
                let right_class = constructed_class_name(right);
                let left_class = self.bound_class_name(&logical.left);
                match (left_class, right_class) {
                    (Some(left), Some(right_name)) if left == right_name => right,
                    (None, Some(_)) => right,
                    (Some(_), None) => &logical.left,
                    _ => init,
                }
            }
            _ => init,
        }
    }

    /// The class bound to an identifier or static member receiver, if any.
    fn bound_class_name(&self, expr: &Expression<'_>) -> Option<String> {
        let source = static_member_object_name(unwrap_static_expr(expr))?;
        let source = self.qualify_this_scope(&source);
        match self.resolve_bound_object_name(&source) {
            Some(BindingTarget::Class(class_name)) => Some(class_name),
            _ => None,
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
                        member: name.text().into_owned(),
                    });
                }
            }
            _ => {
                self.whole_object_uses
                    .push(ROUTE_LOADER_DATA_OBJECT.to_string());
            }
        }
    }

    pub(super) fn record_route_loader_data_whole_arg_use(&mut self, expr: &CallExpression<'a>) {
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
    pub(super) fn record_variable_declarator(
        &mut self,
        decl: &VariableDeclaration<'a>,
        declarator: &VariableDeclarator<'a>,
    ) {
        self.record_variable_declarator_metadata(declarator);
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            self.record_map_binding_from_declaration(
                id.name.to_string(),
                declarator.type_annotation.as_deref(),
                declarator.init.as_ref(),
            );
        }

        let Some(init) = &declarator.init else {
            self.record_uninitialized_variable_bindings(declarator);
            self.record_projection_declarator(declarator);
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

        self.record_declarator_collection_bindings(declarator, init);

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

        if let Some((call, source)) = self.try_extract_require(init) {
            self.record_dompurify_require_binding(declarator, &source);
            self.record_child_process_require_binding(declarator, &source);
            self.handle_require_declaration(declarator, call, &source);
            return;
        }

        self.record_declarator_instance_bindings(declarator, init);
        self.record_declarator_value_alias(declarator, decl.kind, init);
        self.record_projection_declarator(declarator);
        self.record_factory_property_declarator(declarator, init);

        if let Expression::Identifier(ident) = init
            && self
                .namespace_binding_names
                .iter()
                .any(|n| n == ident.name.as_str())
        {
            // A destructure names its members: `handle_namespace_destructuring`
            // records them (or the whole object, for a rest element), so the
            // initializer is not a bare pass (issue #2377). A plain
            // `const alias = NS` binds the object itself and is one.
            if matches!(declarator.id, BindingPattern::ObjectPattern(_)) {
                self.mark_structured_namespace_reference(ident);
            }
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

        self.record_dynamic_import_declarator(declarator, init);
    }

    fn record_declarator_collection_bindings(
        &mut self,
        declarator: &VariableDeclarator<'a>,
        init: &Expression<'a>,
    ) {
        // Type array / reactive-array bindings to their element class so
        // iteration callbacks and `for...of` loops can credit item member
        // accesses. Module-scope entries remain visible to template scanners;
        // function/block locals stay scoped to the active visitor stack.
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Some((element, origin)) =
                crate::visitor::helpers::infer_array_binding_element_target(
                    declarator.type_annotation.as_deref(),
                    Some(init),
                )
                .or_else(|| {
                    self.promise_all_map_element_type(init)
                        .map(|name| (name, None))
                })
                // `const tables = all()` with `function all(): Table[]` (issue #4313).
                .or_else(|| {
                    declarator
                        .type_annotation
                        .is_none()
                        .then(|| self.local_call_array_element_type(init))
                        .flatten()
                        .map(|name| (name, None))
                })
        {
            let binding = id.name.to_string();
            self.record_vue_ref_value_array_binding_type(&binding, init, &element);
            let origin = self.resolve_class_type_param_origin(&element, origin);
            self.record_binding_iteration_proof(&binding, &element, origin, false);
            if let Expression::CallExpression(call) = init
                && Self::is_vue_array_wrapper_call(call)
            {
                self.record_binding_iteration_proof(
                    &format!("{binding}.value"),
                    &element,
                    origin,
                    false,
                );
            }
            self.record_array_binding_element_type(binding, element);
        }

        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            self.record_object_array_member_binding_types(
                id.name.as_str(),
                declarator.type_annotation.as_deref(),
            );
            if declarator.type_annotation.is_none()
                && let Some(value) = self.map_get_value_type(init)
            {
                let object = unwrap_static_expr(init);
                if let Expression::CallExpression(call) = object
                    && let Expression::StaticMemberExpression(member) = &call.callee
                    && let Some(source) = static_member_object_name(&member.object)
                    && let Some(proof) = self.iteration_binding_class_proof(
                        &self.qualify_this_scope(&source),
                        iteration_source_reference(&member.object),
                    )
                {
                    self.record_captured_binding_class_proof(id.name.as_str(), proof);
                }
                self.insert_class_binding_target(id.name.to_string(), value);
            }
        }
    }

    /// Record the dynamic-import edges of a declarator init: a
    /// `Promise.all([import(...)])` array destructure, a call of a local
    /// loader function, or a direct `import(...)`.
    fn record_dynamic_import_declarator(
        &mut self,
        declarator: &VariableDeclarator<'a>,
        init: &Expression<'a>,
    ) {
        if let BindingPattern::ArrayPattern(pattern) = &declarator.id
            && let Some(elements) = extract_promise_all_elements(init)
        {
            self.handle_promise_all_dynamic_imports(pattern, elements);
            return;
        }

        if self.record_loader_call_declaration(declarator, init) {
            return;
        }

        if let Some((span, source)) = ssr_load_module_call(init) {
            self.handle_dynamic_import_declaration(&declarator.id, span, &[source]);
            return;
        }

        let Some(import_expr) = extract_import_expression(init) else {
            return;
        };
        let mut sources = Vec::new();
        self.collect_dynamic_import_specifiers(&import_expr.source, &mut sources);
        if sources.is_empty() {
            return;
        }
        self.handle_dynamic_import_declaration(&declarator.id, import_expr.span, &sources);
        if is_unawaited_import(init)
            && let BindingPattern::BindingIdentifier(id) = &declarator.id
        {
            self.promise_import_bindings.insert(id.name.to_string());
        }
    }
}
