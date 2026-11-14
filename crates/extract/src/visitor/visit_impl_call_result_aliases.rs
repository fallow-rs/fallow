//! Locals that hold the result of a method call on a bound receiver, or a
//! conditional whose branches are bound receivers.
//!
//! `const rotated = local.rotate(angle)` reads members of the return type of
//! `rotate`, as the chain `local.rotate(angle).translate()` does. The local
//! takes the call path of the receiver class (`Point.rotate()`), and the
//! analysis resolves that path through the declared return type.
//!
//! `const rotated = angle ? local.rotate(angle) : local` can hold either
//! branch. A read on the local is a read on each branch that has a bound
//! class, so each branch gets the access.

use oxc_ast::ast::*;

use super::super::super::{BindingTarget, ModuleInfoExtractor};
use super::super::visit_chain_names::is_member_call;
use super::super::{call_receiver_name, static_member_object_name, unwrap_static_expr};

impl ModuleInfoExtractor {
    /// Bind a local that holds a method call result: `this.m()` through the
    /// frame of the class method, and `local.m()` or a conditional of such
    /// calls through the receiver class.
    pub(in crate::visitor) fn record_method_result_aliases(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        self.record_this_method_result_alias(declarator, init);
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            let init = self.class_carrying_init(init);
            self.record_call_result_receiver_alias(id.name.as_str(), init);
        }
    }

    /// Bind `binding` to the class path of a method call init, or to each
    /// class path of a conditional init. Other inits bind nothing here.
    fn record_call_result_receiver_alias(&mut self, binding: &str, init: &Expression<'_>) {
        let init = unwrap_static_expr(init);
        if !is_member_call(init) && !matches!(init, Expression::ConditionalExpression(_)) {
            return;
        }
        let mut targets = Vec::new();
        self.collect_receiver_alias_targets(init, &mut targets);
        targets.sort_unstable();
        targets.dedup();
        match targets.len() {
            0 => {}
            1 => {
                let target = targets.swap_remove(0);
                self.insert_class_binding_target(binding.to_string(), target);
            }
            _ => self
                .receiver_alias_alternatives
                .entry(binding.to_string())
                .or_default()
                .extend(targets),
        }
    }

    fn collect_receiver_alias_targets(&self, expr: &Expression<'_>, targets: &mut Vec<String>) {
        crate::cost::walk_step();
        let expr = unwrap_static_expr(expr);
        let source = match expr {
            Expression::ConditionalExpression(conditional) => {
                self.collect_receiver_alias_targets(&conditional.consequent, targets);
                self.collect_receiver_alias_targets(&conditional.alternate, targets);
                return;
            }
            // `f ? new A() : new B()` binds no class (see
            // `class_carrying_init`), so a `new` branch gives no target here.
            // A free function call has no receiver class. A call on `this`
            // binds through `record_this_method_result_alias`, which keeps
            // the frame of the class method.
            _ if is_member_call(expr) => call_receiver_name(expr).filter(|source| {
                source
                    .split_once('.')
                    .map_or(source.as_str(), |(root, _)| root)
                    != "this"
            }),
            Expression::CallExpression(_) | Expression::ChainExpression(_) => None,
            _ => static_member_object_name(expr),
        };
        let Some(source) = source else {
            return;
        };
        // An element source (`m.days[0]`) gives no target, as in
        // `record_declarator_receiver_aliases`.
        let source = self.qualify_this_scope(&source);
        if source.contains("[]") {
            return;
        }
        if let Some(BindingTarget::Class(target)) = self.resolve_bound_object_name(&source) {
            targets.push(target);
        }
    }

    /// The class paths that a receiver with an alternative local as its root
    /// reads: `rotated.x` with the alternatives `Point.rotate()` and `Point`
    /// gives `Point.rotate().x` and `Point.x`.
    pub(in crate::visitor) fn receiver_alias_alternative_targets(
        &self,
        object: &str,
    ) -> Vec<String> {
        let root = object.split_once('.').map_or(object, |(root, _)| root);
        let Some(alternatives) = self.receiver_alias_alternatives.get(root) else {
            return Vec::new();
        };
        let rest = &object[root.len()..];
        alternatives
            .iter()
            .map(|target| format!("{target}{rest}"))
            .collect()
    }
}
