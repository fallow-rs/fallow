//! Scope-owned homogeneous array contracts and their member projections.

use oxc_ast::ast::{
    Argument, ArrowFunctionBody, AssignmentTarget, BindingPattern, CallExpression,
    ComputedMemberExpression, Expression, ObjectPropertyKind, ReturnStatement, Statement,
    TSSignature, TSType, VariableDeclarator,
};
use oxc_span::Span;

use super::super::{
    BindingClassProof, DirectBindingValue, DirectObjectBindingBranch, ImportedName, MemberAccess,
    ModuleInfoExtractor, ProjectedMemberAccess, SemanticFact, TypedPropertyExpansion,
};
use super::unwrap_static_expr;

pub(in crate::visitor) const MAX_PROJECTION_VALUES: usize = 8192;
const MAX_PROJECTED_ACCESSES: usize = 8192;

type ProjectionValue = (usize, super::super::BindingRequirements);

fn projection_property_path(parent: &str, key: &str) -> Option<String> {
    // Dotted binding paths cannot represent a separator inside one property key.
    (!key.is_empty() && !key.contains('.')).then(|| format!("{parent}.{key}"))
}

fn homogeneous_element<'a, 'b>(ty: &'b TSType<'a>) -> Option<(&'b TSType<'a>, Option<Span>)> {
    match ty {
        TSType::TSArrayType(array) => Some((&array.element_type, None)),
        TSType::TSTypeReference(reference)
            if super::super::helpers::extract_type_reference_name(ty)
                .is_some_and(|name| name == "Array" || name == "ReadonlyArray") =>
        {
            Some((
                reference.type_arguments.as_deref()?.params.first()?,
                super::super::helpers::extract_type_reference_origin(ty),
            ))
        }
        TSType::TSTypeOperatorType(operator)
            if operator.operator == oxc_ast::ast::TSTypeOperatorOperator::Readonly =>
        {
            homogeneous_element(&operator.type_annotation)
        }
        TSType::TSParenthesizedType(parenthesized) => {
            homogeneous_element(&parenthesized.type_annotation)
        }
        TSType::TSUnionType(union) => {
            let mut types = union.types.iter().filter(|ty| {
                !matches!(ty, TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_))
            });
            let element = homogeneous_element(types.next()?)?;
            types.next().is_none().then_some(element)
        }
        _ => None,
    }
}

impl ModuleInfoExtractor {
    pub(super) fn allocate_projection_value(
        &mut self,
        mut value: DirectBindingValue,
    ) -> Option<usize> {
        let conditions = value
            .proof
            .iter()
            .chain(&value.iteration_proof)
            .map(|proof| proof.builtin_containers.len())
            .sum();
        if self.projection_exhausted.get()
            || self.direct_binding_values.len() >= MAX_PROJECTION_VALUES
            || !self.charge_binding_conditions(conditions)
        {
            return None;
        }
        value.constructor_owner = self.active_constructor_function;
        let id = self.direct_binding_values.len();
        self.direct_binding_values.push(value);
        Some(id)
    }

    pub(super) fn record_projection_contract(&mut self, name: &str, span: Span, ty: &TSType<'_>) {
        if let TSType::TSParenthesizedType(parenthesized) = ty {
            self.record_projection_contract(name, span, &parenthesized.type_annotation);
            return;
        }
        if let TSType::TSTypeLiteral(literal) = ty {
            for member in &literal.members {
                let TSSignature::TSPropertySignature(property) = member else {
                    continue;
                };
                if let Some(key) = property.key.static_name()
                    && let Some(annotation) = &property.type_annotation
                    && let Some(path) = projection_property_path(name, &key)
                {
                    self.record_projection_contract(&path, span, &annotation.type_annotation);
                }
            }
            return;
        }
        let value = if matches!(ty, TSType::TSNumberKeyword(_)) {
            DirectBindingValue {
                numeric_contract: true,
                ..DirectBindingValue::default()
            }
        } else if let Some((element, builtin_container)) = homogeneous_element(ty) {
            self.constrain_iteration_container(name, builtin_container);
            let Some(element) = self.projection_type_value(element, builtin_container) else {
                return;
            };
            DirectBindingValue {
                array_element: Some(element),
                iteration_proof: self
                    .direct_binding_value(name)
                    .and_then(|(id, _)| self.projection_value_view(id).iteration_proof.clone()),
                ..DirectBindingValue::default()
            }
        } else {
            return;
        };
        let Some(value) = self.allocate_projection_value(value) else {
            return;
        };
        if !self.prepare_projection_path(name, span) {
            return;
        }
        self.attach_direct_binding_value(name, value);
        let root = name.split('.').next().unwrap_or(name);
        self.direct_binding_entry_mut(root).declaration_span = Some(span);
    }

