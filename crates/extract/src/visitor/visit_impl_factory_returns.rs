//! Factory return and assignment proof helpers for the visitor implementation.

#[allow(
    clippy::wildcard_imports,
    reason = "many factory-return AST types used"
)]
use oxc_ast::ast::*;

use super::super::{FactoryAssignedValue, helpers::is_builtin_constructor};
use super::unwrap_static_expr;

#[derive(Clone, Copy)]
pub(super) struct FactoryReturnFunctionInput<'site, 'ast> {
    pub(super) params: &'site FormalParameters<'ast>,
    pub(super) body: Option<&'site FunctionBody<'ast>>,
    pub(super) is_expression_body: bool,
    pub(super) is_async: bool,
    pub(super) is_generator: bool,
}

/// Visit every `return` statement reachable in `statements` in source order,
/// recursing through control flow (block / if / loops / try / switch / labeled)
/// but NOT into nested functions, which have their own return scope. The shared
/// traversal behind the return-shape helpers (count / first-arg / collect-args)
/// so the control-flow node set lives in one place. See issues #1265, #1441.
fn for_each_return_statement<'b, 'a>(
    statements: &'b [Statement<'a>],
    visit: &mut impl FnMut(&'b ReturnStatement<'a>),
) {
    for stmt in statements {
        for_each_return_in_statement(stmt, visit);
    }
}

fn for_each_return_in_statement<'b, 'a>(
    stmt: &'b Statement<'a>,
    visit: &mut impl FnMut(&'b ReturnStatement<'a>),
) {
    match stmt {
        Statement::ReturnStatement(ret) => visit(ret),
        Statement::BlockStatement(block) => for_each_return_statement(&block.body, visit),
        Statement::IfStatement(s) => {
            for_each_return_in_statement(&s.consequent, visit);
            if let Some(alternate) = s.alternate.as_ref() {
                for_each_return_in_statement(alternate, visit);
            }
        }
        Statement::ForStatement(s) => for_each_return_in_statement(&s.body, visit),
        Statement::ForInStatement(s) => for_each_return_in_statement(&s.body, visit),
        Statement::ForOfStatement(s) => for_each_return_in_statement(&s.body, visit),
        Statement::WhileStatement(s) => for_each_return_in_statement(&s.body, visit),
        Statement::DoWhileStatement(s) => for_each_return_in_statement(&s.body, visit),
        Statement::TryStatement(s) => {
            for_each_return_statement(&s.block.body, visit);
            if let Some(handler) = s.handler.as_ref() {
                for_each_return_statement(&handler.body.body, visit);
            }
            if let Some(finalizer) = s.finalizer.as_ref() {
                for_each_return_statement(&finalizer.body, visit);
            }
        }
        Statement::SwitchStatement(s) => {
            for case in &s.cases {
                for_each_return_statement(&case.consequent, visit);
            }
        }
        Statement::LabeledStatement(s) => for_each_return_in_statement(&s.body, visit),
        _ => {}
    }
}

