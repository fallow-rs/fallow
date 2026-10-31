//! Type-signature and typed-binding helpers for the visitor implementation.

use super::visit_helpers::*;
use super::*;
use crate::js_text::JsText;
use crate::{SemanticFact, TypeAliasSurfaceTargetFact};

impl ModuleInfoExtractor {
    fn collect_type_alias_surface_targets(
        ty: &TSType<'_>,
        type_parameters: &FxHashSet<&str>,
        targets: &mut Vec<String>,
    ) {
        match ty {
            TSType::TSTypeReference(reference) => {
                // `ReturnType<typeof useFooStore>`: a member read through an
                // imported alias credits the store factory.
                if let Some(factory) = Self::store_factory_from_return_type(ty) {
                    targets.push(factory);
                    return;
                }
                let Some((name, _)) = type_name_root(&reference.type_name) else {
                    return;
                };
                if matches!(
                    name.as_str(),
                    "Pick" | "Omit" | "Partial" | "Required" | "Readonly" | "NonNullable"
                ) {
                    if let Some(first) = reference
                        .type_arguments
                        .as_deref()
                        .and_then(|arguments| arguments.params.first())
                    {
                        Self::collect_type_alias_surface_targets(first, type_parameters, targets);
                    }
                } else if !type_parameters.contains(name.as_str()) {
                    targets.push(name);
                }
            }
            TSType::TSUnionType(union) => {
                for branch in &union.types {
                    Self::collect_type_alias_surface_targets(branch, type_parameters, targets);
                }
            }
            TSType::TSIntersectionType(intersection) => {
                for branch in &intersection.types {
                    Self::collect_type_alias_surface_targets(branch, type_parameters, targets);
                }
            }
            TSType::TSParenthesizedType(parenthesized) => {
                Self::collect_type_alias_surface_targets(
                    &parenthesized.type_annotation,
                    type_parameters,
                    targets,
                );
            }
            _ => {}
        }
    }

    fn collect_literal_type_strings(ty: &TSType<'_>, values: &mut Vec<String>) {
        match ty {
            TSType::TSLiteralType(literal) => {
                if let TSLiteral::StringLiteral(value) = &literal.literal {
                    values.push(value.value.text().into_owned());
                }
            }
            TSType::TSUnionType(union) => {
                for branch in &union.types {
                    Self::collect_literal_type_strings(branch, values);
                }
            }
            TSType::TSParenthesizedType(parenthesized) => {
                Self::collect_literal_type_strings(&parenthesized.type_annotation, values);
            }
            _ => {}
        }
    }

    fn collect_pick_member_accesses(
        ty: &TSType<'_>,
        type_parameters: &FxHashSet<&str>,
        accesses: &mut Vec<MemberAccess>,
    ) {
        match ty {
            TSType::TSTypeReference(reference) => {
                let Some((name, _)) = type_name_root(&reference.type_name) else {
                    return;
                };
                if name != "Pick" {
                    if matches!(
                        name.as_str(),
                        "Partial" | "Required" | "Readonly" | "NonNullable"
                    ) && let Some(first) = reference
                        .type_arguments
                        .as_deref()
                        .and_then(|arguments| arguments.params.first())
                    {
                        Self::collect_pick_member_accesses(first, type_parameters, accesses);
                    }
                    return;
                }
                let Some(arguments) = reference.type_arguments.as_deref() else {
                    return;
                };
                let (Some(target), Some(keys)) =
                    (arguments.params.first(), arguments.params.get(1))
                else {
                    return;
                };
                let mut targets = Vec::new();
                Self::collect_type_alias_surface_targets(target, type_parameters, &mut targets);
                let mut members = Vec::new();
                Self::collect_literal_type_strings(keys, &mut members);
                accesses.extend(targets.into_iter().flat_map(|object| {
                    members.iter().cloned().map(move |member| MemberAccess {
                        object: object.clone(),
                        member,
                    })
                }));
            }
            TSType::TSUnionType(union) => {
                for branch in &union.types {
                    Self::collect_pick_member_accesses(branch, type_parameters, accesses);
                }
            }
            TSType::TSIntersectionType(intersection) => {
                for branch in &intersection.types {
                    Self::collect_pick_member_accesses(branch, type_parameters, accesses);
                }
            }
            TSType::TSParenthesizedType(parenthesized) => {
                Self::collect_pick_member_accesses(
                    &parenthesized.type_annotation,
                    type_parameters,
                    accesses,
                );
            }
            _ => {}
        }
    }

