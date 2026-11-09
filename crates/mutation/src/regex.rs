//! Mutations of a regular expression literal.
//!
//! Each mutation changes one token of the pattern:
//!
//! - a `^` at the start of an alternative is removed: at the pattern start,
//!   or right after `(`, `(?:` or `|`;
//! - a `$` at the end of an alternative is removed: at the pattern end, or
//!   right before `)` or `|`;
//! - a `+`, `*` or `?` quantifier is removed, with its lazy `?`. When the
//!   removal would join the atom before it with the next text into another
//!   token, an empty group `(?:)` takes its place: `\1?2` becomes `\1(?:)2`,
//!   not the escape `\12`;
//! - a `{n,m}` or `{n,}` quantifier becomes `{n}`;
//! - a character class is negated (`[a-z]` to `[^a-z]`, `[^a-z]` to `[a-z]`);
//! - a class escape gets the opposite case (`\d` to `\D`, `\W` to `\w`, and
//!   the same for `\s`).
//!
//! The flags stay the same. A pattern with the `v` flag gives no class
//! negation, because a negated class with strings is a syntax error in that
//! mode. An empty pattern becomes `(?:)`, because `//` starts a comment.
//!
//! A mutated literal that does not parse is dropped, when the original
//! literal parses. An invalid literal is an early error, so it would break
//! the whole instrumented file, also while the mutant is inactive.

use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

use crate::MutantChange;

/// The mutated literals of `raw`, the source text of a regular expression
/// literal, with the changed token, in pattern order.
pub fn mutations(raw: &str) -> Vec<(String, MutantChange)> {
    let Some(close) = raw.rfind('/').filter(|&close| close > 0) else {
        return Vec::new();
    };
    let (Some(pattern), Some(flags)) = (raw.get(1..close), raw.get(close + 1..)) else {
        return Vec::new();
    };
    let checked = is_valid_literal(raw);
    edits(pattern, flags.contains('v'))
        .into_iter()
        .map(|(start, end, text, change)| {
            let mutated = format!("{}{text}{}", &pattern[..start], &pattern[end..]);
            let body = if mutated.is_empty() { "(?:)" } else { &mutated };
            (format!("/{body}/{flags}"), change)
        })
        .filter(|(literal, _)| !checked || is_valid_literal(literal))
        .collect()
}

/// True when `literal` parses as a regular expression literal, pattern and
/// flags included.
fn is_valid_literal(literal: &str) -> bool {
    let allocator = Allocator::default();
    let source = format!("({literal});");
    let options = ParseOptions {
        parse_regular_expression: true,
        ..ParseOptions::default()
    };
    let parsed = Parser::new(&allocator, &source, SourceType::mjs())
        .with_options(options)
        .parse();
    !parsed.fatal_error && !parsed.diagnostics.has_errors()
}

/// True when the text `before` a removed quantifier and the text `after` it
/// would form one token without the quantifier:
///
/// - a decimal escape and a digit (`\1?2` to `\12`);
/// - a `\x` or `\u` escape with too few hex digits and a hex digit
///   (`\x?41` to `\x41`), or `\u` and `{`;
/// - `\c` and a letter, or `\k` and `<`;
/// - a literal `{` and the rest of a brace quantifier (`{?3}` to `{3}`).
///
/// An escaped backslash before the atom can also match. The empty group
/// that replaces the quantifier is then not needed, but it does no harm.
fn joins(before: &[u8], after: &[u8]) -> bool {
    let Some(&next) = after.first() else {
        return false;
    };
    let trailing = |pred: fn(&u8) -> bool| before.iter().rev().take_while(|b| pred(b)).count();
    let digits = trailing(u8::is_ascii_digit);
    if digits > 0 && before[..before.len() - digits].ends_with(b"\\") && next.is_ascii_digit() {
        return true;
    }
    let hex = trailing(u8::is_ascii_hexdigit);
    let head = &before[..before.len() - hex];
    let short_hex = (head.ends_with(b"\\x") && hex < 2) || (head.ends_with(b"\\u") && hex < 4);
    if short_hex && (next.is_ascii_hexdigit() || (hex == 0 && next == b'{')) {
        return true;
    }
    if (before.ends_with(b"\\c") && next.is_ascii_alphabetic())
        || (before.ends_with(b"\\k") && next == b'<')
    {
        return true;
    }
    let tail = trailing(|b| b.is_ascii_digit() || *b == b',');
    let brace = before.len() - tail;
    if brace > 0 && before[brace - 1] == b'{' {
        let mut text = before[brace - 1..].to_vec();
        text.extend_from_slice(after);
        return brace_quantifier(&text).is_some();
    }
    false
}

