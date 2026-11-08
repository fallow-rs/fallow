//! Instance bindings of `this.<member> = ...` assignments.

use oxc_ast::ast::*;

use super::super::ModuleInfoExtractor;
use crate::MemberAccess;

impl ModuleInfoExtractor {
    /// Handle `this.member = ...` / `this.#member = ...` assignments: record the
    /// member access and propagate the instance-binding target name from a
    /// `new Class()` RHS, an identifier bound to a known class, and nested
    /// binding targets. `member_name` is the property spelling (`dep` for a
    /// public field, `#dep` for a `#`-private field, issue #1821), so the
    /// recorded key is `this.<member_name>` and matches the receiver key
    /// `static_member_object_name` produces for the same field.
    pub(super) fn handle_this_member_assignment(
        &mut self,
        member_name: &str,
        expr: &AssignmentExpression<'_>,
    ) {
        self.member_accesses.push(MemberAccess {
            object: "this".to_string(),
            member: member_name.to_string(),
        });
        if let Expression::NewExpression(new_expr) = &expr.right
            && let Expression::Identifier(callee) = &new_expr.callee
            && !crate::visitor::helpers::is_builtin_constructor(callee.name.as_str())
        {
            let key = self.this_member_key(member_name);
            self.insert_class_binding_target(key, callee.name.to_string());
        } else if let Expression::Identifier(ident) = &expr.right
            && let Some(target_name) = self.binding_target_names.get(ident.name.as_str()).cloned()
        {
            let key = self.this_member_key(member_name);
            self.binding_target_names.insert(key, target_name);
        }
        if let Expression::Identifier(ident) = &expr.right {
            let target = self.this_member_key(member_name);
            self.copy_nested_binding_targets(ident.name.as_str(), target.as_str());
        }
    }
}
