//! Inline suppression comment parsing.
//!
//! Parses `fallow-ignore-file` and `fallow-ignore-next-line` comments from
//! source files, supporting `//`, `/* */`, JSDoc `/** */`, and `<!-- -->` styles.

use oxc_ast::ast::Comment;

pub use fallow_types::suppress::{
    IssueKind, Suppression, SuppressionTarget, UnknownSuppressionKind, parse_suppression_target,
};

/// Parsed suppressions plus any tokens that did not resolve to a known kind.
///
/// `unknown_kinds` are tokens from a `// fallow-ignore-*` marker that did
/// not parse to any `IssueKind`. Known tokens on the same marker are still
/// recorded as normal `Suppression` entries.
#[derive(Debug, Default, Clone)]
pub struct ParsedSuppressions {
    /// Suppressions for tokens that parsed to a known `IssueKind`.
    pub suppressions: Vec<Suppression>,
    /// Tokens from suppression markers that did not parse.
    pub unknown_kinds: Vec<UnknownSuppressionKind>,
}

/// Convert a byte offset to a 1-based line number.
#[expect(
    clippy::cast_possible_truncation,
    reason = "line count and source length are bounded by file size"
)]
fn byte_offset_to_line(source: &str, byte_offset: u32) -> u32 {
    let byte_offset = byte_offset as usize;
    let prefix = &source[..byte_offset.min(source.len())];
    prefix.bytes().filter(|&b| b == b'\n').count() as u32 + 1
}

/// Split a suppression marker's tail into known targets and unknown tokens.
///
/// Returns the de-duplicated list of known targets in source order, and the
/// list of verbatim unknown tokens (also de-duplicated). Unknown tokens are
/// preserved so the caller can emit a diagnostic per token. Whitespace and
/// commas both separate tokens.
fn parse_suppression_target_list(rest: &str) -> (Vec<SuppressionTarget>, Vec<String>) {
    let mut targets = Vec::new();
    let mut unknown = Vec::new();
    for token in rest
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|token| !token.is_empty())
    {
        match parse_suppression_target(token) {
            Some(target) => {
                if !targets.contains(&target) {
                    targets.push(target);
                }
            }
            None => {
                let owned = token.to_string();
                if !unknown.contains(&owned) {
                    unknown.push(owned);
                }
            }
        }
    }
    (targets, unknown)
}

fn split_suppression_reason(rest: &str) -> (&str, Option<String>) {
    for (idx, _) in rest.match_indices("--") {
        let before_ok = idx == 0
            || rest[..idx]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let after_idx = idx + 2;
        let after_ok = after_idx == rest.len()
            || rest[after_idx..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace);
        if before_ok && after_ok {
            let targets = rest[..idx].trim_end();
            let reason = rest[after_idx..].trim();
            return (
                targets,
                if reason.is_empty() {
                    None
                } else {
                    Some(reason.to_string())
                },
            );
        }
    }

    (rest, None)
}

fn push_suppressions(parsed: &mut ParsedSuppressions, line: u32, comment_line: u32, rest: &str) {
    let (rest, reason) = split_suppression_reason(rest);

    if rest.is_empty() {
        parsed
            .suppressions
            .push(Suppression::all(line, comment_line).with_reason(reason));
        return;
    }

    let is_file_level = line == 0;
    let (targets, unknown) = parse_suppression_target_list(rest);

    parsed
        .suppressions
        .extend(targets.into_iter().map(|target| Suppression {
            line,
            comment_line,
            target: Some(target),
            reason: reason.clone(),
        }));

    parsed
        .unknown_kinds
        .extend(unknown.into_iter().map(|token| UnknownSuppressionKind {
            comment_line,
            is_file_level,
            token,
            reason: reason.clone(),
        }));
}

/// Parse all fallow suppression comments from a file's comment list.
///
/// Unknown tokens are collected into `unknown_kinds` rather than discarding
/// the whole marker.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "source length is bounded by file size"
)]
pub(crate) fn parse_suppressions(comments: &[Comment], source: &str) -> ParsedSuppressions {
    let mut parsed = ParsedSuppressions::default();

    for comment in comments {
        let content_span = comment.content_span();
        let text = &source
            [content_span.start as usize..content_span.end.min(source.len() as u32) as usize];

        let first_line = byte_offset_to_line(source, comment.span.start);
        if comment.is_block() {
            // A block comment may span lines. A next-line marker applies to
            // the line after the comment ENDS, not after the marker line.
            let suppressed_line = byte_offset_to_line(source, comment.span.end) + 1;
            push_block_markers(&mut parsed, text.lines(), first_line, suppressed_line);
        } else if let Some(marker) = match_line_marker(text) {
            push_marker(&mut parsed, marker, first_line, first_line + 1);
        }
    }

    parsed
}