    fn constrain_iteration_container(&mut self, name: &str, container: Option<Span>) {
        let Some(container) = container else {
            return;
        };
        let Some((id, _)) = self.direct_binding_value(name) else {
            return;
        };
        let id = self.projection_view_id(id);
        let Some(proof) = &self.direct_binding_values[id].iteration_proof else {
            return;
        };
        if proof.builtin_containers.contains(&container) {
            return;
        }
        let retained = proof.builtin_containers.len() + 1;
        if !self.charge_binding_conditions(retained) {
            self.direct_binding_values[id].iteration_proof = None;
            return;
        }
        if let Some(proof) = &mut self.direct_binding_values[id].iteration_proof {
            proof.builtin_containers = proof
                .builtin_containers
                .iter()
                .copied()
                .chain([container])
                .collect();
        }
    }

    fn projection_type_value(
        &mut self,
        ty: &TSType<'_>,
        builtin_container: Option<Span>,
    ) -> Option<usize> {
        if let Some(name) = super::super::helpers::extract_type_reference_name(ty) {
            let origin = super::super::helpers::extract_type_reference_origin(ty)?;
            let target = self.resolve_class_type_param(&name)?;
            if !self.charge_binding_conditions(usize::from(builtin_container.is_some())) {
                return None;
            }
            return self.allocate_projection_value(DirectBindingValue {
                proof: Some(BindingClassProof {
                    builtin_containers: builtin_container.into_iter().collect(),
                    target_name: target,
                    target_reference_span: self
                        .resolve_class_type_param_origin(&name, Some(origin))?,
                    requirements: super::super::BindingRequirements::Empty,
                }),
                projection_contract: true,
                ..DirectBindingValue::default()
            });
        }
        let mut properties = Vec::new();
        super::super::helpers::collect_nested_type_bindings(ty, None, &mut properties);
        if properties.is_empty()
            || properties.len()
                > MAX_PROJECTION_VALUES.saturating_sub(self.direct_binding_values.len())
        {
            return None;
        }
        let root = self.allocate_projection_value(DirectBindingValue {
            projection_contract: true,
            ..DirectBindingValue::default()
        })?;
        for (path, name, origin) in properties {
            let mut parent = root;
            let mut parts = path.split('.').peekable();
            while let Some(property) = parts.next() {
                let child = if let Some(child) =
                    self.direct_binding_values[parent].properties.get(property)
                {
                    *child
                } else {
                    let child = self.allocate_projection_value(DirectBindingValue {
                        projection_contract: true,
                        ..DirectBindingValue::default()
                    })?;
                    self.direct_binding_values[parent]
                        .properties
                        .insert(property.to_string(), child);
                    child
                };
                if parts.peek().is_none() {
                    if !self.charge_binding_conditions(usize::from(builtin_container.is_some())) {
                        return None;
                    }
                    let value = &mut self.direct_binding_values[child];
                    value.proof = origin.map(|origin| BindingClassProof {
                        builtin_containers: builtin_container.into_iter().collect(),
                        target_name: name.clone(),
                        target_reference_span: origin,
                        requirements: super::super::BindingRequirements::Empty,
                    });
                    value.projection_contract = true;
                }
                parent = child;
            }
        }
        Some(root)
    }

