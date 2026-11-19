//! Markdown escaping for finding descriptions in PR comments and reviews.

use crate::ci_output::markdown_table_code_span;

/// Escape a finding description for a Markdown table cell or paragraph.
///
/// A balanced code span in the text stays a code span. A name in single
/// quotes (`'lodash'`) becomes a code span. Outside the spans, only the
/// characters that can start Markdown or HTML get a backslash, so `(`, `)`,
/// `!`, `#`, `>` and `=` stay as they are. Pipes are escaped and line endings
/// collapse to spaces, so the text is safe in a table cell.
#[must_use]
pub fn markdown_table_prose(value: &str) -> String {
    let value = value.trim().replace("\r\n", " ").replace(['\r', '\n'], " ");
    let mut out = String::with_capacity(value.len() + 8);
    let mut rest = value.as_str();
    while !rest.is_empty() {
        if let Some((span, after)) = split_code_span(rest) {
            out.push_str(&span.replace('|', "\\|"));
            rest = after;
            continue;
        }
        if let Some((name, after)) = split_quoted_name(rest, out.chars().last()) {
            out.push_str(&markdown_table_code_span(name));
            rest = after;
            continue;
        }
        let mut chars = rest.chars();
        let ch = chars.next().unwrap_or_default();
        let next = chars.clone().next();
        if prose_needs_escape(ch, &out, next) {
            out.push('\\');
        }
        out.push(ch);
        rest = chars.as_str();
    }
    out
}

/// The text with each balanced code span replaced by its content, for a
/// plain-text surface such as a workflow annotation: ``Changing `<` to `<=` ``
/// becomes `Changing < to <=`.
#[must_use]
pub fn unwrap_code_spans(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while !rest.is_empty() {
        if let Some((span, after)) = split_code_span(rest) {
            let fence = span.bytes().take_while(|byte| *byte == b'`').count();
            let inner = &span[fence..span.len() - fence];
            let padded = inner.len() > 2 && inner.starts_with(' ') && inner.ends_with(' ');
            out.push_str(if padded {
                &inner[1..inner.len() - 1]
            } else {
                inner
            });
            rest = after;
            continue;
        }
        let mut chars = rest.chars();
        out.extend(chars.next());
        rest = chars.as_str();
    }
    out
}

/// A code span at the start of `text`: a run of backticks, the content, and a
/// closing run of the same length. `None` when the run has no closing match,
/// so a lone backtick is escaped as text.
fn split_code_span(text: &str) -> Option<(&str, &str)> {
    let fence_len = text.bytes().take_while(|byte| *byte == b'`').count();
    if fence_len == 0 {
        return None;
    }
    let fence = &text[..fence_len];
    let mut search = fence_len;
    while let Some(found) = text[search..].find(fence) {
        let start = search + found;
        let end = start + fence_len;
        let longer = text[end..].starts_with('`');
        if !longer && start > fence_len {
            return Some((&text[..end], &text[end..]));
        }
        search = end + text[end..].bytes().take_while(|byte| *byte == b'`').count();
    }
    None
}

/// A name in single quotes at the start of `text`, such as `'lodash'` or
/// `'./missing-module'`. The quote must open after a space or the start of
/// the text and close before a space, punctuation or the end, and the name
/// has no space, so an apostrophe (`other's`) never matches.
fn split_quoted_name(text: &str, previous: Option<char>) -> Option<(&str, &str)> {
    let inner = text.strip_prefix('\'')?;
    if previous.is_some_and(|ch| !(ch.is_whitespace() || ch == '(')) {
        return None;
    }
    let close = inner.find('\'')?;
    let name = &inner[..close];
    let after = &inner[close + 1..];
    let closes_cleanly = after
        .chars()
        .next()
        .is_none_or(|ch| ch.is_whitespace() || ",.;:)".contains(ch));
    let plain = !name.is_empty() && !name.contains(char::is_whitespace) && !name.contains('`');
    (closes_cleanly && plain).then_some((name, after))
}

