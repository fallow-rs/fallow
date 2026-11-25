//! Free helpers that read `import()` calls, loader callbacks, `.then()`
//! callbacks and destructured names from an expression.

use super::*;

/// The statically named keys of a destructuring pattern, or `None` when the pattern
/// can expose properties it does not name: a rest element captures every remaining
/// property, and a computed key names one that cannot be read from source.
///
/// Contrast `extract_destructured_names`, which silently drops a computed key. Here a
/// single unnameable key makes the WHOLE pattern opaque, because a caller crediting
/// class members off it must abstain rather than credit only the keys it can see.
///
/// A nested pattern (`{ a: { b } }`) yields `a` only. `b` belongs to whatever type
/// `a` has, not to the factory's class, and crediting it would credit a same-named
/// member of an unrelated class.
pub(super) fn destructured_factory_keys(obj_pat: &ObjectPattern<'_>) -> Option<Vec<String>> {
    if obj_pat.rest.is_some() {
        return None;
    }
    obj_pat
        .properties
        .iter()
        .map(|prop| prop.key.static_name().map(|name| name.text().into_owned()))
        .collect()
}

/// The statically named keys of a destructuring pattern, SKIPPING any it cannot name.
///
/// Deliberately not `destructured_factory_keys`: this one drops a computed key and
/// keeps the rest, which is what callers tracking local bindings want. A caller that
/// must know the pattern could expose an unnamed property has to abstain instead, and
/// wants that function.
pub(super) fn extract_destructured_names(obj_pat: &ObjectPattern<'_>) -> Vec<String> {
    if obj_pat.rest.is_some() {
        return Vec::new();
    }
    obj_pat
        .properties
        .iter()
        .filter_map(|prop| prop.key.static_name().map(|n| n.text().into_owned()))
        .collect()
}

pub(super) fn try_extract_property_callback_import<'a, 'b>(
    prop: &'b ObjectProperty<'a>,
) -> Option<(&'b ImportExpression<'a>, Vec<String>)> {
    let property_name = prop.key.static_name()?;
    if !matches!(
        &*property_name.text(),
        "component" | "loadChildren" | "loadComponent"
    ) {
        return None;
    }

    let import_expr = extract_import_from_callable(&prop.value)?;
    let mut sources = Vec::new();
    collect_static_import_specifiers(&import_expr.source, &mut sources);
    if sources.is_empty() {
        return None;
    }
    Some((import_expr, sources))
}

/// The element list of a `Promise.all([...])` initializer, seen through
/// `await` and parentheses. Each element keeps its array index, so a caller
/// can pair it with the matching element of an array destructuring pattern.
/// Returns `None` when the array has a spread element, because a spread
/// moves every later index. `Promise.allSettled` and `Promise.race` are not
/// matched: their results do not hold the module objects at each index.
#[must_use]
pub(super) fn extract_promise_all_elements<'a, 'b>(
    expr: &'b Expression<'a>,
) -> Option<&'b [ArrayExpressionElement<'a>]> {
    let mut inner = expr;
    loop {
        match inner {
            Expression::AwaitExpression(await_expr) => inner = &await_expr.argument,
            Expression::ParenthesizedExpression(paren) => inner = &paren.expression,
            _ => break,
        }
    }
    let Expression::CallExpression(call) = inner else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if member.property.name != "all"
        || !matches!(&member.object, Expression::Identifier(object) if object.name == "Promise")
        || call.arguments.len() != 1
    {
        return None;
    }
    let Some(Expression::ArrayExpression(array)) = call.arguments[0].as_expression() else {
        return None;
    };
    if array
        .elements
        .iter()
        .any(|element| matches!(element, ArrayExpressionElement::SpreadElement(_)))
    {
        return None;
    }
    Some(&array.elements)
}

#[must_use]
/// Recursively unwrap an expression until it reaches an import expression.
pub(super) fn extract_import_expression<'a, 'b>(
    expr: &'b Expression<'a>,
) -> Option<&'b ImportExpression<'a>> {
    match expr {
        Expression::AwaitExpression(await_expr) => extract_import_expression(&await_expr.argument),
        Expression::ImportExpression(imp) => Some(imp),
        Expression::ParenthesizedExpression(paren) => extract_import_expression(&paren.expression),
        _ => None,
    }
}

