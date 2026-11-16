//! Local callees that cannot reach a library sink.
//!
//! Some catalogue rows match a callee by name only (`request(url)`,
//! `x.merge(a, b)`). A module can declare its own function of that name. When
//! the body of that function runs no call and writes no member, the call of it
//! cannot send a request or merge objects, so it is not a sink. A local
//! function that runs a call keeps the name match: its body can forward the
//! argument to the real library, and the taint of the argument is visible
//! only at the outer call.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::ScopeFlags;

use super::super::super::ModuleInfoExtractor;
use super::{SinkBinding, unwrap_static_expr};
use crate::js_text::JsText;

/// Finds an expression that can have an effect outside the function: a call,
/// a `new`, a tagged template, a dynamic import, a `delete`, or a write to a
/// member.
#[derive(Default)]
struct EffectFinder {
    found: bool,
}

impl<'a> Visit<'a> for EffectFinder {
    fn visit_call_expression(&mut self, _expr: &CallExpression<'a>) {
        self.found = true;
    }

    fn visit_new_expression(&mut self, _expr: &NewExpression<'a>) {
        self.found = true;
    }

    fn visit_tagged_template_expression(&mut self, _expr: &TaggedTemplateExpression<'a>) {
        self.found = true;
    }

    fn visit_import_expression(&mut self, _expr: &ImportExpression<'a>) {
        self.found = true;
    }

    fn visit_unary_expression(&mut self, expr: &UnaryExpression<'a>) {
        if expr.operator == UnaryOperator::Delete {
            self.found = true;
            return;
        }
        walk::walk_unary_expression(self, expr);
    }

    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        if !matches!(expr.left, AssignmentTarget::AssignmentTargetIdentifier(_)) {
            self.found = true;
            return;
        }
        walk::walk_assignment_expression(self, expr);
    }

    fn visit_update_expression(&mut self, expr: &UpdateExpression<'a>) {
        if !matches!(
            expr.argument,
            SimpleAssignmentTarget::AssignmentTargetIdentifier(_)
        ) {
            self.found = true;
        }
    }
}

/// Whether the function runs no call and writes no member, in its parameters
/// (default values) or its body. A function without a body (an overload or
/// an ambient declaration) is not inert: its implementation is elsewhere.
fn is_inert_function(function: &Function<'_>) -> bool {
    if function.body.is_none() {
        return false;
    }
    let mut finder = EffectFinder::default();
    finder.visit_function(function, ScopeFlags::Function);
    !finder.found
}

fn is_inert_arrow(arrow: &ArrowFunctionExpression<'_>) -> bool {
    let mut finder = EffectFinder::default();
    finder.visit_arrow_function_expression(arrow);
    !finder.found
}

fn is_inert_function_value(value: &Expression<'_>) -> bool {
    match unwrap_static_expr(value) {
        Expression::FunctionExpression(function) => is_inert_function(function),
        Expression::ArrowFunctionExpression(arrow) => is_inert_arrow(arrow),
        _ => false,
    }
}

/// The sink binding of an inert function value, or of an object literal with
/// inert inline methods (`{ merge(a, b) { return a + b; } }`). A later
/// property of the same name removes a method. A spread or a computed key
/// can replace any method, so it removes all earlier methods.
pub(super) fn inert_value_binding(value: &Expression<'_>) -> Option<SinkBinding> {
    if is_inert_function_value(value) {
        return Some(SinkBinding::InertFunction);
    }
    let Expression::ObjectExpression(object) = unwrap_static_expr(value) else {
        return None;
    };
    let mut methods: Vec<String> = Vec::new();
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            methods.clear();
            continue;
        };
        let Some(key) = property.key.static_name().filter(|_| !property.computed) else {
            methods.clear();
            continue;
        };
        let key = key.text().to_string();
        methods.retain(|method| *method != key);
        if property.kind == PropertyKind::Init && is_inert_function_value(&property.value) {
            methods.push(key);
        }
    }
    (!methods.is_empty()).then_some(SinkBinding::InertMethods(methods))
}

impl ModuleInfoExtractor {
    /// Whether the callee is an inert local function, or an inert inline
    /// method of a local object literal (`controller.merge` for
    /// `const controller = { merge() {} }`).
    pub(super) fn callee_is_inert_local_function(&self, callee: &Expression<'_>) -> bool {
        let Some(path) = self.static_callee_path(callee) else {
            return false;
        };
        let (root, rest) = match path.split_once('.') {
            Some((root, rest)) => (root, Some(rest)),
            None => (path.as_str(), None),
        };
        match (self.sink_binding(root), rest) {
            (Some(SinkBinding::InertFunction), None) => true,
            (Some(SinkBinding::InertMethods(methods)), Some(method)) => {
                methods.iter().any(|name| name == method)
            }
            _ => false,
        }
    }

    /// Record the inert function declarations of a statement list. A
    /// function declaration is hoisted, so a call before it also runs the
    /// local body.
    fn record_inert_function_declarations(&mut self, statements: &[Statement<'_>]) {
        for statement in statements {
            let declaration = match statement {
                Statement::ExportDeclaration(export) => Some(&export.declaration),
                _ => statement.as_declaration(),
            };
            let Some(Declaration::FunctionDeclaration(function)) = declaration else {
                continue;
            };
            let Some(id) = function.id.as_ref() else {
                continue;
            };
            if !is_inert_function(function) {
                continue;
            }
            let name = id.name.as_str();
            if let Some(bindings) = self.declaration_sink_bindings(name) {
                bindings.insert(name.to_string(), SinkBinding::InertFunction);
            }
        }
    }

    /// Preseed the declarations of a nested scope, then its inert function
    /// declarations.
    pub(in crate::visitor) fn preseed_scope_declarations(&mut self, statements: &[Statement<'_>]) {
        self.preseed_nested_declarations(statements);
        self.record_inert_function_declarations(statements);
    }

    /// Record the sanitizer functions and the inert function declarations at
    /// module scope.
    pub(in crate::visitor) fn record_program_function_facts(&mut self, program: &Program<'_>) {
        self.record_program_sanitizer_functions(program);
        self.record_inert_function_declarations(&program.body);
    }
}
