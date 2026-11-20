//! The import declarations of a file, and how often the code refers to each
//! imported name.
//!
//! `fallow fix` removes a name from an `export { ... }` list. When that name
//! came from an import that nothing else in the file uses, the fix must also
//! remove the import. A kept import still counts as a consumer for the type
//! checker, but not for the analysis, so the next fix run would remove the
//! `export` that the import needs.

use std::path::Path;

use oxc_ast::ast::{IdentifierReference, ImportDeclarationSpecifier, Program, Statement};
use oxc_ast_visit::Visit;
use oxc_span::SourceType;
use rustc_hash::FxHashMap;

/// The form of one binding of an import declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportBindingKind {
    /// `import a from 'x'`
    Default,
    /// `import * as a from 'x'`
    Namespace,
    /// `import { a } from 'x'`, `import { b as a } from 'x'`
    Named,
}

/// One local binding of an import declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    /// The local name.
    pub local: String,
    /// The byte offset of the first token of the specifier.
    pub start: u32,
    /// The byte offset after the last token of the specifier.
    pub end: u32,
    /// The form of the binding.
    pub kind: ImportBindingKind,
}

/// One top-level import declaration with at least one binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDeclarationBindings {
    /// The byte offset of the `import` keyword.
    pub start: u32,
    /// The byte offset after the end of the declaration.
    pub end: u32,
    /// The bindings, in source order.
    pub bindings: Vec<ImportBinding>,
}

/// The import declarations of a file and the references to their names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileImportBindings {
    /// The import declarations with bindings, in source order.
    pub declarations: Vec<ImportDeclarationBindings>,
    /// For each imported local name, the number of identifier references
    /// with that name in the file. A specifier of an `export { ... }` list
    /// without a `from` clause is a reference. The count matches by name, so
    /// a shadowing declaration can only make the count higher.
    pub reference_counts: FxHashMap<String, usize>,
}

/// The import bindings of the JavaScript or TypeScript file at `path`.
///
/// Returns `None` for a Vue, Svelte, Astro or other file type, and when the
/// file does not parse cleanly.
#[must_use]
pub fn import_bindings(path: &Path, source: &str) -> Option<FileImportBindings> {
    if fallow_extract::is_sfc_file(path)
        || path
            .extension()
            .is_some_and(|extension| extension == "astro")
    {
        return None;
    }
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

fn collect(program: &Program<'_>) -> FileImportBindings {
    let declarations: Vec<ImportDeclarationBindings> = program
        .body
        .iter()
        .filter_map(|statement| {
            let Statement::ImportDeclaration(import) = statement else {
                return None;
            };
            let bindings: Vec<ImportBinding> = import
                .specifiers
                .as_ref()?
                .iter()
                .map(|specifier| {
                    let (local, span, kind) = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(it) => {
                            (&it.local, it.span, ImportBindingKind::Named)
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(it) => {
                            (&it.local, it.span, ImportBindingKind::Default)
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(it) => {
                            (&it.local, it.span, ImportBindingKind::Namespace)
                        }
                    };
                    ImportBinding {
                        local: local.name.to_string(),
                        start: span.start,
                        end: span.end,
                        kind,
                    }
                })
                .collect();
            (!bindings.is_empty()).then_some(ImportDeclarationBindings {
                start: import.span.start,
                end: import.span.end,
                bindings,
            })
        })
        .collect();
    let mut reference_counts: FxHashMap<String, usize> = declarations
        .iter()
        .flat_map(|declaration| &declaration.bindings)
        .map(|binding| (binding.local.clone(), 0))
        .collect();
    if !reference_counts.is_empty() {
        let mut counter = ReferenceCounter {
            counts: &mut reference_counts,
        };
        counter.visit_program(program);
    }
    FileImportBindings {
        declarations,
        reference_counts,
    }
}

struct ReferenceCounter<'c> {
    counts: &'c mut FxHashMap<String, usize>,
}

impl<'a> Visit<'a> for ReferenceCounter<'_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(count) = self.counts.get_mut(it.name.as_str()) {
            *count += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings(source: &str) -> FileImportBindings {
        import_bindings(Path::new("barrel.ts"), source).unwrap()
    }

    #[test]
    fn counts_export_list_specifiers_as_references() {
        let source = "import { a } from './a';\nimport { used } from './used';\nused();\nexport { a, used };\n";
        let found = bindings(source);
        assert_eq!(found.reference_counts["a"], 1);
        assert_eq!(found.reference_counts["used"], 2);
        assert_eq!(found.declarations.len(), 2);
        let first = &found.declarations[0];
        assert_eq!(
            &source[first.start as usize..first.end as usize],
            "import { a } from './a';"
        );
        assert_eq!(first.bindings[0].kind, ImportBindingKind::Named);
    }

    #[test]
    fn counts_type_and_jsx_references() {
        let source = "import Box, * as ns from './box';\nimport { type T } from './t';\nconst x: T = 1;\nconst y = <Box />;\nexport { x, y };\n";
        let found = import_bindings(Path::new("view.tsx"), source).unwrap();
        assert_eq!(found.reference_counts["T"], 1);
        assert_eq!(found.reference_counts["Box"], 1);
        assert_eq!(found.reference_counts["ns"], 0);
        let kinds: Vec<_> = found.declarations[0]
            .bindings
            .iter()
            .map(|binding| binding.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![ImportBindingKind::Default, ImportBindingKind::Namespace]
        );
    }

    #[test]
    fn skips_side_effect_imports() {
        assert!(bindings("import './polyfill';\n").declarations.is_empty());
    }

    #[test]
    fn gives_none_for_a_component_file_or_a_parse_error() {
        assert_eq!(
            import_bindings(
                Path::new("a.vue"),
                "<script>import { a } from './a';</script>"
            ),
            None
        );
        assert_eq!(import_bindings(Path::new("a.ts"), "import { from;"), None);
    }
}
