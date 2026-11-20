//! Rewrites of `export { ... }` list statements.
//!
//! A list statement can be on one line or span several lines. A formatter
//! puts each specifier on its own line when the list is longer than the print
//! width, and the analysis then reports each name on the line of its
//! specifier, not on the `export` line. The rewrite works on the full
//! statement text, so both layouts get the same result.

/// Upper bound on the number of lines of one export list statement. The scan
/// stops at this bound, so an unclosed brace cannot make it read a full file.
const MAX_STATEMENT_LINES: usize = 2000;

use rustc_hash::FxHashMap;

/// The line range of one export list statement (both ends inclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ExportListStatement {
    pub(super) start_line: usize,
    pub(super) end_line: usize,
}

/// The new text of a list statement after the removal of some specifiers.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ListRewrite {
    /// No specifier is left: delete each line of the statement.
    DeleteStatement,
    /// Replace the statement lines with these lines. The result never has
    /// more lines than the statement.
    Replace(Vec<String>),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ListRewriteOutcome {
    /// The requested names that the statement exports, so the rewrite
    /// removes them.
    pub(super) removed: Vec<String>,
    /// The local name of each removed specifier, once per specifier. Empty
    /// for a statement with a `from` clause, which binds no local name.
    pub(super) removed_locals: Vec<String>,
    pub(super) rewrite: ListRewrite,
}

/// The export list statements of one file. Each statement is joined and
/// parsed once, however many of its names the analysis reports.
#[derive(Default)]
pub(super) struct ExportListIndex {
    by_start_line: FxHashMap<usize, Option<ParsedStatement>>,
}

struct ParsedStatement {
    statement: ExportListStatement,
    /// The statement lines, joined with `\n`.
    text: String,
    list: ParsedList,
}

impl ExportListIndex {
    /// Find the export list statement that holds the specifier on `line_idx`.
    ///
    /// The nearest line at or above `line_idx` that starts with `export` must
    /// open a list statement whose closing brace is at or below `line_idx`.
    pub(super) fn statement_for_line(
        &mut self,
        lines: &[&str],
        line_idx: usize,
    ) -> Option<ExportListStatement> {
        let lower = line_idx.saturating_sub(MAX_STATEMENT_LINES);
        let start = (lower..=line_idx).rev().find(|start| {
            lines
                .get(*start)
                .is_some_and(|line| starts_with_export_keyword(line.trim_start()))
        })?;
        let statement = self
            .by_start_line
            .entry(start)
            .or_insert_with(|| parse_statement_at(lines, start))
            .as_ref()?
            .statement;
        (statement.end_line >= line_idx).then_some(statement)
    }

    /// Remove the specifiers that export one of `names` from `statement`.
    ///
    /// Returns `None` when the statement does not export any of `names`, or
    /// when no safe text edit exists (for example, other code follows the
    /// statement on its last line and every specifier must go).
    pub(super) fn rewrite(
        &self,
        statement: ExportListStatement,
        names: &[&str],
    ) -> Option<ListRewriteOutcome> {
        let parsed = self
            .by_start_line
            .get(&statement.start_line)?
            .as_ref()
            .filter(|parsed| parsed.statement == statement)?;
        rewrite_parsed(parsed, names)
    }
}

/// Parse the list statement that starts on line `start`.
///
/// The text grows to the next line with a `}` until the parse succeeds, so a
/// short statement never joins the rest of the file. A statement without `;`
/// also needs the next code after it, because a `from` or `with` clause on a
/// later line still belongs to the statement.
fn parse_statement_at(lines: &[&str], start: usize) -> Option<ParsedStatement> {
    let indent = indent_len(lines[start]);
    let limit = lines.len().min(start + MAX_STATEMENT_LINES);
    let mut last = start;
    loop {
        while last < limit && !lines[last].contains('}') {
            last += 1;
        }
        if last >= limit {
            return None;
        }
        let mut context_end = last;
        let text = loop {
            let text = lines[start..=context_end].join("\n");
            let open_end = parse_list(&text, indent)
                .is_some_and(|list| !list.closed && is_trivia_only(&text[list.end..]));
            if !open_end || context_end + 1 >= lines.len() {
                break text;
            }
            context_end += 1;
        };
        if let Some(list) = parse_list(&text, indent) {
            let end_line = start + text[..list.end].matches('\n').count();
            let text = lines[start..=end_line].join("\n");
            return Some(ParsedStatement {
                statement: ExportListStatement {
                    start_line: start,
                    end_line,
                },
                text,
                list,
            });
        }
        last += 1;
    }
}

