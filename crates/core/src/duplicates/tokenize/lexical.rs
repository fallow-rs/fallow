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

        if ch.is_whitespace() {
            cursor += ch.len_utf8();
            continue;
        }

        if source[cursor..].starts_with("/*") {
            cursor = source[cursor + 2..]
                .find("*/")
                .map_or(source.len(), |end| cursor + 2 + end + 2);
            continue;
        }

        if source[cursor..].starts_with("//") {
            cursor = source[cursor..]
                .find('\n')
                .map_or(source.len(), |end| cursor + end);
            continue;
        }

        if matches!(ch, '"' | '\'' | '`') {
            let (literal, next) = scan_string(source, cursor, ch);
            tokens.push(token(
                TokenKind::StringLiteral(literal),
                byte_offset + cursor,
                byte_offset + next,
            ));
            cursor = next;
            continue;
        }

        if ch.is_ascii_digit() {
            let next = scan_number(source, cursor);
            tokens.push(token(
                TokenKind::NumericLiteral(source[cursor..next].to_string()),
                byte_offset + cursor,
                byte_offset + next,
            ));
            cursor = next;
            continue;
        }

        if is_identifier_start(ch, source, cursor) {
            let next = scan_identifier(source, cursor);
            tokens.push(token(
                TokenKind::Identifier(source[cursor..next].to_ascii_lowercase()),
                byte_offset + cursor,
                byte_offset + next,
            ));
            cursor = next;
            continue;
        }

        if let Some(kind) = punctuation(ch) {
            let end = cursor + ch.len_utf8();
            tokens.push(token(
                TokenKind::Punctuation(kind),
                byte_offset + cursor,
                byte_offset + end,
            ));
            cursor = end;
            continue;
        }

        if let Some(kind) = operator(ch) {
            let end = cursor + ch.len_utf8();
            tokens.push(token(
                TokenKind::Operator(kind),
                byte_offset + cursor,
                byte_offset + end,
            ));
            cursor = end;
            continue;
        }

        cursor += ch.len_utf8();
    }

    tokens
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
