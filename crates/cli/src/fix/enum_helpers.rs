//! Shared helpers for analyzing whether an exported enum declaration can be
//! safely removed in its entirety. Used by both `exports.rs` (when the export
//! itself is unused) and `enum_members.rs` (when every member of the enum is
//! unused, even if the enum has importers).

use fallow_engine::source_edits::{ExportStatement, ExportStatementKind, SourceEditIndex};

#[derive(Clone, Copy)]
pub(super) struct EnumDeclarationRange {
    pub(super) start_line: usize,
    pub(super) end_line: usize,
}

fn is_ident_char(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphanumeric()
}

fn contains_identifier(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(idx, _)| {
        let before = text[..idx].chars().next_back();
        let after = text[idx + name.len()..].chars().next();
        !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)
    })
}

fn has_identifier_outside_range(lines: &[&str], name: &str, range: EnumDeclarationRange) -> bool {
    lines
        .iter()
        .enumerate()
        .filter(|(idx, _)| *idx < range.start_line || *idx > range.end_line)
        .any(|(_, line)| contains_identifier(line, name))
}

/// The lines of the exported enum `statement` when it is safe to remove the
/// whole statement: no other text shares its lines, and the enum name does
/// not appear outside the statement in the same file. Importers in other
/// files are not consulted; callers must guarantee the enum is dead at the
/// project level before invoking this.
pub(super) fn enum_statement_range(
    index: &SourceEditIndex,
    lines: &[&str],
    statement: &ExportStatement,
) -> Option<EnumDeclarationRange> {
    if statement.kind != ExportStatementKind::Enum
        || !index.owns_its_lines(statement.start, statement.end)
    {
        return None;
    }
    let name = statement.names.first()?;
    let (start_line, end_line) = index.line_range(statement.start, statement.end);
    let range = EnumDeclarationRange {
        start_line,
        end_line,
    };
    (!has_identifier_outside_range(lines, name, range)).then_some(range)
}

/// The lines of the exported enum `enum_name` when it is safe to remove the
/// whole declaration. See [`enum_statement_range`].
pub(super) fn removable_exported_enum_range(
    index: &SourceEditIndex,
    lines: &[&str],
    enum_name: &str,
) -> Option<EnumDeclarationRange> {
    let statement = index.statements().iter().find(|statement| {
        statement.kind == ExportStatementKind::Enum
            && statement
                .names
                .first()
                .is_some_and(|name| name == enum_name)
    })?;
    enum_statement_range(index, lines, statement)
}