/// Whether `expr` is an `import()` without `await`, whose value is the
/// promise of the namespace and not the namespace itself.
pub(super) fn is_unawaited_import(expr: &Expression<'_>) -> bool {
    matches!(expr.without_parentheses(), Expression::ImportExpression(_))
}

pub(super) fn try_extract_arrow_wrapped_import<'a, 'b>(
    arguments: &'b [Argument<'a>],
) -> Option<(&'b ImportExpression<'a>, Vec<String>)> {
    let expr = arguments.first()?.as_expression()?;
    let import_expr = extract_import_from_callable(expr)?;
    let mut sources = Vec::new();
    collect_static_import_specifiers(&import_expr.source, &mut sources);
    if !sources.is_empty() {
        return Some((import_expr, sources));
    }
    None
}

#[must_use]
/// Extract an import expression from a return statement body.
fn extract_import_from_return_body<'a, 'b>(
    stmts: &'b [Statement<'a>],
) -> Option<&'b ImportExpression<'a>> {
    for stmt in stmts.iter().rev() {
        if let Statement::ReturnStatement(ret) = stmt
            && let Some(argument) = &ret.argument
            && let Some(imp) = extract_import_expression(argument)
        {
            return Some(imp);
        }
    }
    None
}

/// Collect each `import()` that a local loader returns. The search includes
/// nested blocks, such as `switch` cases and `if` branches, but not nested
/// functions, because their `return` statements belong to them.
pub(super) fn collect_loader_imports<'a, 'b>(
    expr: &'b Expression<'a>,
) -> Vec<&'b ImportExpression<'a>> {
    let mut imports = Vec::new();
    match expr {
        Expression::ArrowFunctionExpression(arrow) => match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => {
                collect_returned_imports(&body.statements, &mut imports);
            }
            body => imports.extend(body.as_expression().and_then(extract_import_expression)),
        },
        Expression::FunctionExpression(func) => {
            if let Some(body) = &func.body {
                collect_returned_imports(&body.statements, &mut imports);
            }
        }
        _ => {}
    }
    imports
}

pub(super) fn collect_returned_imports<'a, 'b>(
    stmts: &'b [Statement<'a>],
    imports: &mut Vec<&'b ImportExpression<'a>>,
) {
    for stmt in stmts {
        collect_returned_imports_from(stmt, imports);
    }
}

fn collect_returned_imports_from<'a, 'b>(
    stmt: &'b Statement<'a>,
    imports: &mut Vec<&'b ImportExpression<'a>>,
) {
    match stmt {
        Statement::ReturnStatement(ret) => {
            imports.extend(ret.argument.as_ref().and_then(extract_import_expression));
        }
        Statement::BlockStatement(block) => collect_returned_imports(&block.body, imports),
        Statement::IfStatement(stmt) => {
            collect_returned_imports_from(&stmt.consequent, imports);
            if let Some(alternate) = &stmt.alternate {
                collect_returned_imports_from(alternate, imports);
            }
        }
        Statement::SwitchStatement(stmt) => {
            for case in &stmt.cases {
                collect_returned_imports(&case.consequent, imports);
            }
        }
        Statement::TryStatement(stmt) => {
            collect_returned_imports(&stmt.block.body, imports);
            if let Some(handler) = &stmt.handler {
                collect_returned_imports(&handler.body.body, imports);
            }
            if let Some(finalizer) = &stmt.finalizer {
                collect_returned_imports(&finalizer.body, imports);
            }
        }
        Statement::LabeledStatement(stmt) => collect_returned_imports_from(&stmt.body, imports),
        Statement::ForStatement(stmt) => collect_returned_imports_from(&stmt.body, imports),
        Statement::ForInStatement(stmt) => collect_returned_imports_from(&stmt.body, imports),
        Statement::ForOfStatement(stmt) => collect_returned_imports_from(&stmt.body, imports),
        Statement::WhileStatement(stmt) => collect_returned_imports_from(&stmt.body, imports),
        Statement::DoWhileStatement(stmt) => collect_returned_imports_from(&stmt.body, imports),
        _ => {}
    }
}

#[must_use]
/// Extract an import expression from a callable expression body.
pub fn extract_import_from_callable<'a, 'b>(
    expr: &'b Expression<'a>,
) -> Option<&'b ImportExpression<'a>> {
    match expr {
        Expression::ArrowFunctionExpression(arrow) => match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => {
                extract_import_from_return_body(&body.statements)
            }
            body => extract_import_expression(body.as_expression()?),
        },
        Expression::FunctionExpression(func) => {
            let body = func.body.as_ref()?;
            extract_import_from_return_body(&body.statements)
        }
        _ => None,
    }
}

