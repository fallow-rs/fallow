//! Reads of non-public env vars for the security `client-server-leak` rule.
//!
//! The rule needs the location of each read: the finding points at the read,
//! and the React Router route split decides per read whether client code
//! makes it. Three forms count:
//!
//! - `process.env.X` and `import.meta.env.X`;
//! - `process.env['X']` with a static key;
//! - `const { X } = process.env`, also with a rename or a default value.
//!
//! A computed key without a static name and a rest element name no var, so
//! they give no read. A public var (`NEXT_PUBLIC_X`) gives no read.

use oxc_ast::ast::{
    BindingPattern, ComputedMemberExpression, Expression, StaticMemberExpression,
    VariableDeclarator,
};
use oxc_span::GetSpan;

use fallow_types::extract::{EnvRead, EnvReadForm, is_public_env_var};

use super::super::super::ModuleInfoExtractor;
use crate::js_text::JsText;

/// `process.env` or `import.meta.env` when `object` is one of them.
fn env_object(object: &Expression<'_>) -> Option<&'static str> {
    let Expression::StaticMemberExpression(env) = object.without_parentheses() else {
        return None;
    };
    if env.property.name != "env" {
        return None;
    }
    match &env.object {
        Expression::Identifier(id) if id.name == "process" => Some("process.env"),
        Expression::ImportMeta(_) => Some("import.meta.env"),
        _ => None,
    }
}

impl ModuleInfoExtractor {
    fn push_env_read(&mut self, object: &str, var: &str, span_start: u32, form: EnvReadForm) {
        if is_public_env_var(var) {
            return;
        }
        self.env_reads.push(EnvRead {
            name: format!("{object}.{var}"),
            span_start,
            form,
        });
    }

    /// Record `process.env.X` and `import.meta.env.X`.
    pub(crate) fn record_dotted_env_read(&mut self, expr: &StaticMemberExpression<'_>) {
        if let Some(object) = env_object(&expr.object) {
            self.push_env_read(
                object,
                &expr.property.name,
                expr.property.span.start,
                EnvReadForm::Dotted,
            );
        }
    }

    /// Record `process.env['X']` with a static key.
    pub(crate) fn record_computed_env_read(&mut self, expr: &ComputedMemberExpression<'_>) {
        let Some(object) = env_object(&expr.object) else {
            return;
        };
        if let Some(key) = expr.static_property_name() {
            self.push_env_read(
                object,
                &key.text(),
                expr.expression.span().start,
                EnvReadForm::Computed,
            );
        }
    }

    /// Record each statically named key of `const { X, Y: y } = process.env`.
    pub(crate) fn record_destructured_env_reads(&mut self, declarator: &VariableDeclarator<'_>) {
        let Some(init) = &declarator.init else {
            return;
        };
        let Some(object) = env_object(init) else {
            return;
        };
        let BindingPattern::ObjectPattern(pattern) = &declarator.id else {
            return;
        };
        for property in &pattern.properties {
            if let Some(name) = property.key.static_name() {
                self.push_env_read(
                    object,
                    &name.text(),
                    property.key.span().start,
                    EnvReadForm::Destructured,
                );
            }
        }
    }
}