/// A `fallow-ignore-*` marker and the rest of its line (the targets).
#[derive(Clone, Copy)]
enum Marker<'a> {
    File(&'a str),
    NextLine(&'a str),
}

impl<'a> Marker<'a> {
    const fn rest(&self) -> &'a str {
        match self {
            Self::File(rest) | Self::NextLine(rest) => rest,
        }
    }
}

/// Match a marker at the start of the text.
fn split_marker(text: &str) -> Option<Marker<'_>> {
    if let Some(rest) = text.strip_prefix("fallow-ignore-file") {
        Some(Marker::File(rest.trim()))
    } else {
        text.strip_prefix("fallow-ignore-next-line")
            .map(|rest| Marker::NextLine(rest.trim()))
    }
}

/// Match a marker at the start of a line comment (`//` or `<!-- -->`).
fn match_line_marker(text: &str) -> Option<Marker<'_>> {
    split_marker(text.trim())
}

/// Match a marker at the start of one line of a block comment.
///
/// Leading and trailing runs of `*` are stripped first, so the JSDoc forms
/// `/** fallow-ignore-next-line */`, ` * fallow-ignore-next-line` and
/// `/** fallow-ignore-file **/` match.
///
/// A block comment holds prose, and a wrapped prose line can start with the
/// marker word. Such a line is a marker only when no target follows, when the
/// first token is `--` or a known issue kind, or when the first token has a
/// `-` like an issue kind. A typo such as `unused-exprt` then still gives the
/// unknown-kind warning, and ` * fallow-ignore-next-line is how...` does not.
fn match_block_marker(line: &str) -> Option<Marker<'_>> {
    let text = line
        .trim()
        .trim_start_matches('*')
        .trim_end_matches('*')
        .trim();
    let marker = split_marker(text)?;
    let first_token = marker
        .rest()
        .split(|c: char| c == ',' || c.is_whitespace())
        .find(|token| !token.is_empty());
    let is_marker = first_token
        .is_none_or(|token| token.contains('-') || parse_suppression_target(token).is_some());
    is_marker.then_some(marker)
}

fn push_marker(
    parsed: &mut ParsedSuppressions,
    marker: Marker<'_>,
    comment_line: u32,
    suppressed_line: u32,
) {
    match marker {
        Marker::File(rest) => push_suppressions(parsed, 0, comment_line, rest),
        Marker::NextLine(rest) => push_suppressions(parsed, suppressed_line, comment_line, rest),
    }
}

/// Record every marker line of one block comment.
///
/// `lines` is the comment content, `first_line` is the 1-based line of the
/// first content line, and `suppressed_line` is the line after the comment.
/// Only a marker line carries targets: the other lines are prose.
fn push_block_markers<'a>(
    parsed: &mut ParsedSuppressions,
    lines: impl Iterator<Item = &'a str>,
    first_line: u32,
    suppressed_line: u32,
) {
    for (line, comment_line) in lines.zip(first_line..) {
        if let Some(marker) = match_block_marker(line) {
            push_marker(parsed, marker, comment_line, suppressed_line);
        }
    }
}