/// The opposite-case class escape of `byte`, and the change of the escape.
const fn swapped_class_escape(byte: u8) -> Option<(&'static str, MutantChange)> {
    let (swap, from, to) = match byte {
        b'd' => ("D", "\\d", "\\D"),
        b'D' => ("d", "\\D", "\\d"),
        b'w' => ("W", "\\w", "\\W"),
        b'W' => ("w", "\\W", "\\w"),
        b's' => ("S", "\\s", "\\S"),
        b'S' => ("s", "\\S", "\\s"),
        _ => return None,
    };
    Some((swap, MutantChange::new(from, to)))
}

/// One edit of a pattern: `(start, end, replacement, change)`.
type Edit = (usize, usize, &'static str, MutantChange);

/// The byte length of a `{n}`, `{n,}` or `{n,m}` quantifier at the start of
/// `rest`, and the end of its `n` part. A brace that does not start such a
/// quantifier is a literal `{` outside Unicode mode.
fn brace_quantifier(rest: &[u8]) -> Option<(usize, usize)> {
    let digits = |from: usize| {
        rest.get(from..).map_or(0, |tail| {
            tail.iter().take_while(|b| b.is_ascii_digit()).count()
        })
    };
    let low = digits(1);
    if low == 0 {
        return None;
    }
    let low_end = 1 + low;
    match rest.get(low_end) {
        Some(b'}') => Some((low_end + 1, low_end)),
        Some(b',') => {
            let close = low_end + 1 + digits(low_end + 1);
            (rest.get(close) == Some(&b'}')).then_some((close + 1, low_end))
        }
        _ => None,
    }
}

/// The edits of `pattern`, in pattern order.
fn edits(pattern: &str, sets: bool) -> Vec<Edit> {
    let mut scanner = Scanner {
        pattern,
        bytes: pattern.as_bytes(),
        sets,
        edits: Vec::new(),
        depth: 0,
        quantifiable: false,
    };
    let mut index = 0;
    while index < scanner.bytes.len() {
        index = if scanner.depth > 0 {
            scanner.class_token(index)
        } else {
            scanner.token(index)
        };
    }
    scanner.edits
}

/// A left-to-right scan of a pattern. Each token method takes the index of
/// the token and returns the index after it.
struct Scanner<'p> {
    pattern: &'p str,
    bytes: &'p [u8],
    /// True for the `v` flag, where classes nest.
    sets: bool,
    edits: Vec<Edit>,
    /// The depth of character classes at the scan position.
    depth: usize,
    /// True when the previous token can take a quantifier. After `(`, `|`,
    /// an anchor, a lookaround open or the pattern start, a `?` or `*` is
    /// group syntax or a syntax error, so it gets no edit.
    quantifiable: bool,
}

