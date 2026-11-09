//! The candidates of the opt-in operators (`Operator::OPT_IN`).
//!
//! The block, switch case and throw candidates are statement sites: the
//! emitter wraps them in an `if` statement, not in a conditional
//! expression. See `emit`.

use oxc_ast::ast::{
    Argument, ArrayExpression, BlockStatement, CallExpression, Expression, FunctionBody,
    ObjectExpression, Statement, SwitchCase, ThrowStatement, VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};

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

/// True for a statement that declares a name for the whole switch block. A
/// clause with one keeps its statements, because a nested block would
/// change the scope of the name.
fn is_lexical_declaration(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::VariableDeclaration(declaration) => {
            !matches!(declaration.kind, VariableDeclarationKind::Var)
        }
        Statement::FunctionDeclaration(_) | Statement::ClassDeclaration(_) => true,
        _ => false,
    }
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
        if body.statements.is_empty() {
            return;
        }
        if self.frames.last().is_some_and(|frame| frame.constructor) {
            let mut finder = SuperCall::default();
            finder.visit_function_body(body);
            if finder.found {
                return;
            }
        }
        let start = body
            .directives
            .last()
            .map_or(body.span.start + 1, |directive| directive.span.end);
        self.empty_statements((start, body.span.end - 1));
    }

    /// Adds the `block-statement` candidate of a non-empty block.
    pub(super) fn empty_block(&mut self, it: &BlockStatement<'_>) {
        if !it.body.is_empty() {
            self.empty_statements((it.span.start + 1, it.span.end - 1));
        }
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
        if it.consequent.iter().any(is_lexical_declaration) {
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
    fn call_expression(&mut self, it: &CallExpression<'_>) {
        let span = pair(it.span);
        if is_skipped_call(it) || self.covered_calls.contains(&span) {
            return;
        }
        let replacement = if self.continues_after(it.span.end) {
            "(void 0)"
        } else {
            "void 0"
        };
        self.add(
            it.span,
            Operator::CallExpression,
            span,
            replacement.to_owned(),
            MutantChange::new(MutantChange::CALL, "void 0"),
        );
    }

    /// True when the token after `end` continues the expression as a member,
    /// call, template or exponent: `void 0.x` would lex `0.` as a number,
    /// and `void 0 ** 2` is a syntax error.
    fn continues_after(&self, end: u32) -> bool {
        let rest = self
            .source
            .as_bytes()
            .get(end as usize..)
            .unwrap_or_default();
        let next = rest.iter().position(|byte| !byte.is_ascii_whitespace());
        matches!(
            next.map(|index| &rest[index..]),
            Some([b'.' | b'[' | b'(' | b'`' | b'*', ..] | [b'?', b'.', ..])
        )
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
