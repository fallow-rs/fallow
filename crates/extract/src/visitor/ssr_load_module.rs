//! Static `ssrLoadModule` calls of the Vite dev server.

use oxc_ast::ast::{CallExpression, Expression};
use oxc_span::Span;

use crate::js_text::JsText;

/// Vite dev-server method that loads a module by specifier for SSR.
const SSR_LOAD_MODULE_METHOD: &str = "ssrLoadModule";

#[must_use]
/// Match `<receiver>.ssrLoadModule('<literal>')`, with an optional `await` or
/// parentheses around the call, and return the call span and the specifier.
///
/// The Vite dev server loads the module like a dynamic `import()`. A leading
/// `/` is relative to the Vite root, not to the file system. A computed
/// argument is not a static edge, so it returns `None`.
pub fn ssr_load_module_call(expr: &Expression<'_>) -> Option<(Span, String)> {
    let call = match expr.get_inner_expression() {
        Expression::AwaitExpression(await_expr) => {
            match await_expr.argument.get_inner_expression() {
                Expression::CallExpression(call) => call,
                _ => return None,
            }
        }
        Expression::CallExpression(call) => call,
        _ => return None,
    };
    ssr_load_module_source(call).map(|source| (call.span, source))
}

#[must_use]
/// Return the literal specifier of a `<receiver>.ssrLoadModule('<literal>')` call.
pub fn ssr_load_module_source(call: &CallExpression<'_>) -> Option<String> {
    let Expression::StaticMemberExpression(member) = call.callee.without_parentheses() else {
        return None;
    };
    if member.property.name != SSR_LOAD_MODULE_METHOD {
        return None;
    }
    let source = match call.arguments.first()?.as_expression()? {
        Expression::StringLiteral(lit) => lit.value.text().into_owned(),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            tpl.quasis.first()?.value.raw.to_string()
        }
        _ => return None,
    };
    // Vite resolves relative entry URLs from its root, not the caller's directory.
    Some(
        source
            .strip_prefix("./")
            .map_or_else(|| source.clone(), |relative| format!("/{relative}")),
    )
}
