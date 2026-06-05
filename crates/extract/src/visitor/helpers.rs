use oxc_ast::ast::{
    Argument, ArrayExpressionElement, BinaryExpression, BindingPattern, CallExpression, Class,
    ClassElement, Expression, MethodDefinitionKind, ObjectPropertyKind, Statement, TSAccessibility,
    TSSignature, TSType, TSTypeAnnotation, TSTypeName,
};
use oxc_span::{GetSpan, Span};
use rustc_hash::FxHashMap;

use crate::{MemberInfo, MemberKind};

pub struct AngularComponentMetadata {
    pub template_url: Option<String>,
    pub style_urls: Vec<String>,
    pub inline_template: Option<String>,
    pub inline_template_offset: Option<u32>,
    pub decorator_span: Span,
    pub host_member_refs: Vec<String>,
    pub input_output_members: Vec<String>,
}

const ANGULAR_SIGNAL_APIS: &[&str] = &[
    "input",
    "output",
    "outputFromObservable",
    "model",
    "viewChild",
    "viewChildren",
    "contentChild",
    "contentChildren",
];

pub fn extract_angular_component_metadata(class: &Class<'_>) -> Option<AngularComponentMetadata> {
    for decorator in &class.decorators {
        let Expression::CallExpression(call) = &decorator.expression else {
            continue;
        };
        let Expression::Identifier(id) = &call.callee else {
            continue;
        };
        if !matches!(id.name.as_str(), "Component" | "Directive") {
            continue;
        }
        let Some(Argument::ObjectExpression(obj)) = call.arguments.first() else {
            continue;
        };

        let mut template_url = None;
        let mut style_urls = Vec::new();
        let mut inline_template = None;
        let mut inline_template_offset = None;
        let mut host_member_refs = Vec::new();
        let mut input_output_members = Vec::new();

        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(p) = prop else {
                continue;
            };
            let Some(key_name) = p.key.static_name() else {
                continue;
            };
            match key_name.as_ref() {
                "templateUrl" => {
                    if let Expression::StringLiteral(lit) = &p.value {
                        template_url = Some(lit.value.to_string());
                    }
                }
                "template" => {
                    if let Expression::StringLiteral(lit) = &p.value {
                        inline_template = Some(lit.value.to_string());
                        inline_template_offset = Some(lit.span.start.saturating_add(1));
                    } else if let Expression::TemplateLiteral(tpl) = &p.value
                        && tpl.expressions.is_empty()
                        && let Some(quasi) = tpl.quasis.first()
                    {
                        let source = quasi
                            .value
                            .cooked
                            .as_ref()
                            .map_or_else(|| quasi.value.raw.as_str(), |c| c.as_str())
                            .to_string();
                        inline_template = Some(source);
                        inline_template_offset = Some(p.value.span().start.saturating_add(1));
                    }
                }
                "styleUrl" => {
                    if let Expression::StringLiteral(lit) = &p.value {
                        style_urls.push(lit.value.to_string());
                    }
                }
                "styleUrls" => {
                    if let Expression::ArrayExpression(arr) = &p.value {
                        for elem in &arr.elements {
                            if let ArrayExpressionElement::StringLiteral(lit) = elem {
                                style_urls.push(lit.value.to_string());
                            }
                        }
                    }
                }
                "host" => {
                    if let Expression::ObjectExpression(host_obj) = &p.value {
                        extract_host_member_refs(host_obj, &mut host_member_refs);
                    }
                }
                "inputs" | "outputs" => {
                    extract_input_output_members(&p.value, &mut input_output_members);
                }
                "queries" => {
                    extract_query_members(&p.value, &mut input_output_members);
                }
                _ => {}
            }
        }

        let has_data = template_url.is_some()
            || !style_urls.is_empty()
            || inline_template.is_some()
            || !host_member_refs.is_empty()
            || !input_output_members.is_empty();

        if has_data {
            return Some(AngularComponentMetadata {
                template_url,
                style_urls,
                inline_template,
                inline_template_offset,
                decorator_span: decorator.span(),
                host_member_refs,
                input_output_members,
            });
        }
    }
    None
}