    pub(super) fn record_type_alias_surface_targets(&mut self, alias: &TSTypeAliasDeclaration<'_>) {
        let type_parameters: FxHashSet<&str> = alias
            .type_parameters
            .as_deref()
            .into_iter()
            .flat_map(|parameters| &parameters.params)
            .map(|parameter| parameter.name.name.as_str())
            .collect();
        let mut targets = Vec::new();
        Self::collect_type_alias_surface_targets(
            &alias.type_annotation,
            &type_parameters,
            &mut targets,
        );
        targets.sort_unstable();
        targets.dedup();
        self.semantic_facts
            .extend(targets.into_iter().map(|target_name| {
                SemanticFact::TypeAliasSurfaceTarget(TypeAliasSurfaceTargetFact {
                    alias_name: alias.id.name.to_string(),
                    target_name,
                })
            }));
        let mut picked_members = Vec::new();
        Self::collect_pick_member_accesses(
            &alias.type_annotation,
            &type_parameters,
            &mut picked_members,
        );
        self.member_accesses.extend(picked_members);
    }

    fn remove_type_parameter_refs(
        refs: &mut Vec<(String, Span)>,
        type_parameters: Option<&TSTypeParameterDeclaration<'_>>,
    ) {
        let Some(type_parameters) = type_parameters else {
            return;
        };
        refs.retain(|(name, _)| {
            !type_parameters
                .params
                .iter()
                .any(|parameter| parameter.name.name == name.as_str())
        });
    }

    pub(super) fn record_local_type_declaration(&mut self, name: &str, span: Span) {
        self.local_type_declarations.push(LocalTypeDeclaration {
            name: name.to_string(),
            span,
        });
    }

    /// Keep the first declaration and source order while avoiding a growing
    /// linear scan for every declaration visited in large type-heavy modules.
    pub(crate) fn deduplicate_local_type_declarations(
        declarations: &mut Vec<LocalTypeDeclaration>,
    ) {
        let keep = {
            let mut seen = FxHashSet::default();
            seen.reserve(declarations.len());
            declarations
                .iter()
                .map(|declaration| seen.insert(declaration.name.as_str()))
                .collect::<Vec<_>>()
        };
        let mut keep = keep.into_iter();
        declarations.retain(|_| keep.next().unwrap_or(false));
    }

    pub(super) fn record_local_signature_refs(
        &mut self,
        owner_name: &str,
        refs: Vec<(String, Span)>,
    ) {
        self.push_local_signature_refs(owner_name, refs, false);
    }

    /// Record the types of a `satisfies` clause on a module-level binding.
    /// These types are in use, but they do not form the type of the binding.
    pub(super) fn record_local_satisfies_refs(
        &mut self,
        owner_name: &str,
        refs: Vec<(String, Span)>,
    ) {
        self.push_local_signature_refs(owner_name, refs, true);
    }

    fn push_local_signature_refs(
        &mut self,
        owner_name: &str,
        refs: Vec<(String, Span)>,
        from_satisfies: bool,
    ) {
        self.local_signature_type_references
            .extend(refs.into_iter().map(|(type_name, span)| {
                super::super::LocalSignatureTypeReference {
                    owner_name: owner_name.to_string(),
                    type_name,
                    span,
                    from_satisfies,
                }
            }));
    }

    pub(super) fn record_public_signature_refs(
        &mut self,
        export_name: &str,
        refs: Vec<(String, Span)>,
    ) {
        self.public_signature_type_references
            .extend(
                refs.into_iter()
                    .map(|(type_name, span)| PublicSignatureTypeReference {
                        export_name: export_name.to_string(),
                        type_name,
                        span,
                        from_satisfies: false,
                    }),
            );
    }

    fn collect_type_refs_from_annotation(annotation: &TSTypeAnnotation<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        collector.visit_ts_type_annotation(annotation);
        collector.refs
    }

