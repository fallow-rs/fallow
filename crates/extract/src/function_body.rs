//! One view over block function bodies and concise arrow bodies.
//!
//! Before Oxc 0.151 the parser wrapped a concise arrow body (`() => expr`) in a
//! synthetic `FunctionBody` that held one `ExpressionStatement` with the span of
//! `expr`. Oxc now stores the expression directly in `ArrowFunctionBody`. The
//! helpers that read a function or arrow body take a [`BodyRef`], so a concise
//! body keeps the span and the single-expression shape it had before.

use oxc_ast::{
    ast::{ArrowFunctionBody, Expression, FunctionBody, Statement},
    match_expression,
};
use oxc_ast_visit::Visit;
use oxc_span::{GetSpan, Span};

/// A block function body, or the expression of a concise arrow body.
#[derive(Clone, Copy)]
pub enum BodyRef<'b, 'a> {
    /// `function f() { ... }` or `() => { ... }`.
    Block(&'b FunctionBody<'a>),
    /// `() => expr`.
    Concise(&'b Expression<'a>),
}

impl<'b, 'a> BodyRef<'b, 'a> {
    /// The body of an arrow function.
    pub fn arrow(body: &'b ArrowFunctionBody<'a>) -> Self {
        match body {
            ArrowFunctionBody::FunctionBody(block) => Self::Block(block),
            match_expression!(ArrowFunctionBody) => Self::Concise(body.to_expression()),
        }
    }

    /// The span of the block, or of the concise expression.
    pub fn span(self) -> Span {
        match self {
            Self::Block(block) => block.span,
            Self::Concise(expr) => expr.span(),
        }
    }

    /// The statements of a block body. A concise body has none.
    pub fn statements(self) -> &'b [Statement<'a>] {
        match self {
            Self::Block(block) => &block.statements,
            Self::Concise(_) => &[],
        }
    }

    /// Walk the body with `visitor`: `visit_function_body` for a block body,
    /// `visit_expression` for a concise body.
    pub fn visit<V: Visit<'a>>(self, visitor: &mut V) {
        match self {
            Self::Block(block) => visitor.visit_function_body(block),
            Self::Concise(expr) => visitor.visit_expression(expr),
        }
    }

    /// Visit each statement of a block body, or the expression of a concise
    /// body. Directives of a block body are not visited.
    pub fn visit_statements<V: Visit<'a>>(self, visitor: &mut V) {
        match self {
            Self::Block(block) => {
                for statement in &block.statements {
                    visitor.visit_statement(statement);
                }
            }
            Self::Concise(expr) => visitor.visit_expression(expr),
        }
    }
}