fn extract_host_member_refs(host_obj: &oxc_ast::ast::ObjectExpression<'_>, refs: &mut Vec<String>) {
    for prop in &host_obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        if let Expression::StringLiteral(lit) = &p.value {
            extract_identifiers_from_host_expr(&lit.value, refs);
        }
    }
}

fn extract_query_members(value: &Expression<'_>, members: &mut Vec<String>) {
    let Expression::ObjectExpression(obj) = value else {
        return;
    };
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        if let Some(name) = p.key.static_name() {
            let name = name.to_string();
            if !name.is_empty() {
                members.push(name);
            }
        }
    }
}

fn extract_input_output_members(value: &Expression<'_>, members: &mut Vec<String>) {
    let Expression::ArrayExpression(arr) = value else {
        return;
    };
    for elem in &arr.elements {
        let ArrayExpressionElement::StringLiteral(lit) = elem else {
            continue;
        };
        let member = lit
            .value
            .as_ref()
            .split(':')
            .next()
            .unwrap_or_default()
            .trim();
        if !member.is_empty() {
            members.push(member.to_string());
        }
    }
}

fn extract_identifiers_from_host_expr(expr: &str, refs: &mut Vec<String>) {
    let expr = expr.trim();
    if expr.is_empty() {
        return;
    }
    let ident: String = expr
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if !is_valid_member_identifier(&ident) || refs.contains(&ident) {
        return;
    }
    refs.push(ident);
}

fn is_valid_member_identifier(ident: &str) -> bool {
    !ident.is_empty()
        && ident
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && !matches!(
            ident,
            "true"
                | "false"
                | "null"
                | "undefined"
                | "this"
                | "event"
                | "window"
                | "document"
                | "console"
                | "Math"
                | "JSON"
                | "Object"
                | "Array"
                | "String"
                | "Number"
                | "Boolean"
                | "Date"
                | "RegExp"
                | "Error"
                | "Promise"
        )
}

pub fn has_angular_class_decorator(class: &Class<'_>) -> bool {
    class.decorators.iter().any(|d| {
        if let Expression::CallExpression(call) = &d.expression
            && let Expression::Identifier(id) = &call.callee
        {
            matches!(
                id.name.as_str(),
                "Component" | "Directive" | "Injectable" | "Pipe"
            )
        } else {
            false
        }
    })
}

#[derive(Debug, Clone)]
pub(super) enum LitCustomElementDecorator {
    Named { local_name: String },
    Namespace { local_name: String },
}

pub(super) fn lit_custom_element_decorator(class: &Class<'_>) -> Option<LitCustomElementDecorator> {
    class.decorators.iter().find_map(|d| {
        let Expression::CallExpression(call) = &d.expression else {
            return None;
        };
        match &call.callee {
            Expression::Identifier(id) => Some(LitCustomElementDecorator::Named {
                local_name: id.name.to_string(),
            }),
            Expression::StaticMemberExpression(member)
                if member.property.name == "customElement" =>
            {
                let Expression::Identifier(object) = &member.object else {
                    return None;
                };
                Some(LitCustomElementDecorator::Namespace {
                    local_name: object.name.to_string(),
                })
            }
            _ => None,
        }
    })
}

pub fn extract_custom_elements_define(
    call: &oxc_ast::ast::CallExpression<'_>,
) -> Option<(String, String)> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    let Expression::Identifier(obj) = &member.object else {
        return None;
    };
    if obj.name != "customElements" || member.property.name != "define" {
        return None;
    }
    let tag = match call.arguments.first()? {
        Argument::StringLiteral(lit) => lit.value.to_string(),
        _ => return None,
    };
    let class_name = match call.arguments.get(1)? {
        Argument::Identifier(id) => id.name.to_string(),
        _ => return None,
    };
    Some((tag, class_name))
}

fn is_angular_signal_initializer(value: &Expression<'_>) -> bool {
    let Expression::CallExpression(call) = value else {
        return false;
    };
    match &call.callee {
        Expression::Identifier(id) => ANGULAR_SIGNAL_APIS.contains(&id.name.as_str()),
        Expression::StaticMemberExpression(member) => {
            if let Expression::Identifier(obj) = &member.object {
                ANGULAR_SIGNAL_APIS.contains(&obj.name.as_str())
                    && member.property.name == "required"
            } else {
                false
            }
        }
        _ => false,
    }
}

