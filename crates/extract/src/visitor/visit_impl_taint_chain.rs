//! Chained taint identifier collection.

use super::*;

/// Collect bare identifier references from a declarator initializer for the
/// #1146 taint chain step, recursing through the same conservative expression
/// shapes `collect_binding_source_path_candidates` admits (static TS wrappers,
/// await, template substitutions, `+` concat operands, object-literal property
/// values) plus the top-level bare-identifier alias (`const b = a`).
///
/// Deliberately NOT collected (each chained identifier becomes a fresh
/// source-backed binding that can chain further, so every arm here is a
/// false-positive amplifier):
/// - member-expression roots (`const b = a.id`): a property read off a tainted
///   local frequently strips taint in practice (`a.length`,
///   `a.startsWith("/")`); unlike the sink-side `collect_idents_into` member
///   rule (sound because the whole expression flows into a sink), re-tainting
///   the result here would propagate boolean/length/index reads
/// - call expressions (`const b = f(a)`): the call boundary is where
///   sanitizers live; #878 already covers proven helper-return shapes
/// - logical / conditional / sequence expressions (`a || "x"`, `c ? a : b`)
pub(super) fn collect_chained_taint_idents(expr: &Expression<'_>, out: &mut Vec<String>) {
    match expr {
        Expression::Identifier(ident) => push_ident(ident.name.as_str(), out),
        Expression::ParenthesizedExpression(paren) => {
            collect_chained_taint_idents(&paren.expression, out);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_chained_taint_idents(&ts_as.expression, out);
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            collect_chained_taint_idents(&ts_sat.expression, out);
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            collect_chained_taint_idents(&ts_non_null.expression, out);
        }
        Expression::AwaitExpression(await_expr) => {
            collect_chained_taint_idents(&await_expr.argument, out);
        }
        Expression::TemplateLiteral(template) => {
            for expression in &template.expressions {
                collect_chained_taint_idents(expression, out);
            }
        }
        Expression::BinaryExpression(binary)
            if binary.operator == oxc_ast::ast::BinaryOperator::Addition =>
        {
            collect_chained_taint_idents(&binary.left, out);
            collect_chained_taint_idents(&binary.right, out);
        }
        Expression::ObjectExpression(object) => {
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    continue;
                };
                collect_chained_taint_idents(&property.value, out);
            }
        }
        _ => {}
    }
}