/// Count every `return` statement reachable inside a function body (recursively
/// through nested blocks / control flow, but NOT into nested function or arrow
/// bodies, which have their own return scope). More than one means an
/// early-return / multi-branch `load`, which the harvest abstains on.
pub(in super::super) fn count_returns_in_statements(statements: &[Statement<'_>]) -> usize {
    let mut count = 0;
    for_each_return_statement(statements, &mut |_| count += 1);
    count
}

/// The class name in a `new Class()` expression, or `None` for a non-`new`
/// expression, a non-identifier callee, or a builtin constructor.
fn new_expression_class_name(expr: &Expression<'_>) -> Option<String> {
    let Expression::NewExpression(new_expr) = expr else {
        return None;
    };
    let Expression::Identifier(callee) = &new_expr.callee else {
        return None;
    };
    if is_builtin_constructor(callee.name.as_str()) {
        return None;
    }
    Some(callee.name.to_string())
}

/// Classify the right-hand side of a module-local assignment for the alias
/// value-proof: a direct `new Class()`, a bare `callee(...)` call (proven only if
/// `callee` is a strict same-file factory, resolved later), or anything else
/// (which poisons the proof). Unwraps parenthesized / `as` / `satisfies` / `!`
/// wrappers; `await` and every other shape are `Other`. See issue #1441 (Part A).
pub(super) fn classify_factory_assigned_value(expr: &Expression<'_>) -> FactoryAssignedValue {
    let expr = unwrap_static_expr(expr);
    match expr {
        Expression::NewExpression(_) => match new_expression_class_name(expr) {
            Some(class) => FactoryAssignedValue::NewClass(class),
            None => FactoryAssignedValue::Other,
        },
        Expression::CallExpression(call) => match &call.callee {
            Expression::Identifier(callee) => FactoryAssignedValue::Call(callee.name.to_string()),
            _ => FactoryAssignedValue::Other,
        },
        _ => FactoryAssignedValue::Other,
    }
}

/// Collect classified right-hand sides of plain `target = <rhs>` assignments
/// that DOMINATE the function's terminal return, so the proof reflects what the
/// alias actually returns. Only two dominating shapes are accepted:
///
/// - unconditional assignments at the function body's top level (incl. inside
///   an unconditionally-entered block / labeled statement), and
/// - the lazy-singleton init `if (!target) { target = ... }` (and the null /
///   undefined equivalents), whose guard guarantees `target` ends up set.
///
/// Assignments inside arbitrary conditionals, loops, switch, try, or nested
/// functions are NOT counted because they may not run. See #1441 (Part A);
/// covers the canonical `useApi` composable shape.
pub(super) fn collect_self_scope_assignments(
    statements: &[Statement<'_>],
    target: &str,
    out: &mut Vec<FactoryAssignedValue>,
) {
    for stmt in statements {
        collect_self_scope_assignments_in_statement(stmt, target, out);
    }
}

fn collect_self_scope_assignments_in_statement(
    stmt: &Statement<'_>,
    target: &str,
    out: &mut Vec<FactoryAssignedValue>,
) {
    match stmt {
        Statement::ExpressionStatement(expr_stmt) => {
            collect_self_scope_assignment_expr(&expr_stmt.expression, target, out);
        }
        Statement::BlockStatement(block) => {
            collect_self_scope_assignments(&block.body, target, out);
        }
        Statement::LabeledStatement(s) => {
            collect_self_scope_assignments_in_statement(&s.body, target, out);
        }
        // Lazy-singleton init: the `if (!target)` guard guarantees `target` is
        // assigned when it was unset, so the consequent's assignment dominates.
        // Only the consequent (the guarded branch) is inspected.
        Statement::IfStatement(s) if if_test_is_falsy_guard_on(&s.test, target) => {
            collect_self_scope_assignments_in_statement(&s.consequent, target, out);
        }
        _ => {}
    }
}

/// Whether an `if` test is a falsiness guard on `id` for the lazy-init pattern
/// `if (!api) { api = ... }`. An uninitialized typed module local holds
/// `undefined`, NOT `null`, so a STRICT `=== null` guard would never fire and is
/// unsound. Accepted: `!id`; loose `id == null` / `id == undefined` (loose `null`
/// matches `undefined`); strict `id === undefined` (either operand order). A
/// strict `=== null` is intentionally rejected. See #1441 (Part A).
fn if_test_is_falsy_guard_on(test: &Expression<'_>, id: &str) -> bool {
    match test {
        Expression::ParenthesizedExpression(paren) => {
            if_test_is_falsy_guard_on(&paren.expression, id)
        }
        Expression::UnaryExpression(unary) => {
            unary.operator == UnaryOperator::LogicalNot
                && matches!(&unary.argument, Expression::Identifier(arg) if arg.name.as_str() == id)
        }
        Expression::BinaryExpression(bin) => {
            let other = if is_identifier_named(&bin.left, id) {
                &bin.right
            } else if is_identifier_named(&bin.right, id) {
                &bin.left
            } else {
                return false;
            };
            match bin.operator {
                // Loose equality: `== null` and `== undefined` both match an
                // uninitialized (undefined) value.
                BinaryOperator::Equality => is_null_literal(other) || is_undefined(other),
                // Strict: only `=== undefined` matches an uninitialized value.
                BinaryOperator::StrictEquality => is_undefined(other),
                _ => false,
            }
        }
        _ => false,
    }
}

fn is_identifier_named(expr: &Expression<'_>, id: &str) -> bool {
    matches!(expr, Expression::Identifier(ident) if ident.name.as_str() == id)
}

fn is_null_literal(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::NullLiteral(_))
}

fn is_undefined(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Identifier(ident) if ident.name == "undefined")
}

