//! Free helpers that read the shape of an expression, a binding or a body,
//! and the regex risk and imported call site records that use them.

use super::*;

/// Split an Angular `@Component` selector string into its individual selectors.
///
/// A selector value can be a comma-separated list (`'app-foo, [appBar], .baz'`).
/// Each part is trimmed; empty parts are dropped. The raw shape of each selector
/// is preserved (element `app-foo`, attribute `[appFoo]`, class `.foo`,
/// `:not(...)` etc.) so the detector can classify them and restrict its
/// first-cut scope to all-element-selector components.
pub(super) fn split_angular_selectors(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Extract the class name from a route `loadComponent` value of the shape
/// `() => import('./x').then(m => m.FooComponent)`. Returns the member name
/// (`FooComponent`) accessed on the `.then` callback's parameter, which is the
/// lazily-loaded component class. Returns `None` for any other shape (the
/// `.then` already credits the named import elsewhere; this is the
/// entry-point-abstain capture for the Angular `unrendered-component` rule).
pub(super) fn then_callback_member_class(value: &Expression<'_>) -> Option<String> {
    let body = arrow_expression_body(value)?;
    // The body must be a `<expr>.then(<callback>)` call.
    let Expression::CallExpression(call) = body else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if member.property.name.as_str() != "then" {
        return None;
    }
    // The callback is `m => m.FooComponent` (or `({ FooComponent }) => ...`,
    // which is covered by the named-import credit elsewhere; here we only need the
    // member-access shape).
    let Some(Argument::ArrowFunctionExpression(_)) = call.arguments.first() else {
        return None;
    };
    let Some(Argument::ArrowFunctionExpression(cb)) = call.arguments.first() else {
        return None;
    };
    let cb_body = arrow_fn_expression_body(cb)?;
    let Expression::StaticMemberExpression(member) = cb_body else {
        return None;
    };
    Some(member.property.name.to_string())
}

/// The expression body of a `() => <expr>` arrow, unwrapping the optional
/// parenthesized-expression-statement form. Returns `None` for a block body that
/// is not a sole expression statement.
fn arrow_expression_body<'a, 'b>(value: &'b Expression<'a>) -> Option<&'b Expression<'a>> {
    let Expression::ArrowFunctionExpression(arrow) = value else {
        return None;
    };
    arrow_fn_expression_body(arrow)
}

/// The sole expression of an arrow function's body (expression-bodied arrow, or a
/// block body with a single `return <expr>` / `<expr>;` statement).
fn arrow_fn_expression_body<'a, 'b>(
    arrow: &'b oxc_ast::ast::ArrowFunctionExpression<'a>,
) -> Option<&'b Expression<'a>> {
    let ArrowFunctionBody::FunctionBody(body) = &arrow.body else {
        return arrow.body.as_expression();
    };
    match body.statements.first() {
        Some(Statement::ReturnStatement(ret)) => ret.argument.as_ref(),
        Some(Statement::ExpressionStatement(stmt)) => Some(&stmt.expression),
        _ => None,
    }
}

pub(super) fn static_argument_object_name(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::Identifier(ident) => Some(ident.name.to_string()),
        Argument::ThisExpression(_) => Some("this".to_string()),
        Argument::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            static_member_object_name(&member.object)?,
            member.property.name
        )),
        _ => None,
    }
}

pub(super) fn assignment_target_identifier_name<'b>(
    target: &'b AssignmentTarget<'_>,
) -> Option<&'b str> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(ident) => Some(ident.name.as_str()),
        AssignmentTarget::TSAsExpression(ts_as) => expression_identifier_name(&ts_as.expression),
        AssignmentTarget::TSSatisfiesExpression(ts_sat) => {
            expression_identifier_name(&ts_sat.expression)
        }
        AssignmentTarget::TSNonNullExpression(ts_non_null) => {
            expression_identifier_name(&ts_non_null.expression)
        }
        AssignmentTarget::TSTypeAssertion(ts_assertion) => {
            expression_identifier_name(&ts_assertion.expression)
        }
        _ => None,
    }
}

