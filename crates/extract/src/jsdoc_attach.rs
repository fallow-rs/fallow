//! Binding of a leading JSDoc comment to the export it documents.
//!
//! `Comment.attached_to` is the byte offset of the token after the comment,
//! so a tag on `/** @x */ export const a = 1` attaches to the `export`
//! keyword, while `ExportInfo.span` starts at the identifier `a`. A tag
//! belongs to an export when it attaches to the export itself, or to the
//! start of the innermost export statement that holds the export. A tag on
//! one statement never reaches a later statement, also in a file without
//! semicolons.

use oxc_ast::ast::{Declaration, Program, Statement, TSModuleDeclaration, TSModuleDeclarationBody};
use oxc_span::Span;

/// Spans of the export statements of `program`, sorted by start. Export
/// statements inside TypeScript `namespace` and `declare module` bodies are
/// included, so a tag on a nested export binds to its own statement.
pub fn export_statement_spans(program: &Program<'_>) -> Vec<Span> {
    let mut spans = Vec::new();
    collect_export_statements(&program.body, &mut spans);
    spans.sort_unstable_by_key(|span| span.start);
    spans
}

fn collect_export_statements(statements: &[Statement<'_>], spans: &mut Vec<Span>) {
    for statement in statements {
        match statement {
            Statement::ExportNamedDeclaration(export) => {
                spans.push(export.span);
                if let Some(Declaration::TSModuleDeclaration(module)) = &export.declaration {
                    collect_module_body(module, spans);
                }
            }
            Statement::ExportDefaultDeclaration(export) => spans.push(export.span),
            Statement::TSModuleDeclaration(module) => collect_module_body(module, spans),
            _ => {}
        }
    }
}

fn collect_module_body(module: &TSModuleDeclaration<'_>, spans: &mut Vec<Span>) {
    match &module.body {
        Some(TSModuleDeclarationBody::TSModuleBlock(block)) => {
            collect_export_statements(&block.body, spans);
        }
        Some(TSModuleDeclarationBody::TSModuleDeclaration(inner)) => {
            collect_module_body(inner, spans);
        }
        None => {}
    }
}

/// The attachment offsets a tag may have to belong to the export that starts
/// at `export_start`: the export itself, then the start of the innermost
/// export statement that holds it.
pub fn attachment_offsets(export_start: u32, statements: &[Span]) -> [Option<u32>; 2] {
    let idx = statements.partition_point(|span| span.start <= export_start);
    let statement_start = statements[..idx]
        .iter()
        .rev()
        .find(|span| export_start < span.end)
        .map(|span| span.start);
    [Some(export_start), statement_start]
}

/// Index of the tag that belongs to the export starting at `export_start`.
///
/// `tags` must be sorted by attachment offset with a STABLE sort over
/// comments in source order. When several tagged JSDoc blocks attach to the
/// same offset, the last block before the export wins, on every run.
pub fn tag_index_for_export<T>(
    tags: &[T],
    attached_to: impl Fn(&T) -> u32,
    export_start: u32,
    statements: &[Span],
) -> Option<usize> {
    attachment_offsets(export_start, statements)
        .into_iter()
        .flatten()
        .find_map(|offset| {
            let end = tags.partition_point(|tag| attached_to(tag) <= offset);
            let last = end.checked_sub(1)?;
            (attached_to(&tags[last]) == offset).then_some(last)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_tag_at_an_offset_wins() {
        let tags = [
            (5, "a"),
            (10, "first"),
            (10, "second"),
            (10, "third"),
            (20, "b"),
        ];
        let statements = [Span::new(10, 30)];
        let idx = tag_index_for_export(&tags, |tag| tag.0, 12, &statements);
        assert_eq!(idx.map(|i| tags[i].1), Some("third"));
        assert_eq!(tag_index_for_export(&tags, |tag| tag.0, 5, &[]), Some(0));
        assert_eq!(tag_index_for_export(&tags, |tag| tag.0, 7, &[]), None);

        for before in 0..6 {
            for count in 1..12 {
                let tags: Vec<(u32, usize)> = (0..before)
                    .map(|i| (i as u32, i))
                    .chain((0..count).map(|i| (10, before + i)))
                    .collect();
                let idx = tag_index_for_export(&tags, |tag| tag.0, 10, &[]);
                assert_eq!(
                    idx,
                    Some(before + count - 1),
                    "the last of {count} blocks at one offset wins"
                );
            }
        }
    }

    #[test]
    fn an_export_outside_every_statement_only_matches_itself() {
        let statements = [Span::new(10, 20), Span::new(30, 40)];
        assert_eq!(attachment_offsets(25, &statements), [Some(25), None]);
        assert_eq!(attachment_offsets(5, &statements), [Some(5), None]);
    }

    #[test]
    fn an_export_inside_a_statement_also_matches_the_statement_start() {
        let statements = [Span::new(10, 20), Span::new(30, 40)];
        assert_eq!(attachment_offsets(17, &statements), [Some(17), Some(10)]);
        assert_eq!(attachment_offsets(30, &statements), [Some(30), Some(30)]);
    }

    #[test]
    fn a_nested_export_matches_its_innermost_statement() {
        let statements = [Span::new(0, 100), Span::new(10, 20), Span::new(30, 40)];
        assert_eq!(attachment_offsets(35, &statements), [Some(35), Some(30)]);
        assert_eq!(attachment_offsets(50, &statements), [Some(50), Some(0)]);
    }
}
