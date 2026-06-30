//! Route and procedure callback helpers for security source extraction.

#[allow(clippy::wildcard_imports, reason = "many route helper AST types used")]
use oxc_ast::ast::*;

use super::unwrap_parens;

pub(super) fn is_http_route_handler_name(name: &str) -> bool {
    matches!(
        name,
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "HEAD"
    )
}

pub(super) fn is_route_registration_method(method: &str) -> bool {
    matches!(
        method,
        "all" | "delete" | "get" | "head" | "options" | "patch" | "post" | "put" | "use"
    )
}

pub(super) fn is_trpc_procedure_method(method: &str) -> bool {
    matches!(method, "query" | "mutation" | "subscription")
}

pub(super) fn is_trpc_procedure_callee(expr: &Expression<'_>, method: &str) -> bool {
    let Expression::StaticMemberExpression(member) = unwrap_parens(expr) else {
        return false;
    };
    member.property.name == method && trpc_chain_has_procedure(&member.object)
}

fn trpc_chain_has_procedure(expr: &Expression<'_>) -> bool {
    match unwrap_parens(expr) {
        Expression::Identifier(ident) => ident.name.to_ascii_lowercase().ends_with("procedure"),
        Expression::StaticMemberExpression(member) => {
            member.property.name == "procedure" || trpc_chain_has_procedure(&member.object)
        }
        Expression::CallExpression(call) => trpc_chain_has_procedure(&call.callee),
        _ => false,
    }
}

pub(super) fn is_framework_route_receiver_path(callee_path: &str, method: &str) -> bool {
    let Some(receiver_path) = callee_path.strip_suffix(&format!(".{method}")) else {
        return false;
    };
    let Some(receiver) = receiver_path.rsplit('.').next() else {
        return false;
    };
    let receiver = receiver.to_ascii_lowercase();
    matches!(
        receiver.as_str(),
        "app" | "router" | "route" | "routes" | "server" | "fastify"
    ) || receiver.ends_with("app")
        || receiver.ends_with("router")
        || receiver.ends_with("routes")
        || receiver.ends_with("server")
}

pub(in crate::visitor) fn function_body_has_use_server(body: Option<&FunctionBody<'_>>) -> bool {
    body.is_some_and(|body| {
        body.directives
            .iter()
            .any(|directive| directive.directive.as_str() == "use server")
    })
}

pub(super) fn callback_params<'a>(arg: &'a Argument<'a>) -> Option<&'a FormalParameters<'a>> {
    match arg {
        Argument::ArrowFunctionExpression(expr) => Some(&expr.params),
        Argument::FunctionExpression(expr) => Some(&expr.params),
        _ => arg.as_expression().and_then(|expr| match expr {
            Expression::ArrowFunctionExpression(expr) => Some(&*expr.params),
            Expression::FunctionExpression(expr) => Some(&*expr.params),
            _ => None,
        }),
    }
}

pub(super) fn function_like_params<'a>(
    expr: &'a Expression<'a>,
) -> Option<&'a FormalParameters<'a>> {
    match unwrap_parens(expr) {
        Expression::ArrowFunctionExpression(expr) => Some(&expr.params),
        Expression::FunctionExpression(expr) => Some(&expr.params),
        _ => None,
    }
}

pub(super) fn last_callback_params<'a>(
    args: &'a [Argument<'a>],
) -> Option<&'a FormalParameters<'a>> {
    args.iter().rev().find_map(callback_params)
}

pub(super) fn route_callback_params<'a>(
    args: &'a [Argument<'a>],
    method: &str,
) -> Option<&'a FormalParameters<'a>> {
    if method == "use" {
        return last_callback_params(args);
    }
    args.iter().skip(1).find_map(callback_params)
}