fn expression_identifier_name<'b>(expr: &'b Expression<'_>) -> Option<&'b str> {
    match expr {
        Expression::Identifier(ident) => Some(ident.name.as_str()),
        Expression::ParenthesizedExpression(paren) => expression_identifier_name(&paren.expression),
        Expression::TSAsExpression(ts_as) => expression_identifier_name(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => expression_identifier_name(&ts_sat.expression),
        Expression::TSNonNullExpression(ts_non_null) => {
            expression_identifier_name(&ts_non_null.expression)
        }
        Expression::TSTypeAssertion(ts_assertion) => {
            expression_identifier_name(&ts_assertion.expression)
        }
        _ => None,
    }
}

pub(super) fn assignment_target_member_object_name<'b>(
    target: &'b AssignmentTarget<'_>,
) -> Option<&'b str> {
    match target {
        AssignmentTarget::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) => Some(object.name.as_str()),
            _ => None,
        },
        AssignmentTarget::ComputedMemberExpression(member) => match &member.object {
            Expression::Identifier(object) => Some(object.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn is_literal_string_allowlist_expr(expr: &Expression<'_>) -> bool {
    match unwrap_static_expr(expr) {
        Expression::ArrayExpression(array) => is_string_literal_array(array),
        Expression::NewExpression(new_expr) => {
            let Expression::Identifier(callee) = &new_expr.callee else {
                return false;
            };
            if callee.name != "Set" {
                return false;
            }
            let Some(Argument::ArrayExpression(array)) = new_expr.arguments.first() else {
                return false;
            };
            is_string_literal_array(array)
        }
        _ => false,
    }
}

fn is_string_literal_array(array: &ArrayExpression<'_>) -> bool {
    array
        .elements
        .iter()
        .all(|element| matches!(element, ArrayExpressionElement::StringLiteral(_)))
}

/// `null`, `undefined` or `void <expr>`: a value that holds no instance.
pub(super) fn is_nullish_value(expr: &Expression<'_>) -> bool {
    match unwrap_static_expr(expr) {
        Expression::NullLiteral(_) => true,
        Expression::Identifier(id) => id.name == "undefined",
        Expression::UnaryExpression(unary) => unary.operator == UnaryOperator::Void,
        _ => false,
    }
}

/// The class name of a `new X()` expression, when `X` is a plain identifier
/// and not a builtin constructor.
pub(super) fn constructed_class_name<'e>(expr: &Expression<'e>) -> Option<&'e str> {
    let Expression::NewExpression(new_expr) = expr else {
        return None;
    };
    let Expression::Identifier(callee) = &new_expr.callee else {
        return None;
    };
    let name = callee.name.as_str();
    (!crate::visitor::helpers::is_builtin_constructor(name)).then_some(name)
}

pub(super) fn asserted_receiver_type<'ast, 'borrow>(
    expr: &'borrow Expression<'ast>,
) -> Option<&'borrow TSType<'ast>> {
    match expr {
        Expression::ParenthesizedExpression(parenthesized) => {
            asserted_receiver_type(&parenthesized.expression)
        }
        Expression::TSAsExpression(assertion) => Some(&assertion.type_annotation),
        Expression::TSTypeAssertion(assertion) => Some(&assertion.type_annotation),
        _ => None,
    }
}

pub(super) fn asserted_type_root_name<'borrow>(ty: &'borrow TSType<'_>) -> Option<&'borrow str> {
    match ty {
        TSType::TSTypeReference(type_ref) => match &type_ref.type_name {
            TSTypeName::IdentifierReference(ident) => Some(ident.name.as_str()),
            TSTypeName::QualifiedName(qualified) => type_name_root_str(&qualified.left),
            TSTypeName::ThisExpression(_) => None,
        },
        TSType::TSParenthesizedType(parenthesized) => {
            asserted_type_root_name(&parenthesized.type_annotation)
        }
        TSType::TSUnionType(union) => {
            let mut root = None;
            for branch in &union.types {
                if matches!(
                    branch,
                    TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_)
                ) {
                    continue;
                }
                if root.is_some() {
                    return None;
                }
                root = Some(asserted_type_root_name(branch)?);
            }
            root
        }
        _ => None,
    }
}

fn type_name_root_str<'borrow>(name: &'borrow TSTypeName<'_>) -> Option<&'borrow str> {
    match name {
        TSTypeName::IdentifierReference(ident) => Some(ident.name.as_str()),
        TSTypeName::QualifiedName(qualified) => type_name_root_str(&qualified.left),
        TSTypeName::ThisExpression(_) => None,
    }
}

pub(super) fn is_string_coercion_sibling(expr: &Expression<'_>) -> bool {
    matches!(
        unwrap_static_expr(expr),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
    )
}

