//! The string values of a config expression that holds one path or a list
//! of paths.

use fallow_extract::JsText;
use oxc_ast::ast::{Expression, ObjectPropertyKind};

use super::{PathScope, find_property, find_stable_binding_init, path_string_in};
use crate::plugins::config_parser_array_pushes::collect_added_entries;

/// The strings of a string, an array, an object with string or array values,
/// or a Webpack 5 entry descriptor (`{ import: "..." }`).
///
/// An array element that is an object literal gives its `input` property, as
/// in the Angular CLI schema for `styles`, `scripts` and `polyfills`:
/// `{ "input": "src/x.scss", "bundleName": "x", "inject": false }`.
///
/// A conditional expression gives the values of both branches, because the
/// build picks one branch at run time: `flag ? ["a.ts"] : ["a.ts", "b.ts"]`.
///
/// In a program scope, an identifier of a top-level array binding gives the
/// array values and the values that top-level `push`, `unshift` and `splice`
/// calls add, also in a condition: `if (ci) setup.push("./ci.ts")`.
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
        Expression::Identifier(id) => match array_binding_values(&id.name, scope) {
            Some(values) => values,
            None => path_string_in(expr, scope).into_iter().collect(),
        },
        _ => path_string_in(expr, scope).into_iter().collect(),
    }
}

/// The values of a top-level array binding and of its `push`-style calls, or
/// `None` when `name` is not a stable array binding of the program.
fn array_binding_values(name: &str, scope: PathScope<'_>) -> Option<Vec<String>> {
    let PathScope::Program(program) = scope else {
        return None;
    };
    let Expression::ArrayExpression(array) =
        find_stable_binding_init(program, name, true)?.get_inner_expression()
    else {
        return None;
    };
    // Each element is read as one path. An element can name the binding
    // again (`const a = [{ input: a }]`), so the full reader would recurse.
    let mut values: Vec<String> = array
        .elements
        .iter()
        .filter_map(|element| element.as_expression())
        .filter_map(|element| path_string_in(element, scope))
        .collect();
    let mut added = Vec::new();
    collect_added_entries(&program.body, name, &mut added);
    values.extend(
        added
            .into_iter()
            .filter_map(|argument| argument.as_expression())
            .filter_map(|argument| path_string_in(argument, scope)),
    );
    Some(values)
}
