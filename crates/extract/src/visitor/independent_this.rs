//! Bare `this.<member>` reads in a function with its own `this` inside a class
//! body (issue #4351).
//!
//! A nested `function` or an object-literal method inside a class method has
//! its own `this`, which is usually not the class instance. A read there gives
//! no file-level `this` credit, unless the code gives the function the
//! instance: `.bind(this)`, `.call(this)`, `.apply(this)`, a call that also
//! passes `this` (`items.forEach(function () {}, this)`), an assignment to a
//! `this.<field>` or a class field, or a `this` parameter with a named type.

use fallow_types::extract::MemberAccess;
use oxc_ast::ast::*;
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};

use super::ModuleInfoExtractor;
use crate::js_text::JsText;

/// Methods that run a function with the `this` of their first argument.
const THIS_BINDING_METHODS: [&str; 3] = ["bind", "call", "apply"];

/// The state of bare `this` reads in functions with their own `this`.
#[derive(Debug, Default)]
pub(super) struct IndependentThisState {
    /// The name of each open function with its own `this` inside a class
    /// body, innermost last. Other code can use the name to give the
    /// function the class instance.
    frames: Vec<Option<String>>,
    pending_reads: Vec<PendingIndependentThisRead>,
    bound_function_spans: FxHashSet<Span>,
    bound_function_names: FxHashSet<String>,
    function_value_names: FxHashMap<Span, String>,
}

/// What `enter_function_this` changed, for `exit_function_this`.
#[derive(Debug, Clone, Copy)]
pub(super) struct FunctionThis {
    independent: bool,
    frame: bool,
}

/// A bare `this.<member>` read that waits for the end of the file. The read
/// gives credit only when the file gives its function the class instance by
/// name.
#[derive(Debug)]
struct PendingIndependentThisRead {
    function_name: Option<String>,
    member: String,
}

impl ModuleInfoExtractor {
    /// Start a function. A function that is not a class method has its own
    /// `this`, unless the code gives it the class instance (an earlier visit
    /// saw that code) or its `this` parameter has a named type.
    pub(super) fn enter_function_this(&mut self, func: &Function<'_>) -> FunctionThis {
        let typed_this = func.this_param.as_ref().is_some_and(|param| {
            param.type_annotation.as_deref().is_some_and(|annotation| {
                matches!(annotation.type_annotation, TSType::TSTypeReference(_))
            })
        });
        let independent = self.class_method_function_span != Some(func.span)
            && !typed_this
            && !self
                .independent_this
                .bound_function_spans
                .contains(&func.span);
        let frame = independent && !self.class_scope_stack.is_empty();
        if independent {
            self.independent_this_depth += 1;
        }
        if frame {
            let name = self
                .independent_this
                .function_value_names
                .get(&func.span)
                .cloned()
                .or_else(|| func.id.as_ref().map(|id| id.name.to_string()));
            self.independent_this.frames.push(name);
        }
        FunctionThis { independent, frame }
    }

    /// End a function that `enter_function_this` started.
    pub(super) fn exit_function_this(&mut self, function_this: FunctionThis) {
        if function_this.frame {
            self.independent_this.frames.pop();
        }
        if function_this.independent {
            self.independent_this_depth -= 1;
        }
    }

    /// Hold a bare `this.<member>` read in a function with its own `this`
    /// inside a class body. Return false when the read is not in such a
    /// function, so the caller records it as usual.
    pub(super) fn defer_independent_this_read(&mut self, object_name: &str, member: &str) -> bool {
        if object_name != "this"
            || self.independent_this_depth == 0
            || self.class_scope_stack.is_empty()
        {
            return false;
        }
        let Some(function_name) = self.independent_this.frames.last().cloned() else {
            return false;
        };
        self.independent_this
            .pending_reads
            .push(PendingIndependentThisRead {
                function_name,
                member: member.to_string(),
            });
        true
    }

    /// Give credit to each held read whose function the file gives the class
    /// instance by name, for example `g.call(this)` after `function g() {}`.
    pub(super) fn resolve_independent_this_reads(&mut self) {
        for read in std::mem::take(&mut self.independent_this.pending_reads) {
            if read
                .function_name
                .as_deref()
                .is_some_and(|name| self.independent_this.bound_function_names.contains(name))
            {
                self.member_accesses.push(MemberAccess {
                    object: "this".to_string(),
                    member: read.member,
                });
            }
        }
    }