pub(super) struct AngularSignalQuery {
    pub type_arg: String,
    pub plural: bool,
}

pub(super) fn extract_angular_signal_query(value: &Expression<'_>) -> Option<AngularSignalQuery> {
    let Expression::CallExpression(call) = value else {
        return None;
    };
    let Expression::Identifier(id) = &call.callee else {
        return None;
    };
    let plural = match id.name.as_str() {
        "viewChild" | "contentChild" => false,
        "viewChildren" | "contentChildren" => true,
        _ => return None,
    };
    if let Some(type_args) = call.type_arguments.as_deref()
        && let Some(first) = type_args.params.first()
        && let Some(name) = extract_type_reference_name(first)
        && !is_builtin_constructor(&name)
    {
        return Some(AngularSignalQuery {
            type_arg: name,
            plural,
        });
    }
    if let Some(Argument::Identifier(arg_id)) = call.arguments.first()
        && !is_builtin_constructor(arg_id.name.as_str())
    {
        return Some(AngularSignalQuery {
            type_arg: arg_id.name.to_string(),
            plural,
        });
    }
    None
}

pub(super) fn extract_query_list_element_type(annotation: &TSTypeAnnotation<'_>) -> Option<String> {
    extract_query_list_from_type(&annotation.type_annotation)
}

fn extract_query_list_from_type(ty: &TSType<'_>) -> Option<String> {
    match ty {
        TSType::TSTypeReference(type_ref) => {
            let name = extract_type_name(&type_ref.type_name)?;
            if name != "QueryList" {
                return None;
            }
            let type_args = type_ref.type_arguments.as_deref()?;
            let first = type_args.params.first()?;
            let element = extract_type_reference_name(first)?;
            if is_builtin_constructor(&element) {
                None
            } else {
                Some(element)
            }
        }
        TSType::TSParenthesizedType(paren) => extract_query_list_from_type(&paren.type_annotation),
        TSType::TSUnionType(union) => {
            let mut found: Option<String> = None;
            for branch in &union.types {
                match branch {
                    TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => {}
                    other => {
                        if found.is_some() {
                            return None;
                        }
                        found = extract_query_list_from_type(other);
                        found.as_ref()?;
                    }
                }
            }
            found
        }
        _ => None,
    }
}

