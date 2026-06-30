use oxc_span::Span;

use crate::duplicates::tokenize::{OperatorType, PunctuationType, SourceToken, TokenKind};

/// Tokenize authored non-JS regions such as CSS-family source and markup.
///
/// When `css` is true the value scanner canonicalizes CSS values so that
/// semantically-equal CSS hashes equal: a zero-valued numeric collapses its unit
/// (`0px`/`0em`/`0%`/`0.0` -> `0`) and a hex color is expanded to its long,
/// lowercased form (`#FFF`/`#fff` -> `#ffffff`, `#abcd` -> `#aabbccdd`). This is
/// the layer that lets the SA-IS clone engine catch near-miss / value-drifted CSS
/// clones. It is reachable ONLY from the `"style"` path (CSS-family files and SFC
/// `<style>` regions), so JS and markup tokenization is provably untouched.
#[must_use]
pub(super) fn tokenize_lexical_region(
    source: &str,
    byte_offset: usize,
    css: bool,
) -> Vec<SourceToken> {
    let mut tokens = Vec::new();
    let mut cursor = 0;

    while cursor < source.len() {
        let Some((relative, ch)) = source[cursor..].char_indices().next() else {
            break;
        };
        cursor += relative;

        if let Some(next) = skip_trivia(source, cursor, ch) {
            cursor = next;
            continue;
        }

        if let Some((tok, next)) = scan_lexical_token(source, cursor, ch, byte_offset, css) {
            tokens.push(tok);
            cursor = next;
            continue;
        }

        cursor += ch.len_utf8();
    }

    tokens
}

/// Skip whitespace and CSS/JS comments, returning the post-trivia cursor when
/// the current position is trivia, or `None` when it begins a real token.
fn skip_trivia(source: &str, cursor: usize, ch: char) -> Option<usize> {
    if ch.is_whitespace() {
        return Some(cursor + ch.len_utf8());
    }
    if source[cursor..].starts_with("/*") {
        return Some(
            source[cursor + 2..]
                .find("*/")
                .map_or(source.len(), |end| cursor + 2 + end + 2),
        );
    }
    if source[cursor..].starts_with("//") {
        return Some(
            source[cursor..]
                .find('\n')
                .map_or(source.len(), |end| cursor + end),
        );
    }
    None
}

/// Scan a single literal, identifier, punctuation, or operator token starting at
/// `cursor`, returning the token plus the new cursor, or `None` for an
/// unrecognized character the caller should skip.
fn scan_lexical_token(
    source: &str,
    cursor: usize,
    ch: char,
    byte_offset: usize,
    css: bool,
) -> Option<(SourceToken, usize)> {
    if let Some(scanned) = scan_value_token(source, cursor, ch, byte_offset, css) {
        return Some(scanned);
    }

    if let Some(kind) = punctuation(ch) {
        let end = cursor + ch.len_utf8();
        return Some((
            token(
                TokenKind::Punctuation(kind),
                byte_offset + cursor,
                byte_offset + end,
            ),
            end,
        ));
    }

    if let Some(kind) = operator(ch) {
        let end = cursor + ch.len_utf8();
        return Some((
            token(
                TokenKind::Operator(kind),
                byte_offset + cursor,
                byte_offset + end,
            ),
            end,
        ));
    }

    None
}

/// Scan a string, numeric, or identifier token (the multi-character value
/// tokens), returning the token plus the new cursor, or `None` if `ch` does not
/// begin one.
fn scan_value_token(
    source: &str,
    cursor: usize,
    ch: char,
    byte_offset: usize,
    css: bool,
) -> Option<(SourceToken, usize)> {
    if matches!(ch, '"' | '\'' | '`') {
        let (literal, next) = scan_string(source, cursor, ch);
        return Some((
            token(
                TokenKind::StringLiteral(literal),
                byte_offset + cursor,
                byte_offset + next,
            ),
            next,
        ));
    }

    // CSS hex color: `#` followed by 3/4/6/8 hex digits, canonicalized to the
    // long lowercased form so `#fff` and `#ffffff` hash equal. Only in CSS mode;
    // otherwise `#` is not an identifier start and is skipped as before (so an
    // id selector `#main` keeps its current tokenization).
    if css
        && ch == '#'
        && let Some((color, next)) = scan_css_hex_color(source, cursor)
    {
        return Some((
            token(
                TokenKind::Identifier(color),
                byte_offset + cursor,
                byte_offset + next,
            ),
            next,
        ));
    }

    if ch.is_ascii_digit() {
        let next = scan_number(source, cursor);
        let raw = &source[cursor..next];
        let value = if css {
            canonicalize_css_numeric(raw)
        } else {
            raw.to_string()
        };
        return Some((
            token(
                TokenKind::NumericLiteral(value),
                byte_offset + cursor,
                byte_offset + next,
            ),
            next,
        ));
    }

    if is_identifier_start(ch, source, cursor) {
        let next = scan_identifier(source, cursor);
        return Some((
            token(
                TokenKind::Identifier(source[cursor..next].to_ascii_lowercase()),
                byte_offset + cursor,
                byte_offset + next,
            ),
            next,
        ));
    }

    None
}

pub(super) fn boundary_token(name: &str, byte_offset: usize) -> SourceToken {
    token(
        TokenKind::Boundary(name.to_string()),
        byte_offset,
        byte_offset,
    )
}

