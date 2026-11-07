//! Declared return types of top-level local functions.

use oxc_ast::ast::*;

use super::super::ModuleInfoExtractor;
use super::super::helpers::{array_element_type_from_type, return_type_element_name};
use super::unwrap_parens;

impl<'a> ModuleInfoExtractor {
    /// Pre-pass over top-level declarations recording each local function's
    /// declared return element class (`Promise<T>` / `T`, non-builtin) keyed by
    /// name, so `promise_all_map_element_type` resolves a map callback's callee
    /// even when it is declared AFTER its consumer (declaration-order
    /// independence: the repro's `createDb` follows `generate`). See issue #1793.
    /// The pass also records the element class of an array return type
    /// (`function all(): Table[]`) for the reads of issue #4313.
    pub(super) fn record_local_function_return_types(&mut self, program: &Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    self.record_local_function_return_type(function);
                }
                Statement::VariableDeclaration(decl) => {
                    for declarator in &decl.declarations {
                        self.record_local_const_function_return_type(declarator);
                    }
                }
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::FunctionDeclaration(function) => {
                        self.record_local_function_return_type(function);
                    }
                    Declaration::VariableDeclaration(decl) => {
                        for declarator in &decl.declarations {
                            self.record_local_const_function_return_type(declarator);
                        }
                    }
                    _ => {}
                },
                Statement::ExportDefaultDeclaration(export) => {
                    if let ExportDefaultDeclarationKind::FunctionDeclaration(function) =
                        &export.declaration
                    {
                        self.record_local_function_return_type(function);
                    }
                }
                _ => {}
            }
        }
    }

    fn record_local_function_return_type(&mut self, function: &Function<'_>) {
        let Some(id) = function.id.as_ref() else {
            return;
        };
        let Some(return_type) = function.return_type.as_deref() else {
            return;
        };
        self.record_local_function_return_annotation(id.name.as_str(), return_type);
    }

    fn record_local_const_function_return_type(&mut self, declarator: &VariableDeclarator<'_>) {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        let return_type = match declarator.init.as_ref() {
            Some(Expression::ArrowFunctionExpression(arrow)) => arrow.return_type.as_deref(),
            Some(Expression::FunctionExpression(function)) => function.return_type.as_deref(),
            _ => None,
        };
        let Some(return_type) = return_type else {
            return;
        };
        self.record_local_function_return_annotation(id.name.as_str(), return_type);
    }

    fn record_local_function_return_annotation(
        &mut self,
        name: &str,
        return_type: &TSTypeAnnotation<'_>,
    ) {
        if let Some(element) = return_type_element_name(&return_type.type_annotation) {
            self.local_function_return_types
                .entry(name.to_string())
                .or_insert(element);
        }
        if let Some(element) = array_element_type_from_type(&return_type.type_annotation) {
            self.local_function_array_return_types
                .entry(name.to_string())
                .or_insert(element);
        }
    }

    /// The element class of a call of a top-level function that declares an
    /// array return type: `Table` for `all()` with `function all(): Table[]`.
    /// A loop over, an iteration callback on, an index into, and a local that
    /// holds such a call then read that class (issue #4313). The arguments do
    /// not change the declared type. A nested declaration or a catch
    /// parameter of the same name hides the function, so it gives no class.
    /// A call in a namespace body can name a function of the namespace, so it
    /// gives no class. A cast (`as`) can name another type, so it gives no
    /// class.
    pub(super) fn local_call_array_element_type(&self, expr: &Expression<'_>) -> Option<String> {
        let call = match expr {
            Expression::ParenthesizedExpression(paren) => {
                return self.local_call_array_element_type(&paren.expression);
            }
            Expression::TSNonNullExpression(non_null) => {
                return self.local_call_array_element_type(&non_null.expression);
            }
            Expression::CallExpression(call) => call,
            _ => return None,
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        let name = callee.name.as_str();
        // `nested_declaration_stack` records neither a catch parameter nor a
        // declaration in a namespace body, so both checks stay explicit.
        // `namespace_depth` counts exported namespace bodies, and
        // `local_namespace_depth` counts the bodies of the other namespaces.
        if self.namespace_depth > 0
            || self.local_namespace_depth > 0
            || self.is_catch_param_name(name)
            || self.nested_scope_shadows(name)
        {
            return None;
        }
        self.local_function_array_return_types.get(name).cloned()
    }

    /// The element class of an index into a call of a top-level function that
    /// declares an array return type: `Table` for `all()[0]` or `all()[i]`.
    /// A string key reads a property of the array, not an element.
    pub(super) fn local_call_array_index_type(&self, expr: &Expression<'_>) -> Option<String> {
        let Expression::ComputedMemberExpression(index) = unwrap_parens(expr) else {
            return None;
        };
        if matches!(
            index.expression,
            Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
        ) {
            return None;
        }
        self.local_call_array_element_type(&index.object)
    }
}
