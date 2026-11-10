//! The export that a replay calls for one function of a module.

use oxc_ast::ast::{
    BindingPattern, Declaration, ExportDefaultDeclarationKind, Expression, ModuleExportName,
    Program, Statement, VariableDeclaration,
};
use oxc_span::{GetSpan, Span};

use crate::SourceKind;

/// The name of the export whose declaration holds `span`, for example the
/// function of a mutant. A mutant in a nested function gives the export of
/// the outer function. `default` names a default export. A class or an
/// object literal that holds the function is not a function to call, so it
/// gives `None`.
///
/// The lookup reads the top-level statements only: an export declaration
/// (`export function f`, `export const f = () => {}`, `export default`), and
/// a local function or variable that an `export { f as g }` list without a
/// source exports. `None` when the source does not parse or no export holds
/// the span.
pub fn export_containing(source: &str, kind: SourceKind, span: (u32, u32)) -> Option<String> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, kind.source_type()).parse();
    if parsed.fatal_error || parsed.diagnostics.has_errors() {
        return None;
    }
    let target = Span::new(span.0, span.1);
    declared_export(&parsed.program, target).or_else(|| listed_export(&parsed.program, target))
}

const fn holds(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// True for an object literal or a class expression, also inside
/// parentheses or a TypeScript `as`, `satisfies` or `!` wrapper.
fn is_not_callable(expression: &Expression<'_>) -> bool {
    matches!(
        expression.get_inner_expression(),
        Expression::ObjectExpression(_) | Expression::ClassExpression(_)
    )
}

/// The single binding name of a declarator, or `None` for a pattern.
fn binding_name<'a>(pattern: &'a BindingPattern<'_>) -> Option<&'a str> {
    pattern.get_binding_identifier().map(|id| id.name.as_str())
}

/// The name of the declarator of `declaration` whose initializer holds
/// `target`.
fn declarator_holding<'a>(
    declaration: &'a VariableDeclaration<'_>,
    target: Span,
) -> Option<&'a str> {
    declaration.declarations.iter().find_map(|declarator| {
        let init = declarator.init.as_ref()?;
        if is_not_callable(init) {
            return None;
        }
        holds(init.span(), target)
            .then(|| binding_name(&declarator.id))
            .flatten()
    })
}

/// The local name of a top-level declaration that holds `target`.
fn local_holding<'a>(declaration: &'a Declaration<'_>, target: Span) -> Option<&'a str> {
    match declaration {
        Declaration::FunctionDeclaration(function) if holds(function.span, target) => {
            function.id.as_ref().map(|id| id.name.as_str())
        }
        Declaration::VariableDeclaration(variables) => declarator_holding(variables, target),
        _ => None,
    }
}

fn declared_export(program: &Program<'_>, target: Span) -> Option<String> {
    program.body.iter().find_map(|statement| match statement {
        Statement::ExportDeclaration(export) => {
            local_holding(&export.declaration, target).map(str::to_owned)
        }
        Statement::ExportDefaultDeclaration(export) => {
            let span = match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(function) => function.span,
                ExportDefaultDeclarationKind::ClassDeclaration(_)
                | ExportDefaultDeclarationKind::TSInterfaceDeclaration(_)
                | ExportDefaultDeclarationKind::ObjectExpression(_)
                | ExportDefaultDeclarationKind::ClassExpression(_) => return None,
                expression => {
                    if expression.as_expression().is_some_and(is_not_callable) {
                        return None;
                    }
                    expression.span()
                }
            };
            holds(span, target).then(|| "default".to_owned())
        }
        _ => None,
    })
}

fn export_name(name: &ModuleExportName<'_>) -> String {
    name.name().to_string()
}