pub(super) struct ImportThenCallback {
    pub(super) sources: Vec<String>,
    pub(super) import_span: oxc_span::Span,
    pub(super) destructured_names: Vec<String>,
    pub(super) local_name: Option<String>,
    pub(super) callback_start: u32,
}

pub(super) fn try_extract_import_then_callback(
    expr: &CallExpression<'_>,
) -> Option<ImportThenCallback> {
    let Expression::StaticMemberExpression(member) = &expr.callee else {
        return None;
    };
    if member.property.name != "then" {
        return None;
    }

    let Expression::ImportExpression(import_expr) = &member.object else {
        return None;
    };
    let mut sources = Vec::new();
    collect_static_import_specifiers(&import_expr.source, &mut sources);
    if sources.is_empty() {
        return None;
    }
    let import_span = import_expr.span;

    match expr.arguments.first()? {
        Argument::ArrowFunctionExpression(arrow) => {
            arrow_then_callback(arrow, sources, import_span)
        }
        Argument::FunctionExpression(func) => {
            let param = func.params.items.first()?;
            then_callback_from_pattern(&param.pattern, sources, import_span, func.span.start)
        }
        _ => None,
    }
}

/// Build an `ImportThenCallback` from a `.then()` arrow callback, handling the
/// expression-body member-access shape before falling back to the bare param.
fn arrow_then_callback(
    arrow: &oxc_ast::ast::ArrowFunctionExpression<'_>,
    sources: Vec<String>,
    import_span: Span,
) -> Option<ImportThenCallback> {
    let param = arrow.params.items.first()?;
    if let BindingPattern::BindingIdentifier(id) = &param.pattern {
        let param_name = id.name.to_string();
        if let Some(body) = arrow.body.as_expression()
            && let Some(names) = extract_member_names_from_expr(body, &param_name)
        {
            return Some(ImportThenCallback {
                sources,
                import_span,
                destructured_names: names,
                local_name: None,
                callback_start: arrow.span.start,
            });
        }
        return Some(ImportThenCallback {
            sources,
            import_span,
            destructured_names: Vec::new(),
            local_name: Some(param_name),
            callback_start: arrow.span.start,
        });
    }
    then_callback_from_pattern(&param.pattern, sources, import_span, arrow.span.start)
}

/// Build an `ImportThenCallback` from a callback param pattern: object pattern
/// yields destructured names, a bare identifier yields a namespace local.
fn then_callback_from_pattern(
    pattern: &BindingPattern<'_>,
    sources: Vec<String>,
    import_span: Span,
    callback_start: u32,
) -> Option<ImportThenCallback> {
    match pattern {
        BindingPattern::ObjectPattern(obj_pat) => Some(ImportThenCallback {
            sources,
            import_span,
            destructured_names: extract_destructured_names(obj_pat),
            local_name: None,
            callback_start,
        }),
        BindingPattern::BindingIdentifier(id) => Some(ImportThenCallback {
            sources,
            import_span,
            destructured_names: Vec::new(),
            local_name: Some(id.name.to_string()),
            callback_start,
        }),
        _ => None,
    }
}

fn extract_member_names_from_expr(expr: &Expression<'_>, param_name: &str) -> Option<Vec<String>> {
    match expr {
        Expression::StaticMemberExpression(member) => {
            if let Expression::Identifier(obj) = &member.object
                && obj.name == param_name
            {
                Some(vec![member.property.name.to_string()])
            } else {
                None
            }
        }
        Expression::ObjectExpression(obj) => extract_member_names_from_object(obj, param_name),
        Expression::ParenthesizedExpression(paren) => {
            extract_member_names_from_expr(&paren.expression, param_name)
        }
        _ => None,
    }
}

fn extract_member_names_from_object(
    obj: &oxc_ast::ast::ObjectExpression<'_>,
    param_name: &str,
) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop
            && let Expression::StaticMemberExpression(member) = &p.value
            && let Expression::Identifier(obj) = &member.object
            && obj.name == param_name
        {
            names.push(member.property.name.to_string());
        }
    }
    if names.is_empty() { None } else { Some(names) }
}
