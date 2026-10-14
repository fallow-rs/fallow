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

/// The mutated literals of `raw`, the source text of a regular expression
/// literal, in pattern order.
pub fn mutations(raw: &str) -> Vec<String> {
    let Some(close) = raw.rfind('/').filter(|&close| close > 0) else {
        return Vec::new();
    };
    let (Some(pattern), Some(flags)) = (raw.get(1..close), raw.get(close + 1..)) else {
        return Vec::new();
    };
    edits(pattern, flags.contains('v'))
        .into_iter()
        .map(|(start, end, text)| {
            let mutated = format!("{}{text}{}", &pattern[..start], &pattern[end..]);
            let body = if mutated.is_empty() { "(?:)" } else { &mutated };
            format!("/{body}/{flags}")
        })
        .collect()
}

/// The opposite-case class escape of `byte`.
const fn swapped_class_escape(byte: u8) -> Option<&'static str> {
    match byte {
        b'd' => Some("D"),
        b'D' => Some("d"),
        b'w' => Some("W"),
        b'W' => Some("w"),
        b's' => Some("S"),
        b'S' => Some("s"),
        _ => None,
    }
}

/// The edits of `pattern` as `(start, end, replacement)`, in pattern order.
fn edits(pattern: &str, sets: bool) -> Vec<(usize, usize, &'static str)> {
    let bytes = pattern.as_bytes();
    let mut edits = Vec::new();
    if bytes.first() == Some(&b'^') {
        edits.push((0, 1, ""));
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
                if let Some(swap) = bytes.get(index + 1).and_then(|&b| swapped_class_escape(b)) {
                    edits.push((index + 1, index + 2, swap));
                }
                index += 1 + next_len;
                continue;
            }
            b'[' if depth == 0 || sets => {
                if depth == 0 && !sets {
                    if bytes.get(index + 1) == Some(&b'^') {
                        edits.push((index + 1, index + 2, ""));
                    } else {
                        edits.push((index + 1, index + 1, "^"));
                    }
                }
                depth += 1;
            }
            b']' if depth > 0 => depth -= 1,
            b'$' if depth == 0 && index + 1 == bytes.len() => edits.push((index, index + 1, "")),
            _ => {}
        }
        index += 1;
    }
    edits
}