    pub(super) fn collect_function_signature_refs(function: &Function<'_>) -> Vec<(String, Span)> {
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
        } else if let Some(body) = function.body.as_deref() {
            collect_inferred_return_refs(body, &mut collector.refs);
        }
        Self::remove_type_parameter_refs(&mut collector.refs, function.type_parameters.as_deref());
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
        } else if let Some(expression) = arrow.get_expression() {
            collect_returned_expression_refs(
                expression,
                &FxHashMap::default(),
                &mut collector.refs,
            );
        } else if let Some(body) = arrow.get_function_body() {
            collect_inferred_return_refs(body, &mut collector.refs);
        }
        Self::remove_type_parameter_refs(&mut collector.refs, arrow.type_parameters.as_deref());
        collector.refs
    }

    pub(super) fn collect_variable_signature_refs(
        declarator: &VariableDeclarator<'_>,
    ) -> Vec<(String, Span)> {
        let mut refs = Vec::new();
        if let Some(annotation) = declarator.type_annotation.as_deref() {
            refs.extend(Self::collect_type_refs_from_annotation(annotation));
        }
        if let Some(init) = &declarator.init {
            Self::collect_initializer_signature_refs(
                Self::strip_satisfies_clauses(init),
                &mut refs,
            );
        }
        refs
    }

    /// Collect the types that shape the inferred type of a variable
    /// initializer. A type assertion sets the type directly. A call or `new`
    /// wrapper takes its type from its type arguments and from the signature
    /// of a function argument, for example `memo(function C(p: Props) {})`.
    fn collect_initializer_signature_refs(init: &Expression<'_>, refs: &mut Vec<(String, Span)>) {
        match init {
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::collect_initializer_signature_refs(&parenthesized.expression, refs);
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::collect_initializer_signature_refs(&non_null.expression, refs);
            }
            Expression::TSAsExpression(assertion) => {
                Self::collect_asserted_type_refs(&assertion.type_annotation, refs);
            }
            Expression::TSTypeAssertion(assertion) => {
                Self::collect_asserted_type_refs(&assertion.type_annotation, refs);
            }
            Expression::ArrowFunctionExpression(arrow) => {
                refs.extend(Self::collect_arrow_signature_refs(arrow));
            }
            Expression::FunctionExpression(function) => {
                refs.extend(Self::collect_function_signature_refs(function));
            }
            Expression::CallExpression(call) => {
                Self::collect_wrapper_call_signature_refs(
                    call.type_arguments.as_deref(),
                    &call.arguments,
                    refs,
                );
            }
            Expression::NewExpression(new_expression) => {
                Self::collect_wrapper_call_signature_refs(
                    new_expression.type_arguments.as_deref(),
                    &new_expression.arguments,
                    refs,
                );
            }
            _ => {}
        }
    }

    fn collect_asserted_type_refs(ty: &TSType<'_>, refs: &mut Vec<(String, Span)>) {
        let mut collector = SignatureTypeCollector::default();
        collector.visit_ts_type(ty);
        refs.extend(collector.refs);
    }

    fn collect_wrapper_call_signature_refs(
        type_arguments: Option<&TSTypeParameterInstantiation<'_>>,
        arguments: &[Argument<'_>],
        refs: &mut Vec<(String, Span)>,
    ) {
        if let Some(type_arguments) = type_arguments {
            let mut collector = SignatureTypeCollector::default();
            collector.visit_ts_type_parameter_instantiation(type_arguments);
            refs.extend(collector.refs);
        }
        for argument in arguments {
            match argument {
                Argument::ArrowFunctionExpression(arrow) => {
                    refs.extend(Self::collect_arrow_signature_refs(arrow));
                }
                Argument::FunctionExpression(function) => {
                    refs.extend(Self::collect_function_signature_refs(function));
                }
                _ => {}
            }
        }
    }

    /// Return the value under any `satisfies` clauses and parentheses. A
    /// `satisfies` clause does not change the type of the value.
    fn strip_satisfies_clauses<'b>(init: &'b Expression<'b>) -> &'b Expression<'b> {
        match init.without_parentheses() {
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::strip_satisfies_clauses(&satisfies.expression).without_parentheses()
            }
            _ => init,
        }
    }

    /// Collect the types of the `satisfies` clauses on a variable initializer,
    /// for example `Provider` in `["a"] as const satisfies readonly Provider[]`.
    pub(super) fn collect_variable_satisfies_refs(
        declarator: &VariableDeclarator<'_>,
    ) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        let mut current = declarator.init.as_ref();
        while let Some(expression) = current {
            current = match expression.without_parentheses() {
                Expression::TSSatisfiesExpression(satisfies) => {
                    collector.visit_ts_type(&satisfies.type_annotation);
                    Some(&satisfies.expression)
                }
                Expression::TSAsExpression(assertion) => Some(&assertion.expression),
                Expression::TSTypeAssertion(assertion) => Some(&assertion.expression),
                Expression::TSNonNullExpression(non_null) => Some(&non_null.expression),
                _ => None,
            };
        }
        collector.refs
    }

    /// Collect signature type references from a class's heritage clauses: type
    /// parameters, the `extends` super class plus its type arguments, and each
    /// `implements` interface plus its type arguments.
    fn collect_class_heritage_signature_refs(
        class: &Class<'_>,
        collector: &mut SignatureTypeCollector,
    ) {
        if let Some(type_parameters) = class.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        if let Some(heritage) = class.heritage.as_ref() {
            if let Some((name, span)) = expression_root_name(&heritage.expression) {
                collector.refs.push((name, span));
            }
            if let Some(type_arguments) = heritage.type_arguments.as_deref() {
                collector.visit_ts_type_parameter_instantiation(type_arguments);
            }
        }
        for implemented in &class.implements {
            if let Some((name, span)) = type_name_root(&implemented.expression) {
                collector.refs.push((name, span));
            }
            if let Some(type_arguments) = implemented.type_arguments.as_deref() {
                collector.visit_ts_type_parameter_instantiation(type_arguments);
            }
        }
    }

    pub(super) fn collect_class_signature_refs(class: &Class<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        Self::collect_class_heritage_signature_refs(class, &mut collector);
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
        Self::remove_type_parameter_refs(&mut collector.refs, class.type_parameters.as_deref());
        collector.refs
    }

    pub(super) fn collect_interface_signature_refs(
        iface: &TSInterfaceDeclaration<'_>,
    ) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = iface.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        for heritage in &iface.extends {
            if let Some((name, span)) = type_name_root(&heritage.type_name) {
                collector.refs.push((name, span));
            }
            if let Some(type_arguments) = heritage.type_arguments.as_deref() {
                collector.visit_ts_type_parameter_instantiation(type_arguments);
            }
        }
        collector.visit_ts_interface_body(&iface.body);
        Self::remove_type_parameter_refs(&mut collector.refs, iface.type_parameters.as_deref());
        collector.refs
    }

    pub(super) fn collect_type_alias_signature_refs(
        alias: &TSTypeAliasDeclaration<'_>,
    ) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = alias.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        collector.visit_ts_type(&alias.type_annotation);
        Self::remove_type_parameter_refs(&mut collector.refs, alias.type_parameters.as_deref());
        collector.refs
    }

    /// Record the class bindings that code can read before the declaration in
    /// source order: the element type of each typed array field or parameter
    /// property, and the `this` return binding of each method.
    pub(super) fn preseed_class_bindings(&mut self, class: &Class<'_>) {
        for element in &class.body.body {
            if let ClassElement::PropertyDefinition(prop) = element {
                if prop.r#static {
                    continue;
                }
                let name = prop.key.static_name().map_or_else(
                    || match &prop.key {
                        PropertyKey::PrivateIdentifier(id) => Some(format!("#{}", id.name)),
                        _ => None,
                    },
                    |name| Some(name.text().into_owned()),
                );
                if let Some(name) = name
                    && let Some(annotation) = prop.type_annotation.as_deref()
                    && let Some(element_type) =
                        array_element_type_from_type(&annotation.type_annotation)
                {
                    self.iterable_element_types
                        .insert(self.this_member_key(&name), element_type);
                }
                continue;
            }
            let ClassElement::MethodDefinition(method) = element else {
                continue;
            };
            self.record_this_return_binding(method);
            if method.kind != MethodDefinitionKind::Constructor {
                continue;
            }
            for param in &method.value.params.items {
                if param.accessibility.is_none() && !param.readonly {
                    continue;
                }
                let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                    continue;
                };
                let Some(annotation) = param.type_annotation.as_deref() else {
                    continue;
                };
                if let Some(element_type) =
                    array_element_type_from_type(&annotation.type_annotation)
                {
                    self.iterable_element_types
                        .insert(self.this_member_key(id.name.as_str()), element_type);
                }
            }
        }
    }

    /// Prepare the visit of a method body. Set the method span and reset the
    /// independent `this` depth. Return the previous span and depth, which the
    /// caller restores after the walk.
    pub(super) fn enter_method_definition(
        &mut self,
        method: &MethodDefinition<'_>,
    ) -> (Option<Span>, usize) {
        let previous_span = self.class_method_function_span.replace(method.value.span);
        let previous_depth = std::mem::replace(&mut self.independent_this_depth, 0);
        (previous_span, previous_depth)
    }

    /// Record a `this` binding for an instance method or getter that declares
    /// a named return type: `this.<name>()` for a method, `this.<name>` for a
    /// getter. A chain such as `this.current()?.onDown?.()` inside the class
    /// then credits `ReturnType.onDown` (issue #3960). The key is scoped to
    /// the enclosing class, and only code of that class reaches `this`, so a
    /// private or a `#`-private member also gives a binding. A static member
    /// gives no binding. A return type that names a type parameter of the
    /// method gives no binding, because that name has no meaning outside the
    /// method. The class visit calls this for each method before it walks the
    /// class body. Thus a local such as `const old = this.current()` resolves
    /// also when the method comes after the call site (issue #4033).
    pub(super) fn record_this_return_binding(&mut self, method: &MethodDefinition<'_>) {
        let call_suffix = match method.kind {
            MethodDefinitionKind::Method => "()",
            MethodDefinitionKind::Get => "",
            _ => return,
        };
        if method.r#static || self.class_scope_stack.is_empty() {
            return;
        }
        let Some(type_annotation) = method.value.return_type.as_deref() else {
            return;
        };
        let name = match &method.key {
            PropertyKey::PrivateIdentifier(id) => format!("#{}", id.name),
            key => match key.static_name() {
                Some(name) => name.text().into_owned(),
                None => return,
            },
        };
        let names_method_type_param = extract_type_annotation_name(type_annotation)
            .zip(method.value.type_parameters.as_deref())
            .is_some_and(|(type_name, params)| {
                params
                    .params
                    .iter()
                    .any(|param| param.name.name.as_str() == type_name)
            });
        if names_method_type_param {
            return;
        }
        let key = self.this_member_key(&format!("{name}{call_suffix}"));
        self.record_typed_binding(&key, type_annotation);
    }

    /// Bind a local to the declared return type of a method of the enclosing
    /// class: `const old = this.current();` binds `old` to `Tool` when the
    /// class declares `current(): Tool | undefined`. A later
    /// `old?.deactivate?.()` then credits `Tool.deactivate`, as the direct
    /// `this.current()?.deactivate?.()` does (issue #4033).
    ///
    /// Only a call with no arguments directly on `this` gives a binding. A
    /// cast (`as`), a call with arguments, and a call in a function with its
    /// own `this` give no binding.
    pub(super) fn record_this_method_result_alias(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        if self.class_scope_stack.is_empty() || self.independent_this_depth > 0 {
            return;
        }
        let Some(name) = this_method_call_name(init) else {
            return;
        };
        let key = self.this_member_key(&format!("{name}()"));
        if let Some(BindingTarget::Class(type_name)) = self.binding_target_names.get(&key) {
            let type_name = type_name.clone();
            self.insert_class_binding_target(id.name.to_string(), type_name);
        }
    }

    pub(super) fn record_typed_binding(
        &mut self,
        binding_name: &str,
        type_annotation: &TSTypeAnnotation<'_>,
    ) {
        if let Some(factory) = self.store_factory_for_type(&type_annotation.type_annotation) {
            self.insert_class_binding_target(binding_name.to_string(), factory);
            self.store_instance_locals.insert(binding_name.to_string());
        } else if let Some(type_name) = extract_type_annotation_name(type_annotation)
            && let Some(resolved) = self.resolve_class_type_param(&type_name)
        {
            let origin = self.resolve_class_type_param_origin(
                &type_name,
                super::super::helpers::extract_type_reference_origin(
                    &type_annotation.type_annotation,
                ),
            );
            self.record_binding_class_proof(binding_name, &resolved, origin);
            self.insert_class_binding_target(binding_name.to_string(), resolved);
        }

        self.record_typed_nested_bindings(binding_name, type_annotation);
    }

    pub(super) fn record_typed_nested_bindings(
        &mut self,
        binding_name: &str,
        type_annotation: &TSTypeAnnotation<'_>,
    ) {
        for (property_path, type_name, origin) in
            super::super::helpers::extract_nested_type_bindings(type_annotation)
        {
            if let Some(factory) = self.store_factory_for_type_name(&type_name) {
                self.insert_class_binding_target(
                    format!("{binding_name}.{property_path}"),
                    factory,
                );
                continue;
            }
            let Some(resolved) = self.resolve_class_type_param(&type_name) else {
                continue;
            };
            self.record_binding_class_proof(
                &format!("{binding_name}.{property_path}"),
                &resolved,
                self.resolve_class_type_param_origin(&type_name, origin),
            );
            self.insert_class_binding_target(format!("{binding_name}.{property_path}"), resolved);
        }
    }

    /// Record destructured bindings with type annotations.
    pub(super) fn record_typed_destructure_binding(
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
                let Some(class_name) = properties.get(&key) else {
                    continue;
                };
                if let Some(factory) = self.store_factory_for_type_name(&class_name.name) {
                    self.insert_class_binding_target(local.clone(), factory);
                    self.store_instance_locals.insert(local);
                    continue;
                }
                let origin = super::super::helpers::extract_nested_type_bindings(type_annotation)
                    .into_iter()
                    .find(|(path, _, _)| path == &key)
                    .and_then(|(_, _, origin)| origin);
                self.record_binding_class_proof(&local, &class_name.name, origin);
                self.insert_class_binding_target_if_absent(local, class_name.name.clone());
            }
        } else if let Some(type_name) = extract_type_annotation_name(type_annotation) {
            for (local, key) in bindings {
                if let Some(target) = self
                    .interface_property_types
                    .get(&type_name)
                    .and_then(|properties| properties.get(&key))
                    .cloned()
                {
                    let owner_reference = super::super::helpers::extract_type_reference_origin(
                        &type_annotation.type_annotation,
                    );
                    if let (Some(reference), Some(owner)) = (owner_reference, target.owner_span) {
                        self.record_binding_class_proof(
                            &local,
                            &target.name,
                            target.reference_span,
                        );
                        self.add_direct_binding_requirement(&local, reference, owner);
                    }
                }
                self.pending_typed_destructures
                    .push((local, key, type_name.clone()));
            }
        }
    }
}

