//! Static import specifier collection.

use super::*;

/// Collect every statically-resolvable module specifier from a dynamic
/// `import()` source expression, following conditional and logical branches so
/// `import(cond ? './a' : './b')` yields both `./a` and `./b`. String literals
/// and no-substitution template literals resolve; genuinely runtime branches (a
/// bare identifier, a call) are skipped, so a mixed
/// `import(cond ? './a' : runtimeVar)` still credits the literal branch while
/// `import(runtimeVar)` yields nothing (correctly left unresolvable). Repeated
/// literals across branches are deduplicated so one call site never yields two
/// identical edges (and thus duplicate unresolved-import findings).
pub(super) fn collect_static_import_specifiers(source: &Expression<'_>, out: &mut Vec<String>) {
    match source {
        Expression::StringLiteral(lit) => {
            let value = lit.value.text().into_owned();
            if !out.contains(&value) {
                out.push(value);
            }
        }
        Expression::TemplateLiteral(tpl)
            if tpl.expressions.is_empty() && !tpl.quasis.is_empty() =>
        {
            let value = tpl.quasis[0].value.raw.to_string();
            if !value.is_empty() && !out.contains(&value) {
                out.push(value);
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_static_import_specifiers(&paren.expression, out);
        }
        Expression::ConditionalExpression(cond) => {
            collect_static_import_specifiers(&cond.consequent, out);
            collect_static_import_specifiers(&cond.alternate, out);
        }
        Expression::LogicalExpression(logical) => {
            collect_static_import_specifiers(&logical.left, out);
            collect_static_import_specifiers(&logical.right, out);
        }
        _ => {}
    }
}