fn collect_self_scope_assignment_expr(
    expr: &Expression<'_>,
    target: &str,
    out: &mut Vec<FactoryAssignedValue>,
) {
    match expr {
        Expression::AssignmentExpression(assign)
            if matches!(assign.operator, AssignmentOperator::Assign)
                && matches!(
                    &assign.left,
                    AssignmentTarget::AssignmentTargetIdentifier(id)
                        if id.name.as_str() == target
                ) =>
        {
            out.push(classify_factory_assigned_value(&assign.right));
        }
        Expression::SequenceExpression(seq) => {
            for inner in &seq.expressions {
                collect_self_scope_assignment_expr(inner, target, out);
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_self_scope_assignment_expr(&paren.expression, target, out);
        }
        _ => {}
    }
}

/// The class a function body returns via `new Class()`: the sole expression of
/// an expression-bodied arrow, or the last top-level `return new Class()` of a
/// block body. Conservative, only a direct `new Class()` is traced (a value
/// first bound to one, or a non-`new` return, is out of scope). See issue #1441.
pub(super) fn function_body_returns_new_class(
    body: &FunctionBody<'_>,
    is_expression_body: bool,
) -> Option<String> {
    if is_expression_body {
        let [Statement::ExpressionStatement(stmt)] = body.statements.as_slice() else {
            return None;
        };
        return new_expression_class_name(&stmt.expression);
    }
    body.statements.iter().rev().find_map(|stmt| {
        let Statement::ReturnStatement(ret) = stmt else {
            return None;
        };
        new_expression_class_name(ret.argument.as_ref()?)
    })
}

/// The class a function body returns when EVERY static return path resolves to
/// `new SameClass()`, the all-paths-unanimous proof. `Some(class)` only when:
/// the body has at least one return, every reachable `return` carries an
/// argument (no bare `return;`), and every such argument is `new <class>()` for
/// the SAME class. A non-`new` return, a bare return, or two different classes
/// abstains. Stricter than `function_body_returns_new_class` (last-return), and
/// required before a factory is exported as cross-module metadata: a wrong
/// cross-module credit is a silent false-negative with a wide blast radius.
/// See issue #1441 (Part A).
pub(super) fn function_body_returns_new_class_unanimous(
    body: &FunctionBody<'_>,
    is_expression_body: bool,
) -> Option<String> {
    if is_expression_body {
        let [Statement::ExpressionStatement(stmt)] = body.statements.as_slice() else {
            return None;
        };
        return new_expression_class_name(&stmt.expression);
    }
    // A body that can fall through to an implicit `undefined` (e.g.
    // `if (flag) return new C()` with no trailing return) does NOT provably
    // return the class on every path, so it must abstain. See #1441 (Part A).
    if !function_body_is_terminal(body, is_expression_body) {
        return None;
    }
    let total_returns = count_returns_in_statements(&body.statements);
    if total_returns == 0 {
        return None;
    }
    let mut args = Vec::new();
    collect_return_args_in_statements(&body.statements, &mut args);
    // A bare `return;` (counted but argless) means a non-instance path exists.
    if args.len() != total_returns {
        return None;
    }
    let mut class: Option<String> = None;
    for arg in args {
        let name = new_expression_class_name(arg)?;
        match &class {
            None => class = Some(name),
            Some(existing) if *existing == name => {}
            Some(_) => return None,
        }
    }
    class
}

/// Whether a function body is guaranteed to return on its terminal path, it
/// cannot fall through to an implicit `undefined`. Conservative: an
/// expression-bodied arrow always returns; a block body qualifies only when its
/// LAST top-level statement is a `return` or `throw`. A branch-only terminal
/// (if/else where both arms return, with no trailing statement) is treated as
/// non-terminal, a safe coverage gap, never an over-credit. Required before a
/// factory may be exported cross-module. See issue #1441 (Part A).
pub(super) fn function_body_is_terminal(body: &FunctionBody<'_>, is_expression_body: bool) -> bool {
    if is_expression_body {
        return true;
    }
    matches!(
        body.statements.last(),
        Some(Statement::ReturnStatement(_) | Statement::ThrowStatement(_))
    )
}

/// Collect the argument of EVERY `return` reachable in `statements` (through
/// control flow, not into nested functions). A bare `return;` contributes
/// nothing, so a shorter result than the return count signals a non-value return
/// path. See issue #1441.
fn collect_return_args_in_statements<'b, 'a>(
    statements: &'b [Statement<'a>],
    out: &mut Vec<&'b Expression<'a>>,
) {
    for_each_return_statement(statements, &mut |ret| {
        if let Some(arg) = ret.argument.as_ref() {
            out.push(arg);
        }
    });
}

/// The bare identifier a function returns as its single, unshadowed result.
/// `Some(id)` only when: an expression-bodied arrow is exactly `id`, or a block
/// body has EXACTLY ONE `return` anywhere (no branching / early returns) whose
/// argument is a bare identifier; AND that identifier is not bound by a parameter
/// or a local declaration in the function. Conservative on purpose: the class is
/// inferred later from a module binding (`let api: RESTApi`), so a shadowed local
/// or a branchy return must abstain rather than credit a class the function does
/// not actually return. Used only when the body does not directly return
/// `new Class()`. See issue #1441 (var-return case).
pub(super) fn function_body_returns_identifier(
    body: &FunctionBody<'_>,
    params: &FormalParameters<'_>,
    is_expression_body: bool,
) -> Option<String> {
    let returned = if is_expression_body {
        let [Statement::ExpressionStatement(stmt)] = body.statements.as_slice() else {
            return None;
        };
        let Expression::Identifier(id) = &stmt.expression else {
            return None;
        };
        id.name.to_string()
    } else {
        if count_returns_in_statements(&body.statements) != 1 {
            return None;
        }
        let Some(Expression::Identifier(id)) = first_return_arg_in_statements(&body.statements)
        else {
            return None;
        };
        id.name.to_string()
    };
    if formal_params_bind_identifier(params, &returned)
        || statements_declare_identifier(&body.statements, &returned)
    {
        return None;
    }
    Some(returned)
}