/// The class name in a `new Class()` expression, or `None` for a non-`new`
/// expression, a non-identifier callee, or a builtin constructor.
pub(super) fn new_expression_class_name(expr: &Expression<'_>) -> Option<String> {
    let Expression::NewExpression(new_expr) = expr else {
        return None;
    };
    let Expression::Identifier(callee) = &new_expr.callee else {
        return None;
    };
    if crate::visitor::helpers::is_builtin_constructor(callee.name.as_str()) {
        return None;
    }
    Some(callee.name.to_string())
}

/// The callee identifier name of the (optionally awaited) call an iteration map
/// callback returns: `async (n) => createDb(n)`, `(n) => await createDb(n)`, or
/// a single-`return` block body. Member-expression callees and multi-statement
/// bodies (including conditional early returns) yield `None`, keeping the
/// Promise.all element inference conservative. See issue #1793.
pub(super) fn map_callback_returned_call_name(arg: &Argument<'_>) -> Option<String> {
    let returned = match arg {
        Argument::ArrowFunctionExpression(arrow) => match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => single_return_expr(body)?,
            body => body.as_expression()?,
        },
        Argument::FunctionExpression(function) => single_return_expr(function.body.as_deref()?)?,
        _ => return None,
    };
    let Expression::CallExpression(call) = unwrap_await_paren_expr(returned) else {
        return None;
    };
    match &call.callee {
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// The single returned expression of a `{ return <expr>; }` block body. Any
/// other statement count yields `None`, so a multi-statement or conditional
/// callback body does not credit an element class. See issue #1793.
fn single_return_expr<'a, 'b>(body: &'b FunctionBody<'a>) -> Option<&'b Expression<'a>> {
    if body.statements.len() != 1 {
        return None;
    }
    let Statement::ReturnStatement(ret) = body.statements.first()? else {
        return None;
    };
    ret.argument.as_ref()
}

/// Recursively unwrap `await` and parenthesized wrappers to the inner
/// expression. Used by the Promise.all element inference. See issue #1793.
pub(super) fn unwrap_await_paren_expr<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    match expr {
        Expression::AwaitExpression(await_expr) => unwrap_await_paren_expr(&await_expr.argument),
        Expression::ParenthesizedExpression(paren) => unwrap_await_paren_expr(&paren.expression),
        other => other,
    }
}

/// Recursively unwrap parenthesized expressions to reach the inner expression.
pub(super) fn unwrap_parens<'a, 'b>(mut expr: &'b Expression<'a>) -> &'b Expression<'a> {
    while let Expression::ParenthesizedExpression(paren) = expr {
        expr = &paren.expression;
    }
    expr
}

/// Root reference of a static callee without optional or computed members.
fn imported_callee_root<'a>(expression: &'a Expression<'a>) -> Option<&'a IdentifierReference<'a>> {
    match unwrap_parens(expression) {
        Expression::Identifier(identifier) => Some(identifier),
        Expression::StaticMemberExpression(member) if !member.optional => {
            imported_callee_root(&member.object)
        }
        _ => None,
    }
}

pub(super) fn source_returning_helper(
    params: &FormalParameters<'_>,
    expr: &Expression<'_>,
) -> Option<crate::visitor::SourceReturningHelper> {
    let param_names = params
        .items
        .iter()
        .map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut paths = Vec::new();
    for source_path in source_path_candidates(expr) {
        for (arg_index, param_name) in param_names.iter().enumerate() {
            let Some(param_name) = param_name else {
                continue;
            };
            if source_path == *param_name {
                paths.push(crate::visitor::SourceReturnPath {
                    arg_index,
                    suffixes: vec![String::new()],
                });
                break;
            }
            let Some(suffix) = source_path.strip_prefix(&format!("{param_name}.")) else {
                continue;
            };
            paths.push(crate::visitor::SourceReturnPath {
                arg_index,
                suffixes: vec![suffix.to_string()],
            });
            break;
        }
    }
    if paths.is_empty() {
        None
    } else {
        Some(crate::visitor::SourceReturningHelper { paths })
    }
}

pub(super) fn extract_function_body_final_return_expr<'a, 'b>(
    body: &'b oxc_ast::ast::FunctionBody<'a>,
) -> Option<&'b Expression<'a>> {
    let Statement::ReturnStatement(ret) = body.statements.last()? else {
        return None;
    };
    ret.argument.as_ref()
}

