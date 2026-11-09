//! Replacement tables of the operators: which token or constant replaces
//! an operator, and which operators belong to one precedence level.

use oxc_ast::ast::Expression;
use oxc_syntax::operator::{AssignmentOperator, BinaryOperator, UnaryOperator};

use crate::Operator;

/// The comparison operator with its operands swapped: `0 < n` is `n > 0`.
pub const fn mirrored(operator: BinaryOperator) -> BinaryOperator {
    match operator {
        BinaryOperator::LessThan => BinaryOperator::GreaterThan,
        BinaryOperator::LessEqualThan => BinaryOperator::GreaterEqualThan,
        BinaryOperator::GreaterThan => BinaryOperator::LessThan,
        BinaryOperator::GreaterEqualThan => BinaryOperator::LessEqualThan,
        other => other,
    }
}

/// The replacement that is equivalent for `n <operator> 0` when `n` is a
/// non-negative integer: `n > 0` is `n != 0`, `n <= 0` is `n == 0`,
/// `n < 0` is always false and `n >= 0` is always true.
pub const fn equivalent_zero_replacement(operator: BinaryOperator) -> Option<&'static str> {
    match operator {
        BinaryOperator::GreaterThan => Some("!="),
        BinaryOperator::LessEqualThan => Some("=="),
        BinaryOperator::LessThan => Some("false"),
        BinaryOperator::GreaterEqualThan => Some("true"),
        _ => None,
    }
}

pub const fn is_relational(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::LessThan
            | BinaryOperator::LessEqualThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::GreaterEqualThan
            | BinaryOperator::Equality
            | BinaryOperator::Inequality
            | BinaryOperator::StrictEquality
            | BinaryOperator::StrictInequality
    )
}

/// The operators at the relational or equality precedence level.
pub const fn is_comparison_level(operator: BinaryOperator) -> bool {
    is_relational(operator) || matches!(operator, BinaryOperator::In | BinaryOperator::Instanceof)
}

/// True when the replacement operator binds weaker than `operator`, so that
/// a token swap inside a comparison regroups the operands.
pub const fn lowers_precedence(operator: BinaryOperator, replacement: &str) -> bool {
    matches!(
        operator,
        BinaryOperator::LessThan
            | BinaryOperator::LessEqualThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::GreaterEqualThan
    ) && matches!(replacement.as_bytes(), b"==" | b"!=")
}

/// The non-redundant relational replacements: operator texts, then the
/// constants that replace the whole expression. An equality gets both
/// constants, because no operator swap gives the constant result.
pub const fn relational_replacements(
    operator: BinaryOperator,
) -> Option<(&'static [&'static str], &'static [&'static str])> {
    let replacements: (&'static [&'static str], &'static [&'static str]) = match operator {
        BinaryOperator::LessThan => (&["<=", "!="], &["false"]),
        BinaryOperator::LessEqualThan => (&["<", "=="], &["true"]),
        BinaryOperator::GreaterThan => (&[">=", "!="], &["false"]),
        BinaryOperator::GreaterEqualThan => (&[">", "=="], &["true"]),
        BinaryOperator::StrictEquality => (&["!=="], &["false", "true"]),
        BinaryOperator::StrictInequality => (&["==="], &["true", "false"]),
        BinaryOperator::Equality => (&["!="], &["false", "true"]),
        BinaryOperator::Inequality => (&["=="], &["true", "false"]),
        _ => return None,
    };
    Some(replacements)
}

/// True for an expression that always gives a boolean: a comparison or a
/// logical not. Such an operand of `&&` or `||` gets a constant mutant.
pub fn is_boolean_valued(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::BinaryExpression(binary) => is_relational(binary.operator),
        Expression::UnaryExpression(unary) => unary.operator == UnaryOperator::LogicalNot,
        _ => false,
    }
}

/// The swap of a compound assignment operator, and the operator that
/// reports it. `+=` and `-=` keep their mutants for a text operand.
pub const fn assignment_replacement(
    operator: AssignmentOperator,
) -> Option<(Operator, &'static str)> {
    let replacement = match operator {
        AssignmentOperator::Addition => (Operator::ArithmeticOperator, "-="),
        AssignmentOperator::Subtraction => (Operator::ArithmeticOperator, "+="),
        AssignmentOperator::Multiplication => (Operator::ArithmeticOperator, "/="),
        AssignmentOperator::Division | AssignmentOperator::Remainder => {
            (Operator::ArithmeticOperator, "*=")
        }
        AssignmentOperator::ShiftLeft => (Operator::ArithmeticOperator, ">>="),
        AssignmentOperator::ShiftRight => (Operator::ArithmeticOperator, "<<="),
        AssignmentOperator::BitwiseAnd => (Operator::ArithmeticOperator, "|="),
        AssignmentOperator::BitwiseOR => (Operator::ArithmeticOperator, "&="),
        AssignmentOperator::LogicalAnd => (Operator::LogicalOperator, "||="),
        AssignmentOperator::LogicalOr | AssignmentOperator::LogicalNullish => {
            (Operator::LogicalOperator, "&&=")
        }
        _ => return None,
    };
    Some(replacement)
}

pub const fn arithmetic_replacement(operator: BinaryOperator) -> Option<&'static str> {
    match operator {
        BinaryOperator::Addition => Some("-"),
        BinaryOperator::Subtraction => Some("+"),
        BinaryOperator::Multiplication => Some("/"),
        BinaryOperator::Division | BinaryOperator::Remainder => Some("*"),
        _ => None,
    }
}

pub fn is_text_operand(expression: &Expression<'_>) -> bool {
    matches!(
        expression.without_parentheses(),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
    )
}
