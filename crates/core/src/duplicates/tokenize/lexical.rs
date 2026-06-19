use oxc_span::Span;

use crate::duplicates::tokenize::{OperatorType, PunctuationType, SourceToken, TokenKind};

/// Tokenize authored non-JS regions such as CSS-family source and markup.
#[must_use]
pub(super) fn tokenize_lexical_region(source: &str, byte_offset: usize) -> Vec<SourceToken> {
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

        if let Some((tok, next)) = scan_lexical_token(source, cursor, ch, byte_offset) {
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
) -> Option<(SourceToken, usize)> {
    if let Some(scanned) = scan_value_token(source, cursor, ch, byte_offset) {
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

    if ch.is_ascii_digit() {
        let next = scan_number(source, cursor);
        return Some((
            token(
                TokenKind::NumericLiteral(source[cursor..next].to_string()),
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
