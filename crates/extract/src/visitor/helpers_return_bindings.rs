//! Class instance bindings from the declared return type of a getter or a
//! method.

use oxc_ast::ast::{MethodDefinition, TSAccessibility, TSType};
use oxc_span::Span;
use rustc_hash::FxHashMap;

use super::extract_type_annotation_name;
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
/// would name a type that does not exist outside the method.
pub(super) fn collect_method_return_binding(
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
    let key = format!("{}()", name.text());
    if bindings.iter().any(|(existing, _)| *existing == key) {
        return;
    }
    bindings.push((key, resolved));
}
