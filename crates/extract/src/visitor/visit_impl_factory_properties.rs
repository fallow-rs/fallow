//! Source-time property receivers of strict object-returning factories.

use fallow_types::extract::{FactoryPropertySource, FactoryReturnObjectPropertySourceFact};
#[allow(
    clippy::wildcard_imports,
    reason = "factory acquisition matches AST forms"
)]
use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};

use super::super::{
    DirectBindingValue, ExportName, FactoryPropertyOrigin, FactoryPropertyProof,
    ModuleInfoExtractor, SemanticFact,
};
use super::unwrap_static_expr;
use super::visit_factory_returns::object_literal_from_return;
use super::visit_projections::MAX_PROJECTION_VALUES;
use crate::function_body::BodyRef;

/// Whether the module can produce a captured callback proof: a `new` expression
/// and a synchronous callback whose single return is a bare identifier, in a
/// position that a callback wrapper reads (call argument, variable initializer
/// or object property value). The gate opens binding-provenance tracking for the
/// whole module, so it stays closed for ordinary functions that return a local.
pub(super) fn program_has_captured_callback(program: &Program<'_>) -> bool {
    use oxc_ast_visit::{Visit, walk};

    #[derive(Default)]
    struct CapturedCallback {
        new_expression: bool,
        callback: bool,
    }
    impl CapturedCallback {
        fn done(&self) -> bool {
            self.new_expression && self.callback
        }

        fn check(&mut self, expression: &Expression<'_>) {
            self.callback |= returns_captured_identifier(expression);
        }
    }
    impl<'a> Visit<'a> for CapturedCallback {
        fn visit_statement(&mut self, statement: &Statement<'a>) {
            if !self.done() {
                walk::walk_statement(self, statement);
            }
        }

        fn visit_new_expression(&mut self, expression: &NewExpression<'a>) {
            self.new_expression = true;
            walk::walk_new_expression(self, expression);
        }

        fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
            for argument in &call.arguments {
                if let Some(expression) = argument.as_expression() {
                    self.check(expression);
                }
            }
            walk::walk_call_expression(self, call);
        }

        fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
            if let Some(init) = &declarator.init {
                self.check(init);
            }
            walk::walk_variable_declarator(self, declarator);
        }

        fn visit_object_property(&mut self, property: &ObjectProperty<'a>) {
            self.check(&property.value);
            walk::walk_object_property(self, property);
        }
    }

    let mut visitor = CapturedCallback::default();
    visitor.visit_program(program);
    visitor.done()
}

/// Mirrors the body shapes that `callback_capture_proof` accepts.
fn returns_captured_identifier(expression: &Expression<'_>) -> bool {
    let body = match unwrap_static_expr(expression) {
        Expression::ArrowFunctionExpression(function) if !function.r#async => {
            BodyRef::arrow(&function.body)
        }
        Expression::FunctionExpression(function) if !function.r#async && !function.generator => {
            let Some(body) = function.body.as_deref() else {
                return false;
            };
            BodyRef::Block(body)
        }
        _ => return false,
    };
    let returned = match body {
        BodyRef::Concise(expression) => Some(expression),
        BodyRef::Block(body) => {
            (super::visit_factory_returns::count_returns_in_statements(&body.statements) == 1)
                .then(|| {
                    super::visit_factory_returns::first_return_arg_in_statements(&body.statements)
                })
                .flatten()
        }
    };
    returned.is_some_and(|expression| {
        matches!(unwrap_static_expr(expression), Expression::Identifier(_))
    })
}

pub(super) fn program_has_object_factory(program: &Program<'_>) -> bool {
    program.body.iter().any(|statement| {
        let declaration = match statement {
            Statement::ExportDeclaration(export) => Some(&export.declaration),
            _ => statement.as_declaration(),
        };
        match declaration {
            Some(Declaration::FunctionDeclaration(function)) => function
                .body
                .as_deref()
                .is_some_and(|body| object_literal_from_return(BodyRef::Block(body)).is_some()),
            Some(Declaration::VariableDeclaration(declaration)) => {
                declaration.declarations.iter().any(|declarator| {
                    match declarator.init.as_ref().map(unwrap_static_expr) {
                        Some(Expression::ArrowFunctionExpression(arrow)) => {
                            object_literal_from_return(BodyRef::arrow(&arrow.body)).is_some()
                        }
                        Some(Expression::FunctionExpression(function)) => {
                            function.body.as_deref().is_some_and(|body| {
                                object_literal_from_return(BodyRef::Block(body)).is_some()
                            })
                        }
                        _ => false,
                    }
                })
            }
            _ => false,
        }
    })
}