/// The argument of the single `return` reachable in `statements` (through
/// control flow, not into nested functions). The caller guarantees exactly one
/// return exists; a leading bare `return;` (no argument) is skipped. See #1441.
fn first_return_arg_in_statements<'b, 'a>(
    statements: &'b [Statement<'a>],
) -> Option<&'b Expression<'a>> {
    let mut first = None;
    for_each_return_statement(statements, &mut |ret| {
        if first.is_none()
            && let Some(arg) = ret.argument.as_ref()
        {
            first = Some(arg);
        }
    });
    first
}

/// Whether a parameter list binds `name` (including via destructuring / defaults).
fn formal_params_bind_identifier(params: &FormalParameters<'_>, name: &str) -> bool {
    let binds = |pattern: &BindingPattern<'_>| {
        pattern
            .get_binding_identifiers()
            .into_iter()
            .any(|id| id.name.as_str() == name)
    };
    params.items.iter().any(|p| binds(&p.pattern))
}

/// Whether any local declaration in `statements` (recursively through control
/// flow, not into nested functions) binds `name`, a local that would shadow a
/// module binding of the same name, making a `return name` untrustworthy. #1441.
fn statements_declare_identifier(statements: &[Statement<'_>], name: &str) -> bool {
    statements
        .iter()
        .any(|stmt| statement_declares_identifier(stmt, name))
}

fn statement_declares_identifier(stmt: &Statement<'_>, name: &str) -> bool {
    let binds = |pattern: &BindingPattern<'_>| {
        pattern
            .get_binding_identifiers()
            .into_iter()
            .any(|id| id.name.as_str() == name)
    };
    match stmt {
        Statement::VariableDeclaration(decl) => decl.declarations.iter().any(|d| binds(&d.id)),
        Statement::FunctionDeclaration(func) => {
            func.id.as_ref().is_some_and(|id| id.name.as_str() == name)
        }
        Statement::ClassDeclaration(class) => {
            class.id.as_ref().is_some_and(|id| id.name.as_str() == name)
        }
        Statement::BlockStatement(block) => statements_declare_identifier(&block.body, name),
        Statement::IfStatement(if_stmt) => {
            statement_declares_identifier(&if_stmt.consequent, name)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|a| statement_declares_identifier(a, name))
        }
        Statement::ForStatement(s) => {
            matches!(
                s.init.as_ref(),
                Some(ForStatementInit::VariableDeclaration(decl))
                    if decl.declarations.iter().any(|d| binds(&d.id))
            ) || statement_declares_identifier(&s.body, name)
        }
        Statement::ForInStatement(s) => {
            for_statement_left_binds(&s.left, name) || statement_declares_identifier(&s.body, name)
        }
        Statement::ForOfStatement(s) => {
            for_statement_left_binds(&s.left, name) || statement_declares_identifier(&s.body, name)
        }
        Statement::WhileStatement(s) => statement_declares_identifier(&s.body, name),
        Statement::DoWhileStatement(s) => statement_declares_identifier(&s.body, name),
        Statement::TryStatement(s) => {
            statements_declare_identifier(&s.block.body, name)
                || s.handler.as_ref().is_some_and(|h| {
                    h.param.as_ref().is_some_and(|p| binds(&p.pattern))
                        || statements_declare_identifier(&h.body.body, name)
                })
                || s.finalizer
                    .as_ref()
                    .is_some_and(|f| statements_declare_identifier(&f.body, name))
        }
        Statement::SwitchStatement(s) => s
            .cases
            .iter()
            .any(|case| statements_declare_identifier(&case.consequent, name)),
        Statement::LabeledStatement(s) => statement_declares_identifier(&s.body, name),
        _ => false,
    }
}

/// Whether a `for-in` / `for-of` header binds `name` (loop-variable shadowing).
fn for_statement_left_binds(left: &ForStatementLeft<'_>, name: &str) -> bool {
    match left {
        ForStatementLeft::VariableDeclaration(decl) => decl.declarations.iter().any(|d| {
            d.id.get_binding_identifiers()
                .into_iter()
                .any(|id| id.name.as_str() == name)
        }),
        ForStatementLeft::AssignmentTargetIdentifier(id) => id.name.as_str() == name,
        _ => false,
    }
}