pub(super) fn has_angular_plural_query_decorator(
    decorators: &[oxc_ast::ast::Decorator<'_>],
) -> bool {
    decorators.iter().any(|decorator| {
        let Expression::CallExpression(call) = &decorator.expression else {
            return false;
        };
        let Expression::Identifier(id) = &call.callee else {
            return false;
        };
        matches!(id.name.as_str(), "ViewChildren" | "ContentChildren")
    })
}

pub fn extract_class_members(class: &Class<'_>, is_angular_class: bool) -> Vec<MemberInfo> {
    let class_name = class.id.as_ref().map(|id| id.name.as_str());
    let mut members = Vec::new();
    for element in &class.body.body {
        match element {
            ClassElement::MethodDefinition(method) => {
                if let Some(name) = method.key.static_name() {
                    let name_str = name.to_string();
                    if name_str != "constructor"
                        && !matches!(
                            method.accessibility,
                            Some(
                                TSAccessibility::Private | oxc_ast::ast::TSAccessibility::Protected
                            )
                        )
                    {
                        let is_instance_returning_static = method.r#static
                            && is_instance_returning_static_method(method, class_name);
                        let is_self_returning = !method.r#static
                            && is_self_returning_instance_method(method, class_name);
                        let decorator_names = method
                            .decorators
                            .iter()
                            .map(|d| decorator_path(&d.expression))
                            .collect();
                        members.push(MemberInfo {
                            name: name_str,
                            kind: MemberKind::ClassMethod,
                            span: method.span,
                            has_decorator: !method.decorators.is_empty(),
                            decorator_names,
                            is_instance_returning_static,
                            is_self_returning,
                        });
                    }
                }
            }
            ClassElement::PropertyDefinition(prop) => {
                if let Some(name) = prop.key.static_name()
                    && !prop.declare
                    && !matches!(
                        prop.accessibility,
                        Some(TSAccessibility::Private | oxc_ast::ast::TSAccessibility::Protected)
                    )
                {
                    let has_decorator = !prop.decorators.is_empty()
                        || (is_angular_class
                            && prop
                                .value
                                .as_ref()
                                .is_some_and(is_angular_signal_initializer));
                    let decorator_names = prop
                        .decorators
                        .iter()
                        .map(|d| decorator_path(&d.expression))
                        .collect();
                    members.push(MemberInfo {
                        name: name.to_string(),
                        kind: MemberKind::ClassProperty,
                        span: prop.span,
                        has_decorator,
                        decorator_names,
                        is_instance_returning_static: false,
                        is_self_returning: false,
                    });
                }
            }
            _ => {}
        }
    }
    members
}

fn is_instance_returning_static_method(
    method: &oxc_ast::ast::MethodDefinition<'_>,
    class_name: Option<&str>,
) -> bool {
    if returns_named_class_type(method.value.return_type.as_ref(), class_name) {
        return true;
    }
    let Some(body) = method.value.body.as_ref() else {
        return false;
    };
    body.statements
        .last()
        .is_some_and(|stmt| statement_returns_class_instance(stmt, class_name))
}

fn is_self_returning_instance_method(
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

fn returns_named_class_type(
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

fn statement_returns_class_instance(stmt: &Statement<'_>, class_name: Option<&str>) -> bool {
    let Statement::ReturnStatement(ret) = stmt else {
        return false;
    };
    let Some(expr) = ret.argument.as_ref() else {
        return false;
    };
    is_self_construction_expression(expr, class_name)
}

fn is_self_construction_expression(expr: &Expression<'_>, class_name: Option<&str>) -> bool {
    let Expression::NewExpression(new_expr) = expr else {
        return false;
    };
    match &new_expr.callee {
        Expression::ThisExpression(_) => true,
        Expression::Identifier(ident) => class_name.is_some_and(|name| ident.name.as_str() == name),
        _ => false,
    }
}

pub fn extract_super_class_name(class: &Class<'_>) -> Option<String> {
    extract_static_expression_name(class.super_class.as_ref()?)
}

#[must_use]
pub fn extract_implemented_interface_names(class: &Class<'_>) -> Vec<String> {
    class
        .implements
        .iter()
        .filter_map(|item| extract_type_name(&item.expression))
        .collect()
}

#[must_use]
pub fn extract_type_annotation_name(type_annotation: &TSTypeAnnotation<'_>) -> Option<String> {
    extract_type_reference_name(&type_annotation.type_annotation)
}

#[must_use]
pub fn extract_nested_type_bindings(
    type_annotation: &TSTypeAnnotation<'_>,
) -> Vec<(String, String)> {
    let mut bindings = Vec::new();
    collect_nested_type_bindings(&type_annotation.type_annotation, None, &mut bindings);
    bindings
}

fn collect_nested_type_bindings(
    ty: &TSType<'_>,
    prefix: Option<&str>,
    bindings: &mut Vec<(String, String)>,
) {
    match ty {
        TSType::TSTypeLiteral(type_lit) => {
            for member in &type_lit.members {
                let TSSignature::TSPropertySignature(prop) = member else {
                    continue;
                };
                let Some(property_name) = prop.key.static_name() else {
                    continue;
                };
                let path = if let Some(prefix) = prefix {
                    format!("{prefix}.{property_name}")
                } else {
                    property_name.to_string()
                };
                let Some(type_annotation) = prop.type_annotation.as_deref() else {
                    continue;
                };
                if let Some(type_name) = extract_type_annotation_name(type_annotation) {
                    bindings.push((path, type_name));
                } else {
                    collect_nested_type_bindings(
                        &type_annotation.type_annotation,
                        Some(path.as_str()),
                        bindings,
                    );
                }
            }
        }
        TSType::TSParenthesizedType(paren) => {
            collect_nested_type_bindings(&paren.type_annotation, prefix, bindings);
        }
        _ => {}
    }
}

#[must_use]
pub fn extract_class_instance_bindings<F>(
    class: &Class<'_>,
    is_named_import_from: F,
) -> Vec<(String, String)>
where
    F: Fn(&str, &str, &str) -> bool,
{
    let type_param_constraints = collect_class_type_param_constraints(class);
    let resolve = |raw: String| -> Option<String> {
        if let Some(replacement) = type_param_constraints.get(raw.as_str()) {
            return replacement.clone();
        }
        Some(raw)
    };
    let mut bindings: Vec<(String, String)> = Vec::new();
    for element in &class.body.body {
        match element {
            ClassElement::MethodDefinition(method) => {
                if matches!(method.kind, MethodDefinitionKind::Constructor) {
                    for param in &method.value.params.items {
                        let Some(accessibility) = param.accessibility else {
                            continue;
                        };
                        if matches!(accessibility, TSAccessibility::Private) {
                            continue;
                        }
                        let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                            continue;
                        };
                        let Some(type_annotation) = param.type_annotation.as_deref() else {
                            continue;
                        };
                        let Some(type_name) = extract_type_annotation_name(type_annotation) else {
                            continue;
                        };
                        let Some(resolved) = resolve(type_name) else {
                            continue;
                        };
                        bindings.push((id.name.to_string(), resolved));
                    }
                } else if matches!(method.kind, MethodDefinitionKind::Get) {
                    if matches!(method.accessibility, Some(TSAccessibility::Private)) {
                        continue;
                    }
                    let Some(name) = method.key.static_name() else {
                        continue;
                    };
                    let Some(type_annotation) = method.value.return_type.as_deref() else {
                        continue;
                    };
                    let Some(type_name) = extract_type_annotation_name(type_annotation) else {
                        continue;
                    };
                    let Some(resolved) = resolve(type_name) else {
                        continue;
                    };
                    bindings.push((name.to_string(), resolved));
                }
            }
            ClassElement::PropertyDefinition(prop) => {
                if matches!(prop.accessibility, Some(TSAccessibility::Private)) {
                    continue;
                }
                let Some(name) = prop.key.static_name() else {
                    continue;
                };
                if let Some(type_annotation) = prop.type_annotation.as_deref()
                    && let Some(type_name) = extract_type_annotation_name(type_annotation)
                {
                    if let Some(resolved) = resolve(type_name) {
                        bindings.push((name.to_string(), resolved));
                    }
                    continue;
                }
                if let Some(Expression::NewExpression(new_expr)) = &prop.value
                    && let Expression::Identifier(callee) = &new_expr.callee
                    && !is_builtin_constructor(callee.name.as_str())
                {
                    bindings.push((name.to_string(), callee.name.to_string()));
                    continue;
                }
                if let Some(Expression::CallExpression(call)) = &prop.value
                    && let Some(type_name) =
                        extract_angular_inject_target(call, &is_named_import_from)
                {
                    bindings.push((name.to_string(), type_name));
                }
            }
            _ => {}
        }
    }
    bindings
}

pub fn extract_angular_inject_target<F>(
    call: &CallExpression<'_>,
    is_named_import_from: &F,
) -> Option<String>
where
    F: Fn(&str, &str, &str) -> bool,
{
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    if !is_named_import_from(callee.name.as_str(), "@angular/core", "inject") {
        return None;
    }

    if let Some(type_arguments) = call.type_arguments.as_deref()
        && let Some(TSType::TSTypeReference(type_ref)) = type_arguments.params.first()
        && let Some(type_name) = type_name_root(&type_ref.type_name)
    {
        return Some(type_name);
    }

    let Some(Argument::Identifier(target)) = call.arguments.first() else {
        return None;
    };
    Some(target.name.to_string())
}

fn type_name_root(name: &TSTypeName<'_>) -> Option<String> {
    match name {
        TSTypeName::IdentifierReference(ident) => Some(ident.name.to_string()),
        TSTypeName::QualifiedName(qualified) => type_name_root(&qualified.left),
        TSTypeName::ThisExpression(_) => None,
    }
}

#[must_use]
pub fn collect_class_type_param_constraints(
    class: &Class<'_>,
) -> FxHashMap<String, Option<String>> {
    let mut map = FxHashMap::default();
    let Some(type_parameters) = class.type_parameters.as_deref() else {
        return map;
    };
    for param in &type_parameters.params {
        let constraint_name = param
            .constraint
            .as_ref()
            .and_then(extract_type_reference_name);
        map.insert(param.name.name.to_string(), constraint_name);
    }
    map
}

#[must_use]
pub fn extract_type_reference_name(ty: &TSType<'_>) -> Option<String> {
    match ty {
        TSType::TSTypeReference(type_ref) => extract_type_name(&type_ref.type_name),
        TSType::TSParenthesizedType(paren) => extract_type_reference_name(&paren.type_annotation),
        TSType::TSUnionType(union) => extract_nullable_union_name(union),
        _ => None,
    }
}

fn extract_nullable_union_name(union: &oxc_ast::ast::TSUnionType<'_>) -> Option<String> {
    let mut found: Option<String> = None;
    for branch in &union.types {
        match branch {
            TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => {}
            other => {
                if found.is_some() {
                    return None;
                }
                found = Some(extract_type_reference_name(other)?);
            }
        }
    }
    found
}

pub(super) fn decorator_path(expr: &Expression<'_>) -> String {
    match expr {
        Expression::Identifier(id) => id.name.to_string(),
        Expression::StaticMemberExpression(member) => {
            let object = decorator_path(&member.object);
            if object.is_empty() {
                String::new()
            } else {
                format!("{}.{}", object, member.property.name)
            }
        }
        Expression::CallExpression(call) => decorator_path(&call.callee),
        Expression::ParenthesizedExpression(paren) => decorator_path(&paren.expression),
        _ => String::new(),
    }
}

fn extract_static_expression_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            extract_static_expression_name(&member.object)?,
            member.property.name
        )),
        _ => None,
    }
}

