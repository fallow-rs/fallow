//! Calls of a member of `this` (`this.query(x)`) that can reach a library
//! sink.
//!
//! A `this.m(x)` call runs code of the project in most cases: a method of the
//! class itself, a method of a local base class, a method of an options
//! object, or a function on a prototype. Such a call is not a library sink of
//! the same name, so it gives no sink site.
//!
//! One case keeps the name match: the `this` of the call is the instance of a
//! class whose `extends` clause names no class of the file, and the class does
//! not declare `m` itself (`this.$queryRawUnsafe(x)` in a subclass of a
//! database client, `this.insertAdjacentHTML(x)` in a custom element).
//!
//! The `this` of a call belongs to the nearest function that binds `this`. A
//! class method, accessor, constructor, field initializer or static block
//! binds the class instance. A `function` or a method of an object literal
//! binds another value. An arrow function binds no `this` of its own.
//!
//! The visitor keeps one frame per class body: its span, and whether it has an
//! `extends` clause. The member names of the class and the classes of the file
//! are read only after the walk, and only for a class whose body has such a
//! call.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};

use super::super::super::ModuleInfoExtractor;
use super::canonical_call::CanonicalCall;
use crate::js_text::JsText;

/// The `this` frame of a class body.
#[derive(Debug, Clone, Copy)]
pub(super) struct ThisClassFrame {
    span: Span,
    has_extends: bool,
}

/// How a call relates to the `this` of its class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThisMemberCall {
    /// The callee is not a member of `this` itself.
    NotThisMember,
    /// The call runs code of the project. It gives no sink site.
    Project,
    /// The call is on the instance of the class with this body span, and the
    /// class extends another class. The analysis after the walk decides.
    InheritedCandidate(Span),
}

/// Sink sites of one `this.m(x)` call that the walk keeps until the class
/// facts of the file are known.
pub(super) struct ThisMemberSinks {
    class_span: Span,
    method: String,
    start: usize,
    end: usize,
}

/// What the file says about one class that has a pending `this.m(x)` call.
struct ClassFacts {
    /// The identifier that the `extends` clause names, if any.
    base: Option<String>,
    members: Vec<String>,
}

/// Collects the class names of the file and the facts of the wanted classes.
struct FileClasses<'w> {
    wanted: &'w FxHashSet<Span>,
    names: FxHashSet<String>,
    facts: FxHashMap<Span, ClassFacts>,
}

impl<'a> Visit<'a> for FileClasses<'_> {
    fn visit_class(&mut self, class: &Class<'a>) {
        if let Some(id) = &class.id {
            self.names.insert(id.name.to_string());
        }
        if self.wanted.contains(&class.span) {
            let base = match class.heritage.as_ref().map(|heritage| &heritage.expression) {
                Some(Expression::Identifier(ident)) => Some(ident.name.to_string()),
                _ => None,
            };
            self.facts.insert(
                class.span,
                ClassFacts {
                    base,
                    members: class_member_names(class),
                },
            );
        }
        walk::walk_class(self, class);
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let (BindingPattern::BindingIdentifier(id), Some(Expression::ClassExpression(_))) =
            (&declarator.id, declarator.init.as_ref())
        {
            self.names.insert(id.name.to_string());
        }
        walk::walk_variable_declarator(self, declarator);
    }
}

/// The static names of the members that a class body declares.
fn class_member_names(class: &Class<'_>) -> Vec<String> {
    class
        .body
        .body
        .iter()
        .filter_map(|element| match element {
            ClassElement::MethodDefinition(method) => method.key.static_name(),
            ClassElement::PropertyDefinition(property) => property.key.static_name(),
            ClassElement::AccessorProperty(accessor) => accessor.key.static_name(),
            ClassElement::StaticBlock(_) | ClassElement::TSIndexSignature(_) => None,
        })
        .map(|name| name.text().into_owned())
        .collect()
}

impl ModuleInfoExtractor {
    /// Walk a class body with its `this` frame.
    pub(in crate::visitor) fn walk_class_with_this_frame(&mut self, class: &Class<'_>) {
        self.sink_aliases.this_classes.push(ThisClassFrame {
            span: class.span,
            has_extends: class.heritage.is_some(),
        });
        walk::walk_class(self, class);
        self.sink_aliases.this_classes.pop();
    }

    /// How the call relates to the `this` of its class (see the module docs).
    pub(super) fn this_member_call(&self, call: &CanonicalCall<'_, '_>) -> ThisMemberCall {
        if !call.receiver.is_own_member() {
            return ThisMemberCall::NotThisMember;
        }
        match self.sink_aliases.this_classes.last() {
            Some(frame) if frame.has_extends && self.independent_this_depth == 0 => {
                ThisMemberCall::InheritedCandidate(frame.span)
            }
            _ => ThisMemberCall::Project,
        }
    }

    /// Keep the sink sites from `start` to the current end until the walk
    /// ends. They belong to the call `this.<method>(...)` in the class with
    /// the body span `class_span`.
    pub(super) fn defer_this_member_sinks(&mut self, class_span: Span, method: &str, start: usize) {
        let end = self.security_sinks.len();
        if end > start {
            self.sink_aliases.this_member_sinks.push(ThisMemberSinks {
                class_span,
                method: method.to_string(),
                start,
                end,
            });
        }
    }

    /// Mark the deferred sink sites of `this.m(x)` calls that run code of the
    /// project: the class declares `m`, or its `extends` clause names a class
    /// of the file.
    pub(super) fn mark_project_this_member_sinks(
        &mut self,
        program: &Program<'_>,
        dropped: &mut [bool],
    ) {
        let pending = std::mem::take(&mut self.sink_aliases.this_member_sinks);
        if pending.is_empty() {
            return;
        }
        let wanted: FxHashSet<Span> = pending.iter().map(|sinks| sinks.class_span).collect();
        let mut classes = FileClasses {
            wanted: &wanted,
            names: FxHashSet::default(),
            facts: FxHashMap::default(),
        };
        classes.visit_program(program);
        for sinks in &pending {
            let Some(facts) = classes.facts.get(&sinks.class_span) else {
                continue;
            };
            let declared = facts.members.contains(&sinks.method);
            let local_base = facts
                .base
                .as_ref()
                .is_some_and(|base| classes.names.contains(base));
            if declared || local_base {
                for flag in dropped.iter_mut().take(sinks.end).skip(sinks.start) {
                    *flag = true;
                }
            }
        }
    }
}
