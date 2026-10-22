//! Context names for functions that have no declared name.
//!
//! The complexity visitor names a function from its declaration, its binding,
//! a method or property key, or `export default`. A function that has none of
//! these gets a placeholder (`<anonymous>` or `<arrow>`). The helpers here read
//! a name from the call that holds the function, so that a callback body that
//! a user wrote as `Command.make("setup", () => ...)` or
//! `Effect.fn("Users.create")(function* () {...})` has a name in reports.
//!
//! The helpers apply only where the visitor would emit a placeholder. A name
//! that the visitor gives today never changes.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;

use crate::js_text::JsText;

/// Maximum number of characters of a string literal that a context name keeps.
/// A longer literal (a test title, most of all) is cut and ends with
/// [`CUT_LITERAL_SUFFIX`].
const MAX_CONTEXT_LITERAL_CHARS: usize = 40;

/// Suffix of a string literal that a context name cut.
const CUT_LITERAL_SUFFIX: &str = "...";

/// Callee property of the Effect function constructor that takes a span name:
/// `Effect.fn("span")(function* () {...})`.
const SPAN_NAMED_CONSTRUCTOR: &str = "fn";

/// Name for the first argument of `<x>.fn("span")(fn)` or `fn("span")(fn)`.
///
/// Effect uses the string as the name of the function in traces, so the
/// complexity unit uses it too. Returns `None` for any other call shape.
pub fn span_name(call: &CallExpression<'_>) -> Option<String> {
    let Expression::CallExpression(constructor) = call.callee.without_parentheses() else {
        return None;
    };
    let is_span_constructor = match constructor.callee.without_parentheses() {
        Expression::Identifier(ident) => ident.name == SPAN_NAMED_CONSTRUCTOR,
        Expression::StaticMemberExpression(member) => {
            member.property.name == SPAN_NAMED_CONSTRUCTOR
        }
        _ => false,
    };
    if !is_span_constructor {
        return None;
    }
    constructor
        .arguments
        .first()
        .and_then(argument_literal)
        .map(|literal| cut_literal(&literal))
}

/// Name for the function arguments after a string-literal first argument:
/// `router.get("/users", (req) => ...)` gives `router.get("/users")`.
///
/// Returns `None` when the first argument is not a string literal or when the
/// callee has no readable name (a call result, a computed member).
pub fn string_keyed_call_name(call: &CallExpression<'_>) -> Option<String> {
    // Most calls with a string first argument (`require("x")`, `t("key")`)
    // hold no function, so check the shape before any allocation.
    if !call.arguments.iter().skip(1).any(can_hold_named_function) {
        return None;
    }
    let literal = call.arguments.first().and_then(argument_literal)?;
    let callee = callee_label(&call.callee)?;
    // A backslash and a double quote in the literal get a backslash, so the
    // quoted literal in the name stays one token.
    let quoted = cut_literal(&literal)
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    Some(format!("{callee}(\"{quoted}\")"))
}

/// Whether an argument is a function, or a call that can hold an anonymous
/// generator, so that a context name can reach a function through it.
const fn can_hold_named_function(argument: &Argument<'_>) -> bool {
    matches!(
        argument,
        Argument::FunctionExpression(_)
            | Argument::ArrowFunctionExpression(_)
            | Argument::CallExpression(_)
    )
}

