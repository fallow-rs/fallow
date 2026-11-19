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

/// The shared export declarations of the file at `path`.
///
/// A JavaScript or TypeScript file is parsed as a whole. A Vue or Svelte
/// file is parsed per inline `<script>` block and an Astro file per
/// frontmatter block. Each offset is an offset into `source`.
///
/// Returns `None` for another file type, or when a block does not parse
/// cleanly. The caller then does not know the names that a declaration
/// binds.
#[must_use]
pub fn shared_export_declarations(
    path: &Path,
    source: &str,
) -> Option<Vec<SharedExportDeclaration>> {
    if fallow_extract::is_sfc_file(path) {
        let scripts = fallow_extract::extract_sfc_scripts(source);
        return parse_blocks(scripts.iter().filter(|script| script.src.is_none()));
    }
    if path
        .extension()
        .is_some_and(|extension| extension == "astro")
    {
        let frontmatter = fallow_extract::astro::extract_astro_frontmatter(source);
        return parse_blocks(frontmatter.iter());
    }
    let source_type = SourceType::from_path(path).ok()?;
    parse_block(source, source_type, 0)
}

fn parse_blocks<'a>(
    scripts: impl Iterator<Item = &'a fallow_extract::sfc::SfcScript>,
) -> Option<Vec<SharedExportDeclaration>> {
    let mut declarations = Vec::new();
    for script in scripts {
        let source_type = match (script.is_typescript, script.is_jsx) {
            (true, true) => SourceType::tsx(),
            (true, false) => SourceType::ts(),
            (false, true) => SourceType::jsx(),
            (false, false) => SourceType::mjs(),
        };
        let offset = u32::try_from(script.byte_offset).ok()?;
        declarations.extend(parse_block(&script.body, source_type, offset)?);
    }
    Some(declarations)
}

/// Parse one block of script text that starts at byte `offset` of the file.
fn parse_block(
    source: &str,
    source_type: SourceType,
    offset: u32,
) -> Option<Vec<SharedExportDeclaration>> {
    fallow_extract::parse_bounded(source, || {
        let allocator = oxc_allocator::Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
        if parsed.fatal_error || parsed.diagnostics.has_errors() {
            return None;
        }
        Some(collect(&parsed.program, offset))
    })
    .flatten()
}

fn collect(program: &Program<'_>, offset: u32) -> Vec<SharedExportDeclaration> {
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
                start: span.start + offset,
                end: span.end + offset,
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

    /// The text of each declaration that `source` holds.
    fn texts(path: &str, source: &str) -> Vec<String> {
        shared_export_declarations(Path::new(path), source)
            .unwrap()
            .into_iter()
            .map(|declaration| {
                source[declaration.start as usize..declaration.end as usize].to_owned()
            })
            .collect()
    }

    #[test]
    fn reads_each_script_block_of_a_vue_file() {
        let source = "<template><div /></template>\n<script lang=\"ts\">\nexport const c = 1, d = 2;\n</script>\n<script setup lang=\"ts\">\nconst e = 1;\n</script>\n";
        assert_eq!(
            texts("store.vue", source),
            vec!["export const c = 1, d = 2;"]
        );
        assert_eq!(
            names("store.vue", source),
            Some(vec![vec!["c".to_owned(), "d".to_owned()]])
        );
    }

    #[test]
    fn reads_the_module_script_of_a_svelte_file() {
        let source =
            "<script context=\"module\">\n  export const { a, b } = x;\n</script>\n<p>hi</p>\n";
        assert_eq!(
            texts("store.svelte", source),
            vec!["export const { a, b } = x;"]
        );
    }

    #[test]
    fn reads_the_frontmatter_of_an_astro_file() {
        let source = "---\nexport const [a, b] = [1, 2];\n---\n<p>{a}</p>\n";
        assert_eq!(
            texts("page.astro", source),
            vec!["export const [a, b] = [1, 2];"]
        );
    }

    #[test]
    fn gives_none_for_another_file_type() {
        assert_eq!(names("notes.md", "export const { a, b } = x;\n"), None);
    }

    #[test]
    fn gives_none_for_a_file_that_does_not_parse() {
        assert_eq!(
            names("store.ts", "export const [a, b] = x;\nexport const = ;\n"),
            None
        );
    }
}
