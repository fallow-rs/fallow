//! Recognition of literal membership chains for the cyclomatic policy.
//!
//! A literal membership chain is a `||` chain where each operand is a strict
//! equality check of the same identifier against a primitive literal:
//!
//! ```js
//! kind === "a" || kind === "b" || kind === "c"
//! ```
//!
//! The recognition is conservative. A chain qualifies only when all of these
//! are true:
//!
//! - every operator in the chain is `||` (parentheses are transparent);
//! - every operand is `===` between a plain identifier and a string, number,
//!   bigint, boolean, or `null` literal, on either side;
//! - every operand checks the same identifier;
//! - the chain has at least two operands.
//!
//! Member access (`obj.kind === "a"`), calls, loose equality, `!==`, mixed
//! operators, and different identifiers do not qualify. Such a chain keeps
//! the ordinary cyclomatic count under every policy.

use oxc_ast::ast::{
    BinaryExpression, BinaryOperator, Expression, LogicalExpression, LogicalOperator,
};

/// Smallest number of operands that makes a membership chain.
const MIN_CHAIN_OPERANDS: usize = 2;

/// Whether `expr` is the root of a literal membership chain.
pub fn is_literal_membership_chain(expr: &LogicalExpression<'_>) -> bool {
    if expr.operator != LogicalOperator::Or {
        return false;
    }
    let mut subject: Option<&str> = None;
    let mut operands = 0_usize;
    if !collect_chain(&expr.left, &mut subject, &mut operands)
        || !collect_chain(&expr.right, &mut subject, &mut operands)
    {
        return false;
    }
    operands >= MIN_CHAIN_OPERANDS
}

/// Walk one side of an `||` chain. Return `false` when an operand does not
/// qualify or checks a different identifier.
fn collect_chain<'a>(
    expr: &'a Expression<'_>,
    subject: &mut Option<&'a str>,
    operands: &mut usize,
) -> bool {
    match expr.without_parentheses() {
        Expression::LogicalExpression(logical) if logical.operator == LogicalOperator::Or => {
            collect_chain(&logical.left, subject, operands)
                && collect_chain(&logical.right, subject, operands)
        }
        Expression::BinaryExpression(binary) => {
            let Some(name) = literal_equality_subject(binary) else {
                return false;
            };
            if subject.is_some_and(|existing| existing != name) {
                return false;
            }
            *subject = Some(name);
            *operands += 1;
            true
        }
        _ => false,
    }
}

/// The identifier name of `name === literal` or `literal === name`.
fn literal_equality_subject<'a>(binary: &'a BinaryExpression<'_>) -> Option<&'a str> {
    if binary.operator != BinaryOperator::StrictEquality {
        return None;
    }
    let left = binary.left.without_parentheses();
    let right = binary.right.without_parentheses();
    match (left, right) {
        (Expression::Identifier(ident), other) | (other, Expression::Identifier(ident))
            if is_primitive_literal(other) =>
        {
            Some(ident.name.as_str())
        }
        _ => None,
    }
}

const fn is_primitive_literal(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
    )
}

#[cfg(test)]
mod tests {
    use fallow_types::extract::{FunctionComplexity, compute_line_offsets};
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn analyze(source: &str) -> Vec<FunctionComplexity> {
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::ts()).parse();
        let line_offsets = compute_line_offsets(source);
        crate::complexity::compute_complexity(&parser_return.program, source, &line_offsets)
    }

    /// `(standard cyclomatic, literal membership increments)` of `name`.
    fn counts(source: &str, name: &str) -> (u16, u16) {
        let results = analyze(source);
        let function = results
            .iter()
            .find(|function| function.name == name)
            .unwrap_or_else(|| panic!("function '{name}' not found: {results:?}"));
        let membership = function
            .contributions
            .iter()
            .filter(|contribution| contribution.literal_membership)
            .count();
        (
            function.cyclomatic,
            u16::try_from(membership).expect("small count"),
        )
    }

    #[test]
    fn issue_example_chain_is_one_predicate() {
        let source = r#"export function byComparisons(kind: string) {
  if (kind === "a" || kind === "b" || kind === "c" || kind === "d" ||
      kind === "e" || kind === "f" || kind === "g") {
    throw new Error("Unsupported kind");
  }
}"#;
        assert_eq!(counts(source, "byComparisons"), (8, 6));
    }

    #[test]
    fn literal_on_the_left_and_mixed_literal_kinds_qualify() {
        let source = r#"function f(x: unknown) {
  return "a" === x || x === 1 || x === 2n || x === true || (x === null);
}"#;
        assert_eq!(counts(source, "f"), (5, 4));
    }

    #[test]
    fn different_operands_keep_the_standard_count() {
        let source = r#"function f(kind: string, other: string) {
  if (kind === "a" || kind === "b" || other === "c") { return 1; }
  return 0;
}"#;
        assert_eq!(counts(source, "f"), (4, 0));
    }

    #[test]
    fn a_sub_chain_of_a_mixed_chain_does_not_qualify() {
        let source = r#"function f(kind: string, flag: boolean) {
  if (flag || kind === "a" || kind === "b") { return 1; }
  if (kind === "a" || (kind === "b" || flag)) { return 2; }
  return 0;
}"#;
        assert_eq!(counts(source, "f"), (7, 0));
    }

    #[test]
    fn non_qualifying_operands_keep_the_standard_count() {
        let source = r#"function f(obj: { kind: string }, kind: string, n: number) {
  if (obj.kind === "a" || obj.kind === "b") { return 1; }
  if (kind == "a" || kind == "b") { return 2; }
  if (kind !== "a" && kind !== "b") { return 3; }
  if (kind === `a` || kind === "b") { return 4; }
  if (n === -1 || n === 1) { return 5; }
  if (kind === other() || kind === "b") { return 6; }
  return 0;
}
function other() { return "c"; }"#;
        assert_eq!(counts(source, "f"), (13, 0));
    }

    #[test]
    fn a_membership_chain_inside_a_wider_condition_is_one_predicate() {
        let source = r#"function f(kind: string, flag: boolean) {
  if (flag && (kind === "a" || kind === "b" || kind === "c")) { return 1; }
  return 0;
}"#;
        assert_eq!(counts(source, "f"), (5, 2));
    }

    #[test]
    fn unrelated_branches_still_count() {
        let source = r#"function f(kind: string, n: number) {
  if (kind === "a" || kind === "b") { return 1; }
  if (n > 1) { return 2; }
  for (let i = 0; i < n; i++) { if (i === 3) { return 3; } }
  return n > 0 ? 4 : 5;
}"#;
        let (standard, membership) = counts(source, "f");
        assert_eq!((standard, membership), (7, 1));
        assert_eq!(standard - membership, 6);
    }

    #[test]
    fn a_single_comparison_is_not_a_chain() {
        let source = r#"function f(kind: string) {
  return kind === "a";
}"#;
        assert_eq!(counts(source, "f"), (1, 0));
    }
}