fn token(kind: TokenKind, start: usize, end: usize) -> SourceToken {
    SourceToken {
        kind,
        span: Span::new(start as u32, end as u32),
    }
}

fn scan_string(source: &str, start: usize, quote: char) -> (String, usize) {
    let mut out = String::new();
    let mut escaped = false;
    let mut cursor = start + quote.len_utf8();
    for (relative, ch) in source[cursor..].char_indices() {
        let absolute = cursor + relative;
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return (out, absolute + ch.len_utf8());
        }
        out.push(ch);
    }
    cursor = source.len();
    (out, cursor)
}

fn scan_number(source: &str, start: usize) -> usize {
    source[start..]
        .char_indices()
        .find_map(|(idx, ch)| {
            (!ch.is_ascii_digit() && ch != '.' && ch != '%' && !ch.is_ascii_alphabetic())
                .then_some(start + idx)
        })
        .unwrap_or(source.len())
}

/// Canonicalize a CSS numeric token so zero-with-unit collapses to a bare `0`:
/// `0px` / `0em` / `0%` / `0.0` all hash as `0`, while a non-zero value keeps its
/// unit verbatim (`16px` stays `16px`). `scan_number` folds the trailing unit into
/// the token, so the unit lives in `raw`; the leading numeric run is parsed to
/// decide zeroness.
fn canonicalize_css_numeric(raw: &str) -> String {
    let numeric_len = raw
        .char_indices()
        .find_map(|(idx, ch)| (!ch.is_ascii_digit() && ch != '.').then_some(idx))
        .unwrap_or(raw.len());
    if raw[..numeric_len]
        .parse::<f64>()
        .is_ok_and(|value| value == 0.0)
    {
        "0".to_string()
    } else {
        raw.to_string()
    }
}

/// Scan a CSS hex color whose `#` is at `start`. A valid color is `#` followed by
/// exactly 3, 4, 6, or 8 hex digits and then a non-identifier boundary. Returns
/// the canonical long, lowercased form (`#fff` -> `#ffffff`, `#abcd` ->
/// `#aabbccdd`, `#FFFFFF` -> `#ffffff`) plus the end cursor, or `None` when the
/// `#` does not begin a hex color (e.g. an id selector `#main`), so the caller
/// falls back to the existing `#`-skipping behavior.
fn scan_css_hex_color(source: &str, start: usize) -> Option<(String, usize)> {
    let after_hash = start + 1;
    let digits_len = source[after_hash..]
        .char_indices()
        .find_map(|(idx, ch)| (!ch.is_ascii_hexdigit()).then_some(idx))
        .unwrap_or(source.len() - after_hash);
    let end = after_hash + digits_len;
    // The hex run must end at a token boundary, not mid-identifier (so `#deadbeef9`
    // or `#aabbccddee` are not misread as colors).
    if let Some(next) = source[end..].chars().next()
        && is_identifier_continue(next)
    {
        return None;
    }
    if !matches!(digits_len, 3 | 4 | 6 | 8) {
        return None;
    }
    let digits = source[after_hash..end].to_ascii_lowercase();
    let canonical = if matches!(digits_len, 3 | 4) {
        // Expand each shorthand digit to a pair: `fab` -> `ffaabb`.
        digits.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        digits
    };
    Some((format!("#{canonical}"), end))
}

fn is_identifier_start(ch: char, source: &str, start: usize) -> bool {
    ch.is_ascii_alphabetic()
        || ch == '_'
        || ch == '$'
        || ch == '@'
        || source[start..].starts_with("--")
}

fn is_identifier_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '$' | '@' | '#')
}

fn scan_identifier(source: &str, start: usize) -> usize {
    source[start..]
        .char_indices()
        .find_map(|(idx, ch)| (!is_identifier_continue(ch)).then_some(start + idx))
        .unwrap_or(source.len())
}

const fn punctuation(ch: char) -> Option<PunctuationType> {
    match ch {
        '(' => Some(PunctuationType::OpenParen),
        ')' => Some(PunctuationType::CloseParen),
        '{' => Some(PunctuationType::OpenBrace),
        '}' => Some(PunctuationType::CloseBrace),
        '[' => Some(PunctuationType::OpenBracket),
        ']' => Some(PunctuationType::CloseBracket),
        ';' => Some(PunctuationType::Semicolon),
        ':' => Some(PunctuationType::Colon),
        '.' => Some(PunctuationType::Dot),
        _ => None,
    }
}

const fn operator(ch: char) -> Option<OperatorType> {
    match ch {
        '=' => Some(OperatorType::Assign),
        '+' => Some(OperatorType::Add),
        '-' => Some(OperatorType::Sub),
        '*' => Some(OperatorType::Mul),
        '/' => Some(OperatorType::Div),
        '%' => Some(OperatorType::Mod),
        '<' => Some(OperatorType::Lt),
        '>' => Some(OperatorType::Gt),
        '!' => Some(OperatorType::Not),
        '&' => Some(OperatorType::BitwiseAnd),
        '|' => Some(OperatorType::BitwiseOr),
        ',' => Some(OperatorType::Comma),
        '?' => Some(OperatorType::Ternary),
        _ => None,
    }
}
