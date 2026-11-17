//! Local callees that cannot reach a library sink.
//!
//! Some catalogue rows match a callee by name only (`request(url)`,
//! `x.merge(a, b)`). A module can declare its own function of that name. When
//! the body of that function runs no call and writes no member, the call of it
//! cannot send a request or merge objects, so it is not a sink. A local
//! function that runs a call keeps the name match: its body can forward the
//! argument to the real library, and the taint of the argument is visible
//! only at the outer call.
//!
//! A write can replace the local function at run time: a reassignment
//! (`request = got`), a member write (`utils.merge = _.merge`), or a call that
//! takes the object (`Object.assign(utils, ...)`). The order of the source
//! does not show when such a write runs. The visitor therefore keeps the sink
//! sites of each inert callee until the walk ends, and drops them only when
//! the module never writes the callee root.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::ScopeFlags;
use rustc_hash::FxHashSet;

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
    fn visit_expression(&mut self, expr: &Expression<'a>) {
        if !self.found {
            walk::walk_expression(self, expr);
        }
    }

    fn visit_statement(&mut self, stmt: &Statement<'a>) {
        if !self.found {
            walk::walk_statement(self, stmt);
        }
    }

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

/// The names that the module writes or hands to a call: an assignment or
/// update target, the root of a written member, and a call or `new`
/// argument. The walk does not track scopes, so a write of a shadowing local
/// also counts, which keeps more sink sites.
#[derive(Default)]
struct WrittenRoots {
    names: FxHashSet<String>,
    in_target: bool,
}

impl WrittenRoots {
    fn record_argument(&mut self, argument: &Argument<'_>) {
        let expr = match argument {
            Argument::SpreadElement(spread) => &spread.argument,
            _ => match argument.as_expression() {
                Some(expr) => expr,
                None => return,
            },
        };
        if let Expression::Identifier(id) = unwrap_static_expr(expr) {
            self.names.insert(id.name.to_string());
        }
    }
}

impl<'a> Visit<'a> for WrittenRoots {
    fn visit_identifier_reference(&mut self, id: &IdentifierReference<'a>) {
        if self.in_target {
            self.names.insert(id.name.to_string());
        }
    }

    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        let outer = std::mem::replace(&mut self.in_target, true);
        self.visit_assignment_target(&expr.left);
        self.in_target = outer;
        self.visit_expression(&expr.right);
    }

    fn visit_update_expression(&mut self, expr: &UpdateExpression<'a>) {
        let outer = std::mem::replace(&mut self.in_target, true);
        self.visit_simple_assignment_target(&expr.argument);
        self.in_target = outer;
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        for argument in &expr.arguments {
            self.record_argument(argument);
        }
        walk::walk_call_expression(self, expr);
    }

    fn visit_new_expression(&mut self, expr: &NewExpression<'a>) {
        for argument in &expr.arguments {
            self.record_argument(argument);
        }
        walk::walk_new_expression(self, expr);
    }
}

/// The sink sites `start..end` of one call of an inert callee named by `root`.
pub(super) struct InertCalleeSinks {
    root: String,
    start: usize,
    end: usize,
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
    /// The root name of the callee when it is an inert local function, or an
    /// inert inline method of a local object literal (`controller` for
    /// `controller.merge` and `const controller = { merge() {} }`).
    pub(super) fn inert_local_callee_root(&self, callee: &Expression<'_>) -> Option<String> {
        let path = self.static_callee_path(callee)?;
        let (root, rest) = match path.split_once('.') {
            Some((root, rest)) => (root, Some(rest)),
            None => (path.as_str(), None),
        };
        let inert = match (self.sink_binding(root), rest) {
            (Some(SinkBinding::InertFunction), None) => true,
            (Some(SinkBinding::InertMethods(methods)), Some(method)) => {
                methods.iter().any(|name| name == method)
            }
            _ => false,
        };
        inert.then(|| root.to_string())
    }

    /// Keep the sink sites from `start` to the current end until the walk
    /// ends. They belong to one call of the inert callee `root`.
    pub(super) fn defer_inert_callee_sinks(&mut self, root: String, start: usize) {
        let end = self.security_sinks.len();
        if end > start {
            self.sink_aliases
                .inert_callee_sinks
                .push(InertCalleeSinks { root, start, end });
        }
    }

    /// Walk the program, then drop the sink sites of the inert callees that
    /// the module never writes.
    pub(in crate::visitor) fn walk_program_and_drop_inert_callee_sinks(
        &mut self,
        program: &Program<'_>,
    ) {
        walk::walk_program(self, program);
        let deferred = std::mem::take(&mut self.sink_aliases.inert_callee_sinks);
        if deferred.is_empty() {
            return;
        }
        let mut written = WrittenRoots::default();
        written.visit_program(program);
        let mut dropped = vec![false; self.security_sinks.len()];
        for sinks in deferred
            .iter()
            .filter(|sinks| !written.names.contains(&sinks.root))
        {
            for flag in dropped.iter_mut().take(sinks.end).skip(sinks.start) {
                *flag = true;
            }
        }
        let mut index = 0;
        self.security_sinks.retain(|_| {
            let keep = !dropped[index];
            index += 1;
            keep
        });
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
