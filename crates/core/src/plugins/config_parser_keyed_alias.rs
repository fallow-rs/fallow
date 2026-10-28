//! Find the changes through a name that an object literal holds under a key.
//!
//! In `const config = { mode: "production", entry }`, `config` holds `entry`
//! under the key `entry`. A write such as `config.entry.admin = x` changes
//! `entry`, but `config.devtool = x` or `config.plugins.push(x)` does not.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use rustc_hash::{FxHashMap, FxHashSet};

use super::call_arguments::CallArguments;
use super::{Scope, mutated_target, target_names};

/// One use of a keyed alias that can change what it holds.
enum Use {
    /// A write to a member, such as `config.entry.admin = x`.
    Write(Vec<Option<String>>),
    /// A mutating call on a member, such as `config.entry.list.push(x)`.
    Mutation(Vec<Option<String>>),
    /// A call argument that a function can change, such as `configure(config)`.
    CallArgument(Vec<Option<String>>),
    /// A use that this module cannot read, such as a destructuring target.
    Unknown,
}

/// The uses of each keyed alias name. Each use holds the member chain after
/// the name, with `None` for a computed key.
pub(super) struct KeyedUses {
    uses: FxHashMap<String, Vec<Use>>,
}

impl KeyedUses {
    pub(super) fn of(scope: Scope<'_, '_>, names: FxHashSet<&str>, calls: &CallArguments) -> Self {
        let mut collector = UseCollector {
            names,
            calls,
            uses: FxHashMap::default(),
        };
        if !collector.names.is_empty() {
            scope.walk(&mut collector);
        }
        Self {
            uses: collector.uses,
        }
    }

    /// Whether the uses of `name` change, or pass to a call, the value that
    /// `name` holds under `path`. A computed key matches each key.
    pub(super) fn changes(&self, name: &str, path: &[String]) -> (bool, bool) {
        let mut changed = false;
        let mut passed = false;
        for found in self.uses.get(name).into_iter().flatten() {
            match found {
                Use::Write(chain) => changed |= follows(chain, path) && chain.len() > path.len(),
                Use::Mutation(chain) => {
                    changed |= follows(chain, path) && chain.len() >= path.len();
                }
                Use::CallArgument(chain) => passed |= follows(chain, path),
                Use::Unknown => changed = true,
            }
        }
        (changed, passed)
    }
}

/// Whether `chain` and `path` agree on each key that both have.
fn follows(chain: &[Option<String>], path: &[String]) -> bool {
    chain
        .iter()
        .zip(path)
        .all(|(key, expected)| key.as_ref().is_none_or(|key| key == expected))
}

struct UseCollector<'n, 'c> {
    names: FxHashSet<&'n str>,
    calls: &'c CallArguments,
    uses: FxHashMap<String, Vec<Use>>,
}

impl UseCollector<'_, '_> {
    fn record(&mut self, expr: &Expression<'_>, make: fn(Vec<Option<String>>) -> Use) {
        if let Some((root, chain)) = member_chain(expr)
            && self.names.contains(root)
        {
            self.uses
                .entry(root.to_owned())
                .or_default()
                .push(make(chain));
        }
    }

    fn record_member(&mut self, member: &MemberExpression<'_>) {
        let Some((root, mut chain)) = member_chain(member.object()) else {
            return;
        };
        if !self.names.contains(root) {
            return;
        }
        chain.push(
            member
                .static_property_name()
                .and_then(|name| name.as_str().map(str::to_owned)),
        );
        self.uses
            .entry(root.to_owned())
            .or_default()
            .push(Use::Write(chain));
    }

    fn record_arguments(
        &mut self,
        callee: &Expression<'_>,
        span: Span,
        arguments: &[Argument<'_>],
    ) {
        if !self.calls.counts(callee, span) {
            return;
        }
        for expr in CallArguments::direct_arguments(arguments) {
            self.record(expr, Use::CallArgument);
        }
    }
}

impl<'a> Visit<'a> for UseCollector<'_, '_> {
    fn visit_assignment_expression(&mut self, assign: &AssignmentExpression<'a>) {
        if let Some(member) = assign
            .left
            .as_simple_assignment_target()
            .and_then(SimpleAssignmentTarget::as_member_expression)
        {
            self.record_member(member);
        } else if assign.left.as_assignment_target_pattern().is_some() {
            for name in target_names(&assign.left) {
                if self.names.contains(name.as_str()) {
                    self.uses.entry(name).or_default().push(Use::Unknown);
                }
            }
        }
        walk::walk_assignment_expression(self, assign);
    }

    fn visit_update_expression(&mut self, expression: &UpdateExpression<'a>) {
        if let Some(member) = expression.argument.as_member_expression() {
            self.record_member(member);
        }
        walk::walk_update_expression(self, expression);
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        if expression.operator == UnaryOperator::Delete
            && let Some(member) = expression.argument.as_member_expression()
        {
            self.record_member(member);
        }
        walk::walk_unary_expression(self, expression);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(target) = mutated_target(call) {
            self.record(target, Use::Mutation);
        }
        self.record_arguments(&call.callee, call.span, &call.arguments);
        walk::walk_call_expression(self, call);
    }

    fn visit_new_expression(&mut self, expression: &NewExpression<'a>) {
        self.record_arguments(&expression.callee, expression.span, &expression.arguments);
        walk::walk_new_expression(self, expression);
    }
}

/// The root name of a member chain and its keys in source order, such as
/// `config` and `[entry, admin]` for `config.entry.admin`. A computed key is
/// `None`.
fn member_chain<'e>(expr: &'e Expression<'_>) -> Option<(&'e str, Vec<Option<String>>)> {
    let mut keys = Vec::new();
    let mut current = expr.without_parentheses().get_inner_expression();
    loop {
        current = match current {
            Expression::Identifier(identifier) => {
                keys.reverse();
                return Some((identifier.name.as_str(), keys));
            }
            Expression::StaticMemberExpression(inner) => {
                keys.push(Some(inner.property.name.to_string()));
                &inner.object
            }
            Expression::ComputedMemberExpression(inner) => {
                keys.push(None);
                &inner.object
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::StaticMemberExpression(inner) => {
                    keys.push(Some(inner.property.name.to_string()));
                    &inner.object
                }
                ChainElement::ComputedMemberExpression(inner) => {
                    keys.push(None);
                    &inner.object
                }
                _ => return None,
            },
            _ => return None,
        }
        .without_parentheses()
        .get_inner_expression();
    }
}
