//! Class member credit for destructures that read an instance.
//!
//! `const { m } = instance`, `({ m } = instance)` and `({ m }: Service) => ...`
//! each read `m` from a class instance. The binding that holds the read is a
//! plain local, so no later `m` access resolves to the class. These helpers
//! record the access at the destructure site.

#[allow(clippy::wildcard_imports, reason = "many destructure AST types used")]
use oxc_ast::ast::*;

use std::borrow::Cow;

use super::super::{BindingTarget, ModuleInfoExtractor};
use super::{static_member_object_name, unwrap_static_expr};
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
        if let BindingPattern::ObjectPattern(pattern) = &declarator.id {
            self.record_object_destructure(DestructureObject::Binding(pattern), init, true);
        }
    }

    /// `({ member } = instance)` is the assignment form of
    /// `const { member } = instance` and reads the same class members.
    pub(in crate::visitor) fn record_assignment_member_destructuring(
        &mut self,
        expr: &AssignmentExpression<'a>,
    ) {
        if let AssignmentTarget::ObjectAssignmentTarget(target) = &expr.left {
            self.record_object_destructure(
                DestructureObject::Assignment(target),
                &expr.right,
                true,
            );
        }
    }

    /// Credit the members that `pattern` reads from `source`. A nested pattern
    /// follows the source into an object literal property, a static member
    /// path (`{ k: { m } } = holder` reads `holder.k.m`) and a default value
    /// (`{ k: { m } = instance }` can read `instance.m`). The top-level keys
    /// of a source that is not a class instance stay without credit.
    fn record_object_destructure(
        &mut self,
        pattern: DestructureObject<'_, 'a>,
        source: &Expression<'a>,
        top_level: bool,
    ) {
        let source = unwrap_static_expr(source);
        if let Expression::ObjectExpression(object) = source {
            self.record_literal_destructure(pattern, object);
            return;
        }
        let entries = pattern.entries();
        if let Expression::Identifier(ident) = source
            && let Some(BindingTarget::Class(class_name)) =
                self.resolve_bound_object_name(ident.name.as_str())
        {
            let keys: Vec<_> = entries.iter().map(|entry| entry.key.clone()).collect();
            self.credit_destructured_class_members(
                &class_name,
                Some(ident),
                &keys,
                pattern.has_rest(),
            );
        } else if !top_level && let Some(path) = static_member_object_name(source) {
            self.record_path_destructure(pattern, &path);
            return;
        }
        let mut source_path = None;
        for entry in &entries {
            let Some(nested) = entry.nested else {
                continue;
            };
            if let Some(default) = entry.default {
                self.record_object_destructure(nested, default, false);
            }
            let Some(key) = entry.key.as_deref() else {
                continue;
            };
            if let Some(path) = source_path.get_or_insert_with(|| static_member_object_name(source))
            {
                self.record_path_destructure(nested, &format!("{path}.{key}"));
            }
        }
    }

    /// `{ k: { m } } = { k: instance }` reads `m` from the literal value of `k`.
    /// The literal itself has no class members, so only nested patterns and
    /// their defaults can read one.
    fn record_literal_destructure(
        &mut self,
        pattern: DestructureObject<'_, 'a>,
        object: &ObjectExpression<'a>,
    ) {
        for entry in pattern.entries() {
            let Some(nested) = entry.nested else {
                continue;
            };
            if let Some(default) = entry.default {
                self.record_object_destructure(nested, default, false);
            }
            if let Some(value) = entry
                .key
                .as_deref()
                .and_then(|key| object_literal_property_value(object, key))
            {
                self.record_object_destructure(nested, value, false);
            }
        }
    }

    /// A nested pattern under a static source path reads the same members as
    /// the equivalent member expression, so each key goes through the
    /// `path.key` access path that `visit_static_member_expression` uses.
    fn record_path_destructure(&mut self, pattern: DestructureObject<'_, 'a>, path: &str) {
        for entry in pattern.entries() {
            if let (Some(nested), Some(default)) = (entry.nested, entry.default) {
                self.record_object_destructure(nested, default, false);
            }
            let Some(key) = entry.key.as_deref() else {
                continue;
            };
            self.record_named_member_access(path, key);
            if let Some(nested) = entry.nested {
                self.record_path_destructure(nested, &format!("{path}.{key}"));
            }
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
        keys: &[Option<Cow<'_, str>>],
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

/// An object destructure in binding (`const { .. } =`) or assignment
/// (`({ .. } =)`) form.
#[derive(Clone, Copy)]
enum DestructureObject<'b, 'a> {
    Binding(&'b ObjectPattern<'a>),
    Assignment(&'b ObjectAssignmentTarget<'a>),
}

/// One property of a destructure: its static key, the nested object pattern
/// that its value destructures, and the default value of that property.
struct DestructureEntry<'b, 'a> {
    key: Option<Cow<'b, str>>,
    nested: Option<DestructureObject<'b, 'a>>,
    default: Option<&'b Expression<'a>>,
}

impl<'b, 'a> DestructureObject<'b, 'a> {
    fn has_rest(self) -> bool {
        match self {
            Self::Binding(pattern) => pattern.rest.is_some(),
            Self::Assignment(target) => target.rest.is_some(),
        }
    }

    fn entries(self) -> Vec<DestructureEntry<'b, 'a>> {
        match self {
            Self::Binding(pattern) => pattern
                .properties
                .iter()
                .map(|property| {
                    let (value, default) = match &property.value {
                        BindingPattern::AssignmentPattern(assignment) => {
                            (&assignment.left, Some(&assignment.right))
                        }
                        value => (value, None),
                    };
                    let nested = match value {
                        BindingPattern::ObjectPattern(inner) => Some(Self::Binding(inner)),
                        _ => None,
                    };
                    DestructureEntry {
                        key: property.key.static_name(),
                        nested,
                        default: nested.and(default),
                    }
                })
                .collect(),
            Self::Assignment(target) => target
                .properties
                .iter()
                .map(|property| match property {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(shorthand) => {
                        DestructureEntry {
                            key: Some(Cow::Borrowed(shorthand.binding.name.as_str())),
                            nested: None,
                            default: None,
                        }
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                        let (value, default) = match &property.binding {
                            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(
                                with_default,
                            ) => (Some(&with_default.binding), Some(&with_default.init)),
                            value => (value.as_assignment_target(), None),
                        };
                        let nested = match value {
                            Some(AssignmentTarget::ObjectAssignmentTarget(inner)) => {
                                Some(Self::Assignment(inner))
                            }
                            _ => None,
                        };
                        DestructureEntry {
                            key: property.name.static_name(),
                            nested,
                            default: nested.and(default),
                        }
                    }
                })
                .collect(),
        }
    }
}

/// The value of the last `key` property in an object literal. A spread can
/// also set `key`, but the explicit value stays a possible source, so it keeps
/// its credit.
fn object_literal_property_value<'b, 'a>(
    object: &'b ObjectExpression<'a>,
    key: &str,
) -> Option<&'b Expression<'a>> {
    object
        .properties
        .iter()
        .rev()
        .find_map(|property| match property {
            ObjectPropertyKind::ObjectProperty(property)
                if property.kind == PropertyKind::Init
                    && property.key.static_name().as_deref() == Some(key) =>
            {
                Some(&property.value)
            }
            _ => None,
        })
}
