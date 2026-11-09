//! The candidates of the opt-in operators (`Operator::OPT_IN`).
//!
//! The block, switch case and throw candidates are statement sites: the
//! emitter wraps them in an `if` statement, not in a conditional
//! expression. See `emit`.

use oxc_ast::ast::{
    Argument, ArrayExpression, ArrowFunctionExpression, BlockStatement, CallExpression, Class,
    Expression, Function, FunctionBody, ObjectExpression, Statement, SwitchCase, ThrowStatement,
    VariableDeclaration, VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use super::{Candidate, Collector, has_optional_link, is_skipped_call, pair};
use crate::{MutantChange, Operator};

/// The StrykerJS method pairs. A method with `None` is removed with its
/// call: `s.trim()` becomes `s`. `setUTCDate` has no reverse pair, so
/// `setTime` maps to `setDate` only.
const METHODS: [(&str, Option<&str>); 35] = [
    ("charAt", None),
    ("filter", None),
    ("reverse", None),
    ("slice", None),
    ("sort", None),
    ("substr", None),
    ("substring", None),
    ("trim", None),
    ("endsWith", Some("startsWith")),
    ("startsWith", Some("endsWith")),
    ("every", Some("some")),
    ("some", Some("every")),
    ("toLocaleLowerCase", Some("toLocaleUpperCase")),
    ("toLocaleUpperCase", Some("toLocaleLowerCase")),
    ("toLowerCase", Some("toUpperCase")),
    ("toUpperCase", Some("toLowerCase")),
    ("trimEnd", Some("trimStart")),
    ("trimStart", Some("trimEnd")),
    ("min", Some("max")),
    ("max", Some("min")),
    ("setDate", Some("setTime")),
    ("setTime", Some("setDate")),
    ("setFullYear", Some("setMonth")),
    ("setMonth", Some("setFullYear")),
    ("setHours", Some("setMinutes")),
    ("setMinutes", Some("setHours")),
    ("setSeconds", Some("setMilliseconds")),
    ("setMilliseconds", Some("setSeconds")),
    ("setUTCDate", Some("setTime")),
    ("setUTCFullYear", Some("setUTCMonth")),
    ("setUTCMonth", Some("setUTCFullYear")),
    ("setUTCHours", Some("setUTCMinutes")),
    ("setUTCMinutes", Some("setUTCHours")),
    ("setUTCSeconds", Some("setUTCMilliseconds")),
    ("setUTCMilliseconds", Some("setUTCSeconds")),
];

/// The static name and the replacement of a method in `METHODS`.
fn method_replacement(name: &str) -> Option<(&'static str, Option<&'static str>)> {
    METHODS.iter().find(|(method, _)| *method == name).copied()
}

/// True when the statements can move into a nested block and be removed
/// as one mutant:
///
/// - no statement at their own top level is a declaration. A `let`,
///   `const`, `class` or `function` declaration would get the scope of the
///   nested block: `var g; function g() {}` becomes a redeclaration error,
///   and in sloppy code a function declaration no longer replaces a
///   parameter;
/// - no `var` or function declaration exists at any depth outside a nested
///   function. The switch keeps such a binding hoisted, while the applied
///   mutant removes it: a later read of the name gives `undefined` in the
///   run and a `ReferenceError` in the reported mutant.
fn is_movable(statements: &[Statement<'_>]) -> bool {
    if statements.iter().any(Statement::is_declaration) {
        return false;
    }
    let mut finder = HoistedDeclaration::default();
    for statement in statements {
        finder.visit_statement(statement);
    }
    !finder.found
}

/// Finds a `var` or function declaration that hoists out of a block. The
/// search does not enter a function or a class: their declarations have
/// their own scope.
#[derive(Default)]
struct HoistedDeclaration {
    found: bool,
}

impl<'a> Visit<'a> for HoistedDeclaration {
    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        self.found |= it.kind == VariableDeclarationKind::Var;
        walk::walk_variable_declaration(self, it);
    }

    fn visit_function(&mut self, it: &Function<'a>, _flags: ScopeFlags) {
        self.found |= it.is_declaration();
    }

    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _it: &Class<'a>) {}
}

/// True when the node that `visit` walks holds a `super(...)` call.
fn calls_super(visit: impl FnOnce(&mut SuperCall)) -> bool {
    let mut finder = SuperCall::default();
    visit(&mut finder);
    finder.found
}

/// Finds a `super(...)` call.
#[derive(Default)]
struct SuperCall {
    found: bool,
}

impl<'a> Visit<'a> for SuperCall {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        self.found |= matches!(it.callee, Expression::Super(_));
        walk::walk_call_expression(self, it);
    }
}

