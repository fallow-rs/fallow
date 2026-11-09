//! Declared return types of top-level local functions, and the factory
//! returns that a module exports.

use fallow_types::extract::{
    ExportName, FactoryFnMemberAccessFact, FactoryReturnExport, SemanticFact,
};
use oxc_ast::ast::*;

use super::super::ModuleInfoExtractor;
use super::super::helpers::{array_element_type_from_type, return_type_element_name};
use super::unwrap_parens;

/// The receiver suffix of an element of an array: `make()[]`.
const IMPORTED_CALL_ELEMENT_SUFFIX: &str = "[]";

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

    /// Build the cross-module `exported_factory_returns` metadata: join the
    /// strict (all-paths-unanimous) factory map against this module's exports, so
    /// a `const x = useApi()` consumer can credit the returned class across the
    /// boundary. The stored class name is the factory module's own LOCAL name,
    /// resolved at analyze time through this module's imports to the real class
    /// export. Only strict entries qualify, bounding over-credit. Must run after
    /// `resolve_factory_return_aliases` has populated the strict map. See issue
    /// #1441 (Part A).
    pub(in crate::visitor) fn collect_exported_factory_returns(&self) -> Vec<FactoryReturnExport> {
        if self.strict_factory_return_functions.is_empty()
            && self.strict_async_factory_return_functions.is_empty()
            && self.local_function_array_return_types.is_empty()
        {
            return Vec::new();
        }
        let mut out = Vec::new();
        for export in &self.exports {
            if export.is_type_only {
                continue;
            }
            let local_name = match (export.local_name.as_deref(), &export.name) {
                (Some(local), _) => local,
                (None, ExportName::Named(name)) => name.as_str(),
                (None, ExportName::Default) => continue,
            };
            if let Some(class_local_name) = self.strict_factory_return_functions.get(local_name) {
                out.push(FactoryReturnExport {
                    export_name: export.name.to_string(),
                    class_local_name: class_local_name.clone(),
                    is_async: false,
                    returns_array: false,
                });
            } else if let Some(class_local_name) =
                self.strict_async_factory_return_functions.get(local_name)
            {
                out.push(FactoryReturnExport {
                    export_name: export.name.to_string(),
                    class_local_name: class_local_name.clone(),
                    is_async: true,
                    returns_array: false,
                });
            }
            // `export const make = (): Sheet[] => ...`: a consumer in another
            // module reads `Sheet` on an element of `make()`.
            if let Some(class_local_name) = self.local_function_array_return_types.get(local_name) {
                out.push(FactoryReturnExport {
                    export_name: export.name.to_string(),
                    class_local_name: class_local_name.clone(),
                    is_async: false,
                    returns_array: true,
                });
            }
        }
        out
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
        self.local_function_array_return_types
            .get(name)
            .cloned()
            .or_else(|| self.imported_call_element_receiver(name))
    }

    /// The receiver name of an element of a call of an imported function:
    /// `make()[]` for `make()`. The declared return type of `make` is in
    /// another module, so the extraction cannot name the class. A member
    /// read on the element keeps this receiver name, and
    /// `resolve_imported_call_element_accesses` turns it into a fact that
    /// the analysis resolves through the array return type of the export.
    fn imported_call_element_receiver(&self, name: &str) -> Option<String> {
        self.imports
            .iter()
            .any(|import| import.local_name == name && !import.is_type_only)
            .then(|| format!("{name}(){IMPORTED_CALL_ELEMENT_SUFFIX}"))
    }

    /// Emit a factory fact for each member read on an element of a call of
    /// an imported function (`make().forEach((sheet) => sheet.member)`,
    /// `for (const sheet of sheets)` with `const sheets = make()`). The
    /// analysis credits the member only when the export declares an array
    /// return type of a class, so a call of any other import credits nothing.
    pub(in crate::visitor) fn resolve_imported_call_element_accesses(&mut self) {
        let mut facts = Vec::new();
        for access in &self.member_accesses {
            let Some(callee) = access
                .object
                .strip_suffix(IMPORTED_CALL_ELEMENT_SUFFIX)
                .and_then(|call| call.strip_suffix("()"))
            else {
                continue;
            };
            if self
                .imports
                .iter()
                .any(|import| import.local_name == callee && !import.is_type_only)
            {
                facts.push(FactoryFnMemberAccessFact {
                    callee_name: callee.to_string(),
                    member: access.member.clone(),
                    awaited: false,
                    element: true,
                });
            }
        }
        facts.sort_by(|a, b| (&a.callee_name, &a.member).cmp(&(&b.callee_name, &b.member)));
        facts.dedup();
        self.semantic_facts
            .extend(facts.into_iter().map(SemanticFact::FactoryFnMemberAccess));
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