/// A function-valued binding declared directly in a function body.
#[derive(Clone, Copy)]
enum LocalCallable<'b, 'a> {
    Function(&'b Function<'a>),
    Arrow(&'b ArrowFunctionExpression<'a>),
}

/// Collect type references that reach the inferred return type of a function
/// without a return annotation. Only the returned values count: inline
/// functions, `as` assertions, and local functions that a return statement
/// names. Type annotations on other body values stay private, so a type that
/// only annotates a local value keeps its unused finding.
fn collect_inferred_return_refs(body: &FunctionBody<'_>, refs: &mut Vec<(String, Span)>) {
    let locals = collect_local_callables(body);
    let mut collector = ReturnedValueCollector {
        locals: &locals,
        refs,
    };
    collector.visit_function_body(body);
}

fn collect_local_callables<'b, 'a>(
    body: &'b FunctionBody<'a>,
) -> FxHashMap<&'b str, LocalCallable<'b, 'a>> {
    let mut locals = FxHashMap::default();
    for statement in &body.statements {
        match statement {
            Statement::FunctionDeclaration(function) => {
                if let Some(id) = function.id.as_ref() {
                    locals.insert(id.name.as_str(), LocalCallable::Function(function));
                }
            }
            Statement::VariableDeclaration(declaration) => {
                for declarator in &declaration.declarations {
                    let (BindingPattern::BindingIdentifier(id), Some(init)) =
                        (&declarator.id, &declarator.init)
                    else {
                        continue;
                    };
                    let callable = match init.without_parentheses() {
                        Expression::ArrowFunctionExpression(arrow) => LocalCallable::Arrow(arrow),
                        Expression::FunctionExpression(function) => {
                            LocalCallable::Function(function)
                        }
                        _ => continue,
                    };
                    locals.insert(id.name.as_str(), callable);
                }
            }
            _ => {}
        }
    }
    locals
}

