//! Type-signature and typed-binding helpers for the visitor implementation.

use super::visit_helpers::*;
use super::*;
use crate::{SemanticFact, TypeAliasSurfaceTargetFact};

impl ModuleInfoExtractor {
    fn collect_type_alias_surface_targets(
        ty: &TSType<'_>,
        type_parameters: &FxHashSet<&str>,
        targets: &mut Vec<String>,
    ) {
        match ty {
            TSType::TSTypeReference(reference) => {
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
                    values.push(value.value.to_string());
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
        if self
            .local_type_declarations
            .iter()
            .any(|decl| decl.name == name)
        {
            return;
        }
        self.local_type_declarations.push(LocalTypeDeclaration {
            name: name.to_string(),
            span,
        });
    }

    pub(super) fn record_local_signature_refs(
        &mut self,
        owner_name: &str,
        refs: Vec<(String, Span)>,
    ) {
        self.local_signature_type_references
            .extend(refs.into_iter().map(|(type_name, span)| {
                super::super::LocalSignatureTypeReference {
                    owner_name: owner_name.to_string(),
                    type_name,
                    span,
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
            match init {
                Expression::ArrowFunctionExpression(arrow) => {
                    refs.extend(Self::collect_arrow_signature_refs(arrow));
                }
                Expression::FunctionExpression(function) => {
                    refs.extend(Self::collect_function_signature_refs(function));
                }
                _ => {}
            }
        }
        refs
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
        if let Some(super_class) = class.super_class.as_ref()
            && let Some((name, span)) = expression_root_name(super_class)
        {
            collector.refs.push((name, span));
        }
        if let Some(type_arguments) = class.super_type_arguments.as_deref() {
            collector.visit_ts_type_parameter_instantiation(type_arguments);
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
            if let Some((name, span)) = expression_root_name(&heritage.expression) {
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
            self.insert_class_binding_target(binding_name.to_string(), resolved);
        }

        self.record_typed_nested_bindings(binding_name, type_annotation);
    }

    pub(super) fn record_typed_nested_bindings(
        &mut self,
        binding_name: &str,
        type_annotation: &TSTypeAnnotation<'_>,
    ) {
        for (property_path, type_name) in extract_nested_type_bindings(type_annotation) {
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
                if let Some(factory) = self.store_factory_for_type_name(class_name) {
                    self.insert_class_binding_target(local.clone(), factory);
                    self.store_instance_locals.insert(local);
                    continue;
                }
                self.insert_class_binding_target_if_absent(local, class_name.clone());
            }
        } else if let Some(type_name) = extract_type_annotation_name(type_annotation) {
            for (local, key) in bindings {
                self.pending_typed_destructures
                    .push((local, key, type_name.clone()));
            }
        }
    }
}
