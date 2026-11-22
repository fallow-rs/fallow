//! Functions that return an Effect schema, for `effect.schemaExportsAsTypes`.
//!
//! A call `makeUser()` is a schema when `makeUser` is a function declaration,
//! or a `const` with an arrow function or a function expression, and each
//! `return` of the function is a schema by the rules of `effect_schema.rs`.
//! A call `identity(Schema.String)` is a schema when each `return` of
//! `identity` is the same parameter, and the argument at that position is a
//! schema. The check reads syntax only:
//! - an arrow function with an expression body returns that expression;
//! - a block body must end in a `return` or a `throw` on each path, so the
//!   function cannot return `undefined`;
//! - a `const` schema at the top of the body can be the root of a returned
//!   chain;
//! - a `return` can call a second such function, to a depth of
//!   [`MAX_HELPER_DEPTH`] functions;
//! - an `async` function, a generator and a function that the module assigns
//!   again are not such functions;
//! - a parameter that the function assigns or declares again, or a parameter after a
//!   spread argument, gives no schema.

use oxc_ast::AstKind;
use oxc_ast::ast::{
    Argument, ArrowFunctionExpression, BindingPattern, Class, Expression, FormalParameters,
    Function, FunctionBody, ReturnStatement, Statement, VariableDeclarationKind,
};
use oxc_ast_visit::Visit;
use oxc_semantic::SymbolId;
use oxc_syntax::scope::ScopeFlags;

use super::{SchemaShape, SchemaShapes, unwrap};

/// The count of functions that one call can go through:
/// `a()` where `a` returns `b()`, `b` returns `c()`, `c` returns `d()`.
pub(super) const MAX_HELPER_DEPTH: usize = 4;

/// What each `return` of a function gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HelperResult {
    Shape(SchemaShape),
    /// The parameter at this position.
    Param(usize),
}

impl HelperResult {
    fn join(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Shape(left), Self::Shape(right)) => left.join(right).map(Self::Shape),
            (Self::Param(left), Self::Param(right)) => (left == right).then_some(Self::Param(left)),
            _ => None,
        }
    }
}