/// Whether `text` ends with `word` as a whole word.
fn ends_with_word(text: &str, word: &str) -> bool {
    text.strip_suffix(word)
        .is_some_and(|before| before.chars().last().is_none_or(|ch| !ch.is_alphanumeric()))
}

/// `written` is the escaped text so far, so a rule can look back.
fn prose_needs_escape(ch: char, written: &str, next: Option<char>) -> bool {
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let previous = written.chars().last();
    match ch {
        '\\' | '`' | '*' | '[' | ']' | '|' | '~' => true,
        // Intraword underscores never start emphasis.
        '_' => !(word(previous) && word(next)),
        // Only a tag start, a closing tag, a comment or a declaration is HTML.
        '<' => next.is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?')),
        '&' => next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '#'),
        // `#12` links an issue, and `@name` mentions a user or starts the
        // domain of an e-mail autolink.
        '#' => next.is_some_and(|c| c.is_ascii_digit()),
        '@' => word(next),
        // `https://` and `www.` start an autolink. Escaping the `/` or the `.`
        // keeps the text and drops the link.
        '/' => previous == Some(':') && next == Some('/'),
        '.' => ends_with_word(written, "www"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{markdown_table_prose, unwrap_code_spans};

    #[test]
    fn unwrap_code_spans_keeps_the_code_and_drops_the_fences() {
        assert_eq!(
            unwrap_code_spans("Changing `<` to `<=` in `f`. Mutant `ab`."),
            "Changing < to <= in f. Mutant ab."
        );
        assert_eq!(unwrap_code_spans("to ``` `` ``` in g"), "to `` in g");
        assert_eq!(unwrap_code_spans("a lone ` tick"), "a lone ` tick");
    }

    #[test]
    fn prose_keeps_code_spans_and_quotes_names_as_code() {
        assert_eq!(
            markdown_table_prose("event `dead` in 'Child' (remove it)"),
            "event `dead` in `Child` (remove it)"
        );
        assert_eq!(
            markdown_table_prose("Changing `<` to `<=` in `clamp`"),
            "Changing `<` to `<=` in `clamp`"
        );
        assert_eq!(
            markdown_table_prose("Barrel's export 'a b' stays"),
            "Barrel's export 'a b' stays"
        );
        assert_eq!(markdown_table_prose("x ``a`b`` y"), "x ``a`b`` y");
    }

    #[test]
    fn prose_escapes_only_markup_that_can_start() {
        assert_eq!(markdown_table_prose("a <= b -> c"), "a <= b -> c");
        assert_eq!(markdown_table_prose("<img src=x>"), r"\<img src=x>");
        assert_eq!(markdown_table_prose("</b> <!-- x"), r"\</b> \<!-- x");
        assert_eq!(
            markdown_table_prose("[a](u) *b* ~c~"),
            r"\[a\](u) \*b\* \~c\~"
        );
        assert_eq!(
            markdown_table_prose("&#42; &amp; a & b"),
            r"\&\#42; \&amp; a & b"
        );
        assert_eq!(
            markdown_table_prose("snake_case _lead"),
            r"snake_case \_lead"
        );
        assert_eq!(markdown_table_prose("a lone ` tick"), r"a lone \` tick");
        assert_eq!(markdown_table_prose("see #12 and #x"), r"see \#12 and #x");
        assert_eq!(
            markdown_table_prose("ask @team, mail a@b"),
            r"ask \@team, mail a\@b"
        );
        assert_eq!(
            markdown_table_prose("see https://x.io and www.x.io, not awww.x"),
            r"see https:\//x.io and www\.x.io, not awww.x"
        );
    }

    #[test]
    fn prose_is_safe_in_a_table_cell() {
        assert_eq!(markdown_table_prose("a | b"), r"a \| b");
        assert_eq!(markdown_table_prose("`a|b`"), r"`a\|b`");
        assert_eq!(
            markdown_table_prose(" first\r\nsecond\rthird\n"),
            "first second third"
        );
        assert_eq!(markdown_table_prose("é🦀\tx"), "é🦀\tx");
    }
}
