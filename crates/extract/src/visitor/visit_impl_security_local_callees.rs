//! Local callees that cannot reach a library sink.
//!
//! Some catalogue rows match a callee by name only (`request(url)`,
//! `x.merge(a, b)`). A module can declare its own function of that name. When
//! the body of that function is shallow-safe, a call of it cannot send a
//! request or pollute a prototype, so it is not a sink. A body is
//! shallow-safe when it has no effect outside the function:
//!
//! - it calls only the shallow builtins of the global `Object` in
//!   [`SHALLOW_OBJECT_BUILTINS`], and `Object.assign` only into a fresh
//!   object. A module that binds the name `Object` has no shallow-safe body;
//! - it writes a member only one level deep (`copy[key] = value`), and only
//!   on a fresh local: a binding that the function declares in its own scope
//!   with an object literal, `Object.assign({}, ...)` or
//!   `Object.create(null)`, and never reassigns. The function has no other
//!   binding of that name, also not in an inner block or a nested function;
//! - it has no `new`, no tagged template, no `import()` and no `delete`.
//!
//! Any other call keeps the name match, also a call of the function itself (a
//! recursive merge). Such a body can forward the argument to the real
//! library, and the taint of the argument is visible only at the outer call.
//!
//! A write can replace the local function at run time: a reassignment
//! (`request = got`), a member write (`utils.merge = _.merge`), or a call that
//! takes the object (`Object.assign(utils, ...)`). The order of the source
//! does not show when such a write runs. The visitor therefore keeps the sink
//! sites of each inert callee until the walk ends, and drops them only when
//! the module never writes the callee root.
//!
//! The module also records the names under which it exports a shallow-safe
//! function. The analysis reads them to clear a call of that function in a
//! module that imports it directly.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{ScopeFlags, ScopeId};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::Cell;

use super::super::super::ModuleInfoExtractor;
use super::{SinkBinding, unwrap_static_expr};
use crate::js_text::JsText;
use fallow_types::extract::ModuleInfo;

/// The `Object` builtins that a shallow-safe body may call. None of them runs
/// user code. Only `Object.assign` writes to an existing object, so it must
/// target a fresh object.
const SHALLOW_OBJECT_BUILTINS: &[&str] = &[
    "assign",
    "create",
    "entries",
    "freeze",
    "fromEntries",
    "getOwnPropertyNames",
    "hasOwn",
    "isFrozen",
    "keys",
    "values",
];

/// The method name of a static `Object.<method>` callee.
fn object_builtin_name<'b>(callee: &'b Expression<'_>) -> Option<&'b str> {
    let Expression::StaticMemberExpression(member) = unwrap_static_expr(callee) else {
        return None;
    };
    let Expression::Identifier(object) = unwrap_static_expr(&member.object) else {
        return None;
    };
    (object.name == "Object").then_some(member.property.name.as_str())
}

fn first_argument_is_object_literal(call: &CallExpression<'_>) -> bool {
    call.arguments
        .first()
        .and_then(Argument::as_expression)
        .is_some_and(|target| matches!(unwrap_static_expr(target), Expression::ObjectExpression(_)))
}

/// Whether `value` creates a new object: an object literal (also with a
/// spread), `Object.assign({}, ...)` or `Object.create(null)`.
fn is_fresh_object(value: &Expression<'_>) -> bool {
    match unwrap_static_expr(value) {
        Expression::ObjectExpression(_) => true,
        Expression::CallExpression(call) => match object_builtin_name(&call.callee) {
            Some("assign") => first_argument_is_object_literal(call),
            Some("create") => {
                call.arguments.len() == 1
                    && matches!(
                        call.arguments[0].as_expression().map(unwrap_static_expr),
                        Some(Expression::NullLiteral(_))
                    )
            }
            _ => false,
        },
        _ => false,
    }
}

