//! MobX annotation objects that name class members (issue #5297).

use oxc_ast::ast::*;

use crate::MemberAccess;
use crate::js_text::JsText;
use fallow_types::extract::ImportedName;

use super::{ModuleInfoExtractor, unwrap_paren_expr};

/// The package that exports the MobX annotation functions.
const MOBX_SOURCE: &str = "mobx";

/// The MobX functions whose second argument maps member names to annotations.
const MOBX_ANNOTATION_FUNCTIONS: &[&str] = &["makeObservable", "makeAutoObservable"];

impl ModuleInfoExtractor {
    /// Credit each static key of the annotation object in
    /// `makeObservable(this, { member: computed })` and
    /// `makeAutoObservable(this, { member: action.bound })` as a read of
    /// `this.member`.
    ///
    /// MobX reads each named member of the instance when the constructor runs:
    /// it wraps a getter as a computed value and a method as an action. The
    /// callee must be the named import of `mobx`, and the first argument must be
    /// the `this` of the enclosing class.
    pub(super) fn record_mobx_annotation_members(&mut self, call: &CallExpression<'_>) {
        if self.class_scope_stack.is_empty()
            || self.independent_this_depth > 0
            || self.in_static_method
        {
            return;
        }
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if !self.is_mobx_annotation_function(callee.name.as_str()) {
            return;
        }
        let Some(Expression::ThisExpression(_)) = call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .map(unwrap_paren_expr)
        else {
            return;
        };
        let Some(Expression::ObjectExpression(annotations)) = call
            .arguments
            .get(1)
            .and_then(Argument::as_expression)
            .map(unwrap_paren_expr)
        else {
            return;
        };
        for property in &annotations.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            // `member: false` excludes the member from MobX, so MobX does
            // not read it.
            if property.computed
                || matches!(property.value, Expression::BooleanLiteral(ref value) if !value.value)
            {
                continue;
            }
            let Some(name) = property.key.static_name() else {
                continue;
            };
            self.member_accesses.push(MemberAccess {
                object: "this".to_string(),
                member: name.text().into_owned(),
            });
        }
    }

    fn is_mobx_annotation_function(&self, local_name: &str) -> bool {
        if self.local_declaration_names.contains(local_name) {
            return false;
        }
        self.imports.iter().any(|import| {
            import.source == MOBX_SOURCE
                && import.local_name == local_name
                && matches!(
                    &import.imported_name,
                    ImportedName::Named(name) if MOBX_ANNOTATION_FUNCTIONS.contains(&name.as_str())
                )
        })
    }
}