    pub(super) fn record_projection_declarator(&mut self, declarator: &VariableDeclarator<'_>) {
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return;
        };
        if let Some(init) = &declarator.init
            && let Some((value, requirements)) = self.projected_value(init)
        {
            self.attach_direct_binding_value(binding.name.as_str(), value);
            let entry = self.direct_binding_entry_mut(binding.name.as_str());
            entry.declaration_span = Some(binding.span);
            entry.requirements = requirements;
        } else if let Some(annotation) = declarator.type_annotation.as_deref() {
            self.record_projection_contract(
                binding.name.as_str(),
                binding.span,
                &annotation.type_annotation,
            );
        } else if let Some(init) = &declarator.init {
            self.record_projection_source(binding.name.as_str(), binding.span, init);
        }
    }

    fn prepare_projection_path(&mut self, name: &str, span: Span) -> bool {
        let Some((parent, _)) = name.rsplit_once('.') else {
            return true;
        };
        self.prepare_projection_path(parent, span)
            && self.prepare_projection_container(parent, span)
    }

    fn prepare_projection_container(&mut self, name: &str, span: Span) -> bool {
        if !self.charge_projection_work(1) {
            return false;
        }
        if let Some((id, _)) = self.direct_binding_value(name) {
            let id = self.projection_view_id(id);
            if self.is_projection_barrier(id) {
                return false;
            }
            self.direct_binding_values[id].projection_contract = true;
        } else {
            let Some(id) = self.allocate_projection_value(DirectBindingValue {
                projection_contract: true,
                ..DirectBindingValue::default()
            }) else {
                return false;
            };
            self.attach_direct_binding_value(name, id);
        }
        let root = name.split('.').next().unwrap_or(name);
        self.direct_binding_entry_mut(root).declaration_span = Some(span);
        true
    }

    fn record_projection_source(&mut self, name: &str, span: Span, source: &Expression<'_>) {
        let source = unwrap_static_expr(source);
        match source {
            Expression::Identifier(_) | Expression::StaticMemberExpression(_) => {
                if let Some((value, requirements)) = self.projected_value(source)
                    && self.charge_projection_work(self.requirement_len(requirements))
                    && self.prepare_projection_path(name, span)
                {
                    self.attach_direct_binding_value(name, value);
                    let root = name.split('.').next().unwrap_or(name);
                    let combined = self.concat_requirements(
                        self.direct_binding_entry(root)
                            .map_or(super::super::BindingRequirements::Empty, |entry| {
                                entry.requirements
                            }),
                        requirements,
                    );
                    self.direct_binding_entry_mut(root).requirements = combined;
                }
            }
            Expression::AwaitExpression(awaited) => {
                self.record_projection_source(name, span, &awaited.argument);
            }
            Expression::ObjectExpression(object) => {
                if object.properties.iter().any(|property| !matches!(property,
                    ObjectPropertyKind::ObjectProperty(property) if !property.computed && property.key.static_name().is_some()))
                { return; }
                let mut seen = rustc_hash::FxHashSet::default();
                for property in object.properties.iter().rev() {
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        continue;
                    };
                    let Some(key) = property.key.static_name() else {
                        continue;
                    };
                    if let Some(path) = projection_property_path(name, &key)
                        && seen.insert(key.to_string())
                    {
                        self.record_projection_source(&path, span, &property.value);
                    }
                }
            }
            Expression::ArrayExpression(_) => {
                if let Some((target, Some(origin))) =
                    super::super::helpers::infer_array_binding_element_target(None, Some(source))
                {
                    self.attach_projection_array(
                        name,
                        span,
                        (target, origin),
                        None,
                        super::super::BindingRequirements::Empty,
                    );
                }
            }
            Expression::CallExpression(call) => self.record_vue_projection_array(name, span, call),
            _ => {}
        }
    }

    fn attach_projection_array(
        &mut self,
        name: &str,
        span: Span,
        target: (String, Span),
        container: Option<Span>,
        requirements: super::super::BindingRequirements,
    ) {
        if !self.charge_binding_conditions(usize::from(container.is_some())) {
            return;
        }
        let proof = BindingClassProof {
            builtin_containers: container.into_iter().collect(),
            target_name: target.0,
            target_reference_span: target.1,
            requirements,
        };
        let Some(element) = self.allocate_projection_value(DirectBindingValue {
            proof: Some(proof.clone()),
            projection_contract: true,
            ..DirectBindingValue::default()
        }) else {
            return;
        };
        let Some(value) = self.allocate_projection_value(DirectBindingValue {
            array_element: Some(element),
            iteration_proof: self.capture_subclass_provenance.then_some(proof),
            ..DirectBindingValue::default()
        }) else {
            return;
        };
        if !self.prepare_projection_path(name, span) {
            return;
        }
        self.attach_direct_binding_value(name, value);
        let root = name.split('.').next().unwrap_or(name);
        self.direct_binding_entry_mut(root).declaration_span = Some(span);
    }

    fn vue_projection_callee(&self, call: &CallExpression<'_>) -> Option<(String, Span, Span)> {
        let (local, reference, member) = match unwrap_static_expr(&call.callee) {
            Expression::Identifier(identifier) => (identifier.name.as_str(), identifier.span, None),
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(identifier) = unwrap_static_expr(&member.object) else {
                    return None;
                };
                (
                    identifier.name.as_str(),
                    identifier.span,
                    Some(member.property.name.as_str()),
                )
            }
            _ => return None,
        };
        let import = self.imports.iter().find(|import| {
            import.local_name == local
                && !import.is_type_only
                && matches!(import.source.as_str(), "vue" | "@vue/reactivity")
        })?;
        let helper = match (&import.imported_name, member) {
            (ImportedName::Named(name), None) => name.as_str(),
            (ImportedName::Namespace, Some(name)) => name,
            _ => return None,
        };
        if !matches!(helper, "ref" | "shallowRef" | "computed") {
            return None;
        }
        let declaration = self
            .module_direct_object_binding_targets
            .get(local)?
            .declaration_span?;
        Some((helper.to_string(), reference, declaration))
    }

    fn defer_vue_local_array(
        &mut self,
        name: &str,
        span: Span,
        argument: &Argument<'_>,
        requirement: (Span, Span),
    ) -> bool {
        let body = match argument {
            Argument::ArrowFunctionExpression(arrow) if !arrow.r#async => {
                let ArrowFunctionBody::FunctionBody(body) = &arrow.body else {
                    return false;
                };
                body
            }
            Argument::FunctionExpression(function) if !function.r#async && !function.generator => {
                let Some(body) = function.body.as_deref() else {
                    return false;
                };
                body
            }
            _ => return false,
        };
        let Some(Statement::ReturnStatement(returned)) = body.statements.last() else {
            return false;
        };
        let Some(Expression::Identifier(reference)) =
            returned.argument.as_ref().map(unwrap_static_expr)
        else {
            return false;
        };
        if !self.charge_projection_work(body.statements.len()) {
            return true;
        }
        if super::visit_factory_returns::count_returns_in_statements(&body.statements) != 1 {
            return true;
        }
        let declaration = body.statements.iter().find_map(|statement| {
            let Statement::VariableDeclaration(declaration) = statement else {
                return None;
            };
            declaration.declarations.iter().find_map(|variable| {
                let BindingPattern::BindingIdentifier(binding) = &variable.id else {
                    return None;
                };
                (binding.name == reference.name
                    && variable.type_annotation.as_ref().is_some_and(|annotation| {
                        super::super::helpers::array_element_target_from_type(
                            &annotation.type_annotation,
                        )
                        .is_some()
                    }))
                .then_some(binding.span)
            })
        });
        let Some(declaration) = declaration else {
            return true;
        };
        if !self.prepare_projection_container(name, span) {
            return true;
        }
        let Some(destination) = self.allocate_projection_value(DirectBindingValue {
            requirements: self
                .append_requirement(super::super::BindingRequirements::Empty, requirement),
            ..DirectBindingValue::default()
        }) else {
            return true;
        };
        self.attach_direct_binding_value(&format!("{name}.value"), destination);
        self.projection_array_returns
            .insert(returned.span, (destination, declaration));
        true
    }

    pub(super) fn complete_projection_array_return(&mut self, returned: &ReturnStatement<'_>) {
        let Some((destination, declaration)) = self.projection_array_returns.remove(&returned.span)
        else {
            return;
        };
        let Some(argument) = returned.argument.as_ref() else {
            return;
        };
        let Expression::Identifier(reference) = unwrap_static_expr(argument) else {
            return;
        };
        if self
            .direct_binding_entry(reference.name.as_str())
            .and_then(|entry| entry.declaration_span)
            != Some(declaration)
        {
            return;
        }
        let Some((value, requirements)) = self.projected_value(argument) else {
            return;
        };
        if !self.charge_projection_work(self.requirement_len(requirements)) {
            return;
        }
        let source = self.projection_value_view(value);
        let element = source.array_element;
        let iteration = source.iteration_proof.clone();
        let combined_requirements = self.concat_requirements(
            self.direct_binding_values[destination].requirements,
            requirements,
        );
        let target = &mut self.direct_binding_values[destination];
        target.array_element = element;
        target.iteration_proof = iteration;
        target.requirements = combined_requirements;
    }

    fn admits_vue_array_callback(&mut self, argument: &Argument<'_>) -> bool {
        let body = match argument {
            Argument::ArrowFunctionExpression(arrow) if !arrow.r#async => {
                let ArrowFunctionBody::FunctionBody(body) = &arrow.body else {
                    return true;
                };
                body
            }
            Argument::FunctionExpression(function) if !function.r#async && !function.generator => {
                let Some(body) = function.body.as_deref() else {
                    return false;
                };
                body
            }
            _ => return false,
        };
        self.charge_projection_work(body.statements.len())
            && matches!(body.statements.last(), Some(Statement::ReturnStatement(_)))
            && super::visit_factory_returns::count_returns_in_statements(&body.statements) == 1
    }

    fn record_vue_projection_array(&mut self, name: &str, span: Span, call: &CallExpression<'_>) {
        let Some((helper, reference, declaration)) = self.vue_projection_callee(call) else {
            return;
        };
        if helper == "computed"
            && !call
                .arguments
                .first()
                .is_some_and(|argument| self.admits_vue_array_callback(argument))
        {
            return;
        }
        let annotation = call
            .type_arguments
            .as_deref()
            .and_then(|arguments| arguments.params.first());
        if helper == "computed"
            && annotation.is_none()
            && call.arguments.first().is_some_and(|argument| {
                self.defer_vue_local_array(name, span, argument, (reference, declaration))
            })
        {
            return;
        }
        let target = if let Some(annotation) = annotation {
            super::super::helpers::array_element_target_from_type(annotation)
        } else if helper == "computed" {
            call.arguments
                .first()
                .and_then(super::super::helpers::callback_returned_array_element)
        } else {
            call.arguments
                .first()
                .and_then(|arg| arg.as_expression())
                .and_then(|source| {
                    super::super::helpers::infer_array_binding_element_target(None, Some(source))
                })
        };
        let Some((target, Some(origin))) = target else {
            return;
        };
        let container = annotation
            .and_then(homogeneous_element)
            .and_then(|(_, condition)| condition);
        if self.prepare_projection_container(name, span) {
            self.attach_projection_array(
                &format!("{name}.value"),
                span,
                (target, origin),
                container,
                self.append_requirement(
                    super::super::BindingRequirements::Empty,
                    (reference, declaration),
                ),
            );
        }
    }

    fn projected_value(&mut self, expression: &Expression<'_>) -> Option<ProjectionValue> {
        self.resolve_projected_value(expression, true)
    }

    /// Resolve the projection value of `expression`. With `track` false, the
    /// walk adds no requirement nodes and gives `BindingRequirements::Empty`.
    /// A caller can then refuse a value without a proof before it uses the
    /// requirement budget that the subclass proofs share.
    fn resolve_projected_value(
        &mut self,
        expression: &Expression<'_>,
        track: bool,
    ) -> Option<ProjectionValue> {
        match unwrap_static_expr(expression) {
            Expression::Identifier(identifier) => {
                let (value, entry) = self.direct_binding_value(identifier.name.as_str())?;
                let state = self.projection_value_view(value);
                if !state.projection_contract
                    && state.array_element.is_none()
                    && !state.numeric_contract
                {
                    return None;
                }
                let declaration = entry.declaration_span?;
                if !track {
                    return Some((value, super::super::BindingRequirements::Empty));
                }
                let requirements = self.combine_requirements(
                    &[entry.requirements, state.requirements],
                    &[(identifier.span, declaration)],
                );
                Some((value, requirements))
            }
            Expression::StaticMemberExpression(member) => {
                let (parent, requirements) = self.resolve_projected_value(&member.object, track)?;
                let value = self.projected_property(parent, member.property.name.as_str())?;
                if !track {
                    return Some((value, requirements));
                }
                let requirements = self.concat_requirements(
                    requirements,
                    self.projection_value_view(value).requirements,
                );
                Some((value, requirements))
            }
            Expression::ComputedMemberExpression(member) => {
                let (parent, mut requirements) =
                    self.resolve_projected_value(&member.object, track)?;
                let element = self.projection_value_view(parent).array_element?;
                match unwrap_static_expr(&member.expression) {
                    Expression::NumericLiteral(number)
                        if number.value >= 0.0 && number.value.fract() == 0.0 => {}
                    Expression::Identifier(_) => {
                        let (index, index_requirements) =
                            self.resolve_projected_value(&member.expression, track)?;
                        if !self.projection_value_view(index).numeric_contract {
                            return None;
                        }
                        if track {
                            requirements =
                                self.concat_requirements(requirements, index_requirements);
                        }
                    }
                    _ => return None,
                }
                Some((element, requirements))
            }
            _ => None,
        }
    }

    fn projected_property(&mut self, parent: usize, property: &str) -> Option<usize> {
        let view = self.projection_view_id(parent);
        let value = &self.direct_binding_values[view];
        if let Some(child) = value.properties.get(property) {
            return Some(*child);
        }
        if !value.projection_contract || value.unknown_properties {
            return None;
        }
        let proof = value.proof.clone()?;
        let owner = value.constructor_owner;
        let path = if value.projection_path.is_empty() {
            property.to_string()
        } else {
            format!("{}.{property}", value.projection_path)
        };
        let child = self.allocate_projection_value(DirectBindingValue {
            proof: Some(proof),
            projection_contract: true,
            projection_path: path,
            ..DirectBindingValue::default()
        })?;
        self.direct_binding_values[child].constructor_owner = owner;
        self.direct_binding_values[view]
            .properties
            .insert(property.to_string(), child);
        Some(child)
    }

    pub(super) fn record_projected_member_access(
        &mut self,
        receiver: &Expression<'_>,
        member: &str,
    ) {
        if self.projected_member_accesses.len() >= MAX_PROJECTED_ACCESSES {
            return;
        }
        // Check for a proven contract before the walk adds requirement nodes.
        // Reads such as `n.toFixed()` on a numeric binding must not use the
        // requirement budget that the subclass proofs share.
        let Some((target, _)) = self.resolve_projected_value(receiver, false) else {
            return;
        };
        let target = self.projection_value_view(target);
        if !target.projection_contract || target.proof.is_none() {
            return;
        }
        let Some((id, requirements)) = self.projected_value(receiver) else {
            return;
        };
        let value = self.projection_value_view(id);
        if !value.projection_contract {
            return;
        }
        let Some(mut proof) = value.proof.clone() else {
            return;
        };
        proof.requirements = self.concat_requirements(proof.requirements, requirements);
        self.projected_member_accesses.push(ProjectedMemberAccess {
            proof,
            property_path: value.projection_path.clone(),
            member: member.to_string(),
        });
    }

    pub(super) fn projection_view_id(&self, id: usize) -> usize {
        self.direct_object_binding_branch_scopes
            .iter()
            .rev()
            .find_map(|branch| branch.projection_views.get(&id).copied())
            .unwrap_or(id)
    }

    pub(super) fn projection_value_view(&self, id: usize) -> &DirectBindingValue {
        &self.direct_binding_values[self.projection_view_id(id)]
    }

    pub(super) fn charge_binding_conditions(&mut self, amount: usize) -> bool {
        amount == 0 || self.charge_projection_work(amount)
    }

    fn charge_projection_work(&mut self, amount: usize) -> bool {
        if self.projection_exhausted.get()
            || amount > MAX_PROJECTION_VALUES.saturating_sub(self.projection_retained_work)
        {
            self.projection_exhausted.set(true);
            return false;
        }
        self.projection_retained_work += amount;
        true
    }

    pub(super) fn writable_projection_view(&mut self, id: usize) -> Option<usize> {
        let Some(branch) = self.direct_object_binding_branch_scopes.last() else {
            return Some(id);
        };
        if let Some(view) = branch.projection_views.get(&id) {
            return Some(*view);
        }
        let view = self.projection_view_id(id);
        let retained = self.direct_binding_values[view]
            .properties
            .len()
            .saturating_add(self.requirement_len(self.direct_binding_values[view].requirements))
            .saturating_add(1);
        if !self.charge_projection_work(retained) {
            return None;
        }
        if self.direct_binding_values.len() >= MAX_PROJECTION_VALUES {
            self.projection_exhausted.set(true);
            return None;
        }
        let copied = self.direct_binding_values[view].clone();
        let owner = copied.constructor_owner;
        let copy = self.allocate_projection_value(copied)?;
        self.direct_binding_values[copy].constructor_owner = owner;
        self.direct_object_binding_branch_scopes
            .last_mut()?
            .projection_views
            .insert(id, copy);
        Some(copy)
    }

    pub(super) fn invalidate_projection_slot(&mut self, parent: usize, property: Option<&str>) {
        if self.is_projection_barrier(self.projection_view_id(parent)) {
            return;
        }
        let Some(view) = self.writable_projection_view(parent) else {
            self.refuse_projection_mutation(parent);
            return;
        };
        if !self.charge_projection_work(1) {
            self.refuse_projection_mutation(parent);
            return;
        }
        if let Some(branch) = self.direct_object_binding_branch_scopes.last_mut() {
            branch
                .projection_writes
                .entry(parent)
                .or_default()
                .insert(property.map(str::to_string));
        }
        if let Some(property) = property
            && let Some(unknown) = self.allocate_projection_value(DirectBindingValue::unknown())
        {
            self.direct_binding_values[view]
                .properties
                .insert(property.to_string(), unknown);
        } else {
            let value = &mut self.direct_binding_values[view];
            value.array_element = None;
            value.iteration_proof = None;
            value.properties.clear();
            value.unknown_properties = true;
            value.projection_contract = false;
        }
    }

    pub(super) fn is_projection_barrier(&self, id: usize) -> bool {
        self.projection_unknown_value == Some(id)
    }

    pub(super) fn refuse_projection_mutation(&mut self, parent: usize) {
        if self.direct_object_binding_branch_scopes.is_empty() {
            let value = &mut self.direct_binding_values[parent];
            value.array_element = None;
            value.iteration_proof = None;
            value.properties.clear();
            value.unknown_properties = true;
            value.projection_contract = false;
            return;
        }
        // Refusal must survive the join without changing the alternate's input.
        // This one reserved value is immutable; shared writers must reject it.
        let unknown = *self.projection_unknown_value.get_or_insert_with(|| {
            let id = self.direct_binding_values.len();
            self.direct_binding_values
                .push(DirectBindingValue::unknown());
            id
        });
        if let Some(branch) = self.direct_object_binding_branch_scopes.last_mut() {
            branch.projection_views.insert(parent, unknown);
            let writes = branch.projection_writes.entry(parent).or_default();
            writes.clear();
            writes.insert(None);
        }
    }

    pub(super) fn join_projection_writes(
        &mut self,
        consequent: &DirectObjectBindingBranch,
        alternate: &DirectObjectBindingBranch,
    ) {
        for branch in [consequent, alternate] {
            for (value, properties) in &branch.projection_writes {
                for property in properties {
                    self.invalidate_projection_slot(*value, property.as_deref());
                }
            }
        }
    }

    pub(super) fn invalidate_projected_property(
        &mut self,
        object: &Expression<'_>,
        property: Option<&str>,
    ) -> bool {
        let Some((parent, _)) = self.projected_value(object) else {
            return false;
        };
        self.invalidate_projected_named_slot(parent, property)
    }

    pub(super) fn invalidate_projected_computed_property(
        &mut self,
        member: &ComputedMemberExpression<'_>,
    ) -> bool {
        let Some((parent, _)) = self.projected_value(&member.object) else {
            return false;
        };
        let key = member.static_property_name();
        // A named object slot and an index into the homogeneous array are distinct.
        let property = if self.projection_value_view(parent).array_element.is_some() {
            None
        } else {
            key.as_deref()
        };
        self.invalidate_projected_named_slot(parent, property)
    }

    fn invalidate_projected_named_slot(&mut self, parent: usize, property: Option<&str>) -> bool {
        if let Some(property) = property {
            if property.is_empty() || property.contains('.') {
                // Consume the literal slot without handing it to dotted-path bookkeeping.
                self.invalidate_projection_slot(parent, Some(property));
                return true;
            }
            let value = self.projection_value_view(parent);
            if value.array_element.is_none()
                && value.proof.is_none()
                && value.projection_path.is_empty()
                && !value.properties.get(property).is_some_and(|child| {
                    let child = self.projection_value_view(*child);
                    child.projection_contract
                        || child.array_element.is_some()
                        || child.numeric_contract
                })
            {
                // Ordinary siblings keep their possible-target assignment bookkeeping.
                return false;
            }
        }
        self.invalidate_projection_slot(parent, property);
        true
    }

    pub(super) fn invalidate_projected_assignment(
        &mut self,
        target: &AssignmentTarget<'_>,
    ) -> bool {
        match target {
            AssignmentTarget::StaticMemberExpression(member) => self
                .invalidate_projected_property(&member.object, Some(member.property.name.as_str())),
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.invalidate_projected_computed_property(member)
            }
            _ => false,
        }
    }

    fn admitted_projection_origin(&self, proof: &BindingClassProof) -> bool {
        if !self.admitted_binding_containers(proof)
            || !self.admitted_requirements(proof.requirements)
        {
            return false;
        }
        let Some(binding) = self
            .subclass_binding_references
            .get(&proof.target_reference_span)
        else {
            return false;
        };
        self.imports.iter().any(|import| {
            import.local_name == proof.target_name
                && import.span.start <= binding.start
                && import.span.end >= binding.end
        }) || self
            .local_class_exports
            .get(&proof.target_name)
            .is_some_and(|class| class.binding_span == *binding)
            || self
                .interface_property_types
                .get(&proof.target_name)
                .is_some_and(|properties| {
                    properties
                        .values()
                        .any(|property| property.owner_span == Some(*binding))
                })
    }

    pub(in crate::visitor) fn resolve_projected_member_accesses(&mut self) {
        for access in std::mem::take(&mut self.projected_member_accesses) {
            if access.property_path.is_empty()
                && self.emit_local_subclass_member_access(
                    &super::super::SubclassAccessProof::Known(access.proof.clone()),
                    &access.member,
                )
            {
                continue;
            }
            if !self.admitted_projection_origin(&access.proof) {
                continue;
            }
            if access.property_path.is_empty() {
                self.member_accesses.push(MemberAccess {
                    object: access.proof.target_name,
                    member: access.member,
                });
                continue;
            }
            match self.expand_typed_property_compound(
                &format!("{}.{}", access.proof.target_name, access.property_path),
                Some(access.proof.target_reference_span),
            ) {
                TypedPropertyExpansion::Resolved(name, Some(origin), requirements) => {
                    let proof = BindingClassProof {
                        builtin_containers: std::sync::Arc::clone(&access.proof.builtin_containers),
                        target_name: name,
                        target_reference_span: origin,
                        requirements: self.import_requirements(&requirements),
                    };
                    if self.emit_local_subclass_member_access(
                        &super::super::SubclassAccessProof::Known(proof.clone()),
                        &access.member,
                    ) {
                        continue;
                    }
                    if self.admitted_projection_origin(&proof) {
                        self.member_accesses.push(MemberAccess {
                            object: proof.target_name,
                            member: access.member,
                        });
                    }
                }
                TypedPropertyExpansion::CrossModule {
                    type_name,
                    property_path,
                    origin: Some(origin),
                    requirements,
                } => {
                    let proof = BindingClassProof {
                        builtin_containers: std::sync::Arc::clone(&access.proof.builtin_containers),
                        target_name: type_name.clone(),
                        target_reference_span: origin,
                        requirements: self.import_requirements(&requirements),
                    };
                    if !self.admitted_projection_origin(&proof) {
                        continue;
                    }
                    self.semantic_facts
                        .push(SemanticFact::TypedPropertyMemberAccess(
                            fallow_types::extract::TypedPropertyMemberAccessFact {
                                type_name,
                                property_path,
                                member: access.member,
                            },
                        ));
                }
                _ => {}
            }
        }
    }
}
