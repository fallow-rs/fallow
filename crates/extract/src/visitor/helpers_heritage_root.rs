//! Root name of a class heritage expression, for `override` member credit.

use oxc_ast::ast::{Argument, ArrayExpressionElement, Expression};
use rustc_hash::FxHashSet;

/// Whether a heritage call argument is data and not a class to mix in.
///
/// Literals and object literals are data. An object property can name a
/// schema (`{ user: User }` in an Effect error), so object values are not
/// checked. An object spread (`{ ...sharedFields }`) copies own properties
/// into a plain object, so it is data too. An array counts only when no
/// element is a bare identifier, member expression or spread, because
/// `Mixin([LocalBase])` lists classes. A bare identifier counts only when it
/// names a module-scope `const` with an object literal initializer, because
/// `Mixin(LocalBase)` can name a project class.
fn is_heritage_data_argument(
    arg: &Argument<'_>,
    plain_object_names: Option<&FxHashSet<String>>,
) -> bool {
    match arg {
        Argument::StringLiteral(_)
        | Argument::NumericLiteral(_)
        | Argument::BooleanLiteral(_)
        | Argument::NullLiteral(_)
        | Argument::TemplateLiteral(_)
        | Argument::ObjectExpression(_) => true,
        Argument::Identifier(ident) => {
            plain_object_names.is_some_and(|names| names.contains(ident.name.as_str()))
        }
        Argument::ArrayExpression(array) => array.elements.iter().all(|element| {
            !matches!(
                element,
                ArrayExpressionElement::Identifier(_)
                    | ArrayExpressionElement::StaticMemberExpression(_)
                    | ArrayExpressionElement::SpreadElement(_)
            )
        }),
        _ => false,
    }
}

/// Whether `init` is an object literal, also behind parentheses, `as` or
/// `satisfies`. The runtime value of such an initializer is a plain object and
/// never a class.
pub(in crate::visitor) fn is_object_literal_initializer(init: &Expression<'_>) -> bool {
    match init {
        Expression::ObjectExpression(_) => true,
        Expression::ParenthesizedExpression(paren) => {
            is_object_literal_initializer(&paren.expression)
        }
        Expression::TSAsExpression(cast) => is_object_literal_initializer(&cast.expression),
        Expression::TSSatisfiesExpression(cast) => is_object_literal_initializer(&cast.expression),
        _ => false,
    }
}

/// Root identifier of a class `extends` expression.
///
/// `Base` for `extends Base`, `ns` for `extends ns.Base`, `Schema` for
/// `extends Schema.TaggedError<E>()("E", {...})`. A call returns `None` unless
/// every argument is data (see [`is_heritage_data_argument`]): a mixin such
/// as `Mixin(LocalBase)` can take a project class that declares the
/// overridden member.
pub(super) fn heritage_root_name(
    expr: &Expression<'_>,
    plain_object_names: Option<&FxHashSet<String>>,
) -> Option<String> {
    match expr {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => {
            heritage_root_name(&member.object, plain_object_names)
        }
        Expression::CallExpression(call) => {
            if !call
                .arguments
                .iter()
                .all(|arg| is_heritage_data_argument(arg, plain_object_names))
            {
                return None;
            }
            heritage_root_name(&call.callee, plain_object_names)
        }
        Expression::TSInstantiationExpression(inst) => {
            heritage_root_name(&inst.expression, plain_object_names)
        }
        Expression::ParenthesizedExpression(paren) => {
            heritage_root_name(&paren.expression, plain_object_names)
        }
        _ => None,
    }
}
