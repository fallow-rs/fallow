//! Mutations of a regular expression literal.
//!
//! Each mutation changes one token of the pattern:
//!
//! - a `^` at the start of the pattern is removed;
//! - a `$` at the end of the pattern is removed;
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

/// The edits of `pattern`, in pattern order.
fn edits(pattern: &str, sets: bool) -> Vec<Edit> {
    let bytes = pattern.as_bytes();
    let mut edits = Vec::new();
    if bytes.first() == Some(&b'^') {
        edits.push((0, 1, "", MutantChange::new("^", "")));
    }
    let mut depth = 0_usize;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                let next_len = pattern
                    .get(index + 1..)
                    .and_then(|rest| rest.chars().next())
                    .map_or(0, char::len_utf8);
                if let Some((swap, change)) =
                    bytes.get(index + 1).and_then(|&b| swapped_class_escape(b))
                {
                    edits.push((index + 1, index + 2, swap, change));
                }
                index += 1 + next_len;
                continue;
            }
            b'[' if depth == 0 || sets => {
                if depth == 0 && !sets {
                    if bytes.get(index + 1) == Some(&b'^') {
                        edits.push((index + 1, index + 2, "", MutantChange::new("[^", "[")));
                    } else {
                        edits.push((index + 1, index + 1, "^", MutantChange::new("[", "[^")));
                    }
                }
                depth += 1;
            }
            b']' if depth > 0 => depth -= 1,
            b'$' if depth == 0 && index + 1 == bytes.len() => {
                edits.push((index, index + 1, "", MutantChange::new("$", "")));
            }
            _ => {}
        }
        index += 1;
    }
    edits
}