fn rewrite_parsed(parsed: &ParsedStatement, names: &[&str]) -> Option<ListRewriteOutcome> {
    let text = parsed.text.as_str();
    let specifiers = &parsed.list.specifiers;
    let matched: Vec<bool> = specifiers
        .iter()
        .map(|spec| names.contains(&spec.name.as_str()))
        .collect();

    let mut removed: Vec<String> = Vec::new();
    let mut removed_locals: Vec<String> = Vec::new();
    for (spec, _) in specifiers.iter().zip(&matched).filter(|(_, m)| **m) {
        if !removed.contains(&spec.name) {
            removed.push(spec.name.clone());
        }
        if !parsed.list.has_source {
            removed_locals.push(spec.local.clone());
        }
    }
    if removed.is_empty() {
        return None;
    }

    if matched.iter().all(|m| *m) {
        return is_trivia_only(&text[parsed.list.end..]).then_some(ListRewriteOutcome {
            removed,
            removed_locals,
            rewrite: ListRewrite::DeleteStatement,
        });
    }

    let ranges = removal_ranges(text, specifiers, &matched);
    let mut new_text = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end) in ranges {
        new_text.push_str(&text[cursor..start]);
        cursor = end;
    }
    new_text.push_str(&text[cursor..]);
    Some(ListRewriteOutcome {
        removed,
        removed_locals,
        rewrite: ListRewrite::Replace(new_text.split('\n').map(str::to_owned).collect()),
    })
}

/// Byte ranges of `text` to delete, sorted and merged.
///
/// A line that holds only removed specifiers, commas, white space and
/// comments goes away as a full line, so a list with one specifier per line
/// keeps its layout. Otherwise each removed specifier goes away with its
/// comma. The last specifier has no comma of its own, so its range starts at
/// the end of the nearest kept specifier before it and takes that comma.
fn removal_ranges(text: &str, specifiers: &[Specifier], matched: &[bool]) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut idx = 0;
    while idx < specifiers.len() {
        let line_start = line_start_of(text, specifiers[idx].start);
        let mut group_end = idx;
        while group_end + 1 < specifiers.len()
            && line_start_of(text, specifiers[group_end + 1].start) == line_start
        {
            group_end += 1;
        }
        let group = idx..=group_end;
        if let Some(range) = full_line_range(text, line_start, &specifiers[group.clone()])
            && matched[group.clone()].iter().all(|m| *m)
        {
            ranges.push(range);
        } else {
            for spec_idx in group.filter(|i| matched[*i]) {
                ranges.push(specifier_range(text, specifiers, matched, spec_idx));
            }
        }
        idx = group_end + 1;
    }
    let merged = merge_ranges(ranges);
    let extended = merged
        .into_iter()
        .map(|range| extend_over_orphan_comments(text, range))
        .collect();
    merge_ranges(extended)
}

fn merge_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// Widen a removal of full lines over the comment lines directly above it
/// when those comments would describe nothing.
///
/// A comment line above a specifier can describe that specifier or a group
/// of specifiers. The comments go only when the next line that stays is
/// blank or closes the list. When a kept specifier follows, the comments
/// stay, because they can be the header of the group of that specifier.
fn extend_over_orphan_comments(text: &str, (start, end): (usize, usize)) -> (usize, usize) {
    if start != line_start_of(text, start) || !text[..end].ends_with('\n') {
        return (start, end);
    }
    let next_line_end = text[end..]
        .find('\n')
        .map_or(text.len(), |offset| end + offset);
    let next_line = text[end..next_line_end].trim();
    if !(next_line.is_empty() || next_line.starts_with('}')) {
        return (start, end);
    }
    let mut new_start = start;
    while new_start > 0 {
        let previous_start = line_start_of(text, new_start - 1);
        if previous_start == 0 {
            break;
        }
        let previous = text[previous_start..new_start - 1].trim();
        let is_comment = previous.starts_with("//") || previous.starts_with("/*");
        if !is_comment || !is_trivia_only(previous) {
            break;
        }
        new_start = previous_start;
    }
    (new_start, end)
}

