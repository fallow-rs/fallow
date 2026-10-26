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
use crate::js_text::static_name_text;
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
        let work = &mut DestructureWork::default();
        match &declarator.id {
            BindingPattern::ObjectPattern(pattern) => {
                self.record_object_destructure(DestructureObject::Binding(pattern), init, work);
            }
            BindingPattern::ArrayPattern(pattern) => {
                self.record_array_destructure(pattern, init, work);
            }
            _ => {}
        }
    }

    /// Credit the members that a binding pattern element reads from `source`.
    /// A default value is a second possible source.
    fn record_binding_destructure(
        &mut self,
        pattern: &BindingPattern<'a>,
        source: &Expression<'a>,
        work: &mut DestructureWork,
    ) {
        if !work.take() {
            return;
        }
        match pattern {
            BindingPattern::ObjectPattern(pattern) => {
                self.record_object_destructure(DestructureObject::Binding(pattern), source, work);
            }
            BindingPattern::ArrayPattern(pattern) => {
                self.record_array_destructure(pattern, source, work);
            }
            BindingPattern::AssignmentPattern(assignment) => {
                self.record_binding_destructure(&assignment.left, source, work);
                self.record_binding_destructure(&assignment.left, &assignment.right, work);
            }
            BindingPattern::BindingIdentifier(_) => {}
        }
    }

    /// `const [{ m }] = [instance]` reads `m` from the element at the same
    /// index of an array literal. A spread can move each later element to
    /// another index, so no element at or after a spread is a source. A
    /// default value stays a source at each index.
    fn record_array_destructure(
        &mut self,
        pattern: &ArrayPattern<'a>,
        source: &Expression<'a>,
        work: &mut DestructureWork,
    ) {
        if !work.take() {
            return;
        }
        let values = match unwrap_static_expr(source) {
            Expression::ArrayExpression(array) => &array.elements[..],
            _ => &[],
        };
        let mut after_spread = false;
        for (index, element) in pattern.elements.iter().enumerate() {
            let value = values.get(index);
            after_spread |= matches!(value, Some(ArrayExpressionElement::SpreadElement(_)));
            let Some(element) = element else {
                continue;
            };
            match value.and_then(ArrayExpressionElement::as_expression) {
                Some(value) if !after_spread => {
                    self.record_binding_destructure(element, value, work);
                }
                _ => {
                    if let BindingPattern::AssignmentPattern(assignment) = element {
                        self.record_binding_destructure(&assignment.left, &assignment.right, work);
                    }
                }
            }
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
                &mut DestructureWork::default(),
            );
        }
    }

    /// Credit the members that `pattern` reads from `source`. A nested pattern
    /// follows the source into an object literal property, a static member
    /// path (`{ k: { m } } = holder` reads `holder.k.m`) and a default value
    /// (`{ k: { m } = instance }` can read `instance.m`).
    fn record_object_destructure(
        &mut self,
        pattern: DestructureObject<'_, 'a>,
        source: &Expression<'a>,
        work: &mut DestructureWork,
    ) {
        if !work.take() {
            return;
        }
        let source = unwrap_static_expr(source);
        if let Expression::ObjectExpression(object) = source {
            self.record_literal_destructure(pattern, object, work);
            return;
        }
        let Some(path) = static_member_object_name(source) else {
            self.record_nested_entries(&pattern.entries(), None, work);
            return;
        };
        let ident = match source {
            Expression::Identifier(ident) => Some(&**ident),
            _ => None,
        };
        self.record_source_path_destructure(pattern, &path, ident, work);
    }

    /// `const { m } = holder` reads `holder.m`. A local class instance credits
    /// its class directly. Any other static path, such as an imported instance
    /// or `this.svc`, records the same `path.key` accesses as the equivalent
    /// member expression, so the later binding resolution finds the class.
    fn record_source_path_destructure(
        &mut self,
        pattern: DestructureObject<'_, 'a>,
        path: &str,
        source: Option<&IdentifierReference<'_>>,
        work: &mut DestructureWork,
    ) {
        let entries = pattern.entries();
        if let Some(BindingTarget::Class(class_name)) = self.resolve_bound_object_name(path) {
            let keys: Vec<_> = entries.iter().map(|entry| entry.key.clone()).collect();
            self.credit_destructured_class_members(&class_name, source, &keys, pattern.has_rest());
            self.record_nested_entries(&entries, Some(path), work);
            return;
        }
        if pattern.has_rest() || entries.iter().any(|entry| entry.key.is_none()) {
            // Rest copies unknown members, and a dynamic key can select any
            // member, the same as an opaque use of the source. The opaque use
            // covers each key, so only the nested patterns need a record.
            if !self.imported_root_is_shadowed(path) {
                self.record_whole_object_identifier_use(path);
            }
            self.record_nested_entries(&entries, Some(path), work);
            return;
        }
        self.record_path_destructure(pattern, path, work);
    }

    /// Whether the root of `path` is an import local that a nested declaration
    /// shadows. The analysis resolves an opaque use of `shared` through the
    /// import of `shared`, so a shadowed root must not record one.
    fn imported_root_is_shadowed(&self, path: &str) -> bool {
        let root = path.split('.').next().unwrap_or(path);
        self.nested_scope_shadows(root)
            && self.imports.iter().any(|import| import.local_name == root)
    }

    /// Credit the nested patterns of `entries`. Each one reads its default and,
    /// when the source has a static `path`, the member `path.key`.
    fn record_nested_entries(
        &mut self,
        entries: &[DestructureEntry<'_, 'a>],
        path: Option<&str>,
        work: &mut DestructureWork,
    ) {
        for entry in entries {
            let Some(nested) = entry.nested else {
                continue;
            };
            if let Some(default) = entry.default {
                self.record_object_destructure(nested, default, work);
            }
            if let (Some(path), Some(key)) = (path, entry.key.as_deref()) {
                self.record_path_destructure(nested, &format!("{path}.{key}"), work);
            }
        }
    }

    /// `{ k: { m } } = { k: instance }` reads `m` from the literal value of `k`.
    /// The literal itself has no class members, so only nested patterns and
    /// their defaults can read one. A spread `{ ...source }` can supply any
    /// key, so the pattern also destructures each spread argument.
    fn record_literal_destructure(
        &mut self,
        pattern: DestructureObject<'_, 'a>,
        object: &ObjectExpression<'a>,
        work: &mut DestructureWork,
    ) {
        for property in &object.properties {
            if let ObjectPropertyKind::SpreadProperty(spread) = property {
                self.record_object_destructure(pattern, &spread.argument, work);
            }
        }
        for entry in pattern.entries() {
            let Some(nested) = entry.nested else {
                continue;
            };
            if let Some(default) = entry.default {
                self.record_object_destructure(nested, default, work);
            }
            if let Some(value) = entry
                .key
                .as_deref()
                .and_then(|key| object_literal_property_value(object, key))
            {
                self.record_object_destructure(nested, value, work);
            }
        }
    }

    /// A nested pattern under a static source path reads the same members as
    /// the equivalent member expression, so each key goes through the
    /// `path.key` access path that `visit_static_member_expression` uses.
    fn record_path_destructure(
        &mut self,
        pattern: DestructureObject<'_, 'a>,
        path: &str,
        work: &mut DestructureWork,
    ) {
        if !work.take() {
            return;
        }
        for entry in pattern.entries() {
            if let (Some(nested), Some(default)) = (entry.nested, entry.default) {
                self.record_object_destructure(nested, default, work);
            }
            let Some(key) = entry.key.as_deref() else {
                continue;
            };
            self.record_named_member_access(path, key);
            if let Some(nested) = entry.nested {
                self.record_path_destructure(nested, &format!("{path}.{key}"), work);
            }
        }
    }

    /// A signature without a body (overload, `declare function`, abstract
    /// method) never runs, so its destructured keys read nothing.
    pub(in crate::visitor) fn record_function_member_destructuring(&mut self, func: &Function<'a>) {
        if func.body.is_some() {
            self.record_parameter_member_destructuring(&func.params);
        }
    }

    /// `({ m }: Service) => ...` and `({ m } = instance) => ...` read `m`
    /// from the annotated type or from the default value of the parameter.
    pub(in crate::visitor) fn record_parameter_member_destructuring(
        &mut self,
        params: &FormalParameters<'a>,
    ) {
        for param in &params.items {
            let BindingPattern::ObjectPattern(pattern) = &param.pattern else {
                continue;
            };
            if let Some(type_annotation) = param.type_annotation.as_deref() {
                self.record_typed_member_destructuring(pattern, &type_annotation.type_annotation);
            }
            if let Some(initializer) = param.initializer.as_deref() {
                self.record_object_destructure(
                    DestructureObject::Binding(pattern),
                    initializer,
                    &mut DestructureWork::default(),
                );
            }
        }
    }

    /// `for (const { m } of items)` reads `m` from each element. An array
    /// literal gives each element as a source, and a spread in it gives the
    /// elements of the spread iterable.
    pub(in crate::visitor) fn record_for_of_member_destructuring(
        &mut self,
        stmt: &ForOfStatement<'a>,
    ) {
        let pattern = match &stmt.left {
            ForStatementLeft::VariableDeclaration(declaration) => {
                match declaration
                    .declarations
                    .first()
                    .map(|declarator| &declarator.id)
                {
                    Some(BindingPattern::ObjectPattern(pattern)) => {
                        DestructureObject::Binding(pattern)
                    }
                    Some(BindingPattern::ArrayPattern(pattern)) => {
                        self.record_for_of_array_destructure(pattern, &stmt.right);
                        return;
                    }
                    _ => return,
                }
            }
            ForStatementLeft::ObjectAssignmentTarget(target) => {
                DestructureObject::Assignment(target)
            }
            _ => return,
        };
        let work = &mut DestructureWork::default();
        let iterable = unwrap_static_expr(&stmt.right);
        let Expression::ArrayExpression(array) = iterable else {
            self.record_iterable_element_destructure(pattern, iterable);
            return;
        };
        for element in &array.elements {
            if let ArrayExpressionElement::SpreadElement(spread) = element {
                self.record_iterable_element_destructure(pattern, &spread.argument);
            } else if let Some(element) = element.as_expression() {
                self.record_object_destructure(pattern, element, work);
            }
        }
    }

    /// `for (const [{ m }] of [[instance]])` destructures each element of an
    /// array literal with the array pattern.
    fn record_for_of_array_destructure(
        &mut self,
        pattern: &ArrayPattern<'a>,
        iterable: &Expression<'a>,
    ) {
        let Expression::ArrayExpression(array) = unwrap_static_expr(iterable) else {
            return;
        };
        let work = &mut DestructureWork::default();
        for element in &array.elements {
            if let Some(element) = element.as_expression() {
                self.record_array_destructure(pattern, element, work);
            }
        }
    }

    /// Credit the members that `pattern` reads from each element of a typed
    /// iterable, such as `items: Service[]`.
    fn record_iterable_element_destructure(
        &mut self,
        pattern: DestructureObject<'_, 'a>,
        iterable: &Expression<'a>,
    ) {
        let Some(receiver) = static_member_object_name(unwrap_static_expr(iterable)) else {
            return;
        };
        let receiver = self.qualify_this_scope(&receiver);
        if let Some(class_name) = self.iterable_element_type_for(&receiver) {
            let keys: Vec<_> = pattern
                .entries()
                .into_iter()
                .map(|entry| entry.key)
                .collect();
            self.credit_destructured_class_members(&class_name, None, &keys, pattern.has_rest());
        }
    }

    /// `({ k: { m } }: typeof holder)` destructures the type of the value
    /// `holder`, so it reads the same members as `const { k: { m } } = holder`.
    fn record_typeof_destructure(&mut self, pattern: &ObjectPattern<'a>, path: &str) {
        self.record_source_path_destructure(
            DestructureObject::Binding(pattern),
            path,
            None,
            &mut DestructureWork::default(),
        );
    }

    /// `({ member }: Service) => ...` reads `Service.member`. The annotation is
    /// the only class evidence, so only a named type reference or a type
    /// literal property that names one credits members. A nested pattern
    /// recurses only through a type literal, because the property types of a
    /// named type are not known here.
    fn record_typed_member_destructuring(&mut self, pattern: &ObjectPattern<'a>, ty: &TSType<'_>) {
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
                    (signature.key.static_name().as_ref() == Some(&key))
                        .then_some(signature.type_annotation.as_deref())
                        .flatten()
                }) else {
                    continue;
                };
                self.record_typed_member_destructuring(inner, &property_type.type_annotation);
            }
            return;
        }
        if let TSType::TSTypeQuery(query) = ty {
            if query.type_arguments.is_none()
                && let Some(path) = type_query_path(&query.expr_name)
            {
                self.record_typeof_destructure(pattern, &path);
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
            .map(|property| property.key.static_name().map(static_name_text))
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

/// The most destructure steps that one destructure records. A pattern with a
/// default at each level is walked once for the source and once for the
/// default, so the steps double at each level. The cap bounds that cost.
const MAX_DESTRUCTURE_STEPS: usize = 1024;

/// The destructure steps that one destructure has left.
struct DestructureWork(usize);

impl Default for DestructureWork {
    fn default() -> Self {
        Self(MAX_DESTRUCTURE_STEPS)
    }
}

impl DestructureWork {
    /// Use one step. Returns `false` when no step is left.
    fn take(&mut self) -> bool {
        let Some(left) = self.0.checked_sub(1) else {
            return false;
        };
        self.0 = left;
        true
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
                        key: property.key.static_name().map(static_name_text),
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
                            key: property.name.static_name().map(static_name_text),
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
/// its credit. A getter `get key() { return value }` gives `value` when its
/// body is that one return statement.
fn object_literal_property_value<'b, 'a>(
    object: &'b ObjectExpression<'a>,
    key: &str,
) -> Option<&'b Expression<'a>> {
    object.properties.iter().rev().find_map(|property| {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return None;
        };
        if !property.key.static_name().is_some_and(|name| name == key) {
            return None;
        }
        object_property_value(property)
    })
}

/// The value that reading an object literal property gives: the value of a
/// plain property, or the value that a getter returns. A setter gives none.
pub(in crate::visitor) fn object_property_value<'b, 'a>(
    property: &'b ObjectProperty<'a>,
) -> Option<&'b Expression<'a>> {
    match property.kind {
        PropertyKind::Init => Some(&property.value),
        PropertyKind::Get => getter_return_value(&property.value),
        PropertyKind::Set => None,
    }
}

/// The value that a getter `get key() { return value }` gives, when its body
/// is that one return statement.
fn getter_return_value<'b, 'a>(value: &'b Expression<'a>) -> Option<&'b Expression<'a>> {
    let Expression::FunctionExpression(getter) = value else {
        return None;
    };
    match getter.body.as_deref()?.statements.as_slice() {
        [Statement::ReturnStatement(statement)] => statement.argument.as_ref(),
        _ => None,
    }
}

/// The dotted value path of a `typeof` query, such as `holder` or
/// `registry.holder`.
fn type_query_path(name: &TSTypeQueryExprName<'_>) -> Option<String> {
    fn type_name_path(name: &TSTypeName<'_>) -> Option<String> {
        match name {
            TSTypeName::IdentifierReference(ident) => Some(ident.name.to_string()),
            TSTypeName::QualifiedName(qualified) => Some(format!(
                "{}.{}",
                type_name_path(&qualified.left)?,
                qualified.right.name
            )),
            TSTypeName::ThisExpression(_) => None,
        }
    }
    type_name_path(name.as_ts_type_name()?)
}
