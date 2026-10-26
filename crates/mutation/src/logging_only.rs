//! Conditions whose only effect is a logging call (issue #3719).
//!
//! The collector gives no mutant for a logging call. An `if` statement with a
//! pure test and branches that hold only logging calls has the same problem:
//! each mutant of the test can only change which message is logged. The
//! collector skips such a statement completely.

use oxc_ast::ast::{
    Argument, ChainElement, Expression, IfStatement, Statement, TemplateLiteral, UnaryOperator,
};

use crate::collect::is_logging_callee;

/// True when the `if` statement has a pure test, a consequent that holds only
/// logging calls, and no alternate or an alternate that holds only logging
/// calls. An `else if` alternate must meet the same conditions.
///
/// A getter or a `valueOf` method that a pure test reads can still have an
/// effect. The rule accepts this risk, as the logging rule accepts it for
/// the arguments of a logging call.
pub fn is_logging_only_if(it: &IfStatement<'_>) -> bool {
    is_pure(&it.test)
        && is_logging_branch(&it.consequent)
        && it
            .alternate
            .as_ref()
            .is_none_or(|alternate| match alternate {
                Statement::IfStatement(inner) => is_logging_only_if(inner),
                _ => is_logging_branch(alternate),
            })
}

/// A logging call statement, or a non-empty block of them. The arguments of
/// each call must be pure, so the branch changes no state.
fn is_logging_branch(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::BlockStatement(block) => {
            !block.body.is_empty() && block.body.iter().all(is_logging_branch)
        }
        Statement::ExpressionStatement(statement) => {
            match statement.expression.without_parentheses() {
                Expression::CallExpression(call) => {
                    is_logging_callee(&call.callee) && call.arguments.iter().all(is_pure_argument)
                }
                Expression::ChainExpression(chain) => match &chain.expression {
                    ChainElement::CallExpression(call) => {
                        is_logging_callee(&call.callee)
                            && call.arguments.iter().all(is_pure_argument)
                    }
                    _ => false,
                },
                _ => false,
            }
        }
        _ => false,
    }
}

fn is_pure_argument(argument: &Argument<'_>) -> bool {
    argument
        .as_expression()
        .is_some_and(|expression| is_pure(expression))
}

/// An expression without a call, a construction, an assignment, an update,
/// a `delete`, an `await`, a `yield` or a function. Reads, literals and
/// operators are pure.
fn is_pure(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::Identifier(_)
        | Expression::ThisExpression(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::StringLiteral(_) => true,
        Expression::TemplateLiteral(template) => is_pure_template(template),
        Expression::ParenthesizedExpression(inner) => is_pure(&inner.expression),
        Expression::TSAsExpression(inner) => is_pure(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => is_pure(&inner.expression),
        Expression::TSNonNullExpression(inner) => is_pure(&inner.expression),
        Expression::TSTypeAssertion(inner) => is_pure(&inner.expression),
        Expression::UnaryExpression(unary) => {
            unary.operator != UnaryOperator::Delete && is_pure(&unary.argument)
        }
        Expression::BinaryExpression(binary) => is_pure(&binary.left) && is_pure(&binary.right),
        Expression::LogicalExpression(logical) => is_pure(&logical.left) && is_pure(&logical.right),
        Expression::ConditionalExpression(conditional) => {
            is_pure(&conditional.test)
                && is_pure(&conditional.consequent)
                && is_pure(&conditional.alternate)
        }
        Expression::StaticMemberExpression(member) => is_pure(&member.object),
        Expression::PrivateFieldExpression(member) => is_pure(&member.object),
        Expression::ComputedMemberExpression(member) => {
            is_pure(&member.object) && is_pure(&member.expression)
        }
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::StaticMemberExpression(member) => is_pure(&member.object),
            ChainElement::PrivateFieldExpression(member) => is_pure(&member.object),
            ChainElement::ComputedMemberExpression(member) => {
                is_pure(&member.object) && is_pure(&member.expression)
            }
            ChainElement::TSNonNullExpression(inner) => is_pure(&inner.expression),
            ChainElement::CallExpression(_) => false,
        },
        _ => false,
    }
}

fn is_pure_template(template: &TemplateLiteral<'_>) -> bool {
    template.expressions.iter().all(is_pure)
}