pub(super) fn extract_arrow_return_expr<'a, 'b>(
    arrow: &'b oxc_ast::ast::ArrowFunctionExpression<'a>,
) -> Option<&'b Expression<'a>> {
    let expression = match &arrow.body {
        ArrowFunctionBody::FunctionBody(body) => {
            return extract_function_body_final_return_expr(body);
        }
        body => body.as_expression()?,
    };
    if let Expression::ParenthesizedExpression(paren) = expression {
        return Some(&paren.expression);
    }
    Some(expression)
}

pub(super) fn unwrap_paren_expr<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(paren) => &paren.expression,
        other => other,
    }
}

pub(super) fn apply_source_return_path(expr: &Expression<'_>, suffixes: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if suffixes.iter().any(String::is_empty) {
        for candidate in source_path_candidates(expr) {
            push_unique_string(&mut out, candidate);
        }
    }
    let Some(base) = flatten_member_path(expr) else {
        return out;
    };
    for suffix in suffixes {
        if suffix.is_empty() {
            push_unique_string(&mut out, base.clone());
        } else {
            push_unique_string(&mut out, format!("{base}.{suffix}"));
        }
    }
    out
}

pub(super) fn push_ident(name: &str, out: &mut Vec<String>) {
    if !out.iter().any(|n| n == name) {
        out.push(name.to_string());
    }
}

/// The source path for a DESTRUCTURE binding (`const { id } = req.query`): the
/// FULL flattened init path (`req.query`), since the destructured keys are the
/// leaves. A bare-identifier init (`const { id } = req`) yields `req`.
pub(super) fn destructure_source_path(expr: &Expression<'_>) -> Option<String> {
    flatten_member_path(expr)
}

pub(super) fn subclass_receiver_reference<'a, 'b>(
    expression: &'b Expression<'a>,
) -> Option<(&'b IdentifierReference<'a>, Vec<String>)> {
    match unwrap_static_expr(expression) {
        Expression::Identifier(reference) => Some((reference, Vec::new())),
        Expression::NewExpression(new) => {
            let Expression::Identifier(reference) = unwrap_static_expr(&new.callee) else {
                return None;
            };
            Some((reference, Vec::new()))
        }
        Expression::StaticMemberExpression(member) => {
            let (reference, mut path) = subclass_receiver_reference(&member.object)?;
            path.push(member.property.name.to_string());
            Some((reference, path))
        }
        Expression::ComputedMemberExpression(member) => {
            let name = member.static_property_name()?;
            let (reference, mut path) = subclass_receiver_reference(&member.object)?;
            path.push(name.text().into_owned());
            Some((reference, path))
        }
        _ => None,
    }
}

pub(super) fn unwrap_route_loader_data_expr<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::TSAsExpression(as_expr) => unwrap_route_loader_data_expr(&as_expr.expression),
        Expression::TSSatisfiesExpression(sat) => unwrap_route_loader_data_expr(&sat.expression),
        Expression::ParenthesizedExpression(paren) => {
            unwrap_route_loader_data_expr(&paren.expression)
        }
        _ => expr,
    }
}

pub(super) fn is_import_meta_env_object(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::StaticMemberExpression(member)
            if member.property.name == "env"
                && matches!(&member.object, Expression::ImportMeta(_))
    )
}

pub(super) fn for_of_binding_name(left: &ForStatementLeft<'_>) -> Option<String> {
    match left {
        ForStatementLeft::VariableDeclaration(decl) => {
            let declarator = decl.declarations.first()?;
            binding_pattern_value_name(&declarator.id)
        }
        ForStatementLeft::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn binding_pattern_value_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
        BindingPattern::ArrayPattern(array) => array.elements.iter().rev().find_map(|element| {
            let Some(pattern) = element else {
                return None;
            };
            let BindingPattern::BindingIdentifier(id) = pattern else {
                return None;
            };
            Some(id.name.to_string())
        }),
        _ => None,
    }
}

pub(super) fn object_values_or_entries_argument_name(expr: &Expression<'_>) -> Option<String> {
    let Expression::CallExpression(call) = expr else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    let Expression::Identifier(object) = &member.object else {
        return None;
    };
    if object.name != "Object" || !matches!(member.property.name.as_str(), "values" | "entries") {
        return None;
    }
    let Some(Argument::Identifier(arg)) = call.arguments.first() else {
        return None;
    };
    Some(arg.name.to_string())
}