fn extract_type_name(name: &TSTypeName<'_>) -> Option<String> {
    match name {
        TSTypeName::IdentifierReference(ident) => Some(ident.name.to_string()),
        TSTypeName::QualifiedName(name) => Some(format!(
            "{}.{}",
            extract_type_name(&name.left)?,
            name.right.name
        )),
        TSTypeName::ThisExpression(_) => None,
    }
}

pub(super) fn is_meta_url_arg(arg: &Argument<'_>) -> bool {
    if let Argument::StaticMemberExpression(member) = arg
        && member.property.name == "url"
        && matches!(member.object, Expression::MetaProperty(_))
    {
        return true;
    }
    false
}

pub(super) fn ts_import_type_qualifier_root<'a>(
    qualifier: &'a oxc_ast::ast::TSImportTypeQualifier<'a>,
) -> &'a str {
    let mut current = qualifier;
    loop {
        match current {
            oxc_ast::ast::TSImportTypeQualifier::Identifier(id) => return id.name.as_str(),
            oxc_ast::ast::TSImportTypeQualifier::QualifiedName(qn) => current = &qn.left,
        }
    }
}

pub(super) fn extract_concat_parts(
    expr: &BinaryExpression<'_>,
) -> Option<(String, Option<String>)> {
    let prefix = extract_leading_string(&expr.left)?;
    let suffix = extract_trailing_string(&expr.right);
    Some((prefix, suffix))
}