impl Scanner<'_> {
    /// An escape: the class escape swap, and the index after the escape.
    fn escape(&mut self, index: usize) -> usize {
        if let Some((swap, change)) = self
            .bytes
            .get(index + 1)
            .and_then(|&b| swapped_class_escape(b))
        {
            self.edits.push((index + 1, index + 2, swap, change));
        }
        index + 1 + escaped_len(self.pattern, index)
    }

    /// A token inside a character class.
    fn class_token(&mut self, index: usize) -> usize {
        match self.bytes[index] {
            b'\\' => return self.escape(index),
            b'[' if self.sets => self.depth += 1,
            b']' => {
                self.depth -= 1;
                self.quantifiable = self.depth == 0;
            }
            _ => {}
        }
        index + 1
    }

    /// A token outside a character class.
    fn token(&mut self, index: usize) -> usize {
        let byte = self.bytes[index];
        match byte {
            b'\\' => {
                self.quantifiable = !matches!(self.bytes.get(index + 1), Some(b'b' | b'B'));
                return self.escape(index);
            }
            b'[' => self.class_open(index),
            b'^' if anchor_start(self.bytes, index) => self.removal(index, "^"),
            b'$' if matches!(self.bytes.get(index + 1), None | Some(b')' | b'|')) => {
                self.removal(index, "$");
            }
            b'(' => {
                self.quantifiable = false;
                return index + group_open_len(self.bytes, index);
            }
            b'|' | b'^' | b'$' => self.quantifiable = false,
            b'+' | b'*' | b'?' if self.quantifiable => return self.quantifier(index, byte),
            b'{' if self.quantifiable => {
                if let Some(next) = self.brace(index) {
                    return next;
                }
            }
            _ => self.quantifiable = true,
        }
        index + 1
    }

    fn class_open(&mut self, index: usize) {
        if !self.sets {
            if self.bytes.get(index + 1) == Some(&b'^') {
                self.edits
                    .push((index + 1, index + 2, "", MutantChange::new("[^", "[")));
            } else {
                self.edits
                    .push((index + 1, index + 1, "^", MutantChange::new("[", "[^")));
            }
        }
        self.depth = 1;
    }

    /// Removes the anchor `token` at `index`.
    fn removal(&mut self, index: usize, token: &'static str) {
        self.edits
            .push((index, index + 1, "", MutantChange::new(token, "")));
        self.quantifiable = false;
    }

    /// Removes a `+`, `*` or `?` quantifier with its lazy `?`.
    fn quantifier(&mut self, index: usize, byte: u8) -> usize {
        let lazy = usize::from(self.bytes.get(index + 1) == Some(&b'?'));
        let end = index + 1 + lazy;
        let from = match byte {
            b'+' => "+",
            b'*' => "*",
            _ => "?",
        };
        let text = if joins(&self.bytes[..index], &self.bytes[end..]) {
            "(?:)"
        } else {
            ""
        };
        self.edits
            .push((index, end, text, MutantChange::new(from, "")));
        self.quantifiable = false;
        end
    }

    /// Shortens a `{n,m}` or `{n,}` quantifier to `{n}`. Returns `None` for
    /// a brace that is not a quantifier.
    fn brace(&mut self, index: usize) -> Option<usize> {
        let (len, low_end) = brace_quantifier(&self.bytes[index..])?;
        let close = index + len - 1;
        if index + low_end < close {
            self.edits.push((
                index + low_end,
                close,
                "",
                MutantChange::new("{n,m}", "{n}"),
            ));
        }
        self.quantifiable = false;
        let end = index + len;
        Some(end + usize::from(self.bytes.get(end) == Some(&b'?')))
    }
}

/// The byte length of the character after the backslash at `index`.
fn escaped_len(pattern: &str, index: usize) -> usize {
    pattern
        .get(index + 1..)
        .and_then(|rest| rest.chars().next())
        .map_or(0, char::len_utf8)
}

/// True when the `^` at `index` is an anchor that can go: at the pattern
/// start, or right after `(`, `(?:` or `|`. A `^` in a lookaround stays,
/// because `(?<=)` with an empty body is a different assertion.
fn anchor_start(bytes: &[u8], index: usize) -> bool {
    let before = &bytes[..index];
    before.is_empty()
        || before.ends_with(b"|")
        || before.ends_with(b"(?:")
        || (before.ends_with(b"(") && !before.ends_with(b"\\("))
}

/// The byte length of the group opening at `index`: `(`, `(?:`, `(?=`,
/// `(?!`, `(?<=`, `(?<!` or `(?<name>`. The `?` of the opening is not a
/// quantifier.
fn group_open_len(bytes: &[u8], index: usize) -> usize {
    let rest = &bytes[index..];
    if rest.get(1) != Some(&b'?') {
        return 1;
    }
    match rest.get(2) {
        Some(b'<') if matches!(rest.get(3), Some(b'=' | b'!')) => 4,
        Some(b'<') => rest
            .iter()
            .position(|&b| b == b'>')
            .map_or(rest.len(), |close| close + 1),
        Some(_) => 3,
        None => 2,
    }
}