fn listed_export(program: &Program<'_>, target: Span) -> Option<String> {
    let local = program.body.iter().find_map(|statement| match statement {
        Statement::FunctionDeclaration(function) if holds(function.span, target) => {
            function.id.as_ref().map(|id| id.name.as_str())
        }
        Statement::VariableDeclaration(variables) => declarator_holding(variables, target),
        _ => None,
    })?;
    program.body.iter().find_map(|statement| {
        let Statement::ExportNamedDeclaration(export) = statement else {
            return None;
        };
        export
            .specifiers
            .iter()
            .find(|specifier| export_name(&specifier.local) == local)
            .map(|specifier| export_name(&specifier.exported))
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests expect to keep the setup short")]
mod tests {
    use super::*;

    /// The span of the first occurrence of `needle` in `source`.
    fn span_of(source: &str, needle: &str) -> (u32, u32) {
        let start = source.find(needle).expect("needle in source");
        let start = u32::try_from(start).expect("small source");
        let end = start + u32::try_from(needle.len()).expect("small needle");
        (start, end)
    }

    fn lookup(source: &str, needle: &str) -> Option<String> {
        export_containing(source, SourceKind::Ts, span_of(source, needle))
    }

    #[test]
    fn an_exported_function_declaration_names_itself() {
        let source = "export function add(a: number, b: number) { return a + b; }";
        assert_eq!(lookup(source, "a + b").as_deref(), Some("add"));
    }

    #[test]
    fn an_exported_arrow_function_names_its_binding() {
        let source = "export const sum = (a, b) => a + b, other = 1;";
        assert_eq!(lookup(source, "a + b").as_deref(), Some("sum"));
    }

    #[test]
    fn a_nested_function_gives_the_outer_export() {
        let source = "export function outer(n) { const inner = () => n + 1; return inner(); }";
        assert_eq!(lookup(source, "n + 1").as_deref(), Some("outer"));
    }

    #[test]
    fn a_default_export_is_named_default() {
        let source = "export default function (n) { return n > 0; }";
        assert_eq!(lookup(source, "n > 0").as_deref(), Some("default"));
        let arrow = "export default (n) => n > 0;";
        assert_eq!(lookup(arrow, "n > 0").as_deref(), Some("default"));
    }

    #[test]
    fn an_export_list_gives_the_exported_name_of_the_local() {
        let source =
            "function add(a, b) { return a + b; }\nconst x = 1;\nexport { add as plus, x };";
        assert_eq!(lookup(source, "a + b").as_deref(), Some("plus"));
    }

    #[test]
    fn a_function_that_no_export_holds_gives_none() {
        let source = "function hidden(n) { return n + 1; }\nexport const y = 2;";
        assert_eq!(lookup(source, "n + 1"), None);
        let reexport = "function add(a, b) { return a + b; }\nexport { add } from './other';";
        assert_eq!(lookup(reexport, "a + b"), None);
    }

    #[test]
    fn a_class_or_an_object_that_holds_the_function_is_not_callable() {
        let class = "export class Box { get(n) { return n + 1; } }";
        assert_eq!(lookup(class, "n + 1"), None);
        let object = "export const config = { run(n) { return n + 1; } };";
        assert_eq!(lookup(object, "n + 1"), None);
        let default_object = "export default { run(n) { return n + 1; } };";
        assert_eq!(lookup(default_object, "n + 1"), None);
        let listed = "class Box { get(n) { return n + 1; } }\nexport { Box };";
        assert_eq!(lookup(listed, "n + 1"), None);
        let satisfies = "export const config = { run(n) { return n + 1; } } satisfies object;";
        assert_eq!(lookup(satisfies, "n + 1"), None);
        let constant = "export const config = ({ run(n) { return n + 1; } }) as const;";
        assert_eq!(lookup(constant, "n + 1"), None);
        let wrapped = "export default ({ run(n) { return n + 1; } }) satisfies object;";
        assert_eq!(lookup(wrapped, "n + 1"), None);
        let arrow = "export const run = ((n: number) => n + 1) satisfies object;";
        assert_eq!(lookup(arrow, "n + 1").as_deref(), Some("run"));
    }

    #[test]
    fn a_source_that_does_not_parse_gives_none() {
        assert_eq!(lookup("export function (", "("), None);
    }
}
