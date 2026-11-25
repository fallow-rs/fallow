//! Top-level and nested declarations, and the metadata and initialized
//! bindings of a variable declarator.

use super::*;

impl ModuleInfoExtractor {
    pub(super) fn record_top_level_declaration(&mut self, decl: &Declaration<'_>) {
        match decl {
            Declaration::VariableDeclaration(var) => {
                for declarator in &var.declarations {
                    for id in declarator.id.get_binding_identifiers() {
                        self.record_declaration_name(&id.name, var.declare);
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
                self.record_string_enum_member_values(enumd);
            }
            Declaration::TSNamespaceDeclaration(namespace) => {
                self.record_local_declaration_name(&namespace.id.name);
                self.record_local_type_declaration(&namespace.id.name, namespace.id.span);
            }
            _ => {}
        }
    }

    fn record_top_level_class_declaration(&mut self, class: &Class<'_>) {
        if let Some(id) = class.id.as_ref() {
            let super_reference_span = class.heritage.as_ref().and_then(|base| {
                match unwrap_static_expr(&base.expression) {
                    Expression::Identifier(reference) => Some(reference.span),
                    Expression::StaticMemberExpression(member) => {
                        if let Expression::Identifier(reference) = &member.object {
                            Some(reference.span)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            });
            self.subclass_declarations.insert(
                id.name.to_string(),
                crate::visitor::SubclassDeclaration {
                    binding_span: id.span,
                    super_reference_span,
                },
            );
            self.record_local_declaration_name(&id.name);
            self.record_sanitizer_binding(id.name.as_str(), None);
            self.record_literal_allowlist_binding(id.name.as_str(), false);
            self.record_risky_regex_binding(id.name.as_str(), None);
            self.record_path_sink_binding(id.name.as_str(), None);
            self.record_path_relative_binding(id.name.as_str(), None);
            self.record_local_type_declaration(&id.name, id.span);
            let is_angular = has_angular_class_decorator(class);
            self.record_angular_inputs_outputs(class, is_angular);
            let (instance_bindings, binding_origins) =
                crate::visitor::helpers::extract_class_instance_bindings_with_origins(
                    class,
                    |local_name, source, imported_name| {
                        self.is_named_import_from(local_name, source, imported_name)
                    },
                );
            let members = extract_class_members(class, is_angular, self.heritage_objects());
            self.record_factory_property_class(id);
            if self.capture_subclass_provenance {
                self.record_binding_declaration(id.name.as_str(), id.span);
                for member in &members {
                    if member.is_instance_returning_static
                        && member.kind == fallow_types::extract::MemberKind::ClassMethod
                    {
                        self.register_binding_producer(
                            &format!("{}.{}", id.name, member.name),
                            crate::visitor::BindingProducer {
                                name: id.name.to_string(),
                                declaration_span: id.span,
                                result: None,
                                shape: Vec::new(),
                                is_async: false,
                                is_generator: false,
                                is_static_method: true,
                            },
                        );
                    }
                }
            }
            self.record_local_class_export(
                id.name.to_string(),
                crate::visitor::LocalClassExportInfo {
                    members,
                    super_class: extract_super_class_name(class),
                    implemented_interfaces: extract_implemented_interface_names(class),
                    type_parameters: extract_class_type_parameter_names(class),
                    instance_bindings,
                    binding_origins,
                    binding_span: id.span,
                    super_class_type_args: extract_super_class_type_args(class),
                    generic_instance_bindings: extract_class_generic_instance_bindings(class),
                },
            );
            let flow = Self::collect_class_type_flow(class);
            self.record_type_flow(&id.name, flow);
        }
    }

    /// Harvest Angular `@Input()` / `@Output()` / signal `input()` / `output()` /
    /// `model()` members from an Angular-decorated class onto the module-level
    /// input/output accumulators. Gated on `is_angular` so a non-Angular class
    /// with a same-named `input` / `Input` helper never contributes. Called from
    /// every class-extraction site (named export, default export, local
    /// declaration) so the SFC and plain-`.ts` paths both populate it.
    pub(in crate::visitor) fn record_angular_inputs_outputs(
        &mut self,
        class: &Class<'_>,
        is_angular: bool,
    ) {
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
            self.record_declaration_name(&id.name, function.declare);
            self.record_sanitizer_binding(id.name.as_str(), None);
            self.record_literal_allowlist_binding(id.name.as_str(), false);
            self.record_risky_regex_binding(id.name.as_str(), None);
            self.record_path_sink_binding(id.name.as_str(), None);
            self.record_path_relative_binding(id.name.as_str(), None);
            self.record_function_type_flow(&id.name, function);
            let type_parameter_scope_pushed = function
                .type_parameters
                .as_deref()
                .is_some_and(|params| self.push_function_type_parameter_scope(params));
            self.record_local_structural_function(
                id.name.as_str(),
                &function.params,
                function.body.as_deref().map(BodyRef::Block),
                None,
                function.return_type.as_deref(),
            );
            if type_parameter_scope_pushed {
                self.pop_function_type_alias_scope();
            }
            self.record_factory_return_function(
                id.name.as_str(),
                FactoryReturnFunctionInput {
                    binding_span: id.span,
                    params: &function.params,
                    body: function.body.as_deref().map(BodyRef::Block),
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
                &self.create_require_aliases,
                &self.module_require_aliases,
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
        self.record_type_alias_surface_targets(alias);
        self.record_playwright_fixture_type_alias(alias);
        let refs = Self::collect_type_alias_signature_refs(alias);
        self.record_local_signature_refs(&alias.id.name, refs);
        if let TSType::TSTypeLiteral(type_lit) = &alias.type_annotation {
            self.record_required_type_members(&alias.id.name, &type_lit.members);
            let mut properties = collect_object_type_property_types(&type_lit.members);
            for target in properties.values_mut() {
                target.owner_span = Some(alias.id.span);
            }
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
        self.record_required_type_members(&iface.id.name, &iface.body.body);
        self.record_interface_property_types(iface);
        // React props harvest (Feature A): a plain `interface X { a; b }` with no
        // `extends` heritage and no type parameters can back a
        // `(props: X) => props.a` component. An `interface X extends Y` or a
        // generic `interface X<T>` is excluded (fallow cannot expand the parent
        // members / substitute T), so such a typed param abstains.
        if iface.extends.is_empty() && iface.type_parameters.is_none() {
            self.record_react_object_type_props(&iface.id.name, &iface.body.body);
        }
    }

    fn record_required_type_members(&mut self, type_name: &str, members: &[TSSignature<'_>]) {
        for member in members {
            let required_name = match member {
                TSSignature::TSPropertySignature(property) if !property.optional => {
                    property.key.static_name()
                }
                TSSignature::TSMethodSignature(method) if !method.optional => {
                    method.key.static_name()
                }
                _ => None,
            };
            if let Some(member) = required_name {
                self.semantic_facts.push(SemanticFact::RequiredTypeMember(
                    RequiredTypeMemberFact {
                        type_name: type_name.to_string(),
                        member: member.text().into_owned(),
                    },
                ));
            }
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
            props.push((property_name.text().into_owned(), prop.span.start));
        }
        if !props.is_empty() {
            self.react_object_type_props
                .insert(type_name.to_string(), props);
        }
    }

    pub(super) fn record_nested_declaration(&mut self, decl: &Declaration<'_>) {
        match decl {
            Declaration::VariableDeclaration(var) => {
                if var.kind.is_lexical() {
                    self.record_direct_object_binding_scope_roots(
                        var.declarations
                            .iter()
                            .flat_map(|declarator| declarator.id.get_binding_identifiers())
                            .map(|id| id.name.to_string()),
                    );
                }
                self.record_nested_scope_declarators(var);
                self.record_non_instance_declarators(var);
            }
            Declaration::ClassDeclaration(class) => {
                if let Some(id) = class.id.as_ref() {
                    self.record_direct_object_binding_scope_roots([id.name.to_string()]);
                    self.record_nested_named_declaration(id);
                }
            }
            Declaration::FunctionDeclaration(function) => {
                if let Some(id) = function.id.as_ref() {
                    self.record_direct_object_binding_scope_roots([id.name.to_string()]);
                    self.record_nested_named_declaration(id);
                }
            }
            _ => {}
        }
    }

    pub(super) fn preseed_direct_object_binding_scope_roots(
        &mut self,
        statements: &[Statement<'_>],
    ) {
        for statement in statements {
            let declaration = match statement {
                Statement::ExportDeclaration(export) => Some(&export.declaration),
                _ => statement.as_declaration(),
            };
            let Some(declaration) = declaration else {
                continue;
            };
            match declaration {
                Declaration::VariableDeclaration(variable) if variable.kind.is_lexical() => {
                    self.record_direct_object_binding_scope_roots(
                        variable
                            .declarations
                            .iter()
                            .flat_map(|declarator| declarator.id.get_binding_identifiers())
                            .map(|id| id.name.to_string()),
                    );
                }
                Declaration::ClassDeclaration(class) => {
                    if let Some(id) = class.id.as_ref() {
                        self.record_direct_object_binding_scope_roots([id.name.to_string()]);
                    }
                }
                Declaration::FunctionDeclaration(function) => {
                    if let Some(id) = function.id.as_ref() {
                        self.record_direct_object_binding_scope_roots([id.name.to_string()]);
                    }
                }
                _ => {}
            }
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

    fn capture_subclass_producer_member_access(
        &mut self,
        object: &Expression<'_>,
        member: &str,
    ) -> bool {
        if let Expression::CallExpression(call) = object {
            let source = match &call.callee {
                Expression::Identifier(reference) => {
                    Some((reference.name.to_string(), reference.span))
                }
                Expression::StaticMemberExpression(callee) => {
                    static_member_object_name(&callee.object).and_then(|root| {
                        iteration_source_reference(&callee.object)
                            .map(|span| (format!("{root}.{}", callee.property.name), span))
                    })
                }
                _ => None,
            };
            if let Some((source, reference)) = source
                && let Some(id) = self.capture_binding_producer_call(&source, reference, false)
            {
                if let Expression::StaticMemberExpression(callee) = &call.callee
                    && let Expression::Identifier(root) = &callee.object
                {
                    self.set_static_binding_call_origin(id, root.name.as_str(), root.span);
                }
                self.pending_subclass_member_accesses.push(
                    crate::visitor::PendingSubclassMemberAccess {
                        proof: crate::visitor::SubclassAccessProof::Value {
                            id,
                            requirements: crate::visitor::BindingRequirements::Empty,
                        },
                        member: member.to_string(),
                    },
                );
                return true;
            }
        }
        false
    }

    pub(super) fn record_subclass_member_access(&mut self, object: &Expression<'_>, member: &str) {
        if !self.capture_subclass_provenance {
            return;
        }
        let object = unwrap_static_expr(object);
        if let Some(path) = static_member_object_name(object)
            && !matches!(
                object,
                Expression::CallExpression(_) | Expression::NewExpression(_)
            )
        {
            let path = self.qualify_this_scope(&path);
            if !(path.starts_with("this@") && self.independent_this_depth > 0)
                && self.capture_subclass_binding_access(
                    &path,
                    subclass_receiver_reference(object).map(|(reference, _)| reference.span),
                    member,
                )
            {
                return;
            }
        }
        if self.capture_subclass_producer_member_access(object, member) {
            return;
        }
        let map_proof = self.map_get_value_type(object).and_then(|_| {
            let Expression::CallExpression(call) = object else {
                return None;
            };
            let Expression::StaticMemberExpression(member) = &call.callee else {
                return None;
            };
            let source = self.qualify_this_scope(&static_member_object_name(&member.object)?);
            if source.starts_with("this@") && self.independent_this_depth > 0 {
                return None;
            }
            self.iteration_binding_class_proof(&source, iteration_source_reference(&member.object))
        });
        let proof = map_proof.or_else(|| match object {
            Expression::NewExpression(expression) => {
                let Expression::Identifier(reference) = &expression.callee else {
                    return None;
                };
                Some(crate::visitor::BindingClassProof {
                    builtin_containers: std::sync::Arc::default(),
                    target_name: reference.name.to_string(),
                    target_reference_span: reference.span,
                    requirements: crate::visitor::BindingRequirements::Empty,
                })
            }
            Expression::Identifier(reference) => self
                .direct_binding_class_proof(reference.name.as_str(), Some(reference.span))
                .or_else(|| {
                    Some(crate::visitor::BindingClassProof {
                        builtin_containers: std::sync::Arc::default(),
                        target_name: reference.name.to_string(),
                        target_reference_span: reference.span,
                        requirements: crate::visitor::BindingRequirements::Empty,
                    })
                }),
            _ => {
                let path = static_member_object_name(object)?;
                let path = self.qualify_this_scope(&path);
                if path.starts_with("this@") && self.independent_this_depth > 0 {
                    return None;
                }
                self.direct_binding_class_proof(
                    &path,
                    subclass_receiver_reference(object).map(|(reference, _)| reference.span),
                )
            }
        });
        if let Some(proof) = proof {
            self.pending_subclass_member_accesses.push(
                crate::visitor::PendingSubclassMemberAccess {
                    proof: crate::visitor::SubclassAccessProof::Known(proof),
                    member: member.to_string(),
                },
            );
        }
    }

    pub(super) fn record_variable_declarator_metadata(
        &mut self,
        declarator: &VariableDeclarator<'_>,
    ) {
        for id in declarator.id.get_binding_identifiers() {
            self.record_binding_declaration(id.name.as_str(), id.span);
        }
        if self.is_module_scope() {
            self.record_module_variable_signature_refs(declarator);
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

    pub(super) fn record_uninitialized_variable_bindings(
        &mut self,
        declarator: &VariableDeclarator<'_>,
    ) {
        for id in declarator.id.get_binding_identifiers() {
            self.record_sanitizer_binding(id.name.as_str(), None);
            self.record_literal_allowlist_binding(id.name.as_str(), false);
            self.record_risky_regex_binding(id.name.as_str(), None);
            self.record_path_sink_binding(id.name.as_str(), None);
            self.record_path_relative_binding(id.name.as_str(), None);
        }
    }

    pub(super) fn record_initialized_variable_bindings(
        &mut self,
        decl: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        self.record_native_html_element_alias_candidate(decl, declarator, init);
        self.record_local_structural_function_from_variable_declarator(declarator, init);
        self.record_factory_return_candidate(declarator, init);
        self.record_source_returning_helper_from_variable_declarator(decl, declarator, init);
        self.record_sanitizer_helper_from_variable_declarator(decl, declarator, init);
        self.record_initialized_declarator_bindings(decl, declarator, init);
        if let BindingPattern::ObjectPattern(pattern) = &declarator.id {
            self.record_sink_pattern_bindings(pattern, init);
        }
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

    pub(super) fn record_playwright_variable_helpers(
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
