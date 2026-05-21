//! Inline suppression comment parsing.
//!
//! Parses `fallow-ignore-file` and `fallow-ignore-next-line` comments from
//! source files, supporting `//`, `/* */`, and `<!-- -->` styles.

use oxc_ast::ast::Comment;

// Re-export types from fallow-types
pub use fallow_types::suppress::{IssueKind, Suppression, UnknownSuppressionKind};

/// Parsed suppressions plus any tokens that did not resolve to a known kind.
///
/// `unknown_kinds` are tokens from a `// fallow-ignore-*` marker that did
/// not parse to any `IssueKind`. The remaining known tokens on the same
/// marker are still recorded as normal `Suppression` entries; downstream
/// `find_stale` surfaces each unknown token as a `StaleSuppression` finding
/// instead of discarding the entire marker silently. See issue #449.
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

/// Split a suppression marker's tail into known `IssueKind`s and unknown tokens.
///
/// Returns the de-duplicated list of known kinds in source order, and the
/// list of verbatim unknown tokens (also de-duplicated). Unknown tokens are
/// preserved so the caller can emit a diagnostic per token. Whitespace and
/// commas both separate tokens.
fn parse_issue_kind_list(rest: &str) -> (Vec<IssueKind>, Vec<String>) {
    let mut kinds = Vec::new();
    let mut unknown = Vec::new();
    for token in rest
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|token| !token.is_empty())
    {
        match IssueKind::parse(token) {
            Some(kind) => {
                if !kinds.contains(&kind) {
                    kinds.push(kind);
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
    (kinds, unknown)
}

fn push_suppressions(parsed: &mut ParsedSuppressions, line: u32, comment_line: u32, rest: &str) {
    if rest.is_empty() {
        parsed.suppressions.push(Suppression {
            line,
            comment_line,
            kind: None,
        });
        return;
    }

    let is_file_level = line == 0;
    let (kinds, unknown) = parse_issue_kind_list(rest);

    parsed
        .suppressions
        .extend(kinds.into_iter().map(|kind| Suppression {
            line,
            comment_line,
            kind: Some(kind),
        }));

    parsed
        .unknown_kinds
        .extend(unknown.into_iter().map(|token| UnknownSuppressionKind {
            comment_line,
            is_file_level,
            token,
        }));
}

/// Parse all fallow suppression comments from a file's comment list.
///
/// Supports:
/// - `// fallow-ignore-file` to suppress all issues in the file
/// - `// fallow-ignore-file unused-export` to suppress a specific issue type for the file
/// - `// fallow-ignore-next-line` to suppress all issues on the next line
/// - `// fallow-ignore-next-line unused-export` to suppress a specific issue type on the next line
/// - `// fallow-ignore-next-line unused-export, complexity` to suppress multiple issue types on the next line
/// - `<!-- fallow-ignore-file complexity -->` to suppress a specific issue type in HTML-like files
///
/// Unknown tokens (typos, obsolete kind names) are collected into
/// `unknown_kinds` rather than silently discarding the entire marker. The
/// known tokens on the same marker are still recorded as suppressions; see
/// issue #449.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "source length is bounded by file size"
)]
pub fn parse_suppressions(comments: &[Comment], source: &str) -> ParsedSuppressions {
    let mut parsed = ParsedSuppressions::default();

    for comment in comments {
        let content_span = comment.content_span();
        let text = &source
            [content_span.start as usize..content_span.end.min(source.len() as u32) as usize];
        let trimmed = text.trim();

        if let Some(rest) = trimmed.strip_prefix("fallow-ignore-file") {
            let rest = rest.trim();
            let src_comment_line = byte_offset_to_line(source, comment.span.start);
            push_suppressions(&mut parsed, 0, src_comment_line, rest);
        } else if let Some(rest) = trimmed.strip_prefix("fallow-ignore-next-line") {
            let rest = rest.trim();
            let src_comment_line = byte_offset_to_line(source, comment.span.start);
            let suppressed_line = src_comment_line + 1;

            push_suppressions(&mut parsed, suppressed_line, src_comment_line, rest);
        }
    }

    parsed
}

/// Parse suppressions from raw source text using simple string scanning.
/// Used for SFC files where comment byte offsets do not correspond to the original file.
///
/// Returns both recognized suppressions and the unknown tokens that did not
/// parse to any `IssueKind`. See `parse_suppressions` and issue #449.
#[expect(
    clippy::cast_possible_truncation,
    reason = "line count is bounded by file size"
)]
pub fn parse_suppressions_from_source(source: &str) -> ParsedSuppressions {
    let mut parsed = ParsedSuppressions::default();

    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        // Match line, block, and HTML comment styles.
        let comment_text = if let Some(rest) = trimmed.strip_prefix("//") {
            Some(rest.trim())
        } else if let Some(rest) = trimmed.strip_prefix("/*") {
            rest.strip_suffix("*/").map(str::trim)
        } else if let Some(rest) = trimmed.strip_prefix("<!--") {
            rest.strip_suffix("-->").map(str::trim)
        } else {
            None
        };

        let Some(text) = comment_text else {
            continue;
        };

        if let Some(rest) = text.strip_prefix("fallow-ignore-file") {
            let rest = rest.trim();
            let src_comment_line = (line_idx as u32) + 1; // 1-based
            push_suppressions(&mut parsed, 0, src_comment_line, rest);
        } else if let Some(rest) = text.strip_prefix("fallow-ignore-next-line") {
            let rest = rest.trim();
            let src_comment_line = (line_idx as u32) + 1; // 1-based
            let suppressed_line = src_comment_line + 1; // next line

            push_suppressions(&mut parsed, suppressed_line, src_comment_line, rest);
        }
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
        assert!(suppressions[0].kind.is_none());
    }

    #[test]
    fn parse_file_wide_suppression_with_kind() {
        let source = "// fallow-ignore-file unused-export\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedExport));
    }

    #[test]
    fn parse_next_line_suppression() {
        let source =
            "import { x } from './x';\n// fallow-ignore-next-line\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 3); // suppresses line 3 (the export)
        assert!(suppressions[0].kind.is_none());
    }

    #[test]
    fn parse_next_line_suppression_with_kind() {
        let source = "// fallow-ignore-next-line unused-export\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedExport));
    }

    #[test]
    fn parse_next_line_suppression_with_comma_kind_list() {
        let source =
            "// fallow-ignore-next-line unused-export, complexity\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 2);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedExport));
        assert_eq!(suppressions[1].line, 2);
        assert_eq!(suppressions[1].kind, Some(IssueKind::Complexity));
    }

    #[test]
    fn parse_next_line_suppression_with_space_kind_list() {
        let source = "// fallow-ignore-next-line unused-export complexity\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 2);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedExport));
        assert_eq!(suppressions[1].line, 2);
        assert_eq!(suppressions[1].kind, Some(IssueKind::Complexity));
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
        // Issue #449: previously discarded the entire line; now the known
        // kind suppresses normally and the unknown surfaces separately.
        let source =
            "// fallow-ignore-next-line unused-export, complexity-typo\nexport const foo = 1;\n";
        let parsed = parse_suppressions_from_source(source);
        assert_eq!(parsed.suppressions.len(), 1);
        assert_eq!(parsed.suppressions[0].line, 2);
        assert_eq!(parsed.suppressions[0].kind, Some(IssueKind::UnusedExport));
        assert_eq!(parsed.unknown_kinds.len(), 1);
        assert_eq!(parsed.unknown_kinds[0].token, "complexity-typo");
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

        // File-wide suppression
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].kind.is_none());

        // Next-line suppression with kind
        assert_eq!(suppressions[1].line, 3); // suppresses line 3 (export const foo)
        assert_eq!(suppressions[1].kind, Some(IssueKind::UnusedExport));
    }

    #[test]
    fn parse_block_comment_suppression() {
        let source = "/* fallow-ignore-file */\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].kind.is_none());
    }

    #[test]
    fn parse_html_comment_file_suppression() {
        let source = "<!-- fallow-ignore-file complexity -->\n@if (enabled) { <p /> }\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(suppressions[0].kind, Some(IssueKind::Complexity));
    }

    // ── Additional coverage ─────────────────────────────────────

    #[test]
    fn parse_block_comment_next_line_suppression() {
        let source = "/* fallow-ignore-next-line unused-export */\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedExport));
    }

    #[test]
    fn parse_html_comment_next_line_suppression() {
        let source = "<!-- fallow-ignore-next-line complexity -->\n@if (enabled) { <p /> }\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(suppressions[0].kind, Some(IssueKind::Complexity));
    }

    #[test]
    fn parse_multiple_suppressions_on_adjacent_lines() {
        let source = "// fallow-ignore-next-line unused-export\n// fallow-ignore-next-line unused-type\nexport const foo = 1;\nexport type Bar = string;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 2);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedExport));
        assert_eq!(suppressions[1].line, 3);
        assert_eq!(suppressions[1].kind, Some(IssueKind::UnusedType));
    }

    #[test]
    fn parse_file_wide_and_next_line_combined() {
        let source = "// fallow-ignore-file unused-file\n// fallow-ignore-next-line unused-export\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 2);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedFile));
        assert_eq!(suppressions[1].line, 3);
        assert_eq!(suppressions[1].kind, Some(IssueKind::UnusedExport));
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
            ("code-duplication", IssueKind::CodeDuplication),
            ("circular-dependency", IssueKind::CircularDependency),
            ("circular-dependencies", IssueKind::CircularDependency),
        ];
        for (token, expected_kind) in &kinds {
            let source = format!("// fallow-ignore-file {token}\nexport const foo = 1;\n");
            let suppressions = parse_suppressions_from_source(&source).suppressions;
            assert_eq!(suppressions.len(), 1, "Expected 1 suppression for {token}");
            assert_eq!(suppressions[0].kind, Some(*expected_kind));
        }
    }

    #[test]
    fn parse_block_comment_with_whitespace() {
        let source = "/*  fallow-ignore-file  */\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].kind.is_none());
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
        // Inline comments that don't start the line are not parsed
        let source = "export const foo = 1; // fallow-ignore-file\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert!(
            suppressions.is_empty(),
            "Inline comment after code should not be parsed as suppression"
        );
    }

    #[test]
    fn parse_block_comment_without_closing_ignored() {
        // A block comment that doesn't end with */ should not be parsed
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
        // Offset beyond source length should be clamped
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
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedFile));
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
}