/// The source text of a string literal with the value `value`, in the quote
/// of the original literal.
fn quote_string(value: &str, quote: char) -> String {
    let mut text = String::with_capacity(value.len() + 2);
    text.push(quote);
    for ch in value.chars() {
        match ch {
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\u{2028}' => text.push_str("\\u2028"),
            '\u{2029}' => text.push_str("\\u2029"),
            _ if ch == quote => {
                text.push('\\');
                text.push(ch);
            }
            _ => text.push(ch),
        }
    }
    text.push(quote);
    text
}

impl Collector<'_> {
    /// Adds the `block-statement` candidate of a function body. The
    /// directive prologue stays, so that the body keeps its strictness. A
    /// constructor with a `super(...)` call keeps its body: without the call,
    /// each instance creation throws.
    pub(super) fn empty_function_body(&mut self, body: &FunctionBody<'_>) {
        let Some(first) = body.statements.first() else {
            return;
        };
        if !is_movable(&body.statements)
            || (self.in_constructor() && calls_super(|finder| finder.visit_function_body(body)))
        {
            return;
        }
        // After a directive, the range starts at the first statement: a
        // directive without a semicolon ends at a line break, and the
        // wrapper must not follow it on the same line.
        let start = if body.directives.is_empty() {
            body.span.start + 1
        } else {
            first.span().start
        };
        self.empty_statements((start, body.span.end - 1));
    }

    /// Adds the `block-statement` candidate of a non-empty block. In a
    /// constructor, a block with a `super(...)` call keeps its statements,
    /// as the constructor body does.
    pub(super) fn empty_block(&mut self, it: &BlockStatement<'_>) {
        if it.body.is_empty()
            || !is_movable(&it.body)
            || (self.in_constructor() && calls_super(|finder| finder.visit_block_statement(it)))
        {
            return;
        }
        self.empty_statements((it.span.start + 1, it.span.end - 1));
    }

    fn in_constructor(&self) -> bool {
        self.frames.last().is_some_and(|frame| frame.constructor)
    }

    fn empty_statements(&mut self, region: (u32, u32)) {
        self.add_statement(
            region,
            Operator::BlockStatement,
            "",
            MutantChange::new(MutantChange::STATEMENTS, ""),
        );
    }

    /// Adds the `switch-case` candidate: the clause loses its statements, so
    /// it falls through to the next clause.
    pub(super) fn empty_case(&mut self, it: &SwitchCase<'_>) {
        let (Some(first), Some(last)) = (it.consequent.first(), it.consequent.last()) else {
            return;
        };
        if !is_movable(&it.consequent) {
            return;
        }
        self.add_statement(
            (first.span().start, last.span().end),
            Operator::SwitchCase,
            "",
            MutantChange::new(MutantChange::STATEMENTS, ""),
        );
    }

    /// Adds the `throw-statement` candidate of `throw new X(...)`. The
    /// statement becomes `{}`, so that an `if` without braces keeps a body.
    pub(super) fn remove_throw(&mut self, it: &ThrowStatement<'_>) {
        if matches!(
            it.argument.without_parentheses(),
            Expression::NewExpression(_)
        ) {
            self.add_statement(
                pair(it.span),
                Operator::ThrowStatement,
                "{}",
                MutantChange::new(MutantChange::STATEMENT, "{}"),
            );
        }
    }

    /// Adds the `array-literal` candidate.
    pub(super) fn array_literal(&mut self, it: &ArrayExpression<'_>) {
        let (replacement, change) = if it.elements.is_empty() {
            ("[\"fallow\"]", MutantChange::new("[]", "[\"fallow\"]"))
        } else {
            ("[]", MutantChange::new(MutantChange::ARRAY, "[]"))
        };
        self.add(
            it.span,
            Operator::ArrayLiteral,
            pair(it.span),
            replacement.to_owned(),
            change,
        );
    }

    /// Adds the `object-literal` candidate of a non-empty object literal.
    pub(super) fn object_literal(&mut self, it: &ObjectExpression<'_>) {
        if !it.properties.is_empty() {
            self.add(
                it.span,
                Operator::ObjectLiteral,
                pair(it.span),
                "{}".to_owned(),
                MutantChange::new(MutantChange::OBJECT, "{}"),
            );
        }
    }

    /// Adds the `method-expression` and `call-expression` candidates of a
    /// call. A call in an optional chain gets none: a wrapper around it
    /// would end the short circuit of the chain.
    pub(super) fn call_candidates(&mut self, it: &CallExpression<'_>) {
        if it.optional || has_optional_link(&it.callee) {
            return;
        }
        self.method_expression(it);
        self.call_expression(it);
    }

    fn method_expression(&mut self, it: &CallExpression<'_>) {
        let Expression::StaticMemberExpression(member) = &it.callee else {
            return;
        };
        if matches!(member.object, Expression::Super(_)) {
            return;
        }
        let Some((name, other)) = method_replacement(member.property.name.as_str()) else {
            return;
        };
        if let Some(other) = other {
            self.add(
                it.span,
                Operator::MethodExpression,
                pair(member.property.span),
                other.to_owned(),
                MutantChange::new(name, other),
            );
            return;
        }
        let object_end = member.object.span().end;
        let start = self
            .find_token(object_end, member.property.span.start, ".")
            .unwrap_or(object_end);
        self.add(
            it.span,
            Operator::MethodExpression,
            (start, it.span.end),
            String::new(),
            MutantChange::new(MutantChange::CALL, ""),
        );
    }

    /// A call statement, a returned call and a concise arrow body already get
    /// a `removed-call` or `return-value` mutant with the same result.
    ///
    /// The replacement is always `(void 0)`. Without the parentheses, the
    /// applied mutant `f() + 1` would read `void (0 + 1)`, while the switch
    /// evaluates `(void 0) + 1`, and `void 0 ** 2` is a syntax error.
    fn call_expression(&mut self, it: &CallExpression<'_>) {
        let span = pair(it.span);
        if is_skipped_call(it) || self.covered_calls.contains(&span) {
            return;
        }
        self.add(
            it.span,
            Operator::CallExpression,
            span,
            "(void 0)".to_owned(),
            MutantChange::new(MutantChange::CALL, "void 0"),
        );
    }

    /// Adds the `regex-constructor` candidates of `RegExp("...")` or
    /// `new RegExp("...")` with a string pattern and string or no flags.
    pub(super) fn regex_constructor(
        &mut self,
        callee: &Expression<'_>,
        arguments: &[Argument<'_>],
    ) {
        if !callee.is_specific_id("RegExp") {
            return;
        }
        let Some(Expression::StringLiteral(pattern)) =
            arguments.first().and_then(Argument::as_expression)
        else {
            return;
        };
        let flags = match arguments.get(1).map(|argument| argument.as_expression()) {
            None => "",
            Some(Some(Expression::StringLiteral(flags))) => match flags.value.as_str() {
                Some(flags) => flags,
                None => return,
            },
            Some(_) => return,
        };
        let Some(value) = pattern.value.as_str() else {
            return;
        };
        let quote = match self.byte(pattern.span.start) {
            Some(b'"') => '"',
            Some(b'\'') => '\'',
            _ => return,
        };
        for (mutated, change) in crate::regex::pattern_mutations(value, flags) {
            self.add(
                pattern.span,
                Operator::RegexConstructor,
                pair(pattern.span),
                quote_string(&mutated, quote),
                change,
            );
        }
    }

    fn add_statement(
        &mut self,
        region: (u32, u32),
        operator: Operator,
        replacement: &str,
        change: MutantChange,
    ) {
        self.add_site(
            Span::new(region.0, region.1),
            true,
            Candidate {
                operator,
                region,
                replacement: replacement.to_owned(),
                change,
            },
        );
    }
}
