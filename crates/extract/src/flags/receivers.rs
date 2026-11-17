//! Calls of a generic SDK name that go to project code instead of a flag
//! SDK.

use oxc_ast::ast::{
    BindingPattern, CallExpression, Expression, TSType, TSTypeName, VariableDeclarator,
};

use super::{FlagVisitor, unwrap_value};

impl FlagVisitor<'_> {
    /// Whether `call` calls a generic SDK name that the file itself declares
    /// or imports from a module of the project, in a file without a flag
    /// import. Such a call goes to project code, not to a flag SDK.
    ///
    /// A method call on a receiver that holds a value of the file, such as
    /// `manager.isEnabled()` with `manager = new LocalManager()`, also goes
    /// to project code.
    pub(super) fn calls_local_generic_function(
        &self,
        call: &CallExpression<'_>,
        name: &str,
    ) -> bool {
        if self.has_flag_import || !self.is_generic_sdk_name(name) {
            return false;
        }
        match &call.callee {
            Expression::Identifier(_) => self.local_names.contains(name),
            Expression::StaticMemberExpression(member) => match unwrap_value(&member.object) {
                Expression::Identifier(object) => {
                    self.local_receivers.contains(object.name.as_str())
                }
                object => self.is_local_instance(object),
            },
            _ => false,
        }
    }

    /// Whether `expr` is `new C()` for a class `C` from `local_names`.
    fn is_local_instance(&self, expr: &Expression<'_>) -> bool {
        matches!(
            expr,
            Expression::NewExpression(new)
                if matches!(
                    &new.callee,
                    Expression::Identifier(class) if self.local_names.contains(class.name.as_str())
                )
        )
    }

    /// Note a binding whose initializer or type annotation names a value of
    /// the file.
    pub(super) fn note_local_receiver(&mut self, decl: &VariableDeclarator<'_>) {
        if self.has_flag_import {
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &decl.id else {
            return;
        };
        let from_init = decl.init.as_ref().map(unwrap_value).is_some_and(|init| {
            matches!(init, Expression::ObjectExpression(_)) || self.is_local_instance(init)
        });
        let from_type = decl.type_annotation.as_ref().is_some_and(|annotation| {
            matches!(
                &annotation.type_annotation,
                TSType::TSTypeReference(reference)
                    if matches!(
                        &reference.type_name,
                        TSTypeName::IdentifierReference(class)
                            if self.local_names.contains(class.name.as_str())
                    )
            )
        });
        if from_init || from_type {
            self.local_receivers.insert(id.name.to_string());
        }
    }
}
