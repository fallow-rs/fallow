//! The string values of a config expression that holds one path or a list
//! of paths.

use fallow_extract::JsText;
use oxc_ast::ast::{Expression, ObjectPropertyKind};

use super::{PathScope, find_property, path_string_in};

/// The strings of a string, an array, an object with string or array values,
/// or a Webpack 5 entry descriptor (`{ import: "..." }`).
///
/// An array element that is an object literal gives its `input` property, as
/// in the Angular CLI schema for `styles`, `scripts` and `polyfills`:
/// `{ "input": "src/x.scss", "bundleName": "x", "inject": false }`.
///
/// A conditional expression gives the values of both branches, because the
/// build picks one branch at run time: `flag ? ["a.ts"] : ["a.ts", "b.ts"]`.
pub(super) fn string_or_array_in(expr: &Expression, scope: PathScope<'_>) -> Vec<String> {
    match expr {
        Expression::StringLiteral(s) => vec![s.value.text().into_owned()],
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .map(|q| vec![q.value.raw.to_string()])
            .unwrap_or_default(),
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|el| el.as_expression())
            .flat_map(|e| match e {
                Expression::ObjectExpression(obj) => find_property(obj, "input")
                    .map(|p| string_or_array_in(&p.value, scope))
                    .unwrap_or_default(),
                _ => path_string_in(e, scope).into_iter().collect(),
            })
            .collect(),
        Expression::ObjectExpression(obj) => obj
            .properties
            .iter()
            .flat_map(|p| {
                if let ObjectPropertyKind::ObjectProperty(p) = p {
                    match &p.value {
                        Expression::ArrayExpression(_) => string_or_array_in(&p.value, scope),
                        Expression::ObjectExpression(value_obj) => {
                            find_property(value_obj, "import")
                                .map(|import_prop| string_or_array_in(&import_prop.value, scope))
                                .unwrap_or_default()
                        }
                        _ => path_string_in(&p.value, scope).into_iter().collect(),
                    }
                } else {
                    Vec::new()
                }
            })
            .collect(),
        Expression::ConditionalExpression(conditional) => {
            let mut values = string_or_array_in(&conditional.consequent, scope);
            values.extend(string_or_array_in(&conditional.alternate, scope));
            values
        }
        Expression::ParenthesizedExpression(paren)
            if matches!(paren.expression, Expression::ConditionalExpression(_)) =>
        {
            string_or_array_in(&paren.expression, scope)
        }
        _ => path_string_in(expr, scope).into_iter().collect(),
    }
}
