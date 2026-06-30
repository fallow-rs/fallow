//! Shared object and callee helpers for visitor extraction.

#[allow(clippy::wildcard_imports, reason = "many AST node variants used")]
use oxc_ast::ast::*;

pub(super) fn callee_leaf_name(callee: &Expression<'_>) -> Option<String> {
    match callee {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => Some(member.property.name.to_string()),
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => callee_leaf_name(&call.callee),
            ChainElement::StaticMemberExpression(member) => Some(member.property.name.to_string()),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn expression_has_boundary_validation_keys(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::ObjectExpression(obj) => object_has_any_key(obj, &["body", "query", "params"]),
        Expression::ParenthesizedExpression(paren) => {
            expression_has_boundary_validation_keys(&paren.expression)
        }
        Expression::TSAsExpression(ts_as) => {
            expression_has_boundary_validation_keys(&ts_as.expression)
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            expression_has_boundary_validation_keys(&ts_sat.expression)
        }
        _ => false,
    }
}

pub(super) fn expression_has_fastify_schema(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::ObjectExpression(obj) => object_property_value(obj, "schema")
            .is_some_and(|schema| expression_has_boundary_validation_keys(schema)),
        Expression::ParenthesizedExpression(paren) => {
            expression_has_fastify_schema(&paren.expression)
        }
        Expression::TSAsExpression(ts_as) => expression_has_fastify_schema(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => {
            expression_has_fastify_schema(&ts_sat.expression)
        }
        _ => false,
    }
}

pub(super) fn object_has_any_key(obj: &ObjectExpression<'_>, keys: &[&str]) -> bool {
    obj.properties.iter().any(|prop| {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return false;
        };
        prop.key
            .static_name()
            .is_some_and(|name| keys.iter().any(|key| name == *key))
    })
}

fn object_property_value<'a>(
    obj: &'a ObjectExpression<'a>,
    key: &str,
) -> Option<&'a Expression<'a>> {
    obj.properties.iter().find_map(|prop| {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return None;
        };
        prop.key
            .static_name()
            .is_some_and(|name| name == key)
            .then_some(&prop.value)
    })
}

/// Whether an Angular `inject(TOKEN, { optional: true })` call's second argument
/// is an object literal carrying `optional: true`.
pub(super) fn angular_inject_is_optional(expr: &CallExpression<'_>) -> bool {
    let Some(Argument::ObjectExpression(options)) = expr.arguments.get(1) else {
        return false;
    };
    matches!(
        object_property_value(options, "optional"),
        Some(Expression::BooleanLiteral(lit)) if lit.value
    )
}

/// The token identifier passed to an Angular `@Inject(TOKEN)` parameter
/// decorator.
pub(super) fn angular_param_inject_token<'a>(
    decorator: &'a Decorator<'a>,
    is_named_import: &impl Fn(&str, &str, &str) -> bool,
) -> Option<&'a str> {
    let Expression::CallExpression(call) = &decorator.expression else {
        return None;
    };
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    if callee.name != "Inject" || !is_named_import(callee.name.as_str(), "@angular/core", "Inject")
    {
        return None;
    }
    match call.arguments.first() {
        Some(Argument::Identifier(ident)) => Some(ident.name.as_str()),
        _ => None,
    }
}

/// Whether an Angular constructor parameter carries an `@Optional()` decorator
/// imported from `@angular/core`.
pub(super) fn angular_param_is_optional(
    param: &FormalParameter<'_>,
    is_named_import: &impl Fn(&str, &str, &str) -> bool,
) -> bool {
    param.decorators.iter().any(|decorator| {
        let callee = match &decorator.expression {
            Expression::CallExpression(call) => &call.callee,
            other => other,
        };
        matches!(callee, Expression::Identifier(id)
            if id.name == "Optional"
                && is_named_import(id.name.as_str(), "@angular/core", "Optional"))
    })
}