/// Collects the fresh locals of one function: the names whose every binding
/// in the function is a declarator with a fresh object in the function scope
/// itself, and that no assignment or update targets. A parameter, a nested
/// function, a catch parameter or a declaration in an inner scope (a block,
/// a loop or a nested function) of the same name removes the name.
///
/// The visitor runs before semantic analysis, so binding symbols are not
/// available. The rule gives the same answer: when the function has only
/// function-scope bindings of a name, every reference to that name inside
/// the function resolves to one of them.
#[derive(Default)]
struct FreshLocals {
    /// Per name: the count of all bindings and the count of fresh declarators.
    bindings: FxHashMap<String, (u32, u32)>,
    reassigned: FxHashSet<String>,
    /// The scope depth of the walk: 1 is the function scope.
    depth: u32,
}

impl FreshLocals {
    fn finish(self) -> FxHashSet<String> {
        let reassigned = self.reassigned;
        self.bindings
            .into_iter()
            .filter(|(name, (all, fresh))| all == fresh && !reassigned.contains(name))
            .map(|(name, _)| name)
            .collect()
    }
}

impl<'a> Visit<'a> for FreshLocals {
    fn enter_scope(&mut self, _flags: ScopeFlags, _scope_id: &Cell<Option<ScopeId>>) {
        self.depth += 1;
    }

    fn leave_scope(&mut self) {
        self.depth -= 1;
    }

    fn visit_binding_identifier(&mut self, id: &BindingIdentifier<'a>) {
        self.bindings.entry(id.name.to_string()).or_default().0 += 1;
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if self.depth == 1
            && let BindingPattern::BindingIdentifier(id) = &declarator.id
            && declarator.init.as_ref().is_some_and(is_fresh_object)
        {
            self.bindings.entry(id.name.to_string()).or_default().1 += 1;
        }
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_assignment_target(&mut self, target: &AssignmentTarget<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(id) = target {
            self.reassigned.insert(id.name.to_string());
            return;
        }
        if matches!(
            target,
            AssignmentTarget::ArrayAssignmentTarget(_)
                | AssignmentTarget::ObjectAssignmentTarget(_)
        ) {
            let mut written = WrittenRoots {
                in_target: true,
                ..WrittenRoots::default()
            };
            written.visit_assignment_target(target);
            self.reassigned.extend(written.names);
        }
        walk::walk_assignment_target(self, target);
    }

    fn visit_update_expression(&mut self, expr: &UpdateExpression<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &expr.argument {
            self.reassigned.insert(id.name.to_string());
        }
        walk::walk_update_expression(self, expr);
    }
}

/// Finds an expression that can have an effect outside the function: a call
/// other than a shallow `Object` builtin, a `new`, a tagged template, a
/// dynamic import, a `delete`, or a member write other than one level deep
/// on a fresh local.
#[derive(Default)]
struct EffectFinder {
    found: bool,
    fresh: FxHashSet<String>,
}

impl EffectFinder {
    fn is_fresh_root(&self, object: &Expression<'_>) -> bool {
        matches!(
            unwrap_static_expr(object),
            Expression::Identifier(id) if self.fresh.contains(id.name.as_str())
        )
    }

    /// Whether a member write target is one level deep on a fresh local.
    fn is_fresh_member_target(&self, target: &SimpleAssignmentTarget<'_>) -> bool {
        match target {
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.is_fresh_root(&member.object)
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.is_fresh_root(&member.object)
            }
            _ => false,
        }
    }

