//! Functions that return an Effect schema, for `effect.schemaExportsAsTypes`.
//!
//! A call `makeUser()` is a schema when `makeUser` is a function declaration,
//! or a `const` with an arrow function or a function expression, and each
//! `return` of the function is a schema by the rules of `effect_schema.rs`.
//! The check reads syntax only:
//! - an arrow function with an expression body returns that expression;
//! - a block body must end in a `return` or a `throw` on each path, so the
//!   function cannot return `undefined`;
//! - a `const` schema at the top of the body can be the root of a returned
//!   chain;
//! - a `return` can call a second such function, to a depth of
//!   [`MAX_HELPER_DEPTH`] functions;
//! - an `async` function, a generator, a function that the module assigns
//!   again, and a function that returns a parameter are not such functions.

use oxc_ast::AstKind;
use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, Class, Expression, Function, FunctionBody,
    ReturnStatement, Statement, VariableDeclarationKind,
};
use oxc_ast_visit::Visit;
use oxc_semantic::SymbolId;
use oxc_syntax::scope::ScopeFlags;

use super::{SchemaShape, SchemaShapes, unwrap};

/// The count of functions that one call can go through:
/// `a()` where `a` returns `b()`, `b` returns `c()`, `c` returns `d()`.
const MAX_HELPER_DEPTH: usize = 4;

impl<'a> SchemaShapes<'_, 'a> {
    /// True when the module-scope binding `name` is a function whose each
    /// `return` is a schema that the module itself proves.
    pub fn is_schema_helper(&self, name: &str) -> bool {
        let scoping = self.semantic.scoping();
        scoping
            .get_binding(scoping.root_scope_id(), oxc_str::Ident::from(name))
            .and_then(|symbol| self.helper_call_shape(symbol))
            == Some(SchemaShape::Known)
    }

    /// The shape that a call of the function `symbol` gives.
    ///
    /// Only a schema result is kept. A function that is not a schema now can
    /// be one at a later call, because its `return` can read a module-scope
    /// schema that a later statement declares.
    pub(super) fn helper_call_shape(&self, symbol: SymbolId) -> Option<SchemaShape> {
        if let Some(shape) = self.helpers.borrow().get(&symbol) {
            return Some(shape.clone());
        }
        let body = self.helper_body(symbol)?;
        // A function that is under inspection calls itself, directly or
        // through a second function.
        if self.helpers_in_progress.borrow().len() >= MAX_HELPER_DEPTH
            || !self.helpers_in_progress.borrow_mut().insert(symbol)
        {
            return None;
        }
        let shape = match body {
            HelperBody::Expression(expression) => self.expression_shape(expression),
            HelperBody::Block(body) => self.block_shape(body),
        };
        self.helpers_in_progress.borrow_mut().remove(&symbol);
        if let Some(shape) = &shape {
            self.helpers.borrow_mut().insert(symbol, shape.clone());
        }
        shape
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

    /// The shape that a block body returns: every `return` is a schema, and
    /// no path leaves the body without a `return` or a `throw`.
    fn block_shape(&self, body: &FunctionBody<'a>) -> Option<SchemaShape> {
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
            shape: None,
            all_schemas: true,
        };
        returns.visit_function_body(body);
        returns.shape.filter(|_| returns.all_schemas)
    }
}

enum HelperBody<'s, 'a> {
    Expression(&'s Expression<'a>),
    Block(&'s FunctionBody<'a>),
}

fn function_body<'s, 'a>(function: &'s Function<'a>) -> Option<HelperBody<'s, 'a>> {
    if function.r#async || function.generator {
        return None;
    }
    function.body.as_deref().map(HelperBody::Block)
}

fn arrow_body<'s, 'a>(arrow: &'s ArrowFunctionExpression<'a>) -> Option<HelperBody<'s, 'a>> {
    if arrow.r#async {
        return None;
    }
    if let Some(expression) = arrow.get_expression() {
        return Some(HelperBody::Expression(expression));
    }
    arrow.body.as_function_body().map(HelperBody::Block)
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

/// Joins the shapes of the `return` statements of one function body.
struct Returns<'h, 's, 'a> {
    shapes: &'h SchemaShapes<'s, 'a>,
    shape: Option<SchemaShape>,
    all_schemas: bool,
}

impl Returns<'_, '_, '_> {
    /// Each `return` must be a schema. Two returns that depend on different
    /// imports give no shape, because one fact names one import.
    fn join(&mut self, next: Option<SchemaShape>) {
        let Some(next) = next else {
            self.all_schemas = false;
            return;
        };
        match (&self.shape, &next) {
            (None | Some(SchemaShape::Known), _) => self.shape = Some(next),
            (Some(_), SchemaShape::Known) => {}
            (Some(current), _) if *current == next => {}
            _ => self.all_schemas = false,
        }
    }
}

impl<'a> Visit<'a> for Returns<'_, '_, 'a> {
    fn visit_return_statement(&mut self, statement: &ReturnStatement<'a>) {
        let shape = statement
            .argument
            .as_ref()
            .and_then(|argument| self.shapes.expression_shape(argument));
        self.join(shape);
    }

    // A `return` of an inner function or a class member is not a `return` of
    // the inspected function.
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _: &Class<'a>) {}
}