/// Parse suppressions from raw source text using simple string scanning.
///
/// A `//` line, a one-line `<!-- -->` comment, and a `/* */` block comment
/// that starts a line can carry a marker. A block comment may span lines, as
/// in a multi-line JSDoc comment. A single-line block comment counts only
/// when `*/` ends the line.
#[expect(
    clippy::cast_possible_truncation,
    reason = "line count is bounded by file size"
)]
pub fn parse_suppressions_from_source(source: &str) -> ParsedSuppressions {
    const BLOCK_END: &str = "*/";
    let mut parsed = ParsedSuppressions::default();
    let lines: Vec<&str> = source.lines().collect();

    let mut idx = 0;
    while idx < lines.len() {
        let comment_line = (idx as u32) + 1;
        let trimmed = lines[idx].trim();

        if let Some(rest) = trimmed.strip_prefix("//") {
            if let Some(marker) = match_line_marker(rest) {
                push_marker(&mut parsed, marker, comment_line, comment_line + 1);
            }
        } else if let Some(rest) = trimmed.strip_prefix("<!--") {
            if let Some(marker) = rest.strip_suffix("-->").and_then(match_line_marker) {
                push_marker(&mut parsed, marker, comment_line, comment_line + 1);
            }
        } else if let Some(rest) = trimmed.strip_prefix("/*") {
            if let Some(end) = rest.find(BLOCK_END) {
                if end + BLOCK_END.len() == rest.len() {
                    let content = std::iter::once(&rest[..end]);
                    push_block_markers(&mut parsed, content, comment_line, comment_line + 1);
                }
            } else {
                // Collect the lines up to the line that closes the comment.
                // An unclosed comment carries no markers.
                let mut content = vec![rest];
                let mut closed = false;
                while !closed && idx + 1 < lines.len() {
                    idx += 1;
                    let line = lines[idx];
                    let end = line.find(BLOCK_END);
                    closed = end.is_some();
                    content.push(&line[..end.unwrap_or(line.len())]);
                }
                if !closed {
                    break;
                }
                let suppressed_line = (idx as u32) + 2;
                push_block_markers(
                    &mut parsed,
                    content.into_iter(),
                    comment_line,
                    suppressed_line,
                );
            }
        }
        idx += 1;
    }

    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_wide_suppression() {
        let source = "// fallow-ignore-file\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].issue_kind_target().is_none());
    }

    #[test]
    fn parse_file_wide_suppression_with_kind() {
        let source = "// fallow-ignore-file unused-export\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_next_line_suppression() {
        let source =
            "import { x } from './x';\n// fallow-ignore-next-line\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 3); // suppresses line 3 (the export)
        assert!(suppressions[0].issue_kind_target().is_none());
    }

    #[test]
    fn parse_next_line_suppression_with_kind() {
        let source = "// fallow-ignore-next-line unused-export\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_next_line_suppression_with_reason() {
        let source = "// fallow-ignore-next-line unused-export -- legacy public API\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].reason.as_deref(), Some("legacy public API"));
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_blanket_file_suppression_with_reason() {
        let source = "// fallow-ignore-file -- generated route map\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].issue_kind_target().is_none());
        assert_eq!(
            suppressions[0].reason.as_deref(),
            Some("generated route map")
        );
    }

    #[test]
    fn parse_next_line_suppression_with_comma_kind_list() {
        let source =
            "// fallow-ignore-next-line unused-export, complexity\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 2);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
        assert_eq!(suppressions[1].line, 2);
        assert_eq!(
            suppressions[1].issue_kind_target(),
            Some(IssueKind::Complexity)
        );
    }

    #[test]
    fn parse_next_line_suppression_with_space_kind_list() {
        let source = "// fallow-ignore-next-line unused-export complexity\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 2);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
        assert_eq!(suppressions[1].line, 2);
        assert_eq!(
            suppressions[1].issue_kind_target(),
            Some(IssueKind::Complexity)
        );
    }

    #[test]
    fn parse_scoped_policy_suppression() {
        let source =
            "// fallow-ignore-next-line policy-violation:team-policy/no-child-process\nexec();\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 2);
        let target = suppressions[0]
            .policy_rule_target()
            .expect("scoped policy target should parse");
        assert_eq!(target.pack, "team-policy");
        assert_eq!(target.rule_id, "no-child-process");
    }

    #[test]
    fn parse_scoped_policy_suppression_mixed_with_regular_kind() {
        let source = "// fallow-ignore-next-line policy-violation:team-policy/no-child-process, unused-export\nexec();\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 2);
        assert!(suppressions[0].policy_rule_target().is_some());
        assert_eq!(
            suppressions[1].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_malformed_scoped_policy_suppression_as_unknown() {
        let source =
            "// fallow-ignore-next-line policy-violation:team-policy/no:child-process\nexec();\n";
        let parsed = parse_suppressions_from_source(source);
        assert!(parsed.suppressions.is_empty());
        assert_eq!(parsed.unknown_kinds.len(), 1);
        assert_eq!(
            parsed.unknown_kinds[0].token,
            "policy-violation:team-policy/no:child-process"
        );
    }

    #[test]
    fn parse_unknown_kind_surfaces_as_unknown() {
        let source = "// fallow-ignore-next-line typo-kind\nexport const foo = 1;\n";
        let parsed = parse_suppressions_from_source(source);
        assert!(
            parsed.suppressions.is_empty(),
            "no known kinds on the marker, so no suppression should be recorded"
        );
        assert_eq!(parsed.unknown_kinds.len(), 1);
        assert_eq!(parsed.unknown_kinds[0].token, "typo-kind");
        assert_eq!(parsed.unknown_kinds[0].comment_line, 1);
        assert!(!parsed.unknown_kinds[0].is_file_level);
    }

    #[test]
    fn parse_partial_accept_known_kinds_recorded() {
        let source = "// fallow-ignore-next-line unused-export, complexity-typo -- tracked migration\nexport const foo = 1;\n";
        let parsed = parse_suppressions_from_source(source);
        assert_eq!(parsed.suppressions.len(), 1);
        assert_eq!(parsed.suppressions[0].line, 2);
        assert_eq!(
            parsed.suppressions[0].reason.as_deref(),
            Some("tracked migration")
        );
        assert_eq!(
            parsed.suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
        assert_eq!(parsed.unknown_kinds.len(), 1);
        assert_eq!(parsed.unknown_kinds[0].token, "complexity-typo");
        assert_eq!(
            parsed.unknown_kinds[0].reason.as_deref(),
            Some("tracked migration")
        );
        assert_eq!(parsed.unknown_kinds[0].comment_line, 1);
        assert!(!parsed.unknown_kinds[0].is_file_level);
    }

    #[test]
    fn parse_multiple_unknown_kinds_each_recorded() {
        let source = "// fallow-ignore-next-line typo-a, typo-b typo-c\nexport const foo = 1;\n";
        let parsed = parse_suppressions_from_source(source);
        assert!(parsed.suppressions.is_empty());
        assert_eq!(parsed.unknown_kinds.len(), 3);
        let tokens: Vec<&str> = parsed
            .unknown_kinds
            .iter()
            .map(|u| u.token.as_str())
            .collect();
        assert_eq!(tokens, vec!["typo-a", "typo-b", "typo-c"]);
    }

    #[test]
    fn parse_unknown_kind_file_level_carries_is_file_level() {
        let source = "// fallow-ignore-file typo-kind\nexport const foo = 1;\n";
        let parsed = parse_suppressions_from_source(source);
        assert!(parsed.suppressions.is_empty());
        assert_eq!(parsed.unknown_kinds.len(), 1);
        assert_eq!(parsed.unknown_kinds[0].token, "typo-kind");
        assert!(parsed.unknown_kinds[0].is_file_level);
    }

    #[test]
    fn parse_unknown_kind_deduplicates_repeats() {
        let source = "// fallow-ignore-next-line typo, typo\nexport const foo = 1;\n";
        let parsed = parse_suppressions_from_source(source);
        assert_eq!(parsed.unknown_kinds.len(), 1);
    }

    #[test]
    fn parse_oxc_comments() {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;

        let source = "// fallow-ignore-file\n// fallow-ignore-next-line unused-export\nexport const foo = 1;\nexport const bar = 2;\n";
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::mjs()).parse();

        let suppressions = parse_suppressions(&parser_return.program.comments, source).suppressions;
        assert_eq!(suppressions.len(), 2);

        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].issue_kind_target().is_none());

        assert_eq!(suppressions[1].line, 3);
        assert_eq!(
            suppressions[1].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_block_comment_suppression() {
        let source = "/* fallow-ignore-file */\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].issue_kind_target().is_none());
    }

    #[test]
    fn parse_html_comment_file_suppression() {
        let source = "<!-- fallow-ignore-file complexity -->\n@if (enabled) { <p /> }\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::Complexity)
        );
    }

    #[test]
    fn parse_block_comment_next_line_suppression() {
        let source = "/* fallow-ignore-next-line unused-export */\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_html_comment_next_line_suppression() {
        let source = "<!-- fallow-ignore-next-line complexity -->\n@if (enabled) { <p /> }\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::Complexity)
        );
    }

    #[test]
    fn parse_multiple_suppressions_on_adjacent_lines() {
        let source = "// fallow-ignore-next-line unused-export\n// fallow-ignore-next-line unused-type\nexport const foo = 1;\nexport type Bar = string;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 2);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
        assert_eq!(suppressions[1].line, 3);
        assert_eq!(
            suppressions[1].issue_kind_target(),
            Some(IssueKind::UnusedType)
        );
    }

    #[test]
    fn parse_file_wide_and_next_line_combined() {
        let source = "// fallow-ignore-file unused-file\n// fallow-ignore-next-line unused-export\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 2);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedFile)
        );
        assert_eq!(suppressions[1].line, 3);
        assert_eq!(
            suppressions[1].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_suppression_all_issue_kinds() {
        let kinds = [
            ("unused-file", IssueKind::UnusedFile),
            ("unused-export", IssueKind::UnusedExport),
            ("unused-type", IssueKind::UnusedType),
            ("unused-dependency", IssueKind::UnusedDependency),
            ("unused-dev-dependency", IssueKind::UnusedDevDependency),
            ("unused-enum-member", IssueKind::UnusedEnumMember),
            ("unused-class-member", IssueKind::UnusedClassMember),
            ("unresolved-import", IssueKind::UnresolvedImport),
            ("unlisted-dependency", IssueKind::UnlistedDependency),
            ("duplicate-export", IssueKind::DuplicateExport),
            ("duplicate-export-alias", IssueKind::DuplicateExportAlias),
            ("duplicate-export-aliases", IssueKind::DuplicateExportAlias),
            ("code-duplication", IssueKind::CodeDuplication),
            ("circular-dependency", IssueKind::CircularDependency),
            ("circular-dependencies", IssueKind::CircularDependency),
        ];
        for (token, expected_kind) in &kinds {
            let source = format!("// fallow-ignore-file {token}\nexport const foo = 1;\n");
            let suppressions = parse_suppressions_from_source(&source).suppressions;
            assert_eq!(suppressions.len(), 1, "Expected 1 suppression for {token}");
            assert_eq!(suppressions[0].issue_kind_target(), Some(*expected_kind));
        }
    }

    #[test]
    fn parse_block_comment_with_whitespace() {
        let source = "/*  fallow-ignore-file  */\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].issue_kind_target().is_none());
    }

    #[test]
    fn parse_empty_source_no_suppressions() {
        let suppressions = parse_suppressions_from_source("").suppressions;
        assert!(suppressions.is_empty());
    }

    #[test]
    fn parse_no_suppression_comments() {
        let source = "// regular comment\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert!(suppressions.is_empty());
    }

    #[test]
    fn parse_suppression_not_at_line_start_ignored() {
        let source = "export const foo = 1; // fallow-ignore-file\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert!(
            suppressions.is_empty(),
            "Inline comment after code should not be parsed as suppression"
        );
    }

    #[test]
    fn parse_block_comment_without_closing_ignored() {
        let source = "/* fallow-ignore-file\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert!(suppressions.is_empty());
    }

    #[test]
    fn byte_offset_to_line_first_byte() {
        assert_eq!(byte_offset_to_line("abc\ndef\n", 0), 1);
    }

    #[test]
    fn byte_offset_to_line_second_line() {
        assert_eq!(byte_offset_to_line("abc\ndef\n", 4), 2);
    }

    #[test]
    fn byte_offset_to_line_beyond_source() {
        assert_eq!(byte_offset_to_line("abc\n", 100), 2);
    }

    #[test]
    fn parse_oxc_block_comment_suppression() {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;

        let source = "/* fallow-ignore-file unused-file */\nexport const foo = 1;\n";
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::mjs()).parse();

        let suppressions = parse_suppressions(&parser_return.program.comments, source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedFile)
        );
    }

    #[test]
    fn parse_oxc_multiline_block_comment_suppresses_line_after_comment_end() {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;

        let source = "/* fallow-ignore-next-line unused-export\n   kept for the legacy import path */\nexport const foo = 1;\n";
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::mjs()).parse();

        let parsed = parse_suppressions(&parser_return.program.comments, source);
        assert_eq!(parsed.suppressions.len(), 1);
        assert_eq!(
            parsed.suppressions[0].line, 3,
            "suppression must land on the line after `*/`, not inside the comment"
        );
        assert_eq!(parsed.suppressions[0].comment_line, 1);
        assert_eq!(
            parsed.suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
        assert!(
            parsed.unknown_kinds.is_empty(),
            "prose after the marker line must not be tokenized into unknown kinds"
        );
    }

    #[test]
    fn parse_oxc_unknown_kind_surfaces_as_unknown() {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;

        let source = "// fallow-ignore-next-line nonexistent-kind\nexport const foo = 1;\n";
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::mjs()).parse();

        let parsed = parse_suppressions(&parser_return.program.comments, source);
        assert!(parsed.suppressions.is_empty());
        assert_eq!(parsed.unknown_kinds.len(), 1);
        assert_eq!(parsed.unknown_kinds[0].token, "nonexistent-kind");
    }

    fn parse_oxc(source: &str) -> ParsedSuppressions {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::mjs()).parse();
        parse_suppressions(&parser_return.program.comments, source)
    }

    #[test]
    fn parse_oxc_single_line_jsdoc_next_line_suppression() {
        let parsed =
            parse_oxc("/** fallow-ignore-next-line unused-export */\nexport const foo = 1;\n");
        assert_eq!(parsed.suppressions.len(), 1);
        assert_eq!(parsed.suppressions[0].line, 2);
        assert_eq!(parsed.suppressions[0].comment_line, 1);
        assert_eq!(
            parsed.suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
        assert!(parsed.unknown_kinds.is_empty());
    }

    #[test]
    fn parse_oxc_multi_line_jsdoc_next_line_suppression() {
        let parsed = parse_oxc(
            "/**\n * fallow-ignore-next-line unused-export\n */\nexport const foo = 1;\n",
        );
        assert_eq!(parsed.suppressions.len(), 1);
        assert_eq!(
            parsed.suppressions[0].line, 4,
            "suppression must land on the line after `*/`"
        );
        assert_eq!(
            parsed.suppressions[0].comment_line, 2,
            "comment_line must point at the marker line"
        );
        assert_eq!(
            parsed.suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
        assert!(parsed.unknown_kinds.is_empty());
    }

    #[test]
    fn parse_oxc_multi_line_jsdoc_marker_after_prose() {
        let parsed = parse_oxc(
            "/**\n * Kept for the legacy import path.\n * fallow-ignore-next-line unused-export -- legacy\n * More prose.\n */\nexport const foo = 1;\n",
        );
        assert_eq!(parsed.suppressions.len(), 1);
        assert_eq!(parsed.suppressions[0].line, 6);
        assert_eq!(parsed.suppressions[0].comment_line, 3);
        assert_eq!(parsed.suppressions[0].reason.as_deref(), Some("legacy"));
        assert!(
            parsed.unknown_kinds.is_empty(),
            "prose lines must not be tokenized into unknown kinds"
        );
    }

    #[test]
    fn parse_oxc_jsdoc_file_suppression() {
        let parsed = parse_oxc("/** fallow-ignore-file unused-export */\nexport const foo = 1;\n");
        assert_eq!(parsed.suppressions.len(), 1);
        assert_eq!(parsed.suppressions[0].line, 0);
        assert_eq!(
            parsed.suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_oxc_jsdoc_unknown_kind_surfaces_as_unknown() {
        let parsed = parse_oxc(
            "/**\n * fallow-ignore-next-line nonexistent-kind\n */\nexport const foo = 1;\n",
        );
        assert!(parsed.suppressions.is_empty());
        assert_eq!(parsed.unknown_kinds.len(), 1);
        assert_eq!(parsed.unknown_kinds[0].token, "nonexistent-kind");
        assert_eq!(parsed.unknown_kinds[0].comment_line, 2);
    }

    #[test]
    fn parse_oxc_line_comment_marker_after_text_ignored() {
        let parsed =
            parse_oxc("// see fallow-ignore-next-line unused-export\nexport const foo = 1;\n");
        assert!(parsed.suppressions.is_empty());
        assert!(parsed.unknown_kinds.is_empty());
    }

    #[test]
    fn parse_oxc_jsdoc_marker_inside_prose_ignored() {
        let parsed = parse_oxc(
            "/** Do not use fallow-ignore-next-line here */\nexport const foo = 1;\n/**\n * Do not use fallow-ignore-file here.\n */\nexport const bar = 2;\n",
        );
        assert!(parsed.suppressions.is_empty());
        assert!(parsed.unknown_kinds.is_empty());
    }

    #[test]
    fn parse_source_single_line_jsdoc_suppressions() {
        let parsed = parse_suppressions_from_source(
            "/** fallow-ignore-file code-duplication */\n/** fallow-ignore-next-line unused-export */\nexport const foo = 1;\n",
        );
        assert_eq!(parsed.suppressions.len(), 2);
        assert_eq!(parsed.suppressions[0].line, 0);
        assert_eq!(
            parsed.suppressions[0].issue_kind_target(),
            Some(IssueKind::CodeDuplication)
        );
        assert_eq!(parsed.suppressions[1].line, 3);
        assert_eq!(
            parsed.suppressions[1].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
        assert!(parsed.unknown_kinds.is_empty());
    }

    #[test]
    fn parse_source_multi_line_jsdoc_file_suppression() {
        let parsed = parse_suppressions_from_source(
            "/**\n * fallow-ignore-file code-duplication\n */\nexport const foo = 1;\n",
        );
        assert_eq!(parsed.suppressions.len(), 1);
        assert_eq!(parsed.suppressions[0].line, 0);
        assert_eq!(parsed.suppressions[0].comment_line, 2);
        assert_eq!(
            parsed.suppressions[0].issue_kind_target(),
            Some(IssueKind::CodeDuplication)
        );
    }

    #[test]
    fn parse_source_multi_line_jsdoc_next_line_targets_line_after_close() {
        let parsed = parse_suppressions_from_source(
            "/**\n * Kept for the legacy path.\n * fallow-ignore-next-line unused-export\n */\nexport const foo = 1;\n",
        );
        assert_eq!(parsed.suppressions.len(), 1);
        assert_eq!(parsed.suppressions[0].line, 5);
        assert_eq!(parsed.suppressions[0].comment_line, 3);
        assert!(parsed.unknown_kinds.is_empty());
    }

    #[test]
    fn parse_source_block_comment_with_code_after_close_is_scanned_once() {
        let parsed = parse_suppressions_from_source(
            "/*\n * fallow-ignore-next-line unused-export\n */\nexport const foo = 1;\n// fallow-ignore-next-line unused-type\nexport type Bar = 1;\n",
        );
        let lines: Vec<u32> = parsed.suppressions.iter().map(|s| s.line).collect();
        assert_eq!(lines, vec![4, 6]);
    }

    #[test]
    fn jsdoc_wrapped_prose_line_is_not_a_marker() {
        let source = "/**\n * Use the comment below to hide a finding:\n * fallow-ignore-next-line is how you hide a finding.\n */\nexport const foo = 1;\n";
        for parsed in [parse_oxc(source), parse_suppressions_from_source(source)] {
            assert!(parsed.suppressions.is_empty(), "{:?}", parsed.suppressions);
            assert!(
                parsed.unknown_kinds.is_empty(),
                "{:?}",
                parsed.unknown_kinds
            );
        }
    }

    #[test]
    fn jsdoc_marker_with_reason_only_is_a_marker() {
        let parsed = parse_oxc(
            "/**\n * fallow-ignore-next-line -- kept for the legacy path\n */\nexport const foo = 1;\n",
        );
        assert_eq!(parsed.suppressions.len(), 1);
        assert_eq!(parsed.suppressions[0].target, None);
        assert_eq!(
            parsed.suppressions[0].reason.as_deref(),
            Some("kept for the legacy path")
        );
    }

    #[test]
    fn jsdoc_block_with_two_marker_lines_records_both() {
        let source = "/**\n * fallow-ignore-file code-duplication\n * fallow-ignore-next-line unused-export\n */\nexport const foo = 1;\n";
        for parsed in [parse_oxc(source), parse_suppressions_from_source(source)] {
            let found: Vec<(u32, Option<IssueKind>)> = parsed
                .suppressions
                .iter()
                .map(|s| (s.line, s.issue_kind_target()))
                .collect();
            assert_eq!(
                found,
                vec![
                    (0, Some(IssueKind::CodeDuplication)),
                    (5, Some(IssueKind::UnusedExport)),
                ]
            );
        }
    }

    #[test]
    fn line_comment_does_not_strip_stars() {
        let source = "//* fallow-ignore-next-line unused-export\nexport const foo = 1;\n// fallow-ignore-next-line unused-export*\nexport const bar = 1;\n";
        for parsed in [parse_oxc(source), parse_suppressions_from_source(source)] {
            assert!(parsed.suppressions.is_empty(), "{:?}", parsed.suppressions);
            let tokens: Vec<&str> = parsed
                .unknown_kinds
                .iter()
                .map(|u| u.token.as_str())
                .collect();
            assert_eq!(tokens, vec!["unused-export*"]);
        }
    }

    #[test]
    fn duplicate_export_token_does_not_suppress_alias_kind() {
        let source = "// fallow-ignore-next-line duplicate-export\nexport default a;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::DuplicateExport)
        );
        assert!(!suppressions[0].matches_issue_kind(2, IssueKind::DuplicateExportAlias));
    }
}