/// The range of the full line that holds `group`, including its line break,
/// when the line holds nothing else than the group and trailing comments.
fn full_line_range(text: &str, line_start: usize, group: &[Specifier]) -> Option<(usize, usize)> {
    let first = group.first()?;
    let last = group.last()?;
    if !text[line_start..first.start].trim().is_empty() {
        return None;
    }
    let after = last.comma.map_or(last.end, |comma| comma + 1);
    let line_end = after + text[after..].find('\n')?;
    is_trivia_only(&text[after..line_end]).then_some((line_start, line_end + 1))
}

fn specifier_range(
    text: &str,
    specifiers: &[Specifier],
    matched: &[bool],
    idx: usize,
) -> (usize, usize) {
    let spec = &specifiers[idx];
    if let Some(comma) = spec.comma {
        let after = comma + 1;
        let end = after + horizontal_space_len(text[after..].bytes());
        let start = if text[end..].starts_with('\n') {
            spec.start - horizontal_space_len(text[..spec.start].bytes().rev())
        } else {
            spec.start
        };
        return (start, end);
    }
    let previous_kept_end = specifiers[..idx]
        .iter()
        .zip(matched)
        .rev()
        .find(|(_, m)| !**m)
        .map_or(spec.start, |(kept, _)| kept.end);
    (previous_kept_end, spec.end)
}

fn horizontal_space_len(bytes: impl Iterator<Item = u8>) -> usize {
    bytes.take_while(|b| *b == b' ' || *b == b'\t').count()
}

fn line_start_of(text: &str, offset: usize) -> usize {
    text[..offset].rfind('\n').map_or(0, |pos| pos + 1)
}

fn indent_len(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn starts_with_export_keyword(trimmed: &str) -> bool {
    trimmed
        .strip_prefix("export")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|next| next.is_whitespace() || next == '{')
}

fn is_trivia_only(text: &str) -> bool {
    let mut cursor = Cursor { text, pos: 0 };
    cursor.skip_trivia() && cursor.pos == text.len()
}

struct Specifier {
    /// Byte offset of the first token.
    start: usize,
    /// Byte offset just past the last token.
    end: usize,
    /// Byte offset of the comma that follows the specifier.
    comma: Option<usize>,
    /// The name that the specifier exports.
    name: String,
    /// The name that the specifier reads: `a` in `a as b`.
    local: String,
}

struct ParsedList {
    specifiers: Vec<Specifier>,
    /// Byte offset just past the statement (after `;` when present).
    end: usize,
    /// True when the statement ends with `;`, so no later text can still
    /// belong to it. Without `;`, a `from` or `with` clause on a later line
    /// is still part of the statement.
    closed: bool,
    /// True when the statement has a `from` clause.
    has_source: bool,
}

/// Parse `export [type] { ... } [from '...' [with { ... }]] [;]` at `start`.
fn parse_list(text: &str, start: usize) -> Option<ParsedList> {
    let mut c = Cursor { text, pos: start };
    if !c.eat_keyword("export") || !c.skip_trivia() {
        return None;
    }
    if c.eat_keyword("type") && !c.skip_trivia() {
        return None;
    }
    if !c.eat('{') {
        return None;
    }
    let mut specifiers = Vec::new();
    loop {
        if !c.skip_trivia() {
            return None;
        }
        if c.eat('}') {
            break;
        }
        specifiers.push(c.parse_specifier()?);
    }
    let after_brace = c.pos;
    if !c.skip_trivia() {
        return None;
    }
    let has_source = c.eat_keyword("from");
    if has_source {
        if !c.skip_trivia() {
            return None;
        }
        c.read_string()?;
        let after_source = c.pos;
        if c.skip_trivia() && (c.eat_keyword("with") || c.eat_keyword("assert")) {
            if !c.skip_trivia() {
                return None;
            }
            c.skip_balanced_braces()?;
        } else {
            c.pos = after_source;
        }
    } else {
        c.pos = after_brace;
    }
    let before_semicolon = c.pos;
    c.skip_horizontal_space();
    let closed = c.eat(';');
    if !closed {
        c.pos = before_semicolon;
    }
    Some(ParsedList {
        specifiers,
        end: c.pos,
        closed,
        has_source,
    })
}

/// The local name of a specifier from its tokens: `a` in `a`, `type a`,
/// `a as b`, or `type a as b`.
fn local_name(tokens: &[String]) -> Option<String> {
    match tokens {
        [name] | [_, name] | [name, _, _] | [_, name, _, _] => Some(name.clone()),
        _ => None,
    }
}

