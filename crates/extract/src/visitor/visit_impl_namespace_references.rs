//! References to namespace import locals and their whole-object uses.

use super::*;

impl ModuleInfoExtractor {
    pub(super) fn record_whole_object_call_use(&mut self, expr: &CallExpression<'_>) {
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

    /// Pre-register namespace imports and every default-import local.
    ///
    /// [`ModuleInfoExtractor::record_bare_namespace_reference`] needs the answer
    /// at every reference, and an import declaration is legal after the code
    /// that reads it (a function body hoisted above its own import), so the
    /// walk-order `namespace_binding_names` cannot be asked. Namespace objects
    /// bound by `require` or a dynamic import keep their walk-order registry:
    /// they are not statements the language hoists. See issue #2377.
    pub(super) fn record_program_namespace_import_locals(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            match statement {
                Statement::ImportDeclaration(decl) => {
                    let Some(specifiers) = &decl.specifiers else {
                        continue;
                    };
                    for specifier in specifiers {
                        if !decl.import_kind.is_type() {
                            let runtime_local = match specifier {
                                ImportDeclarationSpecifier::ImportSpecifier(named)
                                    if !named.import_kind.is_type() =>
                                {
                                    Some(named.local.name.as_str())
                                }
                                ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                                    Some(default.local.name.as_str())
                                }
                                ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                                    Some(namespace.local.name.as_str())
                                }
                                ImportDeclarationSpecifier::ImportSpecifier(_) => None,
                            };
                            if let Some(local) = runtime_local {
                                self.runtime_import_locals.insert(local.to_owned());
                            }
                        }
                        match specifier {
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                                self.namespace_import_locals
                                    .insert(namespace.local.name.to_string());
                            }
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                                let local = default.local.name.to_string();
                                self.default_import_locals.insert(local.clone());
                                if crate::template_expression_scan::is_css_module_source(
                                    &decl.source.value.text(),
                                ) {
                                    self.namespace_import_locals.insert(local);
                                }
                            }
                            ImportDeclarationSpecifier::ImportSpecifier(named)
                                if named.imported.name() == "default" =>
                            {
                                let local = named.local.name.to_string();
                                self.default_import_locals.insert(local.clone());
                                if crate::template_expression_scan::is_css_module_source(
                                    &decl.source.value.text(),
                                ) {
                                    self.namespace_import_locals.insert(local);
                                }
                            }
                            ImportDeclarationSpecifier::ImportSpecifier(_) => {}
                        }
                    }
                }
                Statement::TSImportEqualsDeclaration(decl) => {
                    self.record_import_equals_namespace_local(decl);
                }
                Statement::ExportDeclaration(decl) => {
                    if let Declaration::TSImportEqualsDeclaration(inner) = &decl.declaration {
                        self.record_import_equals_namespace_local(inner);
                    }
                }
                _ => {}
            }
        }
    }

    /// Register the local of an `import X = require('...')` declaration as a
    /// namespace-import local, so a bare reference that hands the module object
    /// on credits every export the receiver can reach, exactly as the
    /// `import * as X from '...'` spelling does (issues #2365, #2377).
    ///
    /// The entity-name form (`import X = Some.Ns`) names a binding declared in
    /// this file, not a module object, so it is not registered.
    pub(super) fn record_import_equals_namespace_local(
        &mut self,
        decl: &TSImportEqualsDeclaration<'_>,
    ) {
        if matches!(
            &decl.module_reference,
            TSModuleReference::ExternalModuleReference(_)
        ) {
            self.namespace_import_locals
                .insert(decl.id.name.to_string());
        }
    }

    /// Mark a reference the visitor resolved to a specific member, so
    /// [`ModuleInfoExtractor::record_bare_namespace_reference`] does not turn it
    /// into a whole-object use (issue #2377).
    pub(super) fn mark_structured_namespace_reference(&mut self, ident: &IdentifierReference<'_>) {
        if self.carries_namespace_object(ident.name.as_str())
            || self.default_import_locals.contains(ident.name.as_str())
        {
            self.structured_namespace_reference_spans.insert(ident.span);
        }
    }

    /// Whether a bare reference to `name` hands a namespace object on: the
    /// namespace-import local itself, or a local whose object literal holds one
    /// (`const api = { ns }`).
    pub(super) fn carries_namespace_object(&self, name: &str) -> bool {
        self.namespace_import_locals.contains(name)
            || self
                .namespace_binding_names
                .iter()
                .any(|local| local == name)
            || self
                .object_literal_namespace_placements
                .iter()
                .any(|(root, _)| root == name)
    }

    /// Record a namespace-import local placed in an object literal bound to
    /// `root_name`, and exclude the placement itself from the bare-reference
    /// rule (issue #2377).
    pub(super) fn record_object_literal_namespace_placement(
        &mut self,
        root_name: &str,
        value: &IdentifierReference<'_>,
    ) {
        if !self.namespace_import_locals.contains(value.name.as_str()) {
            return;
        }
        self.structured_namespace_reference_spans.insert(value.span);
        let placement = (root_name.to_string(), value.name.to_string());
        if !self
            .object_literal_namespace_placements
            .contains(&placement)
        {
            self.object_literal_namespace_placements.push(placement);
        }
    }

    /// Mark the root identifier of a JSX member-expression tag (`<NS.Card />`),
    /// which is the JSX spelling of `NS.Card`, not a bare pass of `NS`.
    pub(super) fn mark_jsx_member_tag_root(&mut self, name: Option<&JSXElementName<'_>>) {
        let Some(JSXElementName::MemberExpression(member)) = name else {
            return;
        };
        let mut current = member;
        loop {
            match &current.object {
                JSXMemberExpressionObject::MemberExpression(inner) => current = inner,
                JSXMemberExpressionObject::IdentifierReference(root) => {
                    self.mark_structured_namespace_reference(root);
                    return;
                }
                JSXMemberExpressionObject::ThisExpression(_) => return,
            }
        }
    }

    /// A namespace-import local mentioned anywhere the visitor cannot resolve to
    /// one member hands the whole namespace object over: a call argument, a JSX
    /// attribute value, an alias (`const N = NS`), an array or object literal
    /// element, an initializer, an assignment right-hand side, a return value,
    /// or `typeof NS`. The callee (or the reader of the alias) can reach every
    /// export, so narrowing to the dotted accesses alone reports the siblings it
    /// still uses. Recording a whole-object use puts the binding back on the
    /// graph's mark-all path, which over-credits instead (issue #2377).
    ///
    /// The precise positions record their own member access and are excluded
    /// through `structured_namespace_reference_spans`. That includes the
    /// string-computed access `NS['Moon']`, which the parser resolves exactly;
    /// the Astro and MDX text guards cannot and leave it on mark-all.
    ///
    /// The local of the binding's own `import * as NS` is a binding identifier,
    /// never a reference, so the import declaration needs no exclusion.
    pub(super) fn record_bare_namespace_reference(&mut self, ident: &IdentifierReference<'_>) {
        // Every identifier reference of every file reaches here; a file without
        // a namespace-like binding has nothing to answer.
        if self.namespace_import_locals.is_empty()
            && self.default_import_locals.is_empty()
            && self.namespace_binding_names.is_empty()
            && self.object_literal_namespace_placements.is_empty()
        {
            return;
        }
        if self
            .structured_namespace_reference_spans
            .contains(&ident.span)
        {
            return;
        }
        let name = ident.name.as_str();
        if self.namespace_like_binding_is_shadowed(name) {
            return;
        }
        if self.default_import_locals.contains(name)
            && self
                .default_import_whole_object_uses
                .insert(name.to_string())
        {
            self.semantic_facts
                .push(SemanticFact::DefaultImportWholeObjectUse(
                    DefaultImportWholeObjectUseFact {
                        local_name: name.to_string(),
                    },
                ));
        }
        if self.namespace_import_locals.contains(name)
            || self
                .namespace_binding_names
                .iter()
                .any(|local| local == name)
        {
            self.record_namespace_whole_object_use(ident.span, name);
            return;
        }
        self.record_object_literal_namespace_pass(name);
    }

    /// A bare reference to a local whose object literal holds a namespace
    /// (`const api = { ns }; hand(api)`) hands that namespace on with the
    /// object. Placing it there is precise on its own: the object-binding
    /// resolver follows `api.ns.<member>`, and issue #2372 keeps such a binding
    /// off the whole-module closure for exactly that reason. See issue #2377.
    pub(super) fn record_object_literal_namespace_pass(&mut self, root_name: &str) {
        if self.object_literal_namespace_placements.is_empty() {
            return;
        }
        let passed: Vec<String> = self
            .object_literal_namespace_placements
            .iter()
            .filter(|(root, _)| root == root_name)
            .map(|(_, namespace)| namespace.clone())
            .collect();
        for namespace in passed {
            self.record_whole_object_identifier_use(&namespace);
        }
    }

    pub(in crate::visitor) fn record_whole_object_identifier_use(&mut self, name: &str) {
        if name == "loaderData" || self.route_loader_data_bindings.contains(name) {
            self.push_whole_object_use(ROUTE_LOADER_DATA_OBJECT.to_string());
        }
        // A `this.<field>` reflective use (`Object.keys(this.opts.c)`) is keyed
        // per class so it resolves against the same class's binding, then
        // stripped back to `this.` before emission (issue #1821).
        let qualified = self.qualify_this_scope(name);
        self.push_walk_order_whole_object_use(qualified);
    }

    /// Record one name in the whole-object set, skipping a repeat.
    ///
    /// Every consumer asks membership, never a count, and the record is
    /// persisted, so a name mentioned opaquely many times (a namespace handed
    /// to several callees) stays one entry. `extend_whole_object_uses` applies
    /// the same rule to the Astro and MDX template passes.
    pub(in crate::visitor) fn push_whole_object_use(&mut self, name: String) {
        if self.whole_object_uses.contains(&name) {
            return;
        }
        self.whole_object_uses.push(name);
    }
}
