//! Class member credit for destructures that read an instance.
//!
//! `const { m } = instance`, `({ m } = instance)` and `({ m }: Service) => ...`
//! each read `m` from a class instance. The binding that holds the read is a
//! plain local, so no later `m` access resolves to the class. These helpers
//! record the access at the destructure site.

#[allow(clippy::wildcard_imports, reason = "many destructure AST types used")]
use oxc_ast::ast::*;

use super::super::{BindingTarget, ModuleInfoExtractor};
use super::unwrap_static_expr;
use crate::MemberAccess;
use crate::visitor::helpers::extract_type_reference_name;

impl<'a> ModuleInfoExtractor {
    pub(in crate::visitor) fn record_declarator_member_destructuring(
        &mut self,
        declarator: &VariableDeclarator<'a>,
        init: &Expression<'a>,
    ) {
        // `const { member } = instance` reads the class member directly. The
        // binding itself is a plain identifier, so no later `member` access
        // can be resolved through `binding_target_names`; record the access at
        // the destructuring site instead. This is the object-pattern parity
        // of the existing `instance.member` path (see #2560).
        if let BindingPattern::ObjectPattern(pattern) = &declarator.id
            && let Expression::Identifier(source) = unwrap_static_expr(init)
            && let Some(BindingTarget::Class(class_name)) =
                self.resolve_bound_object_name(source.name.as_str())
        {
            let keys: Vec<_> = pattern
                .properties
                .iter()
                .map(|property| property.key.static_name())
                .collect();
            self.credit_destructured_class_members(
                &class_name,
                Some(source),
                &keys,
                pattern.rest.is_some(),
            );
        }
    }

    /// `({ member } = instance)` is the assignment form of
    /// `const { member } = instance` and reads the same class members.
    pub(in crate::visitor) fn record_assignment_member_destructuring(
        &mut self,
        expr: &AssignmentExpression<'a>,
    ) {
        if let AssignmentTarget::ObjectAssignmentTarget(target) = &expr.left
            && let Expression::Identifier(source) = unwrap_static_expr(&expr.right)
            && let Some(BindingTarget::Class(class_name)) =
                self.resolve_bound_object_name(source.name.as_str())
        {
            let keys: Vec<_> = target
                .properties
                .iter()
                .map(|property| match property {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(shorthand) => {
                        Some(std::borrow::Cow::Borrowed(shorthand.binding.name.as_str()))
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                        property.name.static_name()
                    }
                })
                .collect();
            self.credit_destructured_class_members(
                &class_name,
                Some(source),
                &keys,
                target.rest.is_some(),
            );
        }
    }

    pub(in crate::visitor) fn record_typed_parameter_member_destructuring(
        &mut self,
        params: &FormalParameters<'_>,
    ) {
        for param in &params.items {
            if let BindingPattern::ObjectPattern(pattern) = &param.pattern
                && let Some(type_annotation) = param.type_annotation.as_deref()
            {
                self.record_typed_member_destructuring(pattern, &type_annotation.type_annotation);
            }
        }
    }

    /// `({ member }: Service) => ...` reads `Service.member`. The annotation is
    /// the only class evidence, so only a named type reference or a type
    /// literal property that names one credits members. A nested pattern
    /// recurses only through a type literal, because the property types of a
    /// named type are not known here.
    fn record_typed_member_destructuring(&mut self, pattern: &ObjectPattern<'_>, ty: &TSType<'_>) {
        if let TSType::TSTypeLiteral(type_lit) = ty {
            for property in &pattern.properties {
                let Some(key) = property.key.static_name() else {
                    continue;
                };
                let value = match &property.value {
                    BindingPattern::AssignmentPattern(assignment) => &assignment.left,
                    value => value,
                };
                let BindingPattern::ObjectPattern(inner) = value else {
                    continue;
                };
                let Some(property_type) = type_lit.members.iter().find_map(|member| {
                    let TSSignature::TSPropertySignature(signature) = member else {
                        return None;
                    };
                    (signature.key.static_name().as_deref() == Some(key.as_ref()))
                        .then_some(signature.type_annotation.as_deref())
                        .flatten()
                }) else {
                    continue;
                };
                self.record_typed_member_destructuring(inner, &property_type.type_annotation);
            }
            return;
        }
        // Store types keep their own destructure credit in the Pinia path.
        if self.store_factory_for_type(ty).is_some() {
            return;
        }
        let Some(class_name) =
            extract_type_reference_name(ty).and_then(|name| self.resolve_class_type_param(&name))
        else {
            return;
        };
        let keys: Vec<_> = pattern
            .properties
            .iter()
            .map(|property| property.key.static_name())
            .collect();
        self.credit_destructured_class_members(&class_name, None, &keys, pattern.rest.is_some());
    }

    /// Credit each statically named destructured key as a member access on
    /// `class_name`. `source` is the destructured instance binding, when one
    /// exists, so subclass proofs can follow it.
    fn credit_destructured_class_members(
        &mut self,
        class_name: &str,
        source: Option<&IdentifierReference<'_>>,
        keys: &[Option<std::borrow::Cow<'_, str>>],
        has_rest: bool,
    ) {
        if has_rest || keys.iter().any(Option::is_none) {
            // Rest copies unknown members, and a dynamic key can select any
            // member. Keep the same conservative credit as an opaque use.
            self.whole_object_uses.push(class_name.to_string());
        }
        for member in keys.iter().flatten() {
            if let Some(source) = source {
                self.capture_subclass_binding_access(
                    source.name.as_str(),
                    Some(source.span),
                    member,
                );
            }
            self.member_accesses.push(MemberAccess {
                object: class_name.to_string(),
                member: member.to_string(),
            });
        }
    }
}