    fn is_shallow_builtin_call(&self, call: &CallExpression<'_>) -> bool {
        match object_builtin_name(&call.callee) {
            Some("assign") => {
                first_argument_is_object_literal(call)
                    || call
                        .arguments
                        .first()
                        .and_then(Argument::as_expression)
                        .is_some_and(|target| self.is_fresh_root(target))
            }
            Some(name) => SHALLOW_OBJECT_BUILTINS.contains(&name),
            None => false,
        }
    }
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

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        if !self.is_shallow_builtin_call(expr) {
            self.found = true;
            return;
        }
        for argument in &expr.arguments {
            self.visit_argument(argument);
        }
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
        match &expr.left {
            AssignmentTarget::AssignmentTargetIdentifier(_) => {}
            target => match target.as_simple_assignment_target() {
                Some(simple) if self.is_fresh_member_target(simple) => {}
                _ => {
                    self.found = true;
                    return;
                }
            },
        }
        walk::walk_assignment_expression(self, expr);
    }

    fn visit_update_expression(&mut self, expr: &UpdateExpression<'a>) {
        if !matches!(
            expr.argument,
            SimpleAssignmentTarget::AssignmentTargetIdentifier(_)
        ) && !self.is_fresh_member_target(&expr.argument)
        {
            self.found = true;
            return;
        }
        walk::walk_update_expression(self, expr);
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
    /// Whether the module binds the name `Object` (an import, a declaration
    /// or a parameter). `Object.assign` then can be a project function, so no
    /// body of the module is shallow-safe.
    binds_object: bool,
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
    fn visit_binding_identifier(&mut self, id: &BindingIdentifier<'a>) {
        self.binds_object |= id.name == "Object";
    }

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

/// Whether the function body and its parameter defaults are shallow-safe
/// (see the module docs). A function without a body (an overload or an
/// ambient declaration) is not inert: its implementation is elsewhere.
fn is_inert_function(function: &Function<'_>) -> bool {
    if function.body.is_none() {
        return false;
    }
    let mut locals = FreshLocals::default();
    locals.visit_function(function, ScopeFlags::Function);
    let mut finder = EffectFinder {
        fresh: locals.finish(),
        ..EffectFinder::default()
    };
    finder.visit_function(function, ScopeFlags::Function);
    !finder.found
}

fn is_inert_arrow(arrow: &ArrowFunctionExpression<'_>) -> bool {
    let mut locals = FreshLocals::default();
    locals.visit_arrow_function_expression(arrow);
    let mut finder = EffectFinder {
        fresh: locals.finish(),
        ..EffectFinder::default()
    };
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

/// The value that a module-scope declaration binds to a name.
enum DeclaredValue<'d, 'a> {
    Function(&'d Function<'a>),
    Init(Option<&'d Expression<'a>>),
}

impl DeclaredValue<'_, '_> {
    fn is_inert(&self) -> bool {
        match self {
            Self::Function(function) => is_inert_function(function),
            Self::Init(init) => init.is_some_and(is_inert_function_value),
        }
    }
}

/// The names that a function or variable declaration binds. A function
/// without a body (an overload signature) binds no value.
fn declared_bindings<'d, 'a>(
    declaration: &'d Declaration<'a>,
) -> Vec<(&'d str, DeclaredValue<'d, 'a>)> {
    match declaration {
        Declaration::FunctionDeclaration(function) if function.body.is_some() => function
            .id
            .iter()
            .map(|id| (id.name.as_str(), DeclaredValue::Function(function)))
            .collect(),
        Declaration::VariableDeclaration(variable) => variable
            .declarations
            .iter()
            .filter_map(|declarator| match &declarator.id {
                BindingPattern::BindingIdentifier(id) => Some((
                    id.name.as_str(),
                    DeclaredValue::Init(declarator.init.as_ref()),
                )),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The module-scope functions with a shallow-safe body: function
/// declarations and variables bound to a function or arrow expression. A
/// name with a second, non-inert binding is left out.
fn module_inert_functions(program: &Program<'_>) -> FxHashSet<String> {
    let mut inert = FxHashSet::default();
    let mut other = FxHashSet::default();
    for statement in &program.body {
        let declaration = match statement {
            Statement::ExportDeclaration(export) => Some(&export.declaration),
            _ => statement.as_declaration(),
        };
        for (name, value) in declaration.map(declared_bindings).unwrap_or_default() {
            let names = if value.is_inert() {
                &mut inert
            } else {
                &mut other
            };
            names.insert(name.to_string());
        }
    }
    inert.retain(|name| !other.contains(name));
    inert
}

/// A shallow-safe default export and the local name behind it, if any.
enum InertDefault {
    Anonymous,
    Local(String),
}

/// The shallow-safe default export, or `None` when the default export is not
/// shallow-safe.
fn inert_default_export(
    declaration: &ExportDefaultDeclarationKind<'_>,
    locals: &FxHashSet<String>,
) -> Option<InertDefault> {
    if let ExportDefaultDeclarationKind::FunctionDeclaration(function) = declaration {
        return is_inert_function(function).then(|| {
            function.id.as_ref().map_or(InertDefault::Anonymous, |id| {
                InertDefault::Local(id.name.to_string())
            })
        });
    }
    let value = declaration.as_expression()?;
    if is_inert_function_value(value) {
        return Some(InertDefault::Anonymous);
    }
    match unwrap_static_expr(value) {
        Expression::Identifier(id) if locals.contains(id.name.as_str()) => {
            Some(InertDefault::Local(id.name.to_string()))
        }
        _ => None,
    }
}

/// The export names of the module's shallow-safe functions, each with the
/// local name that a write can replace. An anonymous default export has no
/// local name. A re-export (`export { merge } from "./merge"`) is not
/// recorded: the analysis follows one import hop only.
fn inert_export_candidates(program: &Program<'_>) -> Vec<(String, Option<String>)> {
    let locals = module_inert_functions(program);
    let mut exports = Vec::new();
    for statement in &program.body {
        match statement {
            Statement::ExportDeclaration(export) => {
                for (name, _) in declared_bindings(&export.declaration) {
                    if locals.contains(name) {
                        exports.push((name.to_string(), Some(name.to_string())));
                    }
                }
            }
            Statement::ExportNamedDeclaration(export) if !export.export_kind.is_type() => {
                for specifier in &export.specifiers {
                    let local = specifier.local.name();
                    if !specifier.export_kind.is_type() && locals.contains(local.as_str()) {
                        exports.push((
                            specifier.exported.name().to_string(),
                            Some(local.to_string()),
                        ));
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => {
                if let Some(inert) = inert_default_export(&export.declaration, &locals) {
                    let local = match inert {
                        InertDefault::Anonymous => None,
                        InertDefault::Local(name) => Some(name),
                    };
                    exports.push(("default".to_string(), local));
                }
            }
            _ => {}
        }
    }
    exports
}

impl ModuleInfoExtractor {
    /// Append the security facts of this extractor to `info`.
    pub(in crate::visitor) fn merge_security_info(&mut self, info: &mut ModuleInfo) {
        info.security_sinks.append(&mut self.security_sinks);
        info.security_sinks_skipped += self.security_sinks_skipped;
        info.security_unresolved_callee_sites
            .append(&mut self.security_unresolved_callee_sites);
        info.inert_function_exports
            .append(&mut self.inert_function_exports);
        info.tainted_bindings.append(&mut self.tainted_bindings);
        info.sanitized_sink_args
            .append(&mut self.sanitized_sink_args);
        info.security_control_sites
            .append(&mut self.security_control_sites);
    }

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
    /// the module never writes, and record the inert function exports.
    pub(in crate::visitor) fn walk_program_and_finish_inert_callees(
        &mut self,
        program: &Program<'_>,
    ) {
        walk::walk_program(self, program);
        let deferred = std::mem::take(&mut self.sink_aliases.inert_callee_sinks);
        let exports = inert_export_candidates(program);
        if deferred.is_empty() && exports.is_empty() {
            return;
        }
        let mut written = WrittenRoots::default();
        written.visit_program(program);
        if written.binds_object {
            return;
        }
        self.inert_function_exports = exports
            .into_iter()
            .filter(|(_, local)| {
                local
                    .as_ref()
                    .is_none_or(|local| !written.names.contains(local))
            })
            .map(|(exported, _)| exported)
            .collect();
        if deferred.is_empty() {
            return;
        }
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
