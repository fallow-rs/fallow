//! Type-signature and typed-binding helpers for the visitor implementation.

use super::visit_helpers::*;
use super::*;

impl ModuleInfoExtractor {
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

    pub(super) fn collect_type_refs_from_annotation(
        annotation: &TSTypeAnnotation<'_>,
    ) -> Vec<(String, Span)> {
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
        collector.refs
    }

    pub(super) fn collect_arrow_signature_refs(
        arrow: &ArrowFunctionExpression<'_>,
    ) -> Vec<(String, Span)> {
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
    pub(super) fn collect_class_heritage_signature_refs(
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