    /// Record the functions that a call gives the class instance:
    /// `fn.bind(this)`, `fn.call(this)`, `fn.apply(this)`, and each function
    /// argument of a call that also passes `this`.
    pub(super) fn record_this_binding_call(&mut self, call: &CallExpression<'_>) {
        let passes_this = call
            .arguments
            .iter()
            .any(|argument| matches!(argument, Argument::ThisExpression(_)));
        if !passes_this {
            return;
        }
        if let Expression::StaticMemberExpression(callee) = call.callee.get_inner_expression()
            && THIS_BINDING_METHODS.contains(&callee.property.name.as_str())
            && matches!(call.arguments.first(), Some(Argument::ThisExpression(_)))
        {
            self.record_this_bound_value(&callee.object);
        }
        for argument in &call.arguments {
            match argument {
                Argument::ObjectExpression(object) => self.record_this_bound_methods(object),
                Argument::SpreadElement(_) | Argument::ThisExpression(_) => {}
                _ => {
                    if let Some(expression) = argument.as_expression() {
                        self.record_this_bound_value(expression);
                    }
                }
            }
        }
    }

    /// Record a function value that runs with the class instance as `this`.
    /// An inline function is matched by span, a reference by its last name.
    fn record_this_bound_value(&mut self, value: &Expression<'_>) {
        match value.get_inner_expression() {
            Expression::FunctionExpression(func) => {
                self.independent_this.bound_function_spans.insert(func.span);
            }
            Expression::Identifier(reference) => {
                self.independent_this
                    .bound_function_names
                    .insert(reference.name.to_string());
            }
            Expression::StaticMemberExpression(member) => {
                self.independent_this
                    .bound_function_names
                    .insert(member.property.name.to_string());
            }
            _ => {}
        }
    }

    /// `handler = function () {}`: a call through the class field runs the
    /// function with the instance as `this`.
    pub(super) fn record_this_bound_function(&mut self, value: Option<&Expression<'_>>) {
        if let Some(Expression::FunctionExpression(func)) =
            value.map(Expression::get_inner_expression)
        {
            self.independent_this.bound_function_spans.insert(func.span);
        }
    }

    /// `this.handler = function () {}` runs with the instance as `this` when
    /// code calls `this.handler()`. `obj.run = function () {}` gets the name
    /// `run`.
    pub(super) fn record_member_assigned_function(
        &mut self,
        target: &StaticMemberExpression<'_>,
        value: &Expression<'_>,
    ) {
        if matches!(target.object, Expression::ThisExpression(_)) {
            self.record_this_bound_function(Some(value));
        } else {
            self.record_function_value_name(target.property.name.as_str(), value);
        }
    }

    /// `const g = function () {}` gets the name `g`.
    pub(super) fn record_declarator_function_name(&mut self, declarator: &VariableDeclarator<'_>) {
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Some(init) = &declarator.init
        {
            self.record_function_value_name(id.name.as_str(), init);
        }
    }

    /// `{ run() {} }` and `{ run: function () {} }` get the name `run`.
    pub(super) fn record_property_function_name(&mut self, prop: &ObjectProperty<'_>) {
        if let Some(name) = prop.key.static_name() {
            self.record_function_value_name(&name.text(), &prop.value);
        }
    }

    /// `Object.defineProperty(this, "x", { get() {} })`: the methods of an
    /// object literal next to `this` run with the instance as `this`.
    fn record_this_bound_methods(&mut self, object: &ObjectExpression<'_>) {
        for property in &object.properties {
            if let ObjectPropertyKind::ObjectProperty(property) = property {
                self.record_this_bound_value(&property.value);
            }
        }
    }

    /// Record the name of a function value, so a later `name.call(this)` can
    /// give its held reads credit.
    fn record_function_value_name(&mut self, name: &str, value: &Expression<'_>) {
        if let Expression::FunctionExpression(func) = value.get_inner_expression() {
            self.independent_this
                .function_value_names
                .insert(func.span, name.to_string());
        }
    }
}