/// The call that a function body returns, when that call takes a generator
/// function argument: `Effect.gen(function* () {...})`. The generator is then
/// the real body of the enclosing function, so it takes the enclosing name.
///
/// Parentheses, type assertions and trailing `.pipe(...)` calls are looked
/// through: `Effect.gen(function* () {...}).pipe(Effect.withSpan("x"))`.
pub fn returned_generator_call<'a, 'b>(expr: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    match expr.without_parentheses() {
        Expression::TSAsExpression(inner) => returned_generator_call(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => returned_generator_call(&inner.expression),
        Expression::CallExpression(call) => {
            if has_generator_argument(call) {
                return Some(call);
            }
            match call.callee.without_parentheses() {
                Expression::StaticMemberExpression(member) if member.property.name == "pipe" => {
                    returned_generator_call(&member.object)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether an argument is a generator function expression without its own name.
pub fn is_anonymous_generator(argument: &Argument<'_>) -> bool {
    matches!(argument, Argument::FunctionExpression(func) if func.generator && func.id.is_none())
}

/// Whether a frame name is a placeholder (`<anonymous>`, `<arrow>`) or a
/// synthetic unit (`<module>`). Such a name does not pass to a generator.
pub fn is_placeholder_name(name: &str) -> bool {
    name.starts_with('<')
}

fn has_generator_argument(call: &CallExpression<'_>) -> bool {
    call.arguments.iter().any(is_anonymous_generator)
}

/// The value of a string literal or of a template literal without
/// substitutions. A character that breaks a one-line name becomes a space.
fn argument_literal(argument: &Argument<'_>) -> Option<String> {
    let raw = match argument {
        Argument::StringLiteral(literal) => literal.value.text(),
        Argument::TemplateLiteral(template) if template.expressions.is_empty() => template
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked)
            .map(JsText::text)?,
        _ => return None,
    };
    Some(
        raw.chars()
            .map(|c| if breaks_one_line_name(c) { ' ' } else { c })
            .collect(),
    )
}

/// A control character breaks the line. A `|` or a backtick breaks a
/// Markdown table cell or a code span in the CI summaries.
fn breaks_one_line_name(c: char) -> bool {
    c.is_control() || c == '|' || c == '`'
}

fn cut_literal(literal: &str) -> String {
    if literal.chars().count() <= MAX_CONTEXT_LITERAL_CHARS {
        return literal.to_owned();
    }
    let kept: String = literal.chars().take(MAX_CONTEXT_LITERAL_CHARS).collect();
    format!("{}{CUT_LITERAL_SUFFIX}", kept.trim_end())
}

/// Readable callee: a dotted path when every segment is an identifier
/// (`Command.make`), else the last property (`handle` in `a.b().handle`).
fn callee_label(callee: &Expression<'_>) -> Option<String> {
    match callee.without_parentheses() {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => {
            Some(dotted_path(&member.object).map_or_else(
                || member.property.name.to_string(),
                |object| format!("{object}.{}", member.property.name),
            ))
        }
        _ => None,
    }
}

fn dotted_path(expr: &Expression<'_>) -> Option<String> {
    match expr.without_parentheses() {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => {
            dotted_path(&member.object).map(|object| format!("{object}.{}", member.property.name))
        }
        _ => None,
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    /// Parse `source` as one expression statement and apply `read` to it.
    fn with_expression<T>(source: &str, read: impl FnOnce(&Expression<'_>) -> T) -> T {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
            panic!("expected one expression statement in {source}");
        };
        read(&stmt.expression)
    }

    fn call_name(source: &str, name: fn(&CallExpression<'_>) -> Option<String>) -> Option<String> {
        with_expression(source, |expr| match expr {
            Expression::CallExpression(call) => name(call),
            _ => panic!("expected a call in {source}"),
        })
    }

    #[test]
    fn span_name_reads_the_effect_fn_literal() {
        assert_eq!(
            call_name("Effect.fn(\"Users.create\")(function* () {})", span_name),
            Some("Users.create".to_owned())
        );
        assert_eq!(
            call_name("fn(`Users.create`)(function* () {})", span_name),
            Some("Users.create".to_owned())
        );
    }

    #[test]
    fn span_name_ignores_other_curried_calls() {
        assert_eq!(
            call_name("Effect.gen(\"x\")(function* () {})", span_name),
            None
        );
        assert_eq!(
            call_name("Effect.fn(name)(function* () {})", span_name),
            None
        );
        assert_eq!(call_name("Effect.fn(function* () {})", span_name), None);
    }

    #[test]
    fn string_keyed_call_name_uses_the_dotted_callee() {
        assert_eq!(
            call_name(
                "Command.make(\"setup\", {}, () => 1)",
                string_keyed_call_name
            ),
            Some("Command.make(\"setup\")".to_owned())
        );
        assert_eq!(
            call_name(
                "api.group().handle(\"github\", () => 1)",
                string_keyed_call_name
            ),
            Some("handle(\"github\")".to_owned())
        );
        assert_eq!(
            call_name("it(\"works\", () => 1)", string_keyed_call_name),
            Some("it(\"works\")".to_owned())
        );
    }

    #[test]
    fn string_keyed_call_name_needs_a_literal_first_argument() {
        assert_eq!(
            call_name(
                "Match.when({ type: \"a\" }, () => 1)",
                string_keyed_call_name
            ),
            None
        );
        assert_eq!(
            call_name("on(`a${b}`, () => 1)", string_keyed_call_name),
            None
        );
        assert_eq!(
            call_name("f()(\"a\", () => 1)", string_keyed_call_name),
            None
        );
    }

    #[test]
    fn long_literals_are_cut() {
        let name = call_name(
            "test(\"creates, updates, and deletes a machine in one region\", () => 1)",
            string_keyed_call_name,
        );
        assert_eq!(
            name,
            Some("test(\"creates, updates, and deletes a machine...\")".to_owned())
        );
    }

    #[test]
    fn control_characters_become_spaces() {
        assert_eq!(
            call_name("it(`line one\nline two`, () => 1)", string_keyed_call_name),
            Some("it(\"line one line two\")".to_owned())
        );
    }

    #[test]
    fn multi_byte_literals_are_cut_by_character() {
        let title = "\u{e9}".repeat(50);
        let name = call_name(&format!("it(\"{title}\", () => 1)"), string_keyed_call_name);
        assert_eq!(name, Some(format!("it(\"{}...\")", "\u{e9}".repeat(40))));
    }

    #[test]
    fn markdown_breaking_characters_become_spaces() {
        assert_eq!(
            call_name("it(\"a | b `c`\", () => 1)", string_keyed_call_name),
            Some("it(\"a   b  c \")".to_owned())
        );
    }

    #[test]
    fn backslashes_in_literals_get_a_backslash() {
        assert_eq!(
            call_name(r#"it("a\\", () => 1)"#, string_keyed_call_name),
            Some(r#"it("a\\")"#.to_owned())
        );
    }

    #[test]
    fn calls_without_a_function_argument_get_no_name() {
        assert_eq!(call_name("t(\"key\", 1)", string_keyed_call_name), None);
    }

    #[test]
    fn double_quotes_in_literals_get_a_backslash() {
        assert_eq!(
            call_name(r#"it('reads "x"', () => 1)"#, string_keyed_call_name),
            Some(r#"it("reads \"x\"")"#.to_owned())
        );
    }

    #[test]
    fn returned_generator_call_looks_through_pipe_and_assertions() {
        for source in [
            "Effect.gen(function* () {})",
            "(Effect.gen(function* () {}))",
            "Effect.gen(function* () {}).pipe(Effect.withSpan(\"x\"))",
            "Effect.gen(function* () {}) as Effect.Effect<void>",
        ] {
            assert!(
                with_expression(source, |expr| returned_generator_call(expr).is_some()),
                "{source}"
            );
        }
    }

    #[test]
    fn returned_generator_call_needs_an_anonymous_generator() {
        for source in [
            "items.map((item) => item)",
            "Effect.gen(function* named() {})",
            "run(function () {})",
            "Effect.succeed(1).pipe(Effect.map((x) => x))",
        ] {
            assert!(
                with_expression(source, |expr| returned_generator_call(expr).is_none()),
                "{source}"
            );
        }
    }
}