fn extract_leading_string(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::StringLiteral(lit) => Some(lit.value.to_string()),
        Expression::BinaryExpression(bin)
            if bin.operator == oxc_ast::ast::BinaryOperator::Addition =>
        {
            extract_leading_string(&bin.left)
        }
        _ => None,
    }
}

fn extract_trailing_string(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::StringLiteral(lit) => {
            let s = lit.value.to_string();
            if s.is_empty() { None } else { Some(s) }
        }
        _ => None,
    }
}

pub(super) fn regex_pattern_to_suffix(pattern: &str) -> Option<String> {
    let p = pattern.strip_prefix('^').unwrap_or(pattern);
    let p = p.strip_prefix(".*").unwrap_or(p);

    let p = p.strip_prefix("\\.")?;

    let p = p.strip_suffix('$')?;

    if let Some(base) = p.strip_suffix('?') {
        if base.chars().all(|c| c.is_ascii_alphanumeric()) && !base.is_empty() {
            let without_last = &base[..base.len() - 1];
            if without_last.is_empty() {
                return None;
            }
            return Some(format!(".{{{without_last},{base}}}"));
        }
        return None;
    }

    if let Some(inner) = p.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        let exts: Vec<&str> = inner.split('|').collect();
        if exts
            .iter()
            .all(|e| e.chars().all(|c| c.is_ascii_alphanumeric()) && !e.is_empty())
        {
            return Some(format!(".{{{}}}", exts.join(",")));
        }
        return None;
    }

    if p.chars().all(|c| c.is_ascii_alphanumeric()) && !p.is_empty() {
        return Some(format!(".{p}"));
    }

    None
}

