//! Mutations of a regular expression literal.
//!
//! Each mutation changes one token of the pattern:
//!
//! - a `^` at the start of an alternative is removed: at the pattern start,
//!   or right after `(`, `(?:` or `|`;
//! - a `$` at the end of an alternative is removed: at the pattern end, or
//!   right before `)` or `|`;
//! - a `+`, `*` or `?` quantifier is removed, with its lazy `?`;
//! - a `{n,m}` or `{n,}` quantifier becomes `{n}`;
//! - a character class is negated (`[a-z]` to `[^a-z]`, `[^a-z]` to `[a-z]`);
//! - a class escape gets the opposite case (`\d` to `\D`, `\W` to `\w`, and
//!   the same for `\s`).
//!
//! The flags stay the same. A pattern with the `v` flag gives no class
//! negation, because a negated class with strings is a syntax error in that
//! mode. An empty pattern becomes `(?:)`, because `//` starts a comment.

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
    edits(pattern, flags.contains('v'))
        .into_iter()
        .map(|(start, end, text, change)| {
            let mutated = format!("{}{text}{}", &pattern[..start], &pattern[end..]);
            let body = if mutated.is_empty() { "(?:)" } else { &mutated };
            (format!("/{body}/{flags}"), change)
        })
        .collect()
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
    let bytes = pattern.as_bytes();
    let mut edits = Vec::new();
    let mut depth = 0_usize;
    let mut index = 0;
    // True when the previous token can take a quantifier. After `(`, `|`, an
    // anchor, a lookaround open or the pattern start, a `?` or `*` is either
    // group syntax or a syntax error, so it gets no edit.
    let mut quantifiable = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if depth > 0 {
            match byte {
                b'\\' => {
                    if let Some((swap, change)) =
                        bytes.get(index + 1).and_then(|&b| swapped_class_escape(b))
                    {
                        edits.push((index + 1, index + 2, swap, change));
                    }
                    index += 1 + escaped_len(pattern, index);
                    continue;
                }
                b'[' if sets => depth += 1,
                b']' => {
                    depth -= 1;
                    quantifiable = depth == 0;
                }
                _ => {}
            }
            index += 1;
            continue;
        }
        match byte {
            b'\\' => {
                let next = bytes.get(index + 1).copied();
                if let Some((swap, change)) = next.and_then(swapped_class_escape) {
                    edits.push((index + 1, index + 2, swap, change));
                }
                quantifiable = !matches!(next, Some(b'b' | b'B'));
                index += 1 + escaped_len(pattern, index);
                continue;
            }
            b'[' => {
                if !sets {
                    if bytes.get(index + 1) == Some(&b'^') {
                        edits.push((index + 1, index + 2, "", MutantChange::new("[^", "[")));
                    } else {
                        edits.push((index + 1, index + 1, "^", MutantChange::new("[", "[^")));
                    }
                }
                depth = 1;
            }
            b'^' if anchor_start(bytes, index) => {
                edits.push((index, index + 1, "", MutantChange::new("^", "")));
                quantifiable = false;
            }
            b'$' if matches!(bytes.get(index + 1), None | Some(b')' | b'|')) => {
                edits.push((index, index + 1, "", MutantChange::new("$", "")));
                quantifiable = false;
            }
            b'(' => {
                index += group_open_len(bytes, index);
                quantifiable = false;
                continue;
            }
            b'|' | b'^' | b'$' => quantifiable = false,
            b'+' | b'*' | b'?' if quantifiable => {
                let lazy = usize::from(bytes.get(index + 1) == Some(&b'?'));
                let end = index + 1 + lazy;
                let from = match byte {
                    b'+' => "+",
                    b'*' => "*",
                    _ => "?",
                };
                edits.push((index, end, "", MutantChange::new(from, "")));
                quantifiable = false;
                index = end;
                continue;
            }
            b'{' if quantifiable => {
                if let Some((len, low_end)) = brace_quantifier(&bytes[index..]) {
                    let close = index + len - 1;
                    if index + low_end < close {
                        edits.push((
                            index + low_end,
                            close,
                            "",
                            MutantChange::new("{n,m}", "{n}"),
                        ));
                    }
                    quantifiable = false;
                    index += len;
                    if bytes.get(index) == Some(&b'?') {
                        index += 1;
                    }
                    continue;
                }
                quantifiable = true;
            }
            _ => quantifiable = true,
        }
        index += 1;
    }
    edits
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
