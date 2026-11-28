//! Statement boundaries around a removed class member.
//!
//! A class field with no `;` ends at a line break only when the next token
//! cannot continue its initializer or type (automatic semicolon insertion).
//! When the removed member was that next token, the following kept member can
//! join the field: `value = 1` followed by `[key]() {}` reads as
//! `value = 1[key]() {}`. An empty `;` class element is always valid, so the
//! fixer puts one in front of such a member.

use std::ops::Range;

/// Replacement for one removed member range.
pub(super) struct BoundaryEdit {
    pub(super) range: Range<usize>,
    pub(super) replacement: String,
}

/// Compute the edit for each removed range. `ranges` must be sorted by start
/// and must not overlap. The result has the same order and length.
pub(super) fn boundary_edits(content: &str, ranges: &[Range<usize>]) -> Vec<BoundaryEdit> {
    let bytes = content.as_bytes();
    let mut edits = Vec::with_capacity(ranges.len());
    let mut run_start: Option<usize> = None;
    for (index, range) in ranges.iter().enumerate() {
        let boundary_start = *run_start.get_or_insert(range.start);
        let next_token = next_token_start(bytes, range.end);
        let followed_by_removed = ranges
            .get(index + 1)
            .is_some_and(|next| next_token.is_none_or(|token| next.start <= token));
        if followed_by_removed {
            edits.push(plain_removal(range));
            continue;
        }
        run_start = None;
        let needs_semicolon = next_token.is_some_and(|token| {
            starts_a_continuation(&content[token..])
                && !previous_token_ends_element(bytes, boundary_start)
        });
        if !needs_semicolon {
            edits.push(plain_removal(range));
            continue;
        }
        let token = next_token.unwrap_or(range.end);
        edits.push(semicolon_edit(content, range, token));
    }
    edits
}

fn plain_removal(range: &Range<usize>) -> BoundaryEdit {
    BoundaryEdit {
        range: range.clone(),
        replacement: String::new(),
    }
}

fn semicolon_edit(content: &str, range: &Range<usize>, token: usize) -> BoundaryEdit {
    let bytes = content.as_bytes();
    let whole_lines = range.start == 0 || bytes[range.start - 1] == b'\n';
    if !whole_lines {
        return BoundaryEdit {
            range: range.clone(),
            replacement: ";".to_string(),
        };
    }
    let indent_len = bytes[range.start..range.end]
        .iter()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    let indent = &content[range.start..range.start + indent_len];
    if bytes[range.end..token].iter().all(u8::is_ascii_whitespace) {
        // `;` goes directly in front of the kept member, in the place of its
        // own indentation.
        return BoundaryEdit {
            range: range.start..token,
            replacement: format!("{indent};"),
        };
    }
    let line_ending = if content[..range.end].ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    BoundaryEdit {
        range: range.clone(),
        replacement: format!("{indent};{line_ending}"),
    }
}

/// Offset of the first byte after `from` that is not whitespace or a comment.
fn next_token_start(bytes: &[u8], from: usize) -> Option<usize> {
    let mut index = from;
    while index < bytes.len() {
        match bytes[index] {
            byte if byte.is_ascii_whitespace() => index += 1,
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = bytes[index..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(bytes.len(), |offset| index + offset + 1);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let close = bytes[index + 2..]
                    .windows(2)
                    .position(|pair| pair == b"*/")?;
                index += 2 + close + 2;
            }
            _ => return Some(index),
        }
    }
    None
}

/// True when the text at the next kept token can continue an expression or a
/// type on the line before it. Other tokens (an identifier other than `in`
/// and `instanceof`, `#`, a quote, a digit, `@`, `}`, `;`) cannot, so the
/// line break still ends the field.
fn starts_a_continuation(rest: &str) -> bool {
    let Some(first) = rest.bytes().next() else {
        return false;
    };
    if matches!(
        first,
        b'[' | b'(' | b'`' | b'*' | b'+' | b'-' | b'.' | b'<' | b'?' | b'!' | b'=' | b',' | b'/'
    ) {
        return true;
    }
    let word_len = rest
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
        .count();
    // TypeScript does not continue an expression with `as` or `satisfies`
    // after a line break, so only the operator keywords count.
    matches!(&rest[..word_len], "in" | "instanceof")
}

