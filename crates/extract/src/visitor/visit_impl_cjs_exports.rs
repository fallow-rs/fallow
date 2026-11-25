//! CommonJS export assignments and the proofs of the static export shape.

use super::*;

impl<'a> ModuleInfoExtractor {
    /// Record a CommonJS named export (`module.exports.X = ...` /
    /// `exports.X = ...` / a `module.exports = {...}` key) and flag the module
    /// as carrying CJS exports.
    pub(super) fn push_cjs_named_export(&mut self, name: String, span: Span) {
        self.has_cjs_exports = true;
        self.exports.push(ExportInfo {
            name: ExportName::Named(name),
            local_name: None,
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span,
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        });
    }

    pub(super) fn record_cjs_assignment(&mut self, is_single_static_object_map_candidate: bool) {
        self.cjs_assignment_count = self.cjs_assignment_count.saturating_add(1);
        self.cjs_single_static_object_map = self.cjs_assignment_count == 1
            && is_single_static_object_map_candidate
            && !self.has_cjs_es_module_marker;
    }

    pub(super) fn is_cjs_exports_expression(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::Identifier(identifier) => identifier.name == "exports",
            Expression::StaticMemberExpression(member) => {
                member.property.name == "exports"
                    && matches!(
                        &member.object,
                        Expression::Identifier(identifier) if identifier.name == "module"
                    )
            }
            Expression::ComputedMemberExpression(member) => {
                matches!(
                    (&member.object, &member.expression),
                    (
                        Expression::Identifier(identifier),
                        Expression::StringLiteral(property)
                    ) if identifier.name == "module" && property.value == "exports"
                )
            }
            Expression::TSAsExpression(expression) => {
                Self::is_cjs_exports_expression(&expression.expression)
            }
            Expression::TSSatisfiesExpression(expression) => {
                Self::is_cjs_exports_expression(&expression.expression)
            }
            Expression::TSNonNullExpression(expression) => {
                Self::is_cjs_exports_expression(&expression.expression)
            }
            Expression::TSTypeAssertion(expression) => {
                Self::is_cjs_exports_expression(&expression.expression)
            }
            Expression::ParenthesizedExpression(expression) => {
                Self::is_cjs_exports_expression(&expression.expression)
            }
            _ => false,
        }
    }

    pub(super) fn is_computed_cjs_assignment_target(member: &ComputedMemberExpression<'_>) -> bool {
        matches!(&member.object, Expression::Identifier(identifier) if identifier.name == "exports")
            || matches!(
                (&member.object, &member.expression),
                (
                    Expression::Identifier(identifier),
                    Expression::StringLiteral(property)
                ) if identifier.name == "module" && property.value == "exports"
            )
            || Self::is_cjs_exports_expression(&member.object)
    }

    pub(super) fn is_static_cjs_assignment_target(member: &StaticMemberExpression<'_>) -> bool {
        (member.property.name == "exports"
            && matches!(
                &member.object,
                Expression::Identifier(identifier) if identifier.name == "module"
            ))
            || Self::is_cjs_exports_expression(&member.object)
    }

    pub(super) fn is_cjs_mutation_expression(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::StaticMemberExpression(member) => {
                Self::is_static_cjs_assignment_target(member)
            }
            Expression::ComputedMemberExpression(member) => {
                Self::is_computed_cjs_assignment_target(member)
            }
            Expression::TSAsExpression(expression) => {
                Self::is_cjs_mutation_expression(&expression.expression)
            }
            Expression::TSSatisfiesExpression(expression) => {
                Self::is_cjs_mutation_expression(&expression.expression)
            }
            Expression::TSNonNullExpression(expression) => {
                Self::is_cjs_mutation_expression(&expression.expression)
            }
            Expression::TSTypeAssertion(expression) => {
                Self::is_cjs_mutation_expression(&expression.expression)
            }
            Expression::ParenthesizedExpression(expression) => {
                Self::is_cjs_mutation_expression(&expression.expression)
            }
            _ => Self::is_cjs_exports_expression(expression),
        }
    }

    pub(super) fn assignment_target_mutates_cjs_exports(target: &AssignmentTarget<'_>) -> bool {
        if target
            .get_expression()
            .is_some_and(Self::is_cjs_mutation_expression)
        {
            return true;
        }
        match target {
            AssignmentTarget::StaticMemberExpression(member) => {
                Self::is_static_cjs_assignment_target(member)
            }
            AssignmentTarget::ComputedMemberExpression(member) => {
                Self::is_computed_cjs_assignment_target(member)
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                array.elements.iter().flatten().any(|element| {
                    Self::assignment_target_maybe_default_mutates_cjs_exports(element)
                }) || array
                    .rest
                    .as_ref()
                    .is_some_and(|rest| Self::assignment_target_mutates_cjs_exports(&rest.target))
            }
            AssignmentTarget::ObjectAssignmentTarget(object) => {
                object.properties.iter().any(|property| match property {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(_) => false,
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                        Self::assignment_target_maybe_default_mutates_cjs_exports(&property.binding)
                    }
                }) || object
                    .rest
                    .as_ref()
                    .is_some_and(|rest| Self::assignment_target_mutates_cjs_exports(&rest.target))
            }
            _ => false,
        }
    }

    pub(super) fn assignment_target_maybe_default_mutates_cjs_exports(
        target: &AssignmentTargetMaybeDefault<'_>,
    ) -> bool {
        if let AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) = target {
            return Self::assignment_target_mutates_cjs_exports(&default.binding);
        }
        target
            .as_assignment_target()
            .is_some_and(Self::assignment_target_mutates_cjs_exports)
    }

    pub(super) fn is_cjs_es_module_marker_call(call: &CallExpression<'_>) -> bool {
        let Expression::StaticMemberExpression(callee) = &call.callee else {
            return false;
        };
        if callee.property.name != "defineProperty"
            || !matches!(
                &callee.object,
                Expression::Identifier(identifier) if identifier.name == "Object"
            )
        {
            return false;
        }
        let Some(target) = call.arguments.first().and_then(Argument::as_expression) else {
            return false;
        };
        let Some(Expression::StringLiteral(property)) =
            call.arguments.get(1).and_then(Argument::as_expression)
        else {
            return false;
        };
        Self::is_cjs_exports_expression(target) && property.value == "__esModule"
    }

    pub(super) fn is_static_cjs_object_map(obj: &ObjectExpression<'_>) -> bool {
        let mut names = FxHashSet::default();
        for property in &obj.properties {
            let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) = property else {
                return false;
            };
            if property.computed {
                return false;
            }
            let Some(name) = property.key.static_name() else {
                return false;
            };
            if !names.insert(name.clone()) {
                return false;
            }
        }
        true
    }

    /// Handle CommonJS export assignments: `module.exports = { a, b }` (each key
    /// becomes a named export), `exports.X = ...`, and `module.exports.X = ...`.
    pub(super) fn handle_cjs_member_export(
        &mut self,
        member: &StaticMemberExpression<'a>,
        expr: &AssignmentExpression<'a>,
    ) {
        if let Expression::Identifier(obj) = &member.object {
            if obj.name == "module" && member.property.name == "exports" {
                self.has_cjs_exports = true;
                let object_map = if let Expression::ObjectExpression(obj_expr) = &expr.right {
                    Some(obj_expr)
                } else {
                    None
                };
                let is_top_level =
                    self.block_depth == 0 && self.function_depth == 0 && self.namespace_depth == 0;
                self.record_cjs_assignment(
                    matches!(expr.operator, AssignmentOperator::Assign)
                        && is_top_level
                        && object_map
                            .is_some_and(|object_map| Self::is_static_cjs_object_map(object_map)),
                );
                if let Some(object_map) = object_map {
                    for property in &object_map.properties {
                        if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) = property
                            && let Some(name) = property.key.static_name()
                        {
                            self.push_cjs_named_export(name.text().into_owned(), property.span);
                        }
                    }
                }
            }
            if obj.name == "exports" {
                self.record_cjs_assignment(false);
                self.push_cjs_named_export(member.property.name.to_string(), expr.span);
            }
        } else if Self::is_cjs_exports_expression(&member.object) {
            self.record_cjs_assignment(false);
            self.push_cjs_named_export(member.property.name.to_string(), expr.span);
        }
    }

    pub(super) fn invalidate_assignment_target_proof(&mut self, target: &AssignmentTarget<'_>) {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(id) => {
                self.invalidate_direct_binding_path(id.name.as_str(), false);
            }
            AssignmentTarget::StaticMemberExpression(member) => {
                if let Some(path) = static_member_object_name(&member.object) {
                    self.invalidate_direct_binding_path(
                        &format!("{path}.{}", member.property.name),
                        false,
                    );
                }
            }
            AssignmentTarget::ComputedMemberExpression(member) => {
                if let Some(path) = static_member_object_name(&member.object) {
                    if let Some(key) = member.static_property_name().map(JsText::text) {
                        self.invalidate_direct_binding_path(&format!("{path}.{key}"), false);
                    } else {
                        self.invalidate_direct_binding_path(&path, true);
                    }
                }
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                for element in array.elements.iter().flatten() {
                    self.invalidate_assignment_target_default_proof(element);
                }
                if let Some(rest) = &array.rest {
                    self.invalidate_assignment_target_proof(&rest.target);
                }
            }
            AssignmentTarget::ObjectAssignmentTarget(object) => {
                for property in &object.properties {
                    match property {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(property) => {
                            self.invalidate_direct_binding_path(
                                property.binding.name.as_str(),
                                false,
                            );
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                            self.invalidate_assignment_target_default_proof(&property.binding);
                        }
                    }
                }
                if let Some(rest) = &object.rest {
                    self.invalidate_assignment_target_proof(&rest.target);
                }
            }
            _ => {
                if let Some(name) = assignment_target_identifier_name(target) {
                    self.invalidate_direct_binding_path(name, false);
                }
            }
        }
    }

    pub(super) fn invalidate_assignment_target_default_proof(
        &mut self,
        target: &AssignmentTargetMaybeDefault<'_>,
    ) {
        if let AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) = target {
            self.invalidate_assignment_target_proof(&default.binding);
        } else if let Some(target) = target.as_assignment_target() {
            self.invalidate_assignment_target_proof(target);
        }
    }

    /// Record sanitizer / allowlist / regex / path bindings cleared by an
    /// assignment to an identifier or member-object target.
    pub(super) fn record_assignment_target_bindings(&mut self, left: &AssignmentTarget<'a>) {
        if let Some(name) = assignment_target_identifier_name(left) {
            self.record_sanitizer_binding(name, None);
            self.record_literal_allowlist_binding(name, false);
            self.record_risky_regex_binding(name, None);
            self.record_path_sink_binding(name, None);
            self.record_path_relative_binding(name, None);
        } else if let Some(name) = assignment_target_member_object_name(left)
            && self.literal_allowlist_binding(name)
        {
            self.record_literal_allowlist_binding(name, false);
        }
    }
}