fn property_observations(
    object: &ObjectExpression<'_>,
    prefix: &str,
    out: &mut Vec<(String, Span)>,
) -> Option<()> {
    let mut keys = rustc_hash::FxHashSet::default();
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return None;
        };
        let key = property.key.static_name()?;
        if key.is_empty()
            || key.contains('.')
            || !keys.insert(key.to_string())
            || property.method
            || out.len() >= MAX_PROJECTION_VALUES
        {
            return None;
        }
        let path = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        match unwrap_static_expr(&property.value) {
            Expression::ObjectExpression(nested) => property_observations(nested, &path, out)?,
            value => out.push((path, value.span())),
        }
    }
    Some(())
}

fn receiver_path(expression: &Expression<'_>) -> Option<(String, Span)> {
    match unwrap_static_expr(expression) {
        Expression::Identifier(id) => Some((id.name.to_string(), id.span)),
        Expression::StaticMemberExpression(member) => {
            let (parent, reference) = receiver_path(&member.object)?;
            Some((format!("{parent}.{}", member.property.name), reference))
        }
        _ => None,
    }
}

impl ModuleInfoExtractor {
    pub(super) fn prepare_factory_property_imports(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            let Statement::ImportDeclaration(import) = statement else {
                continue;
            };
            for specifier in import.specifiers.iter().flatten() {
                let local = match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(specifier) => &specifier.local,
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                        &specifier.local
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                        &specifier.local
                    }
                };
                self.direct_binding_entry_mut(local.name.as_str())
                    .declaration_span = Some(local.span);
                if let Some(value) = self.allocate_projection_value(DirectBindingValue {
                    factory_callee: Some((local.name.to_string(), local.span)),
                    ..DirectBindingValue::default()
                }) {
                    self.attach_direct_binding_value(local.name.as_str(), value);
                }
            }
        }
    }

    pub(super) fn record_factory_property_class(&mut self, binding: &BindingIdentifier<'_>) {
        if !self.capture_factory_properties {
            return;
        }
        self.record_binding_declaration(binding.name.as_str(), binding.span);
        let mut value = self
            .direct_binding_value(binding.name.as_str())
            .map(|(id, _)| self.projection_value_view(id).clone())
            .unwrap_or_default();
        value.factory_callee = Some((binding.name.to_string(), binding.span));
        if let Some(id) = self.allocate_projection_value(value) {
            self.attach_direct_binding_value(binding.name.as_str(), id);
        } else {
            self.invalidate_direct_binding_proof(binding.name.as_str(), false);
        }
    }

    pub(super) fn prepare_factory_property_observations(
        &mut self,
        declaration: Span,
        params: &FormalParameters<'_>,
        body: BodyRef<'_, '_>,
    ) {
        let Some(object) = object_literal_from_return(body) else {
            return;
        };
        let mut observations = Vec::new();
        if property_observations(object, "", &mut observations).is_none()
            || observations.len()
                > MAX_PROJECTION_VALUES.saturating_sub(self.factory_property_observations.len())
        {
            return;
        }
        let returned_binding = match body {
            BodyRef::Block(body) => {
                super::visit_factory_returns::first_return_arg_in_statements(&body.statements)
                    .and_then(|argument| {
                        if let Expression::Identifier(id) = unwrap_static_expr(argument) {
                            Some((id.name.as_str(), id.span))
                        } else {
                            None
                        }
                    })
            }
            BodyRef::Concise(_) => None,
        };
        for (path, span) in observations {
            if let Some((root, reference)) = returned_binding {
                let destination = format!("{root}.{path}");
                self.factory_property_destinations
                    .insert(span, destination.clone());
                self.factory_property_return_reads
                    .entry(reference)
                    .or_default()
                    .push((span, destination, reference));
            }
            self.factory_property_observations
                .insert(span, (declaration, path, None));
        }
        self.prepare_callback_property_observations(object, params);
    }

    pub(super) fn prepare_factory_result_observation(
        &mut self,
        name: &str,
        binding: Span,
        body: BodyRef<'_, '_>,
    ) {
        let expression = match body {
            BodyRef::Concise(expression) => Some(expression),
            BodyRef::Block(body)
                if super::visit_factory_returns::count_returns_in_statements(&body.statements)
                    == 1 =>
            {
                super::visit_factory_returns::first_return_arg_in_statements(&body.statements)
            }
            BodyRef::Block(_) => None,
        };
        let Some(Expression::Identifier(id)) = expression.map(unwrap_static_expr) else {
            return;
        };
        if self.factory_property_result_observations.len() < MAX_PROJECTION_VALUES {
            self.factory_property_result_observations
                .insert(id.span, (name.to_string(), binding));
        }
    }

    pub(super) fn complete_factory_result_observation(
        &mut self,
        reference: &IdentifierReference<'_>,
    ) {
        let Some((name, declaration)) = self
            .factory_property_result_observations
            .remove(&reference.span)
        else {
            return;
        };
        let Some((source, _)) = self.direct_binding_value(reference.name.as_str()) else {
            return;
        };
        let source = self.projection_value_view(source);
        if source.unknown_properties || !source.factory_constructed {
            return;
        }
        let local_value = source.constructor_owner == self.active_constructor_function;
        let Some(proof) =
            self.direct_binding_class_proof(reference.name.as_str(), Some(reference.span))
        else {
            return;
        };
        let Some(value) = self
            .module_direct_object_binding_targets
            .get(&name)
            .and_then(|entry| entry.initial_producer_value)
        else {
            return;
        };
        let Some(producer) = &mut self.direct_binding_values[value].producer else {
            return;
        };
        if producer.declaration_span == declaration && producer.result.is_none() {
            producer.result = Some(proof);
            if local_value {
                self.factory_completed_value_results.insert(declaration);
            }
        }
    }

    fn factory_property_value(
        &mut self,
        expression: &Expression<'_>,
    ) -> Option<FactoryPropertyProof> {
        let expression = unwrap_static_expr(expression);
        if let Expression::CallExpression(call) = expression {
            let (name, reference, method) = match unwrap_static_expr(&call.callee) {
                Expression::Identifier(id) => (id.name.to_string(), id.span, None),
                Expression::StaticMemberExpression(member) => {
                    let Expression::Identifier(id) = unwrap_static_expr(&member.object) else {
                        return None;
                    };
                    (
                        id.name.to_string(),
                        id.span,
                        Some(member.property.name.to_string()),
                    )
                }
                _ => return None,
            };
            let entry = self.direct_binding_entry(&name)?;
            let binding = entry.declaration_span?;
            let mut name = name;
            let mut declaration = binding;
            let mut value_requirements = super::super::BindingRequirements::Empty;
            if let Some(id) = entry.value {
                let value = self.projection_value_view(id);
                if value.unknown_properties {
                    return None;
                }
                value_requirements = value.requirements;
                if let Some((target, span)) = &value.factory_callee {
                    name.clone_from(target);
                    declaration = *span;
                } else if let Some(producer) = &value.producer {
                    name.clone_from(&producer.name);
                    declaration = producer.declaration_span;
                } else {
                    return None;
                }
            }
            let requirements = self.combine_requirements(
                &[entry.requirements, value_requirements],
                &[(reference, binding)],
            );
            if !self.charge_binding_conditions(call.arguments.len()) {
                return None;
            }
            let callbacks = call
                .arguments
                .iter()
                .take_while(|argument| !matches!(argument, Argument::SpreadElement(_)))
                .enumerate()
                .filter_map(|(index, argument)| {
                    Some((
                        u32::try_from(index).ok()?,
                        self.callback_result_proof(argument.as_expression()?)?,
                    ))
                })
                .collect();
            return Some(FactoryPropertyProof {
                projection: String::new(),
                vue_only: false,
                assigned_class: None,
                origin: FactoryPropertyOrigin::Call {
                    name,
                    declaration,
                    reference,
                    method,
                    callbacks,
                },
                requirements,
            });
        }
        let (path, reference) = receiver_path(expression)?;
        self.factory_property_path(&path, reference)
    }

    fn factory_property_path(
        &mut self,
        path: &str,
        reference: Span,
    ) -> Option<FactoryPropertyProof> {
        if let Some((id, entry)) = self.direct_binding_value(path) {
            let value = self.projection_value_view(id);
            if !value.unknown_properties
                && let Some(mut proof) = value.factory_property.clone()
            {
                proof.requirements = self.combine_requirements(
                    &[proof.requirements, value.requirements, entry.requirements],
                    &[(reference, entry.declaration_span?)],
                );
                return Some(proof);
            }
        }
        let (root, remainder) = path.split_once('.')?;
        let (mut value, entry) = self.direct_binding_value(root)?;
        if !self.projection_value_view(value).unknown_properties
            && let Some(mut proof) = self.projection_value_view(value).factory_property.clone()
            && let FactoryPropertyOrigin::Call { callbacks, .. } = &proof.origin
        {
            let written = !self.factory_property_prefix_is_unwritten(root, remainder);
            if written && callbacks.is_empty() {
                return None;
            }
            proof.vue_only |= written;
            proof.requirements = self.combine_requirements(
                &[proof.requirements, entry.requirements],
                &[(reference, entry.declaration_span?)],
            );
            if !proof.projection.is_empty() {
                proof.projection.push('.');
            }
            proof.projection.push_str(remainder);
            return Some(proof);
        }
        let mut prefix_end = root.len();
        for part in remainder.split('.') {
            let current = self.projection_value_view(value);
            if current.unknown_properties {
                return None;
            }
            let Some(child) = current.properties.get(part) else {
                let proof =
                    self.direct_binding_class_proof(&path[..prefix_end], Some(reference))?;
                return Some(FactoryPropertyProof {
                    projection: String::new(),
                    vue_only: false,
                    assigned_class: None,
                    requirements: proof.requirements,
                    origin: FactoryPropertyOrigin::Typed {
                        proof,
                        path: path[prefix_end + 1..].to_string(),
                    },
                });
            };
            value = *child;
            prefix_end += part.len() + 1;
        }
        // A materialized slot owns its current value, not the original root's type.
        None
    }

    pub(super) fn record_factory_property_declarator(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        expression: &Expression<'_>,
    ) {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        if matches!(
            unwrap_static_expr(expression),
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
        ) {
            if !self.is_module_scope()
                && let Some(callback) = self.callback_result_proof(expression)
            {
                self.retain_callback_result(id.name.as_str(), id.span, callback);
            }
            return;
        }
        if self.has_factory_value_root(id.name.as_str()) {
            self.direct_binding_entry_mut(id.name.as_str())
                .declaration_span = Some(id.span);
        }
        if let Expression::ObjectExpression(object) = unwrap_static_expr(expression) {
            if !self.capture_factory_properties {
                return;
            }
            let mut paths = Vec::new();
            if property_observations(object, id.name.as_str(), &mut paths).is_some()
                && paths.len()
                    <= MAX_PROJECTION_VALUES
                        .saturating_sub(self.factory_property_destinations.len())
            {
                self.record_binding_declaration(id.name.as_str(), id.span);
                for (path, span) in paths {
                    self.factory_property_destinations.insert(span, path);
                }
            }
            return;
        }
        let Some(proof) = self.factory_property_value(expression) else {
            return;
        };
        self.direct_binding_entry_mut(id.name.as_str())
            .declaration_span = Some(id.span);
        self.retain_factory_property(id.name.as_str(), proof);
    }

    fn retain_factory_property(&mut self, path: &str, proof: FactoryPropertyProof) {
        if let Some((value, _)) = self.direct_binding_value(path) {
            let value = self.projection_view_id(value);
            if !self.is_projection_barrier(value)
                && !self.direct_binding_values[value].unknown_properties
            {
                self.direct_binding_values[value]
                    .factory_property
                    .get_or_insert(proof);
                return;
            }
        }
        let Some(value) = self.allocate_projection_value(DirectBindingValue {
            factory_property: Some(proof),
            ..DirectBindingValue::default()
        }) else {
            self.invalidate_direct_binding_proof(path, false);
            return;
        };
        self.attach_direct_binding_value(path, value);
    }

    pub(super) fn capture_factory_property_observation(&mut self, expression: &Expression<'_>) {
        let expression = unwrap_static_expr(expression);
        let span = expression.span();
        if !self.factory_property_observations.contains_key(&span)
            && !self.factory_property_destinations.contains_key(&span)
        {
            return;
        }
        let callback = self
            .factory_property_observations
            .get(&span)
            .and_then(|(_, _, proof)| proof.as_ref())
            .filter(|proof| {
                matches!(
                    proof.origin,
                    FactoryPropertyOrigin::CallbackParameter { .. }
                )
            })
            .cloned();
        let proof = callback.or_else(|| self.factory_property_value(expression));
        if let Some(destination) = self.factory_property_destinations.get(&span).cloned() {
            if let Some(proof) = proof {
                let root = destination.split('.').next().unwrap_or(&destination);
                let Some(declaration) = self
                    .direct_binding_entry(root)
                    .and_then(|entry| entry.declaration_span)
                else {
                    return;
                };
                if !self.prepare_projection_path(&destination, declaration) {
                    self.invalidate_direct_binding_proof(&destination, false);
                    return;
                }
                self.retain_factory_property(&destination, proof);
            }
            return;
        }
        if let Some(observation) = self.factory_property_observations.get_mut(&span) {
            observation.2 = proof;
        }
    }

    pub(super) fn complete_factory_property_return(&mut self, statement: &ReturnStatement<'_>) {
        let Some(argument) = &statement.argument else {
            return;
        };
        let Some(reads) = self
            .factory_property_return_reads
            .remove(&unwrap_static_expr(argument).span())
        else {
            return;
        };
        for (span, path, reference) in reads {
            let proof = self.factory_property_path(&path, reference);
            if let Some(observation) = self.factory_property_observations.get_mut(&span) {
                observation.2 = proof;
            }
        }
    }

    pub(super) fn capture_factory_property_access(
        &mut self,
        object: &Expression<'_>,
        member: &str,
    ) {
        if self.factory_property_accesses.len() >= MAX_PROJECTION_VALUES {
            return;
        }
        let mut receiver = unwrap_static_expr(object);
        let mut properties = Vec::new();
        while let Expression::StaticMemberExpression(property) = receiver {
            properties.push(property.property.name.as_str());
            receiver = unwrap_static_expr(&property.object);
        }
        if matches!(receiver, Expression::CallExpression(_)) && !properties.is_empty() {
            if let Some(proof) = self.factory_property_value(receiver) {
                properties.reverse();
                self.retain_factory_property_access(proof, properties.join("."), member);
            }
            return;
        }
        let Some((path, reference)) = receiver_path(object) else {
            return;
        };
        let Some(proof) = self.factory_property_path(&path, reference) else {
            return;
        };
        if matches!(proof.origin, FactoryPropertyOrigin::Call { .. })
            && !proof.projection.is_empty()
        {
            let projection = proof.projection.clone();
            self.retain_factory_property_access(proof, projection, member);
        }
    }

    fn retain_factory_property_access(
        &mut self,
        proof: FactoryPropertyProof,
        projection: String,
        member: &str,
    ) {
        if let FactoryPropertyOrigin::Call { callbacks, .. } = &proof.origin
            && !callbacks.is_empty()
            && !self.charge_binding_conditions(callbacks.len())
        {
            return;
        }
        self.factory_property_accesses
            .push((proof, projection, member.to_string()));
    }

    fn factory_property_prefix_is_unwritten(&self, root: &str, path: &str) -> bool {
        let Some((root_value, _)) = self.direct_binding_value(root) else {
            return false;
        };
        let mut value = root_value;
        let mut prefix_end = 0;
        for part in path.split('.') {
            prefix_end += part.len();
            let current = self.projection_value_view(value);
            if current.unknown_properties {
                return false;
            }
            let Some(child) = current.properties.get(part) else {
                return true;
            };
            let child_value = self.projection_value_view(*child);
            let Some(super::super::PendingBindingValue::Projection {
                result,
                path: original,
            }) = &child_value.pending
            else {
                return false;
            };
            if child_value.unknown_properties
                || self.projection_view_id(*result) != self.projection_view_id(root_value)
                || original != &path[..prefix_end]
            {
                return false;
            }
            prefix_end += 1;
            value = *child;
        }
        !self.projection_value_view(value).unknown_properties
    }

    pub(super) fn is_factory_property_value(&self, id: usize) -> bool {
        let value = self.projection_value_view(id);
        if value.factory_property.is_some()
            || value.factory_callee.is_some()
            || value.callback_result.is_some()
            || (self.capture_factory_properties && value.proof.is_some())
        {
            return true;
        }
        if let Some(super::super::PendingBindingValue::Projection { result, .. }) = &value.pending {
            let origin = self.projection_value_view(*result);
            return origin.factory_property.is_some() || origin.factory_callee.is_some();
        }
        false
    }

    pub(super) fn has_factory_value_root(&self, path: &str) -> bool {
        let root = path.split('.').next().unwrap_or(path);
        self.direct_binding_value(root)
            .is_some_and(|(id, _)| self.is_factory_property_value(id))
    }

    pub(in crate::visitor) fn resolve_factory_property_observations(&mut self) {
        for (_, (factory, property_path, observation)) in
            std::mem::take(&mut self.factory_property_observations)
        {
            let Some(proof) = observation else {
                continue;
            };
            if !self.admitted_requirements(proof.requirements) || !proof.projection.is_empty() {
                continue;
            }
            let source = if let Some(class) = self.resolve_factory_property_class(&proof.origin) {
                let source = FactoryPropertySource::LocalClass {
                    class_local_name: class.target_name.clone(),
                };
                self.factory_property_classes
                    .entry(factory)
                    .or_default()
                    .insert(property_path.clone(), class);
                source
            } else if let Some(source) = self.resolve_factory_property_origin(&proof.origin) {
                source
            } else {
                continue;
            };
            self.factory_property_sources
                .entry(factory)
                .or_default()
                .insert(property_path.clone(), source.clone());
        }
        let mut emitted = 0;
        for export in &self.exports {
            if export.is_type_only {
                continue;
            }
            let local = match (export.local_name.as_deref(), &export.name) {
                (Some(local), _) => local,
                (None, ExportName::Named(name)) => name.as_str(),
                _ => continue,
            };
            let Some(declaration) = self
                .module_direct_object_binding_targets
                .get(local)
                .and_then(|entry| entry.declaration_span)
            else {
                continue;
            };
            let Some(sources) = self.factory_property_sources.get(&declaration) else {
                continue;
            };
            for (property_path, source) in sources {
                if emitted >= MAX_PROJECTION_VALUES {
                    return;
                }
                self.semantic_facts
                    .push(SemanticFact::FactoryReturnObjectPropertySource(Box::new(
                        FactoryReturnObjectPropertySourceFact {
                            factory_export_name: export.name.to_string(),
                            property_path: property_path.clone(),
                            source: source.clone(),
                        },
                    )));
                emitted += 1;
            }
        }
    }

    pub(in crate::visitor) fn resolve_factory_property_accesses(&mut self) {
        for (proof, path, member) in std::mem::take(&mut self.factory_property_accesses) {
            if !self.admitted_requirements(proof.requirements) {
                continue;
            }
            if let FactoryPropertyOrigin::Call {
                name, declaration, ..
            } = &proof.origin
                && self.factory_import_binding(name, *declaration)
            {
                self.emit_imported_callback_property_access(&proof, &path, &member);
                continue;
            }
            let FactoryPropertyOrigin::Call {
                declaration,
                method,
                callbacks,
                ..
            } = proof.origin
            else {
                continue;
            };
            if method.is_some() || proof.vue_only {
                continue;
            }
            let Some(sources) = self.factory_property_sources.get(&declaration) else {
                continue;
            };
            if let Some(source) = sources.get(&path) {
                let fact = match source {
                    FactoryPropertySource::CallbackParameterReturn { parameter_index } => {
                        self.emit_local_callback_property_access(
                            &callbacks,
                            *parameter_index,
                            member,
                        );
                        continue;
                    }
                    FactoryPropertySource::LocalClass { class_local_name } => {
                        let class_local_name = class_local_name.clone();
                        if let Some(class) = self
                            .factory_property_classes
                            .get(&declaration)
                            .and_then(|properties| properties.get(&path))
                            .cloned()
                        {
                            self.emit_local_subclass_member_access(
                                &super::super::SubclassAccessProof::Known(class),
                                &member,
                            );
                        }
                        self.member_accesses.push(super::super::MemberAccess {
                            object: class_local_name,
                            member,
                        });
                        continue;
                    }
                    FactoryPropertySource::TypedProperty {
                        type_name,
                        property_path,
                    } => SemanticFact::TypedPropertyMemberAccess(
                        fallow_types::extract::TypedPropertyMemberAccessFact {
                            type_name: type_name.clone(),
                            property_path: property_path.clone(),
                            member: member.clone(),
                        },
                    ),
                    FactoryPropertySource::FunctionReturn { callee_name } => {
                        SemanticFact::FactoryFnMemberAccess(
                            fallow_types::extract::FactoryFnMemberAccessFact {
                                callee_name: callee_name.clone(),
                                member: member.clone(),
                                awaited: false,
                            },
                        )
                    }
                    FactoryPropertySource::StaticFactoryReturn {
                        callee_object,
                        callee_method,
                    } => SemanticFact::FactoryCallMemberAccess(
                        fallow_types::extract::FactoryCallMemberAccessFact {
                            callee_object: callee_object.clone(),
                            callee_method: callee_method.clone(),
                            member: member.clone(),
                        },
                    ),
                };
                self.semantic_facts.push(fact);
            }
        }
    }

    fn resolve_factory_property_class(
        &self,
        origin: &FactoryPropertyOrigin,
    ) -> Option<super::super::BindingClassProof> {
        let FactoryPropertyOrigin::Call {
            name,
            declaration,
            reference,
            method,
            ..
        } = origin
        else {
            return None;
        };
        if let Some(method) = method {
            if self.subclass_declarations.get(name)?.binding_span != *declaration {
                return None;
            }
            return self
                .local_class_exports
                .get(name)?
                .members
                .iter()
                .any(|member| {
                    member.name == *method
                        && member.kind == fallow_types::extract::MemberKind::ClassMethod
                        && member.is_instance_returning_static
                })
                .then(|| super::super::BindingClassProof {
                    target_name: name.clone(),
                    target_reference_span: *reference,
                    requirements: super::super::BindingRequirements::Empty,
                    builtin_containers: std::sync::Arc::default(),
                });
        }
        let class = self.strict_factory_return_functions.get(name);
        let value = self
            .module_direct_object_binding_targets
            .get(name)?
            .initial_producer_value?;
        let producer = self.direct_binding_values[value].producer.as_ref()?;
        if producer.declaration_span != *declaration {
            return None;
        }
        let proof = producer.result.as_ref()?;
        ((class == Some(&proof.target_name)
            || self.factory_completed_value_results.contains(declaration))
            && self.admitted_projection_origin(proof))
        .then(|| proof.clone())
    }

    pub(super) fn factory_import_binding(&self, name: &str, declaration: Span) -> bool {
        self.imports.iter().any(|import| {
            !import.is_type_only
                && import.local_name == name
                && import.span.start <= declaration.start
                && import.span.end >= declaration.end
        })
    }

    fn resolve_factory_property_origin(
        &self,
        origin: &FactoryPropertyOrigin,
    ) -> Option<FactoryPropertySource> {
        match origin {
            FactoryPropertyOrigin::CallbackParameter {
                parameter_index,
                declaration,
                reference,
            } => (self.subclass_binding_references.get(reference) == Some(declaration)
                && self.subclass_immutable_references.contains(reference))
            .then_some(FactoryPropertySource::CallbackParameterReturn {
                parameter_index: *parameter_index,
            }),
            FactoryPropertyOrigin::Typed { proof, path } => {
                if path.is_empty() || !self.admitted_projection_origin(proof) {
                    return None;
                }
                Some(FactoryPropertySource::TypedProperty {
                    type_name: proof.target_name.clone(),
                    property_path: path.clone(),
                })
            }
            FactoryPropertyOrigin::Call {
                name,
                declaration,
                method,
                ..
            } => {
                if !self.factory_import_binding(name, *declaration) {
                    return None;
                }
                Some(method.as_ref().map_or_else(
                    || FactoryPropertySource::FunctionReturn {
                        callee_name: name.clone(),
                    },
                    |method| FactoryPropertySource::StaticFactoryReturn {
                        callee_object: name.clone(),
                        callee_method: method.clone(),
                    },
                ))
            }
        }
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use std::fmt::Write;

    #[test]
    fn factory_properties_preserve_local_static_class_identity() {
        let module = crate::tests::parse_ts(
            "import { C } from './c'; class Local extends C { static build(): Local { return new Local(); } } export const createUi = () => ({ page: Local.build() });",
        );
        let properties: Vec<_> = fallow_types::extract::SemanticFactView::new(
            &module.semantic_facts,
            &module.member_accesses,
        )
        .factory_return_object_property_sources()
        .map(|fact| {
            (
                fact.factory_export_name.as_str(),
                fact.property_path.as_str(),
                fact.source.clone(),
            )
        })
        .collect();
        assert_eq!(
            properties,
            [(
                "createUi",
                "page",
                fallow_types::extract::FactoryPropertySource::LocalClass {
                    class_local_name: "Local".into()
                }
            )]
        );
    }

    #[test]
    fn factory_properties_keep_completed_reads_and_saved_values_after_arena_refusal() {
        let mut source = String::from(
            "import { make } from './producer'; export const before = () => ({ page: make() }); export function after() { let page = make(); const saved = page;",
        );
        for index in 0..MAX_PROJECTION_VALUES {
            write!(source, "const pressure{index} = new Unknown();").expect("source fixture");
        }
        source.push_str("page = opaque(); return { page, saved }; }");
        let module = crate::tests::parse_ts(&source);
        let mut properties: Vec<_> = fallow_types::extract::SemanticFactView::new(
            &module.semantic_facts,
            &module.member_accesses,
        )
        .factory_return_object_property_sources()
        .map(|fact| {
            (
                fact.factory_export_name.as_str(),
                fact.property_path.as_str(),
            )
        })
        .collect();
        properties.sort_unstable();
        assert_eq!(properties, [("after", "saved"), ("before", "page")]);
    }

    fn opens_callback_gate(source: &str) -> bool {
        let allocator = oxc_allocator::Allocator::default();
        let program = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts())
            .parse()
            .program;
        program_has_captured_callback(&program)
    }

    #[test]
    fn callback_gate_opens_for_captured_callback_positions() {
        for source in [
            "let c = new C(); wrap(() => c);",
            "let c = new C(); wrap(function () { return c; });",
            "let c = new C(); const getter = () => (c);",
            "let c = new C(); computed({ get: () => c });",
            "wrap(() => c); let c = new C();",
        ] {
            assert!(opens_callback_gate(source), "gate must open: {source}");
        }
    }

    #[test]
    fn callback_gate_stays_closed_without_a_captured_callback() {
        for source in [
            "function f() { const r = compute(); return r; }",
            "const m = new Map(); function f() { const r = compute(); return r; }",
            "const m = new Map(); class K { get value() { return m; } }",
            "items.map(x => x);",
            "const m = new Map(); items.map(x => x.id);",
            "const m = new Map(); wrap(() => { if (m) return a; return b; });",
            "const m = new Map(); wrap(async () => m);",
            "const m = new Map(); wrap(function* () { return m; });",
        ] {
            assert!(
                !opens_callback_gate(source),
                "gate must stay closed: {source}"
            );
        }
    }
}
