//! The exported variable declarations whose `export` keyword exports more
//! than one name.
//!
//! One `export` keyword exports every name of `export const { a, b } = x;`,
//! `export const [a, b] = x;` and `export const a = 1, b = 2;`. `fallow fix`
//! can remove that keyword only when every one of these names is unused, so
//! it must know all the names, also of a nested pattern, a default value or a
//! rest element.

use std::path::Path;

use oxc_ast::ast::{Declaration, Program, Statement};
use oxc_span::{GetSpan, SourceType};

/// One top-level exported variable declaration that binds more than one
/// name or binds its names through a destructuring pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedExportDeclaration {
    /// The byte offset of the `export` keyword.
    pub start: u32,
    /// The byte offset after the end of the declaration.
    pub end: u32,
    /// Every name that the declaration binds, in source order.
    pub names: Vec<String>,
}

/// The shared export declarations of the JavaScript or TypeScript file at
/// `path`.
///
/// Returns `None` when the file is not a plain script file (for example a
/// Vue or Svelte file), or when it does not parse cleanly. The caller then
/// does not know the names that a declaration binds.
#[must_use]
pub fn shared_export_declarations(
    path: &Path,
    source: &str,
) -> Option<Vec<SharedExportDeclaration>> {
    let source_type = SourceType::from_path(path).ok()?;
    fallow_extract::parse_bounded(source, || {
        let allocator = oxc_allocator::Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
        if parsed.fatal_error || parsed.diagnostics.has_errors() {
            return None;
        }
        Some(collect(&parsed.program))
    })
    .flatten()
}

fn collect(program: &Program<'_>) -> Vec<SharedExportDeclaration> {
    program
        .body
        .iter()
        .filter_map(|statement| {
            let Statement::ExportDeclaration(export) = statement else {
                return None;
            };
            let Declaration::VariableDeclaration(variables) = &export.declaration else {
                return None;
            };
            let is_pattern = variables
                .declarations
                .iter()
                .any(|declarator| declarator.id.get_binding_identifier().is_none());
            if variables.declarations.len() < 2 && !is_pattern {
                return None;
            }
            let names = variables
                .declarations
                .iter()
                .flat_map(|declarator| declarator.id.get_binding_identifiers())
                .map(|identifier| identifier.name.to_string())
                .collect();
            let span = export.span();
            Some(SharedExportDeclaration {
                start: span.start,
                end: span.end,
                names,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(path: &str, source: &str) -> Option<Vec<Vec<String>>> {
        shared_export_declarations(Path::new(path), source).map(|declarations| {
            declarations
                .into_iter()
                .map(|declaration| declaration.names)
                .collect()
        })
    }

    #[test]
    fn finds_every_name_of_each_pattern_form() {
        let source = "export const { a = 1, b: { c }, ...rest } = x;\nexport let [d, , [e], ...f] = y;\nexport var g = 1, h = 2;\nexport const single = 1;\nexport function fn() {}\n";
        assert_eq!(
            names("store.ts", source),
            Some(vec![
                vec!["a".to_owned(), "c".to_owned(), "rest".to_owned()],
                vec!["d".to_owned(), "e".to_owned(), "f".to_owned()],
                vec!["g".to_owned(), "h".to_owned()],
            ])
        );
    }

    #[test]
    fn spans_the_full_declaration() {
        let source = "const x = {};\nexport const {\n  a,\n} = x;\n";
        let declarations = shared_export_declarations(Path::new("store.ts"), source).unwrap();
        let declaration = &declarations[0];
        assert_eq!(
            &source[declaration.start as usize..declaration.end as usize],
            "export const {\n  a,\n} = x;"
        );
    }

    #[test]
    fn gives_none_for_a_file_that_is_not_script() {
        assert_eq!(names("store.vue", "export const { a, b } = x;\n"), None);
    }

    #[test]
    fn gives_none_for_a_file_that_does_not_parse() {
        assert_eq!(
            names("store.ts", "export const [a, b] = x;\nexport const = ;\n"),
            None
        );
    }
}