/// True when the last token before `start` is `;` or the `{` that opens the
/// class body. A `}` can end a method body or an initializer such as
/// `function () {}`, so it does not count. The scan does not read comments,
/// strings or regular expressions, so a last line with `//` in it or a last
/// token `*/` gives `false`. The fixer then adds a `;`, which is always
/// valid in a class body.
fn previous_token_ends_element(bytes: &[u8], start: usize) -> bool {
    let text = &bytes[..start];
    let Some(last) = text.iter().rposition(|byte| !byte.is_ascii_whitespace()) else {
        return false;
    };
    let line_start = text[..last]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let line = &text[line_start..=last];
    if line.ends_with(b"*/") || line.windows(2).any(|pair| pair == b"//") {
        return false;
    }
    matches!(text[last], b';' | b'{')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(content: &str, ranges: &[Range<usize>]) -> String {
        let mut text = content.to_string();
        for edit in boundary_edits(content, ranges).into_iter().rev() {
            text.replace_range(edit.range, &edit.replacement);
        }
        text
    }

    fn line_range(content: &str, line: &str) -> Range<usize> {
        let start = content.find(line).unwrap();
        start..start + line.len()
    }

    #[test]
    fn puts_one_semicolon_after_a_run_of_removed_members() {
        let content = "class A {\n  v = 1\n  a() {}\n\n\n  b() {}\n  [k]() {}\n}\n";
        let ranges = [
            line_range(content, "  a() {}\n"),
            line_range(content, "  b() {}\n"),
        ];
        assert_eq!(
            apply(content, &ranges),
            "class A {\n  v = 1\n\n\n  ;[k]() {}\n}\n"
        );
    }

    #[test]
    fn keeps_a_comment_before_the_next_member() {
        let content = "class A {\n  v = 1\n  a() {}\n  // note\n  [k]() {}\n}\n";
        let ranges = [line_range(content, "  a() {}\n")];
        assert_eq!(
            apply(content, &ranges),
            "class A {\n  v = 1\n  ;\n  // note\n  [k]() {}\n}\n"
        );
    }

    #[test]
    fn treats_only_an_operator_keyword_as_a_continuation() {
        assert!(starts_a_continuation("in(): void {}"));
        assert!(starts_a_continuation("instanceof(): void {}"));
        assert!(!starts_a_continuation("inner(): void {}"));
        assert!(!starts_a_continuation("as(): void {}"));
        assert!(!starts_a_continuation("satisfies(): void {}"));
        assert!(!starts_a_continuation("#secret = 1"));
        assert!(!starts_a_continuation("}"));
    }

    /// The scan does not read comments, strings or regular expressions. A
    /// last token that could be the end of a comment gives `false`, so the
    /// fixer adds a `;`, which is always valid.
    #[test]
    fn treats_a_possible_comment_end_as_an_open_field() {
        let ends = |before: &str| previous_token_ends_element(before.as_bytes(), before.len());
        assert!(!ends("class A {\n  v = 1 // see {\n"));
        assert!(!ends("class A {\n  v = 1 /* x; */\n"));
        assert!(!ends("class A {\n  v = 1; // note\n"));
        assert!(!ends("class A {\n  v = \"a;//\"\n"));
        assert!(!ends("class A {\n  /** doc */ v = /a*/\n"));
        assert!(!ends("class A {\n  v = `{//`\n"));
        assert!(!ends("class A {\n  v = 1 /* x; /* y */\n"));
        assert!(!ends("class A {\n  v = 1\n"));
        assert!(!ends("class A {\n  m() {}\n"));
        assert!(ends("class A {\n  v = 1;\n"));
        assert!(ends("class A {\n"));
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let content = "class A {\r\n  v = 1\r\n  a() {}\r\n  /* c */ [k]() {}\r\n}\r\n";
        let ranges = [line_range(content, "  a() {}\r\n")];
        assert_eq!(
            apply(content, &ranges),
            "class A {\r\n  v = 1\r\n  ;\r\n  /* c */ [k]() {}\r\n}\r\n"
        );
    }
}