pub(super) fn try_extract_factory_new_class(arguments: &[Argument<'_>]) -> Option<String> {
    for arg in arguments {
        let class_name = match arg {
            Argument::ArrowFunctionExpression(arrow) => {
                if arrow.expression {
                    extract_new_class_from_statement(arrow.body.statements.first()?)
                } else {
                    extract_new_class_from_return_body(&arrow.body.statements)
                }
            }
            Argument::FunctionExpression(func) => {
                extract_new_class_from_return_body(&func.body.as_ref()?.statements)
            }
            _ => None,
        };
        if let Some(name) = class_name
            && !is_builtin_constructor(&name)
        {
            return Some(name);
        }
    }
    None
}

fn extract_new_class_from_statement(stmt: &Statement<'_>) -> Option<String> {
    if let Statement::ExpressionStatement(expr_stmt) = stmt
        && let Expression::NewExpression(new_expr) = &expr_stmt.expression
        && let Expression::Identifier(callee) = &new_expr.callee
    {
        return Some(callee.name.to_string());
    }
    None
}

fn extract_new_class_from_return_body(stmts: &[Statement<'_>]) -> Option<String> {
    for stmt in stmts.iter().rev() {
        if let Statement::ReturnStatement(ret) = stmt
            && let Some(Expression::NewExpression(new_expr)) = &ret.argument
            && let Expression::Identifier(callee) = &new_expr.callee
        {
            return Some(callee.name.to_string());
        }
    }
    None
}

pub(super) fn is_builtin_constructor(name: &str) -> bool {
    matches!(
        name,
        "Array"
            | "ArrayBuffer"
            | "Blob"
            | "Boolean"
            | "DataView"
            | "Date"
            | "Error"
            | "EvalError"
            | "Event"
            | "Float32Array"
            | "Float64Array"
            | "FormData"
            | "Headers"
            | "Int8Array"
            | "Int16Array"
            | "Int32Array"
            | "Map"
            | "Number"
            | "Object"
            | "Promise"
            | "Proxy"
            | "RangeError"
            | "ReferenceError"
            | "RegExp"
            | "Request"
            | "Response"
            | "Set"
            | "SharedArrayBuffer"
            | "String"
            | "SyntaxError"
            | "TypeError"
            | "URIError"
            | "URL"
            | "URLSearchParams"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Uint16Array"
            | "Uint32Array"
            | "WeakMap"
            | "WeakRef"
            | "WeakSet"
            | "Worker"
            | "AbortController"
            | "ReadableStream"
            | "WritableStream"
            | "TransformStream"
            | "TextEncoder"
            | "TextDecoder"
            | "MutationObserver"
            | "IntersectionObserver"
            | "ResizeObserver"
            | "PerformanceObserver"
            | "MessageChannel"
            | "BroadcastChannel"
            | "WebSocket"
            | "XMLHttpRequest"
            | "EventEmitter"
            | "Buffer"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_suffix_with_caret_anchor() {
        assert_eq!(
            regex_pattern_to_suffix(r"^\.vue$"),
            Some(".vue".to_string())
        );
        assert_eq!(
            regex_pattern_to_suffix(r"^\.json$"),
            Some(".json".to_string())
        );
    }

    #[test]
    fn regex_suffix_with_dotstar_anchor() {
        assert_eq!(
            regex_pattern_to_suffix(r".*\.css$"),
            Some(".css".to_string())
        );
    }

    #[test]
    fn regex_suffix_with_both_anchors() {
        assert_eq!(
            regex_pattern_to_suffix(r"^.*\.ts$"),
            Some(".ts".to_string())
        );
    }

    #[test]
    fn regex_suffix_single_char_optional_returns_none() {
        assert_eq!(regex_pattern_to_suffix(r"\.x?$"), None);
    }

    #[test]
    fn regex_suffix_two_char_optional() {
        assert_eq!(
            regex_pattern_to_suffix(r"\.ts?$"),
            Some(".{t,ts}".to_string())
        );
    }

    #[test]
    fn regex_suffix_no_dollar_sign_returns_none() {
        assert_eq!(regex_pattern_to_suffix(r"\.vue"), None);
    }

    #[test]
    fn regex_suffix_no_escaped_dot_returns_none() {
        assert_eq!(regex_pattern_to_suffix(r"vue$"), None);
    }

    #[test]
    fn regex_suffix_empty_alternation_returns_none() {
        assert_eq!(regex_pattern_to_suffix(r"\.()$"), None);
    }

    #[test]
    fn regex_suffix_alternation_with_special_chars_returns_none() {
        assert_eq!(regex_pattern_to_suffix(r"\.(j.s|ts)$"), None);
    }

    #[test]
    fn regex_suffix_complex_wildcard_returns_none() {
        assert_eq!(regex_pattern_to_suffix(r"\..+$"), None);
        assert_eq!(regex_pattern_to_suffix(r"\.[a-z]+$"), None);
    }

    #[test]
    fn builtin_constructors_recognized() {
        assert!(is_builtin_constructor("Array"));
        assert!(is_builtin_constructor("Map"));
        assert!(is_builtin_constructor("Set"));
        assert!(is_builtin_constructor("WeakMap"));
        assert!(is_builtin_constructor("WeakSet"));
        assert!(is_builtin_constructor("Promise"));
        assert!(is_builtin_constructor("URL"));
        assert!(is_builtin_constructor("URLSearchParams"));
        assert!(is_builtin_constructor("RegExp"));
        assert!(is_builtin_constructor("Date"));
        assert!(is_builtin_constructor("Error"));
        assert!(is_builtin_constructor("TypeError"));
        assert!(is_builtin_constructor("Request"));
        assert!(is_builtin_constructor("Response"));
        assert!(is_builtin_constructor("Headers"));
        assert!(is_builtin_constructor("FormData"));
        assert!(is_builtin_constructor("Blob"));
        assert!(is_builtin_constructor("AbortController"));
        assert!(is_builtin_constructor("ReadableStream"));
        assert!(is_builtin_constructor("WritableStream"));
        assert!(is_builtin_constructor("TransformStream"));
        assert!(is_builtin_constructor("TextEncoder"));
        assert!(is_builtin_constructor("TextDecoder"));
        assert!(is_builtin_constructor("Worker"));
        assert!(is_builtin_constructor("WebSocket"));
        assert!(is_builtin_constructor("EventEmitter"));
        assert!(is_builtin_constructor("Buffer"));
        assert!(is_builtin_constructor("MutationObserver"));
        assert!(is_builtin_constructor("IntersectionObserver"));
        assert!(is_builtin_constructor("ResizeObserver"));
        assert!(is_builtin_constructor("MessageChannel"));
        assert!(is_builtin_constructor("BroadcastChannel"));
    }

    #[test]
    fn user_defined_classes_not_builtin() {
        assert!(!is_builtin_constructor("MyService"));
        assert!(!is_builtin_constructor("UserRepository"));
        assert!(!is_builtin_constructor("AppController"));
        assert!(!is_builtin_constructor("DatabaseConnection"));
        assert!(!is_builtin_constructor("Logger"));
        assert!(!is_builtin_constructor("Config"));
        assert!(!is_builtin_constructor(""));
    }

    #[test]
    fn builtin_names_are_case_sensitive() {
        assert!(!is_builtin_constructor("array"));
        assert!(!is_builtin_constructor("map"));
        assert!(!is_builtin_constructor("url"));
        assert!(!is_builtin_constructor("MAP"));
        assert!(!is_builtin_constructor("ARRAY"));
    }
}
