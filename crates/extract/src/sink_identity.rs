//! Reformat-independent text of a security sink expression.
//!
//! The stable id of a security finding hashes this text. The extractor
//! records the byte span of the full sink expression. This module lexes the
//! source in that span with the Oxc parser and joins the token texts. The
//! result does not depend on whitespace, comments, line ends or trailing
//! commas. The text inside string, template and regex literals stays as it
//! is.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_parser::config::TokensParserConfig;
use oxc_parser::{Kind, ParseOptions, Parser};
use oxc_span::SourceType;

/// Wrappers that make the sink span a complete program.
///
/// A call, a `new` expression, an assignment and a tagged template parse
/// inside parentheses. An object property (`key: value`) parses inside an
/// object literal. A JSX attribute (`name={value}`) parses inside a JSX
/// element. The line end before each suffix keeps a line comment at the end
/// of the span away from the suffix.
const WRAPPERS: &[(&str, &str)] = &[("(", "\n)"), ("({", "\n})"), ("<x ", "\n/>")];

/// The separator between two token texts in the result. Without it, two
/// tokens such as `typeof` and `x` would give the same text as the one
/// identifier `typeofx`.
const TOKEN_SEPARATOR: char = ' ';

/// The normalized text of the sink expression at bytes `start..end` of
/// `source`, the full text of the file at `path`.
///
/// The offsets are the offsets of the extractor: they count from the end of
/// a leading byte order mark. CRLF line ends count as LF. Returns `None` when
/// the span is not in `source`, or when the span text does not parse as one
/// complete expression, property or JSX attribute without a syntax error.
#[must_use]
pub fn sink_identity_text(path: &Path, source: &str, start: u32, end: u32) -> Option<String> {
    let source = crate::strip_bom(source);
    let span = source.get(usize::try_from(start).ok()?..usize::try_from(end).ok()?)?;
    if span.trim().is_empty() {
        return None;
    }
    let span = span.replace("\r\n", "\n");
    let source_types = candidate_source_types(path);
    WRAPPERS.iter().find_map(|&(prefix, suffix)| {
        source_types
            .iter()
            .find_map(|&source_type| tokens_text(&span, prefix, suffix, source_type))
    })
}

/// The source types to try for a span in the file at `path`. A file that is
/// not a JavaScript or TypeScript file (Vue, Svelte, Astro, MDX, HTML) holds
/// TypeScript or TSX in its script blocks.
fn candidate_source_types(path: &Path) -> Vec<SourceType> {
    let mut types = Vec::with_capacity(3);
    if let Ok(source_type) = SourceType::from_path(path) {
        types.push(source_type.with_module(true));
    }
    for fallback in [SourceType::ts(), SourceType::tsx()] {
        if !types.contains(&fallback) {
            types.push(fallback);
        }
    }
    types
}

/// The joined token texts of `span`, parsed as `prefix + span + suffix`.
/// Returns `None` when the parse reports an error, or when a part of `span`
/// that is not whitespace is not in a token or a comment.
fn tokens_text(span: &str, prefix: &str, suffix: &str, source_type: SourceType) -> Option<String> {
    let program = format!("{prefix}{span}{suffix}");
    let lo = u32::try_from(prefix.len()).ok()?;
    let hi = u32::try_from(prefix.len() + span.len()).ok()?;
    crate::parse_bounded(&program, || {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, &program, source_type)
            .with_options(ParseOptions {
                allow_return_outside_function: true,
                ..ParseOptions::default()
            })
            .with_config(TokensParserConfig)
            .parse();
        if parsed.fatal_error || !parsed.diagnostics.is_empty() {
            return None;
        }
        let tokens: Vec<(u32, u32, Kind)> = parsed
            .tokens
            .iter()
            .filter(|token| token.kind() != Kind::Eof)
            .filter(|token| token.start() >= lo && token.end() <= hi)
            .map(|token| (token.start(), token.end(), token.kind()))
            .collect();
        let mut covered: Vec<(u32, u32)> = tokens.iter().map(|&(s, e, _)| (s, e)).collect();
        covered.extend(
            parsed
                .program
                .comments
                .iter()
                .filter(|comment| comment.span.start >= lo && comment.span.end <= hi)
                .map(|comment| (comment.span.start, comment.span.end)),
        );
        if !only_whitespace_outside(&program, lo, hi, covered) {
            return None;
        }
        Some(join_tokens(&program, &tokens))
    })
    .flatten()
}

