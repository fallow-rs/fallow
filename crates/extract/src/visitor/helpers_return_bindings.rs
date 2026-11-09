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
