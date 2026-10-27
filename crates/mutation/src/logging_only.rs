//! Conditions whose only effect is a logging call (issue #3719).
//!
//! The collector gives no mutant for a logging call. An `if` statement with a
//! pure test and branches that hold only logging calls has the same problem:
//! each mutant of the test can only change which message is logged. The
//! collector skips such a statement completely.

use oxc_ast::ast::{
    Argument, BindingIdentifier, ChainElement, Expression, IfStatement, MemberExpression, Program,
    Statement, TemplateLiteral, UnaryOperator,
};
use oxc_ast_visit::Visit;

use crate::collect::callee_without_wrappers;

/// Which logging roots of a file are the globals. A file that declares or
/// imports a binding named `console` or `debug` anywhere can call a function
/// with effects through that name, so the rule does not apply to that name
/// in the file. The check reads names, not scopes, so a binding in one
/// function also turns the rule off in the rest of the file.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoggingGlobals {
    console: bool,
    debug: bool,
}

impl LoggingGlobals {
    /// Scans `program` for a binding named `console` or `debug`.
    pub fn scan(program: &Program<'_>, source: &str) -> Self {
        if !source.contains("console") && !source.contains("debug") {
            return Self::default();
        }
        let mut bindings = LoggingBindings::default();
        bindings.visit_program(program);
        Self {
            console: !bindings.console,
            debug: !bindings.debug,
        }
    }
}

#[derive(Default)]
struct LoggingBindings {
    console: bool,
    debug: bool,
}

impl<'a> Visit<'a> for LoggingBindings {
    fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
        match it.name.as_str() {
            "console" => self.console = true,
            "debug" => self.debug = true,
            _ => {}
        }
    }
}

/// True when the `if` statement has a pure test, a consequent that holds only
/// logging calls, and no alternate or an alternate that holds only logging
/// calls. An `else if` alternate must meet the same conditions.
///
/// A getter, a `valueOf` method, a Proxy trap or a `Symbol.hasInstance`
/// method that a pure test reaches can still have an effect. The rule
/// accepts this risk, as the logging rule accepts it for the arguments of a
/// logging call.
pub fn is_logging_only_if(it: &IfStatement<'_>, globals: LoggingGlobals) -> bool {
    is_pure(&it.test)
        && is_logging_branch(&it.consequent, globals)
        && it
            .alternate
            .as_ref()
            .is_none_or(|alternate| match alternate {
                Statement::IfStatement(inner) => is_logging_only_if(inner, globals),
                _ => is_logging_branch(alternate, globals),
            })
}

/// A `console.*` call or a `debug(...)` call on the global name. A computed
/// key such as `console[name]` must be pure.
fn is_global_logging_callee(callee: &Expression<'_>, globals: LoggingGlobals) -> bool {
    let callee = callee_without_wrappers(callee);
    if callee.is_specific_id("debug") {
        return globals.debug;
    }
    let Some(member) = callee.as_member_expression() else {
        return false;
    };
    let on_console = callee_without_wrappers(member.object()).is_specific_id("console");
    let pure_key = match member {
        MemberExpression::ComputedMemberExpression(computed) => is_pure(&computed.expression),
        MemberExpression::StaticMemberExpression(_) => true,
        MemberExpression::PrivateFieldExpression(_) => false,
    };
    on_console && pure_key && globals.console
}

/// A logging call statement, or a non-empty block of them. The arguments of
/// each call must be pure, so the branch changes no state.
fn is_logging_branch(statement: &Statement<'_>, globals: LoggingGlobals) -> bool {
    match statement {
        Statement::BlockStatement(block) => {
            !block.body.is_empty()
                && block
                    .body
                    .iter()
                    .all(|statement| is_logging_branch(statement, globals))
        }
        Statement::ExpressionStatement(statement) => {
            match statement.expression.without_parentheses() {
                Expression::CallExpression(call) => {
                    is_global_logging_callee(&call.callee, globals)
                        && call.arguments.iter().all(is_pure_argument)
                }
                Expression::ChainExpression(chain) => match &chain.expression {
                    ChainElement::CallExpression(call) => {
                        is_global_logging_callee(&call.callee, globals)
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