/// True when every byte of `text[lo..hi]` outside the `covered` ranges is
/// whitespace.
fn only_whitespace_outside(text: &str, lo: u32, hi: u32, mut covered: Vec<(u32, u32)>) -> bool {
    covered.sort_unstable();
    let mut cursor = lo;
    for (start, end) in covered.into_iter().chain([(hi, hi)]) {
        if start > cursor {
            let Some(gap) = text.get(cursor as usize..start as usize) else {
                return false;
            };
            if !gap.chars().all(char::is_whitespace) {
                return false;
            }
        }
        cursor = cursor.max(end);
    }
    true
}

/// Join the token texts with [`TOKEN_SEPARATOR`]. A comma before a closing
/// bracket is dropped, because a formatter adds or removes it when it wraps
/// a call, an array or an object.
fn join_tokens(program: &str, tokens: &[(u32, u32, Kind)]) -> String {
    let mut out = String::new();
    for (index, &(start, end, kind)) in tokens.iter().enumerate() {
        let next = tokens.get(index + 1).map(|&(_, _, kind)| kind);
        if kind == Kind::Comma && matches!(next, Some(Kind::RParen | Kind::RBrack | Kind::RCurly)) {
            continue;
        }
        if !out.is_empty() {
            out.push(TOKEN_SEPARATOR);
        }
        out.push_str(&program[start as usize..end as usize]);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::sink_identity_text;

    fn text(source: &str, needle_start: &str, needle_end: &str) -> Option<String> {
        let start = source.find(needle_start)?;
        let end = source[start..].find(needle_end)? + start + needle_end.len();
        sink_identity_text(
            Path::new("src/a.ts"),
            source,
            u32::try_from(start).ok()?,
            u32::try_from(end).ok()?,
        )
    }

    #[test]
    fn a_reformat_of_a_call_keeps_the_text() {
        let one_line = "exec(`git ${a}`, { cwd: dir })\n";
        let wrapped = "exec(\n  `git ${a}`, // the command\n  {\n    cwd: dir,\n  },\n)\n";
        let one = text(one_line, "exec", "})").expect("one-line text");
        assert_eq!(one, "exec ( `git ${ a }` , { cwd : dir } )");
        assert_eq!(
            text(wrapped, "exec", "},\n)").as_deref(),
            Some(one.as_str())
        );
        let crlf = wrapped.replace('\n', "\r\n");
        assert_eq!(
            text(&crlf, "exec", "},\r\n)").as_deref(),
            Some(one.as_str())
        );
    }

    #[test]
    fn literal_text_stays_as_it_is() {
        let source = "el.innerHTML = s.replace(/\\/\\//g, '  a  ') + `x  ${y}`";
        assert_eq!(
            text(source, "el", "}`").as_deref(),
            Some("el . innerHTML = s . replace ( /\\/\\//g , '  a  ' ) + `x  ${ y }`")
        );
    }

    #[test]
    fn a_jsx_attribute_and_an_object_property_have_text() {
        let jsx = "<div dangerouslySetInnerHTML={{ __html: html }} />";
        assert_eq!(
            text(jsx, "dangerously", "}}").as_deref(),
            Some("dangerouslySetInnerHTML = { { __html : html } }")
        );
        let property = "const props = { dangerouslySetInnerHTML: { __html: html } }";
        assert_eq!(
            text(property, "dangerously", "html }").as_deref(),
            Some("dangerouslySetInnerHTML : { __html : html }")
        );
    }

    #[test]
    fn a_leading_byte_order_mark_does_not_move_the_span() {
        let source = "exec(cmd)\n";
        let with_bom = format!("\u{feff}{source}");
        let plain = sink_identity_text(Path::new("a.js"), source, 0, 9);
        assert_eq!(plain.as_deref(), Some("exec ( cmd )"));
        assert_eq!(
            sink_identity_text(Path::new("a.js"), &with_bom, 0, 9),
            plain
        );
    }

    #[test]
    fn a_span_that_is_not_a_complete_expression_has_no_text() {
        let source = "exec(cmd, other)\n";
        assert_eq!(sink_identity_text(Path::new("a.js"), source, 0, 8), None);
        assert_eq!(sink_identity_text(Path::new("a.js"), source, 0, 400), None);
        assert_eq!(sink_identity_text(Path::new("a.js"), source, 4, 4), None);
    }
}
