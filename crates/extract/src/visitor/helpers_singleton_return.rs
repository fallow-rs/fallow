//! Detection of a static singleton getter without a return type annotation.
//!
//! `static getInstance() { ...; return Cls.instance; }` gives an instance of
//! the class when the static field holds one. Calls such as
//! `Cls.getInstance().member` must then credit `member`. See issue #5233.

use oxc_ast::ast::{
    AssignmentExpression, AssignmentTarget, Class, ClassElement, Expression, FunctionBody,
    LogicalOperator, PropertyKey, Statement,
};
use oxc_ast_visit::Visit;

use super::{extract_type_annotation_name, is_self_construction_expression};

/// A static field of the class. `#instance` and `instance` are different
/// fields, so the private flag is part of the identity.
#[derive(Clone, Copy, PartialEq, Eq)]
struct StaticField<'a> {
    name: &'a str,
    private: bool,
}

/// Whether the last statement of `body` returns the instance that a static
/// field of `class` holds.
///
/// These return forms qualify, also inside `!`, `as` or `satisfies`:
/// - `return (Cls.instance ??= new Cls())`, also with `=` and `||=`;
/// - `return Cls.instance ?? (Cls.instance = new Cls())`, also with `||`;
/// - `return Cls.instance` (or `this.instance`), when the field declares the
///   class as its type, starts as `new Cls()`, or the body assigns
///   `new Cls()` to it.
pub(super) fn returns_static_singleton_instance(
    class: &Class<'_>,
    body: &FunctionBody<'_>,
    class_name: Option<&str>,
) -> bool {
    let Some(name) = class_name else {
        return false;
    };
    let Some(Statement::ReturnStatement(ret)) = body.statements.last() else {
        return false;
    };
    let Some(argument) = ret.argument.as_ref() else {
        return false;
    };
    match argument.get_inner_expression() {
        Expression::AssignmentExpression(assign) => singleton_assignment(assign, name).is_some(),
        Expression::LogicalExpression(logical)
            if matches!(
                logical.operator,
                LogicalOperator::Coalesce | LogicalOperator::Or
            ) =>
        {
            let Expression::AssignmentExpression(assign) = logical.right.get_inner_expression()
            else {
                return false;
            };
            let left = static_field(logical.left.get_inner_expression(), name);
            left.is_some() && singleton_assignment(assign, name) == left
        }
        expr => static_field(expr, name).is_some_and(|field| {
            static_field_holds_instance(class, field, name)
                || body_assigns_instance(body, field, name)
        }),
    }
}

/// The field of `Cls.field`, `Cls.#field`, `this.field` or `this.#field`.
/// In a static method, `this` is the class.
fn static_field<'a>(expr: &'a Expression<'_>, class_name: &str) -> Option<StaticField<'a>> {
    let (object, field) = match expr {
        Expression::StaticMemberExpression(member) => (
            &member.object,
            StaticField {
                name: member.property.name.as_str(),
                private: false,
            },
        ),
        Expression::PrivateFieldExpression(member) => (
            &member.object,
            StaticField {
                name: member.field.name.as_str(),
                private: true,
            },
        ),
        _ => return None,
    };
    is_class_receiver(object, class_name).then_some(field)
}

fn is_class_receiver(object: &Expression<'_>, class_name: &str) -> bool {
    match object.get_inner_expression() {
        Expression::ThisExpression(_) => true,
        Expression::Identifier(ident) => ident.name.as_str() == class_name,
        _ => false,
    }
}

/// The field when `assign` writes `new Cls()` to a static field.
fn singleton_assignment<'a>(
    assign: &'a AssignmentExpression<'_>,
    class_name: &str,
) -> Option<StaticField<'a>> {
    if !is_self_construction_expression(assign.right.get_inner_expression(), Some(class_name)) {
        return None;
    }
    let (object, field) = match &assign.left {
        AssignmentTarget::StaticMemberExpression(member) => (
            &member.object,
            StaticField {
                name: member.property.name.as_str(),
                private: false,
            },
        ),
        AssignmentTarget::PrivateFieldExpression(member) => (
            &member.object,
            StaticField {
                name: member.field.name.as_str(),
                private: true,
            },
        ),
        _ => return None,
    };
    is_class_receiver(object, class_name).then_some(field)
}

fn static_field_holds_instance(
    class: &Class<'_>,
    field: StaticField<'_>,
    class_name: &str,
) -> bool {
    class.body.body.iter().any(|element| {
        let ClassElement::PropertyDefinition(prop) = element else {
            return false;
        };
        if !prop.r#static || property_key_field(&prop.key) != Some(field) {
            return false;
        }
        let typed_as_class = prop
            .type_annotation
            .as_deref()
            .and_then(extract_type_annotation_name)
            .is_some_and(|ty| ty == class_name);
        let starts_as_instance = prop.value.as_ref().is_some_and(|value| {
            is_self_construction_expression(value.get_inner_expression(), Some(class_name))
        });
        typed_as_class || starts_as_instance
    })
}

fn property_key_field<'a>(key: &'a PropertyKey<'_>) -> Option<StaticField<'a>> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some(StaticField {
            name: ident.name.as_str(),
            private: false,
        }),
        PropertyKey::PrivateIdentifier(ident) => Some(StaticField {
            name: ident.name.as_str(),
            private: true,
        }),
        _ => None,
    }
}

fn body_assigns_instance(
    body: &FunctionBody<'_>,
    field: StaticField<'_>,
    class_name: &str,
) -> bool {
    let mut finder = SingletonAssignmentFinder {
        field,
        class_name,
        found: false,
    };
    finder.visit_function_body(body);
    finder.found
}

struct SingletonAssignmentFinder<'s> {
    field: StaticField<'s>,
    class_name: &'s str,
    found: bool,
}

impl<'a> Visit<'a> for SingletonAssignmentFinder<'_> {
    fn visit_assignment_expression(&mut self, assign: &AssignmentExpression<'a>) {
        if singleton_assignment(assign, self.class_name) == Some(self.field) {
            self.found = true;
            return;
        }
        oxc_ast_visit::walk::walk_assignment_expression(self, assign);
    }

    // A nested function or class body may never run, so an assignment in it
    // proves nothing about the value that the getter returns.
    fn visit_function(
        &mut self,
        _func: &oxc_ast::ast::Function<'a>,
        _flags: oxc_syntax::scope::ScopeFlags,
    ) {
    }

    fn visit_arrow_function_expression(
        &mut self,
        _arrow: &oxc_ast::ast::ArrowFunctionExpression<'a>,
    ) {
    }

    fn visit_class(&mut self, _class: &Class<'a>) {}
}
