//! Class instance bindings from declared types: the return type of a getter
//! or a method, and the element type of an array. Also the return type checks
//! of methods that return an instance of their class.

use oxc_ast::ast::{
    Class, Expression, MethodDefinition, Statement, TSAccessibility, TSType, TSTypeAnnotation,
    TSTypeName,
};
use oxc_span::Span;
use rustc_hash::FxHashMap;

use super::{array_element_type_from_type, extract_type_annotation_name};
use crate::js_text::JsText;

/// Push an instance binding for a non-private typed getter's return type.
pub(super) fn collect_getter_binding(
    method: &MethodDefinition<'_>,
    resolve: &impl Fn(String) -> Option<String>,
    resolve_origin: &impl Fn(&TSType<'_>) -> Option<Span>,
    bindings: &mut Vec<(String, String)>,
    origins: &mut FxHashMap<String, Span>,
) {
    if matches!(method.accessibility, Some(TSAccessibility::Private)) {
        return;
    }
    let Some(name) = method.key.static_name() else {
        return;
    };
    let Some(type_annotation) = method.value.return_type.as_deref() else {
        return;
    };
    push_array_element_binding(&name.text(), type_annotation, resolve, bindings);
    let Some(type_name) = extract_type_annotation_name(type_annotation) else {
        return;
    };
    let Some(resolved) = resolve(type_name) else {
        return;
    };
    if let Some(origin) = resolve_origin(&type_annotation.type_annotation) {
        origins.insert(name.text().into_owned(), origin);
    }
    bindings.push((name.text().into_owned(), resolved));
}

/// Push an instance binding `"<method>()" -> <ReturnType>` for a non-private,
/// non-static method with a declared named return type. A consumer chain
/// segment `current()` (from `manager.current().member`) then resolves to the
/// return type, so the access credits `ReturnType.member` (issue #3868).
/// A return type that names a method type parameter is skipped: the binding
/// would name a type that does not exist outside the method. A method that
/// returns `this` binds to the class itself, so a chain such as
/// `link.setId(id).setLabel(label)` credits `setLabel` on the class.
pub(super) fn collect_method_return_binding(
    class: &Class<'_>,
    method: &MethodDefinition<'_>,
    resolve: &impl Fn(String) -> Option<String>,
    bindings: &mut Vec<(String, String)>,
) {
    if method.r#static || matches!(method.accessibility, Some(TSAccessibility::Private)) {
        return;
    }
    let Some(name) = method.key.static_name() else {
        return;
    };
    let Some(type_annotation) = method.value.return_type.as_deref() else {
        return;
    };
    let key = format!("{}()", name.text());
    // A generic method can return an array of its own type parameter, a name
    // that does not exist outside the method.
    if method.value.type_parameters.is_none() {
        push_array_element_binding(&key, type_annotation, resolve, bindings);
    }
    if matches!(type_annotation.type_annotation, TSType::TSThisType(_)) {
        if let Some(id) = &class.id
            && !bindings.iter().any(|(existing, _)| *existing == key)
        {
            bindings.push((key, id.name.to_string()));
        }
        return;
    }
    let Some(type_name) = extract_type_annotation_name(type_annotation) else {
        return;
    };
    let is_method_type_param = method
        .value
        .type_parameters
        .as_deref()
        .is_some_and(|params| {
            params
                .params
                .iter()
                .any(|param| param.name.name.as_str() == type_name)
        });
    if is_method_type_param {
        return;
    }
    let Some(resolved) = resolve(type_name) else {
        return;
    };
    if bindings.iter().any(|(existing, _)| *existing == key) {
        return;
    }
    bindings.push((key, resolved));
}

/// Push an instance binding `"<name>[]" -> <Element>` when the annotation is
/// an array of a named type (`days: Day[]`, `Array<Day>`). A receiver chain
/// spells an element of the array as a `[]` link (`month.days[]` for
/// `month.days[0]`, a loop variable, or an iteration callback parameter), so
/// the chain resolves to the element class through this binding.
pub(super) fn push_array_element_binding(
    name: &str,
    type_annotation: &TSTypeAnnotation<'_>,
    resolve: &impl Fn(String) -> Option<String>,
    bindings: &mut Vec<(String, String)>,
) {
    let Some(element) = array_element_type_from_type(&type_annotation.type_annotation) else {
        return;
    };
    let Some(resolved) = resolve(element) else {
        return;
    };
    let key = format!("{name}[]");
    if !bindings.iter().any(|(existing, _)| *existing == key) {
        bindings.push((key, resolved));
    }
}

pub(super) fn is_self_returning_instance_method(
    method: &oxc_ast::ast::MethodDefinition<'_>,
    class_name: Option<&str>,
) -> bool {
    if returns_named_class_type(method.value.return_type.as_ref(), class_name) {
        return true;
    }
    if returns_this_type(method.value.return_type.as_ref()) {
        return true;
    }
    let Some(body) = method.value.body.as_ref() else {
        return false;
    };
    body.statements.last().is_some_and(|stmt| {
        let Statement::ReturnStatement(ret) = stmt else {
            return false;
        };
        matches!(ret.argument.as_ref(), Some(Expression::ThisExpression(_)))
    })
}

