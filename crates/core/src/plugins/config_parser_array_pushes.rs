//! The values that top-level code adds to an array binding of a config file,
//! such as `if (needsDocker) globalSetup.push("./docker.ts")`.

use oxc_ast::ast::{Argument, Expression, Statement};

/// Collect the arguments that `binding.push(...)`, `binding.unshift(...)` and
/// `binding.splice(start, count, ...)` calls add to the array of `binding`.
///
/// The calls can be top-level statements or sit in a top-level block, `if`
/// statement or `try` statement, such as
/// `if (isDev) plugins.push("expo-dev-client")`. A call inside a function
/// body is not read, because the config can call the function zero times.
pub(super) fn collect_added_entries<'a>(
    statements: &'a [Statement<'a>],
    binding: &str,
    added: &mut Vec<&'a Argument<'a>>,
) {
    for statement in statements {
        match statement {
            Statement::ExpressionStatement(statement) => {
                added.extend(added_arguments(&statement.expression, binding));
            }
            Statement::BlockStatement(block) => collect_added_entries(&block.body, binding, added),
            Statement::IfStatement(statement) => {
                collect_added_entries(std::slice::from_ref(&statement.consequent), binding, added);
                if let Some(alternate) = &statement.alternate {
                    collect_added_entries(std::slice::from_ref(alternate), binding, added);
                }
            }
            Statement::TryStatement(statement) => {
                collect_added_entries(&statement.block.body, binding, added);
                if let Some(handler) = &statement.handler {
                    collect_added_entries(&handler.body.body, binding, added);
                }
                if let Some(finalizer) = &statement.finalizer {
                    collect_added_entries(&finalizer.body, binding, added);
                }
            }
            _ => {}
        }
    }
}

/// The arguments that one call expression adds to the array of `binding`.
fn added_arguments<'a>(
    expression: &'a Expression<'a>,
    binding: &str,
) -> impl Iterator<Item = &'a Argument<'a>> {
    let call = match expression.get_inner_expression() {
        Expression::CallExpression(call) => Some(call),
        _ => None,
    };
    let skip = call.and_then(|call| {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        if !matches!(member.object.get_inner_expression(), Expression::Identifier(id) if id.name == binding)
        {
            return None;
        }
        match member.property.name.as_str() {
            "push" | "unshift" => Some(0),
            "splice" => Some(2),
            _ => None,
        }
    });
    call.zip(skip)
        .into_iter()
        .flat_map(|(call, skip)| call.arguments.iter().skip(skip))
}