/// The exported name of a specifier from its tokens: `a`, `type a`,
/// `a as b`, or `type a as b`. String names (`"a-b"`) arrive unquoted.
fn exported_name(tokens: &[String]) -> Option<String> {
    match tokens {
        [name] => Some(name.clone()),
        [modifier, name] if modifier == "type" => Some(name.clone()),
        [_, keyword, alias] if keyword == "as" => Some(alias.clone()),
        [modifier, _, keyword, alias] if modifier == "type" && keyword == "as" => {
            Some(alias.clone())
        }
        _ => None,
    }
}

struct Cursor<'a> {
    text: &'a str,
    pos: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn eat_keyword(&mut self, keyword: &str) -> bool {
        let rest = &self.text[self.pos..];
        if !rest.starts_with(keyword) {
            return false;
        }
        if rest[keyword.len()..]
            .chars()
            .next()
            .is_some_and(is_identifier_char)
        {
            return false;
        }
        self.pos += keyword.len();
        true
    }

    /// Skip white space and comments. Returns `false` for a block comment
    /// without an end, because a cut through such a comment breaks the file.
    fn skip_trivia(&mut self) -> bool {
        loop {
            let rest = &self.text[self.pos..];
            if rest.starts_with("//") {
                self.pos += rest.find('\n').unwrap_or(rest.len());
            } else if let Some(body) = rest.strip_prefix("/*") {
                let Some(close) = body.find("*/") else {
                    return false;
                };
                self.pos += close + 4;
            } else if let Some(ch) = rest.chars().next()
                && ch.is_whitespace()
            {
                self.pos += ch.len_utf8();
            } else {
                return true;
            }
        }
    }

    fn skip_horizontal_space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }

    fn read_identifier(&mut self) -> Option<String> {
        let rest = &self.text[self.pos..];
        let len: usize = rest
            .chars()
            .take_while(|ch| is_identifier_char(*ch))
            .map(char::len_utf8)
            .sum();
        if len == 0 {
            return None;
        }
        self.pos += len;
        Some(rest[..len].to_owned())
    }

    /// Read a quoted string literal and return its content without quotes.
    fn read_string(&mut self) -> Option<String> {
        let quote = self.peek().filter(|ch| *ch == '"' || *ch == '\'')?;
        let body_start = self.pos + 1;
        let mut escaped = false;
        for (offset, ch) in self.text[body_start..].char_indices() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '\n' {
                return None;
            } else if ch == quote {
                let body = self.text[body_start..body_start + offset].to_owned();
                self.pos = body_start + offset + 1;
                return Some(body);
            }
        }
        None
    }

    fn skip_balanced_braces(&mut self) -> Option<()> {
        if !self.eat('{') {
            return None;
        }
        let mut depth = 1usize;
        while depth > 0 {
            if !self.skip_trivia() {
                return None;
            }
            match self.peek()? {
                '"' | '\'' => {
                    self.read_string()?;
                }
                '{' => {
                    depth += 1;
                    self.pos += 1;
                }
                '}' => {
                    depth -= 1;
                    self.pos += 1;
                }
                ch => self.pos += ch.len_utf8(),
            }
        }
        Some(())
    }

    /// Parse one specifier and its comma. The cursor stops before the `}`
    /// when the specifier is the last one without a trailing comma.
    fn parse_specifier(&mut self) -> Option<Specifier> {
        let start = self.pos;
        let mut end = start;
        let mut tokens: Vec<String> = Vec::new();
        loop {
            match self.peek()? {
                ',' | '}' => break,
                '"' | '\'' => tokens.push(self.read_string()?),
                _ => tokens.push(self.read_identifier()?),
            }
            end = self.pos;
            if !self.skip_trivia() {
                return None;
            }
        }
        let name = exported_name(&tokens)?;
        let local = local_name(&tokens)?;
        let comma = if self.peek() == Some(',') {
            let comma = self.pos;
            self.pos += 1;
            Some(comma)
        } else {
            None
        };
        Some(Specifier {
            start,
            end,
            comma,
            name,
            local,
        })
    }
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$' || !ch.is_ascii()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(source: &str) -> Vec<&str> {
        source.split('\n').collect()
    }

    fn find(lines: &[&str], line_idx: usize) -> Option<ExportListStatement> {
        ExportListIndex::default().statement_for_line(lines, line_idx)
    }

    /// Apply the rewrite for `names` to the statement that holds `line_idx`
    /// and return the new source.
    fn rewrite(source: &str, line_idx: usize, names: &[&str]) -> Option<String> {
        let lines = lines(source);
        let mut index = ExportListIndex::default();
        let statement = index.statement_for_line(&lines, line_idx)?;
        let outcome = index.rewrite(statement, names)?;
        let mut out: Vec<String> = lines[..statement.start_line]
            .iter()
            .map(ToString::to_string)
            .collect();
        if let ListRewrite::Replace(new_lines) = outcome.rewrite {
            out.extend(new_lines);
        }
        out.extend(
            lines[statement.end_line + 1..]
                .iter()
                .map(ToString::to_string),
        );
        Some(out.join("\n"))
    }

    #[test]
    fn finds_statement_from_a_specifier_line() {
        let source = "import x from './x';\nexport {\n\ta,\n\tb\n} from './mod';\nexport { c };\n";
        let lines = lines(source);
        let expected = Some(ExportListStatement {
            start_line: 1,
            end_line: 4,
        });
        assert_eq!(find(&lines, 2), expected);
        assert_eq!(find(&lines, 3), expected);
        assert_eq!(find(&lines, 1), expected);
        assert_eq!(
            find(&lines, 5),
            Some(ExportListStatement {
                start_line: 5,
                end_line: 5,
            })
        );
        assert_eq!(find(&lines, 0), None);
    }

    #[test]
    fn does_not_claim_a_line_after_the_statement() {
        let source = "export { a };\nconst b = 1;\n";
        assert_eq!(find(&lines(source), 1), None);
    }

    #[test]
    fn does_not_claim_a_line_of_a_declaration() {
        let source = "export const value = {\n  a: 1,\n};\n";
        assert_eq!(find(&lines(source), 1), None);
    }

    #[test]
    fn deletes_a_multi_line_statement_with_one_name() {
        let source = "export {\n\ta\n} from './mod';\nexport { used } from './mod';\n";
        assert_eq!(
            rewrite(source, 1, &["a"]).as_deref(),
            Some("export { used } from './mod';\n")
        );
    }

    #[test]
    fn removes_some_lines_of_a_multi_line_statement() {
        let source = "export {\n  a,\n  kept,\n  b,\n} from './mod';\n";
        assert_eq!(
            rewrite(source, 1, &["a", "b"]).as_deref(),
            Some("export {\n  kept,\n} from './mod';\n")
        );
    }

    #[test]
    fn removes_the_last_name_without_a_trailing_comma() {
        let source = "export {\n  kept,\n  b\n} from './mod';\n";
        assert_eq!(
            rewrite(source, 2, &["b"]).as_deref(),
            Some("export {\n  kept,\n} from './mod';\n")
        );
    }

    #[test]
    fn keeps_comments_of_kept_names_and_drops_trailing_comments_of_removed_names() {
        let source = "export {\n  // group\n  a, // gone\n  /* keep */ kept,\n} from './mod';\n";
        assert_eq!(
            rewrite(source, 2, &["a"]).as_deref(),
            Some("export {\n  // group\n  /* keep */ kept,\n} from './mod';\n")
        );
    }

    #[test]
    fn handles_type_lists_and_type_modifiers() {
        let source = "export type {\n  A,\n  B,\n} from './types';\nexport {\n  type C,\n  d,\n} from './mod';\n";
        assert_eq!(
            rewrite(source, 1, &["A"]).as_deref(),
            Some(
                "export type {\n  B,\n} from './types';\nexport {\n  type C,\n  d,\n} from './mod';\n"
            )
        );
        assert_eq!(
            rewrite(source, 5, &["C"]).as_deref(),
            Some("export type {\n  A,\n  B,\n} from './types';\nexport {\n  d,\n} from './mod';\n")
        );
    }

    fn removed_locals(source: &str, names: &[&str]) -> Vec<String> {
        let lines = lines(source);
        let mut index = ExportListIndex::default();
        let statement = index.statement_for_line(&lines, 0).unwrap();
        index.rewrite(statement, names).unwrap().removed_locals
    }

    #[test]
    fn reports_the_local_name_of_each_removed_specifier() {
        assert_eq!(
            removed_locals("export { a as b, type c, a, d };\n", &["b", "c", "a"]),
            vec!["a".to_owned(), "c".to_owned(), "a".to_owned()]
        );
        assert!(removed_locals("export { a } from './mod';\n", &["a"]).is_empty());
    }

    #[test]
    fn matches_the_exported_alias_not_the_local_name() {
        let source = "export {\n  a as b,\n  a,\n} from './mod';\n";
        assert_eq!(
            rewrite(source, 1, &["b"]).as_deref(),
            Some("export {\n  a,\n} from './mod';\n")
        );
    }

    #[test]
    fn handles_names_on_the_brace_lines() {
        let source = "export { a,\n  kept, b } from './mod';\n";
        assert_eq!(
            rewrite(source, 0, &["a", "b"]).as_deref(),
            Some("export {\n  kept } from './mod';\n")
        );
    }

    #[test]
    fn single_line_lists_keep_their_format() {
        assert_eq!(
            rewrite("export { Foo, Bar, Baz } from \"./mod\";", 0, &["Bar"]).as_deref(),
            Some("export { Foo, Baz } from \"./mod\";")
        );
        assert_eq!(
            rewrite("export { Foo, Bar, Baz } from \"./mod\";", 0, &["Baz"]).as_deref(),
            Some("export { Foo, Bar } from \"./mod\";")
        );
        assert_eq!(
            rewrite("export { A, B, C };", 0, &["B", "C"]).as_deref(),
            Some("export { A };")
        );
    }

    #[test]
    fn handles_import_attributes_and_no_semicolon() {
        let source =
            "export {\n  a,\n  kept\n} from './data.json' with { type: 'json' }\nconst x = 1;\n";
        assert_eq!(
            rewrite(source, 1, &["a"]).as_deref(),
            Some("export {\n  kept\n} from './data.json' with { type: 'json' }\nconst x = 1;\n")
        );
        assert_eq!(
            rewrite(source, 1, &["a", "kept"]).as_deref(),
            Some("const x = 1;\n")
        );
    }

    #[test]
    fn refuses_a_full_delete_when_code_follows_on_the_last_line() {
        let source = "export {\n  a\n} from './mod'; const x = 1;\n";
        assert_eq!(rewrite(source, 1, &["a"]), None);
    }

    #[test]
    fn returns_none_when_no_name_matches() {
        let source = "export {\n  a\n} from './mod';\n";
        assert_eq!(rewrite(source, 1, &["z"]), None);
    }

    #[test]
    fn handles_string_names() {
        let source = "export {\n  \"a-b\" as c,\n  d,\n} from './mod';\n";
        assert_eq!(
            rewrite(source, 1, &["c"]).as_deref(),
            Some("export {\n  d,\n} from './mod';\n")
        );
    }

    #[test]
    fn removes_a_comment_that_would_describe_nothing() {
        let source = "export {\n  kept,\n  // about b\n  /* more */\n  b,\n} from './mod';\n";
        assert_eq!(
            rewrite(source, 4, &["b"]).as_deref(),
            Some("export {\n  kept,\n} from './mod';\n")
        );
        let grouped =
            "export {\n  // group one\n  a,\n\n  // group two\n  kept,\n} from './mod';\n";
        assert_eq!(
            rewrite(grouped, 2, &["a"]).as_deref(),
            Some("export {\n\n  // group two\n  kept,\n} from './mod';\n")
        );
    }

    #[test]
    fn keeps_a_comment_above_the_next_kept_name() {
        let source = "export {\n  // group\n  a,\n  kept,\n} from './mod';\n";
        assert_eq!(
            rewrite(source, 2, &["a"]).as_deref(),
            Some("export {\n  // group\n  kept,\n} from './mod';\n")
        );
    }

    #[test]
    fn reads_a_from_clause_on_a_later_line() {
        let source = "export {\n  a,\n  kept\n}\nfrom './mod'\nconst x = 1;\n";
        assert_eq!(
            rewrite(source, 1, &["a", "kept"]).as_deref(),
            Some("const x = 1;\n")
        );
        assert_eq!(
            rewrite(source, 1, &["a"]).as_deref(),
            Some("export {\n  kept\n}\nfrom './mod'\nconst x = 1;\n")
        );
    }

    #[test]
    fn refuses_an_unclosed_block_comment() {
        let source = "export {\n  a, /* open\n  b\n} from './mod';\n";
        assert_eq!(rewrite(source, 1, &["a"]), None);
    }
}