/// Collect `(local_name, class_name)` pairs from an `instanceof` guard expression.
///
/// Recurses through `&&`-chained conditions so `a instanceof A && b instanceof B`
/// yields both pairs. Only simple identifier left-hand sides (`x instanceof Cls`)
/// are collected; complex left-hand expressions are skipped conservatively.
pub(super) fn collect_instanceof_narrowings<'a>(
    expr: &'a Expression<'a>,
    out: &mut Vec<(String, String, Span, Span)>,
) {
    match expr {
        Expression::BinaryExpression(bin) if bin.operator == BinaryOperator::Instanceof => {
            if let Expression::Identifier(left) = &bin.left
                && let Expression::Identifier(right) = &bin.right
            {
                out.push((
                    left.name.to_string(),
                    right.name.to_string(),
                    left.span,
                    right.span,
                ));
            }
        }
        Expression::LogicalExpression(logical) if logical.operator == LogicalOperator::And => {
            collect_instanceof_narrowings(&logical.left, out);
            collect_instanceof_narrowings(&logical.right, out);
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_instanceof_narrowings(&paren.expression, out);
        }
        _ => {}
    }
}

impl ModuleInfoExtractor {
    pub(super) fn risky_regex_fragment_for_expr(&self, expr: &Expression<'_>) -> Option<String> {
        match unwrap_static_expr(expr) {
            Expression::RegExpLiteral(lit) => risky_redos_fragment(&lit.regex.pattern.text),
            Expression::NewExpression(new_expr) => Self::risky_regex_fragment_for_new(new_expr),
            Expression::CallExpression(call) => Self::risky_regex_fragment_for_call(call),
            Expression::Identifier(ident) => self
                .risky_regex_binding(ident.name.as_str())
                .map(ToString::to_string),
            _ => None,
        }
    }

    fn risky_regex_fragment_for_new(expr: &oxc_ast::ast::NewExpression<'_>) -> Option<String> {
        let Expression::Identifier(callee) = &expr.callee else {
            return None;
        };
        if callee.name != "RegExp" {
            return None;
        }
        let pattern = expr
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(static_string_literal_value)?;
        risky_redos_fragment(&pattern)
    }

    fn risky_regex_fragment_for_call(expr: &CallExpression<'_>) -> Option<String> {
        let Expression::Identifier(callee) = &expr.callee else {
            return None;
        };
        if callee.name != "RegExp" {
            return None;
        }
        let pattern = expr
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(static_string_literal_value)?;
        risky_redos_fragment(&pattern)
    }

    pub(super) fn redos_regex_application<'b, 'c>(
        &self,
        expr: &'b CallExpression<'c>,
    ) -> Option<(&'b Expression<'c>, String)> {
        let Expression::StaticMemberExpression(member) = &expr.callee else {
            return None;
        };
        let method = member.property.name.as_str();
        if matches!(method, "test" | "exec") {
            let input = expr.arguments.first().and_then(Argument::as_expression)?;
            let pattern = self.risky_regex_fragment_for_expr(&member.object)?;
            return Some((input, pattern));
        }
        if matches!(
            method,
            "match" | "search" | "replace" | "replaceAll" | "split"
        ) {
            let pattern = expr
                .arguments
                .first()
                .and_then(Argument::as_expression)
                .and_then(|arg| self.risky_regex_fragment_for_expr(arg))?;
            return Some((&member.object, pattern));
        }
        None
    }

    /// Capture each static call and defer lexical import admission to semantics.
    pub(super) fn record_imported_call_site(&mut self, call: &CallExpression<'_>) {
        if call.optional || self.runtime_import_locals.is_empty() {
            return;
        }
        let Some(root) = imported_callee_root(&call.callee) else {
            return;
        };
        if !self.runtime_import_locals.contains(root.name.as_str()) {
            return;
        }
        let Some(path) = flatten_callee_path(&call.callee) else {
            return;
        };
        let member_path = path.split_once('.').map_or("", |(_, rest)| rest).to_owned();
        let first_argument = call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(static_string_literal_value);
        self.pending_imported_call_sites.push((
            root.span,
            ImportedCallSite {
                local_name: root.name.to_string(),
                member_path,
                first_argument,
                span_start: call.span.start,
            },
        ));
    }
}

pub(super) fn iteration_source_reference(expression: &Expression<'_>) -> Option<Span> {
    match unwrap_static_expr(expression) {
        Expression::Identifier(reference) => Some(reference.span),
        Expression::StaticMemberExpression(member) => iteration_source_reference(&member.object),
        Expression::CallExpression(call) => iteration_source_reference(&call.callee),
        _ => None,
    }
}