fn collect_returned_expression_refs(
    expression: &Expression<'_>,
    locals: &FxHashMap<&str, LocalCallable<'_, '_>>,
    refs: &mut Vec<(String, Span)>,
) {
    match expression {
        Expression::ParenthesizedExpression(inner) => {
            collect_returned_expression_refs(&inner.expression, locals, refs);
        }
        Expression::TSSatisfiesExpression(inner) => {
            collect_returned_expression_refs(&inner.expression, locals, refs);
        }
        Expression::TSNonNullExpression(inner) => {
            collect_returned_expression_refs(&inner.expression, locals, refs);
        }
        Expression::TSAsExpression(assertion) => {
            let mut collector = SignatureTypeCollector::default();
            collector.visit_ts_type(&assertion.type_annotation);
            refs.extend(collector.refs);
        }
        Expression::ArrowFunctionExpression(arrow) => {
            refs.extend(ModuleInfoExtractor::collect_arrow_signature_refs(arrow));
        }
        Expression::FunctionExpression(function) => {
            refs.extend(ModuleInfoExtractor::collect_function_signature_refs(
                function,
            ));
        }
        Expression::Identifier(identifier) => match locals.get(identifier.name.as_str()) {
            Some(LocalCallable::Function(function)) => {
                refs.extend(ModuleInfoExtractor::collect_function_signature_refs(
                    function,
                ));
            }
            Some(LocalCallable::Arrow(arrow)) => {
                refs.extend(ModuleInfoExtractor::collect_arrow_signature_refs(arrow));
            }
            None => {}
        },
        Expression::ObjectExpression(object) => {
            for property in &object.properties {
                if let ObjectPropertyKind::ObjectProperty(property) = property {
                    collect_returned_expression_refs(&property.value, locals, refs);
                }
            }
        }
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                if let Some(element) = element.as_expression() {
                    collect_returned_expression_refs(element, locals, refs);
                }
            }
        }
        Expression::ConditionalExpression(conditional) => {
            collect_returned_expression_refs(&conditional.consequent, locals, refs);
            collect_returned_expression_refs(&conditional.alternate, locals, refs);
        }
        Expression::LogicalExpression(logical) => {
            collect_returned_expression_refs(&logical.left, locals, refs);
            collect_returned_expression_refs(&logical.right, locals, refs);
        }
        _ => {}
    }
}

