//! The wraps that record the arguments of exported functions.
//!
//! With [`crate::MutationOptions::record_exports`], each exported function
//! that holds a kept mutant gets a wrap. The wrap calls the runtime through
//! [`RECORD_FUNCTION`], which gives the function back unchanged unless the
//! runtime records arguments. A wrap needs a function value:
//!
//! - `export function f` and a local function that an `export { f as g }`
//!   list exports get ` ;f = __fallow_m_w("path#name", f);` after the
//!   declaration, on the same line, so the lines of the file stay. A function declaration is a mutable binding, so importers
//!   see the wrap through the live binding.
//! - `export const f = () => {}` and a local `const f = function () {}`
//!   that a list exports get `__fallow_m_w("path#name", ` before the
//!   initializer and `)` after it.
//!
//! A default export declaration, a class, an object literal, an overload
//! signature and a `declare` function get no wrap. A local that an export
//! list names twice gets the wrap of its first name only. `export { f as
//! default }` gives the key `path#default`. A re-export (`export { f } from
//! "./m"`) names no local of the file, so it gives no wrap. An offset inside a mutation site gets no
//! wrap either, because the mutated branches copy the original text without
//! it.

use oxc_ast::ast::{
    Declaration, Expression, Function, ModuleExportName, Program, Statement, VariableDeclaration,
};
use oxc_span::GetSpan;

use crate::emit::Insertion;

use crate::RECORD_FUNCTION;

/// The facts that select the wraps of one text.
pub struct RecordInput<'a> {
    pub path: &'a str,
    /// The spans of the kept mutants.
    pub mutants: &'a [(u32, u32)],
    /// The spans of the emitted sites.
    pub sites: &'a [(u32, u32)],
}

const fn holds(outer: (u32, u32), inner: (u32, u32)) -> bool {
    outer.0 <= inner.0 && inner.1 <= outer.1
}

impl RecordInput<'_> {
    fn has_mutant(&self, span: (u32, u32)) -> bool {
        self.mutants.iter().any(|&mutant| holds(span, mutant))
    }

    /// True when `offset` is strictly inside a site.
    fn inside_site(&self, offset: u32) -> bool {
        self.sites
            .iter()
            .any(|&(start, end)| start < offset && offset < end)
    }

    fn key(&self, name: &str) -> String {
        format!("{}#{name}", self.path)
    }
}

/// A function value with a body: an arrow or a function expression, also
/// inside parentheses or a TypeScript wrapper.
fn is_function_value(expression: &Expression<'_>) -> bool {
    matches!(
        expression.get_inner_expression(),
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    )
}

const fn has_body(function: &Function<'_>) -> bool {
    function.body.is_some() && !function.declare
}

fn span_of(node: &impl GetSpan) -> (u32, u32) {
    let span = node.span();
    (span.start, span.end)
}

/// The wrap of a function declaration: a reassignment after it.
fn reassign(input: &RecordInput<'_>, local: &str, name: &str, end: u32) -> Vec<Insertion> {
    if input.inside_site(end) {
        return Vec::new();
    }
    vec![Insertion {
        offset: end,
        text: format!(
            " ;{local} = {RECORD_FUNCTION}(\"{}\", {local});",
            input.key(name)
        ),
    }]
}

/// The wraps of the declarators of `variables` whose local name `name_of`
/// maps to an export name.
fn wrap_initializers(
    input: &RecordInput<'_>,
    variables: &VariableDeclaration<'_>,
    name_of: impl Fn(&str) -> Option<String>,
) -> Vec<Insertion> {
    let mut insertions = Vec::new();
    for declarator in &variables.declarations {
        let (Some(id), Some(init)) = (declarator.id.get_binding_identifier(), &declarator.init)
        else {
            continue;
        };
        let Some(name) = name_of(id.name.as_str()) else {
            continue;
        };
        let span = span_of(init);
        if !is_function_value(init)
            || !input.has_mutant(span)
            || input.inside_site(span.0)
            || input.inside_site(span.1)
        {
            continue;
        }
        insertions.push(Insertion {
            offset: span.0,
            text: format!("{RECORD_FUNCTION}(\"{}\", ", input.key(&name)),
        });
        insertions.push(Insertion {
            offset: span.1,
            text: ")".to_owned(),
        });
    }
    insertions
}

/// The exported names of the local bindings of `export { a as b }` lists
/// without a source, as (local, exported) pairs.
fn listed_exports(program: &Program<'_>) -> Vec<(String, String)> {
    let name = |name: &ModuleExportName<'_>| name.name().to_string();
    program
        .body
        .iter()
        .filter_map(|statement| match statement {
            Statement::ExportNamedDeclaration(export) if !export.export_kind.is_type() => {
                Some(export)
            }
            _ => None,
        })
        .flat_map(|export| {
            export
                .specifiers
                .iter()
                .filter(|specifier| !specifier.export_kind.is_type())
                .map(|specifier| (name(&specifier.local), name(&specifier.exported)))
        })
        .collect()
}

fn function_wrap(
    input: &RecordInput<'_>,
    function: &Function<'_>,
    exported: impl Fn(&str) -> Option<String>,
) -> Vec<Insertion> {
    let Some(id) = &function.id else {
        return Vec::new();
    };
    let local = id.name.as_str();
    let span = span_of(function);
    match exported(local) {
        Some(name) if has_body(function) && input.has_mutant(span) => {
            reassign(input, local, &name, span.1)
        }
        _ => Vec::new(),
    }
}

/// The insertions of the recording wraps of `program`, sorted by offset.
pub fn record_insertions(program: &Program<'_>, input: &RecordInput<'_>) -> Vec<Insertion> {
    let listed = listed_exports(program);
    let listed_name = |local: &str| {
        listed
            .iter()
            .find(|(candidate, _)| candidate == local)
            .map(|(_, exported)| exported.clone())
    };
    let mut insertions = Vec::new();
    for statement in &program.body {
        match statement {
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::FunctionDeclaration(function) => {
                    insertions.extend(function_wrap(input, function, |local| {
                        Some(local.to_owned())
                    }));
                }
                Declaration::VariableDeclaration(variables) => {
                    insertions.extend(wrap_initializers(input, variables, |local| {
                        Some(local.to_owned())
                    }));
                }
                _ => {}
            },
            Statement::FunctionDeclaration(function) => {
                insertions.extend(function_wrap(input, function, listed_name));
            }
            Statement::VariableDeclaration(variables) => {
                insertions.extend(wrap_initializers(input, variables, listed_name));
            }
            _ => {}
        }
    }
    insertions.sort_by_key(|insertion| insertion.offset);
    insertions
}