pub(super) fn returns_named_class_type(
    return_type: Option<&oxc_allocator::Box<'_, oxc_ast::ast::TSTypeAnnotation<'_>>>,
    class_name: Option<&str>,
) -> bool {
    let Some(name) = class_name else {
        return false;
    };
    let Some(annotation) = return_type.map(|boxed| boxed.as_ref()) else {
        return false;
    };
    extract_type_annotation_name(annotation).is_some_and(|ty| ty == name)
}

fn returns_this_type(
    return_type: Option<&oxc_allocator::Box<'_, oxc_ast::ast::TSTypeAnnotation<'_>>>,
) -> bool {
    let Some(annotation) = return_type.map(|boxed| boxed.as_ref()) else {
        return false;
    };
    is_this_type(&annotation.type_annotation)
}

fn is_this_type(ty: &TSType<'_>) -> bool {
    match ty {
        TSType::TSThisType(_) => true,
        TSType::TSTypeReference(type_ref) => match &type_ref.type_name {
            TSTypeName::ThisExpression(_) => true,
            TSTypeName::IdentifierReference(ident) => ident.name == "this",
            TSTypeName::QualifiedName(_) => false,
        },
        TSType::TSParenthesizedType(paren) => is_this_type(&paren.type_annotation),
        _ => false,
    }
}

/// Mark each static method whose last statement returns `new Self(...)`
/// through a chain of instance methods that return `this`, as in
/// `static from() { return new Self().withA().withB(); }`. Such a method
/// returns an instance of the class (issue #5298).
pub(super) fn mark_builder_chain_statics(
    class: &Class<'_>,
    class_name: Option<&str>,
    members: &mut [crate::MemberInfo],
) {
    let self_returning: rustc_hash::FxHashSet<&str> = members
        .iter()
        .filter(|member| !member.is_static && member.is_self_returning)
        .map(|member| member.name.as_str())
        .collect();
    if self_returning.is_empty() {
        return;
    }
    let mut builder_statics = Vec::new();
    for element in &class.body.body {
        let oxc_ast::ast::ClassElement::MethodDefinition(method) = element else {
            continue;
        };
        if !method.r#static || method.value.r#async || method.value.generator {
            continue;
        }
        let Some(name) = method.key.static_name() else {
            continue;
        };
        let returns_chain = method.value.body.as_ref().is_some_and(|body| {
            every_return_is_builder_chain(&body.statements, class_name, &self_returning)
        });
        if returns_chain {
            builder_statics.push(name.text().into_owned());
        }
    }
    for member in members.iter_mut() {
        if member.is_static && builder_statics.contains(&member.name) {
            member.is_instance_returning_static = true;
        }
    }
}

/// Whether the body ends in `return` and every `return` in it gives a
/// builder chain of the class. An early return of another value, or a bare
/// `return;`, would make the call result a different type.
fn every_return_is_builder_chain(
    statements: &[Statement<'_>],
    class_name: Option<&str>,
    self_returning: &rustc_hash::FxHashSet<&str>,
) -> bool {
    if !matches!(statements.last(), Some(Statement::ReturnStatement(_))) {
        return false;
    }
    let mut arguments = Vec::new();
    super::super::visit_impl::collect_return_args_in_statements(statements, &mut arguments);
    arguments.len() == super::super::visit_impl::count_returns_in_statements(statements)
        && arguments
            .iter()
            .all(|argument| is_builder_chain_of_self(argument, class_name, self_returning))
}

/// Whether `expr` is `new Self(...)` followed by at least one call of a
/// method in `self_returning`.
fn is_builder_chain_of_self(
    expr: &Expression<'_>,
    class_name: Option<&str>,
    self_returning: &rustc_hash::FxHashSet<&str>,
) -> bool {
    let mut current = expr;
    let mut steps = 0usize;
    loop {
        match current {
            Expression::CallExpression(call) => {
                let Some(member) = call.callee.as_member_expression() else {
                    return false;
                };
                let Some(method) = member.static_property_name() else {
                    return false;
                };
                if !self_returning.contains(method.text().as_ref()) {
                    return false;
                }
                steps += 1;
                current = member.object();
            }
            Expression::ParenthesizedExpression(paren) => current = &paren.expression,
            Expression::NewExpression(new_expr) => {
                return steps > 0
                    && match &new_expr.callee {
                        Expression::ThisExpression(_) => true,
                        Expression::Identifier(ident) => {
                            class_name.is_some_and(|name| ident.name.as_str() == name)
                        }
                        _ => false,
                    };
            }
            _ => return false,
        }
    }
}
