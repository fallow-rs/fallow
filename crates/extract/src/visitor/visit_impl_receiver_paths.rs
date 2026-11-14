//! The parameter paths that a receiver of a structural parameter reads.
//!
//! A receiver is a tracked parameter, or a local alias of a parameter path.
//! A local that holds a method call result on a parameter path
//! (`const rotated = local.rotate(angle)`) is an alias of the call path
//! (`rotate()`). A local with a conditional init is an alias of the path of
//! each branch (issue #4552).

use oxc_ast::ast::*;

use super::super::visit_chain_names::is_member_call;
use super::super::{call_receiver_name, static_member_object_name, unwrap_static_expr};
use super::StructuralParamMemberCollector;
use super::durable_objects::{ReceiverAlias, ResolvedReceiver};

impl StructuralParamMemberCollector {
    /// The tracked parameter and the property path that `object` reads. With
    /// `allow_call_arguments`, a method call with arguments is a `()` link, as
    /// in `call_receiver_name`. Use that form only to record a member access.
    pub(super) fn resolve_receiver_path(
        &self,
        object: &Expression<'_>,
        allow_call_arguments: bool,
    ) -> Option<(String, String)> {
        self.resolve_receiver(object, allow_call_arguments)
            .map(|receiver| (receiver.param, receiver.path))
    }

    /// [`Self::resolve_receiver_path`], and whether the path goes through a
    /// Durable Object stub local. For a local with alternatives, this is the
    /// first path only.
    fn resolve_receiver(
        &self,
        object: &Expression<'_>,
        allow_call_arguments: bool,
    ) -> Option<ResolvedReceiver> {
        self.resolve_receivers(object, allow_call_arguments)
            .into_iter()
            .next()
    }

    /// Each parameter path that `object` can read. A local with a conditional
    /// init gives one path for each branch.
    pub(super) fn resolve_receivers(
        &self,
        object: &Expression<'_>,
        allow_call_arguments: bool,
    ) -> Vec<ResolvedReceiver> {
        let object_path = if allow_call_arguments {
            call_receiver_name(object)
        } else {
            static_member_object_name(object)
        };
        let Some(object_path) = object_path else {
            return Vec::new();
        };
        let (root, suffix) = object_path
            .split_once('.')
            .map_or((object_path.as_str(), ""), |(root, suffix)| (root, suffix));
        for (aliases, shadowed) in self.alias_stack.iter().zip(&self.shadowed_stack).rev() {
            if let Some(alias) = aliases.get(root) {
                return std::iter::once(&alias.target)
                    .chain(&alias.alternatives)
                    .map(|(param, alias_path)| ResolvedReceiver {
                        param: param.clone(),
                        path: join_receiver_path(alias_path, suffix),
                        via_stub: alias.stub,
                    })
                    .collect();
            }
            if shadowed.contains(root) {
                return Vec::new();
            }
        }
        if self.target_params.contains(root) {
            vec![ResolvedReceiver {
                param: root.to_string(),
                path: suffix.to_string(),
                via_stub: false,
            }]
        } else {
            Vec::new()
        }
    }

    /// The parameter paths that a local with the init `init` holds:
    /// `local.rotate(angle)` gives the `rotate()` path of `local`, and a
    /// conditional gives the paths of its branches (issue #4552).
    fn call_result_alias(&self, init: &Expression<'_>) -> Option<ReceiverAlias> {
        let init = unwrap_static_expr(init);
        if !is_member_call(init) && !matches!(init, Expression::ConditionalExpression(_)) {
            return None;
        }
        let mut targets = Vec::new();
        self.collect_call_result_alias_targets(init, &mut targets);
        let mut unique: Vec<(String, String)> = Vec::with_capacity(targets.len());
        for target in targets {
            if !unique.contains(&target) {
                unique.push(target);
            }
        }
        let mut targets = unique.into_iter();
        let target = targets.next()?;
        Some(ReceiverAlias {
            target,
            stub: false,
            alternatives: targets.collect(),
        })
    }

    fn collect_call_result_alias_targets(
        &self,
        expr: &Expression<'_>,
        targets: &mut Vec<(String, String)>,
    ) {
        crate::cost::walk_step();
        let expr = unwrap_static_expr(expr);
        let receivers = match expr {
            Expression::ConditionalExpression(conditional) => {
                self.collect_call_result_alias_targets(&conditional.consequent, targets);
                self.collect_call_result_alias_targets(&conditional.alternate, targets);
                return;
            }
            _ if is_member_call(expr) => self.resolve_receivers(expr, true),
            Expression::CallExpression(_)
            | Expression::ChainExpression(_)
            | Expression::NewExpression(_) => return,
            _ => self.resolve_receivers(expr, false),
        };
        targets.extend(
            receivers
                .into_iter()
                .filter(|receiver| !receiver.via_stub)
                .map(|receiver| (receiver.param, receiver.path)),
        );
    }

    /// The alias that `declarator` makes: a Durable Object stub, or a local
    /// of a method call result or a conditional on parameter paths. A local
    /// that holds a Durable Object id is not a receiver of class members.
    pub(super) fn declarator_receiver_alias(
        &mut self,
        declarator: &VariableDeclarator<'_>,
    ) -> Option<(String, ReceiverAlias)> {
        if let Some(stub) = self.durable_object_stub_alias(declarator) {
            return Some(stub);
        }
        let BindingPattern::BindingIdentifier(local) = &declarator.id else {
            return None;
        };
        if self.durable_objects.holds_id(local.name.as_str()) {
            return None;
        }
        let alias = self.call_result_alias(declarator.init.as_ref()?)?;
        Some((local.name.to_string(), alias))
    }
}

/// The property path of `suffix` read on the alias path `alias_path`.
fn join_receiver_path(alias_path: &str, suffix: &str) -> String {
    match (alias_path.is_empty(), suffix.is_empty()) {
        (true, _) => suffix.to_string(),
        (_, true) => alias_path.to_string(),
        (false, false) => format!("{alias_path}.{suffix}"),
    }
}