impl<'a> SchemaShapes<'_, 'a> {
    /// True when the module-scope binding `name` is a function whose each
    /// `return` is a schema that the module itself proves.
    pub fn is_schema_helper(&self, name: &str) -> bool {
        let scoping = self.semantic.scoping();
        scoping
            .get_binding(scoping.root_scope_id(), oxc_str::Ident::from(name))
            .and_then(|symbol| self.helper_result(symbol))
            == Some(HelperResult::Shape(SchemaShape::Known))
    }

    /// The shape that a call of the function `symbol` with `arguments` gives.
    pub(super) fn helper_call_shape(
        &self,
        symbol: SymbolId,
        arguments: &[Argument<'a>],
    ) -> Option<SchemaShape> {
        let result = self.helper_result(symbol)?;
        self.apply(result, arguments)
    }

    /// The shape that a call of a function with `body` and `arguments` gives.
    pub(super) fn body_call_shape(
        &self,
        body: HelperBody<'_, 'a>,
        arguments: &[Argument<'a>],
    ) -> Option<SchemaShape> {
        let result = self.body_result(body)?;
        self.apply(result, arguments)
    }

    fn apply(&self, result: HelperResult, arguments: &[Argument<'a>]) -> Option<SchemaShape> {
        match result {
            HelperResult::Shape(shape) => Some(shape),
            HelperResult::Param(index) => {
                let before = arguments.get(..=index)?;
                if before.iter().any(Argument::is_spread) {
                    return None;
                }
                self.expression_shape(arguments[index].as_expression()?)
            }
        }
    }

    /// What each `return` of the function `symbol` gives.
    ///
    /// Only a result is kept. A function that is not a schema now can be one
    /// at a later call, because its `return` can read a module-scope schema
    /// that a later statement declares.
    fn helper_result(&self, symbol: SymbolId) -> Option<HelperResult> {
        if let Some(result) = self.helpers.borrow().get(&symbol) {
            return Some(result.clone());
        }
        let result = self.body_result(self.helper_body(symbol)?)?;
        self.helpers.borrow_mut().insert(symbol, result.clone());
        Some(result)
    }

    /// A function that is under inspection calls itself, directly or through
    /// a second function, and gives no result.
    fn body_result(&self, body: HelperBody<'_, 'a>) -> Option<HelperResult> {
        self.guarded(body.span_start, || match body.kind {
            HelperKind::Expression(expression) => self.return_result(expression, body.params),
            HelperKind::Block(block) => self.block_result(block, body.params),
        })
    }

    fn return_result(
        &self,
        expression: &Expression<'a>,
        params: &FormalParameters<'a>,
    ) -> Option<HelperResult> {
        if let Some(index) = self.param_index(expression, params) {
            return Some(HelperResult::Param(index));
        }
        self.expression_shape(expression).map(HelperResult::Shape)
    }

    /// The position of the parameter that `expression` reads, when the
    /// function never assigns that parameter again.
    fn param_index(
        &self,
        expression: &Expression<'a>,
        params: &FormalParameters<'a>,
    ) -> Option<usize> {
        let Expression::Identifier(identifier) = unwrap(expression) else {
            return None;
        };
        let symbol = self.symbol(identifier)?;
        let scoping = self.semantic.scoping();
        // A `var` or a function declaration of the same name in the body
        // gives the parameter a new value, and is no assignment.
        if scoping.symbol_is_mutated(symbol) || scoping.symbol_declarations(symbol).nth(1).is_some()
        {
            return None;
        }
        params.items.iter().position(|param| {
            matches!(&param.pattern, BindingPattern::BindingIdentifier(id) if id.symbol_id() == symbol)
        })
    }

    /// The body of the function that `symbol` binds, when the binding always
    /// holds that one function. A type of the same name
    /// (`export type Make = ...`) and an overload signature are declarations
    /// of the symbol too, and they hold no value.
    fn helper_body(&self, symbol: SymbolId) -> Option<HelperBody<'_, 'a>> {
        let scoping = self.semantic.scoping();
        if scoping.symbol_is_mutated(symbol) {
            return None;
        }
        let mut body = None;
        for declaration in scoping.symbol_declarations(symbol) {
            let found = match self.semantic.nodes().kind(declaration) {
                AstKind::TSTypeAliasDeclaration(_) | AstKind::TSInterfaceDeclaration(_) => continue,
                AstKind::Function(function) if function.body.is_none() => continue,
                AstKind::Function(function) => function_body(function)?,
                AstKind::VariableDeclarator(declarator) => {
                    if !scoping.symbol_flags(symbol).is_const_variable() {
                        return None;
                    }
                    match unwrap(declarator.init.as_ref()?) {
                        Expression::ArrowFunctionExpression(arrow) => arrow_body(arrow)?,
                        Expression::FunctionExpression(function) => function_body(function)?,
                        _ => return None,
                    }
                }
                _ => return None,
            };
            if body.replace(found).is_some() {
                return None;
            }
        }
        body
    }

    /// What a block body returns: every `return` gives one result, and no
    /// path leaves the body without a `return` or a `throw`.
    fn block_result(
        &self,
        body: &FunctionBody<'a>,
        params: &FormalParameters<'a>,
    ) -> Option<HelperResult> {
        if !always_exits(&body.statements) {
            return None;
        }
        for statement in &body.statements {
            let Statement::VariableDeclaration(declaration) = statement else {
                continue;
            };
            if declaration.kind != VariableDeclarationKind::Const {
                continue;
            }
            for declarator in &declaration.declarations {
                let (BindingPattern::BindingIdentifier(id), Some(init)) =
                    (&declarator.id, &declarator.init)
                else {
                    continue;
                };
                if let Some(shape) = self.expression_shape(init) {
                    self.helper_locals
                        .borrow_mut()
                        .insert(id.symbol_id(), shape);
                }
            }
        }
        let mut returns = Returns {
            shapes: self,
            params,
            result: None,
            failed: false,
        };
        returns.visit_function_body(body);
        returns.result.filter(|_| !returns.failed)
    }
}

/// The parameters and the body of a function.
#[derive(Clone, Copy)]
pub(super) struct HelperBody<'s, 'a> {
    span_start: u32,
    params: &'s FormalParameters<'a>,
    kind: HelperKind<'s, 'a>,
}

#[derive(Clone, Copy)]
enum HelperKind<'s, 'a> {
    Expression(&'s Expression<'a>),
    Block(&'s FunctionBody<'a>),
}

pub(super) fn function_body<'s, 'a>(function: &'s Function<'a>) -> Option<HelperBody<'s, 'a>> {
    if function.r#async || function.generator {
        return None;
    }
    let body = function.body.as_deref()?;
    Some(HelperBody {
        span_start: body.span.start,
        params: &function.params,
        kind: HelperKind::Block(body),
    })
}

pub(super) fn arrow_body<'s, 'a>(
    arrow: &'s ArrowFunctionExpression<'a>,
) -> Option<HelperBody<'s, 'a>> {
    if arrow.r#async {
        return None;
    }
    let kind = match arrow.get_expression() {
        Some(expression) => HelperKind::Expression(expression),
        None => HelperKind::Block(arrow.body.as_function_body()?),
    };
    Some(HelperBody {
        span_start: arrow.span.start,
        params: &arrow.params,
        kind,
    })
}

/// True when control cannot go past the end of the statements: the last one
/// is a `return`, a `throw`, a block that ends in one, or an `if` whose two
/// branches end in one.
fn always_exits(statements: &[Statement<'_>]) -> bool {
    statements.last().is_some_and(statement_always_exits)
}

fn statement_always_exits(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => always_exits(&block.body),
        Statement::IfStatement(branch) => {
            statement_always_exits(&branch.consequent)
                && branch
                    .alternate
                    .as_ref()
                    .is_some_and(statement_always_exits)
        }
        _ => false,
    }
}

/// Joins the results of the `return` statements of one function body.
struct Returns<'h, 's, 'a> {
    shapes: &'h SchemaShapes<'s, 'a>,
    params: &'h FormalParameters<'a>,
    result: Option<HelperResult>,
    failed: bool,
}

impl Returns<'_, '_, '_> {
    /// Each `return` must give a result, and the results must join.
    fn join(&mut self, next: Option<HelperResult>) {
        let joined = match (self.result.take(), next) {
            (_, None) => None,
            (None, next) => next,
            (Some(current), Some(next)) => current.join(next),
        };
        self.failed |= joined.is_none();
        self.result = joined;
    }
}

impl<'a> Visit<'a> for Returns<'_, '_, 'a> {
    fn visit_return_statement(&mut self, statement: &ReturnStatement<'a>) {
        let result = statement
            .argument
            .as_ref()
            .and_then(|argument| self.shapes.return_result(argument, self.params));
        self.join(result);
    }

    // A `return` of an inner function or a class member is not a `return` of
    // the inspected function.
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _: &Class<'a>) {}
}