/// Visit the return statements of one function body. Nested functions and
/// classes own their return statements, so the walk does not enter them.
struct ReturnedValueCollector<'r, 'b, 'a> {
    locals: &'r FxHashMap<&'b str, LocalCallable<'b, 'a>>,
    refs: &'r mut Vec<(String, Span)>,
}

impl<'a> Visit<'a> for ReturnedValueCollector<'_, '_, '_> {
    fn visit_return_statement(&mut self, statement: &ReturnStatement<'a>) {
        if let Some(argument) = &statement.argument {
            collect_returned_expression_refs(argument, self.locals, self.refs);
        }
    }

    fn visit_function(&mut self, _function: &Function<'a>, _flags: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _arrow: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _class: &Class<'a>) {}
}

/// The method name of a call with no arguments on `this`: `current` for
/// `this.current()` or `this.current?.()`, `#current` for `this.#current()`.
/// Parentheses and a non-null assertion around the call are transparent. Any
/// other expression, a cast included, gives `None`.
fn this_method_call_name(init: &Expression<'_>) -> Option<String> {
    let mut expr = init;
    let call = loop {
        match expr {
            Expression::ParenthesizedExpression(wrapper) => expr = &wrapper.expression,
            Expression::TSNonNullExpression(wrapper) => expr = &wrapper.expression,
            Expression::CallExpression(call) => break call,
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::CallExpression(call) => break call,
                _ => return None,
            },
            _ => return None,
        }
    };
    if !call.arguments.is_empty() {
        return None;
    }
    match &call.callee {
        Expression::StaticMemberExpression(member)
            if matches!(member.object, Expression::ThisExpression(_)) =>
        {
            Some(member.property.name.to_string())
        }
        Expression::PrivateFieldExpression(member)
            if matches!(member.object, Expression::ThisExpression(_)) =>
        {
            Some(format!("#{}", member.field.name))
        }
        _ => None,
    }
}
